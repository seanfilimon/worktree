import { useMutation } from "@tanstack/react-query";
import { CircleAlert, CircleCheck, Loader2 } from "lucide-react";
import { useId, useState, type FormEvent } from "react";
import { Link, useSearchParams } from "react-router";
import { errorMessage, isApiError } from "@/api/client";
import { useSecurityApi } from "@/api/security";
import { useAuth } from "@/auth/context";
import { PasswordField } from "@/components/people/PasswordField";
import { MIN_PASSWORD_LENGTH } from "@/components/people/validation";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { AuthField } from "./AuthField";
import { AuthShell } from "./AuthShell";

const CODE_RE = /^wtr_\S+$/;

/** Public page for `/reset-password?code=wtr_…` (§12.2 admin-issued codes). */
export function ResetPasswordPage() {
  const sec = useSecurityApi();
  const { token } = useAuth();
  const [params] = useSearchParams();
  const nextId = useId();
  const confirmId = useId();
  const ids = { next: nextId, confirm: confirmId };
  const [code, setCode] = useState(() => params.get("code") ?? "");
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [touched, setTouched] = useState(false);

  const reset = useMutation({ mutationFn: sec.passwordReset, meta: { inlineError: true } });

  const errors = {
    code: CODE_RE.test(code.trim()) ? undefined : "Paste the reset code you were given (starts with wtr_).",
    password: password.length >= MIN_PASSWORD_LENGTH ? undefined : `At least ${MIN_PASSWORD_LENGTH} characters.`,
    confirm: confirm === password ? undefined : "Passwords do not match.",
  };
  const valid = !errors.code && !errors.password && !errors.confirm;

  const onSubmit = (e: FormEvent) => {
    e.preventDefault();
    setTouched(true);
    if (valid) reset.mutate({ code: code.trim(), new_password: password });
  };

  if (reset.isSuccess) {
    return (
      <AuthShell title="Password updated" description="Your new password is set.">
        <div className="grid gap-4">
          <Alert>
            <CircleCheck aria-hidden="true" />
            <AlertTitle>Password reset</AlertTitle>
            <AlertDescription>
              {token ? "Your existing sessions were signed out." : "Sign in with your new password."}
            </AlertDescription>
          </Alert>
          <Button asChild className="w-full">
            <Link to="/login">Go to sign in</Link>
          </Button>
        </div>
      </AuthShell>
    );
  }

  const err = reset.error;
  const rejected =
    isApiError(err) && (err.status === 400 || err.status === 401 || err.status === 404 || err.status === 410);
  const fieldError = (msg: string | undefined) => (touched ? msg : undefined);

  return (
    <AuthShell
      title="Reset password"
      description="Use the one-time code from your server administrator to choose a new password."
      footer={
        <p>
          Remembered it?{" "}
          <Link to="/login" className="font-medium text-foreground underline-offset-4 hover:underline">
            Sign in
          </Link>
        </p>
      }
    >
      <form onSubmit={onSubmit} className="grid gap-4" noValidate aria-label="Reset password">
        {err && (
          <Alert>
            <CircleAlert aria-hidden="true" />
            <AlertTitle>{rejected ? "This reset code is invalid or has expired" : "Reset failed"}</AlertTitle>
            <AlertDescription>
              {rejected ? "Ask your administrator for a new code." : errorMessage(err)}
            </AlertDescription>
          </Alert>
        )}
        <AuthField
          label="Reset code"
          name="code"
          autoComplete="off"
          autoCapitalize="none"
          spellCheck={false}
          className="font-mono"
          value={code}
          onChange={(e) => setCode(e.target.value)}
          error={fieldError(errors.code)}
        />
        <div className="grid gap-2">
          <Label htmlFor={ids.next}>New password</Label>
          <PasswordField
            id={ids.next}
            value={password}
            onChange={setPassword}
            autoComplete="new-password"
            meter
            aria-invalid={fieldError(errors.password) ? true : undefined}
            aria-describedby={`${ids.next}-note`}
          />
          <p
            id={`${ids.next}-note`}
            className={fieldError(errors.password) ? "text-xs font-medium" : "text-xs text-muted-foreground"}
          >
            {fieldError(errors.password) ?? `At least ${MIN_PASSWORD_LENGTH} characters.`}
          </p>
        </div>
        <div className="grid gap-2">
          <Label htmlFor={ids.confirm}>Confirm new password</Label>
          <PasswordField
            id={ids.confirm}
            value={confirm}
            onChange={setConfirm}
            autoComplete="new-password"
            aria-invalid={fieldError(errors.confirm) ? true : undefined}
            aria-describedby={fieldError(errors.confirm) ? `${ids.confirm}-note` : undefined}
          />
          {fieldError(errors.confirm) && (
            <p id={`${ids.confirm}-note`} className="text-xs font-medium">
              {errors.confirm}
            </p>
          )}
        </div>
        <Button type="submit" disabled={reset.isPending} className="w-full">
          {reset.isPending && <Loader2 aria-hidden="true" className="animate-spin motion-reduce:animate-none" />}
          Set new password
        </Button>
      </form>
    </AuthShell>
  );
}
