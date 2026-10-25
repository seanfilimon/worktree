import type { AuditEvent, ServerEvent, SystemInfo } from "@/api/types";
import { bob, summary, tenant } from "./fixtures";

const tree = summary.tree;
const ago = (minutes: number) => new Date(Date.now() - minutes * 60_000).toISOString();

/** A /metrics scrape in the shape services/server-go emits. */
export function metricsText({ requests = 100, errors = 2, sum = 1.5, buckets = [60, 90, 98, 100] } = {}): string {
  const [b1, b2, b3, b4] = buckets;
  return `# HELP worktree_http_requests_total Total HTTP requests by route and status code.
# TYPE worktree_http_requests_total counter
worktree_http_requests_total{route="GET /api/v1/tenants",code="200"} ${requests - errors}
worktree_http_requests_total{route="GET /api/v1/tenants",code="500"} ${errors}
# HELP worktree_http_request_duration_seconds HTTP request latency by route.
# TYPE worktree_http_request_duration_seconds histogram
worktree_http_request_duration_seconds_bucket{route="GET /api/v1/tenants",le="0.005"} ${b1}
worktree_http_request_duration_seconds_bucket{route="GET /api/v1/tenants",le="0.01"} ${b2}
worktree_http_request_duration_seconds_bucket{route="GET /api/v1/tenants",le="0.1"} ${b3}
worktree_http_request_duration_seconds_bucket{route="GET /api/v1/tenants",le="+Inf"} ${b4}
worktree_http_request_duration_seconds_sum{route="GET /api/v1/tenants"} ${sum}
worktree_http_request_duration_seconds_count{route="GET /api/v1/tenants"} ${requests}
# HELP worktree_http_requests_in_flight HTTP requests currently being served.
# TYPE worktree_http_requests_in_flight gauge
worktree_http_requests_in_flight 1
# HELP worktree_build_info Build information; the value is always 1.
# TYPE worktree_build_info gauge
worktree_build_info{version="2.0.0-test",goversion="go1.25.0"} 1
# HELP go_goroutines Number of goroutines that currently exist.
# TYPE go_goroutines gauge
go_goroutines 42
`;
}

export const systemInfo: SystemInfo = {
  version: "2.0.0-test",
  go_version: "go1.25.0",
  os: "linux",
  arch: "amd64",
  started_at: ago(90),
  uptime_seconds: 5400,
  allow_registration: true,
  data_dir: "/var/lib/worktree",
  num_cpu: 8,
  num_goroutine: 42,
  memory: { alloc_bytes: 64 * 1024 * 1024, sys_bytes: 256 * 1024 * 1024, heap_inuse_bytes: 80 * 1024 * 1024, gc_runs: 17 },
  storage: { objects: 98765, object_bytes: 5 * 1024 * 1024 * 1024, meta_db_bytes: 32 * 1024 * 1024 },
  event_bus: { published: 1200, delivered: 3400, dropped: 3, subscribers: 2 },
};

/** GET /api/v1/events/recent (oldest first). */
export const recentEvents: ServerEvent[] = [
  { type: "tree.created", tenant_id: tenant.id, actor: "alice", payload: { name: "docs" }, at: ago(20) },
  { type: "branch.pushed", tenant_id: tenant.id, tree_id: tree.id, actor: "bob", payload: { tree: "web", branch: "main" }, at: ago(5) },
];

export const auditEvents: AuditEvent[] = [
  {
    id: "e2",
    tenant_id: tenant.id,
    actor_id: bob.id,
    actor: "bob",
    action: "branch.push",
    resource_type: "branch",
    resource_id: `${tree.id}/main`,
    details: { tree: "web", branch: "main", count: "2" },
    remote_addr: "10.0.0.2",
    at: ago(3),
  },
  {
    id: "e1",
    tenant_id: tenant.id,
    actor_id: "acc-1",
    actor: "alice",
    action: "auth.login",
    resource_type: "account",
    resource_id: "acc-1",
    remote_addr: "127.0.0.1",
    at: ago(30),
  },
];
