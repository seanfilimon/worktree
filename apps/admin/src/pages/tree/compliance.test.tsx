import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { apiError, json, noContent, type MockRoute } from "@/test/mockFetch";
import { renderApp, treeRoutes } from "@/test/renderApp";
import * as fx from "@/test/fixtures";
import {
  allowFormErrors,
  maxFileText,
  normAudit,
  normSettings,
  normSummary,
  type ComplianceSettings,
  type LicenseAudit,
  type SecretFinding,
} from "./complianceApi";

const C = "/api/v1/trees/acme/web/compliance";
const FP = "683e63ab39e025dc3e6ea642";

const settings: ComplianceSettings = {
  tree_id: "tree-web",
  mode: "warn",
  exclude: ["fixtures/**"],
  max_file_bytes: 0,
};

const finding: SecretFinding = {
  id: "f-1",
  tree_id: "tree-web",
  path: "config/aws.env",
  line: 2,
  rule: "aws-access-key",
  excerpt: "AWS_ACCESS_KEY_ID=AKIA****Y3U6",
  fingerprint: FP,
  branch: "main",
  snapshot_id: "0b5c2f1e-0000-4000-8000-000000000001",
  status: "open",
  action: "blocked",
  count: 2,
  pushed_by: "bob",
  first_seen_at: "2026-10-01T10:00:00Z",
  last_seen_at: "2026-10-02T10:00:00Z",
};

const resolved: SecretFinding = {
  ...finding,
  id: "f-2",
  path: "src/app.rs",
  line: 4,
  rule: "github-token",
  status: "resolved",
  action: "warned",
  resolution: "revoked",
};

const audit: LicenseAudit = {
  tree_id: "tree-web",
  ref: "main",
  snapshot_id: "0b5c2f1e-0000-4000-8000-000000000001",
  files: 5,
  licenses: [
    { spdx: "MIT", files: 2, proprietary: false, globs: ["src/**"] },
    {
      spdx: "LicenseRef-Proprietary",
      files: 1,
      proprietary: true,
      globs: null as unknown as string[],
    },
  ],
  unlicensed: ["README.md"],
  unlicensed_count: 1,
  exposed: [
    {
      path: "vendor/blob.c",
      spdx: "LicenseRef-Proprietary",
      grantee: "",
      grant: "read-only",
      glob: "vendor/**",
    },
  ],
  exposed_count: 1,
  rules: [
    {
      path_glob: "tools/**",
      spdx: "GPL-2.0",
      grant: "",
      grantee: "",
      expired: false,
      files: 1,
      valid: false,
      problems: ['"GPL-2.0" is deprecated on the SPDX License List'],
    },
    {
      path_glob: "src/**",
      spdx: "MIT",
      grant: "",
      grantee: "",
      expired: false,
      files: 2,
      valid: true,
      problems: null as unknown as string[],
    },
  ],
  invalid_rules: 1,
  spdx_list_version: "3.25",
  truncated: false,
};

function routes(extra: Record<string, MockRoute> = {}) {
  return treeRoutes({
    [`GET ${C}/settings`]: settings,
    [`GET ${C}/findings`]: (req: { url: URL }) =>
      req.url.searchParams.get("status") === "resolved"
        ? { findings: [resolved], open: 1, resolved: 1 }
        : { findings: [finding], open: 1, resolved: 1 },
    [`GET ${C}/allowlist`]: { tree_id: "tree-web", entries: null },
    [`GET ${C}/licenses`]: audit,
    ...extra,
  });
}

describe("compliance helpers", () => {
  it("validates the allowlist form and formats sizes", () => {
    expect(allowFormErrors("", "")).toEqual({
      fingerprint: "Required.",
      reason: "Required.",
    });
    expect(allowFormErrors("xyz", "ok").fingerprint).toMatch(/24 hexadecimal/);
    expect(allowFormErrors(FP.toUpperCase(), "fixture")).toEqual({
      fingerprint: "",
      reason: "",
    });
    expect(allowFormErrors(FP, "x".repeat(501)).reason).toMatch(/500/);
    expect(maxFileText(0)).toBe("1 MiB (default)");
    expect(maxFileText(2 << 20)).toBe("2 MiB");
    expect(maxFileText(512 * 1024)).toBe("512 KiB");
    expect(maxFileText(1000)).toBe("1000 bytes");
  });

  it("normalises null lists", () => {
    expect(
      normSettings({
        ...settings,
        mode: "" as never,
        exclude: null as unknown as string[],
      }),
    ).toMatchObject({ mode: "warn", exclude: [] });
    const a = normAudit({ ...audit, unlicensed: null as unknown as string[] });
    expect(a.unlicensed).toEqual([]);
    expect(a.licenses[1]?.globs).toEqual([]);
    expect(a.rules[1]?.problems).toEqual([]);
    const s = normSummary({
      modes: null,
      tree_list: null,
      recent: null,
    } as never);
    expect(s.modes).toEqual({ off: 0, warn: 0, block: 0 });
    expect(s.tree_list).toEqual([]);
  });
});

describe("compliance tab", () => {
  it("shows settings, findings and the license audit", async () => {
    renderApp({ route: "/trees/acme/web/compliance", api: routes() });
    const form = await screen.findByRole("form", {
      name: "Secret scanning settings",
    });
    expect(within(form).getByRole("radio", { name: /Warn/ })).toBeChecked();
    expect(within(form).getByText("fixtures/**")).toBeInTheDocument();

    const table = await screen.findByRole("table", { name: "Findings" });
    const rows = within(table).getAllByRole("row").slice(1);
    expect(rows).toHaveLength(1);
    expect(rows[0]).toHaveTextContent("config/aws.env:2");
    expect(rows[0]).toHaveTextContent("aws-access-key");
    expect(rows[0]).toHaveTextContent("Blocked");
    expect(rows[0]).toHaveTextContent(FP);
    expect(rows[0]).toHaveTextContent("2 pushes");

    expect(
      await screen.findByText("The allowlist is empty."),
    ).toBeInTheDocument();
    const stats = await screen.findByLabelText("License audit summary");
    expect(stats).toHaveTextContent("Without a license rule1");
    expect(
      screen.getByRole("table", { name: "Exposed paths" }),
    ).toHaveTextContent("vendor/blob.cevery other tenant (read-only)");
    expect(
      screen.getByRole("table", { name: "Rule checks" }),
    ).toHaveTextContent("deprecated");
    expect(
      screen.getByLabelText("Paths without a license rule"),
    ).toHaveTextContent("README.md");
  });

  it("switches the mode and edits exclusions", async () => {
    const { user, fetchMock } = renderApp({
      route: "/trees/acme/web/compliance",
      api: routes({
        [`PUT ${C}/settings`]: (req: { body: object }) =>
          json({ ...settings, ...req.body, updated_by: "ada" }),
      }),
    });
    const form = await screen.findByRole("form", {
      name: "Secret scanning settings",
    });
    const save = within(form).getByRole("button", { name: "Save settings" });
    expect(save).toBeDisabled();
    await user.click(within(form).getByRole("radio", { name: /Block/ }));
    await user.type(
      within(form).getByLabelText("Largest file scanned (MiB)"),
      "2",
    );
    await user.click(save);
    await waitFor(() =>
      expect(fetchMock.find(`PUT ${C}/settings`)).toHaveLength(1),
    );
    expect(fetchMock.find(`PUT ${C}/settings`)[0]?.body).toEqual({
      mode: "block",
      exclude: ["fixtures/**"],
      max_file_bytes: 2 << 20,
    });
  });

  it("rejects an out-of-range size", async () => {
    const { user } = renderApp({
      route: "/trees/acme/web/compliance",
      api: routes(),
    });
    const form = await screen.findByRole("form", {
      name: "Secret scanning settings",
    });
    await user.type(
      within(form).getByLabelText("Largest file scanned (MiB)"),
      "99",
    );
    expect(within(form).getByText("Between 0 and 16 MiB.")).toBeInTheDocument();
    expect(
      within(form).getByRole("button", { name: "Save settings" }),
    ).toBeDisabled();
  });

  it("resolves, allowlists and reopens findings", async () => {
    const { user, fetchMock } = renderApp({
      route: "/trees/acme/web/compliance",
      api: routes({
        [`POST ${C}/findings/f-1/resolve`]: (req: { body: object }) =>
          json({ ...finding, status: "resolved", ...req.body }),
        [`POST ${C}/findings/f-2/resolve`]: json({
          ...resolved,
          status: "open",
        }),
        [`POST ${C}/allowlist`]: (req: {
          body: { fingerprint: string; reason: string };
        }) =>
          json(
            {
              ...req.body,
              created_by: "ada",
              created_at: "2026-10-03T00:00:00Z",
            },
            201,
          ),
      }),
    });
    await user.click(
      await screen.findByRole("button", { name: "Resolve config/aws.env:2" }),
    );
    const dialog = await screen.findByRole("dialog", {
      name: "Resolve finding",
    });
    await user.click(
      within(dialog).getByRole("radio", { name: "False positive" }),
    );
    await user.type(
      within(dialog).getByLabelText("Note (optional)"),
      "example value",
    );
    await user.click(within(dialog).getByRole("button", { name: "Resolve" }));
    await waitFor(() =>
      expect(fetchMock.find(`POST ${C}/findings/f-1/resolve`)).toHaveLength(1),
    );
    expect(fetchMock.find(`POST ${C}/findings/f-1/resolve`)[0]?.body).toEqual({
      resolution: "false_positive",
      note: "example value",
    });

    await user.click(
      screen.getByRole("button", { name: "Allowlist config/aws.env:2" }),
    );
    const allow = await screen.findByRole("dialog", {
      name: "Allowlist a secret",
    });
    expect(within(allow).getByLabelText("Fingerprint")).toHaveValue(FP);
    await user.click(within(allow).getByRole("button", { name: "Allowlist" }));
    expect(within(allow).getByText("Required.")).toBeInTheDocument();
    await user.type(within(allow).getByLabelText("Reason"), "test fixture");
    await user.click(within(allow).getByRole("button", { name: "Allowlist" }));
    await waitFor(() =>
      expect(fetchMock.find(`POST ${C}/allowlist`)).toHaveLength(1),
    );
    expect(fetchMock.find(`POST ${C}/allowlist`)[0]?.body).toEqual({
      fingerprint: FP,
      reason: "test fixture",
    });

    await user.click(screen.getByRole("radio", { name: "Resolved" }));
    await user.click(
      await screen.findByRole("button", { name: "Reopen src/app.rs:4" }),
    );
    await waitFor(() =>
      expect(fetchMock.find(`POST ${C}/findings/f-2/resolve`)).toHaveLength(1),
    );
    expect(fetchMock.find(`POST ${C}/findings/f-2/resolve`)[0]?.body).toEqual({
      reopen: true,
    });
  });

  it("removes an allowlist entry", async () => {
    const { user, fetchMock } = renderApp({
      route: "/trees/acme/web/compliance",
      api: routes({
        [`GET ${C}/allowlist`]: {
          tree_id: "tree-web",
          entries: [
            {
              fingerprint: FP,
              reason: "fixture",
              created_by: "ada",
              created_at: "2026-10-01T00:00:00Z",
            },
          ],
        },
        [`DELETE ${C}/allowlist`]: noContent(),
      }),
    });
    const table = await screen.findByRole("table", { name: "Allowlist" });
    expect(table).toHaveTextContent("fixture");
    await user.click(
      within(table).getByRole("button", { name: `Remove ${FP}` }),
    );
    const confirm = await screen.findByRole("alertdialog");
    await user.click(within(confirm).getByRole("button", { name: "Remove" }));
    await waitFor(() =>
      expect(fetchMock.find(`DELETE ${C}/allowlist`)).toHaveLength(1),
    );
    expect(
      fetchMock
        .find(`DELETE ${C}/allowlist`)[0]
        ?.url.searchParams.get("fingerprint"),
    ).toBe(FP);
  });

  it("is read-only for members who cannot manage the tree", async () => {
    renderApp({
      route: "/trees/acme/web/compliance",
      api: { ...routes(), "GET /api/v1/auth/me": fx.memberMe },
    });
    const form = await screen.findByRole("form", {
      name: "Secret scanning settings",
    });
    expect(
      within(form).queryByRole("button", { name: "Save settings" }),
    ).not.toBeInTheDocument();
    expect(
      await screen.findByText("Tree administrators can run the license audit."),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /^Resolve / }),
    ).not.toBeInTheDocument();
  });

  it("shows a notice when the server has no compliance support", async () => {
    renderApp({
      route: "/trees/acme/web/compliance",
      api: treeRoutes({
        [`GET ${C}/settings`]: () =>
          apiError(
            501,
            "not_implemented",
            "Secret scanning and license compliance is not available on this server",
          ),
      }),
    });
    expect(
      await screen.findByText(
        "Secret scanning and license compliance is not available on this server",
      ),
    ).toBeInTheDocument();
  });
});
