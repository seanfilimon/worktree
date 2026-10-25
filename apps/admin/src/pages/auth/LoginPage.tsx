import { useMutation } from "@tanstack/react-query";
import { CircleAlert, Loader2 } from "lucide-react";
import { useState, type FormEvent } from "react";
import { Link, Navigate, useLocation, useNavigate } from "react-router";
import { errorMessage, isApiError } from "@/api/client";
import { useApi } from "@/api/context";
import { useHealth } from "@/api/queries";
import { useAuth } from "@/auth/context";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { AuthField } from "./AuthField";
import { AuthShell } from "./AuthShell";

/** Only follow same-app relative redirects. */
function safeRedirect(from: unknown): string {
  return typeof from === "string" && from.startsWith("/") && !from.startsWith("//") ? from : "/";
}

export function LoginPage() {
  const api = useApi();
  const { token, signIn } = useAuth();
  const navigate = useNavigate();
  const location = useLocation();
  const health = useHealth();
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const from = safeRedirect((location.state as { from?: unknown } | null)?.from);

  const login = useMutation({
    mutationFn: api.auth.login,
    meta: { inlineError: true },
    onSuccess: (res) => {
      signIn(res);
      void navigate(from, { replace: true });
    },
  });

  if (token && !login.isPending && !login.isSuccess) return <Navigate to={from} replace />;

  const onSubmit = (e: FormEvent) => {
    e.preventDefault();
    login.mutate({ username: username.trim(), password });
  };

  const err = login.error;
  const badCredentials = isApiError(err) && err.status === 401;
  const allowRegistration = health.data?.allow_registration !== false;

  return (
    <AuthShell
      title="Sign in"
      description="Use your W0rkTree account to manage this server."
      footer={
        allowRegistration && (
          <p>
            No account?{" "}
            <Link to="/register" className="font-medium text-foreground underline-offset-4 hover:underline">
              Create one
            </Link>
          </p>
        )
      }
    >
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
    </AuthShell>
  );
}
