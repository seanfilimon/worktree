package iam

import (
	"context"
	"encoding/json"
	"fmt"
	"strings"

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
	_, err = tx.Exec(ctx, "DELETE FROM tenant_permissions WHERE tenant = $1", tenant)
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

		for _, action := range rule.Actions {
			for _, res := range rule.Resources {
				_, err = tx.Exec(ctx, `
					INSERT INTO tenant_permissions (tenant, effect, account, action, resource_pattern, conditions)
					VALUES ($1, $2, $3, $4, $5, $6)
				`, tenant, rule.Effect, rule.Account, action, res, condJSON)
				if err != nil {
					return fmt.Errorf("insert rule: %w", err)
				}
			}
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

	normalizedResource := resource
	if normalizedResource != "" && normalizedResource != "staged" && !strings.HasPrefix(normalizedResource, "tenant:") {
		normalizedResource = "tenant:" + normalizedResource
	}

	// Fetch rules for the tenant and the global tenant ("")
	// Ordering by effect DESC ensures 'deny' comes before 'allow'
	rows, err := a.pool.Query(ctx, `
		SELECT effect, conditions
		FROM tenant_permissions
		WHERE tenant IN ($1, '')
		  AND (account = '' OR account = $2)
		  AND $3 LIKE REPLACE(action, '*', '%')
		  AND $4 LIKE REPLACE(REPLACE(REPLACE(resource_pattern, '${tenant}', $1), '${account}', $2), '*', '%')
		ORDER BY effect DESC
	`, principal.Tenant, principal.Account, action, normalizedResource)
	if err != nil {
		return Deny, fmt.Errorf("query permissions: %w", err)
	}
	defer rows.Close()

	matchedAllow := false
	for rows.Next() {
		var effect Effect
		var condJSON []byte
		if err := rows.Scan(&effect, &condJSON); err != nil {
			return Deny, fmt.Errorf("scan policy: %w", err)
		}

		var conditions map[string]string
		if len(condJSON) > 0 {
			if err := json.Unmarshal(condJSON, &conditions); err != nil {
				return Deny, fmt.Errorf("unmarshal conditions: %w", err)
			}
		}

		if !conditionsMatch(conditions, principal, action) {
			continue
		}

		if effect == EffectDeny {
			return Deny, nil
		}
		if effect == EffectAllow {
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
