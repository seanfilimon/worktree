import { useMutation } from "@tanstack/react-query";
import { useId, useState, type FormEvent } from "react";
import { useApi } from "@/api/context";
import type { Account } from "@/api/types";
import { Button } from "@/components/ui/button";
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { useToast } from "@/toast/store";
import { PasswordField } from "./PasswordField";
import { FormError } from "./states";
import { MIN_PASSWORD_LENGTH } from "./validation";

interface ResetPasswordDialogProps {
  /** Account to reset; null closes the dialog. */
  account: Account | null;
  onClose: () => void;
}

export function ResetPasswordDialog({ account, onClose }: ResetPasswordDialogProps) {
  const api = useApi();
  const toast = useToast();
  const id = useId();
  const [password, setPassword] = useState("");
  const [submitted, setSubmitted] = useState(false);
  const tooShort = password.length < MIN_PASSWORD_LENGTH;

  const reset = useMutation({
    mutationFn: (v: { username: string; password: string }) => api.admin.setPassword(v.username, v.password),
    meta: { inlineError: true },
    onSuccess: (_r, v) => {
      toast.success("Password reset", `${v.username}'s sessions and tokens were revoked.`);
      close();
    },
  });

  function close() {
    setPassword("");
    setSubmitted(false);
    reset.reset();
    onClose();
  }

  const onSubmit = (e: FormEvent) => {
    e.preventDefault();
    setSubmitted(true);
    if (account && !tooShort) reset.mutate({ username: account.username, password });
  };

  return (
    <Dialog open={account !== null} onOpenChange={(open) => !open && close()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Reset password</DialogTitle>
          <DialogDescription>
            Set a new password for <span className="font-mono text-foreground">{account?.username}</span>. This signs them out everywhere and
            revokes their API tokens.
          </DialogDescription>
        </DialogHeader>
        <form onSubmit={onSubmit} className="flex flex-col gap-4" noValidate>
          <div className="grid gap-2">
            <Label htmlFor={id}>New password</Label>
            <PasswordField
              id={id}
              generate
              meter
              autoComplete="new-password"
              value={password}
              onChange={setPassword}
              aria-invalid={submitted && tooShort ? true : undefined}
              aria-describedby={submitted && tooShort ? `${id}-error` : undefined}
            />
            {submitted && tooShort && (
              <p id={`${id}-error`} className="text-sm font-medium">
                At least {MIN_PASSWORD_LENGTH} characters.
              </p>
            )}
          </div>
          <FormError error={reset.error} />
          <DialogFooter>
            <DialogClose asChild>
              <Button type="button" variant="outline">
                Cancel
              </Button>
            </DialogClose>
            <Button type="submit" disabled={reset.isPending}>
              {reset.isPending ? "Saving…" : "Reset password"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
