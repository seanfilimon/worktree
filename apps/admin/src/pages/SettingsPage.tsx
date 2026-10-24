import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useState, type FormEvent } from "react";
import { useApi } from "@/api/context";
import { keys, useTokens } from "@/api/queries";
import type { CreatedToken } from "@/api/types";
import { useAuth } from "@/auth/context";
import { Icon } from "@/components/icons";
import { PageHeader } from "@/components/PageHeader";
import { Badge } from "@/components/legacy/Badge";
import { Button } from "@/components/legacy/Button";
import { Section } from "@/components/legacy/Card";
import { ConfirmButton } from "@/components/legacy/ConfirmButton";
import { SelectField, TextField } from "@/components/legacy/Field";
import { Alert, EmptyState, ErrorState, SkeletonRows } from "@/components/legacy/States";
import { Table, Td, Th, Tr } from "@/components/legacy/Table";
import { RelativeTime } from "@/components/legacy/Time";
import { cn } from "@/lib/cn";
import { THEME_PREFERENCES, useTheme } from "@/theme/theme";
import { useToast } from "@/toast/store";

const TTL_OPTIONS = [
  { hours: 24, label: "1 day" },
  { hours: 24 * 7, label: "7 days" },
  { hours: 24 * 30, label: "30 days" },
  { hours: 24 * 90, label: "90 days" },
  { hours: 24 * 365, label: "1 year" },
  { hours: 0, label: "Never" },
];

function NewTokenNotice({ created, onDismiss }: { created: CreatedToken; onDismiss: () => void }) {
  const toast = useToast();
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(created.token);
      toast.success("Token copied to clipboard");
    } catch {
      toast.push({ kind: "error", title: "Copy failed", message: "Select the token and copy it manually." });
    }
  };
  return (
    <Alert tone="success" title={`Token “${created.api_token.name}” created`} className="mb-4">
      <p>Copy it now — it will not be shown again.</p>
      <div className="mt-2 flex flex-wrap items-center gap-2">
        <input
          readOnly
          aria-label="New API token"
          value={created.token}
          onFocus={(e) => e.currentTarget.select()}
          className="h-8 min-w-0 flex-1 rounded-md border border-border-strong bg-surface px-2 font-mono text-xs text-fg"
        />
        <Button size="sm" onClick={() => void copy()}>
          <Icon name="copy" className="size-3.5" />
          Copy
        </Button>
        <Button size="sm" variant="ghost" onClick={onDismiss}>
          Done
        </Button>
      </div>
    </Alert>
  );
}

function TokensSection() {
  const api = useApi();
  const tokens = useTokens();
  const queryClient = useQueryClient();
  const toast = useToast();
  const [name, setName] = useState("");
  const [ttl, setTtl] = useState(String(24 * 30));
  const [created, setCreated] = useState<CreatedToken | null>(null);
  const [now] = useState(Date.now);

  const create = useMutation({
    mutationFn: api.auth.createToken,
    onSuccess: async (res) => {
      setCreated(res);
      setName("");
      await queryClient.invalidateQueries({ queryKey: keys.tokens });
    },
  });
  const revoke = useMutation({
    mutationFn: api.auth.revokeToken,
    onSuccess: async () => {
      toast.success("Token revoked");
      await queryClient.invalidateQueries({ queryKey: keys.tokens });
    },
  });

  const onSubmit = (e: FormEvent) => {
    e.preventDefault();
    if (name.trim()) create.mutate({ name: name.trim(), ttl_hours: Number(ttl) });
  };

  return (
    <Section title="API tokens" description="Personal access tokens for the wt CLI, CI and scripts. They act with your permissions." flush>
      <div className="border-b border-border p-4">
        {created && <NewTokenNotice created={created} onDismiss={() => setCreated(null)} />}
        <form onSubmit={onSubmit} aria-label="Create API token" className="flex flex-wrap items-end gap-3">
          <TextField label="Token name" value={name} onChange={(e) => setName(e.target.value)} placeholder="ci-deploy" fieldClassName="w-56" />
          <SelectField label="Expires" value={ttl} onChange={(e) => setTtl(e.target.value)} fieldClassName="w-36">
            {TTL_OPTIONS.map((o) => (
              <option key={o.hours} value={o.hours}>
                {o.label}
              </option>
            ))}
          </SelectField>
          <Button type="submit" variant="primary" loading={create.isPending} disabled={!name.trim()}>
            Create token
          </Button>
        </form>
      </div>
      {tokens.isPending ? (
        <SkeletonRows rows={3} label="Loading tokens" />
      ) : tokens.isError ? (
        <ErrorState error={tokens.error} onRetry={() => void tokens.refetch()} />
      ) : tokens.data.length === 0 ? (
        <EmptyState title="No API tokens" />
      ) : (
        <Table label="API tokens">
          <thead>
            <tr>
              <Th>Name</Th>
              <Th>Created</Th>
              <Th>Last used</Th>
              <Th>Expires</Th>
              <Th>
                <span className="sr-only">Actions</span>
              </Th>
            </tr>
          </thead>
          <tbody>
            {tokens.data.map((t) => {
              const expired = t.expires_at ? Date.parse(t.expires_at) < now : false;
              return (
                <Tr key={t.id}>
                  <Td className="font-medium">
                    {t.name}
                    {expired && (
                      <Badge tone="danger" className="ml-1.5">
                        expired
                      </Badge>
                    )}
                  </Td>
                  <Td>
                    <RelativeTime value={t.created_at} className="text-muted-foreground" />
                  </Td>
                  <Td>{t.last_used_at ? <RelativeTime value={t.last_used_at} className="text-muted-foreground" /> : <span className="text-subtle">never</span>}</Td>
                  <Td>{t.expires_at ? <RelativeTime value={t.expires_at} className="text-muted-foreground" /> : <span className="text-subtle">never</span>}</Td>
                  <Td className="text-right">
                    <ConfirmButton confirmLabel="Revoke" onConfirm={() => revoke.mutate(t.id)} aria-label={`Revoke ${t.name}`}>
                      Revoke
                    </ConfirmButton>
                  </Td>
                </Tr>
              );
            })}
          </tbody>
        </Table>
      )}
    </Section>
  );
}

function ThemeSection() {
  const { preference, setPreference } = useTheme();
  return (
    <Section title="Appearance">
      <fieldset>
        <legend className="mb-2 text-[13px] font-medium">Theme</legend>
        <div className="inline-flex rounded-md border border-border-strong p-0.5">
          {THEME_PREFERENCES.map((p) => (
            <label
              key={p}
              className={cn(
                "cursor-pointer rounded px-3 py-1 text-sm font-medium capitalize has-[:focus-visible]:outline-2 has-[:focus-visible]:outline-ring",
                preference === p ? "bg-surface-2 text-fg" : "text-muted-foreground hover:text-fg",
              )}
            >
              <input type="radio" name="theme" value={p} checked={preference === p} onChange={() => setPreference(p)} className="sr-only" />
              {p}
            </label>
          ))}
        </div>
      </fieldset>
    </Section>
  );
}

function ProfileSection() {
  const { account, me } = useAuth();
  if (!account) return null;
  const memberships = me.data?.memberships ?? [];
  return (
    <Section title="Profile">
      <dl className="grid gap-x-6 gap-y-2 text-[13px] sm:grid-cols-[max-content_1fr]">
        <dt className="text-muted-foreground">Username</dt>
        <dd className="font-mono">{account.username}</dd>
        <dt className="text-muted-foreground">Email</dt>
        <dd>{account.email}</dd>
        <dt className="text-muted-foreground">Role</dt>
        <dd>{account.is_admin ? <Badge tone="accent">server admin</Badge> : "member"}</dd>
        <dt className="text-muted-foreground">Tenants</dt>
        <dd className="flex flex-wrap gap-1">
          {memberships.length === 0
            ? "—"
            : memberships.map((m) => (
                <Badge key={m.tenant.id}>
                  {m.tenant.slug} · {m.role}
                </Badge>
              ))}
        </dd>
      </dl>
    </Section>
  );
}

export function SettingsPage() {
  return (
    <>
      <PageHeader title="Settings" breadcrumbs={[{ label: "Settings" }]} />
      <div className="flex flex-col gap-5">
        <ProfileSection />
        <TokensSection />
        <ThemeSection />
      </div>
    </>
  );
}
