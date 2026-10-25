import { Building2, Globe, Lock, ShieldCheck, UserRound, Users, type LucideIcon } from "lucide-react";
import type { Role, TenantKind, TenantPlan, Visibility } from "@/api/types";
import { Badge } from "@/components/ui/badge";

/** Tenant kind: always outline, distinguished by icon and label. */
export function TenantKindBadge({ kind }: { kind: TenantKind }) {
  const Icon = kind === "org" ? Building2 : UserRound;
  return (
    <Badge variant="outline">
      <Icon aria-hidden="true" />
      {kind === "org" ? "Organisation" : "Personal"}
    </Badge>
  );
}

const PLAN_VARIANT: Record<TenantPlan, "outline" | "secondary" | "default"> = {
  free: "outline",
  pro: "secondary",
  enterprise: "default",
};

/** Plan: weight increases with tier (outline → grey → filled). */
export function PlanBadge({ plan }: { plan: TenantPlan }) {
  return (
    <Badge variant={PLAN_VARIANT[plan]} className="capitalize">
      {plan}
    </Badge>
  );
}

/** Roles that can manage the tenant are filled; the rest are outline. */
export function RoleBadge({ role }: { role: Role }) {
  const strong = role === "owner" || role === "admin";
  return (
    <Badge variant={strong ? "default" : "outline"} className="capitalize">
      {strong && <ShieldCheck aria-hidden="true" />}
      {role}
    </Badge>
  );
}

const VISIBILITY_ICON: Record<Visibility, LucideIcon> = { private: Lock, internal: Users, public: Globe };

export function VisibilityBadge({ visibility }: { visibility: Visibility }) {
  const Icon = VISIBILITY_ICON[visibility];
  return (
    <Badge variant={visibility === "public" ? "secondary" : "outline"} className="capitalize">
      <Icon aria-hidden="true" />
      {visibility}
    </Badge>
  );
}
