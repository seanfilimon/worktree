import { screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { backupAuthor, backupFileName, type BackupInfo, type BackupStatus } from "@/api/backups";
import * as fx from "@/test/fixtures";
import { apiError, json, noContent } from "@/test/mockFetch";
import { renderApp } from "@/test/renderApp";

// storage-backends (docs/ENGINEERING_PLAN.md §15.2).

const manual: BackupInfo = {
  id: "bk-20261020T101500Z-a1b2c3",
  path: "/var/lib/worktree/backups/bk-20261020T101500Z-a1b2c3.wtbak",
  size_bytes: 3 * 1024 * 1024,
  objects: 1234,
  created_at: "2026-10-20T10:15:00Z",
  created_by: "admin",
  note: "before upgrade",
};
const scheduled: BackupInfo = {
  id: "bk-20261021T000000Z-d4e5f6",
  path: "/var/lib/worktree/backups/bk-20261021T000000Z-d4e5f6.wtbak",
  size_bytes: 4096,
  objects: 1300,
  created_at: "2026-10-21T00:00:00Z",
  created_by: "system",
};

const status: BackupStatus = {
  config: { dir: "/var/lib/worktree/backups", interval: "24h0m0s", interval_seconds: 86400, keep: 7 },
  store: { kind: "s3", endpoint: "minio:9000", bucket: "wt-objects", prefix: "prod/", secure: false, compress: true },
  supported: true,
  running: false,
  next_run_at: "2026-10-22T00:00:00Z",
  backups: [scheduled, manual],
};

const routes = { "GET /api/v1/admin/backups": status };

afterEach(() => {
  vi.restoreAllMocks();
});

describe("BackupsPage", () => {
  it("is restricted to server admins", async () => {
    const { fetchMock } = renderApp({ route: "/backups", api: { ...routes, "GET /api/v1/auth/me": fx.memberMe } });
    expect(await screen.findByText("Server administrators only")).toBeInTheDocument();
    expect(fetchMock.find("GET /api/v1/admin/backups")).toHaveLength(0);
  });

  it("lists backups and shows the S3 store and schedule", async () => {
    renderApp({ route: "/backups", api: routes });
    const table = await screen.findByRole("table", { name: "Backups" });
    const rows = within(table).getAllByRole("row");
    expect(rows).toHaveLength(3);
    expect(rows[1]).toHaveTextContent(scheduled.id);
    expect(rows[1]).toHaveTextContent("schedule");
    expect(rows[2]).toHaveTextContent("before upgrade");
    expect(rows[2]).toHaveTextContent("3.0 MiB");
    expect(rows[2]).toHaveTextContent("1,234");

    const config = screen.getByLabelText("Backup configuration");
    expect(config).toHaveTextContent("http://minio:9000");
    expect(config).toHaveTextContent("wt-objects");
    expect(config).toHaveTextContent("prod/");
    expect(config).toHaveTextContent("Every 24h0m0s");
    expect(config).toHaveTextContent("7 scheduled backups");
    expect(screen.getByText("s3")).toBeInTheDocument();
    expect(screen.getByText(/worktree-server restore --data/)).toBeInTheDocument();
  });

  it("shows a disk store and the empty state", async () => {
    renderApp({
      route: "/backups",
      api: {
        "GET /api/v1/admin/backups": {
          ...status,
          store: { kind: "disk", root: "/data", compress: false },
          config: { ...status.config, interval: "0s", interval_seconds: 0, keep: 0 },
          next_run_at: undefined,
          backups: [],
        },
      },
    });
    expect(await screen.findByText("No backups yet")).toBeInTheDocument();
    const config = screen.getByLabelText("Backup configuration");
    expect(config).toHaveTextContent("/data/objects");
    expect(config).toHaveTextContent("Off");
    expect(config).toHaveTextContent("All");
  });

  it("creates a backup with a note", async () => {
    let list = status;
    const created: BackupInfo = { ...manual, id: "bk-20261022T120000Z-0f0f0f", note: "pre-0.8" };
    const { user, fetchMock } = renderApp({
      route: "/backups",
      api: {
        "GET /api/v1/admin/backups": () => json(list),
        "POST /api/v1/admin/backups": () => {
          list = { ...status, backups: [created, ...status.backups] };
          return json(created, 201);
        },
      },
    });
    await user.click(await screen.findByRole("button", { name: "Create backup" }));
    const dialog = await screen.findByRole("dialog", { name: "Create a backup" });
    await user.type(within(dialog).getByLabelText("Note (optional)"), "pre-0.8");
    await user.click(within(dialog).getByRole("button", { name: "Create backup" }));
    expect(await screen.findByText("Backup created")).toBeInTheDocument();
    expect(fetchMock.find("POST /api/v1/admin/backups")[0]!.body).toEqual({ note: "pre-0.8" });
    await waitFor(() => expect(screen.getByRole("table", { name: "Backups" })).toHaveTextContent(created.id));
  });

  it("explains a backup that is already running", async () => {
    const { user } = renderApp({
      route: "/backups",
      api: { ...routes, "POST /api/v1/admin/backups": apiError(409, "backup_running", "a backup is already running") },
    });
    await user.click(await screen.findByRole("button", { name: "Create backup" }));
    const dialog = await screen.findByRole("dialog", { name: "Create a backup" });
    await user.click(within(dialog).getByRole("button", { name: "Create backup" }));
    expect(await within(dialog).findByText(/a backup is already running/)).toBeInTheDocument();
  });

  it("downloads a backup through the authenticated API", async () => {
    const createObjectURL = vi.fn(() => "blob:backup");
    Object.assign(URL, { createObjectURL, revokeObjectURL: vi.fn() });
    const click = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => undefined);
    const { user, fetchMock } = renderApp({
      route: "/backups",
      api: {
        ...routes,
        [`GET /api/v1/admin/backups/${manual.id}/download`]: () =>
          new Response(new Uint8Array([0x28, 0xb5, 0x2f, 0xfd]), { headers: { "Content-Type": "application/octet-stream" } }),
      },
    });
    await user.click(await screen.findByRole("button", { name: `Download ${manual.id}` }));
    await waitFor(() => expect(click).toHaveBeenCalledTimes(1));
    expect(createObjectURL).toHaveBeenCalledTimes(1);
    const req = fetchMock.find(`GET /api/v1/admin/backups/${manual.id}/download`)[0]!;
    expect(req.headers.get("Authorization")).toBe("Bearer test-token");
  });

  it("deletes a backup only after confirmation", async () => {
    let list = status;
    const { user, fetchMock } = renderApp({
      route: "/backups",
      api: {
        "GET /api/v1/admin/backups": () => json(list),
        [`DELETE /api/v1/admin/backups/${manual.id}`]: () => {
          list = { ...status, backups: [scheduled] };
          return noContent();
        },
      },
    });
    await user.click(await screen.findByRole("button", { name: `Delete ${manual.id}` }));
    const confirm = await screen.findByRole("alertdialog", { name: "Delete this backup?" });
    expect(fetchMock.find(`DELETE /api/v1/admin/backups/${manual.id}`)).toHaveLength(0);
    await user.click(within(confirm).getByRole("button", { name: "Delete backup" }));
    expect(await screen.findByText("Backup deleted")).toBeInTheDocument();
    await waitFor(() => expect(screen.getByRole("table", { name: "Backups" })).not.toHaveTextContent(manual.id));
  });

  it("explains unsupported stores, failed schedules and 501", async () => {
    const { unmount } = renderApp({
      route: "/backups",
      api: { "GET /api/v1/admin/backups": { ...status, supported: false, last_error: "disk full" } },
    });
    expect(await screen.findByText(/stores cannot be backed up/)).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent("disk full");
    expect(screen.getByRole("button", { name: "Create backup" })).toBeDisabled();
    unmount();

    renderApp({ route: "/backups", api: { "GET /api/v1/admin/backups": apiError(501, "not_implemented", "backups are not available") } });
    expect(await screen.findByText("Backup is not available on this server")).toBeInTheDocument();
  });
});

describe("backup helpers", () => {
  it("formats authors and file names", () => {
    expect(backupAuthor(scheduled)).toBe("schedule");
    expect(backupAuthor(manual)).toBe("admin");
    expect(backupAuthor({ ...manual, created_by: "" })).toBe("—");
    expect(backupFileName("bk-1")).toBe("bk-1.wtbak");
  });
});
