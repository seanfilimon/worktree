import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import * as v5 from "@/test/fixtures.v05";
import { json } from "@/test/mockFetch";
import { renderApp, treeRoutes } from "@/test/renderApp";
import { parseChecksums, SBOM_ASSET, type TrustedRelease } from "./trust";

// v0.7 trust (ENGINEERING_PLAN §15.8): verified badges, checksums, SBOM.

const T = "/api/v1/trees/acme/web";
const SUM_A = "a".repeat(64);
const SUM_B = "b".repeat(64);

const signedTag = {
  tree_id: "tree-web",
  name: "v1.0.0",
  target: "snap-3",
  message: "First stable",
  tagger: "alice",
  created_at: new Date().toISOString(),
  signature: "AAAA",
  signing_key_id: "key-1",
  verified: true,
  signer: "alice",
  signed_tagger: "Alice <alice@example.com>",
};
const plainTag = { ...signedTag, name: "nightly", signature: undefined, signing_key_id: undefined, verified: false, signer: undefined, signed_tagger: undefined };

const verifiedRelease: TrustedRelease = {
  ...v5.release,
  verified: true,
  signer: "alice",
  signing_key_id: "key-1",
  assets: v5.release.assets.map((a) => ({ ...a, sha256: SUM_A })),
};

describe("trust helpers", () => {
  it("parses SHA256SUMS text", () => {
    expect(parseChecksums(`${SUM_A}  web.tar.gz\n${SUM_B.toUpperCase()} *b in.bin\r\nnot a line\n`)).toEqual([
      { name: "web.tar.gz", sha256: SUM_A },
      { name: "b in.bin", sha256: SUM_B },
    ]);
  });
});

describe("tags tab trust", () => {
  it("marks verified tags and shows the signed tagger", async () => {
    renderApp({ route: "/trees/acme/web/tags", api: treeRoutes({ [`GET ${T}/tags`]: [signedTag, plainTag] }) });
    const table = await screen.findByRole("table", { name: "Tags" });
    const rows = within(table).getAllByRole("row").slice(1);
    const signed = rows.find((r) => r.textContent?.includes("v1.0.0"))!;
    const plain = rows.find((r) => r.textContent?.includes("nightly"))!;
    expect(within(signed).getByLabelText("Verified: signed by alice")).toHaveTextContent("Verified");
    expect(within(signed).getByText("Alice <alice@example.com>")).toBeInTheDocument();
    expect(within(plain).queryByText("Verified")).not.toBeInTheDocument();
  });
});

describe("releases tab trust", () => {
  it("shows verified releases, asset checksums and the SHA256SUMS dialog", async () => {
    const { user, fetchMock } = renderApp({
      route: "/trees/acme/web/releases",
      api: treeRoutes({
        [`GET ${T}/releases`]: [verifiedRelease],
        [`GET ${T}/releases/v1.0.0/checksums`]: () => new Response(`${SUM_A}  web.tar.gz\n`, { headers: { "Content-Type": "text/plain" } }),
      }),
    });
    const card = await screen.findByRole("article", { name: "Release First stable" });
    expect(within(card).getByLabelText("Verified: signed by alice")).toBeInTheDocument();
    const assets = within(card).getByRole("table", { name: "Assets of v1.0.0" });
    expect(within(assets).getByLabelText(`SHA-256 ${SUM_A}`)).toHaveTextContent(SUM_A.slice(0, 12));
    expect(within(assets).getByLabelText("Checksum of a verified release")).toBeInTheDocument();

    await user.click(within(card).getByRole("button", { name: "Checksums of v1.0.0" }));
    const dialog = await screen.findByRole("dialog", { name: "Checksums of v1.0.0" });
    expect(await within(dialog).findByLabelText("SHA256SUMS")).toHaveTextContent(`${SUM_A} web.tar.gz`);
    expect(fetchMock.find(`GET ${T}/releases/v1.0.0/checksums`)).toHaveLength(1);
  });

  it("generates and downloads the SBOM", async () => {
    let current: TrustedRelease = { ...v5.release, verified: false };
    const { user, fetchMock } = renderApp({
      route: "/trees/acme/web/releases",
      api: treeRoutes({
        [`GET ${T}/releases`]: () => json([current]),
        [`POST ${T}/releases/v1.0.0/sbom`]: () => {
          const asset = { name: SBOM_ASSET, size: 900, hash: "h", content_type: "application/spdx+json", uploaded_at: new Date().toISOString(), sha256: SUM_B };
          current = { ...current, assets: [...current.assets, asset] };
          return json(asset, 201);
        },
        [`GET ${T}/releases/v1.0.0/assets/${SBOM_ASSET}`]: () => new Response("{}"),
      }),
    });
    const card = await screen.findByRole("article", { name: "Release First stable" });
    expect(within(card).queryByText("Verified")).not.toBeInTheDocument();
    expect(within(card).queryByRole("button", { name: "Download SBOM" })).not.toBeInTheDocument();
    await user.click(within(card).getByRole("button", { name: "Generate SBOM" }));
    await waitFor(() => expect(fetchMock.find(`POST ${T}/releases/v1.0.0/sbom`)).toHaveLength(1));
    const download = await within(card).findByRole("button", { name: "Download SBOM" });
    expect(within(card).getByRole("button", { name: "Regenerate SBOM" })).toBeInTheDocument();
    await user.click(download);
    await waitFor(() => expect(fetchMock.find(`GET ${T}/releases/v1.0.0/assets/${SBOM_ASSET}`)).toHaveLength(1));
  });
});
