//! Operational/admin surface: /healthz, /metrics, admin stats, audit log,
//! SSE event stream, and read-back of pushed data over REST.

use std::time::Duration;

use serde_json::{json, Value};
use worktree_e2e::json::{self, assert_keys, find_objects, str_field};
use worktree_e2e::{require_e2e, TestEnv};

use crate::common::TREE;

const STATS_KEYS: &[&str] = &[
    "tenants",
    "accounts",
    "trees",
    "branches",
    "snapshots",
    "staged",
    "objects",
    "storage_bytes",
];

fn num(v: &Value, key: &str) -> i64 {
    v[key]
        .as_i64()
        .unwrap_or_else(|| panic!("numeric `{key}` in\n{}", json::pretty(v)))
}

/// True if `line` looks like a Prometheus sample: `name{labels} value`.
fn is_sample(line: &str) -> bool {
    if line.starts_with('#') || line.trim().is_empty() {
        return false;
    }
    let name_ok = line
        .chars()
        .next()
        .map(|c| c.is_ascii_alphabetic() || c == '_' || c == ':')
        .unwrap_or(false);
    let value = line.rsplit(' ').next().unwrap_or_default();
    name_ok && (value.parse::<f64>().is_ok() || ["NaN", "+Inf", "-Inf"].contains(&value))
}

#[test]
fn admin_api() {
    require_e2e!();
    let env = TestEnv::with_server("admin_api");
    let server = env.server();
    let anon = server.api();

    // --- health + metrics (anonymous)
    let health = anon.get("/healthz").ok_json();
    assert_eq!(
        health["status"],
        json!("ok"),
        "healthz:\n{}",
        json::pretty(&health)
    );
    assert!(health["version"].is_string(), "healthz.version");
    assert!(
        health["uptime_seconds"].is_number(),
        "healthz.uptime_seconds"
    );
    let metrics = anon.get("/metrics");
    metrics.assert_ok();
    assert!(
        metrics.content_type.starts_with("text/plain"),
        "metrics content type\n{metrics}"
    );
    assert!(
        metrics.text().lines().any(is_sample),
        "metrics has samples\n{metrics}"
    );

    // --- admin-only endpoints
    anon.get("/api/v1/admin/stats")
        .assert_error(401, "unauthenticated");
    let admin = server.admin_api();
    let stats0 = admin.get("/api/v1/admin/stats").ok_json();
    assert_keys(&stats0, STATS_KEYS, "admin stats");

    let alice = env.user("alice");
    alice.register();
    let alice_api = alice.api();
    alice_api
        .get("/api/v1/admin/stats")
        .assert_error(403, "forbidden");
    let me = alice_api.get("/api/v1/auth/me").ok_json();
    assert_eq!(
        me["account"]["username"],
        json!("alice"),
        "me:\n{}",
        json::pretty(&me)
    );
    let alice_tenant_id = str_field(&me["account"], "tenant_id");
    let owner_of_own = find_objects(&me, &["tenant", "role"])
        .into_iter()
        .any(|m| m["tenant"]["slug"] == json!("alice") && m["role"] == json!("owner"));
    assert!(
        owner_of_own,
        "alice owns her personal tenant:\n{}",
        json::pretty(&me)
    );
    let accounts = admin.get("/api/v1/admin/accounts").ok_json();
    assert!(
        find_objects(&accounts, &["username", "is_admin"])
            .iter()
            .any(|a| a["username"] == json!("alice")),
        "admin account list:\n{}",
        json::pretty(&accounts)
    );

    // --- subscribe to events, then push
    let mut events = admin.sse("/api/v1/events");
    if !events.wait_connected(Duration::from_secs(10)) {
        eprintln!("note: SSE headers not flushed within 10s; continuing (events may still arrive)");
    }

    let repo = alice.init(TREE);
    repo.commit(
        &[
            ("README.md", "# admin demo\n"),
            ("data/blob.bin", "0123456789abcdef\n"),
        ],
        "first",
    );
    repo.commit(
        &[("README.md", "# admin demo\n\nSecond revision.\n")],
        "second",
    );
    repo.add_remote("origin", &env.url(), "alice");
    repo.push();

    let ev = events.wait_for("branch.pushed", Duration::from_secs(30));
    let data = ev.json();
    assert_eq!(
        data["type"],
        json!("branch.pushed"),
        "event:\n{}",
        json::pretty(&data)
    );
    assert_eq!(
        data["tenant_id"],
        json!(alice_tenant_id),
        "event tenant:\n{}",
        json::pretty(&data)
    );

    // --- stats reflect the push
    let stats1 = admin.get("/api/v1/admin/stats").ok_json();
    assert_keys(&stats1, STATS_KEYS, "admin stats");
    assert!(
        num(&stats1, "accounts") > num(&stats0, "accounts"),
        "accounts grew:\n{}",
        json::pretty(&stats1)
    );
    assert!(
        num(&stats1, "tenants") > num(&stats0, "tenants"),
        "tenants grew"
    );
    assert!(num(&stats1, "trees") > num(&stats0, "trees"), "trees grew");
    assert!(
        num(&stats1, "branches") > num(&stats0, "branches"),
        "branches grew"
    );
    assert!(
        num(&stats1, "snapshots") >= num(&stats0, "snapshots") + 2,
        "snapshots grew by 2"
    );
    assert!(
        num(&stats1, "objects") > num(&stats0, "objects"),
        "objects grew"
    );
    assert!(
        num(&stats1, "storage_bytes") > num(&stats0, "storage_bytes"),
        "storage grew"
    );

    // --- audit log has the push
    let audit = admin.get("/api/v1/audit?action=branch.push").ok_json();
    let pushes = find_objects(&audit, &["action", "actor"]);
    assert!(
        pushes
            .iter()
            .any(|e| e["action"] == json!("branch.push") && e["actor"] == json!("alice")),
        "audit log lacks alice's branch.push:\n{}",
        json::pretty(&audit)
    );
    assert!(
        pushes.iter().all(|e| e["action"] == json!("branch.push")),
        "action filter applied"
    );

    // --- pushed data readable over REST
    let tree_path = format!("/api/v1/trees/alice/{TREE}");
    let summary = alice_api.get(&tree_path).ok_json();
    assert_eq!(
        summary["snapshot_count"].as_u64(),
        Some(2),
        "summary:\n{}",
        json::pretty(&summary)
    );
    assert_eq!(summary["branch_count"].as_u64(), Some(1));
    let snaps = alice_api
        .get(&format!("{tree_path}/snapshots?branch=main"))
        .ok_json();
    let ids: Vec<String> = find_objects(&snaps, &["id", "parents"])
        .iter()
        .map(|s| str_field(s, "id"))
        .collect();
    assert_eq!(
        ids,
        json::ids(&repo.log(&[])),
        "REST snapshot list (newest first) matches local log"
    );
    let detail = alice_api
        .get(&format!("{tree_path}/snapshots/{}", ids[0]))
        .ok_json();
    let files: Vec<String> = find_objects(&detail, &["path", "hash"])
        .iter()
        .map(|f| str_field(f, "path"))
        .collect();
    assert_eq!(
        files,
        vec!["README.md".to_string(), "data/blob.bin".to_string()],
        "snapshot files"
    );
    let blob = alice_api.get(&format!("{tree_path}/blob/{}/README.md", ids[0]));
    blob.assert_ok();
    assert_eq!(
        blob.body,
        repo.read("README.md").into_bytes(),
        "blob bytes match"
    );
    let nested = alice_api.get(&format!("{tree_path}/blob/{}/data/blob.bin", ids[0]));
    nested.assert_ok();
    assert_eq!(
        nested.body,
        b"0123456789abcdef\n".to_vec(),
        "nested-path blob bytes match"
    );
    alice_api
        .get(&format!("{tree_path}/blob/{}/nope.txt", ids[0]))
        .assert_error(404, "not_found");

    // Health still fine after activity.
    anon.get("/healthz").assert_ok();
}
