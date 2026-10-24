import type { Metadata } from "next";
import Link from "next/link";
import { Badge } from "@/components/ui/badge";
import { CURRENT_VERSION, REPO_URL } from "@/lib/site";

export const metadata: Metadata = {
  title: "Security - W0rkTree",
  description:
    "How W0rkTree protects your code and credentials, and how to report a vulnerability.",
};

interface Control {
  title: string;
  description: string;
  roadmap?: boolean;
}

const controls: Control[] = [
  {
    title: "Secret Scanning",
    description:
      "Added and modified files are scanned before every snapshot. Findings block the snapshot and are reported with redacted excerpts.",
  },
  {
    title: "Append-Only History",
    description:
      "Snapshots are immutable. Reverts add new snapshots, branches are archived instead of deleted, and the server has no force push.",
  },
  {
    title: "Password Hashing",
    description: "Account passwords are hashed with argon2id.",
  },
  {
    title: "Tokens",
    description:
      "Bearer tokens on every gRPC and REST call. Session tokens expire; personal API tokens can carry a TTL and are stored only as SHA-256 hashes.",
  },
  {
    title: "Authorization",
    description:
      "Five tenant roles, private/internal/public tree visibility, branch protection and per-path license rules, all enforced by the server.",
  },
  {
    title: "Audit Log",
    description:
      "Pushes, sign-ins and administrative changes are written to an append-only audit log, visible to tenant and server admins.",
  },
  {
    title: "Transport",
    description:
      "The server speaks cleartext HTTP/2 (h2c). Run it behind a TLS-terminating reverse proxy for anything beyond localhost.",
  },
  {
    title: "Snapshot Signing",
    description: "Cryptographic signatures on snapshots.",
    roadmap: true,
  },
  {
    title: "Encryption at Rest",
    description: "Server-side encryption of metadata and objects.",
    roadmap: true,
  },
];

export default function SecurityPage() {
  return (
    <div className="container mx-auto max-w-4xl px-4 py-20">
      <h1 className="mb-6 text-4xl font-bold">Security</h1>
      <p className="text-lg text-muted-foreground">
        The controls below describe W0rkTree {CURRENT_VERSION}. For deployment
        guidance and the full threat model, read the{" "}
        <Link href="/docs/security" className="underline underline-offset-4 hover:text-foreground">
          security documentation
        </Link>
        .
      </p>

      <div className="mt-8 grid grid-cols-1 gap-6 md:grid-cols-2">
        {controls.map((control) => (
          <div key={control.title} className="rounded-xl border bg-card p-6">
            <div className="flex items-center justify-between gap-3">
              <h2 className="font-bold">{control.title}</h2>
              {control.roadmap && <Badge variant="outline">Roadmap</Badge>}
            </div>
            <p className="mt-2 text-sm text-muted-foreground">{control.description}</p>
          </div>
        ))}
      </div>

      <section className="mt-12 rounded-xl border bg-muted/30 p-6" aria-labelledby="report-heading">
        <h2 id="report-heading" className="text-2xl font-semibold">
          Reporting a vulnerability
        </h2>
        <p className="mt-3 text-muted-foreground">
          Please do not open a public issue for security problems. Report them
          privately through{" "}
          <a
            href={`${REPO_URL}/security/advisories/new`}
            target="_blank"
            rel="noopener noreferrer"
            className="underline underline-offset-4 hover:text-foreground"
          >
            GitHub security advisories
          </a>{" "}
          with steps to reproduce, the affected component (CLI, daemon, server or
          admin panel) and the commit you tested.
        </p>
      </section>
    </div>
  );
}
