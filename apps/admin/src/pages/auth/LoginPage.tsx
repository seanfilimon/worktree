import { useMutation } from "@tanstack/react-query";
import { useState, type FormEvent } from "react";
import { Link, Navigate, useLocation, useNavigate } from "react-router";
import { errorMessage, isApiError } from "@/api/client";
import { useApi } from "@/api/context";
import { useHealth } from "@/api/queries";
import { useAuth } from "@/auth/context";
import { Button } from "@/components/ui/Button";
import { TextField } from "@/components/ui/Field";
import { Alert } from "@/components/ui/States";
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
  const allowRegistration = health.data?.allow_registration !== false;

  return (
    <AuthShell
      title="Sign in"
      footer={
        allowRegistration && (
          <>
            No account?{" "}
            <Link to="/register" className="font-medium text-accent hover:underline">
              Create one
            </Link>
          </>
        )
      }
    >
      <form onSubmit={onSubmit} className="flex flex-col gap-3" noValidate>
        {err && (
          <Alert title={isApiError(err) && err.status === 401 ? "Invalid username or password" : "Sign-in failed"}>
            {isApiError(err) && err.status === 401 ? null : errorMessage(err)}
          </Alert>
        )}
        <TextField
          label="Username"
          name="username"
          autoComplete="username"
          autoFocus
          required
          value={username}
          onChange={(e) => setUsername(e.target.value)}
        />
        <TextField
          label="Password"
          name="password"
          type="password"
          autoComplete="current-password"
          required
          value={password}
          onChange={(e) => setPassword(e.target.value)}
        />
        <Button type="submit" variant="primary" loading={login.isPending} disabled={!username.trim() || !password} className="mt-1 w-full">
          Sign in
        </Button>
      </form>
    </AuthShell>
  );
}
