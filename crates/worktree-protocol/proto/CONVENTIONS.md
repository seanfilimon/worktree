# Protobuf Schema Conventions

Project-wide conventions for `.proto` files in `crates/worktree-protocol/proto/`.
Applies to `sync.proto` today and any future schema files (BranchService,
TagService, MergeRequestService, AuthService, etc.).

These are codified to prevent re-litigating in every PR. Updates require a
ticket and Sean's sign-off.

---

## 1. Field numbering

- **Start at `1`** per message.
- **Hot-path fields** (frequently accessed) get tags `1`–`15`. Protobuf's
  varint encoding makes tags 1–15 single-byte; tags 16+ are at least 2 bytes.
- **Reserve `100 to 199`** at the bottom of every message for additive
  future expansion. Gives 99 slots for new fields without colliding with
  the hot-path range.
- **Reserve removed tags** explicitly: when a field is deleted, replace
  with `reserved <tag>, "<name>";` to prevent accidental reuse. See
  protobuf docs on schema evolution.

## 2. Optional fields

- Rust `Option<T>` → proto3 `optional T` (proto3 re-enabled `optional`
  in 2020 for presence detection).
- `tonic-build` (Rust codegen) emits `Option<T>` for `optional` scalar
  fields and `Option<MessageType>` for messages (messages always carry
  presence).
- `protoc-gen-go` emits `*T` for `optional` scalars and pointer types
  for messages by default.

## 3. Enum patterns

### Unit-variant Rust enums → proto `enum`

Rust enums where every variant is a unit (no payload) map cleanly:

```rust
pub enum AccessConfigType {
    Roles,
    Policies,
    TenantAccess,
    BranchProtection,
    License,
}
```

→

```proto
enum AccessConfigType {
  ACCESS_CONFIG_TYPE_UNSPECIFIED = 0;
  ACCESS_CONFIG_TYPE_ROLES = 1;
  ACCESS_CONFIG_TYPE_POLICIES = 2;
  ACCESS_CONFIG_TYPE_TENANT_ACCESS = 3;
  ACCESS_CONFIG_TYPE_BRANCH_PROTECTION = 4;
  ACCESS_CONFIG_TYPE_LICENSE = 5;
}
```

The `_UNSPECIFIED = 0;` first value is **mandatory** in proto3 — it's the
default for the field.

### Payload-carrying Rust enums (sum types) → proto `oneof`

Rust enums whose variants carry data — like `PushRejection` — cannot use
proto `enum` (which is just an integer tag). Use `message` + `oneof`:

```rust
pub enum PushRejection {
    ConflictDetected { server_tip: SnapshotId },
    BranchProtection { rule: String },
    AccessDenied { reason: String },
    // ...
}
```

→

```proto
message PushRejection {
  oneof reason {
    ConflictDetected conflict_detected = 1;
    BranchProtection branch_protection = 2;
    AccessDenied access_denied = 3;
    // ...
  }

  message ConflictDetected {
    SnapshotId server_tip = 1;
  }
  message BranchProtection {
    string rule = 1;
  }
  message AccessDenied {
    string reason = 1;
  }
  // ...
}
```

Each variant becomes its own nested message; the outer `oneof` switches
between them. Tonic generates a Rust enum; protoc-gen-go generates a Go
interface with each variant as a struct.

## 4. ID-type representation

**All ID types are wrapped single-field messages**, not bare scalars.
This trades a tiny wire-size overhead for cross-language type safety
(Go compiler refuses to swap `TenantId` and `TreeId`).

```proto
message TenantId   { string value = 1; }
message TreeId     { string value = 1; }
message BranchId   { string value = 1; }
message SnapshotId { string value = 1; }
message AccountId  { string value = 1; }

message ContentHash { bytes value = 1; }  // BLAKE3 — 32 bytes
```

String-backed for UUID IDs (matches their Rust `Display` impl).
Bytes-backed for hash content.

## 5. Timestamps

Rust `chrono::DateTime<Utc>` → `google.protobuf.Timestamp`.

```proto
import "google/protobuf/timestamp.proto";

message SyncState {
  google.protobuf.Timestamp last_sync = 1;
  // ...
}
```

Both tonic (Rust) and protoc-gen-go (Go) have native well-known type
support. No manual conversion shims needed.

## 6. Versioning

- **Package versioning**: `worktree.<service>.v1` for v1, `worktree.<service>.v2`
  for v2.
- **Additive changes** (new field with new tag, new enum value, new RPC)
  stay in `v1`. Always safe.
- **Breaking changes** (rename a field's wire identity, change a field's
  type, remove a field, renumber an enum value) trigger a v2 in a sibling
  file with new package name. v1 and v2 coexist during migration; v1 gets
  a `// Deprecated: use vX.` comment and eventually a `WT-PROTO-` ticket
  to remove.
- **Buf lint** + **buf breaking** enforce the contract automatically (see
  `buf.yaml` config).

## 7. Reserved tag declaration (defensive)

Every message ends with:

```proto
message Foo {
  // ... fields ...

  reserved 100 to 199;
}
```

This makes additive evolution safe by default.

## 8. RPC naming

- gRPC service methods use **PascalCase verbs**: `Push`, `Pull`,
  `Stage`, `Negotiate`. Match the action they perform.
- Bidirectional / streaming RPCs (when introduced — `WT-PROTO-3`) name
  the verb in a way that makes the streaming nature clear:
  `StreamChunks`, `WatchEvents`, etc.

## 9. Field naming

- Use `lower_snake_case` for proto field names. tonic + protoc-gen-go
  translate to language conventions (`lowerSnakeCase` stays in Go; Rust
  generates `lower_snake_case` fields).

## 10. Comments

Every message and field should have a leading `//` comment matching the
doc comments on the Rust domain types. Helps reviewers + downstream
language code-readers understand intent without bouncing back to Rust.
