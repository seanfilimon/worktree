//! v0.7 hardening e2e gaps (ENGINEERING_PLAN §15.10).
//!
//! - `webhook_retries_and_deactivation`: with the testing-only
//!   `--webhook-retry-schedule 1s,2s`, a receiver answering 500 twice and
//!   then 200 sees three attempts of one delivery spaced by the schedule,
//!   and the delivery log follows them (attempts 1, 2, 3, failed then
//!   succeeded). A receiver that always fails deactivates its hook after 20
//!   consecutive failed attempts (manual pings do not count): `active` turns
//!   false, `webhook.failing` is delivered to a hook subscribed to it, and
//!   later pushes neither create deliveries nor reach the receiver.
//! - `grpc_gzip_negotiation`: pushing and cloning a large compressible
//!   file moves `worktree_grpc_encoding_total{encoding="request/gzip"}` and
//!   `{encoding="response/gzip"}` on GET /metrics; the clone is identical.
//! - `access_apply_policy_versions`: `wt access apply` of a tenant and a
//!   tree document records policy versions with the "applied from the
//!   access document of ..." comment (create, update, delete; deleted
//!   policies listed by `wt policy deleted`), and deleting the tree records
//!   `delete` versions for the tree document's policies.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::Value;
use worktree_e2e::json::{find_objects, pretty, str_field};
use worktree_e2e::{fsutil, poll_until, require_e2e, Api, TestEnv};

use crate::common::{publish_tree, TREE};

// ---- a scripted webhook receiver --------------------------------------------

/// One request received by a [`Hook`] receiver.
#[derive(Debug, Clone)]
struct Hit {
    /// Lower-cased header names.
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
    at: Instant,
    /// HTTP status the receiver answered.
    status: u16,
}

impl Hit {
    fn header(&self, name: &str) -> &str {
        self.headers.get(name).map(String::as_str).unwrap_or("")
    }

    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }
}

/// Status the receiver answers for its n-th request (0-based).
type Script = Arc<dyn Fn(usize) -> u16 + Send + Sync>;

fn serve(stream: TcpStream, n: usize, script: &Script, tx: &Sender<Hit>) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut headers = BTreeMap::new();
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h)? == 0 || h.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    let at = Instant::now();
    let len: usize = headers
        .get("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body)?;
    let status = script(n);
    let reason = if status < 300 { "OK" } else { "Error" };
    let text = if status < 300 { "received" } else { "failing" };
    let mut stream = stream;
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
        text.len()
    )?;
    stream.flush()?;
    let _ = tx.send(Hit {
        headers,
        body,
        at,
        status,
    });
    Ok(())
}

/// A local HTTP receiver on 127.0.0.1 answering per `script`.
fn start_receiver(script: Script) -> (String, Receiver<Hit>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind receiver");
    let url = format!(
        "http://127.0.0.1:{}/hooks/wt",
        listener.local_addr().unwrap().port()
    );
    let (tx, rx) = channel();
    let count = Arc::new(AtomicUsize::new(0));
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let tx = tx.clone();
            let script = script.clone();
            let n = count.fetch_add(1, Ordering::SeqCst);
            std::thread::spawn(move || {
                let _ = serve(stream, n, &script, &tx);
            });
        }
    });
    (url, rx)
}

fn drain(rx: &Receiver<Hit>, seen: &mut Vec<Hit>) {
    while let Ok(h) = rx.try_recv() {
        seen.push(h);
    }
}

/// Seconds since the Unix epoch of a UTC RFC 3339 timestamp as Go writes
/// it ("2026-10-25T10:11:12.123456789Z").
fn epoch_seconds(ts: &str) -> f64 {
    let (date, time) = ts
        .strip_suffix('Z')
        .and_then(|t| t.split_once('T'))
        .unwrap_or_else(|| panic!("not a UTC RFC 3339 timestamp: {ts:?}"));
    let d: Vec<i64> = date.split('-').map(|p| p.parse().unwrap()).collect();
    let (y, m, day) = (d[0], d[1], d[2]);
    // Days from civil (Howard Hinnant).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let t: Vec<&str> = time.split(':').collect();
    let (h, mi): (i64, i64) = (t[0].parse().unwrap(), t[1].parse().unwrap());
    let s: f64 = t[2].parse().unwrap();
    (days * 86_400 + h * 3600 + mi * 60) as f64 + s
}

#[test]
fn epoch_seconds_parses_go_timestamps() {
    assert_eq!(epoch_seconds("1970-01-01T00:00:00Z"), 0.0);
    assert_eq!(epoch_seconds("2000-03-01T00:00:01Z"), 951_868_801.0);
    let a = epoch_seconds("2026-12-31T23:59:59.5Z");
    let b = epoch_seconds("2027-01-01T00:00:01.25Z");
    assert!((b - a - 1.75).abs() < 1e-6, "{a} {b}");
}

/// The deliveries of a tenant hook (REST, newest first).
fn deliveries(api: &Api, hook: &str) -> Vec<Value> {
    let v = api
        .get(&format!(
            "/api/v1/tenants/alice/webhooks/{hook}/deliveries?limit=200"
        ))
        .ok_json();
    find_objects(&v, &["id", "attempts", "succeeded"])
        .into_iter()
        .cloned()
        .collect()
}

#[test]
fn webhook_retries_and_deactivation() {
    require_e2e!();
    let env = TestEnv::with_server_args(
        "webhook_retries",
        &[
            "--webhook-allow-private",
            "--webhook-retry-schedule",
            "1s,2s",
        ],
    );
    let alice = env.user("alice");
    let a = publish_tree(&env, &alice, &[("README.md", "# retries\n")]);
    let api = alice.api();
    let create = |url: &str, event: &str| {
        let v = a
            .wt()
            .args([
                "webhook", "create", url, "--event", event, "--tenant", "alice",
            ])
            .json();
        str_field(&v, "id")
    };
    let show = |id: &str| {
        a.wt()
            .args(["webhook", "show", id, "--tenant", "alice"])
            .json()
    };

    // --- retries: 500, 500, then 200
    let (flaky_url, flaky_rx) = start_receiver(Arc::new(|n| if n < 2 { 500 } else { 200 }));
    let flaky = create(&flaky_url, "branch.pushed");
    let mut flaky_seen = Vec::new();
    a.commit(&[("retry.txt", "retry me\n")], "trigger retries");
    a.push();

    // Follow the delivery log while the attempts happen.
    let mut observed: Vec<(u64, bool, u64)> = Vec::new(); // (attempts, succeeded, status)
    let mut first_failed: Option<Value> = None;
    let delivery = poll_until(
        "the delivery to succeed on its third attempt",
        Duration::from_secs(30),
        Duration::from_millis(50),
        || {
            drain(&flaky_rx, &mut flaky_seen);
            let list = deliveries(&api, &flaky);
            let Some(d) = list.first().cloned() else {
                return Err("no delivery yet".to_string());
            };
            let state = (
                d["attempts"].as_u64().unwrap_or(0),
                d["succeeded"] == true,
                d["status_code"].as_u64().unwrap_or(0),
            );
            if observed.last() != Some(&state) {
                observed.push(state);
            }
            if state.0 == 1 && !state.1 && first_failed.is_none() {
                first_failed = Some(d.clone());
            }
            if state.1 {
                Ok(d)
            } else {
                Err(pretty(&d))
            }
        },
    );
    assert_eq!(
        deliveries(&api, &flaky).len(),
        1,
        "retries reuse the delivery"
    );
    assert_eq!(
        delivery["attempts"].as_u64(),
        Some(3),
        "{}",
        pretty(&delivery)
    );
    assert_eq!(delivery["status_code"].as_u64(), Some(200));
    assert!(
        delivery.get("next_retry_at").is_none_or(Value::is_null),
        "no retry after success:\n{}",
        pretty(&delivery)
    );
    // Attempt numbers only grow; failures (500) come before the success.
    assert!(
        observed.windows(2).all(|w| w[0].0 < w[1].0),
        "attempts must increase: {observed:?}"
    );
    assert!(
        observed
            .iter()
            .filter(|s| !s.1 && s.0 > 0)
            .all(|s| s.2 == 500 && s.0 <= 2),
        "{observed:?}"
    );
    assert_eq!(observed.last().map(|s| (s.0, s.1)), Some((3, true)));
    // The first failure was logged with its error and the next retry 1s
    // after the attempt (the first delay of the schedule).
    // (The 1s window between the first two attempts is ample for 50ms
    // polling of the delivery log.)
    let d = first_failed
        .unwrap_or_else(|| panic!("the first failed attempt was not observed: {observed:?}"));
    assert!(
        str_field(&d, "error").contains("500"),
        "the failed attempt's error is logged:\n{}",
        pretty(&d)
    );
    let gap = epoch_seconds(&str_field(&d, "next_retry_at"))
        - epoch_seconds(&str_field(&d, "delivered_at"));
    assert!((gap - 1.0).abs() < 0.05, "first retry delay {gap}s");
    // The receiver saw the same delivery three times, spaced ~1s and ~2s.
    let hits: Vec<Hit> = poll_until(
        "three attempts at the flaky receiver",
        Duration::from_secs(10),
        Duration::from_millis(50),
        || {
            drain(&flaky_rx, &mut flaky_seen);
            if flaky_seen.len() >= 3 {
                Ok(flaky_seen.clone())
            } else {
                Err(format!("{} hits", flaky_seen.len()))
            }
        },
    );
    let id = hits[0].header("x-wt-delivery").to_string();
    assert!(
        hits.iter().all(|h| h.header("x-wt-delivery") == id),
        "{hits:?}"
    );
    assert_eq!(str_field(&delivery, "id"), id);
    assert_eq!(
        hits.iter().map(|h| h.status).collect::<Vec<_>>(),
        vec![500, 500, 200]
    );
    let gap1 = hits[1].at.duration_since(hits[0].at).as_secs_f64();
    let gap2 = hits[2].at.duration_since(hits[1].at).as_secs_f64();
    assert!(
        (0.9..4.0).contains(&gap1),
        "first retry after {gap1:.2}s (schedule 1s)"
    );
    assert!(
        (1.9..5.0).contains(&gap2),
        "second retry after {gap2:.2}s (schedule 2s)"
    );
    let shown = show(&flaky);
    assert_eq!(shown["active"], Value::Bool(true), "{}", pretty(&shown));
    assert_eq!(
        shown["failure_count"].as_u64(),
        Some(0),
        "a success resets the failure count:\n{}",
        pretty(&shown)
    );

    // --- deactivation: a receiver that always fails
    let (dead_url, dead_rx) = start_receiver(Arc::new(|_| 500));
    let dead = create(&dead_url, "branch.pushed");
    // The flaky receiver (healthy by now) listens for webhook.failing.
    let watcher = create(&flaky_url, "webhook.failing");
    let mut dead_seen = Vec::new();

    // Manual pings fail but do not count towards deactivation.
    for _ in 0..3 {
        a.wt()
            .args(["webhook", "test", &dead, "--tenant", "alice"])
            .fails();
    }
    let shown = show(&dead);
    assert_eq!(shown["active"], Value::Bool(true), "{}", pretty(&shown));
    assert_eq!(
        shown["failure_count"].as_u64(),
        Some(0),
        "{}",
        pretty(&shown)
    );

    // Seven pushes x three attempts = 21 >= 20 failed attempts.
    for i in 0..7 {
        a.commit(
            &[(&format!("fail-{i}.txt"), "nobody is listening\n")],
            &format!("failing delivery {i}"),
        );
        a.push();
    }
    let shown = poll_until(
        "the always-failing hook to be deactivated",
        Duration::from_secs(60),
        Duration::from_millis(200),
        || {
            let v = show(&dead);
            if v["active"] == false {
                Ok(v)
            } else {
                Err(pretty(&v))
            }
        },
    );
    assert!(
        shown["failure_count"].as_u64().unwrap_or(0) >= 20,
        "{}",
        pretty(&shown)
    );
    assert_eq!(
        shown["last_status"].as_u64(),
        Some(500),
        "{}",
        pretty(&shown)
    );
    drain(&dead_rx, &mut dead_seen);
    let event_attempts = dead_seen
        .iter()
        .filter(|h| h.header("x-wt-event") == "branch.pushed")
        .count();
    assert!(
        event_attempts >= 20,
        "{event_attempts} event attempts before deactivation"
    );
    assert_eq!(
        dead_seen
            .iter()
            .filter(|h| h.header("x-wt-event") == "ping")
            .count(),
        3
    );

    // webhook.failing reached the watcher hook.
    let failing = poll_until(
        "webhook.failing at the watcher hook",
        Duration::from_secs(30),
        Duration::from_millis(100),
        || {
            drain(&flaky_rx, &mut flaky_seen);
            flaky_seen
                .iter()
                .find(|h| {
                    h.header("x-wt-event") == "webhook.failing"
                        && h.header("x-wt-hook-id") == watcher
                })
                .cloned()
                .ok_or_else(|| format!("{} hits at the flaky receiver", flaky_seen.len()))
        },
    );
    let payload = &failing.json()["payload"];
    assert_eq!(
        str_field(payload, "webhook_id"),
        dead,
        "{}",
        pretty(payload)
    );
    assert_eq!(str_field(payload, "url"), dead_url, "{}", pretty(payload));
    assert!(
        payload["failure_count"].as_u64().unwrap_or(0) >= 20,
        "{}",
        pretty(payload)
    );
    let failing_events = flaky_seen
        .iter()
        .filter(|h| h.header("x-wt-event") == "webhook.failing")
        .count();
    assert_eq!(failing_events, 1, "webhook.failing is published once");

    // The deactivated hook no longer receives anything.
    std::thread::sleep(Duration::from_millis(1500));
    drain(&dead_rx, &mut dead_seen);
    let hits_before = dead_seen.len();
    let logged_before = deliveries(&api, &dead).len();
    a.commit(
        &[("quiet.txt", "after deactivation\n")],
        "after deactivation",
    );
    a.push();
    std::thread::sleep(Duration::from_secs(4)); // longer than the whole schedule
    drain(&dead_rx, &mut dead_seen);
    assert_eq!(
        dead_seen.len(),
        hits_before,
        "a deactivated hook must not be delivered to"
    );
    assert_eq!(
        deliveries(&api, &dead).len(),
        logged_before,
        "no deliveries are created for a deactivated hook"
    );
    assert!(
        deliveries(&api, &dead)
            .iter()
            .all(|d| d["succeeded"] == false),
        "every delivery to the dead receiver failed"
    );
    // The healthy hook got the last push.
    poll_until(
        "the flaky (now healthy) hook to get the last push",
        Duration::from_secs(20),
        Duration::from_millis(100),
        || {
            let list = deliveries(&api, &flaky);
            if list.len() >= 9 && list.iter().all(|d| d["succeeded"] == true) {
                Ok(())
            } else {
                Err(format!("{list:?}"))
            }
        },
    );
}

// ---- gRPC gzip negotiation ---------------------------------------------------

/// `worktree_grpc_encoding_total` samples of GET /metrics by label.
fn encoding_counts(env: &TestEnv) -> BTreeMap<String, u64> {
    let text = Api::new(&env.url()).get("/metrics").assert_ok().text();
    assert!(
        text.contains("# TYPE worktree_grpc_encoding_total counter"),
        "the counter is rendered:\n{text}"
    );
    let prefix = "worktree_grpc_encoding_total{encoding=\"";
    text.lines()
        .filter_map(|l| l.strip_prefix(prefix))
        .filter_map(|rest| {
            let (label, value) = rest.split_once("\"} ")?;
            Some((label.to_string(), value.trim().parse().ok()?))
        })
        .collect()
}

fn delta(before: &BTreeMap<String, u64>, after: &BTreeMap<String, u64>, label: &str) -> u64 {
    after.get(label).copied().unwrap_or(0) - before.get(label).copied().unwrap_or(0)
}

#[test]
fn grpc_gzip_negotiation() {
    require_e2e!();
    // pack-delta (§16.1): with packs, objects travel in zstd packs; this
    // scenario checks the gzip negotiation of UploadObjects/DownloadObjects.
    let env = TestEnv::with_server_args("grpc_gzip", &["--disable-packs"]);
    let alice = env.user("alice");
    let a = publish_tree(&env, &alice, &[("README.md", "# gzip\n")]);
    let big: String = (0..40_000)
        .map(|i| format!("{i:06} all work and no play makes a dull tree\n"))
        .collect();

    let before = encoding_counts(&env);
    a.commit(&[("big.txt", big.as_str())], "big compressible file");
    a.push();
    let pushed = encoding_counts(&env);
    assert!(
        delta(&before, &pushed, "request/gzip") >= 1,
        "UploadObjects is sent gzip-compressed: {before:?} -> {pushed:?}"
    );
    assert!(
        delta(&before, &pushed, "response/gzip") >= 1,
        "the server answers a gzip request in kind: {before:?} -> {pushed:?}"
    );
    assert!(
        delta(&before, &pushed, "request/identity") >= 1,
        "other RPCs (Push) stay uncompressed: {before:?} -> {pushed:?}"
    );

    let c = alice.clone_tree("alice", TREE, "proj-clone");
    let cloned = encoding_counts(&env);
    assert!(
        delta(&pushed, &cloned, "request/gzip") >= 1,
        "DownloadObjects asks with gzip: {pushed:?} -> {cloned:?}"
    );
    assert!(
        delta(&pushed, &cloned, "response/gzip") >= 1,
        "objects come back gzip-compressed: {pushed:?} -> {cloned:?}"
    );
    assert_eq!(cloned.get("request/other").copied().unwrap_or(0), 0);
    assert!(c.read("big.txt") == big, "the clone's big.txt differs");
    assert_eq!(c.head_id(), a.head_id());
    assert_eq!(fsutil::read(&c.path("big.txt")), big.as_bytes());
}

// ---- policy versions recorded by `wt access apply` ---------------------------

const APPLY_TENANT: &str = "applied from the access document of tenant alice";

fn tenant_doc(release_desc: &str, with_drop: bool) -> String {
    let mut s = format!(
        r#"
[[policy]]
name = "keep"
description = "{release_desc}"
enabled = true
[[policy.statement]]
sid = "no-release-push"
effect = "deny"
principals = ["role:viewer"]
actions = ["branch:push"]
resources = ["wt:branch/alice/{TREE}/release/*"]
"#
    );
    if with_drop {
        s.push_str(&format!(
            r#"
[[policy]]
name = "drop"
description = "removed later"
enabled = true
[[policy.statement]]
sid = "no-secret"
effect = "deny"
principals = ["role:viewer"]
actions = ["tree:read"]
resources = ["wt:path/alice/{TREE}/secret/**"]
"#
        ));
    }
    s
}

fn tree_doc() -> String {
    format!(
        r#"
[[policy]]
name = "freeze"
description = "viewers never push to main"
enabled = true
[[policy.statement]]
sid = "no-main"
effect = "deny"
principals = ["role:viewer"]
actions = ["branch:push"]
resources = ["wt:branch/alice/{TREE}/main"]
"#
    )
}

/// `name -> id` of the tenant's policies.
fn policy_ids(api: &Api) -> BTreeMap<String, String> {
    let v = api.get("/api/v1/tenants/alice/policies").ok_json();
    find_objects(&v, &["id", "name", "statements"])
        .into_iter()
        .map(|p| (str_field(p, "name"), str_field(p, "id")))
        .collect()
}

/// (version, change, comment, changed_by) of a policy's REST versions.
fn rest_versions(api: &Api, id: &str) -> Vec<(u64, String, String, String)> {
    let v = api
        .get(&format!("/api/v1/tenants/alice/policies/{id}/versions"))
        .ok_json();
    find_objects(&v, &["version", "change"])
        .into_iter()
        .map(|e| {
            (
                e["version"].as_u64().unwrap_or(0),
                str_field(e, "change"),
                e["comment"].as_str().unwrap_or_default().to_string(),
                str_field(e, "changed_by"),
            )
        })
        .collect()
}

/// (version, change) of `wt --json policy history ID`.
fn cli_history(env_user: &worktree_e2e::User, id: &str) -> Vec<(u64, String)> {
    let v = env_user
        .wt()
        .args(["policy", "history", id, "--tenant", "alice"])
        .json();
    find_objects(&v, &["version", "change"])
        .into_iter()
        .map(|e| (e["version"].as_u64().unwrap_or(0), str_field(e, "change")))
        .collect()
}

#[test]
fn access_apply_policy_versions() {
    require_e2e!();
    let env = TestEnv::with_server("access_apply_versions");
    let alice = env.user("alice");
    let a = publish_tree(&env, &alice, &[("README.md", "# versions\n")]);
    let api = alice.api();
    let tenant_dir = env.path("access-tenant");
    let tree_dir = env.path("access-tree");
    let apply_tenant = || {
        let v = a
            .wt()
            .args(["access", "apply", "--yes", "--dir"])
            .arg(&tenant_dir)
            .json();
        assert_eq!(v["accepted"], Value::Bool(true), "{}", pretty(&v));
    };

    // --- tenant document: two policies created
    fsutil::write(
        &tenant_dir.join("policies.toml"),
        tenant_doc("viewers do not push releases", true),
    );
    apply_tenant();
    let ids = policy_ids(&api);
    let keep = ids.get("keep").cloned().expect("keep policy");
    let drop = ids.get("drop").cloned().expect("drop policy");
    for id in [&keep, &drop] {
        let vs = rest_versions(&api, id);
        assert_eq!(vs.len(), 1, "{vs:?}");
        assert_eq!((vs[0].0, vs[0].1.as_str()), (1, "create"), "{vs:?}");
        assert_eq!(vs[0].2, APPLY_TENANT, "{vs:?}");
        assert_eq!(vs[0].3, "alice", "{vs:?}");
    }
    assert_eq!(cli_history(&alice, &keep), vec![(1, "create".to_string())]);
    let text = alice
        .wt()
        .args(["policy", "history", &keep, "--tenant", "alice"])
        .ok();
    assert!(text.mentions("create"), "{text}");

    // Re-applying the same document records nothing.
    apply_tenant();
    assert_eq!(rest_versions(&api, &keep).len(), 1);

    // --- a changed document: an update version
    fsutil::write(
        &tenant_dir.join("policies.toml"),
        tenant_doc("viewers never push releases", true),
    );
    apply_tenant();
    let vs = rest_versions(&api, &keep);
    assert_eq!(
        vs.iter().map(|v| (v.0, v.1.as_str())).collect::<Vec<_>>(),
        vec![(1, "create"), (2, "update")],
        "{vs:?}"
    );
    assert_eq!(vs[1].2, APPLY_TENANT);
    assert_eq!(rest_versions(&api, &drop).len(), 1, "drop is unchanged");
    let v2 = alice
        .wt()
        .args(["policy", "show-version", &keep, "2", "--tenant", "alice"])
        .json();
    assert!(
        v2.to_string().contains("viewers never push releases"),
        "{}",
        pretty(&v2)
    );

    // --- a policy removed from the document: a delete version
    fsutil::write(
        &tenant_dir.join("policies.toml"),
        tenant_doc("viewers never push releases", false),
    );
    apply_tenant();
    assert!(!policy_ids(&api).contains_key("drop"));
    let vs = rest_versions(&api, &drop);
    assert_eq!(
        vs.iter().map(|v| (v.0, v.1.as_str())).collect::<Vec<_>>(),
        vec![(1, "create"), (2, "delete")],
        "{vs:?}"
    );
    assert_eq!(vs[1].2, APPLY_TENANT);
    let deleted = alice
        .wt()
        .args(["policy", "deleted", "--tenant", "alice"])
        .json();
    assert!(deleted.to_string().contains(&drop), "{}", pretty(&deleted));
    assert!(!deleted.to_string().contains(&keep), "{}", pretty(&deleted));

    // --- tree document: stored as "<tree>/<name>", versioned too
    fsutil::write(&tree_dir.join("policies.toml"), tree_doc());
    let v = a
        .wt()
        .args(["access", "apply", "--tree-doc", "--yes", "--dir"])
        .arg(&tree_dir)
        .json();
    assert_eq!(v["accepted"], Value::Bool(true), "{}", pretty(&v));
    let freeze = policy_ids(&api)
        .get(&format!("{TREE}/freeze"))
        .cloned()
        .unwrap_or_else(|| panic!("tree policy in {:?}", policy_ids(&api)));
    let vs = rest_versions(&api, &freeze);
    assert_eq!(vs.len(), 1, "{vs:?}");
    assert_eq!(vs[0].1, "create");
    assert_eq!(
        vs[0].2,
        format!("applied from the access document of tree alice/{TREE}")
    );

    // --- deleting the tree records delete versions for its policies
    api.delete(&format!("/api/v1/trees/alice/{TREE}"))
        .assert_status(204);
    let vs = poll_until(
        "a delete version for the tree's policy",
        Duration::from_secs(20),
        Duration::from_millis(100),
        || {
            let vs = rest_versions(&api, &freeze);
            if vs.last().is_some_and(|v| v.1 == "delete") {
                Ok(vs)
            } else {
                Err(format!("{vs:?}"))
            }
        },
    );
    assert_eq!(vs.len(), 2, "{vs:?}");
    assert_eq!(vs[1].0, 2);
    assert_eq!(vs[1].2, format!("tree {TREE} deleted"), "{vs:?}");
    assert_eq!(vs[1].3, "alice", "attributed to whoever deleted the tree");
    assert!(!policy_ids(&api).contains_key(&format!("{TREE}/freeze")));
    let deleted = api.get("/api/v1/tenants/alice/policies/deleted").ok_json();
    assert!(
        deleted.to_string().contains(&freeze) && deleted.to_string().contains(&drop),
        "{}",
        pretty(&deleted)
    );
    assert_eq!(
        cli_history(&alice, &freeze),
        vec![(1, "create".to_string()), (2, "delete".to_string())]
    );
    // Tenant-document policies are not touched by the tree deletion.
    assert_eq!(rest_versions(&api, &keep).len(), 2);
}
