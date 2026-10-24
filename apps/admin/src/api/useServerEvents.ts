import { useEffect, useEffectEvent, useState } from "react";
import { useAuth } from "@/auth/context";
import { useApi } from "./context";
import { EventStream, type EventFilter, type StreamStatus } from "./events";
import type { ServerEvent } from "./types";

/**
 * Subscribes to the server's SSE stream for the lifetime of the component.
 * Reconnects automatically; returns the connection status.
 */
export function useServerEvents(filter: EventFilter, onEvent: (e: ServerEvent) => void): StreamStatus {
  const api = useApi();
  const { token } = useAuth();
  const [status, setStatus] = useState<StreamStatus>("connecting");
  const handle = useEffectEvent(onEvent);
  const { tenant, tree } = filter;

  useEffect(() => {
    if (!token) return;
    const stream = new EventStream({
      baseUrl: api.client.baseUrl,
      filter: { tenant, tree },
      getToken: () => api.client.token,
      onEvent: (e) => handle(e),
      onStatus: setStatus,
    });
    stream.start();
    return () => stream.stop();
  }, [api, token, tenant, tree]);

  return status;
}
