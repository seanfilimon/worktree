import { useMutation, useQueryClient } from "@tanstack/react-query";
import { BadgeCheck, CircleDashed, Mail, Send } from "lucide-react";
import { useState, type FormEvent } from "react";
import { useSearchParams } from "react-router";
import { keys } from "@/api/queries";
import { EMAIL_RE, useLifecycleApi, type TenantV08, type VerificationSent } from "@/api/tenantLifecycle";
import { FormError } from "@/components/people/states";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { absoluteTime } from "@/lib/format";
import { useToast } from "@/toast/store";

/** Verified = filled badge; unverified = outline badge. */
export function EmailStatusBadge({ verified }: { verified: boolean }) {
  return verified ? (
    <Badge>
      <BadgeCheck aria-hidden="true" />
      Verified
    </Badge>
  ) : (
    <Badge variant="outline">
      <CircleDashed aria-hidden="true" />
      Unverified
    </Badge>
  );
}

function sentText(s: VerificationSent): string {
  return s.delivery === "log"
    ? `SMTP is not configured on this server: the code for ${s.email} was written to the server log.`
    : `A code was emailed to ${s.email}; it expires ${absoluteTime(s.expires_at)}.`;
}

/**
 * Tenant contact email (§16.4): set or clear it, verify it with the
 * emailed one-time code (`?email_token=` pre-fills the code) and resend
 * codes. A verified email names the tenant in grants and license rules.
 */
export function TenantEmailCard({ tenant, editable }: { tenant: TenantV08; editable: boolean }) {
  const api = useLifecycleApi();
  const queryClient = useQueryClient();
  const toast = useToast();
  const [params] = useSearchParams();
  const current = tenant.email ?? "";
  const verified = Boolean(tenant.email_verified_at);
  const [email, setEmail] = useState(current);
  const [code, setCode] = useState(params.get("email_token") ?? "");
  const [sent, setSent] = useState<VerificationSent | null>(null);

  const trimmed = email.trim();
  const emailError = trimmed !== "" && !EMAIL_RE.test(trimmed) ? "Enter an address like ops@example.com." : undefined;
  const dirty = trimmed !== current;

  const store = async (updated: TenantV08) => {
    queryClient.setQueryData(keys.tenant(tenant.slug), updated);
    await queryClient.invalidateQueries({ queryKey: keys.tenants });
  };

  const save = useMutation({
    mutationFn: (value: string) => api.setEmail(tenant.slug, value),
    meta: { inlineError: true },
    onSuccess: async (updated) => {
      await store(updated);
      setSent(null);
      toast.success(updated.email ? "Email saved" : "Email removed", updated.email ? "A verification code was sent." : tenant.slug);
    },
  });
  const verify = useMutation({
    mutationFn: (value: string) => api.verifyEmail(tenant.slug, value),
    meta: { inlineError: true },
    onSuccess: async (updated) => {
      await store(updated);
      setCode("");
      toast.success("Email verified", updated.email);
    },
  });
  const resend = useMutation({
    mutationFn: () => api.resendEmail(tenant.slug),
    meta: { inlineError: true },
    onSuccess: (s) => setSent(s),
  });

  const onSave = (e: FormEvent) => {
    e.preventDefault();
    if (!emailError && dirty) save.mutate(trimmed);
  };
  const onVerify = (e: FormEvent) => {
    e.preventDefault();
    if (code.trim()) verify.mutate(code.trim());
  };

  return (
    <Card>
      <CardHeader>
        <CardTitle>Contact email</CardTitle>
        <CardDescription>
          A verified address names this tenant in grants and license rules, e.g. a tenant grant for{" "}
          <span className="font-mono">{current || "ops@example.com"}</span>.
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-6">
        <div role="group" aria-label="Email status" className="flex flex-wrap items-center gap-3">
          <Mail aria-hidden="true" className="size-4 text-muted-foreground" />
          {current ? (
            <>
              <span className="font-mono text-sm">{current}</span>
              <EmailStatusBadge verified={verified} />
              {verified && tenant.email_verified_at && (
                <span className="text-sm text-muted-foreground">since {absoluteTime(tenant.email_verified_at)}</span>
              )}
            </>
          ) : (
            <span className="text-sm text-muted-foreground">No contact email.</span>
          )}
        </div>

        {editable && (
          <form onSubmit={onSave} aria-label="Contact email" noValidate className="flex flex-col gap-2">
            <Label htmlFor="tenant-email">Email address</Label>
            <div className="flex flex-wrap gap-2">
              <Input
                id="tenant-email"
                type="email"
                className="max-w-sm"
                value={email}
                placeholder="ops@example.com"
                aria-invalid={emailError ? true : undefined}
                aria-describedby={emailError ? "tenant-email-error" : undefined}
                onChange={(e) => setEmail(e.target.value)}
              />
              <Button type="submit" disabled={!dirty || Boolean(emailError) || save.isPending}>
                {save.isPending ? "Saving…" : trimmed === "" && current !== "" ? "Remove email" : "Save email"}
              </Button>
            </div>
            {emailError && (
              <p id="tenant-email-error" className="text-sm font-medium">
                {emailError}
              </p>
            )}
            <FormError error={save.error} />
          </form>
        )}

        {editable && current && !verified && (
          <form onSubmit={onVerify} aria-label="Verify email" noValidate className="flex flex-col gap-2 border-t pt-6">
            <Label htmlFor="tenant-email-code">Verification code</Label>
            <p className="text-sm text-muted-foreground">
              Enter the single-use code (<span className="font-mono">wte_…</span>, valid 24 hours) sent to {current}.
            </p>
            <div className="flex flex-wrap gap-2">
              <Input
                id="tenant-email-code"
                className="max-w-sm font-mono"
                value={code}
                autoComplete="one-time-code"
                onChange={(e) => setCode(e.target.value)}
              />
              <Button type="submit" disabled={!code.trim() || verify.isPending}>
                {verify.isPending ? "Verifying…" : "Verify"}
              </Button>
              <Button type="button" variant="outline" disabled={resend.isPending} onClick={() => resend.mutate()}>
                <Send aria-hidden="true" />
                {resend.isPending ? "Sending…" : "Resend code"}
              </Button>
            </div>
            {sent && (
              <p role="status" className="text-sm">
                {sentText(sent)}
              </p>
            )}
            <FormError error={verify.error ?? resend.error} />
          </form>
        )}
      </CardContent>
    </Card>
  );
}
