import { createContext, useContext } from "react";
import type { StreamStatus } from "@/api/events";

export const LiveStatusContext = createContext<StreamStatus>("closed");

/** Status of the app-wide event stream (shown in the header bar). */
export function useLiveStatus(): StreamStatus {
  return useContext(LiveStatusContext);
}
