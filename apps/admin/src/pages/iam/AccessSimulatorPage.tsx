import { useMutation } from "@tanstack/react-query";
import { Bot, Play, ShieldQuestionMark, UserRound } from "lucide-react";
import { useId, useMemo, useState, type FormEvent } from "react";
import { useSearchParams } from "react-router";
import {
  actionArea,
  groupByArea,
  useEffectivePermissions,
  useIamActions,
  useIamApi,
  type EffectiveQuery,
  type PrincipalType,
  type SimulateRequest,
} from "@/api/iam";
import { useTenants, useTenantTrees } from "@/api/queries";
import { useAuth } from "@/auth/context";
import { EmptyState, FormError } from "@/components/people/states";
import { PageHeader } from "@/components/PageHeader";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectGroup, SelectItem, SelectLabel, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { DecisionCard, EffectivePanel } from "./DecisionCard";
import { FieldErrors } from "./shared";

const NO_TREE = "__tenant__";
const IP_RE = /^(?:\d{1,3}(?:\.\d{1,3}){3}|[0-9a-fA-F:]*:[0-9a-fA-F:.]*)$/;

interface Submitted {
  request: SimulateRequest;
  effective: EffectiveQuery;
  summary: string;
  scope: string;
}

/** `/iam/simulator`: evaluate a hypothetical request and show effective permissions. */
export function AccessSimulatorPage() {
  const id = useId();
  const iam = useIamApi();
  const { account } = useAuth();
  const [params] = useSearchParams();
  const tenants = useTenants();
  const actions = useIamActions();

  const [ptype, setPtype] = useState<PrincipalType>("user");
  const [pname, setPname] = useState(() => params.get("principal") ?? account?.username ?? "");
  const [action, setAction] = useState(() => params.get("action") ?? "tree:read");
  const [tenantSel, setTenant] = useState(() => params.get("tenant") ?? "");
  const [tree, setTree] = useState(() => params.get("tree") ?? NO_TREE);
  const [branch, setBranch] = useState("");
  const [path, setPath] = useState("");
  const [ip, setIp] = useState("");
  const [mfa, setMfa] = useState(false);
  const [touched, setTouched] = useState(false);
  const [submitted, setSubmitted] = useState<Submitted | null>(null);

  const tenant = tenantSel || tenants.data?.[0]?.slug || "";
  const trees = useTenantTrees(tenant);
  const actionGroups = useMemo(() => groupByArea(actions.data ?? [], (a) => a.area || actionArea(a.action)), [actions.data]);
  const hasTree = tree !== NO_TREE;

  const simulate = useMutation({
    mutationFn: (req: SimulateRequest) => iam.simulate(req),
    meta: { inlineError: true },
  });
  const effective = useEffectivePermissions(submitted?.effective ?? null);

  const name = pname.trim();
  const errors = {
    principal: !name
      ? ["Required."]
      : ptype === "service_account" && name.split("/").length > 2
        ? ["Use name or tenant/name."]
        : [],
    tenant: tenant ? [] : ["Choose a tenant."],
    ip: !ip.trim() || IP_RE.test(ip.trim()) ? [] : ["Enter an IPv4 or IPv6 address."],
  };

  const submit = (e: FormEvent) => {
    e.preventDefault();
    setTouched(true);
    if (errors.principal.length || errors.tenant.length || errors.ip.length) return;
    const [saTenant, saName] = name.includes("/") ? (name.split("/") as [string, string]) : [tenant, name];
    const principal =
      ptype === "user" ? { type: ptype, name } : { type: ptype, name: saName, tenant: saTenant };
    const request: SimulateRequest = {
      principal,
      action,
      tenant,
      ...(hasTree ? { tree } : {}),
      ...(hasTree && branch.trim() ? { branch: branch.trim() } : {}),
      ...(hasTree && path.trim() ? { path: path.trim() } : {}),
      context: { ...(ip.trim() ? { ip: ip.trim() } : {}), ...(mfa ? { mfa: true } : {}) },
    };
    const principalRef = ptype === "user" ? `user:${name}` : `sa:${saTenant}/${saName}`;
    const scope = hasTree ? `${tenant}/${tree}` : tenant;
    let resource = hasTree ? `wt:tree/${tenant}/${tree}` : `wt:tenant/${tenant}`;
    if (hasTree && branch.trim()) resource = `wt:branch/${tenant}/${tree}/${branch.trim()}`;
    if (hasTree && path.trim()) resource = `wt:path/${tenant}/${tree}/${path.trim().replace(/^\/+/, "")}`;
    setSubmitted({
      request,
      effective: { tenant, ...(hasTree ? { tree } : {}), principal: principalRef },
      summary: `${principalRef} → ${action} on ${resource}`,
      scope: `${principalRef} in ${scope}`,
    });
    simulate.mutate(request);
  };

  return (
    <>
      <PageHeader
        title="Access simulator"
        breadcrumbs={[{ label: "Access simulator" }]}
        description="Evaluate a hypothetical request against roles, grants and policies, and see every contributing reason."
      />
      <div className="grid items-start gap-4 xl:grid-cols-[minmax(0,26rem)_minmax(0,1fr)]">
        <Card>
          <CardHeader>
            <CardTitle>Request</CardTitle>
            <CardDescription>Who wants to do what, where.</CardDescription>
          </CardHeader>
          <CardContent>
            <form onSubmit={submit} className="flex flex-col gap-4" noValidate aria-label="Simulation request">
              <fieldset className="grid gap-2">
                <legend className="mb-2 text-sm font-medium">Principal</legend>
                <ToggleGroup
                  type="single"
                  variant="outline"
                  value={ptype}
                  onValueChange={(v) => v && setPtype(v as PrincipalType)}
                  aria-label="Principal type"
                  className="w-fit"
                >
                  <ToggleGroupItem value="user" className="data-[state=on]:bg-foreground data-[state=on]:text-background">
                    <UserRound aria-hidden="true" />
                    User
                  </ToggleGroupItem>
                  <ToggleGroupItem value="service_account" className="data-[state=on]:bg-foreground data-[state=on]:text-background">
                    <Bot aria-hidden="true" />
                    Service account
                  </ToggleGroupItem>
                </ToggleGroup>
                <Label htmlFor={`${id}-principal`} className="sr-only">
                  {ptype === "user" ? "Username" : "Service account"}
                </Label>
                <div className="flex items-center rounded-md border border-input shadow-xs focus-within:border-ring focus-within:ring-[3px] focus-within:ring-ring/50">
                  <span className="pl-3 font-mono text-sm text-muted-foreground" aria-hidden="true">
                    {ptype === "user" ? "user:" : "sa:"}
                  </span>
                  <Input
                    id={`${id}-principal`}
                    value={pname}
                    onChange={(e) => setPname(e.target.value)}
                    placeholder={ptype === "user" ? "alice" : `${tenant || "acme"}/ci-bot`}
                    className="border-0 pl-1 font-mono shadow-none focus-visible:ring-0"
                    autoComplete="off"
                    spellCheck={false}
                    aria-invalid={touched && errors.principal.length > 0}
                  />
                </div>
                {ptype === "service_account" && <p className="text-xs text-muted-foreground">Name, or tenant/name for another tenant's account.</p>}
                {touched && <FieldErrors messages={errors.principal} />}
              </fieldset>

              <div className="grid gap-2">
                <Label htmlFor={`${id}-action`}>Action</Label>
                <Select value={action} onValueChange={setAction}>
                  <SelectTrigger id={`${id}-action`} className="w-full font-mono">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {actionGroups.length === 0 && <SelectItem value={action}>{action}</SelectItem>}
                    {actionGroups.map(([area, list]) => (
                      <SelectGroup key={area}>
                        <SelectLabel>{area}</SelectLabel>
                        {list.map((a) => (
                          <SelectItem key={a.action} value={a.action} className="font-mono">
                            {a.action}
                          </SelectItem>
                        ))}
                      </SelectGroup>
                    ))}
                  </SelectContent>
                </Select>
              </div>

              <fieldset className="grid gap-3">
                <legend className="mb-2 text-sm font-medium">Resource</legend>
                <div className="grid gap-3 sm:grid-cols-2">
                  <div className="grid gap-2">
                    <Label htmlFor={`${id}-tenant`}>Tenant</Label>
                    <Select
                      value={tenant}
                      onValueChange={(t) => {
                        setTenant(t);
                        setTree(NO_TREE);
                      }}
                    >
                      <SelectTrigger id={`${id}-tenant`} className="w-full font-mono" aria-invalid={touched && errors.tenant.length > 0}>
                        <SelectValue placeholder="Choose" />
                      </SelectTrigger>
                      <SelectContent>
                        {tenantSel && !tenants.data?.some((t) => t.slug === tenantSel) && <SelectItem value={tenantSel}>{tenantSel}</SelectItem>}
                        {(tenants.data ?? []).map((t) => (
                          <SelectItem key={t.id} value={t.slug} className="font-mono">
                            {t.slug}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                    {touched && <FieldErrors messages={errors.tenant} />}
                  </div>
                  <div className="grid gap-2">
                    <Label htmlFor={`${id}-tree`}>Tree</Label>
                    <Select value={tree} onValueChange={setTree}>
                      <SelectTrigger id={`${id}-tree`} className="w-full font-mono">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value={NO_TREE}>None (tenant-level)</SelectItem>
                        {hasTree && !trees.data?.some((t) => t.tree.name === tree) && <SelectItem value={tree}>{tree}</SelectItem>}
                        {(trees.data ?? []).map((t) => (
                          <SelectItem key={t.tree.id} value={t.tree.name} className="font-mono">
                            {t.tree.name}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  </div>
                  <div className="grid gap-2">
                    <Label htmlFor={`${id}-branch`}>Branch</Label>
                    <Input
                      id={`${id}-branch`}
                      value={branch}
                      onChange={(e) => setBranch(e.target.value)}
                      disabled={!hasTree}
                      placeholder="main"
                      className="font-mono"
                      spellCheck={false}
                    />
                  </div>
                  <div className="grid gap-2">
                    <Label htmlFor={`${id}-path`}>Path</Label>
                    <Input
                      id={`${id}-path`}
                      value={path}
                      onChange={(e) => setPath(e.target.value)}
                      disabled={!hasTree}
                      placeholder="secrets/prod.env"
                      className="font-mono"
                      spellCheck={false}
                    />
                  </div>
                </div>
              </fieldset>

              <fieldset className="grid gap-3">
                <legend className="mb-2 text-sm font-medium">Context</legend>
                <div className="grid gap-2">
                  <Label htmlFor={`${id}-ip`}>Client IP</Label>
                  <Input
                    id={`${id}-ip`}
                    value={ip}
                    onChange={(e) => setIp(e.target.value)}
                    placeholder="203.0.113.7"
                    className="font-mono"
                    spellCheck={false}
                    aria-invalid={touched && errors.ip.length > 0}
                  />
                  {touched && <FieldErrors messages={errors.ip} />}
                </div>
                <div className="flex items-center justify-between gap-4 rounded-md border p-3">
                  <Label htmlFor={`${id}-mfa`}>Signed in with MFA</Label>
                  <Switch id={`${id}-mfa`} checked={mfa} onCheckedChange={setMfa} />
                </div>
              </fieldset>

              <Button type="submit" disabled={simulate.isPending}>
                <Play aria-hidden="true" />
                {simulate.isPending ? "Simulating…" : "Simulate"}
              </Button>
            </form>
          </CardContent>
        </Card>

        <div className="flex min-w-0 flex-col gap-4">
          {simulate.isError ? (
            <FormError error={simulate.error} />
          ) : simulate.data && submitted ? (
            <DecisionCard decision={simulate.data} summary={submitted.summary} />
          ) : (
            <Card>
              <EmptyState icon={ShieldQuestionMark} title="No simulation yet">
                Fill in the request and choose Simulate to see whether it would be allowed and why.
              </EmptyState>
            </Card>
          )}
          {submitted && <EffectivePanel query={effective} scope={submitted.scope} />}
        </div>
      </div>
    </>
  );
}
