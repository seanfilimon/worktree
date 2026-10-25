import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { installDomPolyfills } from "@/test/dom.history";
import { fileDiffs } from "@/test/fixtures.history";
import { DiffViewer } from "./DiffViewer";

installDomPolyfills();

/** All cells, including the aria-hidden +/- gutters. */
const cellText = (row: HTMLElement) => [...row.querySelectorAll("td")].map((c) => c.textContent);

function setup() {
  const user = userEvent.setup();
  render(<DiffViewer files={fileDiffs} />);
  return { user };
}

describe("DiffViewer", () => {
  it("summarises the change set and lists files with their status", () => {
    setup();
    expect(screen.getByText("5 files changed")).toBeInTheDocument();
    expect(screen.getByText("0 / 5 viewed")).toBeInTheDocument();
    const nav = screen.getByRole("navigation", { name: "Changed files" });
    const items = within(nav).getAllByRole("button");
    expect(items).toHaveLength(5);
    expect(items[3]).toHaveTextContent("app.ts");
    expect(items[3]).toHaveTextContent("modified");
    expect(items[4]).toHaveTextContent("added");
  });

  it("renders unified hunks with line numbers, markers and placeholders", () => {
    setup();
    const table = screen.getByRole("table", { name: "Changes to src/app.ts" });
    expect(within(table).getByText("@@ -1,4 +1,5 @@ export function main()")).toBeInTheDocument();

    const rows = within(table).getAllByRole("row");
    const del = rows.find((r) => r.dataset.kind === "del")!;
    const add = rows.find((r) => r.dataset.kind === "add")!;
    expect(cellText(del)).toEqual(["2", "", "-", "const b = 2;"]);
    expect(cellText(add)).toEqual(["", "2", "+", "const b = 3;"]);
    expect(del).toHaveClass("bg-diff-remove");
    expect(add).toHaveClass("bg-diff-add");
    expect(within(del).getByText("const b = 2;")).toHaveClass("line-through");
    expect(within(table).getByText("no newline at end of file")).toBeInTheDocument();

    expect(screen.getByText(/Binary file not shown/)).toHaveTextContent("1.0 KiB → 2.0 KiB");
    expect(screen.getByText(/File too large to display/)).toBeInTheDocument();
  });

  it("switches to a side-by-side layout and remembers it", async () => {
    const { user } = setup();
    await user.click(screen.getByRole("radio", { name: "Split view" }));
    const table = screen.getByRole("table", { name: "Changes to src/app.ts" });
    const changed = within(table)
      .getAllByRole("row")
      .find((r) => r.textContent?.includes("const b = 2;"))!;
    expect(cellText(changed)).toEqual(["2", "-", "const b = 2;", "2", "+", "const b = 3;"]);
    expect(localStorage.getItem("wt.diff.view")).toBe("split");
  });

  it("collapses, expands and marks files as viewed", async () => {
    const { user } = setup();
    await user.click(screen.getByRole("button", { name: "Collapse all" }));
    expect(screen.queryByRole("table", { name: "Changes to src/app.ts" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Expand all" }));
    expect(screen.getByRole("table", { name: "Changes to src/app.ts" })).toBeInTheDocument();

    const card = screen.getByRole("region", { name: "src/app.ts" });
    await user.click(within(card).getByRole("checkbox", { name: "Viewed" }));
    expect(screen.queryByRole("table", { name: "Changes to src/app.ts" })).not.toBeInTheDocument();
    expect(screen.getByText("1 / 5 viewed")).toBeInTheDocument();

    await user.click(within(card).getByRole("button", { name: "Expand src/app.ts" }));
    expect(screen.getByRole("table", { name: "Changes to src/app.ts" })).toBeInTheDocument();
  });

  it("shows an empty state without files", () => {
    render(<DiffViewer files={[]} />);
    expect(screen.getByText("No file changes.")).toBeInTheDocument();
  });
});
