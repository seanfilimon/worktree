import { useMutation } from "@tanstack/react-query";
import { CircleAlert, Loader2, LogIn } from "lucide-react";
import { useState, type FormEvent } from "react";
import { Link, Navigate, useLocation, useNavigate } from "react-router";
import { errorMessage, isApiError } from "@/api/client";
import { useHealth } from "@/api/queries";
import { isMfaChallenge, useAuthProviders, useSecurityApi, type MfaChallenge } from "@/api/security";
import type { AuthResponse } from "@/api/types";
import { safeRedirect, useAuth } from "@/auth/context";
import { appPath } from "@/auth/ssoHash";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Separator } from "@/components/ui/separator";
import { AuthField } from "./AuthField";
import { AuthShell } from "./AuthShell";
import { MfaStep } from "./MfaStep";

export function LoginPage() {
  const sec = useSecurityApi();
  const { token, signIn, ssoError, clearSsoError } = useAuth();
  const navigate = useNavigate();
  const location = useLocation();
  const health = useHealth();
  const providers = useAuthProviders();
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [challenge, setChallenge] = useState<MfaChallenge | null>(null);
  const [finishing, setFinishing] = useState(false);
  const from = safeRedirect((location.state as { from?: unknown } | null)?.from);

  const finish = (res: AuthResponse) => {
    setFinishing(true);
    signIn(res);
    void navigate(from, { replace: true });
  };

  const login = useMutation({
    mutationFn: sec.login,
    meta: { inlineError: true },
    onSuccess: (res) => {
      if (isMfaChallenge(res)) {
        setChallenge(res);
        setPassword("");
      } else {
        finish(res);
      }
    },
  });

  if (token && !finishing && !login.isPending) return <Navigate to={from} replace />;

  const onSubmit = (e: FormEvent) => {
    e.preventDefault();
    clearSsoError();
    login.mutate({ username: username.trim(), password });
  };

  const err = login.error;
  const badCredentials = isApiError(err) && err.status === 401;
  const allowRegistration = providers.data ? providers.data.registration : health.data?.allow_registration !== false;
  const sso = providers.data?.sso.enabled ? providers.data.sso : null;
  const passwordLogin = providers.data?.password !== false;
  const registerLink = (
    <Link to="/register" state={{ from }} className="font-medium text-foreground underline-offset-4 hover:underline">
      Create one
    </Link>
  );

  if (challenge) {
    return (
      <AuthShell title="Two-factor authentication" description="One more step to confirm it's you.">
        <MfaStep
          challenge={challenge}
          onSuccess={finish}
          onCancel={() => {
            setChallenge(null);
            login.reset();
          }}
        />
      </AuthShell>
    );
  }

  return (
    <AuthShell
      title="Sign in"
      description="Use your W0rkTree account to manage this server."
      footer={allowRegistration && <p>No account? {registerLink}</p>}
    >
      <div className="grid gap-4">
        {ssoError && (
          <Alert>
            <CircleAlert aria-hidden="true" />
            <AlertTitle>Single sign-on failed</AlertTitle>
            <AlertDescription>{ssoError}</AlertDescription>
          </Alert>
        )}
        {sso && (
          <>
            <Button asChild variant="outline" className="w-full">
              <a href={sec.ssoStartUrl(appPath(from))}>
                <LogIn aria-hidden="true" />
                Sign in with {sso.name || "single sign-on"}
              </a>
            </Button>
            {passwordLogin && (
              <div className="flex items-center gap-3 text-xs text-muted-foreground" aria-hidden="true">
                <Separator className="flex-1" />
                or
                <Separator className="flex-1" />
              </div>
            )}
          </>
        )}
        {passwordLogin && (
          <form onSubmit={onSubmit} className="grid gap-4" noValidate>
            {err && (
              <Alert>
                <CircleAlert aria-hidden="true" />
                <AlertTitle>{badCredentials ? "Invalid username or password" : "Sign-in failed"}</AlertTitle>
                <AlertDescription>
                  {badCredentials ? "Check your credentials and try again." : errorMessage(err)}
                </AlertDescription>
              </Alert>
            )}
            <AuthField
              label="Username"
              name="username"
              autoComplete="username"
              autoCapitalize="none"
              spellCheck={false}
              autoFocus
              required
              value={username}
              onChange={(e) => setUsername(e.target.value)}
            />
            <AuthField
              label="Password"
              name="password"
              type="password"
              autoComplete="current-password"
              required
              value={password}
              onChange={(e) => setPassword(e.target.value)}
            />
            <Button type="submit" disabled={login.isPending || !username.trim() || !password} className="w-full">
              {login.isPending && <Loader2 aria-hidden="true" className="animate-spin motion-reduce:animate-none" />}
              Sign in
            </Button>
          </form>
        )}
      </div>
    </AuthShell>
  );
}
