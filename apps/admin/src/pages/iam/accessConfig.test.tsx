import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import * as fx from "@/test/fixtures";
import * as ox from "@/test/fixtures.ops";
import { apiError } from "@/test/mockFetch";
import { renderApp } from "@/test/renderApp";

const route = "/tenants/acme?tab=access-config";
const base = { "GET /api/v1/tenants/acme": fx.tenant };

const DOC = JSON.stringify({ roles: [{ name: "ci", description: "", permissions: ["ci:report"] }], policies: [] });

/** Pastes text into the editor (user.type would interpret braces). */
function paste(text: string) {
  fireEvent.change(screen.getByLabelText("Access document (JSON)"), { target: { value: text } });
}

describe("tenant IAM: access config", () => {
  it("shows the last applied document and loads it into the editor", async () => {
    const { user } = renderApp({ route, api: { ...base, "GET /api/v1/tenants/acme/access-config": ox.accessDoc } });
    const pre = await screen.findByLabelText("Applied document");
    expect(pre).toHaveTextContent('"name": "release-manager"');
    expect(screen.getByText("b3a1c0ffee").closest("dl")).toHaveTextContent("by alice");

    await user.click(screen.getByRole("button", { name: "Load into editor" }));
    expect(screen.getByLabelText("Access document (JSON)")).toHaveValue(JSON.stringify(ox.accessContent, null, 2));
  });

  it("shows an empty state when nothing was applied (404)", async () => {
    renderApp({ route, api: base });
    expect(await screen.findByText("No document applied yet")).toBeInTheDocument();
  });

  it("validates JSON locally before planning", async () => {
    const { user, fetchMock } = renderApp({ route, api: base });
    await screen.findByText("No document applied yet");
    paste("{ not json");
    await user.click(screen.getByRole("button", { name: "Plan" }));
    expect(await screen.findByText(/^Invalid JSON/)).toBeInTheDocument();
    paste("[]");
    expect(screen.getByText("The document must be a JSON object.")).toBeInTheDocument();
    expect(fetchMock.find("POST /api/v1/tenants/acme/access-config/plan")).toHaveLength(0);
  });

  it("plans, then applies after confirmation", async () => {
    const { user, fetchMock } = renderApp({
      route,
      api: {
        ...base,
        "POST /api/v1/tenants/acme/access-config/plan": { accepted: true, errors: null, changes: ["role ci: create"] },
        "POST /api/v1/tenants/acme/access-config/apply": { accepted: true, errors: [], changes: ["role ci: create"] },
      },
    });
    await screen.findByText("No document applied yet");
    const apply = screen.getByRole("button", { name: "Apply" });
    paste(DOC);
    expect(apply).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "Plan" }));

    const result = await screen.findByRole("region", { name: "Plan result" });
    expect(result).toHaveTextContent("Accepted");
    expect(within(result).getByRole("list", { name: "Changes" })).toHaveTextContent("role ci: create");
    expect(fetchMock.find("POST /api/v1/tenants/acme/access-config/plan")[0]!.body).toEqual({ content: JSON.parse(DOC) as unknown });
    expect(apply).toBeEnabled();

    await user.click(apply);
    const confirm = await screen.findByRole("alertdialog", { name: "Apply this access document?" });
    expect(confirm).toHaveTextContent("1 planned change to acme");
    await user.click(within(confirm).getByRole("button", { name: "Apply" }));
    expect(await screen.findByRole("region", { name: "Apply result" })).toHaveTextContent("Applied");
    expect(fetchMock.find("POST /api/v1/tenants/acme/access-config/apply")[0]!.body).toEqual({ content: JSON.parse(DOC) as unknown });
    // The applied document is reloaded.
    await waitFor(() => expect(fetchMock.find("GET /api/v1/tenants/acme/access-config").length).toBeGreaterThanOrEqual(2));
    expect(screen.getByRole("button", { name: "Apply" })).toBeDisabled();
  });

  it("shows plan errors and keeps apply disabled; editing invalidates the plan", async () => {
    const { user } = renderApp({
      route,
      api: {
        ...base,
        "POST /api/v1/tenants/acme/access-config/plan": {
          accepted: false,
          errors: ["E2001 unknown action \"ci:fly\"", { code: "E2003", message: "bad resource" }],
          changes: [],
        },
      },
    });
    await screen.findByText("No document applied yet");
    paste(DOC);
    await user.click(screen.getByRole("button", { name: "Plan" }));
    const result = await screen.findByRole("region", { name: "Plan result" });
    expect(result).toHaveTextContent("Rejected");
    const errors = within(result).getByRole("list", { name: "Errors" });
    expect(errors).toHaveTextContent('E2001 unknown action "ci:fly"');
    expect(errors).toHaveTextContent("E2003: bad resource");
    expect(screen.getByRole("button", { name: "Apply" })).toBeDisabled();

    paste(`${DOC} `);
    expect(screen.queryByRole("region", { name: "Plan result" })).not.toBeInTheDocument();
    expect(screen.getByText("The document changed since the last plan. Plan again before applying.")).toBeInTheDocument();
  });

  it("explains 501 not_implemented", async () => {
    const { user } = renderApp({
      route,
      api: {
        ...base,
        "GET /api/v1/tenants/acme/access-config": apiError(501, "not_implemented", "access config is not configured"),
        "POST /api/v1/tenants/acme/access-config/plan": apiError(501, "not_implemented", "access config is not configured"),
      },
    });
    expect(await screen.findByText("Declarative access config is not available on this server")).toBeInTheDocument();
    paste(DOC);
    await user.click(screen.getByRole("button", { name: "Plan" }));
    await waitFor(() =>
      expect(screen.getAllByText("Declarative access config is not available on this server")).toHaveLength(2),
    );
  });
});
