import { useMutation, useQueryClient } from "@tanstack/react-query";
import { KeyRound, Loader2 } from "lucide-react";
import { useId, useState, type FormEvent, type ReactNode } from "react";
import { isApiError } from "@/api/client";
import { securityKeys, useSecurityApi, useSsoConfig, type SsoConfig, type SsoConfigUpdate } from "@/api/security";
import { RequireAdmin } from "@/auth/RequireAuth";
import { useAuth } from "@/auth/context";
import { PageHeader } from "@/components/PageHeader";
import { FormError, QueryError } from "@/components/people/states";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { absoluteTime, relativeTime } from "@/lib/format";
import { useToast } from "@/toast/store";

const DEFAULT_SCOPES = ["openid", "email", "profile"];
const DEFAULT_USERNAME_CLAIM = "preferred_username";

function defaultRedirectUrl(): string {
  return `${window.location.origin}/api/v1/auth/sso/callback`;
}

const EMPTY: SsoConfig = {
  enabled: false,
  name: "",
  issuer: "",
  client_id: "",
  has_client_secret: false,
  redirect_url: "",
  scopes: DEFAULT_SCOPES,
  allowed_domains: [],
  auto_create: false,
  username_claim: DEFAULT_USERNAME_CLAIM,
};

/** "a, b c\nd" → ["a", "b", "c", "d"]. */
function splitList(value: string): string[] {
  return value
    .split(/[\s,]+/)
    .map((s) => s.trim())
    .filter(Boolean);
}

function isUrl(value: string): boolean {
  try {
    const u = new URL(value);
    return u.protocol === "https:" || u.protocol === "http:";
  } catch {
    return false;
  }
}

interface FieldProps {
  label: string;
  hint?: ReactNode;
  error?: string;
  children: (props: { id: string; "aria-invalid"?: true; "aria-describedby"?: string }) => ReactNode;
  badge?: ReactNode;
}

function Field({ label, hint, error, children, badge }: FieldProps) {
  const id = useId();
  const note = error ?? hint;
  return (
    <div className="grid gap-2">
      <div className="flex items-center gap-2">
        <Label htmlFor={id}>{label}</Label>
        {badge}
      </div>
      {children({ id, "aria-invalid": error ? true : undefined, "aria-describedby": note ? `${id}-note` : undefined })}
      {note && (
        <p id={`${id}-note`} className={error ? "text-xs font-medium" : "text-xs text-muted-foreground"}>
          {note}
        </p>
      )}
    </div>
  );
}

function SsoForm({ config }: { config: SsoConfig }) {
  const sec = useSecurityApi();
  const queryClient = useQueryClient();
  const toast = useToast();
  const [enabled, setEnabled] = useState(config.enabled);
  const [name, setName] = useState(config.name);
  const [issuer, setIssuer] = useState(config.issuer);
  const [clientId, setClientId] = useState(config.client_id);
  // Write-only: never pre-filled; an empty field keeps the stored secret.
  const [secret, setSecret] = useState("");
  const [hasSecret, setHasSecret] = useState(config.has_client_secret);
  const [redirectUrl, setRedirectUrl] = useState(config.redirect_url || defaultRedirectUrl());
  const [scopes, setScopes] = useState((config.scopes?.length ? config.scopes : DEFAULT_SCOPES).join(" "));
  const [domains, setDomains] = useState((config.allowed_domains ?? []).join(", "));
  const [autoCreate, setAutoCreate] = useState(config.auto_create);
  const [usernameClaim, setUsernameClaim] = useState(config.username_claim || DEFAULT_USERNAME_CLAIM);
  const [submitted, setSubmitted] = useState(false);
  const enabledId = useId();
  const autoCreateId = useId();

  const save = useMutation({
    mutationFn: sec.admin.saveSso,
    meta: { inlineError: true },
    onSuccess: async (saved) => {
      queryClient.setQueryData(securityKeys.sso, saved);
      setSecret("");
      setHasSecret(saved.has_client_secret);
      toast.success(
        "Single sign-on settings saved",
        saved.enabled ? `Sign in with ${saved.name} is live.` : "SSO is off.",
      );
      await queryClient.invalidateQueries({ queryKey: securityKeys.providers });
    },
  });

  const scopeList = splitList(scopes);
  const errors = {
    name: enabled && !name.trim() ? "Shown on the sign-in button, e.g. “Okta”." : undefined,
    issuer:
      issuer.trim() && !isUrl(issuer.trim())
        ? "Enter the issuer URL, e.g. https://example.okta.com."
        : enabled && !issuer.trim()
          ? "Required when SSO is enabled."
          : undefined,
    clientId: enabled && !clientId.trim() ? "Required when SSO is enabled." : undefined,
    redirectUrl: !isUrl(redirectUrl.trim()) ? "Enter an absolute URL." : undefined,
    scopes: !scopeList.includes("openid") ? "Must include openid." : undefined,
    usernameClaim: !usernameClaim.trim() ? "Enter a claim name." : undefined,
  };
  const valid = Object.values(errors).every((e) => e === undefined);
  const show = (e: string | undefined) => (submitted ? e : undefined);

  const onSubmit = (e: FormEvent) => {
    e.preventDefault();
    setSubmitted(true);
    if (!valid) return;
    const body: SsoConfigUpdate = {
      enabled,
      name: name.trim(),
      issuer: issuer.trim().replace(/\/+$/, ""),
      client_id: clientId.trim(),
      redirect_url: redirectUrl.trim(),
      scopes: scopeList,
      allowed_domains: splitList(domains).map((d) => d.replace(/^@/, "").toLowerCase()),
      auto_create: autoCreate,
      username_claim: usernameClaim.trim(),
      ...(secret ? { client_secret: secret } : {}),
    };
    save.mutate(body);
  };

  return (
    <form onSubmit={onSubmit} noValidate aria-label="Single sign-on settings" className="flex flex-col gap-4">
      <Card>
        <CardHeader>
          <CardTitle>OpenID Connect provider</CardTitle>
          <CardDescription>
            One provider (Okta, Entra ID, Google Workspace, Keycloak…). Register this server as a web application and
            use the redirect URL below.
          </CardDescription>
        </CardHeader>
        <CardContent className="grid gap-5">
          <div className="flex items-start justify-between gap-4 rounded-md border p-3">
            <div className="grid gap-0.5">
              <Label htmlFor={enabledId}>Enable single sign-on</Label>
              <p className="text-xs text-muted-foreground">Adds a “Sign in with …” button to the sign-in page.</p>
            </div>
            <Switch id={enabledId} checked={enabled} onCheckedChange={setEnabled} />
          </div>
          <div className="grid gap-4 md:grid-cols-2">
            <Field label="Button label" error={show(errors.name)} hint="Shown as “Sign in with <label>”.">
              {(p) => <Input {...p} value={name} placeholder="Okta" onChange={(e) => setName(e.target.value)} />}
            </Field>
            <Field
              label="Issuer URL"
              error={show(errors.issuer)}
              hint="Discovery is read from /.well-known/openid-configuration."
            >
              {(p) => (
                <Input
                  {...p}
                  type="url"
                  value={issuer}
                  placeholder="https://example.okta.com"
                  className="font-mono text-xs"
                  onChange={(e) => setIssuer(e.target.value)}
                />
              )}
            </Field>
            <Field label="Client ID" error={show(errors.clientId)}>
              {(p) => (
                <Input
                  {...p}
                  value={clientId}
                  autoComplete="off"
                  spellCheck={false}
                  className="font-mono text-xs"
                  onChange={(e) => setClientId(e.target.value)}
                />
              )}
            </Field>
            <Field
              label="Client secret"
              badge={hasSecret && <Badge variant="outline">stored</Badge>}
              hint={
                hasSecret
                  ? "A secret is stored and never shown. Leave blank to keep it; type a new one to replace it."
                  : "Not set. Leave blank for public clients (PKCE only)."
              }
            >
              {(p) => (
                <Input
                  {...p}
                  type="password"
                  value={secret}
                  autoComplete="new-password"
                  spellCheck={false}
                  placeholder={hasSecret ? "•••••••• (unchanged)" : ""}
                  className="font-mono text-xs"
                  onChange={(e) => setSecret(e.target.value)}
                />
              )}
            </Field>
          </div>
          <Field
            label="Redirect URL"
            error={show(errors.redirectUrl)}
            hint="Register exactly this URL with your identity provider."
          >
            {(p) => (
              <div className="flex gap-2">
                <Input
                  {...p}
                  type="url"
                  value={redirectUrl}
                  className="font-mono text-xs"
                  onChange={(e) => setRedirectUrl(e.target.value)}
                />
                <Button type="button" variant="outline" onClick={() => setRedirectUrl(defaultRedirectUrl())}>
                  Use this server
                </Button>
              </div>
            )}
          </Field>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Accounts</CardTitle>
          <CardDescription>How identities from the provider map to W0rkTree accounts.</CardDescription>
        </CardHeader>
        <CardContent className="grid gap-5">
          <div className="grid gap-4 md:grid-cols-2">
            <Field label="Scopes" error={show(errors.scopes)} hint="Space separated.">
              {(p) => (
                <Input
                  {...p}
                  value={scopes}
                  className="font-mono text-xs"
                  onChange={(e) => setScopes(e.target.value)}
                />
              )}
            </Field>
            <Field
              label="Username claim"
              error={show(errors.usernameClaim)}
              hint="ID-token claim used as the username."
            >
              {(p) => (
                <Input
                  {...p}
                  value={usernameClaim}
                  className="font-mono text-xs"
                  onChange={(e) => setUsernameClaim(e.target.value)}
                />
              )}
            </Field>
          </div>
          <Field
            label="Allowed email domains"
            hint="Comma separated, e.g. example.com. Empty allows any verified email."
          >
            {(p) => (
              <Input
                {...p}
                value={domains}
                placeholder="example.com, example.org"
                className="font-mono text-xs"
                onChange={(e) => setDomains(e.target.value)}
              />
            )}
          </Field>
          <div className="flex items-start justify-between gap-4 rounded-md border p-3">
            <div className="grid gap-0.5">
              <Label htmlFor={autoCreateId}>Create accounts on first sign-in</Label>
              <p className="text-xs text-muted-foreground">
                Otherwise an account with a matching email must already exist.
              </p>
            </div>
            <Switch id={autoCreateId} checked={autoCreate} onCheckedChange={setAutoCreate} />
          </div>
        </CardContent>
        <CardFooter className="flex-col items-stretch gap-3 border-t sm:flex-row sm:items-center sm:justify-between">
          <p className="text-xs text-muted-foreground">
            {config.updated_at ? (
              <>
                Last saved{" "}
                <time dateTime={config.updated_at} title={absoluteTime(config.updated_at)}>
                  {relativeTime(config.updated_at)}
                </time>
              </>
            ) : (
              "Not configured yet."
            )}
          </p>
          <Button type="submit" disabled={save.isPending}>
            {save.isPending && <Loader2 aria-hidden="true" className="animate-spin motion-reduce:animate-none" />}
            Save settings
          </Button>
        </CardFooter>
      </Card>
      <FormError error={save.error} />
    </form>
  );
}

function SsoSettings() {
  const { isAdmin } = useAuth();
  const config = useSsoConfig(isAdmin);
  if (config.isPending) {
    return (
      <div role="status" aria-label="Loading SSO settings" className="flex flex-col gap-4">
        <Skeleton className="h-64 w-full" />
        <Skeleton className="h-48 w-full" />
      </div>
    );
  }
  if (config.isError) {
    // Never configured: start from the defaults.
    if (isApiError(config.error) && config.error.status === 404) return <SsoForm config={EMPTY} />;
    return (
      <QueryError error={config.error} onRetry={() => void config.refetch()} title="Could not load SSO settings" />
    );
  }
  return <SsoForm key={config.data.updated_at ?? "new"} config={{ ...EMPTY, ...config.data }} />;
}

/** Administration → Single sign-on (§12.3 [iam-sso], server admins only). */
export function SsoSettingsPage() {
  return (
    <>
      <PageHeader
        title="Single sign-on"
        breadcrumbs={[{ label: "Single sign-on" }]}
        description={
          <span className="flex items-center gap-1.5">
            <KeyRound aria-hidden="true" className="size-3.5" />
            Let people sign in with your OpenID Connect identity provider.
          </span>
        }
      />
      <RequireAdmin>
        <SsoSettings />
      </RequireAdmin>
    </>
  );
}
