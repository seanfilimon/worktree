import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Bell, Check, CheckCheck, Inbox, Trash2 } from "lucide-react";
import { useId, useState } from "react";
import { Link, useNavigate } from "react-router";
import { PageHeader } from "@/components/PageHeader";
import { EmptyState, QueryError, TableSkeleton } from "@/components/people/states";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { absoluteTime, relativeTime } from "@/lib/format";
import { tenantPath, treePath } from "@/lib/paths";
import { cn } from "@/lib/utils";
import { useToast } from "@/toast/store";
import {
  isWatchLevel,
  REASON_LABEL,
  REASONS,
  reasonLabel,
  treeOfUrl,
  useInboxActions,
  useNotifications,
  useNotificationSettings,
  useNotificationsApi,
  useSetWatch,
  useWatches,
  WATCH_DESCRIPTION,
  WATCH_LABEL,
  WATCH_LEVELS,
  notificationKeys,
  type Notification,
  type WatchView,
} from "./api";

/** The page lists up to this many notifications (the server keeps 1000). */
const LIMIT = 200;

type Show = "unread" | "all";

function NotificationRow({ n, onOpen }: { n: Notification; onOpen: (n: Notification) => void }) {
  const { read, remove } = useInboxActions();
  const unread = !n.read_at;
  const tree = treeOfUrl(n.url);
  return (
    <TableRow data-state={unread ? "unread" : "read"}>
      <TableCell className="w-6 pr-0">
        <span
          aria-label={unread ? "Unread" : "Read"}
          className={cn("block size-2 rounded-full", unread ? "bg-foreground" : "border border-muted-foreground/40")}
        />
      </TableCell>
      <TableCell className="min-w-0">
        <button type="button" onClick={() => onOpen(n)} className={cn("text-left hover:underline", unread ? "font-semibold" : "text-muted-foreground")}>
          {n.title}
        </button>
        {n.body && <p className="line-clamp-2 text-xs whitespace-pre-line text-muted-foreground">{n.body}</p>}
      </TableCell>
      <TableCell className="hidden sm:table-cell">
        <Badge variant="outline">{reasonLabel(n.reason)}</Badge>
      </TableCell>
      <TableCell className="hidden font-mono text-xs md:table-cell">{tree ?? "–"}</TableCell>
      <TableCell className="text-xs whitespace-nowrap text-muted-foreground">
        <time dateTime={n.created_at} title={absoluteTime(n.created_at)}>
          {relativeTime(n.created_at)}
        </time>
      </TableCell>
      <TableCell className="text-right whitespace-nowrap">
        {unread && (
          <Button variant="ghost" size="icon" className="size-7" aria-label={`Mark "${n.title}" read`} onClick={() => read.mutate(n.id)} disabled={read.isPending}>
            <Check aria-hidden="true" />
          </Button>
        )}
        <Button variant="ghost" size="icon" className="size-7" aria-label={`Done with "${n.title}"`} onClick={() => remove.mutate(n.id)} disabled={remove.isPending}>
          <Trash2 aria-hidden="true" />
        </Button>
      </TableCell>
    </TableRow>
  );
}

function InboxCard() {
  const [show, setShow] = useState<Show>("unread");
  const [reason, setReason] = useState<string>("any");
  const list = useNotifications(show === "unread", LIMIT);
  const { read, readAll } = useInboxActions();
  const navigate = useNavigate();
  const toast = useToast();
  const items = (list.data?.notifications ?? []).filter((n) => reason === "any" || n.reason === reason);
  const unreadCount = list.data?.unread_count ?? 0;

  const open = (n: Notification) => {
    if (!n.read_at) read.mutate(n.id);
    if (n.url) void navigate(n.url);
  };

  return (
    <Card>
      <CardHeader className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
        <div className="space-y-1">
          <CardTitle className="flex items-center gap-2">
            <Inbox aria-hidden="true" className="size-4" />
            Inbox
            {unreadCount > 0 && <Badge variant="secondary">{unreadCount} unread</Badge>}
          </CardTitle>
          <CardDescription>Review requests, mentions, your merge requests, CI failures and what you watch.</CardDescription>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <ToggleGroup type="single" variant="outline" size="sm" value={show} onValueChange={(v) => v && setShow(v as Show)} aria-label="Show">
            <ToggleGroupItem value="unread">Unread</ToggleGroupItem>
            <ToggleGroupItem value="all">All</ToggleGroupItem>
          </ToggleGroup>
          <Select value={reason} onValueChange={setReason}>
            <SelectTrigger size="sm" className="w-44" aria-label="Reason">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="any">Every reason</SelectItem>
              {REASONS.map((r) => (
                <SelectItem key={r} value={r}>
                  {REASON_LABEL[r]}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Button
            variant="outline"
            size="sm"
            disabled={unreadCount === 0 || readAll.isPending}
            onClick={() => readAll.mutate(undefined, { onSuccess: (r) => toast.success("Marked all read", `${r?.updated ?? 0} notification(s)`) })}
          >
            <CheckCheck aria-hidden="true" />
            Mark all read
          </Button>
        </div>
      </CardHeader>
      <CardContent>
        {list.isError ? (
          <QueryError error={list.error} onRetry={() => void list.refetch()} title="Could not load notifications" />
        ) : list.isPending ? (
          <TableSkeleton label="Loading notifications" rows={4} columns={5} />
        ) : items.length === 0 ? (
          <EmptyState icon={Bell} title={show === "unread" ? "You're all caught up" : "No notifications"}>
            {show === "unread" ? "New review requests, mentions and activity on your merge requests show up here." : "Nothing matches this filter."}
          </EmptyState>
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead className="w-6 pr-0">
                  <span className="sr-only">State</span>
                </TableHead>
                <TableHead>Notification</TableHead>
                <TableHead className="hidden sm:table-cell">Reason</TableHead>
                <TableHead className="hidden md:table-cell">Tree</TableHead>
                <TableHead>When</TableHead>
                <TableHead className="text-right">
                  <span className="sr-only">Actions</span>
                </TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {items.map((n) => (
                <NotificationRow key={n.id} n={n} onOpen={open} />
              ))}
            </TableBody>
          </Table>
        )}
      </CardContent>
    </Card>
  );
}

function EmailSettings() {
  const id = useId();
  const settings = useNotificationSettings();
  const api = useNotificationsApi();
  const queryClient = useQueryClient();
  const toast = useToast();
  const save = useMutation({
    mutationFn: (email: boolean) => api.putSettings({ email }),
    onSuccess: async (s) => {
      toast.success(s.email ? "Email notifications on" : "Email notifications off");
      await queryClient.invalidateQueries({ queryKey: notificationKeys.settings });
    },
  });
  const s = settings.data;
  return (
    <Card>
      <CardHeader>
        <CardTitle>Email</CardTitle>
        <CardDescription>
          {s && !s.email_configured
            ? "This server has no SMTP server configured (--smtp-url), so no emails are sent."
            : "Emails go to the address of your account."}
        </CardDescription>
      </CardHeader>
      <CardContent>
        {settings.isError ? (
          <QueryError error={settings.error} onRetry={() => void settings.refetch()} title="Could not load settings" />
        ) : (
          <div className="flex items-start gap-3">
            <Switch
              id={`${id}-email`}
              checked={s?.email ?? false}
              disabled={!s || save.isPending}
              onCheckedChange={(on) => save.mutate(on)}
              aria-describedby={`${id}-email-desc`}
            />
            <div className="space-y-1">
              <Label htmlFor={`${id}-email`}>Email me about things that involve me</Label>
              <p id={`${id}-email-desc`} className="text-xs text-muted-foreground">
                Review requests, mentions, CI failures on your merge requests, TODOs assigned to you and activity on your merge requests. Emails for
                watched trees are set per tree.
              </p>
            </div>
          </div>
        )}
      </CardContent>
    </Card>
  );
}

function WatchRow({ w }: { w: WatchView }) {
  const set = useSetWatch();
  const toast = useToast();
  const scope = { tenant: w.tenant, tree: w.tree || undefined };
  const level = isWatchLevel(w.level) ? w.level : "participating";
  const name = w.tree ? `${w.tenant}/${w.tree}` : w.tenant;
  const update = (next: typeof level, email = w.email) =>
    set.mutate({ scope, level: next, email }, { onSuccess: () => toast.success(`${WATCH_LABEL[next]} ${name}`) });
  return (
    <TableRow>
      <TableCell>
        <Link to={w.tree ? treePath(w.tenant, w.tree) : tenantPath(w.tenant)} className="font-mono text-sm hover:underline">
          {name}
        </Link>
        {!w.tree && <span className="ml-2 text-xs text-muted-foreground">every tree</span>}
      </TableCell>
      <TableCell>
        <Select value={level} onValueChange={(v) => isWatchLevel(v) && update(v)} disabled={set.isPending}>
          <SelectTrigger size="sm" className="w-40" aria-label={`Watch level for ${name}`}>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {WATCH_LEVELS.map((l) => (
              <SelectItem key={l} value={l} title={WATCH_DESCRIPTION[l]}>
                {WATCH_LABEL[l]}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </TableCell>
      <TableCell>
        <Switch checked={w.email} onCheckedChange={(on) => update(level, on)} disabled={set.isPending} aria-label={`Email watching notifications for ${name}`} />
      </TableCell>
      <TableCell className="text-xs text-muted-foreground">
        <time dateTime={w.updated_at} title={absoluteTime(w.updated_at)}>
          {relativeTime(w.updated_at)}
        </time>
      </TableCell>
    </TableRow>
  );
}

function WatchSettings() {
  const watches = useWatches();
  return (
    <Card>
      <CardHeader>
        <CardTitle>Watching</CardTitle>
        <CardDescription>
          Trees and tenants you set a level for. Everything else is participating. Change a tree with the Watch button on its page.
        </CardDescription>
      </CardHeader>
      <CardContent>
        {watches.isError ? (
          <QueryError error={watches.error} onRetry={() => void watches.refetch()} title="Could not load watch settings" />
        ) : watches.isPending ? (
          <TableSkeleton label="Loading watch settings" rows={2} columns={4} />
        ) : watches.data.length === 0 ? (
          <p className="text-sm text-muted-foreground">You participate in every tree: only what involves you notifies you.</p>
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Tree or tenant</TableHead>
                <TableHead>Level</TableHead>
                <TableHead>Email</TableHead>
                <TableHead>Updated</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {watches.data.map((w) => (
                <WatchRow key={`${w.tenant_id}/${w.tree_id ?? ""}`} w={w} />
              ))}
            </TableBody>
          </Table>
        )}
      </CardContent>
    </Card>
  );
}

/** Your notifications inbox and watch settings (§15.3). */
export function NotificationsPage() {
  return (
    <div className="flex flex-col gap-6">
      <PageHeader title="Notifications" description="Your inbox, email preferences and watched trees." />
      <InboxCard />
      <div className="grid gap-6 lg:grid-cols-2">
        <EmailSettings />
        <WatchSettings />
      </div>
    </div>
  );
}
