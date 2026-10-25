import { useQuery } from "@tanstack/react-query";
import { useApi } from "@/api/context";

/** Query key of the raw Prometheus scrape (GET /metrics). */
export const metricsKey = ["metrics"] as const;

/** Raw /metrics text, polled every 15 s while enabled (server admins). */
export function useMetricsText(enabled: boolean) {
  const api = useApi();
  return useQuery({
    queryKey: metricsKey,
    queryFn: api.system.metrics,
    enabled,
    refetchInterval: 15_000,
    staleTime: 0,
  });
}
