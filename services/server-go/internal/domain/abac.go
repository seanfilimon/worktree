package domain

import (
	"fmt"
	"regexp"
	"sort"
	"strings"
	"unicode"
	"unicode/utf8"
)

// ---- v0.8 ABAC attributes (docs/ENGINEERING_PLAN.md §16.5) ----
//
// Tenants, accounts and trees carry free-form string attributes
// (Tenant.Attributes, Account.Attributes, Tree.Attributes). Policy
// statements test them with PolicyConditions.Attributes.

// Attribute limits.
const (
	MaxAttributes        = 64  // per tenant, account or tree
	MaxAttributeValueLen = 256 // bytes
	// MaxAttributeConditions bounds PolicyConditions.Attributes.
	MaxAttributeConditions = 32
	// MaxAttributeCondValues bounds AttributeCondition.Values.
	MaxAttributeCondValues = 64
)

// AttributeKeyPattern is the syntax of attribute keys.
const AttributeKeyPattern = `^[a-z][a-z0-9_.-]{0,63}$`

var attributeKeyRe = regexp.MustCompile(AttributeKeyPattern)

// Attribute scopes: the prefix of an AttributeCondition.Key names the
// object whose attribute is tested.
const (
	AttrScopePrincipal = "principal" // the calling account (service accounts have none)
	AttrScopeTree      = "tree"      // the tree of the resource (none for tenant-level requests)
	AttrScopeTenant    = "tenant"    // the tenant of the resource (the policy's tenant)
)

// Attribute condition operators.
const (
	AttrOpEquals     = "equals"      // exactly one value; the attribute is set and equal
	AttrOpNotEquals  = "not_equals"  // exactly one value; unset or different
	AttrOpIn         = "in"          // one or more values; set and equal to one
	AttrOpNotIn      = "not_in"      // one or more values; unset or equal to none
	AttrOpStartsWith = "starts_with" // one or more prefixes; set and starts with one
	AttrOpExists     = "exists"      // no values; the attribute is set
	AttrOpNotExists  = "not_exists"  // no values; the attribute is unset
)

// AttributeOps lists the operators in documentation order.
var AttributeOps = []string{AttrOpEquals, AttrOpNotEquals, AttrOpIn, AttrOpNotIn, AttrOpStartsWith, AttrOpExists, AttrOpNotExists}

// AttributeCondition tests one attribute. Key is "<scope>.<attribute key>"
// with scope principal, tree or tenant (for example "principal.department",
// "tree.classification", "tenant.region"). Comparisons are exact and
// case-sensitive. An unset attribute fails equals / in / starts_with /
// exists and satisfies not_equals / not_in / not_exists.
type AttributeCondition struct {
	Key    string   `json:"key"`
	Op     string   `json:"op"`
	Values []string `json:"values,omitempty"`
}

// ValidAttributeKey reports whether k is a valid attribute key.
func ValidAttributeKey(k string) bool { return attributeKeyRe.MatchString(k) }

// ValidateAttributeValue checks one attribute value: 1..256 bytes of UTF-8
// without control characters.
func ValidateAttributeValue(v string) error {
	switch {
	case v == "":
		return fmt.Errorf("value is empty")
	case len(v) > MaxAttributeValueLen:
		return fmt.Errorf("value is longer than %d bytes", MaxAttributeValueLen)
	case !utf8.ValidString(v):
		return fmt.Errorf("value is not valid UTF-8")
	case strings.IndexFunc(v, unicode.IsControl) >= 0:
		return fmt.Errorf("value contains control characters")
	}
	return nil
}

// ValidateAttributes checks the attributes of a tenant, account or tree:
// at most MaxAttributes entries, keys matching AttributeKeyPattern, values
// accepted by ValidateAttributeValue. Problems are reported in key order.
func ValidateAttributes(attrs map[string]string) error {
	if len(attrs) > MaxAttributes {
		return fmt.Errorf("at most %d attributes are allowed, got %d: %w", MaxAttributes, len(attrs), ErrInvalid)
	}
	keys := make([]string, 0, len(attrs))
	for k := range attrs {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	for _, k := range keys {
		if !ValidAttributeKey(k) {
			return fmt.Errorf("attribute key %q must match %s: %w", k, AttributeKeyPattern, ErrInvalid)
		}
		if err := ValidateAttributeValue(attrs[k]); err != nil {
			return fmt.Errorf("attribute %q: %v: %w", k, err, ErrInvalid)
		}
	}
	return nil
}

// SplitAttributeRef splits a condition key "<scope>.<key>" into its scope
// and attribute key, checking both.
func SplitAttributeRef(ref string) (scope, key string, err error) {
	scope, key, ok := strings.Cut(ref, ".")
	if !ok || key == "" {
		return "", "", fmt.Errorf("key %q must be principal.<key>, tree.<key> or tenant.<key>", ref)
	}
	switch scope {
	case AttrScopePrincipal, AttrScopeTree, AttrScopeTenant:
	default:
		return "", "", fmt.Errorf("key %q has unknown scope %q (want principal, tree or tenant)", ref, scope)
	}
	if !ValidAttributeKey(key) {
		return "", "", fmt.Errorf("attribute key %q in %q must match %s", key, ref, AttributeKeyPattern)
	}
	return scope, key, nil
}
