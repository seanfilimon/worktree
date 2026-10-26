import { useQueryClient } from "@tanstack/react-query";
import type { ReactNode } from "react";
import type { ServerEvent } from "@/api/types";
import { useServerEvents } from "@/api/useServerEvents";
import { useAuth } from "@/auth/context";
import { LiveStatusContext } from "./liveStatus";

/**
 * Keeps one unscoped event stream open while signed in so the header can
 * show whether the server connection is live. Pages subscribe separately
 * (with their own filters) through `useServerEvents`.
 *
 * Mounted above the router so the stream starts as soon as a token exists,
 * before any page subscribes.
 */
export function LiveStatusProvider({ children }: { children: ReactNode }) {
  const { token } = useAuth();
  // ---- notifications (§15.3): the account's notification.created events
  // arrive on this unscoped stream; refresh the bell and inbox.
  const queryClient = useQueryClient();
  const onEvent = (e: ServerEvent) => {
    if (e.type === "notification.created") void queryClient.invalidateQueries({ queryKey: ["notifications"] });
  };
  // ---- end notifications ----
  const status = useServerEvents({}, onEvent);
  return <LiveStatusContext.Provider value={token ? status : "closed"}>{children}</LiveStatusContext.Provider>;
}
