//! # ABAC attributes (v0.8, ENGINEERING_PLAN §16.5)
//!
//! Tenants, accounts and trees carry string attributes (`department =
//! "eng"`, `classification = "secret"`, `region = "eu-west"`). Policy
//! statements test them with attribute conditions:
//!
//! ```toml
//! [[policy.statement]]
//! effect = "deny"
//! principals = ["*"]
//! actions = ["tree:*"]
//! resources = ["wt:tree/acme/*"]
//! [[policy.statement.conditions.attributes]]
//! key = "tree.classification"
//! op = "equals"
//! value = "secret"            # `value` = one value, `values` = a list
//! [[policy.statement.conditions.attributes]]
//! key = "principal.clearance"
//! op = "not_in"
//! values = ["secret", "top-secret"]
//! ```
//!
//! (the inline form `conditions = { attributes = [{ key = "...", op =
//! "...", values = [...] }] }` is equivalent). This module holds the rules
//! shared by the declarative validator and the CLI; they mirror the Go
//! server's `domain/abac.go` and `authz/attributes.go`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// At most this many attributes per tenant, account or tree.
pub const MAX_ATTRIBUTES: usize = 64;
/// Attribute values are at most this many bytes.
pub const MAX_VALUE_LEN: usize = 256;
/// At most this many attribute conditions per statement.
pub const MAX_CONDITIONS: usize = 32;
/// At most this many values per condition.
pub const MAX_CONDITION_VALUES: usize = 64;

/// Condition key scopes: `principal.<key>`, `tree.<key>`, `tenant.<key>`.
pub const SCOPES: &[&str] = &["principal", "tree", "tenant"];

/// Condition operators, in documentation order.
pub const OPS: &[&str] = &[
    "equals",
    "not_equals",
    "in",
    "not_in",
    "starts_with",
    "exists",
    "not_exists",
];

/// One attribute condition of a statement (`domain.AttributeCondition`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttributeCondition {
    pub key: String,
    pub op: String,
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "null_as_empty"
    )]
    pub values: Vec<String>,
}

fn null_as_empty<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    Ok(Option::<Vec<String>>::deserialize(d)?.unwrap_or_default())
}

/// Whether `key` is a valid attribute key: `[a-z][a-z0-9_.-]{0,63}`.
pub fn valid_key(key: &str) -> bool {
    let b = key.as_bytes();
    !b.is_empty()
        && b.len() <= 64
        && b[0].is_ascii_lowercase()
        && b.iter().all(|c| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'_' | b'.' | b'-')
        })
}

/// Check one attribute value: 1..=256 bytes without control characters.
pub fn check_value(value: &str) -> Result<(), String> {
    if value.is_empty() {
        Err("value is empty".into())
    } else if value.len() > MAX_VALUE_LEN {
        Err(format!("value is longer than {MAX_VALUE_LEN} bytes"))
    } else if value.chars().any(char::is_control) {
        Err("value contains control characters".into())
    } else {
        Ok(())
    }
}

/// Check the attributes of a tenant, account or tree (problems in key order).
pub fn check_attributes(attrs: &BTreeMap<String, String>) -> Result<(), String> {
    if attrs.len() > MAX_ATTRIBUTES {
        return Err(format!(
            "at most {MAX_ATTRIBUTES} attributes are allowed, got {}",
            attrs.len()
        ));
    }
    for (k, v) in attrs {
        if !valid_key(k) {
            return Err(format!(
                "attribute key \"{k}\" must match [a-z][a-z0-9_.-]{{0,63}}"
            ));
        }
        check_value(v).map_err(|e| format!("attribute \"{k}\": {e}"))?;
    }
    Ok(())
}

/// Split a condition key `<scope>.<key>` into scope and attribute key.
pub fn split_ref(reference: &str) -> Result<(&str, &str), String> {
    let Some((scope, key)) = reference.split_once('.').filter(|(_, k)| !k.is_empty()) else {
        return Err(format!(
            "key \"{reference}\" must be principal.<key>, tree.<key> or tenant.<key>"
        ));
    };
    if !SCOPES.contains(&scope) {
        return Err(format!(
            "key \"{reference}\" has unknown scope \"{scope}\" (want principal, tree or tenant)"
        ));
    }
    if !valid_key(key) {
        return Err(format!(
            "attribute key \"{key}\" in \"{reference}\" must match [a-z][a-z0-9_.-]{{0,63}}"
        ));
    }
    Ok((scope, key))
}

/// Check one condition; returns `(field, message)` problems, where field is
/// `key`, `op`, `values` or `values[i]` (the server's field names).
pub fn check_condition(c: &AttributeCondition) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let key = c.key.trim();
    if key.is_empty() {
        out.push((
            "key".into(),
            "is required (principal.<key>, tree.<key> or tenant.<key>)".into(),
        ));
    } else if let Err(e) = split_ref(key) {
        out.push(("key".into(), e));
    }
    let op = c.op.trim().to_ascii_lowercase();
    let n = c.values.len();
    match op.as_str() {
        "equals" | "not_equals" if n != 1 => out.push((
            "values".into(),
            format!("{op} needs exactly one value, got {n}"),
        )),
        "in" | "not_in" | "starts_with" if n == 0 => {
            out.push(("values".into(), format!("{op} needs at least one value")))
        }
        "exists" | "not_exists" if n != 0 => {
            out.push(("values".into(), format!("{op} takes no values, got {n}")))
        }
        "" => out.push(("op".into(), format!("is required ({})", OPS.join(", ")))),
        o if !OPS.contains(&o) => out.push((
            "op".into(),
            format!("unknown operator \"{}\" (want {})", c.op, OPS.join(", ")),
        )),
        _ => {}
    }
    if n > MAX_CONDITION_VALUES {
        out.push((
            "values".into(),
            format!("at most {MAX_CONDITION_VALUES} values are allowed"),
        ));
    }
    for (i, v) in c.values.iter().enumerate() {
        if let Err(e) = check_value(v) {
            out.push((format!("values[{i}]"), e));
        }
    }
    out
}

/// Evaluate a (valid) condition against an attribute value (`None` = unset).
/// An unset attribute fails `equals` / `in` / `starts_with` / `exists` and
/// satisfies `not_equals` / `not_in` / `not_exists`.
pub fn evaluate(c: &AttributeCondition, value: Option<&str>) -> bool {
    let has = |v: &str| c.values.iter().any(|x| x == v);
    match (c.op.trim().to_ascii_lowercase().as_str(), value) {
        ("equals", Some(v)) => c.values.first().is_some_and(|x| x == v),
        ("not_equals", Some(v)) => c.values.first().is_none_or(|x| x != v),
        ("in", Some(v)) => has(v),
        ("not_in", Some(v)) => !has(v),
        ("starts_with", Some(v)) => c.values.iter().any(|p| v.starts_with(p.as_str())),
        ("exists", Some(_)) | ("not_exists", None) => true,
        ("not_equals" | "not_in", None) => true,
        _ => false,
    }
}

/// Human form of a condition: `principal.department in ["eng", "ops"]`.
pub fn describe(c: &AttributeCondition) -> String {
    let op = c.op.trim().to_ascii_lowercase();
    let q = |v: &String| format!("{v:?}");
    match op.as_str() {
        "exists" | "not_exists" => format!("{} {op}", c.key),
        "equals" | "not_equals" if c.values.len() == 1 => {
            format!("{} {op} {}", c.key, q(&c.values[0]))
        }
        _ => format!(
            "{} {op} [{}]",
            c.key,
            c.values.iter().map(q).collect::<Vec<_>>().join(", ")
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cond(key: &str, op: &str, values: &[&str]) -> AttributeCondition {
        AttributeCondition {
            key: key.into(),
            op: op.into(),
            values: values.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn keys_and_values() {
        for ok in [
            "a",
            "department",
            "org.unit",
            "cost-center_2",
            &"a".repeat(64),
        ] {
            assert!(valid_key(ok), "{ok}");
        }
        for bad in ["", "A", "1a", "_a", "dept!", "dé", &"a".repeat(65)] {
            assert!(!valid_key(bad), "{bad}");
        }
        assert!(check_value("eu-west").is_ok());
        assert!(check_value("").is_err());
        assert!(check_value(&"x".repeat(257)).is_err());
        assert!(check_value(&"é".repeat(128)).is_ok()); // 256 bytes
        assert!(check_value("a\tb").is_err());
        let mut attrs = BTreeMap::new();
        attrs.insert("region".to_string(), "eu".to_string());
        assert!(check_attributes(&attrs).is_ok());
        attrs.insert("Bad".into(), "x".into());
        assert!(check_attributes(&attrs).unwrap_err().contains("\"Bad\""));
        let many: BTreeMap<String, String> =
            (0..65).map(|i| (format!("k{i}"), "v".into())).collect();
        assert!(check_attributes(&many).is_err());
    }

    #[test]
    fn refs_split_and_check() {
        assert_eq!(
            split_ref("principal.org.unit"),
            Ok(("principal", "org.unit"))
        );
        assert_eq!(split_ref("tenant.region"), Ok(("tenant", "region")));
        for bad in ["department", "account.x", "tree.", "tree.X", ".x"] {
            assert!(split_ref(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn condition_rules_match_the_server() {
        assert!(check_condition(&cond("principal.department", "equals", &["eng"])).is_empty());
        assert!(check_condition(&cond("tree.classification", "NOT_IN", &["a", "b"])).is_empty());
        assert!(check_condition(&cond("tenant.region", "exists", &[])).is_empty());
        let fields = |c: AttributeCondition| -> Vec<String> {
            check_condition(&c).into_iter().map(|(f, _)| f).collect()
        };
        assert_eq!(fields(cond("department", "exists", &[])), ["key"]);
        assert_eq!(fields(cond("", "exists", &[])), ["key"]);
        assert_eq!(fields(cond("tree.x", "like", &["a"])), ["op"]);
        assert_eq!(fields(cond("tree.x", "", &[])), ["op"]);
        assert_eq!(fields(cond("tree.x", "equals", &["a", "b"])), ["values"]);
        assert_eq!(fields(cond("tree.x", "in", &[])), ["values"]);
        assert_eq!(fields(cond("tree.x", "exists", &["a"])), ["values"]);
        assert_eq!(
            fields(cond("tree.x", "in", &["", "a\nb"])),
            ["values[0]", "values[1]"]
        );
        let many: Vec<String> = (0..65).map(|i| i.to_string()).collect();
        let c = AttributeCondition {
            key: "tree.x".into(),
            op: "in".into(),
            values: many,
        };
        assert_eq!(fields(c), ["values"]);
    }

    #[test]
    fn operators_evaluate_like_the_server() {
        let cases: &[(&str, &[&str], Option<&str>, bool)] = &[
            ("equals", &["eng"], Some("eng"), true),
            ("equals", &["eng"], Some("Eng"), false),
            ("equals", &["eng"], None, false),
            ("not_equals", &["eng"], Some("eng"), false),
            ("not_equals", &["eng"], None, true),
            ("in", &["a", "b"], Some("b"), true),
            ("in", &["a"], None, false),
            ("not_in", &["a", "b"], Some("c"), true),
            ("not_in", &["a"], Some("a"), false),
            ("not_in", &["a"], None, true),
            ("starts_with", &["eu-"], Some("eu-west"), true),
            ("starts_with", &["eu-"], Some("us"), false),
            ("starts_with", &["eu-"], None, false),
            ("exists", &[], Some("x"), true),
            ("exists", &[], None, false),
            ("not_exists", &[], None, true),
            ("not_exists", &[], Some("x"), false),
        ];
        for (op, values, value, want) in cases {
            assert_eq!(
                evaluate(&cond("principal.d", op, values), *value),
                *want,
                "{op} {values:?} {value:?}"
            );
        }
    }

    #[test]
    fn describes_conditions() {
        assert_eq!(
            describe(&cond("principal.d", "in", &["eng", "ops"])),
            "principal.d in [\"eng\", \"ops\"]"
        );
        assert_eq!(
            describe(&cond("tree.c", "equals", &["s"])),
            "tree.c equals \"s\""
        );
        assert_eq!(
            describe(&cond("tenant.r", "exists", &[])),
            "tenant.r exists"
        );
    }

    #[test]
    fn json_shape() {
        let c = cond("tree.c", "exists", &[]);
        assert_eq!(
            serde_json::to_string(&c).unwrap(),
            r#"{"key":"tree.c","op":"exists"}"#
        );
        let back: AttributeCondition =
            serde_json::from_str(r#"{"key":"tree.c","op":"in","values":null}"#).unwrap();
        assert!(back.values.is_empty());
    }
}
