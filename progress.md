# Worktree Architecture Enhancements - PR 2

## Compiled Policy Evaluation
- [x] Add TOML parser dependency (`github.com/pelletier/go-toml/v2`).
- [x] Create database migration for `tenant_policies` table.
- [x] Update `iam.ParsePolicies` to parse `.wt/access/*.toml` files instead of JSON.
- [x] Update Canonical `Service.processPolicies` to look for `.toml` files.
- [x] Implement database-backed policy storage (inserting/updating `tenant_policies`).
- [x] Refactor `Authorizer` interface implementation to evaluate rules against the `tenant_policies` table via fast SQL queries.
- [x] Add tests for the new database-backed policy evaluation.
