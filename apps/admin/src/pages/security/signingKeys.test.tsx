import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import * as ox from "@/test/fixtures.ops";
import * as px from "@/test/fixtures.people";
import { apiError, json, noContent } from "@/test/mockFetch";
import { renderApp } from "@/test/renderApp";

const base = { "GET /api/v1/auth/me": px.meMulti, "GET /api/v1/auth/tokens": [] };

async function keysCard() {
  const table = await screen.findByRole("table", { name: "Signing keys" });
  return table;
}

describe("settings: signing keys", () => {
  it("lists active and revoked keys", async () => {
    renderApp({ route: "/settings", api: { ...base, "GET /api/v1/auth/keys": [ox.signingKey, ox.revokedKey] } });
    const table = await keysCard();
    const active = within(table).getByText("laptop").closest("tr")!;
    expect(active).toHaveTextContent("SHA256:abcDEF123");
    expect(within(active).getByRole("button", { name: "Revoke signing key: laptop" })).toBeInTheDocument();
    const revoked = within(table).getByText("old-desktop").closest("tr")!;
    expect(revoked).toHaveTextContent("Revoked");
    expect(within(revoked).queryByRole("button")).not.toBeInTheDocument();
  });

  it("validates and adds a base64 public key", async () => {
    const pub = btoa(String.fromCharCode(...new Uint8Array(32).fill(9)));
    const { user, fetchMock } = renderApp({
      route: "/settings",
      api: {
        ...base,
        "GET /api/v1/auth/keys": [],
        "POST /api/v1/auth/keys": (req) => json({ ...ox.signingKey, ...(req.body as object), id: "key-new" }, 201),
      },
    });
    expect(await screen.findByText("No signing keys")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Add key" }));
    const dialog = await screen.findByRole("dialog", { name: "Add signing key" });

    await user.click(within(dialog).getByRole("button", { name: "Add key" }));
    expect(await within(dialog).findByText("Required.")).toBeInTheDocument();
    expect(within(dialog).getByText("Paste the public key.")).toBeInTheDocument();

    await user.type(within(dialog).getByLabelText("Name"), "desktop");
    await user.type(within(dialog).getByLabelText("Public key (base64)"), "AAAA");
    await user.click(within(dialog).getByRole("button", { name: "Add key" }));
    expect(await within(dialog).findByText("An Ed25519 public key is 32 bytes; this is 3.")).toBeInTheDocument();
    expect(fetchMock.find("POST /api/v1/auth/keys")).toHaveLength(0);

    const field = within(dialog).getByLabelText("Public key (base64)");
    await user.clear(field);
    await user.type(field, `  ${pub}\n`);
    await user.click(within(dialog).getByRole("button", { name: "Add key" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(fetchMock.find("POST /api/v1/auth/keys")[0]!.body).toEqual({ name: "desktop", public_key: pub });
    expect(fetchMock.find("GET /api/v1/auth/keys").length).toBeGreaterThanOrEqual(2);
  });

  it("shows server errors inside the dialog", async () => {
    const { user } = renderApp({
      route: "/settings",
      api: {
        ...base,
        "GET /api/v1/auth/keys": [],
        "POST /api/v1/auth/keys": apiError(409, "conflict", "key already registered"),
      },
    });
    await user.click(await screen.findByRole("button", { name: "Add key" }));
    const dialog = await screen.findByRole("dialog", { name: "Add signing key" });
    await user.type(within(dialog).getByLabelText("Name"), "dup");
    await user.type(within(dialog).getByLabelText("Public key (base64)"), "07".repeat(32));
    await user.click(within(dialog).getByRole("button", { name: "Add key" }));
    expect(await within(dialog).findByText("key already registered")).toBeInTheDocument();
  });

  it("revokes a key after confirmation", async () => {
    const { user, fetchMock } = renderApp({
      route: "/settings",
      api: { ...base, "GET /api/v1/auth/keys": [ox.signingKey], "DELETE /api/v1/auth/keys/key-1": noContent() },
    });
    await user.click(await screen.findByRole("button", { name: "Revoke signing key: laptop" }));
    const confirm = await screen.findByRole("alertdialog", { name: "Revoke this signing key?" });
    await user.click(within(confirm).getByRole("button", { name: "Revoke" }));
    expect(await screen.findByText("Signing key revoked")).toBeInTheDocument();
    expect(fetchMock.find("DELETE /api/v1/auth/keys/key-1")).toHaveLength(1);
  });

  it("explains 501 not_implemented", async () => {
    renderApp({
      route: "/settings",
      api: { ...base, "GET /api/v1/auth/keys": apiError(501, "not_implemented", "signing is not configured") },
    });
    expect(await screen.findByText("Snapshot signing is not available on this server")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Add key" })).not.toBeInTheDocument();
  });
});
