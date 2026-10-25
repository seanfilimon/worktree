import { useMutation, useQueryClient } from "@tanstack/react-query";
import { KeyRound, Loader2, RefreshCw, ShieldCheck, ShieldOff } from "lucide-react";
import { useId, useState, type FormEvent } from "react";
import {
  securityKeys,
  useMfaStatus,
  useSecurityApi,
  type RecoveryCodes as Codes,
  type TotpEnrollment,
} from "@/api/security";
import { useAuth } from "@/auth/context";
import { CopyButton } from "@/components/people/CopyButton";
import { PasswordField } from "@/components/people/PasswordField";
import { FormError, QueryError } from "@/components/people/states";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardAction, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
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
import { Skeleton } from "@/components/ui/skeleton";
import { absoluteTime, relativeTime } from "@/lib/format";
import { useToast } from "@/toast/store";
import { normalizeTotp, TOTP_RE as CODE_RE } from "./codes";
import { QrCode } from "./QrCode";
import { RecoveryCodes } from "./RecoveryCodes";

/** Groups a base32 secret in blocks of four for manual entry. */
function groupSecret(secret: string): string {
  return secret.replace(/(.{4})(?=.)/g, "$1 ");
}

interface CodeFieldProps {
  value: string;
  onChange: (value: string) => void;
  error?: string;
  label?: string;
}

function CodeField({ value, onChange, error, label = "Authentication code" }: CodeFieldProps) {
  const id = useId();
  return (
    <div className="grid gap-2">
      <Label htmlFor={id}>{label}</Label>
      <Input
        id={id}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        inputMode="numeric"
        autoComplete="one-time-code"
        placeholder="123456"
        maxLength={10}
        className="font-mono tracking-widest sm:max-w-48"
        aria-invalid={error ? true : undefined}
        aria-describedby={error ? `${id}-error` : undefined}
      />
      {error && (
        <p id={`${id}-error`} className="text-sm font-medium">
          {error}
        </p>
      )}
    </div>
  );
}

/**
 * "Set up authenticator" button plus the enrollment dialog it opens. The
 * dialog stays mounted after the status flips to enabled (hiding the button)
 * so the recovery codes remain on screen.
 */
function Enroll({ showButton }: { showButton: boolean }) {
  const sec = useSecurityApi();
  const [open, setOpen] = useState(false);
  const { account } = useAuth();
  const queryClient = useQueryClient();
  const [code, setCode] = useState("");
  const [submitted, setSubmitted] = useState(false);
  const [codes, setCodes] = useState<string[] | null>(null);

  const enroll = useMutation<TotpEnrollment>({ mutationFn: sec.mfa.enroll, meta: { inlineError: true } });
  const confirm = useMutation<Codes, Error, string>({
    mutationFn: sec.mfa.confirm,
    meta: { inlineError: true },
    onSuccess: async (res) => {
      setCodes(res.recovery_codes);
      await queryClient.invalidateQueries({ queryKey: securityKeys.mfa });
    },
  });

  const start = () => {
    // A fresh secret each time; the server keeps it pending until confirmed.
    setOpen(true);
    enroll.mutate();
  };

  const changeOpen = (next: boolean) => {
    if (!next) {
      setCode("");
      setSubmitted(false);
      setCodes(null);
      enroll.reset();
      confirm.reset();
    }
    setOpen(next);
  };

  const normalized = normalizeTotp(code);
  const codeError = CODE_RE.test(normalized) ? undefined : "Enter the 6-digit code from your authenticator app.";

  const onSubmit = (e: FormEvent) => {
    e.preventDefault();
    setSubmitted(true);
    if (!codeError) confirm.mutate(normalized);
  };

  return (
    <>
      {showButton && (
        <CardAction>
          <Button size="sm" onClick={start}>
            <KeyRound aria-hidden="true" />
            Set up authenticator
          </Button>
        </CardAction>
      )}
      <Dialog open={open} onOpenChange={changeOpen}>
        <DialogContent className="sm:max-w-lg">
          {codes ? (
            <>
              <DialogHeader>
                <DialogTitle>Two-factor authentication enabled</DialogTitle>
                <DialogDescription>Save your recovery codes before you close this dialog.</DialogDescription>
              </DialogHeader>
              <RecoveryCodes codes={codes} username={account?.username} />
              <DialogFooter>
                <DialogClose asChild>
                  <Button>Done</Button>
                </DialogClose>
              </DialogFooter>
            </>
          ) : (
            <form onSubmit={onSubmit} noValidate className="flex flex-col gap-4">
              <DialogHeader>
                <DialogTitle>Set up an authenticator app</DialogTitle>
                <DialogDescription>
                  Scan the QR code with an authenticator app (1Password, Authy, Google Authenticator…), then enter the
                  code it shows.
                </DialogDescription>
              </DialogHeader>
              {enroll.isPending ? (
                <div role="status" aria-label="Generating secret" className="flex items-center gap-4">
                  <Skeleton className="size-44 rounded-md" />
                  <div className="flex flex-1 flex-col gap-2">
                    <Skeleton className="h-4 w-3/4" />
                    <Skeleton className="h-4 w-1/2" />
                  </div>
                </div>
              ) : enroll.isError ? (
                <FormError error={enroll.error} />
              ) : enroll.data ? (
                <div className="flex flex-col items-center gap-4 sm:flex-row sm:items-start">
                  <QrCode
                    value={enroll.data.otpauth_url}
                    label="QR code for your authenticator app"
                    className="shrink-0"
                  />
                  <div className="flex min-w-0 flex-col gap-2 text-sm">
                    <p className="font-medium">Can't scan it?</p>
                    <p className="text-muted-foreground">
                      Enter this secret manually (time-based, 6 digits, 30 seconds):
                    </p>
                    <code
                      aria-label="Secret key"
                      className="rounded-md border bg-muted/40 px-2 py-1.5 font-mono text-xs break-all"
                    >
                      {groupSecret(enroll.data.secret)}
                    </code>
                    <div>
                      <CopyButton value={enroll.data.secret} label="Copy secret" copiedMessage="Secret copied" />
                    </div>
                  </div>
                </div>
              ) : null}
              <CodeField
                value={code}
                onChange={setCode}
                label="Verification code"
                error={submitted ? codeError : undefined}
              />
              <FormError error={confirm.error} />
              <DialogFooter>
                <DialogClose asChild>
                  <Button type="button" variant="outline">
                    Cancel
                  </Button>
                </DialogClose>
                <Button type="submit" disabled={!enroll.isSuccess || confirm.isPending}>
                  {confirm.isPending && (
                    <Loader2 aria-hidden="true" className="animate-spin motion-reduce:animate-none" />
                  )}
                  Verify and enable
                </Button>
              </DialogFooter>
            </form>
          )}
        </DialogContent>
      </Dialog>
    </>
  );
}

function DisableDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
  const sec = useSecurityApi();
  const queryClient = useQueryClient();
  const toast = useToast();
  const id = useId();
  const [password, setPassword] = useState("");
  const [submitted, setSubmitted] = useState(false);

  const disable = useMutation({
    mutationFn: sec.mfa.disable,
    meta: { inlineError: true },
    onSuccess: async () => {
      toast.success("Two-factor authentication disabled");
      changeOpen(false);
      await queryClient.invalidateQueries({ queryKey: securityKeys.mfa });
    },
  });

  function changeOpen(next: boolean) {
    if (!next) {
      setPassword("");
      setSubmitted(false);
      disable.reset();
    }
    onOpenChange(next);
  }

  const onSubmit = (e: FormEvent) => {
    e.preventDefault();
    setSubmitted(true);
    if (password) disable.mutate(password);
  };

  return (
    <Dialog open={open} onOpenChange={changeOpen}>
      <DialogContent>
        <form onSubmit={onSubmit} noValidate className="flex flex-col gap-4">
          <DialogHeader>
            <DialogTitle>Disable two-factor authentication?</DialogTitle>
            <DialogDescription>
              Signing in will only need your password. Your recovery codes stop working. Confirm with your password.
            </DialogDescription>
          </DialogHeader>
          <div className="grid gap-2">
            <Label htmlFor={id}>Current password</Label>
            <PasswordField
              id={id}
              value={password}
              onChange={setPassword}
              autoComplete="current-password"
              aria-invalid={submitted && !password ? true : undefined}
              aria-describedby={submitted && !password ? `${id}-error` : undefined}
            />
            {submitted && !password && (
              <p id={`${id}-error`} className="text-sm font-medium">
                Enter your password.
              </p>
            )}
          </div>
          <FormError error={disable.error} />
          <DialogFooter>
            <DialogClose asChild>
              <Button type="button" variant="outline">
                Cancel
              </Button>
            </DialogClose>
            <Button type="submit" disabled={disable.isPending}>
              {disable.isPending ? "Disabling…" : "Disable"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

function RegenerateDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
  const sec = useSecurityApi();
  const { account } = useAuth();
  const queryClient = useQueryClient();
  const [code, setCode] = useState("");
  const [submitted, setSubmitted] = useState(false);

  const regenerate = useMutation<Codes, Error, string>({
    mutationFn: sec.mfa.regenerateRecoveryCodes,
    meta: { inlineError: true },
    onSuccess: () => queryClient.invalidateQueries({ queryKey: securityKeys.mfa }),
  });

  const changeOpen = (next: boolean) => {
    if (!next) {
      setCode("");
      setSubmitted(false);
      regenerate.reset();
    }
    onOpenChange(next);
  };

  const normalized = normalizeTotp(code);
  const codeError = CODE_RE.test(normalized) ? undefined : "Enter the 6-digit code from your authenticator app.";

  const onSubmit = (e: FormEvent) => {
    e.preventDefault();
    setSubmitted(true);
    if (!codeError) regenerate.mutate(normalized);
  };

  return (
    <Dialog open={open} onOpenChange={changeOpen}>
      <DialogContent>
        {regenerate.data ? (
          <>
            <DialogHeader>
              <DialogTitle>New recovery codes</DialogTitle>
              <DialogDescription>Your previous recovery codes no longer work.</DialogDescription>
            </DialogHeader>
            <RecoveryCodes codes={regenerate.data.recovery_codes} username={account?.username} />
            <DialogFooter>
              <DialogClose asChild>
                <Button>Done</Button>
              </DialogClose>
            </DialogFooter>
          </>
        ) : (
          <form onSubmit={onSubmit} noValidate className="flex flex-col gap-4">
            <DialogHeader>
              <DialogTitle>Regenerate recovery codes</DialogTitle>
              <DialogDescription>
                This replaces every unused recovery code. Confirm with a code from your authenticator app.
              </DialogDescription>
            </DialogHeader>
            <CodeField value={code} onChange={setCode} error={submitted ? codeError : undefined} />
            <FormError error={regenerate.error} />
            <DialogFooter>
              <DialogClose asChild>
                <Button type="button" variant="outline">
                  Cancel
                </Button>
              </DialogClose>
              <Button type="submit" disabled={regenerate.isPending}>
                {regenerate.isPending ? "Generating…" : "Generate new codes"}
              </Button>
            </DialogFooter>
          </form>
        )}
      </DialogContent>
    </Dialog>
  );
}

/** Settings → Security: TOTP two-factor authentication. */
export function MfaCard() {
  const status = useMfaStatus();
  const [dialog, setDialog] = useState<"disable" | "regenerate" | null>(null);
  const onOpenChange = (open: boolean) => !open && setDialog(null);
  const enabled = status.data?.totp_enabled ?? false;

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          Two-factor authentication
          {status.isSuccess && (
            <Badge variant={enabled ? "default" : "outline"}>
              {enabled ? <ShieldCheck aria-hidden="true" /> : <ShieldOff aria-hidden="true" />}
              {enabled ? "Enabled" : "Off"}
            </Badge>
          )}
        </CardTitle>
        <CardDescription>
          Require a code from an authenticator app (TOTP) in addition to your password when you sign in.
        </CardDescription>
        <Enroll showButton={status.isSuccess && !enabled} />
      </CardHeader>
      <CardContent className="text-sm">
        {status.isPending ? (
          <div role="status" aria-label="Loading two-factor status" className="flex flex-col gap-2">
            <Skeleton className="h-4 w-1/2" />
            <Skeleton className="h-4 w-1/3" />
          </div>
        ) : status.isError ? (
          <QueryError
            error={status.error}
            onRetry={() => void status.refetch()}
            title="Could not load two-factor status"
          />
        ) : enabled ? (
          <div className="flex flex-col gap-4">
            <dl className="grid grid-cols-[auto_1fr] gap-x-6 gap-y-1">
              {status.data.enabled_at && (
                <>
                  <dt className="text-muted-foreground">Enabled</dt>
                  <dd>
                    <time dateTime={status.data.enabled_at} title={absoluteTime(status.data.enabled_at)}>
                      {relativeTime(status.data.enabled_at)}
                    </time>
                  </dd>
                </>
              )}
              <dt className="text-muted-foreground">Recovery codes left</dt>
              <dd className={status.data.recovery_codes_left <= 2 ? "font-semibold" : undefined}>
                {status.data.recovery_codes_left}
                {status.data.recovery_codes_left <= 2 && " — regenerate them soon"}
              </dd>
            </dl>
            <div className="flex flex-wrap gap-2">
              <Button variant="outline" size="sm" onClick={() => setDialog("regenerate")}>
                <RefreshCw aria-hidden="true" />
                Regenerate recovery codes
              </Button>
              <Button variant="outline" size="sm" onClick={() => setDialog("disable")}>
                <ShieldOff aria-hidden="true" />
                Disable two-factor
              </Button>
            </div>
          </div>
        ) : (
          <p className="text-muted-foreground">
            Two-factor authentication is off. Anyone with your password can sign in.
          </p>
        )}
      </CardContent>
      <DisableDialog open={dialog === "disable"} onOpenChange={onOpenChange} />
      <RegenerateDialog open={dialog === "regenerate"} onOpenChange={onOpenChange} />
    </Card>
  );
}
