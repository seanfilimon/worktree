import { Bot, Check, Minus, Puzzle, ShieldCheck, UserRound, Users, type LucideIcon } from "lucide-react";
import { useState, type MouseEvent, type ReactNode } from "react";
import { errorMessage, isApiError } from "@/api/client";
import { actionArea, groupByArea, useRoles, type SubjectType } from "@/api/iam";
import { FormError } from "@/components/people/states";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Card } from "@/components/ui/card";
import { Select, SelectContent, SelectGroup, SelectItem, SelectLabel, SelectTrigger, SelectValue } from "@/components/ui/select";
import { cn } from "@/lib/utils";
import { BUILTIN_ROLES, isBuiltinRole, SUBJECT_LABEL } from "./util";

/** Card with a toolbar row (description + actions) above a table. */
export function TableCard({ description, actions, children }: { description: ReactNode; actions?: ReactNode; children: ReactNode }) {
  return (
    <Card className="gap-0 overflow-hidden py-0">
      <div className="flex flex-wrap items-center justify-between gap-2 border-b p-3">
        <p className="text-sm text-muted-foreground">{description}</p>
        {actions && <div className="flex flex-wrap items-center gap-2">{actions}</div>}
      </div>
      {children}
    </Card>
  );
}

/** Built-in owner/admin are filled; other built-ins outline; custom roles dashed with an icon. */
export function IamRoleBadge({ role }: { role: string }) {
  if (!role) return <span className="text-muted-foreground">—</span>;
  if (isBuiltinRole(role)) {
    const strong = role === "owner" || role === "admin";
    return (
      <Badge variant={strong ? "default" : "outline"} className="capitalize">
        {strong && <ShieldCheck aria-hidden="true" />}
        {role}
      </Badge>
    );
  }
  return (
    <Badge variant="outline" className="border-dashed font-mono" title="Custom role">
      <Puzzle aria-hidden="true" />
      {role}
    </Badge>
  );
}

const SUBJECT_ICON: Record<SubjectType, LucideIcon> = { user: UserRound, team: Users, service_account: Bot };

export function SubjectIcon({ type, className }: { type: SubjectType; className?: string }) {
  const Icon = SUBJECT_ICON[type];
  return <Icon aria-hidden="true" className={className} />;
}

export function SubjectLabel({ type, name }: { type: SubjectType; name: string }) {
  const Icon = SUBJECT_ICON[type];
  return (
    <span className="flex min-w-0 items-center gap-2">
      <span className="flex size-7 shrink-0 items-center justify-center rounded-full border bg-muted" title={SUBJECT_LABEL[type]}>
        <Icon aria-hidden="true" className="size-3.5" />
      </span>
      <span className="min-w-0">
        <span className="block truncate font-mono text-sm font-medium">{name}</span>
        <span className="block text-xs text-muted-foreground">{SUBJECT_LABEL[type]}</span>
      </span>
    </span>
  );
}

interface RoleSelectProps {
  tenant: string;
  value: string;
  onChange: (role: string) => void;
  id?: string;
  "aria-label"?: string;
  disabled?: boolean;
  size?: "sm" | "default";
  className?: string;
  /** Built-in roles that cannot be picked (e.g. owner). */
  disabledRoles?: readonly string[];
}

/** Built-in plus the tenant's custom roles. Falls back to built-ins if roles cannot be read. */
export function RoleSelect({ tenant, value, onChange, id, disabled, size, className, disabledRoles = [], ...rest }: RoleSelectProps) {
  const roles = useRoles(tenant);
  const custom = (roles.data ?? []).filter((r) => !r.builtin).map((r) => r.name);
  const builtin = roles.data ? roles.data.filter((r) => r.builtin).map((r) => r.name) : [...BUILTIN_ROLES];
  return (
    <Select value={value} onValueChange={onChange} disabled={disabled}>
      <SelectTrigger id={id} size={size} className={cn("w-full", className)} aria-label={rest["aria-label"]}>
        <SelectValue placeholder="Choose a role" />
      </SelectTrigger>
      <SelectContent>
        <SelectGroup>
          <SelectLabel>Built-in</SelectLabel>
          {builtin.map((r) => (
            <SelectItem key={r} value={r} disabled={disabledRoles.includes(r)} className="capitalize">
              {r}
            </SelectItem>
          ))}
        </SelectGroup>
        {custom.length > 0 && (
          <SelectGroup>
            <SelectLabel>Custom</SelectLabel>
            {custom.map((r) => (
              <SelectItem key={r} value={r} className="font-mono">
                {r}
              </SelectItem>
            ))}
          </SelectGroup>
        )}
      </SelectContent>
    </Select>
  );
}

/** Permission chips grouped by area. */
export function PermissionChips({ permissions, emptyLabel = "No permissions" }: { permissions: readonly string[]; emptyLabel?: string }) {
  if (permissions.length === 0) return <p className="text-sm text-muted-foreground">{emptyLabel}</p>;
  const groups = groupByArea([...permissions].sort(), actionArea);
  return (
    <dl className="grid gap-3">
      {groups.map(([area, list]) => (
        <div key={area} className="grid gap-1.5 sm:grid-cols-[8rem_1fr] sm:items-baseline">
          <dt className="text-xs font-medium tracking-wide text-muted-foreground uppercase">{area}</dt>
          <dd>
            <ul aria-label={`${area} permissions`} className="flex flex-wrap gap-1.5">
              {list.map((p) => (
                <li key={p}>
                  <Badge variant={p.endsWith("*") ? "default" : "secondary"} className="font-mono">
                    {p}
                  </Badge>
                </li>
              ))}
            </ul>
          </dd>
        </div>
      ))}
    </dl>
  );
}

/** Check / dash cell for permission matrices. */
export function AllowedMark({ allowed, label }: { allowed: boolean; label: string }) {
  return allowed ? (
    <span className="inline-flex size-5 items-center justify-center rounded-sm bg-foreground text-background" title={`${label}: allowed`}>
      <Check aria-hidden="true" className="size-3.5" />
      <span className="sr-only">{`${label}: allowed`}</span>
    </span>
  ) : (
    <span className="inline-flex size-5 items-center justify-center text-muted-foreground" title={`${label}: not allowed`}>
      <Minus aria-hidden="true" className="size-3.5" />
      <span className="sr-only">{`${label}: not allowed`}</span>
    </span>
  );
}

interface ConfirmActionProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  description: ReactNode;
  confirmLabel: string;
  /** Resolves when done; the dialog closes on success and shows the error inline on failure. */
  onConfirm: () => Promise<unknown>;
  /** Extra guidance shown with a 409 conflict. */
  conflictHint?: string;
}

/**
 * Confirmation dialog that keeps server errors inline (use with mutations
 * declaring `meta: { inlineError: true }`).
 */
export function ConfirmAction({ open, onOpenChange, title, description, confirmLabel, onConfirm, conflictHint }: ConfirmActionProps) {
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<unknown>(null);

  const change = (next: boolean) => {
    if (!next) setError(null);
    onOpenChange(next);
  };

  const confirm = async (e: MouseEvent) => {
    e.preventDefault();
    setPending(true);
    setError(null);
    try {
      await onConfirm();
      change(false);
    } catch (err) {
      setError(err);
    } finally {
      setPending(false);
    }
  };

  const conflict = isApiError(error) && error.status === 409;

  return (
    <AlertDialog open={open} onOpenChange={change}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{title}</AlertDialogTitle>
          <AlertDialogDescription>{description}</AlertDialogDescription>
        </AlertDialogHeader>
        {conflict ? (
          <Alert>
            <AlertTitle>Cannot {confirmLabel.toLowerCase()}</AlertTitle>
            <AlertDescription>
              <p>{errorMessage(error)}</p>
              {conflictHint && <p>{conflictHint}</p>}
            </AlertDescription>
          </Alert>
        ) : (
          <FormError error={error} />
        )}
        <AlertDialogFooter>
          <AlertDialogCancel disabled={pending}>{conflict ? "Close" : "Cancel"}</AlertDialogCancel>
          {!conflict && (
            <AlertDialogAction disabled={pending} onClick={(e) => void confirm(e)}>
              {pending ? "Working…" : confirmLabel}
            </AlertDialogAction>
          )}
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}

/** Small inline list of field errors. */
export function FieldErrors({ id, messages }: { id?: string; messages: readonly string[] }) {
  if (messages.length === 0) return null;
  return (
    <ul id={id} className="grid gap-0.5 text-xs font-medium text-foreground">
      {messages.map((m) => (
        <li key={m} className="before:mr-1 before:content-['—']">
          {m}
        </li>
      ))}
    </ul>
  );
}
