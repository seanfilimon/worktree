import { useMutation } from "@tanstack/react-query";
import { ArrowLeft, CircleAlert, Loader2 } from "lucide-react";
import { useState, type FormEvent } from "react";
import { errorMessage, isApiError } from "@/api/client";
import { useSecurityApi, type MfaChallenge } from "@/api/security";
import type { AuthResponse } from "@/api/types";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { normalizeRecoveryCode, normalizeTotp, RECOVERY_RE, TOTP_RE } from "@/pages/security/codes";
import { AuthField } from "./AuthField";

interface MfaStepProps {
  challenge: MfaChallenge;
  onSuccess: (res: AuthResponse) => void;
  /** Back to the username / password step. */
  onCancel: () => void;
}

/** Second login step (§12.2): a TOTP code or a single-use recovery code. */
export function MfaStep({ challenge, onSuccess, onCancel }: MfaStepProps) {
  const sec = useSecurityApi();
  const [mode, setMode] = useState<"totp" | "recovery">("totp");
  const [code, setCode] = useState("");
  const [touched, setTouched] = useState(false);

  const verify = useMutation({
    mutationFn: sec.loginMfa,
    meta: { inlineError: true },
    onSuccess,
  });

  const normalized = mode === "totp" ? normalizeTotp(code) : normalizeRecoveryCode(code);
  const formatError =
    mode === "totp"
      ? TOTP_RE.test(normalized)
        ? undefined
        : "Enter the 6-digit code from your authenticator app."
      : RECOVERY_RE.test(normalized)
        ? undefined
        : "Recovery codes look like abcd-1234.";

  const onSubmit = (e: FormEvent) => {
    e.preventDefault();
    setTouched(true);
    if (!formatError) verify.mutate({ mfa_token: challenge.mfa_token, code: normalized });
  };

  const switchMode = () => {
    setMode((m) => (m === "totp" ? "recovery" : "totp"));
    setCode("");
    setTouched(false);
    verify.reset();
  };

  const err = verify.error;
  const rejected = isApiError(err) && (err.status === 401 || err.status === 400);

  return (
    <form onSubmit={onSubmit} className="grid gap-4" noValidate aria-label="Two-factor verification">
      <p className="text-sm text-muted-foreground">
        Signing in as <span className="font-medium text-foreground">{challenge.account.username}</span>.{" "}
        {mode === "totp"
          ? "Open your authenticator app and enter the current code."
          : "Enter one of the recovery codes you saved when you set up two-factor authentication."}
      </p>
      {err && (
        <Alert>
          <CircleAlert aria-hidden="true" />
          <AlertTitle>{rejected ? "That code did not work" : "Verification failed"}</AlertTitle>
          <AlertDescription>
            {rejected
              ? "Check the code and try again. If this keeps happening, go back and sign in again."
              : errorMessage(err)}
          </AlertDescription>
        </Alert>
      )}
      {mode === "totp" ? (
        <AuthField
          key="totp"
          label="Authentication code"
          name="code"
          inputMode="numeric"
          autoComplete="one-time-code"
          placeholder="123456"
          maxLength={10}
          autoFocus
          className="font-mono tracking-widest"
          value={code}
          onChange={(e) => setCode(e.target.value)}
          error={touched ? formatError : undefined}
        />
      ) : (
        <AuthField
          key="recovery"
          label="Recovery code"
          name="recovery-code"
          autoComplete="off"
          autoCapitalize="none"
          spellCheck={false}
          placeholder="abcd-1234"
          maxLength={12}
          autoFocus
          className="font-mono"
          value={code}
          onChange={(e) => setCode(e.target.value)}
          hint="Each recovery code works only once."
          error={touched ? formatError : undefined}
        />
      )}
      <Button type="submit" disabled={verify.isPending || !code.trim()} className="w-full">
        {verify.isPending && <Loader2 aria-hidden="true" className="animate-spin motion-reduce:animate-none" />}
        Verify
      </Button>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <Button type="button" variant="ghost" size="sm" onClick={onCancel}>
          <ArrowLeft aria-hidden="true" />
          Back
        </Button>
        <Button type="button" variant="link" size="sm" onClick={switchMode}>
          {mode === "totp" ? "Use a recovery code" : "Use an authenticator code"}
        </Button>
      </div>
    </form>
  );
}
