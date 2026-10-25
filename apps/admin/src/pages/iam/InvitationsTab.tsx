import { useMutation, useQueryClient } from "@tanstack/react-query";
import { AtSign, Ban, Check, Clock, Hourglass, Mail, MailPlus, UserRound } from "lucide-react";
import { useId, useState, type FormEvent } from "react";
import {
  iamKeys,
  invitationStatus,
  useIamApi,
  useInvitations,
  useTeams,
  type CreatedInvitation,
  type Invitation,
  type InvitationStatus,
} from "@/api/iam";
import { CopyButton } from "@/components/people/CopyButton";
import { EmptyState, FormError, QueryError, TableSkeleton } from "@/components/people/states";
import { USERNAME_HINT, USERNAME_RE } from "@/components/people/validation";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { absoluteTime, relativeTime } from "@/lib/format";
import { useToast } from "@/toast/store";
import { ConfirmAction, FieldErrors, IamRoleBadge, RoleSelect, TableCard } from "./shared";
import { EMAIL_RE, inviteLink } from "./util";

interface InvitationsTabProps {
  slug: string;
  editable: boolean;
}

function StatusBadge({ status }: { status: InvitationStatus }) {
  switch (status) {
    case "pending":
      return (
        <Badge variant="outline">
          <Clock aria-hidden="true" />
          Pending
        </Badge>
      );
    case "accepted":
      return (
        <Badge>
          <Check aria-hidden="true" />
          Accepted
        </Badge>
      );
    case "expired":
      return (
        <Badge variant="outline" className="border-dashed text-muted-foreground">
          <Hourglass aria-hidden="true" />
          Expired
        </Badge>
      );
    case "revoked":
      return (
        <Badge variant="secondary" className="line-through">
          <Ban aria-hidden="true" />
          Revoked
        </Badge>
      );
  }
}

export function InvitationsTab({ slug, editable }: InvitationsTabProps) {
  const invitations = useInvitations(slug);
  const teams = useTeams(slug);
  const iam = useIamApi();
  const queryClient = useQueryClient();
  const toast = useToast();
  const [creating, setCreating] = useState(false);
  const [revoking, setRevoking] = useState<Invitation | null>(null);
  const teamName = new Map((teams.data ?? []).map((t) => [t.id, t.slug]));

  const revoke = useMutation({
    mutationFn: (inv: Invitation) => iam.invitations.revoke(slug, inv.id),
    meta: { inlineError: true },
    onSuccess: async () => {
      toast.success("Invitation revoked");
      await queryClient.invalidateQueries({ queryKey: iamKeys.invitations(slug) });
    },
  });


  return (
    <>
      <TableCard
        description="Invite people by email or username. Each invitation is a one-time link that expires after 7 days."
        actions={
          editable && (
            <Button size="sm" onClick={() => setCreating(true)}>
              <MailPlus aria-hidden="true" />
              Invite
            </Button>
          )
        }
      >
        {invitations.isPending ? (
          <TableSkeleton label="Loading invitations" rows={3} />
        ) : invitations.isError ? (
          <QueryError error={invitations.error} onRetry={() => void invitations.refetch()} title="Could not load invitations" />
        ) : invitations.data.length === 0 ? (
          <EmptyState icon={Mail} title="No invitations">
            Invite someone to join {slug} with a role and, optionally, a team.
          </EmptyState>
        ) : (
          <Table aria-label="Invitations">
            <TableHeader className="sticky top-0 z-10 bg-background">
              <TableRow className="hover:bg-transparent">
                <TableHead>Invitee</TableHead>
                <TableHead>Role</TableHead>
                <TableHead>Status</TableHead>
                <TableHead className="hidden md:table-cell">Invited</TableHead>
                <TableHead className="hidden sm:table-cell">Expires</TableHead>
                {editable && (
                  <TableHead>
                    <span className="sr-only">Actions</span>
                  </TableHead>
                )}
              </TableRow>
            </TableHeader>
            <TableBody>
              {invitations.data.map((inv) => {
                const status = invitationStatus(inv);
                const team = inv.team_id ? (teamName.get(inv.team_id) ?? inv.team_id) : "";
                return (
                  <TableRow key={inv.id} className={status === "pending" ? undefined : "text-muted-foreground"}>
                    <TableCell>
                      <p className="flex items-center gap-1.5 font-medium">
                        {inv.email ? <Mail aria-hidden="true" className="size-3.5" /> : <AtSign aria-hidden="true" className="size-3.5" />}
                        {inv.email || inv.username || "anyone with the link"}
                      </p>
                      {team && <p className="text-xs text-muted-foreground">joins team <span className="font-mono">{team}</span></p>}
                      {inv.accepted_by && <p className="text-xs text-muted-foreground">accepted by {inv.accepted_by}</p>}
                    </TableCell>
                    <TableCell>
                      <IamRoleBadge role={inv.role} />
                    </TableCell>
                    <TableCell>
                      <StatusBadge status={status} />
                    </TableCell>
                    <TableCell className="hidden md:table-cell">
                      <time dateTime={inv.created_at} title={absoluteTime(inv.created_at)}>
                        {relativeTime(inv.created_at)}
                      </time>{" "}
                      by {inv.invited_by}
                    </TableCell>
                    <TableCell className="hidden sm:table-cell">
                      <time dateTime={inv.expires_at} title={absoluteTime(inv.expires_at)}>
                        {relativeTime(inv.expires_at)}
                      </time>
                    </TableCell>
                    {editable && (
                      <TableCell className="w-24 text-right">
                        {status === "pending" && (
                          <Button variant="ghost" size="sm" onClick={() => setRevoking(inv)} aria-label={`Revoke invitation for ${inv.email || inv.username || inv.id}`}>
                            <Ban aria-hidden="true" />
                            Revoke
                          </Button>
                        )}
                      </TableCell>
                    )}
                  </TableRow>
                );
              })}
            </TableBody>
          </Table>
        )}
      </TableCard>
      {editable && <CreateInvitationDialog open={creating} onOpenChange={setCreating} tenant={slug} />}
      <ConfirmAction
        open={revoking !== null}
        onOpenChange={(o) => !o && setRevoking(null)}
        title="Revoke invitation?"
        description={`The link for ${revoking?.email || revoking?.username || "this invitation"} stops working.`}
        confirmLabel="Revoke invitation"
        onConfirm={() => (revoking ? revoke.mutateAsync(revoking) : Promise.resolve())}
      />
    </>
  );
}

type Target = "email" | "username";
const NO_TEAM = "__none__";

function CreateInvitationDialog({ open, onOpenChange, tenant }: { open: boolean; onOpenChange: (open: boolean) => void; tenant: string }) {
  const iam = useIamApi();
  const queryClient = useQueryClient();
  const teams = useTeams(tenant);
  const id = useId();
  const [target, setTarget] = useState<Target>("email");
  const [value, setValue] = useState("");
  const [role, setRole] = useState("developer");
  const [team, setTeam] = useState(NO_TEAM);
  const [touched, setTouched] = useState(false);
  const [created, setCreated] = useState<CreatedInvitation | null>(null);

  const v = value.trim();
  const create = useMutation({
    mutationFn: () =>
      iam.invitations.create(tenant, {
        ...(target === "email" ? { email: v } : { username: v.toLowerCase() }),
        role,
        ...(team !== NO_TEAM ? { team } : {}),
      }),
    meta: { inlineError: true },
    onSuccess: async (res) => {
      setCreated(res);
      await queryClient.invalidateQueries({ queryKey: iamKeys.invitations(tenant) });
    },
  });

  function change(next: boolean) {
    if (!next) {
      setTarget("email");
      setValue("");
      setRole("developer");
      setTeam(NO_TEAM);
      setTouched(false);
      setCreated(null);
      create.reset();
    }
    onOpenChange(next);
  }

  const errors =
    target === "email"
      ? EMAIL_RE.test(v)
        ? []
        : ["Enter an email address."]
      : USERNAME_RE.test(v.toLowerCase())
        ? []
        : [USERNAME_HINT];

  const submit = (e: FormEvent) => {
    e.preventDefault();
    setTouched(true);
    if (errors.length) return;
    create.mutate();
  };

  const link = created ? inviteLink(created.code) : "";

  return (
    <Dialog open={open} onOpenChange={change}>
      <DialogContent>
        {created ? (
          <>
            <DialogHeader>
              <DialogTitle>Invitation created</DialogTitle>
              <DialogDescription>
                Send this link to {created.invitation.email || created.invitation.username}. It works once and expires{" "}
                {relativeTime(created.invitation.expires_at)}.
              </DialogDescription>
            </DialogHeader>
            <Alert>
              <MailPlus aria-hidden="true" />
              <AlertTitle>W0rkTree does not send email</AlertTitle>
              <AlertDescription>Share the link yourself. It cannot be shown again.</AlertDescription>
            </Alert>
            <div className="flex items-center gap-2">
              <Label htmlFor={`${id}-link`} className="sr-only">
                Invitation link
              </Label>
              <Input id={`${id}-link`} readOnly value={link} className="font-mono text-xs" onFocus={(e) => e.target.select()} />
              <CopyButton value={link} label="Copy invitation link" copiedMessage="Link copied" />
            </div>
            <DialogFooter>
              <Button onClick={() => change(false)}>Done</Button>
            </DialogFooter>
          </>
        ) : (
          <>
            <DialogHeader>
              <DialogTitle>Invite to {tenant}</DialogTitle>
              <DialogDescription>The invitee signs in (or registers) and accepts to join with the chosen role.</DialogDescription>
            </DialogHeader>
            <form onSubmit={submit} className="flex flex-col gap-4" noValidate>
              <div className="grid gap-2">
                <span id={`${id}-target`} className="text-sm leading-none font-medium">
                  Invite by
                </span>
                <ToggleGroup
                  type="single"
                  variant="outline"
                  value={target}
                  onValueChange={(t) => t && setTarget(t as Target)}
                  aria-labelledby={`${id}-target`}
                  className="w-fit"
                >
                  <ToggleGroupItem value="email" className="data-[state=on]:bg-foreground data-[state=on]:text-background">
                    <Mail aria-hidden="true" />
                    Email
                  </ToggleGroupItem>
                  <ToggleGroupItem value="username" className="data-[state=on]:bg-foreground data-[state=on]:text-background">
                    <UserRound aria-hidden="true" />
                    Username
                  </ToggleGroupItem>
                </ToggleGroup>
              </div>
              <div className="grid gap-2">
                <Label htmlFor={`${id}-value`}>{target === "email" ? "Email" : "Username"}</Label>
                <Input
                  id={`${id}-value`}
                  type={target === "email" ? "email" : "text"}
                  value={value}
                  onChange={(e) => setValue(e.target.value)}
                  placeholder={target === "email" ? "jane@example.com" : "jane"}
                  autoComplete="off"
                  spellCheck={false}
                  aria-invalid={touched && errors.length > 0}
                />
                {touched && <FieldErrors messages={errors} />}
              </div>
              <div className="grid gap-4 sm:grid-cols-2">
                <div className="grid gap-2">
                  <Label htmlFor={`${id}-role`}>Role</Label>
                  <RoleSelect id={`${id}-role`} tenant={tenant} value={role} onChange={setRole} disabledRoles={["owner"]} />
                </div>
                <div className="grid gap-2">
                  <Label htmlFor={`${id}-team`}>Team (optional)</Label>
                  <Select value={team} onValueChange={setTeam}>
                    <SelectTrigger id={`${id}-team`} className="w-full">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value={NO_TEAM}>No team</SelectItem>
                      {(teams.data ?? []).map((t) => (
                        <SelectItem key={t.id} value={t.slug}>
                          {t.name || t.slug}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>
              </div>
              <FormError error={create.error} />
              <DialogFooter>
                <DialogClose asChild>
                  <Button type="button" variant="outline">
                    Cancel
                  </Button>
                </DialogClose>
                <Button type="submit" disabled={create.isPending}>
                  {create.isPending ? "Creating…" : "Create invitation"}
                </Button>
              </DialogFooter>
            </form>
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}
