/** v0.6 fixtures (docs/ENGINEERING_PLAN.md §14.9). */
import type { CodeSearchResult, PolicyVersion, Webhook, WebhookDelivery } from "@/api/v06";
import { tenant } from "./fixtures";
import { policy } from "./fixtures.iam";

const ago = (minutes: number) => new Date(Date.now() - minutes * 60_000).toISOString();

export const webhook: Webhook = {
  id: "wh-1",
  tenant_id: tenant.id,
  url: "https://ci.example.com/hooks/wt",
  description: "Trigger CI",
  events: ["branch.pushed", "mr.*"],
  active: true,
  has_secret: true,
  created_by: "alice",
  created_at: ago(600),
  updated_at: ago(60),
  last_delivery_at: ago(5),
  last_status: 200,
  failure_count: 0,
};

export const failingWebhook: Webhook = {
  ...webhook,
  id: "wh-2",
  url: "https://chat.example.com/notify",
  description: "",
  events: ["*"],
  active: false,
  has_secret: false,
  last_status: 502,
  failure_count: 20,
};

export const delivery: WebhookDelivery = {
  id: "dl-1",
  webhook_id: "wh-1",
  event: "branch.pushed",
  payload: { id: "ev-1", event: "branch.pushed", tenant: "acme", tree: "web" },
  attempts: 1,
  status_code: 200,
  response: "queued build 42",
  duration_ms: 87,
  succeeded: true,
  created_at: ago(5),
  delivered_at: ago(5),
};

export const failedDelivery: WebhookDelivery = {
  ...delivery,
  id: "dl-2",
  event: "mr.opened",
  status_code: 0,
  response: undefined,
  error: "dial tcp: connection refused",
  succeeded: false,
  attempts: 3,
  next_retry_at: new Date(Date.now() + 30 * 60_000).toISOString(),
  created_at: ago(30),
};

export const codeResults: CodeSearchResult = {
  hits: [
    {
      tenant: "acme",
      tree: "web",
      branch: "main",
      snapshot_id: "snap-3",
      path: "src/app.ts",
      line: 2,
      text: "const greeting = 'hello';",
      before: ["// entry point"],
      after: ["export default greeting;"],
    },
    {
      tenant: "acme",
      tree: "web",
      branch: "main",
      snapshot_id: "snap-3",
      path: "src/app.ts",
      line: 9,
      text: "log(greeting)",
      before: null as unknown as string[],
      after: [],
    },
    {
      tenant: "acme",
      tree: "api",
      branch: "trunk",
      snapshot_id: "api-snap-1",
      path: "server/greeting.go",
      line: 14,
      text: "func Greeting() string { return \"hello\" }",
    },
  ],
  truncated: false,
  searched_trees: 4,
};

export const policyVersions: PolicyVersion[] = [
  {
    tenant_id: tenant.id,
    policy_id: policy.id,
    version: 1,
    change: "create",
    policy: { ...policy, version: 1, statements: [{ ...policy.statements[0]!, sid: "fri" }] },
    changed_by: "alice",
    changed_at: ago(300),
  },
  {
    tenant_id: tenant.id,
    policy_id: policy.id,
    version: 2,
    change: "update",
    policy: { ...policy, version: 2, enabled: false, statements: [{ ...policy.statements[0]!, sid: "fri", actions: ["branch:push", "tag:push"] }] },
    changed_by: "bob",
    changed_at: ago(200),
    comment: "also block tags",
  },
  {
    tenant_id: tenant.id,
    policy_id: policy.id,
    version: 3,
    change: "update",
    policy: { ...policy, version: 3 },
    changed_by: "alice",
    changed_at: ago(100),
  },
];
