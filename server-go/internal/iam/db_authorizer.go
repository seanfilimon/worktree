package iam

import (
	"context"
	"encoding/json"
	"fmt"

	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/ramizik/worktree/server-go/internal/auth"
)

type DBAuthorizer struct {
	pool *pgxpool.Pool
}

func NewDBAuthorizer(pool *pgxpool.Pool) *DBAuthorizer {
	return &DBAuthorizer{pool: pool}
}

func (a *DBAuthorizer) UpdateTenantRules(ctx context.Context, tenant string, rules []PolicyRule) error {
	tx, err := a.pool.Begin(ctx)
	if err != nil {
		return fmt.Errorf("begin tx: %w", err)
	}
	defer tx.Rollback(ctx)

	// Clear existing rules for the tenant
	_, err = tx.Exec(ctx, "DELETE FROM tenant_policies WHERE tenant = $1", tenant)
	if err != nil {
		return fmt.Errorf("delete old rules: %w", err)
	}

	// Insert new rules
	for _, rule := range rules {
		var condJSON []byte
		if len(rule.Conditions) > 0 {
			condJSON, err = json.Marshal(rule.Conditions)
			if err != nil {
				return fmt.Errorf("marshal conditions: %w", err)
			}
		}

		_, err = tx.Exec(ctx, `
			INSERT INTO tenant_policies (tenant, effect, account, actions, resources, conditions)
			VALUES ($1, $2, $3, $4, $5, $6)
		`, tenant, rule.Effect, rule.Account, rule.Actions, rule.Resources, condJSON)
		if err != nil {
			return fmt.Errorf("insert rule: %w", err)
		}
	}

	if err := tx.Commit(ctx); err != nil {
		return fmt.Errorf("commit tx: %w", err)
	}
	return nil
}

func (a *DBAuthorizer) Authorize(ctx context.Context, principal auth.Principal, action string, resource string) (Decision, error) {
	if err := ctx.Err(); err != nil {
		return Deny, err
	}
	if !principal.Authenticated {
		return Deny, nil
	}

	// Fetch rules for the tenant and the global tenant ("")
	rows, err := a.pool.Query(ctx, `
		SELECT tenant, effect, account, actions, resources, conditions
		FROM tenant_policies
		WHERE tenant IN ($1, '') AND (account = '' OR account = $2)
	`, principal.Tenant, principal.Account)
	if err != nil {
		return Deny, fmt.Errorf("query policies: %w", err)
	}
	defer rows.Close()

	matchedAllow := false
	for rows.Next() {
		var rule PolicyRule
		var condJSON []byte
		if err := rows.Scan(&rule.Tenant, &rule.Effect, &rule.Account, &rule.Actions, &rule.Resources, &condJSON); err != nil {
			return Deny, fmt.Errorf("scan policy: %w", err)
		}
		if len(condJSON) > 0 {
			if err := json.Unmarshal(condJSON, &rule.Conditions); err != nil {
				return Deny, fmt.Errorf("unmarshal conditions: %w", err)
			}
		}

		if !matchesAny(rule.Actions, action) {
			continue
		}
		if !resourceMatchesAny(rule.Resources, principal, resource) {
			continue
		}
		if !conditionsMatch(rule.Conditions, principal, action) {
			continue
		}
		if rule.Effect == EffectDeny {
			return Deny, nil
		}
		if rule.Effect == EffectAllow {
			matchedAllow = true
		}
	}
	if err := rows.Err(); err != nil {
		return Deny, fmt.Errorf("rows iteration: %w", err)
	}

	if matchedAllow {
		return Allow, nil
	}
	return Deny, nil
}
