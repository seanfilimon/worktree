import { Bell, CheckCheck } from "lucide-react";
import { useNavigate } from "react-router";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { relativeTime } from "@/lib/format";
import { reasonLabel, treeOfUrl, useInboxActions, useNotifications, type Notification } from "@/pages/notifications/api";

/** How many unread notifications the menu previews. */
const PREVIEW = 6;
/** Fallback refresh when the event stream is down (ms). */
const POLL_MS = 60_000;

function countLabel(n: number): string {
  return n > 99 ? "99+" : String(n);
}

/**
 * Top-bar bell (§15.3): the unread count and a preview of the newest
 * unread notifications. It refreshes on `notification.created` (via the
 * shell's event stream) and once a minute. A server without notifications
 * just shows the bell.
 */
export function NotificationBell() {
  const unread = useNotifications(true, PREVIEW, { refetchInterval: POLL_MS });
  const { read, readAll } = useInboxActions();
  const navigate = useNavigate();
  const count = unread.data?.unread_count ?? 0;
  const items = unread.data?.notifications ?? [];
  const label = count > 0 ? `Notifications (${count} unread)` : "Notifications";

  const open = (n: Notification) => {
    read.mutate(n.id);
    void navigate(n.url || "/notifications");
  };

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="ghost" size="icon" className="relative size-8" aria-label={label} title={label}>
          <Bell aria-hidden="true" />
          {count > 0 && (
            <span
              aria-hidden="true"
              data-testid="notification-count"
              className="absolute -top-0.5 -right-0.5 flex h-4 min-w-4 items-center justify-center rounded-full bg-foreground px-1 text-[10px] leading-none font-semibold text-background tabular-nums"
            >
              {countLabel(count)}
            </span>
          )}
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-80">
        <div className="flex items-center justify-between gap-2 pr-1">
          <DropdownMenuLabel>Notifications</DropdownMenuLabel>
          {count > 0 && (
            <Button variant="ghost" size="sm" className="h-7 gap-1 px-2 text-xs" onClick={() => readAll.mutate()} disabled={readAll.isPending}>
              <CheckCheck aria-hidden="true" />
              Mark all read
            </Button>
          )}
        </div>
        <DropdownMenuSeparator />
        {items.length === 0 ? (
          <p className="px-2 py-6 text-center text-sm text-muted-foreground">{unread.isError ? "Notifications are unavailable." : "You're all caught up."}</p>
        ) : (
          items.map((n) => (
            <DropdownMenuItem key={n.id} onSelect={() => open(n)} className="flex flex-col items-start gap-0.5 py-2">
              <span className="line-clamp-2 text-sm font-medium">{n.title}</span>
              <span className="text-xs text-muted-foreground">
                {reasonLabel(n.reason)}
                {treeOfUrl(n.url) ? ` · ${treeOfUrl(n.url)}` : ""} · {relativeTime(n.created_at)}
              </span>
            </DropdownMenuItem>
          ))
        )}
        <DropdownMenuSeparator />
        <DropdownMenuItem onSelect={() => void navigate("/notifications")} className="justify-center text-sm">
          View all notifications
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
