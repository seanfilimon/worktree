import { useMutation } from "@tanstack/react-query";
import { useState, type FormEvent } from "react";
import { useSecurityApi, type ResetCode } from "@/api/security";
import type { Account } from "@/api/types";
import { appUrl } from "@/auth/ssoHash";
import { CopyButton } from "@/components/people/CopyButton";
import { FormError } from "@/components/people/states";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { absoluteTime, relativeTime } from "@/lib/format";

const TTL_OPTIONS = [
  { hours: 1, label: "1 hour" },
  { hours: 4, label: "4 hours" },
  { hours: 24, label: "1 day" },
  { hours: 72, label: "3 days" },
];

/** Link to the public reset page with the code pre-filled. */
function resetLink(code: string): string {
  return `${appUrl("/reset-password")}?code=${encodeURIComponent(code)}`;
}

interface IssueResetCodeDialogProps {
  /** Account to issue a code for; null closes the dialog. */
  account: Account | null;
  onClose: () => void;
}

/** Admin: one-time password reset code (§12.3 POST /admin/accounts/{u}/reset-code). */
export function IssueResetCodeDialog({ account, onClose }: IssueResetCodeDialogProps) {
  const sec = useSecurityApi();
  const [ttl, setTtl] = useState("1");
  const [issued, setIssued] = useState<ResetCode | null>(null);

  const issue = useMutation({
    mutationFn: (v: { username: string; ttl: number }) => sec.admin.resetCode(v.username, v.ttl),
    meta: { inlineError: true },
    onSuccess: setIssued,
  });

  const changeOpen = (open: boolean) => {
    if (open) return;
    // The code is dropped here: it is never shown again.
    setIssued(null);
    setTtl("1");
    issue.reset();
    onClose();
  };

  const onSubmit = (e: FormEvent) => {
    e.preventDefault();
    if (account) issue.mutate({ username: account.username, ttl: Number(ttl) });
  };

  const link = issued ? resetLink(issued.code) : "";

  return (
    <Dialog open={account !== null} onOpenChange={changeOpen}>
      <DialogContent>
        {issued ? (
          <>
            <DialogHeader>
              <DialogTitle>Reset code for {account?.username}</DialogTitle>
              <DialogDescription>
                Share the code or link over a secure channel. It works once and is not shown again.
              </DialogDescription>
            </DialogHeader>
            <div className="grid gap-2">
              <Label htmlFor="reset-code-value">One-time code</Label>
              <div className="flex gap-2">
                <Input
                  id="reset-code-value"
                  readOnly
                  value={issued.code}
                  onFocus={(e) => e.currentTarget.select()}
                  className="font-mono text-xs"
                />
                <CopyButton value={issued.code} label="Copy reset code" copiedMessage="Reset code copied" />
              </div>
            </div>
            <div className="grid gap-2">
              <Label htmlFor="reset-code-link">Reset link</Label>
              <div className="flex gap-2">
                <Input
                  id="reset-code-link"
                  readOnly
                  value={link}
                  onFocus={(e) => e.currentTarget.select()}
                  className="font-mono text-xs"
                />
                <CopyButton value={link} label="Copy reset link" copiedMessage="Reset link copied" />
              </div>
            </div>
            <p className="text-xs text-muted-foreground">
              Expires{" "}
              <time dateTime={issued.expires_at} title={absoluteTime(issued.expires_at)}>
                {relativeTime(issued.expires_at)}
              </time>
              .
            </p>
            <DialogFooter>
              <DialogClose asChild>
                <Button>Done</Button>
              </DialogClose>
            </DialogFooter>
          </>
        ) : (
          <form onSubmit={onSubmit} noValidate className="flex flex-col gap-4">
            <DialogHeader>
              <DialogTitle>Issue a reset code for {account?.username}?</DialogTitle>
              <DialogDescription>
                They choose a new password themselves at the reset page. Their current password keeps working until
                then.
              </DialogDescription>
            </DialogHeader>
            <div className="grid gap-2">
              <Label htmlFor="reset-code-ttl">Valid for</Label>
              <Select value={ttl} onValueChange={setTtl}>
                <SelectTrigger id="reset-code-ttl" className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {TTL_OPTIONS.map((o) => (
                    <SelectItem key={o.hours} value={String(o.hours)}>
                      {o.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <FormError error={issue.error} />
            <DialogFooter>
              <DialogClose asChild>
                <Button type="button" variant="outline">
                  Cancel
                </Button>
              </DialogClose>
              <Button type="submit" disabled={issue.isPending}>
                {issue.isPending ? "Issuing…" : "Issue code"}
              </Button>
            </DialogFooter>
          </form>
        )}
      </DialogContent>
    </Dialog>
  );
}
