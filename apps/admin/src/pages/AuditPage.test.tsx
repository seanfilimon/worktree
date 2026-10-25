import { fireEvent, screen, within } from "@testing-library/react";
import { beforeAll, describe, expect, it, vi } from "vitest";
import type { AuditEvent } from "@/api/types";
import { toCsv } from "@/components/dashboard/csv";
import * as fx from "@/test/fixtures";
import * as dx from "@/test/fixtures.dashboard";
import { polyfillRadixSelect } from "@/test/radixPolyfills";
import { renderApp } from "@/test/renderApp";

beforeAll(polyfillRadixSelect);

/** jsdom's Blob has no text(). */
function readText(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(reader.result as string);
    reader.onerror = () => reject(reader.error ?? new Error("read failed"));
    reader.readAsText(blob);
  });
}

const page = (n: number): AuditEvent[] =>
  Array.from({ length: n }, (_, i) => ({ ...dx.auditEvents[1]!, id: `p${i}`, actor: `user${i}` }));

describe("AuditPage", () => {
  it("builds the query string from the filters", async () => {
    const { user, fetchMock, router } = renderApp({ route: "/audit", api: { "GET /api/v1/audit": dx.auditEvents } });
    expect(await screen.findByRole("table", { name: "Audit events" })).toHaveTextContent("auth.login");
    const first = fetchMock.find("GET /api/v1/audit")[0]!.url.searchParams;
    expect([...first.keys()]).toEqual(["limit"]);
    expect(first.get("limit")).toBe("50");

    await user.click(screen.getByRole("combobox", { name: "Tenant" }));
    await user.click(await screen.findByRole("option", { name: "acme" }));
    await user.type(screen.getByLabelText("Action"), "branch.push");
    await user.type(screen.getByLabelText("Actor"), "BO");
    fireEvent.change(screen.getByLabelText("Since"), { target: { value: "2026-10-01" } });
    await user.click(screen.getByRole("button", { name: "Apply" }));

    const last = fetchMock.find("GET /api/v1/audit").at(-1)!.url.searchParams;
    expect(last.get("tenant")).toBe("acme");
    expect(last.get("action")).toBe("branch.push");
    expect(last.get("since")).toBe(new Date("2026-10-01T00:00:00").toISOString().replace(/\.\d{3}Z$/, "Z"));
    expect(last.get("limit")).toBe("50");
    // The server has no actor filter: it is applied to the loaded events.
    expect(last.has("actor")).toBe(false);

    const table = screen.getByRole("table", { name: "Audit events" });
    expect(within(table).getByText("bob")).toBeInTheDocument();
    expect(within(table).queryByText("alice")).not.toBeInTheDocument();
    expect(screen.getByText(/1 event by actors matching “BO” · 2 loaded/)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Clear filters" }));
    expect(router.state.location.search).toBe("");
    expect(await within(table).findByText("alice")).toBeInTheDocument();
    expect(screen.getByLabelText("Action")).toHaveValue("");
    expect(screen.queryByRole("button", { name: "Clear filters" })).not.toBeInTheDocument();
  });

  it("loads more events by raising the limit", async () => {
    const { user, fetchMock } = renderApp({
      route: "/audit",
      api: { "GET /api/v1/audit": (req) => page(Math.min(Number(req.url.searchParams.get("limit")), 70)) },
    });
    const table = await screen.findByRole("table", { name: "Audit events" });
    expect(within(table).getAllByRole("row")).toHaveLength(51);

    await user.click(screen.getByRole("button", { name: "Load 50 more" }));
    expect(await screen.findByText(/70 events · 70 loaded/)).toBeInTheDocument();
    expect(fetchMock.find("GET /api/v1/audit").at(-1)!.url.searchParams.get("limit")).toBe("100");
    // Fewer than the limit came back: everything is loaded.
    expect(screen.queryByRole("button", { name: /more/ })).not.toBeInTheDocument();
  });

  it("expands a row to show the event JSON", async () => {
    const { user } = renderApp({ route: "/audit", api: { "GET /api/v1/audit": dx.auditEvents } });
    const toggle = await screen.findByRole("button", { name: "Details: branch.push by bob" });
    expect(toggle).toHaveAttribute("aria-expanded", "false");

    await user.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    const details = document.getElementById(toggle.getAttribute("aria-controls")!)!;
    expect(details).toHaveTextContent('"remote_addr": "10.0.0.2"');
    expect(details).toHaveTextContent('"count": "2"');
    expect(details).toHaveTextContent(dx.auditEvents[0]!.at);

    await user.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByText(/"remote_addr"/)).not.toBeInTheDocument();
  });

  it("exports the visible events as CSV", async () => {
    let blob: Blob | undefined;
    const create = vi.fn((b: Blob) => {
      blob = b;
      return "blob:audit";
    });
    // jsdom has no object URLs; the subclass is unstubbed after the test.
    vi.stubGlobal(
      "URL",
      class extends URL {
        static override createObjectURL = create;
        static override revokeObjectURL = vi.fn();
      },
    );
    const click = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => undefined);

    const { user } = renderApp({ route: "/audit", api: { "GET /api/v1/audit": dx.auditEvents } });
    await screen.findByRole("table", { name: "Audit events" });
    await user.click(screen.getByRole("button", { name: "Export CSV" }));

    expect(create).toHaveBeenCalledOnce();
    expect(click).toHaveBeenCalledOnce();
    expect((click.mock.contexts[0] as HTMLAnchorElement).download).toMatch(/^audit-\d{8}-\d{6}\.csv$/);
    expect(blob?.type).toBe("text/csv;charset=utf-8");
    const [push, login] = dx.auditEvents;
    expect(await readText(blob!)).toBe(
      [
        "at,actor,actor_id,action,resource_type,resource_id,tenant,remote_addr,details",
        `${push!.at},bob,${fx.bob.id},branch.push,branch,tree-web/main,acme,10.0.0.2,"{""tree"":""web"",""branch"":""main"",""count"":""2""}"`,
        `${login!.at},alice,acc-1,auth.login,account,acc-1,acme,127.0.0.1,`,
        "",
      ].join("\r\n"),
    );
  });

  it("disables CSV export when nothing is shown", async () => {
    renderApp({ route: "/audit", api: { "GET /api/v1/audit": [] } });
    expect(await screen.findByText("No matching events")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Export CSV" })).toBeDisabled();
  });
});

describe("toCsv", () => {
  it("quotes separators, doubles quotes and neutralises formulas", () => {
    expect(toCsv(["a", "b"], [["x,y", 'say "hi"'], ["line\nbreak", undefined], ["=SUM(A1)", 42], ["@cmd", null]])).toBe(
      'a,b\r\n"x,y","say ""hi"""\r\n"line\nbreak",\r\n\'=SUM(A1),42\r\n\'@cmd,\r\n',
    );
  });
});
