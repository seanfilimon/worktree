import { useMutation } from "@tanstack/react-query";
import { useState, type FormEvent } from "react";
import { Link, Navigate, useNavigate } from "react-router";
import { errorMessage, isApiError } from "@/api/client";
import { useApi } from "@/api/context";
import { useAuth } from "@/auth/context";
import { Button } from "@/components/ui/Button";
import { TextField } from "@/components/ui/Field";
import { Alert } from "@/components/ui/States";
import { AuthShell } from "./AuthShell";

const USERNAME_RE = /^[a-z0-9][a-z0-9-]{0,38}$/;
const MIN_PASSWORD = 8;

export function RegisterPage() {
  const api = useApi();
  const { token, signIn } = useAuth();
  const navigate = useNavigate();
  const [username, setUsername] = useState("");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [touched, setTouched] = useState(false);

  const register = useMutation({
    mutationFn: api.auth.register,
    meta: { inlineError: true },
    onSuccess: (res) => {
      signIn(res);
      void navigate("/", { replace: true });
    },
  });

  if (token && !register.isPending && !register.isSuccess) return <Navigate to="/" replace />;

  const errors = {
    username: USERNAME_RE.test(username) ? undefined : "Lowercase letters, digits and dashes; must start with a letter or digit.",
    email: /^[^\s@]+@[^\s@]+$/.test(email) ? undefined : "Enter a valid email address.",
    password: password.length >= MIN_PASSWORD ? undefined : `At least ${MIN_PASSWORD} characters.`,
  };
  const valid = !errors.username && !errors.email && !errors.password;

  const onSubmit = (e: FormEvent) => {
    e.preventDefault();
    setTouched(true);
    if (valid) register.mutate({ username, email: email.trim(), password });
  };

  const err = register.error;
  return (
    <AuthShell
      title="Create account"
      footer={
        <>
          Already registered?{" "}
          <Link to="/login" className="font-medium text-accent hover:underline">
            Sign in
          </Link>
        </>
      }
    >
      <form onSubmit={onSubmit} className="flex flex-col gap-3" noValidate>
        {err && (
          <Alert title={isApiError(err) && err.status === 403 ? "Registration is disabled on this server" : "Registration failed"}>
            {isApiError(err) && err.status === 403 ? "Ask a server administrator to create an account for you." : errorMessage(err)}
          </Alert>
        )}
        <TextField
          label="Username"
          autoComplete="username"
          autoFocus
          value={username}
          onChange={(e) => setUsername(e.target.value.toLowerCase())}
          error={touched ? errors.username : undefined}
        />
        <TextField
          label="Email"
          type="email"
          autoComplete="email"
          value={email}
          onChange={(e) => setEmail(e.target.value)}
          error={touched ? errors.email : undefined}
        />
        <TextField
          label="Password"
          type="password"
          autoComplete="new-password"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          hint={touched ? undefined : `At least ${MIN_PASSWORD} characters.`}
          error={touched ? errors.password : undefined}
        />
        <Button type="submit" variant="primary" loading={register.isPending} className="mt-1 w-full">
          Create account
        </Button>
      </form>
    </AuthShell>
  );
}
