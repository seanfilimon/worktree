import { Download } from "lucide-react";
import { CopyButton } from "@/components/people/CopyButton";
import { Button } from "@/components/ui/button";

interface RecoveryCodesProps {
  codes: string[];
  username?: string;
}

/** Saves the codes as a plain-text file in the browser (no server round trip). */
function download(codes: string[], username: string | undefined) {
  const text = [
    `W0rkTree recovery codes${username ? ` for ${username}` : ""}`,
    `Generated ${new Date().toISOString()}`,
    "Each code can be used once instead of an authenticator code.",
    "",
    ...codes,
    "",
  ].join("\n");
  const url = URL.createObjectURL(new Blob([text], { type: "text/plain" }));
  const a = document.createElement("a");
  a.href = url;
  a.download = "w0rktree-recovery-codes.txt";
  document.body.append(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 0);
}

/** One-time display of freshly generated recovery codes. */
export function RecoveryCodes({ codes, username }: RecoveryCodesProps) {
  return (
    <div className="flex flex-col gap-3">
      <ol
        aria-label="Recovery codes"
        className="grid grid-cols-2 gap-x-6 gap-y-1 rounded-md border bg-muted/40 p-4 font-mono text-sm sm:grid-cols-2"
      >
        {codes.map((c) => (
          <li key={c}>{c}</li>
        ))}
      </ol>
      <p className="text-xs text-muted-foreground">
        Store these somewhere safe. Each code works once if you lose your authenticator. They are not shown again.
      </p>
      <div className="flex flex-wrap gap-2">
        <CopyButton value={codes.join("\n")} label="Copy recovery codes" copiedMessage="Recovery codes copied" />
        <Button type="button" variant="outline" size="sm" onClick={() => download(codes, username)}>
          <Download aria-hidden="true" />
          Download
        </Button>
      </div>
    </div>
  );
}
