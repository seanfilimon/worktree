import { useMutation } from "@tanstack/react-query";
import { CircleAlert, Loader2 } from "lucide-react";
import { useState, type FormEvent } from "react";
import { Link, Navigate, useLocation, useNavigate } from "react-router";
import { errorMessage, isApiError } from "@/api/client";
import { useApi } from "@/api/context";
import { safeRedirect, useAuth } from "@/auth/context";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { AuthField } from "./AuthField";
import { AuthShell } from "./AuthShell";

const USERNAME_RE = /^[a-z0-9][a-z0-9-]{0,38}$/;
const MIN_PASSWORD = 8;

export function RegisterPage() {
  const api = useApi();
  const { token, signIn } = useAuth();
  const navigate = useNavigate();
  const location = useLocation();
  const from = safeRedirect((location.state as { from?: unknown } | null)?.from);
  const [username, setUsername] = useState("");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [touched, setTouched] = useState(false);

  const register = useMutation({
    mutationFn: api.auth.register,
    meta: { inlineError: true },
    onSuccess: (res) => {
      signIn(res);
      void navigate(from, { replace: true });
    },
  });

  if (token && !register.isPending && !register.isSuccess) return <Navigate to={from} replace />;

  const errors = {
    username: USERNAME_RE.test(username)
      ? undefined
      : "Lowercase letters, digits and dashes; must start with a letter or digit.",
    email: /^[^\s@]+@[^\s@]+$/.test(email.trim()) ? undefined : "Enter a valid email address.",
    password: password.length >= MIN_PASSWORD ? undefined : `At least ${MIN_PASSWORD} characters.`,
  };
  const valid = !errors.username && !errors.email && !errors.password;

  const onSubmit = (e: FormEvent) => {
    e.preventDefault();
    setTouched(true);
    if (valid) register.mutate({ username, email: email.trim(), password });
  };

  const err = register.error;
  const disabled = isApiError(err) && err.status === 403;
  return (
    <AuthShell
      title="Create account"
      description="Register a new account on this W0rkTree server."
      footer={
        <p>
          Already registered?{" "}
          <Link to="/login" state={{ from }} className="font-medium text-foreground underline-offset-4 hover:underline">
            Sign in
          </Link>
        </p>
      }
    >
      <form onSubmit={onSubmit} className="grid gap-4" noValidate>
        {err && (
          <Alert>
            <CircleAlert aria-hidden="true" />
            <AlertTitle>{disabled ? "Registration is disabled on this server" : "Registration failed"}</AlertTitle>
            <AlertDescription>
              {disabled ? "Ask a server administrator to create an account for you." : errorMessage(err)}
            </AlertDescription>
          </Alert>
        )}
        <AuthField
          label="Username"
          autoComplete="username"
          autoCapitalize="none"
          spellCheck={false}
          autoFocus
          value={username}
          onChange={(e) => setUsername(e.target.value.toLowerCase())}
          error={touched ? errors.username : undefined}
        />
        <AuthField
          label="Email"
          type="email"
          autoComplete="email"
          value={email}
          onChange={(e) => setEmail(e.target.value)}
          error={touched ? errors.email : undefined}
        />
        <AuthField
          label="Password"
          type="password"
          autoComplete="new-password"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          hint={`At least ${MIN_PASSWORD} characters.`}
          error={touched ? errors.password : undefined}
        />
        <Button type="submit" disabled={register.isPending} className="w-full">
          {register.isPending && <Loader2 aria-hidden="true" className="animate-spin motion-reduce:animate-none" />}
          Create account
        </Button>
      </form>
    </AuthShell>
  );
}
