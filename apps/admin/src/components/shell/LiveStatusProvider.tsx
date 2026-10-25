import type { ReactNode } from "react";
import { useServerEvents } from "@/api/useServerEvents";
import { useAuth } from "@/auth/context";
import { LiveStatusContext } from "./liveStatus";

const ignore = () => undefined;

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
  const status = useServerEvents({}, ignore);
  return <LiveStatusContext.Provider value={token ? status : "closed"}>{children}</LiveStatusContext.Provider>;
}
