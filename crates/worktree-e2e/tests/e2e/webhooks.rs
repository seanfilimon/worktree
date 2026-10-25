//! Webhooks (ENGINEERING_PLAN §14.2): a tenant webhook for `branch.*` with
//! a secret receives a signed delivery (valid `X-WT-Signature` HMAC-SHA256
//! and the documented headers) at a local HTTP receiver when alice pushes;
//! `wt webhook deliveries` lists it, `redeliver` sends it again as a new
//! delivery and `test` sends a `ping`. The server runs with
//! `--webhook-allow-private` so that a 127.0.0.1 target is allowed.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

use serde_json::Value;
use sha2::{Digest, Sha256};
use worktree_e2e::json::{find_objects, pretty, str_field};
use worktree_e2e::{require_e2e, TestEnv};

use crate::common::{publish_tree, TREE};

const SECRET: &str = "e2e-webhook-secret-7d1f";

/// One request received by [`Receiver`].
#[derive(Debug, Clone)]
struct Hit {
    method: String,
    path: String,
    /// Lower-cased header names.
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}

impl Hit {
    fn header(&self, name: &str) -> &str {
        self.headers.get(name).map(String::as_str).unwrap_or("")
    }

    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or_else(|e| {
            panic!(
                "delivery body is not JSON ({e}): {}",
                String::from_utf8_lossy(&self.body)
            )
        })
    }
}

/// HMAC-SHA256 (RFC 2104).
fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let mut k = [0u8; 64];
    if key.len() > 64 {
        k[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha256::new();
    inner.update(k.map(|b| b ^ 0x36));
    inner.update(msg);
    let inner = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(k.map(|b| b ^ 0x5c));
    outer.update(inner);
    outer.finalize().into()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The `X-WT-Signature` value of `body` under `secret`.
fn signature(secret: &str, body: &[u8]) -> String {
    format!("sha256={}", hex(&hmac_sha256(secret.as_bytes(), body)))
}

/// Read one HTTP/1.1 request, answer 200 and report it.
fn serve(stream: TcpStream, tx: &Sender<Hit>) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or_default().to_string();
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
    let len: usize = headers
        .get("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body)?;
    let mut stream = stream;
    stream.write_all(
        b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 8\r\nConnection: close\r\n\r\nreceived",
    )?;
    stream.flush()?;
    let _ = tx.send(Hit {
        method,
        path,
        headers,
        body,
    });
    Ok(())
}

/// A local HTTP receiver on 127.0.0.1 (accepts forever on a thread that
/// ends with the test process).
fn start_receiver() -> (String, Receiver<Hit>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind receiver");
    let url = format!(
        "http://127.0.0.1:{}/hooks/wt",
        listener.local_addr().unwrap().port()
    );
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let tx = tx.clone();
            std::thread::spawn(move || {
                let _ = serve(stream, &tx);
            });
        }
    });
    (url, rx)
}

/// Wait for a received request with `X-WT-Event` == `event` (earlier hits
/// are collected into `seen`).
#[track_caller]
fn wait_event(rx: &Receiver<Hit>, seen: &mut Vec<Hit>, event: &str) -> Hit {
    if let Some(h) = seen.iter().find(|h| h.header("x-wt-event") == event) {
        return h.clone();
    }
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if let Ok(hit) = rx.recv_timeout(Duration::from_millis(200)) {
            seen.push(hit.clone());
            if hit.header("x-wt-event") == event {
                return hit;
            }
        }
    }
    panic!(
        "no `{event}` delivery within 30s; received: {:?}",
        seen.iter()
            .map(|h| h.header("x-wt-event").to_string())
            .collect::<Vec<_>>()
    );
}

/// Check the documented delivery headers and signature of `hit`.
#[track_caller]
fn assert_signed(hit: &Hit, hook_id: &str, secret: &str) {
    assert_eq!(hit.method, "POST", "{hit:?}");
    assert_eq!(hit.path, "/hooks/wt", "{hit:?}");
    assert!(
        hit.header("content-type").starts_with("application/json"),
        "{hit:?}"
    );
    assert!(
        hit.header("user-agent").starts_with("W0rkTree-Hookshot/"),
        "{hit:?}"
    );
    assert_eq!(hit.header("x-wt-hook-id"), hook_id, "{hit:?}");
    assert!(!hit.header("x-wt-delivery").is_empty(), "{hit:?}");
    assert_eq!(
        hit.header("x-wt-signature"),
        signature(secret, &hit.body),
        "X-WT-Signature must be sha256=<hex HMAC-SHA256(secret, body)>"
    );
    assert_ne!(
        hit.header("x-wt-signature"),
        signature("some other secret", &hit.body)
    );
    let body = hit.json();
    assert_eq!(str_field(&body, "id"), hit.header("x-wt-delivery"));
    assert_eq!(str_field(&body, "event"), hit.header("x-wt-event"));
    assert_eq!(str_field(&body, "tenant"), "alice", "{}", pretty(&body));
}

#[test]
fn hmac_matches_rfc4231() {
    // RFC 4231 test case 2 (no server needed).
    assert_eq!(
        hex(&hmac_sha256(b"Jefe", b"what do ya want for nothing?")),
        "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
    );
}

#[test]
fn webhooks() {
    require_e2e!();
    let env = TestEnv::with_server_args("webhooks", &["--webhook-allow-private"]);
    let alice = env.user("alice");
    let a = publish_tree(&env, &alice, &[("README.md", "# hooks\n")]);
    let (url, rx) = start_receiver();
    let mut seen = Vec::new();

    // --- create a tenant webhook for branch.* with our own secret
    let created = a
        .wt()
        .args([
            "webhook",
            "create",
            &url,
            "--event",
            "branch.*",
            "--secret",
            SECRET,
            "--description",
            "e2e receiver",
            "--tenant",
            "alice",
        ])
        .json();
    let hook_id = str_field(&created, "id");
    assert_eq!(created["active"], Value::Bool(true), "{}", pretty(&created));
    assert_eq!(
        created["has_secret"],
        Value::Bool(true),
        "{}",
        pretty(&created)
    );
    let list = a.wt().args(["webhook", "list", "--tenant", "alice"]).json();
    assert!(
        find_objects(&list, &["id", "url"])
            .iter()
            .any(|h| h["id"] == hook_id.as_str() && h["url"] == url.as_str()),
        "{}",
        pretty(&list)
    );
    assert!(
        !list.to_string().contains(SECRET),
        "secrets are never listed"
    );

    // --- and a tree webhook for mr.* with a server-generated secret
    let tree_hook = a
        .wt()
        .args([
            "webhook",
            "create",
            &url,
            "--event",
            "mr.*",
            "--tree",
            &format!("alice/{TREE}"),
        ])
        .json();
    let tree_hook_id = str_field(&tree_hook, "id");
    let tree_secret = str_field(&tree_hook, "secret");
    assert!(tree_secret.len() >= 16, "{}", pretty(&tree_hook));
    let shown = a
        .wt()
        .args([
            "webhook",
            "show",
            &tree_hook_id,
            "--tree",
            &format!("alice/{TREE}"),
        ])
        .json();
    assert!(
        !shown.to_string().contains(&tree_secret),
        "the generated secret is shown once:\n{}",
        pretty(&shown)
    );
    let tenant_list = a.wt().args(["webhook", "list", "--tenant", "alice"]).json();
    assert!(
        !tenant_list.to_string().contains(&tree_hook_id),
        "tree hooks are not tenant hooks:\n{}",
        pretty(&tenant_list)
    );

    // --- a push delivers a signed branch.pushed event
    let tip = a.commit(&[("hooked.txt", "push me\n")], "trigger the hook");
    a.push();
    let hit = wait_event(&rx, &mut seen, "branch.pushed");
    assert_signed(&hit, &hook_id, SECRET);
    let body = hit.json();
    assert_eq!(str_field(&body, "tree"), TREE, "{}", pretty(&body));
    assert!(
        body["payload"].to_string().contains(&tip),
        "the payload names the pushed tip {tip}:\n{}",
        pretty(&body)
    );
    let first_delivery = hit.header("x-wt-delivery").to_string();

    // --- the delivery log records it as succeeded
    let deliveries = worktree_e2e::poll_until(
        "the delivery to be logged as succeeded",
        Duration::from_secs(20),
        Duration::from_millis(200),
        || {
            let v = a
                .wt()
                .args(["webhook", "deliveries", &hook_id, "--tenant", "alice"])
                .json();
            let ok = find_objects(&v, &["id", "succeeded"])
                .iter()
                .any(|d| d["id"] == first_delivery.as_str() && d["succeeded"] == true);
            if ok {
                Ok(v)
            } else {
                Err(pretty(&v))
            }
        },
    );
    let d = find_objects(&deliveries, &["id", "succeeded"])
        .into_iter()
        .find(|d| d["id"] == first_delivery.as_str())
        .cloned()
        .unwrap();
    assert_eq!(str_field(&d, "event"), "branch.pushed");
    assert_eq!(d["status_code"].as_u64(), Some(200), "{}", pretty(&d));
    let detail = a
        .wt()
        .args([
            "webhook",
            "deliveries",
            &hook_id,
            &first_delivery,
            "--tenant",
            "alice",
        ])
        .json();
    assert_eq!(str_field(&detail, "id"), first_delivery);
    assert!(
        detail.to_string().contains("received"),
        "the receiver's response body is logged:\n{}",
        pretty(&detail)
    );
    let text = a
        .wt()
        .args(["webhook", "deliveries", &hook_id, "--tenant", "alice"])
        .ok();
    assert!(text.mentions("branch.pushed"), "{text}");

    // --- redeliver: a new delivery of the same event, signed again
    a.wt()
        .args([
            "webhook",
            "redeliver",
            &hook_id,
            &first_delivery,
            "--tenant",
            "alice",
        ])
        .ok();
    let redelivered = worktree_e2e::poll_until(
        "the redelivery to arrive",
        Duration::from_secs(30),
        Duration::from_millis(50),
        || {
            while let Ok(h) = rx.try_recv() {
                seen.push(h);
            }
            seen.iter()
                .find(|h| {
                    h.header("x-wt-event") == "branch.pushed"
                        && h.header("x-wt-delivery") != first_delivery
                })
                .cloned()
                .ok_or_else(|| format!("{} requests so far", seen.len()))
        },
    );
    assert_signed(&redelivered, &hook_id, SECRET);
    assert_eq!(
        redelivered.json()["payload"],
        body["payload"],
        "a redelivery carries the original payload"
    );

    // --- test: a ping event
    a.wt()
        .args(["webhook", "test", &hook_id, "--tenant", "alice"])
        .ok();
    let ping = wait_event(&rx, &mut seen, "ping");
    assert_signed(&ping, &hook_id, SECRET);

    // --- a server-side merge moves main: delivered as a branch event too
    a.wt().args(["switch", "-c", "feat"]).ok();
    let feat_tip = a.commit(&[("feat.txt", "feature\n")], "feature");
    a.wt().args(["push", "--branch", "feat"]).ok();
    a.wt().args(["switch", "main"]).ok();
    a.commit(&[("main.txt", "diverge\n")], "diverge main");
    a.push();
    let mr = a
        .wt()
        .args([
            "mr", "create", "--source", "feat", "--target", "main", "--title", "Feat",
        ])
        .json();
    let n = mr["number"].as_u64().unwrap().to_string();
    let merged = a.wt().args(["mr", "merge", &n, "--method", "merge"]).json();
    let merge_id = str_field(&merged, "merged_snapshot");
    let merge_hit = worktree_e2e::poll_until(
        "a branch event for the server-side merge",
        Duration::from_secs(30),
        Duration::from_millis(50),
        || {
            while let Ok(h) = rx.try_recv() {
                seen.push(h);
            }
            seen.iter()
                .find(|h| {
                    h.header("x-wt-event").starts_with("branch.")
                        && h.json()["payload"].to_string().contains(&merge_id)
                })
                .cloned()
                .ok_or_else(|| {
                    format!(
                        "events so far: {:?}",
                        seen.iter()
                            .map(|h| h.header("x-wt-event").to_string())
                            .collect::<Vec<_>>()
                    )
                })
        },
    );
    assert_signed(&merge_hit, &hook_id, SECRET);
    assert_ne!(merge_id, feat_tip);
    a.pull();

    // The tree hook saw the merge request open and merge, signed with its
    // generated secret.
    for event in ["mr.opened", "mr.merged"] {
        let hit = worktree_e2e::poll_until(
            event,
            Duration::from_secs(30),
            Duration::from_millis(50),
            || {
                while let Ok(h) = rx.try_recv() {
                    seen.push(h);
                }
                seen.iter()
                    .find(|h| h.header("x-wt-event") == event)
                    .cloned()
                    .ok_or_else(|| format!("{} requests so far", seen.len()))
            },
        );
        assert_signed(&hit, &tree_hook_id, &tree_secret);
        assert_eq!(str_field(&hit.json(), "tree"), TREE);
    }

    // Each hook only received the events it subscribed to.
    while let Ok(h) = rx.try_recv() {
        seen.push(h);
    }
    for h in &seen {
        let ev = h.header("x-wt-event");
        if h.header("x-wt-hook-id") == hook_id {
            assert!(
                ev.starts_with("branch.") || ev == "ping",
                "unexpected event {ev} delivered to the branch.* hook"
            );
        } else {
            assert_eq!(h.header("x-wt-hook-id"), tree_hook_id);
            assert!(
                ev.starts_with("mr."),
                "unexpected event {ev} delivered to the mr.* hook"
            );
        }
    }

    // --- an inactive hook receives nothing
    a.wt()
        .args([
            "webhook",
            "update",
            &hook_id,
            "--inactive",
            "--tenant",
            "alice",
        ])
        .ok();
    let shown = a
        .wt()
        .args(["webhook", "show", &hook_id, "--tenant", "alice"])
        .json();
    assert_eq!(shown["active"], Value::Bool(false), "{}", pretty(&shown));
    let before = seen.len();
    a.commit(&[("quiet.txt", "no hook\n")], "while inactive");
    a.push();
    std::thread::sleep(Duration::from_secs(2));
    while let Ok(h) = rx.try_recv() {
        seen.push(h);
    }
    assert_eq!(
        seen.len(),
        before,
        "an inactive hook must not be delivered to"
    );
}
