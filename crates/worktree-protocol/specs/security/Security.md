# Security Specification

## Overview
Security is a foundational concern in W0rkTree, not an afterthought. Unlike Git (which has no auth, no encryption in its native protocol, and no access control), W0rkTree builds security into every layer: transport encryption, mandatory authentication, granular access control, license compliance as code theft prevention, audit logging, and secret scanning.

## Transport Security
- All bgprocess ↔ server communication encrypted
- Primary: QUIC (TLS 1.3 built into the protocol)
- Fallback: gRPC over HTTP/2 with TLS
- No unencrypted transport. No anonymous mode.
- Certificate pinning supported for enterprise deployments

## Authentication

### Initial Authentication
- OAuth2 device flow (recommended for interactive CLI)
- API key (for CI/CD, automation)
- CLI-initiated token exchange

### Token Management
- JWT tokens stored in .wt/identity/token
- Tokens have configurable expiry
- Refresh token flow for long-lived sessions
- BGProcess handles token refresh automatically

### Multi-Account Support
- Users can be members of multiple tenants
- `wt tenant switch <name>` to change active tenant context
- Each worktree can be associated with a different tenant

## Access Control
See IAM.md for full specification. Key security properties:
- 20+ atomic permissions across tree, branch, snapshot, sync, management, admin categories
- 5 built-in roles with superset hierarchy
- RBAC + ABAC engine
- Scope hierarchy: Global → Tenant → Tree → Branch → RegisteredPath
- Deny always beats Allow at same scope level
- .wt/access/ defines the permission ceiling — trees cannot expand

## License Compliance as Security
See LicenseCompliance.md. License compliance prevents code theft:
- Proprietary code cannot be exported, forked, or synced without explicit grant
- Server blocks unauthorized operations at the protocol level
- Even with full IAM permissions, license restrictions apply (dual enforcement)

## Secret Scanning
- BGProcess scans file content on snapshot creation
- Configurable patterns in .wt/config.toml:

```toml
[secret_scanning]
enabled = true
patterns = [
    "AKIA[0-9A-Z]{16}",           # AWS access keys
    "sk_live_[a-zA-Z0-9]{24}",    # Stripe secret keys
    "ghp_[a-zA-Z0-9]{36}",        # GitHub personal access tokens
    "-----BEGIN.*PRIVATE KEY-----", # Private keys
]
block_on_match = true              # Prevent snapshot if secret detected
```

- If a secret is detected and block_on_match = true, the snapshot is aborted with an error
- False positives can be excluded per-file

### Detection Pipeline
1. BGProcess intercepts snapshot creation
2. All staged file content is scanned against configured patterns
3. Matches are reported with file path, line number, and pattern name
4. If block_on_match is enabled, the snapshot is rejected
5. Users can override with `--skip-secret-scan` (if they have the permission)

### Custom Pattern Support
Teams can define custom patterns for their own secrets:
- Database connection strings
- Internal API tokens
- Service account credentials
- Custom regex patterns with named groups

## Snapshot Signing
- Snapshots can be cryptographically signed
- Ed25519 signatures
- Signing key stored in .wt/identity/identity.toml or user global config
- Branch protection can require signed snapshots (require_snapshot_signature)
- Server verifies signatures on push

### Signing Flow
1. User creates a snapshot
2. BGProcess computes the snapshot hash
3. BGProcess signs the hash with the user's Ed25519 private key
4. Signature is attached to the snapshot metadata
5. On push, server verifies the signature against the user's registered public key

### Key Management
- Keys generated via `wt identity generate`
- Public key registered with the server via `wt identity register`
- Private key never leaves the local machine
- Key rotation supported: old signatures remain valid, new snapshots use new key

## Audit Logging
The server maintains a comprehensive audit log:
- Every access decision (allow/deny) with timestamp, user, action, resource, decision, reason
- Every policy change
- Every tenant access grant/revoke
- Every license grant/revoke
- Every branch protection rule change
- Audit logs are immutable (append-only)
- Retention configurable per tenant

### Audit Log Format
```json
{
    "timestamp": "2025-01-15T14:32:00Z",
    "event": "access_decision",
    "user": "alice@company.com",
    "tenant": "acme-corp",
    "action": "tree:write",
    "resource": "services/auth-service/src/oauth.rs",
    "scope": "path:src/oauth.rs",
    "decision": "allow",
    "policies_evaluated": ["backend-team-full-access"],
    "license_check": "pass"
}
```

### Audit Event Types
| Event Type | Description |
|---|---|
| access_decision | IAM allow/deny decision |
| policy_change | Role or permission assignment change |
| tenant_access | Tenant membership grant/revoke |
| license_change | License grant/revoke/modify |
| branch_protection | Branch protection rule change |
| snapshot_signed | Snapshot signature verification |
| secret_detected | Secret scanning match found |
| auth_event | Login, logout, token refresh, token revocation |
| sync_event | Push, pull, fetch operations |

### Audit Log Access
- Audit logs queryable via `wt audit log` CLI command
- Filterable by user, action, resource, time range, decision
- Export to SIEM systems supported (enterprise feature)
- Only users with `admin:audit_read` permission can access audit logs

## Data Encryption

### In Transit
- Transport: TLS 1.3 (all traffic)
- QUIC provides encryption at the transport layer
- gRPC fallback uses standard TLS
- Mutual TLS (mTLS) supported for enterprise deployments

### At Rest
- Optional per-tenant encryption (enterprise feature)
- Object store: objects are integrity-verified by BLAKE3 hash
- Encryption at rest uses per-tenant keys managed by the server
- Key rotation supported without re-encrypting existing objects (envelope encryption)

### Key Hierarchy
1. Master key (HSM-backed for enterprise, server-managed otherwise)
2. Per-tenant data encryption key (DEK)
3. DEK encrypted by master key (envelope encryption)
4. Objects encrypted with tenant DEK before storage

## IPC Security (BGProcess ↔ CLI)
- Unix socket with filesystem permissions (owner-only)
- Windows named pipe with ACLs
- No network exposure — local IPC only
- BGProcess validates that connecting process belongs to the same user
- No authentication tokens passed over IPC (BGProcess manages tokens internally)

## Threat Model

| Threat | Mitigation |
|---|---|
| Eavesdropping on sync | TLS 1.3 / QUIC encryption |
| Unauthorized access | Mandatory authentication, IAM |
| Code theft | License compliance enforcement |
| Secret leakage | Pre-snapshot secret scanning |
| History tampering | BLAKE3 integrity verification, append-only history, no rebase |
| Privilege escalation | Scope hierarchy, deny-beats-allow, permission ceiling model |
| Stolen credentials | Token expiry, refresh flow, revocation |
| Insider threat | Audit logging, path-level access, license grants |
| Man-in-the-middle | TLS 1.3, certificate pinning |
| Supply chain attack | Snapshot signing, signature verification |
| Data exfiltration | License compliance, export restrictions |
| Denial of service | Rate limiting, storage quotas |

### Trust Boundaries
1. **CLI ↔ BGProcess**: Local IPC, same-user trust
2. **BGProcess ↔ Server**: Authenticated, encrypted channel over network
3. **Server ↔ Storage**: Internal, encrypted at rest
4. **Server ↔ Server** (federation): Mutual TLS, tenant-scoped trust

## Branch Protection as Security
See the branch protection rules spec (in the main spec plan §3.18):
- no_direct_push: prevents bypassing code review
- require_merge_review: mandatory peer review
- require_snapshot_signature: prevents unsigned/tampered snapshots
- require_ci_pass: ensures automated testing
- Server-enforced (cannot be bypassed by client modification)

### Protection Rule Enforcement
Branch protection rules are evaluated server-side on every push:
1. Server receives push request
2. Branch protection rules for the target branch are loaded
3. Each rule is evaluated in order
4. If any rule fails, the push is rejected with a descriptive error
5. Audit log entry created for the decision

### Protection Bypass
- Only users with `admin:bypass_protection` permission can bypass
- Bypass is logged in the audit trail with reason
- Bypass can be disabled entirely at the tenant level

## Rate Limiting
- Server enforces rate limits per user and per tenant
- Prevents abuse and denial-of-service attacks
- Configurable per plan:

| Plan | Requests/min | Sync ops/hour | Max concurrent syncs |
|---|---|---|---|
| Free | 60 | 100 | 2 |
| Pro | 600 | 1000 | 10 |
| Enterprise | Custom | Custom | Custom |

## Security Headers and API Hardening
- All server HTTP responses include security headers
- CORS restricted to authorized origins
- Content-Type validation on all uploads
- Request size limits enforced
- Input validation and sanitization on all API endpoints

## Incident Response
- Automated alerting on suspicious activity patterns
- Token revocation API for compromised credentials
- Tenant-wide lockdown capability for active incidents
- Audit log preservation during incidents (extended retention)

## Implementation Status
- IMPLEMENTED: BLAKE3 hashing, basic auth token support
- TODO: Secret scanning, snapshot signing, audit logging
- PLANNED: OAuth2 device flow, certificate pinning, encryption at rest