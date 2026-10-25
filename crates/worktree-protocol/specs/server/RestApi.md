# REST API

The W0rkTree server exposes a JSON REST API under `/api/v1` on the same port
as the gRPC sync service. It serves the admin panel and every server-side
`wt` command (`wt tenant`, `wt mr`, `wt policy`, ...). This spec describes
the conventions shared by all routes. The authoritative per-route
description is the OpenAPI 3.1 document
`services/server-go/internal/httpapi/openapi.yaml`, which every server
serves at `GET /api/v1/openapi.yaml`.

## Table of Contents

1. [The OpenAPI document](#1-the-openapi-document)
2. [Requests and responses](#2-requests-and-responses)
3. [Authentication](#3-authentication)
4. [Authorization and `x-wt-action`](#4-authorization-and-x-wt-action)
5. [Errors](#5-errors)
6. [Lists, limits and pagination](#6-lists-limits-and-pagination)
7. [Versioning and compatibility](#7-versioning-and-compatibility)
8. [`wt api`](#8-wt-api)
9. [Keeping the document in sync](#9-keeping-the-document-in-sync)
10. [Implementation status](#10-implementation-status)
11. [Roadmap](#11-roadmap)
12. [References](#12-references)

## 1. The OpenAPI document

- `openapi.yaml` describes every registered route, including the
  operational ones outside the prefix (`GET /healthz`, `GET /metrics`). The
  static admin SPA (`/admin/*`) is not part of the API.
- It is embedded in the server binary with `go:embed` and served at
  `GET /api/v1/openapi.yaml` (`Content-Type: application/yaml`, no
  authentication, `Cache-Control: public, max-age=300`).
- Paths are absolute (`servers: [{url: /}]`), so a client resolves them
  against whichever server served the document.
- Each operation has an `operationId`, a summary, tags, parameters, request
  body, responses with exact status codes, and the `x-wt-action` extension
  (§4).
- Component schemas carry the Go JSON field names. Domain types
  (`internal/domain`) and the handlers' request and response types have one
  schema each (`Tenant`, `TreeSummary`, `CompareResult`, ...). A field that
  Go encodes even when it is `nil` (a slice, map or pointer without
  `omitempty`) is typed `[<type>, "null"]`. Every non-`omitempty` field of a
  response type is `required`. Request schemas (names ending in `Request`)
  list no required properties, because the decoder does not demand them.
  Each handler validates its own input.
- Route wildcards that match several segments (`{name...}` in Go, e.g.
  branch names with `/`) are written `{name}` and their description says so.

## 2. Requests and responses

- Bodies are JSON (`application/json`; `+json` media types are accepted).
  A request body is limited to 1 MiB (`413 payload_too_large`), must hold a
  single JSON value, and must not contain unknown fields (`400 bad_request`
  naming the field). A `Content-Type` other than JSON answers
  `415 unsupported_media_type`. Raw uploads (release assets) are the
  exception: they take any media type, up to 64 MiB.
- Field names are snake_case, exactly the `json` tags of the Go types.
  Timestamps are RFC 3339 in UTC. Hashes are lowercase hex BLAKE3-256, and
  `[]byte` fields are base64.
- List endpoints answer a JSON array (or an object that wraps one, such as
  `{"todos":[...]}`). Top-level lists are `[]` rather than `null` when empty.
- Creating answers `201` with the new resource. Deleting answers `204` with
  no body. Updates answer `200` with the updated resource. `PATCH` bodies
  hold only the fields to change. `PUT` replaces the whole resource or
  creates it.
- Non-JSON responses are blobs and release assets (`application/octet-stream`
  or the stored type, with `ETag`, `Range` and conditional-request support),
  archives (`application/gzip`, `application/zip`), the audit NDJSON export
  (`application/x-ndjson`), the SSE stream (`text/event-stream`), Prometheus
  metrics (`text/plain`) and this document (`application/yaml`).
- Every response carries `X-Request-Id`. A well-formed incoming id is echoed
  back; otherwise the server generates one. Browser origins
  `http://localhost:5173` and `WT_CORS_ORIGINS` are allowed by CORS.

## 3. Authentication

- `Authorization: Bearer <token>`. The token can be a session token
  (`POST /auth/login`, then `/auth/login/mfa` when MFA is on), a personal
  API token (`POST /auth/tokens`, optionally scoped) or a service-account
  key. The document declares this as the `bearerAuth` security scheme.
- `GET` requests may instead pass `?access_token=<token>`. `EventSource`
  cannot set headers, so the SSE stream relies on this. The document
  declares it as `accessTokenQuery`.
- A token that is present but invalid answers `401 unauthenticated` (with
  `WWW-Authenticate: Bearer realm="worktree"`) on every route, public ones
  included. Without a token, the request is anonymous. Anonymous callers can
  read public trees and use the public routes (registration, login,
  invitation preview, password reset, SSO, `iam/actions`, health, this
  document).
- The document's global security is `bearerAuth`. Operations that also work
  anonymously declare `security: [{}, {bearerAuth: []}]`.

## 4. Authorization and `x-wt-action`

The handlers authorize through `domain.Authorizer` with an action from the
IAM catalogue (`GET /api/v1/iam/actions`). The same evaluation applies as
for gRPC: token scopes, roles, grants, visibility and policies (see
[IAM](../iam/IAM.md)). `x-wt-action` names the action an operation checks:

| Value | Meaning |
|---|---|
| `tree:read`, `iam:admin`, `mr:merge`, ... | The catalogue action checked on the route's tenant or tree. |
| `public` | No check. |
| `authenticated` | Any signed-in principal. |
| `self` | The caller's own account (tokens, sessions, MFA, keys). |
| `server:admin` | Server administrators only. |

When a route checks several actions (for example `tree:read` to see a
merge request and `mr:approve` to review it), the value is a list. The
operation description explains when each one applies. A refusal answers
`403 forbidden` (or a feature-specific code) and is audited as
`access.denied`.

## 5. Errors

Every error uses one envelope:

```json
{"error": {"code": "not_found", "message": "tree acme/web not found"}}
```

`code` is stable and machine-readable. `message` is for humans and may
change. The document's `components.responses` holds the common error
responses (`BadRequest`, `Unauthorized`, `Forbidden`, `NotFound`,
`Conflict`, `PayloadTooLarge`, `UnsupportedMediaType`,
`UnavailableForLegalReasons`, `NotImplemented`, ...). Operations list
feature-specific codes such as `path_denied` (403), `license_restricted`
(451), `merge_blocked` / `managed_by_config` / `quota_exceeded` /
`last_owner` (409) and `gc_running` (409). Every operation can also answer
`429 rate_limited` (with `Retry-After`, `X-RateLimit-Limit` and
`X-RateLimit-Remaining`) and `500 internal`, which carries no detail; the
cause is logged with the request id. An unknown path under `/api/` answers
`404 not_found`. A known path with the wrong method answers
`405 method_not_allowed` with an `Allow` header. Features whose service is
not configured answer `501 not_implemented`. The full table is in
[Server § Error model](./Server.md#17-error-model).

## 6. Lists, limits and pagination

There are no page cursors. List endpoints that can grow without bound take
`limit` (a positive integer; values above the maximum are clamped, and zero
or garbage answers 400). Snapshots, file history and the audit log are
returned newest first; `/events/recent` returns the last `limit` events
oldest first:

| Endpoint | Default | Maximum |
|---|---|---|
| `GET {tree}/snapshots` | 50 | 1000 |
| `GET {tree}/history` | 50 | 1000 |
| `GET /audit` | 100 | 1000 (`format=ndjson` streams every match) |
| `GET /events/recent` | 50 | 200 |
| `GET /search` | 20 | 50 |
| `GET /search/code` | 50 | 1000 (`truncated` marks a cut result) |
| `GET .../webhooks/{id}/deliveries` | 50 | 200 |

To page further back, narrow the query with the endpoint's filters, such as
`until=` on `/audit` or `branch=` on snapshots. Small collections (members,
teams, roles, policies, branches, tags) are returned whole.

## 7. Versioning and compatibility

- The path prefix `/api/v1` is the major version. Within v1, changes are
  additive only: new routes, new optional request fields, new response
  fields, new error codes. Clients must ignore unknown response fields.
  Request decoding rejects unknown fields, so a client must not send fields
  that the target server version does not know.
- A breaking change would ship a new prefix (`/api/v2`) next to v1.
- `info.version` of the document is the W0rkTree release it describes.
  `GET /healthz` reports the running server version.

## 8. `wt api`

`wt api METHOD PATH [-d JSON|@FILE|@-] [-H 'Name: value']... [--raw]`
makes raw authenticated calls:

- The server is resolved like every server-side command: `--server` /
  `WT_SERVER`, else the worktree's origin, else the only saved login. The
  request carries that server's saved token (or `WT_TOKEN`). An explicit
  `-H 'Authorization: ...'` overrides it.
- A `PATH` that does not start with `/` is relative to `/api/v1/`
  (`wt api GET tenants`). It may include a query string. Full URLs are
  refused.
- `-d` sends a body: inline text, `@file` or `@-` (stdin). Without a
  `Content-Type` header, the body must be valid JSON and is sent as
  `application/json`. With one, the bytes are sent as given, which is how
  release assets are uploaded.
- JSON responses are pretty-printed with their key order and number
  spelling kept. Other bodies, and every body with `--raw`, are written
  unchanged. A `204` prints nothing (`null` with `--json`).
- A non-2xx response exits 1 and prints `HTTP <status> <code>: <message>`
  from the error envelope on stderr. Usage errors exit 2 before any
  request is sent.

## 9. Keeping the document in sync

`services/server-go/internal/httpapi/openapi_test.go` holds the guards:

- **`TestOpenAPICoverage`** records every pattern registered through
  `api.handle` (`api.patterns`), normalizes `{x...}` to `{x}`, and compares
  the `METHOD /path` set with the document's operations in both directions.
  A route the document lacks, or a documented operation the router does not
  serve, fails the test.
- **`TestOpenAPIDocumentValid`** checks that the YAML parses and is 3.1, and
  that every `$ref` resolves. It also requires unique `operationId`s,
  declared tags, `x-wt-action` on every operation, at least one 2xx/3xx
  response, known security schemes, and path parameters that match the path
  template.
- **`TestOpenAPISchemasMatchGoTypes`** checks the component schemas listed
  in `openAPITypes` (`openapi_schema_test.go`) against the Go types by
  reflection: the property names must be exactly the JSON members, and for
  response types `required` must be exactly the non-`omitempty` members.
- `WT_OPENAPI_GEN=<file> go test -run TestGenerateOpenAPISchemas
  ./internal/httpapi` writes the schemas of every registered type, with
  descriptions taken from the Go doc comments, as a starting point for new
  components.

To add a route, register it, add the operation to `openapi.yaml` (and its
types to `openAPITypes` when it introduces new ones), then run
`go test ./internal/httpapi`.

## 10. Implementation status

| Item | Status |
|---|---|
| OpenAPI 3.1 document of every registered route, served at `GET /api/v1/openapi.yaml` | Built |
| Error envelope, bearer and `access_token` authentication, `x-wt-action` per operation | Built |
| Router ↔ document coverage test, `$ref`/structure test, schema ↔ Go type test | Built |
| `wt api` | Built |
| Routes of the v0.6 workstreams (webhooks, code search, blame, fsck, policy history, server-side merges, staged file browsing) | Partial: added to the document as their routes land |

## 11. Roadmap

- Generated client SDKs (TypeScript for the admin panel, Rust) from the
  document.
- Cursor-based pagination (`next` links) for the audit log and snapshot
  history.
- A JSON rendering of the document (`openapi.json`) and an interactive
  explorer page in the admin panel.
- Response examples per operation.

## 12. References

- `services/server-go/internal/httpapi/openapi.yaml` — the document
- `services/server-go/internal/httpapi/openapi.go` — embedding and the route
- `services/server-go/internal/httpapi/openapi_test.go`,
  `openapi_schema_test.go` — coverage and schema tests, generator
- `services/server-go/internal/httpapi/httpapi.go` (`routes`, `handle`),
  `respond.go` (envelope, codes), `middleware.go` (request ids, CORS, rate
  limits), `internal/iam/middleware.go` (token parsing)
- `crates/worktree-cli/src/commands/api.rs` — `wt api`
- `docs/ENGINEERING_PLAN.md` §7, §11.1, §12.3, §13, §14.10
- Related specs: [Server](./Server.md), [IAM](../iam/IAM.md)
