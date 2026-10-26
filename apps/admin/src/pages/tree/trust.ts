/**
 * v0.7 trust (docs/ENGINEERING_PLAN.md §15.8): signed tags and verified
 * releases — wire fields and the checksums/SBOM endpoints used by the Tags
 * and Releases tabs (components in ./TrustBadges.tsx).
 *
 * Field names mirror services/server-go/internal/domain (the `v0.7 trust`
 * blocks of types.go and v05.go).
 */
import { useMemo } from "react";
import type { ApiClient } from "@/api/client";
import { useApi } from "@/api/context";
import type { Tag } from "@/api/types";
import type { Release, ReleaseAsset } from "@/api/v05tree";

/** Signature fields of a tag (all optional: unsigned tags omit them). */
export interface TagTrust {
  /** Base64 Ed25519 signature. */
  signature?: string;
  signing_key_id?: string;
  /** Set by the server after checking the signature against a registered key. */
  verified?: boolean;
  /** Username owning the signing key. */
  signer?: string;
  /** The tagger value the signature covers. */
  signed_tagger?: string;
}
export type SignedTag = Tag & TagTrust;

/** Computed trust fields of a release (from its tag). */
export interface ReleaseTrust {
  verified?: boolean;
  signing_key_id?: string;
  signer?: string;
}
export type TrustedRelease = Release & ReleaseTrust;

/** Asset checksum (absent for assets uploaded before v0.7). */
export type TrustedAsset = ReleaseAsset & { sha256?: string };

/** Name of the generated SBOM asset. */
export const SBOM_ASSET = "sbom.spdx.json";

const seg = encodeURIComponent;
const releasePath = (tenant: string, tree: string, tag: string) => `/api/v1/trees/${seg(tenant)}/${seg(tree)}/releases/${seg(tag)}`;

export function createTrustApi(client: ApiClient) {
  return {
    /** SHA256SUMS text of a release's assets. */
    checksums: async (tenant: string, tree: string, tag: string) =>
      (await client.raw(`${releasePath(tenant, tree, tag)}/checksums`)).text(),
    /** Generate the SPDX SBOM and attach it as sbom.spdx.json. */
    generateSbom: (tenant: string, tree: string, tag: string) =>
      client.request<TrustedAsset>(`${releasePath(tenant, tree, tag)}/sbom`, { method: "POST" }),
  };
}

export function useTrustApi() {
  const { client } = useApi();
  return useMemo(() => createTrustApi(client), [client]);
}

export const trustKeys = {
  checksums: (t: string, n: string, tag: string) => ["tree", t, n, "releases", tag, "checksums"] as const,
};

/** Parse SHA256SUMS lines into (name, sha256) pairs. */
export function parseChecksums(text: string): { name: string; sha256: string }[] {
  const out: { name: string; sha256: string }[] = [];
  for (const line of text.split(/\r?\n/)) {
    const m = /^([0-9a-fA-F]{64}) [ *](.+)$/.exec(line);
    if (m) out.push({ sha256: m[1]!.toLowerCase(), name: m[2]! });
  }
  return out;
}
