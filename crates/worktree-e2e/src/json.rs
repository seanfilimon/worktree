//! Helpers for `wt --json` output.
//!
//! The CLI serializes the SDK model types (`worktree_sdk::model`), whose
//! serde field names are the contract (`Snapshot{id, parents, message, ...}`,
//! `StatusReport{tree, branch, head, changes[{path, kind}], merge}`, ...).
//! The *envelope* (bare array vs. `{"snapshots": [...]}` etc.) is not fixed
//! by the contract, so these helpers locate records structurally: they find
//! every JSON object carrying a given set of keys, wherever it is nested.

use serde_json::Value;

/// Pretty-print for assertion messages.
pub fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string())
}

/// All objects (depth-first, document order) that contain every key in
/// `keys`. Matched objects are not searched further, so e.g. conflict
/// entries inside a status report are not mistaken for changes.
pub fn find_objects<'a>(v: &'a Value, keys: &[&str]) -> Vec<&'a Value> {
    let mut out = Vec::new();
    walk(v, keys, &mut out);
    out
}

fn walk<'a>(v: &'a Value, keys: &[&str], out: &mut Vec<&'a Value>) {
    match v {
        Value::Object(map) => {
            if keys.iter().all(|k| map.contains_key(*k)) {
                out.push(v);
                return;
            }
            for child in map.values() {
                walk(child, keys, out);
            }
        }
        Value::Array(items) => {
            for child in items {
                walk(child, keys, out);
            }
        }
        _ => {}
    }
}

/// Required string field (panics with the object on failure).
#[track_caller]
pub fn str_field(v: &Value, key: &str) -> String {
    match v.get(key) {
        Some(Value::String(s)) => s.clone(),
        other => panic!(
            "expected string field `{key}`, got {other:?} in\n{}",
            pretty(v)
        ),
    }
}

/// Optional string field (`null`/missing -> None).
pub fn opt_str(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(String::from)
}

/// Required bool field.
#[track_caller]
pub fn bool_field(v: &Value, key: &str) -> bool {
    v.get(key)
        .and_then(Value::as_bool)
        .unwrap_or_else(|| panic!("expected bool field `{key}` in\n{}", pretty(v)))
}

/// Required array field.
#[track_caller]
pub fn array_field<'a>(v: &'a Value, key: &str) -> &'a Vec<Value> {
    v.get(key)
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("expected array field `{key}` in\n{}", pretty(v)))
}

/// Assert an object has all `keys`.
#[track_caller]
pub fn assert_keys(v: &Value, keys: &[&str], what: &str) {
    let missing: Vec<&str> = keys
        .iter()
        .copied()
        .filter(|k| v.get(*k).is_none())
        .collect();
    assert!(
        missing.is_empty(),
        "{what}: missing keys {missing:?} in\n{}",
        pretty(v)
    );
}

/// Snapshot records (`{id, parents, message, ...}`) in output order.
#[track_caller]
pub fn snapshots(v: &Value) -> Vec<Value> {
    find_objects(v, &["id", "parents", "message"])
        .into_iter()
        .cloned()
        .collect()
}

/// Parent ids of a snapshot record.
#[track_caller]
pub fn parents(snapshot: &Value) -> Vec<String> {
    array_field(snapshot, "parents")
        .iter()
        .map(|p| {
            p.as_str()
                .map(String::from)
                .unwrap_or_else(|| panic!("parent is not a string in\n{}", pretty(snapshot)))
        })
        .collect()
}

/// Ids of snapshot records.
#[track_caller]
pub fn ids(snapshots: &[Value]) -> Vec<String> {
    snapshots.iter().map(|s| str_field(s, "id")).collect()
}

/// Messages of snapshot records.
#[track_caller]
pub fn messages(snapshots: &[Value]) -> Vec<String> {
    snapshots.iter().map(|s| str_field(s, "message")).collect()
}

/// Status reports (`{tree, branch, changes, ...}`).
pub fn status_reports(v: &Value) -> Vec<&Value> {
    find_objects(v, &["changes", "branch"])
}

/// Pick the status report for `tree` (or the only one).
#[track_caller]
pub fn status_report<'a>(v: &'a Value, tree: Option<&str>) -> &'a Value {
    let reports = status_reports(v);
    if reports.len() == 1 {
        return reports[0];
    }
    if let Some(tree) = tree {
        if let Some(r) = reports
            .iter()
            .find(|r| r.get("tree").and_then(Value::as_str) == Some(tree))
        {
            return r;
        }
    }
    panic!(
        "expected exactly one status report (or one for tree {tree:?}), found {} in\n{}",
        reports.len(),
        pretty(v)
    )
}

/// (path, kind) pairs of a status report's `changes`.
#[track_caller]
pub fn changes(report: &Value) -> Vec<(String, String)> {
    array_field(report, "changes")
        .iter()
        .map(|c| (str_field(c, "path"), str_field(c, "kind")))
        .collect()
}

/// Named records (branches, tags, trees): the `name` field of every object
/// that has `name` plus all of `extra_keys`.
#[track_caller]
pub fn names(v: &Value, extra_keys: &[&str]) -> Vec<String> {
    let mut keys = vec!["name"];
    keys.extend_from_slice(extra_keys);
    find_objects(v, &keys)
        .into_iter()
        .map(|o| str_field(o, "name"))
        .collect()
}

/// The object with `name == name` among records having `extra_keys`.
#[track_caller]
pub fn named<'a>(v: &'a Value, name: &str, extra_keys: &[&str]) -> Option<&'a Value> {
    let mut keys = vec!["name"];
    keys.extend_from_slice(extra_keys);
    find_objects(v, &keys)
        .into_iter()
        .find(|o| o.get("name").and_then(Value::as_str) == Some(name))
}

/// First boolean value found under `key` anywhere in `v`.
pub fn find_bool(v: &Value, key: &str) -> Option<bool> {
    find_objects(v, &[key])
        .into_iter()
        .find_map(|o| o.get(key).and_then(Value::as_bool))
}

/// Every string anywhere in `v` (for loose "mentions" checks).
pub fn all_strings(v: &Value) -> Vec<String> {
    let mut out = Vec::new();
    fn go(v: &Value, out: &mut Vec<String>) {
        match v {
            Value::String(s) => out.push(s.clone()),
            Value::Array(a) => a.iter().for_each(|x| go(x, out)),
            Value::Object(m) => m.iter().for_each(|(k, x)| {
                out.push(k.clone());
                go(x, out)
            }),
            _ => {}
        }
    }
    go(v, &mut out);
    out
}
