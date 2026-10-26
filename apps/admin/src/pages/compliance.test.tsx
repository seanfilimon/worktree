import { screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { apiError } from "@/test/mockFetch";
import { renderApp } from "@/test/renderApp";
import type { ComplianceSummary } from "./tree/complianceApi";

const summary: ComplianceSummary = {
  trees: 2,
  modes: { off: 0, warn: 1, block: 1 },
  open_findings: 3,
  resolved_findings: 1,
  allowlisted: 2,
  license_rules: 4,
  invalid_rules: 1,
  spdx_list_version: "3.25",
  rules_version: 1,
  scan_rules: 16,
  tree_list: [
    {
      tree_id: "t1",
      tenant: "acme",
      tree: "web",
      mode: "block",
      open_findings: 3,
      resolved_findings: 1,
      blocked_findings: 2,
      allowlisted: 2,
      license_rules: 3,
      invalid_rules: 1,
      last_finding_at: "2026-10-02T10:00:00Z",
    },
    {
      tree_id: "t2",
      tenant: "acme",
      tree: "docs",
      mode: "warn",
      open_findings: 0,
      resolved_findings: 0,
      blocked_findings: 0,
      allowlisted: 0,
      license_rules: 1,
      invalid_rules: 0,
    },
  ],
  recent: [
    {
      id: "f-1",
      tree_id: "t1",
      tenant: "acme",
      tree_name: "web",
      path: "config/aws.env",
      line: 2,
      rule: "aws-access-key",
      excerpt: "AWS_ACCESS_KEY_ID=AKIA****Y3U6",
      fingerprint: "683e63ab39e025dc3e6ea642",
      branch: "main",
      snapshot_id: "s",
      status: "open",
      action: "blocked",
      count: 1,
      pushed_by: "bob",
      first_seen_at: "2026-10-02T10:00:00Z",
      last_seen_at: "2026-10-02T10:00:00Z",
    },
  ],
};

describe("compliance page", () => {
  it("summarises the server", async () => {
    renderApp({
      route: "/compliance",
      api: { "GET /api/v1/admin/compliance": summary },
    });
    expect(await screen.findByText("Open findings")).toBeInTheDocument();
    expect(screen.getByText("1 block")).toBeInTheDocument();
    expect(
      screen.getByText(/of 4 rules · SPDX License List 3.25/),
    ).toBeInTheDocument();
    const trees = screen.getByRole("table", { name: "Trees" });
    const rows = within(trees).getAllByRole("row").slice(1);
    expect(rows).toHaveLength(2);
    expect(rows[0]).toHaveTextContent("acme/web");
    expect(rows[0]).toHaveTextContent("Block");
    expect(rows[0]).toHaveTextContent("(1 invalid)");
    expect(
      within(rows[0]!).getByRole("link", { name: "acme/web" }),
    ).toHaveAttribute("href", "/trees/acme/web/compliance");
    const recent = screen.getByRole("table", { name: "Latest open findings" });
    expect(recent).toHaveTextContent("config/aws.env:2");
    expect(recent).toHaveTextContent("blocked");
  });

  it("explains an empty server", async () => {
    renderApp({
      route: "/compliance",
      api: {
        "GET /api/v1/admin/compliance": {
          ...summary,
          tree_list: null,
          recent: null,
          open_findings: 0,
        },
      },
    });
    expect(
      await screen.findByText("There are no trees on this server yet."),
    ).toBeInTheDocument();
    expect(screen.getByText("No open findings")).toBeInTheDocument();
  });

  it("shows errors and missing support", async () => {
    renderApp({
      route: "/compliance",
      api: {
        "GET /api/v1/admin/compliance": () =>
          apiError(501, "not_implemented", "nope"),
      },
    });
    expect(
      await screen.findByText(
        "Secret scanning and license compliance is not available on this server",
      ),
    ).toBeInTheDocument();
  });
});
