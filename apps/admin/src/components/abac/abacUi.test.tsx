import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { Decision } from "@/api/iam";
import * as fx from "@/test/fixtures";
import * as ix from "@/test/fixtures.iam";
import * as px from "@/test/fixtures.people";
import { apiError, json } from "@/test/mockFetch";
import { renderApp, treeRoutes } from "@/test/renderApp";

// These are long interactive flows through whole pages: allow for slow CI hosts.
vi.setConfig({ testTimeout: 30_000 });

const TA = "/api/v1/tenants/acme/attributes";

const tenantRoutes = {
  "GET /api/v1/tenants/acme": fx.tenant,
  "GET /api/v1/tenants/acme/trees": px.acmeTrees,
  "GET /api/v1/tenants/acme/members": px.acmeMembers,
};

const echo = (req: { body: unknown }) => json(req.body);

describe("attribute editor", () => {
  it("edits tenant attributes, validates and saves the full map", async () => {
    const { user, fetchMock } = renderApp({
      route: "/tenants/acme?tab=settings",
      api: { ...tenantRoutes, [`GET ${TA}`]: { attributes: { department: "eng" } }, [`PUT ${TA}`]: echo },
    });
    const form = await screen.findByRole("form", { name: "Tenant attributes" });
    expect(within(form).getByLabelText("Attribute key 1")).toHaveValue("department");
    expect(within(form).getByLabelText("Attribute value 1")).toHaveValue("eng");
    expect(within(form).getByRole("button", { name: "Save attributes" })).toBeDisabled();

    await user.click(within(form).getByRole("button", { name: "Add attribute" }));
    await user.type(within(form).getByLabelText("Attribute key 2"), "Region");
    expect(within(form).getByText(/Invalid key/)).toBeInTheDocument();
    expect(within(form).getByLabelText("Attribute key 2")).toHaveAttribute("aria-invalid", "true");
    await user.click(within(form).getByRole("button", { name: "Save attributes" }));
    expect(within(form).getByText("Enter a value.")).toBeInTheDocument();
    expect(fetchMock.find(`PUT ${TA}`)).toHaveLength(0);

    await user.clear(within(form).getByLabelText("Attribute key 2"));
    await user.type(within(form).getByLabelText("Attribute key 2"), "region");
    await user.type(within(form).getByLabelText("Attribute value 2"), "eu-west");
    await user.click(within(form).getByRole("button", { name: "Save attributes" }));
    await waitFor(() => expect(fetchMock.find(`PUT ${TA}`)).toHaveLength(1));
    expect(fetchMock.find(`PUT ${TA}`)[0]?.body).toEqual({ attributes: { department: "eng", region: "eu-west" } });
    expect(await screen.findByText("Attributes saved")).toBeInTheDocument();
    // Saved: the form is clean again.
    await waitFor(() =>
      expect(within(screen.getByRole("form", { name: "Tenant attributes" })).getByRole("button", { name: "Save attributes" })).toBeDisabled(),
    );
  });

  it("clears every attribute with an empty map and reports server errors inline", async () => {
    let fail = true;
    const { user, fetchMock } = renderApp({
      route: "/tenants/acme?tab=settings",
      api: {
        ...tenantRoutes,
        [`GET ${TA}`]: { attributes: { department: "eng" } },
        [`PUT ${TA}`]: (req) => (fail ? apiError(400, "bad_request", "attribute store is read-only") : json(req.body)),
      },
    });
    const form = await screen.findByRole("form", { name: "Tenant attributes" });
    await user.click(within(form).getByRole("button", { name: "Remove attribute department" }));
    expect(within(form).getByText("No attributes")).toBeInTheDocument();
    await user.click(within(form).getByRole("button", { name: "Save attributes" }));
    expect(await within(form).findByText("attribute store is read-only")).toBeInTheDocument();

    fail = false;
    await user.click(within(form).getByRole("button", { name: "Save attributes" }));
    await waitFor(() => expect(fetchMock.find(`PUT ${TA}`)).toHaveLength(2));
    expect(fetchMock.find(`PUT ${TA}`)[1]?.body).toEqual({ attributes: {} });
  });

  it("resets unsaved edits", async () => {
    const { user } = renderApp({
      route: "/tenants/acme?tab=settings",
      api: { ...tenantRoutes, [`GET ${TA}`]: { attributes: { department: "eng" } } },
    });
    const form = await screen.findByRole("form", { name: "Tenant attributes" });
    await user.clear(within(form).getByLabelText("Attribute value 1"));
    await user.type(within(form).getByLabelText("Attribute value 1"), "ops");
    await user.click(within(form).getByRole("button", { name: "Reset attributes" }));
    expect(within(form).getByLabelText("Attribute value 1")).toHaveValue("eng");
  });

  it("is read-only for tenant members without admin rights", async () => {
    renderApp({
      route: "/tenants/acme?tab=settings",
      api: { ...tenantRoutes, "GET /api/v1/auth/me": fx.memberMe, [`GET ${TA}`]: { attributes: { department: "eng" } } },
    });
    const card = await screen.findByRole("region", { name: "Tenant attributes" });
    expect(await within(card).findByText("department")).toBeInTheDocument();
    expect(within(card).getByText("eng")).toBeInTheDocument();
    expect(within(card).queryByRole("textbox")).not.toBeInTheDocument();
    expect(within(card).queryByRole("button", { name: "Add attribute" })).not.toBeInTheDocument();
  });

  it("edits tree attributes from tree settings", async () => {
    const A = "/api/v1/trees/acme/web/attributes";
    const { user, fetchMock } = renderApp({
      route: "/trees/acme/web/settings",
      api: treeRoutes({ [`GET ${A}`]: { attributes: null }, [`PUT ${A}`]: echo }),
    });
    const form = await screen.findByRole("form", { name: "Tree attributes" });
    expect(within(form).getByText("No attributes")).toBeInTheDocument();
    await user.click(within(form).getByRole("button", { name: "Add attribute" }));
    await user.type(within(form).getByLabelText("Attribute key 1"), "classification");
    await user.type(within(form).getByLabelText("Attribute value 1"), "confidential");
    await user.click(within(form).getByRole("button", { name: "Save attributes" }));
    await waitFor(() => expect(fetchMock.find(`PUT ${A}`)[0]?.body).toEqual({ attributes: { classification: "confidential" } }));
  });

  it("edits account attributes in the account sheet", async () => {
    const A = "/api/v1/admin/accounts/bob/attributes";
    const { user, fetchMock } = renderApp({
      route: "/accounts",
      api: {
        "GET /api/v1/admin/accounts": [fx.account, fx.bob, px.carol],
        "GET /api/v1/tenants/acme/members": px.acmeMembers,
        [`GET ${A}`]: { attributes: { department: "eng" } },
        [`PUT ${A}`]: echo,
      },
    });
    await user.click(await screen.findByRole("button", { name: "bob" }));
    const sheet = await screen.findByRole("dialog", { name: "bob" });
    const form = await within(sheet).findByRole("form", { name: "Account attributes" });
    await user.clear(within(form).getByLabelText("Attribute value 1"));
    await user.type(within(form).getByLabelText("Attribute value 1"), "ops");
    await user.click(within(form).getByRole("button", { name: "Save attributes" }));
    await waitFor(() => expect(fetchMock.find(`PUT ${A}`)[0]?.body).toEqual({ attributes: { department: "ops" } }));
  });

  it("shows the caller's own attributes read-only in settings", async () => {
    renderApp({ route: "/settings", api: { "GET /api/v1/me/attributes": { attributes: { team: "web" } } } });
    const card = await screen.findByRole("region", { name: "My attributes" });
    expect(await within(card).findByText("team")).toBeInTheDocument();
    expect(within(card).getByText("web")).toBeInTheDocument();
    expect(within(card).queryByRole("button", { name: "Save attributes" })).not.toBeInTheDocument();
  });
});

const T = "/api/v1/tenants/acme";

function openPolicies(api: Record<string, unknown> = {}) {
  return renderApp({ route: "/tenants/acme?tab=policies", api: { ...ix.tenantIamRoutes(), ...(api as object) } });
}

describe("policy attribute condition builder", () => {
  it("adds conditions and serialises values per operator", async () => {
    const { user, fetchMock } = openPolicies({
      [`POST ${T}/policies`]: (req: { body: unknown }) => json({ ...ix.policy, id: "pol-2", ...(req.body as object), version: 1 }, 201),
    });
    await user.click(await screen.findByRole("button", { name: "New policy" }));
    const dialog = await screen.findByRole("dialog", { name: "New policy" });
    await user.type(within(dialog).getByLabelText("Name"), "eng-only");
    const st = within(dialog).getByRole("group", { name: "Statement 1" });
    await user.click(within(st).getByRole("button", { name: "Add wt:tree/acme/*" }));
    await user.click(within(st).getByRole("button", { name: /Conditions/ }));
    const section = within(st).getByRole("group", { name: "Attribute conditions" });

    await user.click(within(section).getByRole("button", { name: "Add attribute condition" }));
    expect(within(section).getByRole("combobox", { name: "Condition 1 scope" })).toHaveTextContent("principal");
    await user.type(within(section).getByLabelText("Condition 1 attribute"), "department");
    // equals takes a single value.
    await user.type(within(section).getByLabelText("Condition 1 value"), "eng");
    await user.click(within(section).getByRole("combobox", { name: "Condition 1 operator" }));
    await user.click(await screen.findByRole("option", { name: "in" }));
    // in keeps the value as a chip and takes more.
    expect(within(section).queryByLabelText("Condition 1 value")).not.toBeInTheDocument();
    await user.type(within(section).getByLabelText("Condition 1 values"), "ops{Enter}");
    expect(within(section).getByRole("listitem", { name: "Condition 1" })).toHaveTextContent("principal.department in [eng, ops]");

    await user.click(within(section).getByRole("button", { name: "Add attribute condition" }));
    await user.click(within(section).getByRole("combobox", { name: "Condition 2 scope" }));
    await user.click(await screen.findByRole("option", { name: "tree" }));
    await user.type(within(section).getByLabelText("Condition 2 attribute"), "classification");
    await user.click(within(section).getByRole("combobox", { name: "Condition 2 operator" }));
    await user.click(await screen.findByRole("option", { name: "not exists" }));
    expect(within(section).queryByLabelText("Condition 2 value")).not.toBeInTheDocument();
    expect(within(section).queryByLabelText("Condition 2 values")).not.toBeInTheDocument();

    // Inline validation of what was typed.
    await user.click(within(section).getByRole("button", { name: "Add attribute condition" }));
    await user.click(within(section).getByRole("combobox", { name: "Condition 3 scope" }));
    await user.click(await screen.findByRole("option", { name: "tenant" }));
    await user.type(within(section).getByLabelText("Condition 3 attribute"), "Region");
    expect(within(section).getByRole("listitem", { name: "Condition 3" })).toHaveTextContent(/Invalid key/);
    await user.click(within(section).getByRole("button", { name: "Remove condition 3" }));

    await waitFor(() =>
      expect(fetchMock.find(`POST ${T}/policies/validate`).at(-1)?.body).toMatchObject({
        statements: [
          {
            conditions: {
              attributes: [
                { key: "principal.department", op: "in", values: ["eng", "ops"] },
                { key: "tree.classification", op: "not_exists" },
              ],
            },
          },
        ],
      }),
    );
    await user.click(within(dialog).getByRole("button", { name: "Create policy" }));
    await waitFor(() => expect(fetchMock.find(`POST ${T}/policies`)).toHaveLength(1));
    expect(fetchMock.find(`POST ${T}/policies`)[0]?.body).toEqual({
      name: "eng-only",
      description: "",
      enabled: true,
      statements: [
        {
          effect: "allow",
          actions: [],
          resources: ["wt:tree/acme/*"],
          principals: [],
          conditions: {
            attributes: [
              { key: "principal.department", op: "in", values: ["eng", "ops"] },
              { key: "tree.classification", op: "not_exists" },
            ],
          },
        },
      ],
    });
  });

  it("round-trips conditions.attributes between JSON and the visual builder", async () => {
    const statements = [
      {
        effect: "deny",
        actions: ["branch:push"],
        resources: ["wt:branch/acme/*/main"],
        principals: ["*"],
        conditions: {
          attributes: [
            { key: "principal.clearance", op: "not_in", values: ["high", "top"] },
            { key: "tree.classification", op: "equals", values: ["secret"] },
          ],
        },
      },
    ];
    const { user, fetchMock } = openPolicies({
      [`POST ${T}/policies/validate`]: (req: { body: unknown }) => {
        const body = req.body as { statements: { conditions: { attributes?: { op: string }[] } }[] };
        const attrs = body.statements[0]?.conditions.attributes ?? [];
        return attrs[1]?.op === "equals"
          ? { valid: false, errors: [{ statement: 0, field: "conditions.attributes[1].values", message: "secret is reserved" }] }
          : { valid: true, errors: [] };
      },
      [`PUT ${T}/policies/pol-1`]: (req: { body: unknown }) => ({ ...ix.policy, ...(req.body as object), version: 4 }),
    });
    await user.click(await screen.findByRole("button", { name: "Edit no-friday-deploys" }));
    const dialog = await screen.findByRole("dialog", { name: "Edit policy no-friday-deploys" });
    await user.click(within(dialog).getByRole("tab", { name: "JSON" }));
    const editor = within(dialog).getByLabelText("Statements (JSON array)");
    expect((editor as HTMLTextAreaElement).value).not.toContain('"attributes"');
    await user.clear(editor);
    await user.paste(JSON.stringify(statements));
    const errors = await within(dialog).findByRole("list", { name: "Validation errors" });
    expect(errors).toHaveTextContent("Statement 1 · conditions.attributes[1].values: secret is reserved");

    await user.click(within(dialog).getByRole("tab", { name: "Visual" }));
    const section = within(within(dialog).getByRole("group", { name: "Statement 1" })).getByRole("group", { name: "Attribute conditions" });
    expect(within(section).getByLabelText("Condition 1 attribute")).toHaveValue("clearance");
    expect(within(section).getByRole("combobox", { name: "Condition 1 operator" })).toHaveTextContent("not in");
    expect(within(section).getByRole("listitem", { name: "Condition 1" })).toHaveTextContent("principal.clearance not_in [high, top]");
    expect(within(section).getByRole("combobox", { name: "Condition 2 scope" })).toHaveTextContent("tree");
    expect(within(section).getByLabelText("Condition 2 value")).toHaveValue("secret");
    // The server error sits on its condition.
    expect(within(section).getByRole("listitem", { name: "Condition 2" })).toHaveTextContent("secret is reserved");

    await user.clear(within(section).getByLabelText("Condition 2 value"));
    await user.type(within(section).getByLabelText("Condition 2 value"), "internal");
    await user.click(within(dialog).getByRole("tab", { name: "JSON" }));
    const text = within(dialog).getByLabelText<HTMLTextAreaElement>("Statements (JSON array)").value;
    expect(JSON.parse(text)).toEqual([
      {
        ...statements[0],
        conditions: {
          attributes: [
            { key: "principal.clearance", op: "not_in", values: ["high", "top"] },
            { key: "tree.classification", op: "equals", values: ["internal"] },
          ],
        },
      },
    ]);
    await user.click(within(dialog).getByRole("button", { name: "Save policy" }));
    await waitFor(() => expect(fetchMock.find(`PUT ${T}/policies/pol-1`)).toHaveLength(1));
    expect((fetchMock.find(`PUT ${T}/policies/pol-1`)[0]?.body as { statements: unknown }).statements).toEqual(JSON.parse(text));
  });
});

const simRoutes = {
  "GET /api/v1/iam/actions": ix.actions,
  [`GET ${T}/trees`]: px.acmeTrees,
  "GET /api/v1/iam/effective": ix.effective,
};

const abacDecision: Decision = {
  allowed: false,
  effect: "implicit_deny",
  reasons: [{ source: "attribute", detail: "eng-only statement #1: tree.classification equals secret not met", effect: "info" }],
  attributes: { principal: { department: "eng" }, tree: {}, tenant: { region: "eu" } },
  conditions: [
    {
      policy: "eng-only",
      policy_id: "pol-2",
      statement: "#1",
      effect: "allow",
      key: "principal.department",
      op: "in",
      values: ["eng", "ops"],
      value: "eng",
      matched: true,
    },
    {
      policy: "eng-only",
      policy_id: "pol-2",
      statement: "#1",
      effect: "allow",
      key: "tree.classification",
      op: "equals",
      values: ["secret"],
      value: null,
      matched: false,
    },
  ],
};

describe("simulator attribute explanation", () => {
  it("sends attribute overrides and explains every attribute condition", async () => {
    const { user, fetchMock } = renderApp({
      route: "/iam/simulator",
      api: { ...simRoutes, "POST /api/v1/iam/simulate": abacDecision },
    });
    const form = await screen.findByRole("form", { name: "Simulation request" });
    await waitFor(() => expect(within(form).getByRole("combobox", { name: "Tenant" })).toHaveTextContent("acme"));
    await user.click(within(form).getByRole("button", { name: "Add override" }));
    await user.type(within(form).getByLabelText("Override 1 key"), "department");
    await user.type(within(form).getByLabelText("Override 1 value"), "eng");
    await user.click(within(form).getByRole("button", { name: "Simulate" }));
    expect(within(form).getByText(/principal\., tree\. or tenant\./)).toBeInTheDocument();
    expect(fetchMock.find("POST /api/v1/iam/simulate")).toHaveLength(0);

    await user.clear(within(form).getByLabelText("Override 1 key"));
    await user.type(within(form).getByLabelText("Override 1 key"), "principal.department");
    await user.click(within(form).getByRole("button", { name: "Add override" }));
    await user.type(within(form).getByLabelText("Override 2 key"), "tree.classification");
    await user.click(within(form).getByRole("checkbox", { name: "Override 2 unset" }));
    expect(within(form).getByLabelText("Override 2 value")).toBeDisabled();
    await user.click(within(form).getByRole("button", { name: "Simulate" }));

    await waitFor(() => expect(fetchMock.find("POST /api/v1/iam/simulate")).toHaveLength(1));
    expect((fetchMock.find("POST /api/v1/iam/simulate")[0]?.body as { context: unknown }).context).toEqual({
      attributes: { "principal.department": "eng", "tree.classification": null },
    });

    const card = await screen.findByLabelText("Attribute evaluation");
    expect(within(card).getByRole("list", { name: "principal attributes" })).toHaveTextContent("department = eng");
    expect(within(card).getByRole("list", { name: "tenant attributes" })).toHaveTextContent("region = eu");
    const rows = within(within(card).getByRole("table", { name: "Attribute conditions" })).getAllByRole("row").slice(1);
    expect(rows).toHaveLength(2);
    expect(rows[0]).toHaveTextContent("eng-only");
    expect(rows[0]).toHaveTextContent("principal.department in [eng, ops]");
    expect(rows[0]).toHaveTextContent("Matched");
    expect(rows[1]).toHaveTextContent("tree.classification equals secret");
    expect(rows[1]).toHaveTextContent("unset");
    expect(rows[1]).toHaveTextContent("Not met");
    const decision = screen.getByLabelText("Decision");
    expect(within(decision).getByRole("list", { name: "Reasons" })).toHaveTextContent("Attribute");
  });

  it("hides the explanation when the decision has no attributes or conditions", async () => {
    const { user, fetchMock } = renderApp({
      route: "/iam/simulator",
      api: { ...simRoutes, "POST /api/v1/iam/simulate": { ...ix.decision, attributes: { principal: null, tree: null, tenant: null }, conditions: null } },
    });
    const form = await screen.findByRole("form", { name: "Simulation request" });
    await waitFor(() => expect(within(form).getByRole("combobox", { name: "Tenant" })).toHaveTextContent("acme"));
    await user.click(within(form).getByRole("button", { name: "Simulate" }));
    expect(await screen.findByLabelText("Decision")).toBeInTheDocument();
    expect(screen.queryByLabelText("Attribute evaluation")).not.toBeInTheDocument();
    expect((fetchMock.find("POST /api/v1/iam/simulate")[0]?.body as { context: unknown }).context).toEqual({});
  });
});
