package iam

import (
	"encoding/json"
	"fmt"
)

type PolicyFile struct {
	Rules []PolicyRule `json:"rules"`
}

func ParsePolicies(data []byte) ([]PolicyRule, error) {
	var file PolicyFile
	if err := json.Unmarshal(data, &file); err != nil {
		return nil, fmt.Errorf("parse policy file: %w", err)
	}
	for i, rule := range file.Rules {
		if rule.Effect != EffectAllow && rule.Effect != EffectDeny {
			return nil, fmt.Errorf("policy rule %d: effect must be allow or deny", i)
		}
		if len(rule.Actions) == 0 {
			return nil, fmt.Errorf("policy rule %d: at least one action is required", i)
		}
		if len(rule.Resources) == 0 {
			return nil, fmt.Errorf("policy rule %d: at least one resource is required", i)
		}
	}
	return file.Rules, nil
}
