//! Server operations (ENGINEERING_PLAN §16.11). The server keeps backups in
//! an S3 bucket (`--backup-target s3`, served by `worktree-server fake-s3`)
//! and queues notification email in its durable outbox while the SMTP
//! server is down. `wt admin backup` shows the S3 target; policy version
//! retention prunes old versions (`wt admin retention run`); with the
//! server stopped, `wt push --queue` shows up in `wt status`; the data
//! directory is wiped and restored by backup id straight from the bucket
//! (`worktree-server restore --list` / `restore ID`). The restarted server
//! still holds the queued email, which `wt admin outbox retry` delivers
//! once SMTP is back, and the queued push is flushed.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use worktree_e2e::json::{find_objects, pretty, str_field};
use worktree_e2e::wait::poll_until;
use worktree_e2e::{bins, require_e2e, Cmd, TestEnv};

use crate::common::{add_member, publish_tree, TREE};

const WAIT: Duration = Duration::from_secs(30);
const POLL: Duration = Duration::from_millis(200);
const BUCKET: &str = "wt-backups";

fn free_port() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").expect("bind 127.0.0.1:0");
    l.local_addr().expect("local_addr").port()
}

/// `worktree-server fake-s3` on a fixed port; killed on drop.
struct FakeS3 {
    child: Child,
    url: String,
}

impl FakeS3 {
    fn start() -> Self {
        let port = free_port();
        let addr = format!("127.0.0.1:{port}");
        let child = Command::new(bins::server_bin())
            .args(["fake-s3", "--addr", &addr, "--bucket", BUCKET])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn worktree-server fake-s3");
        let deadline = Instant::now() + WAIT;
        while TcpStream::connect(&addr).is_err() {
            assert!(Instant::now() < deadline, "fake-s3 did not start on {addr}");
            std::thread::sleep(Duration::from_millis(50));
        }
        Self {
            child,
            url: format!("http://{addr}"),
        }
    }
}

impl Drop for FakeS3 {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Recipients and DATA of one accepted message.
type Mail = (Vec<String>, String);

/// A minimal SMTP server (EHLO, MAIL, RCPT, DATA, QUIT) bound to `port`.
#[derive(Clone)]
struct FakeSmtp {
    mails: Arc<Mutex<Vec<Mail>>>,
}

impl FakeSmtp {
    fn start_on(port: u16) -> Self {
        let deadline = Instant::now() + WAIT;
        let listener = loop {
            match TcpListener::bind(("127.0.0.1", port)) {
                Ok(l) => break l,
                Err(e) => {
                    assert!(Instant::now() < deadline, "bind smtp port {port}: {e}");
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        };
        let mails = Arc::new(Mutex::new(Vec::new()));
        let store = mails.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let store = store.clone();
                std::thread::spawn(move || {
                    let _ = serve_smtp(stream, &store);
                });
            }
        });
        Self { mails }
    }

    fn mails(&self) -> Vec<Mail> {
        self.mails.lock().unwrap().clone()
    }
}

fn serve_smtp(stream: TcpStream, store: &Mutex<Vec<Mail>>) -> std::io::Result<()> {
    let mut w = stream.try_clone()?;
    let mut r = BufReader::new(stream);
    let say = |w: &mut TcpStream, s: &str| w.write_all(format!("{s}\r\n").as_bytes());
    say(&mut w, "220 fake-smtp.e2e ESMTP")?;
    let mut rcpt = Vec::new();
    let mut line = String::new();
    loop {
        line.clear();
        if r.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let cmd = line.trim_end().to_ascii_uppercase();
        if cmd.starts_with("EHLO") || cmd.starts_with("HELO") {
            say(&mut w, "250 fake-smtp.e2e")?;
        } else if cmd.starts_with("MAIL FROM:") {
            say(&mut w, "250 ok")?;
        } else if cmd.starts_with("RCPT TO:") {
            rcpt.push(
                line.trim_end()["RCPT TO:".len()..]
                    .trim_matches(|c| c == '<' || c == '>' || c == ' ')
                    .to_string(),
            );
            say(&mut w, "250 ok")?;
        } else if cmd == "DATA" {
            say(&mut w, "354 go ahead")?;
            let mut data = String::new();
            loop {
                let mut l = String::new();
                if r.read_line(&mut l)? == 0 {
                    return Ok(());
                }
                if l == ".\r\n" || l == ".\n" {
                    break;
                }
                data.push_str(&l);
            }
            store
                .lock()
                .unwrap()
                .push((std::mem::take(&mut rcpt), data));
            say(&mut w, "250 queued")?;
        } else if cmd == "QUIT" {
            say(&mut w, "221 bye")?;
            return Ok(());
        } else {
            say(&mut w, "250 ok")?;
        }
    }
}

fn server_cmd(env: &TestEnv) -> Cmd {
    Cmd::new(bins::server_bin(), &env.logs).actor("worktree-server")
}

fn policy(description: &str) -> Value {
    json!({
        "name": "ops-rules",
        "description": description,
        "enabled": true,
        "statements": [{
            "sid": "read",
            "effect": "allow",
            "actions": ["tree:read"],
            "resources": [format!("wt:tree/alice/{TREE}")],
            "principals": ["user:bob"],
            "conditions": {}
        }]
    })
}

/// Outbox emails (newest first) of `wt --json admin outbox list`.
fn outbox_emails(v: &Value) -> Vec<Value> {
    v["emails"].as_array().cloned().unwrap_or_default()
}

#[test]
fn server_ops() {
    require_e2e!();
    let s3 = FakeS3::start();
    let smtp_port = free_port(); // nothing listens yet: SMTP is "down"
    let smtp_url = format!("smtp://127.0.0.1:{smtp_port}");
    let s3_flags: Vec<String> = [
        "--backup-target",
        "s3",
        "--s3-endpoint",
        &s3.url,
        "--s3-bucket",
        BUCKET,
        "--s3-region",
        "us-east-1",
        "--s3-access-key",
        "e2e",
        "--s3-secret-key",
        "e2e-secret-key",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let mut flags: Vec<&str> = s3_flags.iter().map(String::as_str).collect();
    flags.extend([
        "--backup-max-age",
        "30d",
        "--smtp-url",
        &smtp_url,
        "--smtp-from",
        "W0rkTree <wt@e2e.test>",
        "--policy-version-retention",
        "2",
        "--webhook-delivery-keep",
        "5",
    ]);
    let mut env = TestEnv::with_server_args("server_ops", &flags);

    let alice = env.user("alice");
    let a = publish_tree(&env, &alice, &[("README.md", "# server ops\n")]);
    let alice_api = alice.api();
    let bob = env.user("bob");
    bob.register();
    add_member(&alice_api, "alice", "bob", "maintainer");
    let admin = env.admin_user();
    admin.login();

    // --- only server admins see the outbox and retention
    for args in [
        &["admin", "outbox", "list"][..],
        &["admin", "retention"][..],
    ] {
        let denied = alice.wt().args(args).fails();
        assert!(
            denied.mentions("permission")
                || denied.mentions("denied")
                || denied.mentions("forbidden"),
            "{}",
            denied.combined()
        );
    }

    // --- a mention queues an email for bob; SMTP is down so it stays pending
    a.wt().args(["switch", "-c", "feature"]).ok();
    a.commit(&[("feature.txt", "v1\n")], "feature v1");
    a.wt().args(["push", "--branch", "feature"]).ok();
    a.wt()
        .args([
            "mr",
            "create",
            "--source",
            "feature",
            "--target",
            "main",
            "--title",
            "Ops change",
            "--description",
            "cc @bob",
        ])
        .ok();
    a.wt().args(["switch", "main"]).ok();
    let pending = poll_until("the email to fail once", WAIT, POLL, || {
        let v = admin.wt().args(["admin", "outbox", "list"]).json();
        outbox_emails(&v)
            .into_iter()
            .find(|e| {
                str_field(e, "to") == "bob@example.com"
                    && str_field(e, "status") == "pending"
                    && e["attempts"].as_u64().unwrap_or(0) >= 1
            })
            .ok_or_else(|| pretty(&v))
    });
    let email_id = str_field(&pending, "id");
    assert!(
        !str_field(&pending, "last_error").is_empty(),
        "{}",
        pretty(&pending)
    );
    let list = admin.wt().args(["admin", "outbox", "list"]).json();
    assert_eq!(list["enabled"], true, "{}", pretty(&list));
    assert_eq!(list["dead_after"], "24h0m0s", "{}", pretty(&list));
    let human = admin
        .wt()
        .args(["admin", "outbox", "list", "--status", "pending"])
        .ok();
    assert!(
        human.mentions(&email_id) && human.mentions("bob@example.com"),
        "{}",
        human.combined()
    );

    // --- policy version retention keeps the newest 2 versions
    let created = alice_api
        .post("/api/v1/tenants/alice/policies", policy("v1"))
        .ok_json();
    let pid = str_field(&created, "id");
    for d in ["v2", "v3", "v4"] {
        alice_api
            .put(&format!("/api/v1/tenants/alice/policies/{pid}"), policy(d))
            .ok_json();
    }
    let versions = |api: &worktree_e2e::Api| {
        let v = api
            .get(&format!("/api/v1/tenants/alice/policies/{pid}/versions"))
            .ok_json();
        find_objects(&v, &["version", "change"]).len()
    };
    assert_eq!(versions(&alice_api), 4);
    let rep = admin.wt().args(["admin", "retention", "run"]).json();
    assert_eq!(
        rep["policy_versions_pruned"].as_u64(),
        Some(2),
        "{}",
        pretty(&rep)
    );
    assert_eq!(versions(&alice_api), 2, "the newest two versions remain");
    let st = admin.wt().args(["admin", "retention"]).json();
    assert_eq!(
        st["config"]["policy_versions"]["keep"],
        2,
        "{}",
        pretty(&st)
    );
    assert_eq!(st["config"]["webhook_deliveries"]["keep"], 5);
    assert_eq!(st["config"]["backups"]["max_age"], "720h0m0s");
    assert!(st["last_run_at"].is_string(), "{}", pretty(&st));

    // --- a backup goes to the bucket
    let created = admin
        .wt()
        .args(["admin", "backup", "create", "--note", "to s3"])
        .json();
    let id = str_field(&created, "id");
    assert_eq!(
        str_field(&created, "path"),
        format!("s3://{BUCKET}/backups/{id}.wtbak"),
        "{}",
        pretty(&created)
    );
    let status = admin.wt().args(["admin", "backup", "list"]).json();
    assert_eq!(
        str_field(&status["config"], "target"),
        "s3",
        "{}",
        pretty(&status)
    );
    assert_eq!(
        str_field(&status["config"], "location"),
        format!("s3://{BUCKET}/backups/")
    );
    let human = admin.wt().args(["admin", "backup", "list"]).ok();
    assert!(
        human.mentions(&format!("s3://{BUCKET}/backups/")) && human.mentions(&id),
        "{}",
        human.combined()
    );
    let file = env.path("downloads/s3.wtbak");
    let dl = admin
        .wt()
        .args(["admin", "backup", "download", &id, "-o"])
        .arg(&file)
        .json();
    assert_eq!(dl["size_bytes"], created["size_bytes"], "{}", pretty(&dl));
    let bytes = std::fs::read(&file).expect("downloaded archive");
    assert_eq!(&bytes[..4], &[0x28, 0xb5, 0x2f, 0xfd], "a zstd stream");
    let before_tips = alice_api
        .get(&format!("/api/v1/trees/alice/{TREE}/branches"))
        .ok_json();

    // --- server down: a queued push shows up in `wt status`
    env.server_mut().stop();
    a.commit(&[("offline.txt", "written offline\n")], "offline");
    a.wt().args(["push", "--queue"]).ok();
    let st = a.wt().arg("status").ok();
    assert!(
        st.mentions("queued offline") && st.mentions("wt sync flush"),
        "{}",
        st.combined()
    );
    let st = a.wt().arg("status").json();
    assert_eq!(
        st["outbox"].as_array().map(Vec::len),
        Some(1),
        "{}",
        pretty(&st)
    );

    // --- disaster: wipe the data directory, restore by id from the bucket
    let data_dir = env.server().data_dir.clone();
    std::fs::remove_dir_all(&data_dir).expect("wipe the data directory");
    let listed = server_cmd(&env)
        .args(["restore", "--list", "--data"])
        .arg(&data_dir)
        .args(&s3_flags)
        .ok();
    assert!(
        listed.mentions(&id) && listed.mentions(&format!("s3://{BUCKET}/backups/")),
        "{}",
        listed.combined()
    );
    let restored = server_cmd(&env)
        .args(["restore", "--data"])
        .arg(&data_dir)
        .args(&s3_flags)
        .arg(&id)
        .ok();
    assert!(
        restored.mentions("hashes verified") && restored.mentions(&id),
        "{}",
        restored.combined()
    );

    // SMTP comes back; the server restarts on the restored data.
    let smtp = FakeSmtp::start_on(smtp_port);
    env.restart_server();
    let after_tips = alice_api
        .get(&format!("/api/v1/trees/alice/{TREE}/branches"))
        .ok_json();
    assert_eq!(
        after_tips, before_tips,
        "the restored server serves the same branches"
    );

    // The queued email survived (it was in the backup) and is delivered on retry.
    let list = admin.wt().args(["admin", "outbox", "list"]).json();
    assert!(
        outbox_emails(&list)
            .iter()
            .any(|e| str_field(e, "id") == email_id),
        "{}",
        pretty(&list)
    );
    admin
        .wt()
        .args(["admin", "outbox", "retry", &email_id])
        .ok();
    poll_until("the queued email to be delivered", WAIT, POLL, || {
        let mails = smtp.mails();
        mails
            .iter()
            .any(|(rcpt, data)| {
                rcpt.iter().any(|r| r == "bob@example.com") && data.contains("Ops change")
            })
            .then_some(())
            .ok_or_else(|| format!("{} mails", mails.len()))
    });
    poll_until("the email to be marked sent", WAIT, POLL, || {
        let v = admin
            .wt()
            .args(["admin", "outbox", "list", "--status", "sent"])
            .json();
        outbox_emails(&v)
            .iter()
            .any(|e| str_field(e, "id") == email_id)
            .then_some(())
            .ok_or_else(|| pretty(&v))
    });
    let again = admin
        .wt()
        .args(["admin", "outbox", "retry", "--all"])
        .json();
    assert_eq!(again["retried"], 0, "{}", pretty(&again));
    let sent_again = admin
        .wt()
        .args(["admin", "outbox", "retry", &email_id])
        .fails();
    assert!(sent_again.mentions("already"), "{}", sent_again.combined());

    // The queued push goes out; `wt status` no longer mentions it.
    a.wt().args(["sync", "flush"]).ok();
    let st = a.wt().arg("status").ok();
    assert!(!st.mentions("queued offline"), "{}", st.combined());

    // The archive is still in the bucket (listed without its index entry,
    // which postdates the snapshot); deleting it removes it from S3.
    let status = admin.wt().args(["admin", "backup", "list"]).json();
    assert!(pretty(&status).contains(&id), "{}", pretty(&status));
    admin
        .wt()
        .args(["admin", "backup", "delete", &id, "--yes"])
        .ok();
    let listed = server_cmd(&env)
        .args(["restore", "--list", "--data"])
        .arg(&data_dir)
        .args(&s3_flags)
        .ok();
    assert!(!listed.mentions(&id), "{}", listed.combined());
}
