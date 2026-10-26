//! Notifications (ENGINEERING_PLAN §15.3): alice and bob collaborate on
//! `alice/proj`. A mention in a merge request description, a review
//! request (when the server has the v0.7 reviewers endpoint), comments and
//! a merge land in the right inboxes (never the actor's); `notification.created`
//! reaches only its account over SSE; watch levels (`watching` adds every
//! MR event, `ignoring` silences the tree, a tenant-wide setting); `wt inbox
//! read --all` / `read PREFIX` / `done PREFIX`; and SMTP delivery to a fake
//! mail server started by the test (`--smtp-url`).

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use worktree_e2e::json::{pretty, str_field};
use worktree_e2e::wait::poll_until;
use worktree_e2e::{require_e2e, Repo, TestEnv};

use crate::common::{add_member, publish_tree, TREE};

const WAIT: Duration = Duration::from_secs(30);
const POLL: Duration = Duration::from_millis(200);

/// One message accepted by [`FakeSmtp`].
#[derive(Debug, Clone)]
struct Mail {
    rcpt: Vec<String>,
    data: String,
}

impl Mail {
    /// The body with quoted-printable soft line breaks removed.
    fn text(&self) -> String {
        self.data.replace("=\r\n", "").replace("=3D", "=")
    }

    fn subject(&self) -> String {
        self.data
            .lines()
            .find_map(|l| l.strip_prefix("Subject: "))
            .unwrap_or_default()
            .to_string()
    }
}

/// A minimal SMTP server on 127.0.0.1 (EHLO, MAIL, RCPT, DATA, QUIT).
#[derive(Clone)]
struct FakeSmtp {
    addr: String,
    mails: Arc<Mutex<Vec<Mail>>>,
}

impl FakeSmtp {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind smtp");
        let addr = listener.local_addr().unwrap().to_string();
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
        Self { addr, mails }
    }

    fn mails(&self) -> Vec<Mail> {
        self.mails.lock().unwrap().clone()
    }

    /// Wait for a message to `rcpt` whose subject contains `subject`.
    #[track_caller]
    fn wait_for(&self, rcpt: &str, subject: &str) -> Mail {
        poll_until(
            &format!("an email to {rcpt} about {subject:?}"),
            WAIT,
            POLL,
            || {
                let all = self.mails();
                all.iter()
                    .find(|m| m.rcpt.iter().any(|r| r == rcpt) && m.subject().contains(subject))
                    .cloned()
                    .ok_or_else(|| {
                        all.iter()
                            .map(|m| format!("{:?}: {}", m.rcpt, m.subject()))
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
            },
        )
    }
}

fn serve_smtp(stream: TcpStream, store: &Mutex<Vec<Mail>>) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut w = stream;
    let say = |w: &mut TcpStream, line: &str| w.write_all(format!("{line}\r\n").as_bytes());
    say(&mut w, "220 fake-smtp.e2e ESMTP")?;
    let mut rcpt = Vec::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let cmd = line.trim_end().to_ascii_uppercase();
        if cmd.starts_with("EHLO") || cmd.starts_with("HELO") {
            say(&mut w, "250 fake-smtp.e2e")?;
        } else if cmd.starts_with("MAIL FROM:") {
            rcpt.clear();
            say(&mut w, "250 ok")?;
        } else if cmd.starts_with("RCPT TO:") {
            let to = line.trim_end()["RCPT TO:".len()..]
                .trim_matches(|c| c == '<' || c == '>' || c == ' ')
                .to_string();
            rcpt.push(to);
            say(&mut w, "250 ok")?;
        } else if cmd == "DATA" {
            say(&mut w, "354 end with .")?;
            let mut data = String::new();
            loop {
                let mut l = String::new();
                if reader.read_line(&mut l)? == 0 {
                    return Ok(());
                }
                if l == ".\r\n" || l == ".\n" {
                    break;
                }
                data.push_str(l.strip_prefix('.').unwrap_or(&l));
            }
            store.lock().unwrap().push(Mail {
                rcpt: std::mem::take(&mut rcpt),
                data,
            });
            say(&mut w, "250 queued")?;
        } else if cmd == "QUIT" {
            say(&mut w, "221 bye")?;
            return Ok(());
        } else {
            say(&mut w, "250 ok")?;
        }
    }
}

/// `wt --json inbox [--all]` as the repo's user.
#[track_caller]
fn inbox(repo: &Repo, all: bool) -> Value {
    let mut args = vec!["inbox", "-n", "200"];
    if all {
        args.push("--all");
    }
    repo.wt().args(args).json()
}

fn items(v: &Value) -> Vec<Value> {
    v["notifications"].as_array().cloned().unwrap_or_default()
}

/// Wait until the user's inbox (read ones included) has a notification
/// with `reason` whose title contains `title`; returns it.
#[track_caller]
fn wait_notification(repo: &Repo, reason: &str, title: &str) -> Value {
    poll_until(
        &format!("a {reason} notification {title:?}"),
        WAIT,
        POLL,
        || {
            let v = inbox(repo, true);
            items(&v)
                .into_iter()
                .find(|n| n["reason"] == reason && str_field(n, "title").contains(title))
                .ok_or_else(|| pretty(&v))
        },
    )
}

fn count(repo: &Repo, reason: &str) -> usize {
    items(&inbox(repo, true))
        .iter()
        .filter(|n| n["reason"] == reason)
        .count()
}

#[test]
fn notifications() {
    require_e2e!();
    let smtp = FakeSmtp::start();
    let smtp_url = format!("smtp://{}", smtp.addr);
    let env = TestEnv::with_server_args(
        "notifications",
        &[
            "--smtp-url",
            &smtp_url,
            "--smtp-from",
            "W0rkTree <wt@e2e.test>",
            "--public-url",
            "http://wt.e2e.test",
        ],
    );
    let alice = env.user("alice");
    let a = publish_tree(&env, &alice, &[("README.md", "# notify\n")]);
    let alice_api = alice.api();
    let bob = env.user("bob");
    bob.register();
    add_member(&alice_api, "alice", "bob", "maintainer");
    let b = bob.clone_tree("alice", TREE, TREE);
    let bob_api = bob.api();
    let tree_spec = format!("alice/{TREE}");

    // Empty inboxes; the server reports that it can send email.
    let empty = inbox(&b, false);
    assert_eq!(empty["unread_count"], 0, "{}", pretty(&empty));
    assert!(items(&empty).is_empty(), "{}", pretty(&empty));
    let settings = b.wt().args(["inbox", "settings"]).json();
    assert_eq!(settings["email"], true, "{}", pretty(&settings));
    assert_eq!(settings["email_configured"], true, "{}", pretty(&settings));

    // Per-account SSE streams.
    let mut bob_sse = bob_api.sse("/api/v1/events?types=notification.created");
    let mut alice_sse = alice_api.sse("/api/v1/events?types=notification.created");
    assert!(bob_sse.wait_connected(Duration::from_secs(10)));
    assert!(alice_sse.wait_connected(Duration::from_secs(10)));

    // --- a mention in the MR description notifies bob (not alice, the actor)
    a.wt().args(["switch", "-c", "feature"]).ok();
    a.commit(&[("feature.txt", "v1\n")], "feature v1");
    a.wt().args(["push", "--branch", "feature"]).ok();
    let mr = a
        .wt()
        .args([
            "mr",
            "create",
            "--source",
            "feature",
            "--target",
            "main",
            "--title",
            "Add feature",
            "--description",
            "cc @bob please have a look",
        ])
        .json();
    assert_eq!(mr["number"].as_u64(), Some(1), "{}", pretty(&mr));
    let mention = wait_notification(&b, "mentioned", "alice opened !1 Add feature");
    assert_eq!(
        mention["url"],
        format!("/trees/alice/{TREE}/merge-requests/1")
    );
    assert_eq!(mention["actor"], "alice");
    assert!(mention["read_at"].is_null(), "{}", pretty(&mention));
    let ev = bob_sse.wait_for("notification.created", WAIT).json();
    assert_eq!(ev["payload"]["id"], mention["id"], "{}", pretty(&ev));
    assert_eq!(
        ev["tenant_id"], "",
        "notification events are not tenant-scoped"
    );

    // --- review request (v0.7 reviewers endpoint, when this server has it)
    let req = alice_api.post(
        &format!("/api/v1/trees/alice/{TREE}/merge-requests/1/reviewers"),
        json!({ "reviewers": ["user:bob"] }),
    );
    if req.is_success() {
        wait_notification(&b, "review_requested", "requested a review on !1");
    } else {
        eprintln!(
            "e2e[notifications]: no reviewers endpoint ({}); skipping review requests",
            req.text()
        );
    }

    // --- comments: a mention of alice, then plain activity on her MR
    b.wt()
        .args(["mr", "comment", "1", "-m", "One question for @alice"])
        .ok();
    wait_notification(&a, "mentioned", "bob commented on !1");
    b.wt()
        .args(["mr", "comment", "1", "-m", "Never mind, looks fine"])
        .ok();
    let commented = wait_notification(&a, "mr_commented", "bob commented on !1");
    assert!(
        str_field(&commented, "body").contains("looks fine"),
        "{}",
        pretty(&commented)
    );
    assert_eq!(
        count(&b, "mr_commented") + count(&b, "mentioned"),
        1,
        "bob is never notified about his own comments"
    );
    // alice's stream carried only alice's notifications.
    let ev = alice_sse.wait_for("notification.created", WAIT).json();
    assert_eq!(ev["payload"]["account_id"], commented["account_id"]);
    assert_ne!(ev["payload"]["account_id"], mention["account_id"]);

    // --- watch levels: watching adds every MR event of the tree
    let w = b
        .wt()
        .args([
            "inbox", "watch", &tree_spec, "--level", "watching", "--email",
        ])
        .json();
    assert_eq!(
        (w["level"].as_str(), w["email"].as_bool()),
        (Some("watching"), Some(true)),
        "{}",
        pretty(&w)
    );
    a.wt()
        .args(["mr", "comment", "1", "-m", "Pushing a fix"])
        .ok();
    let watching = wait_notification(&b, "watching", "alice commented on !1");
    assert!(str_field(&watching, "body").contains("Pushing a fix"));

    // ignoring silences even mentions; processing is in event order, so
    // once alice sees bob's later comment, the earlier event was handled.
    b.wt()
        .args(["inbox", "watch", &tree_spec, "--level", "ignoring"])
        .ok();
    let shown = b.wt().args(["inbox", "watch", &tree_spec]).json();
    assert_eq!(shown["level"], "ignoring", "{}", pretty(&shown));
    assert_eq!(shown["email"], true, "--level alone keeps the email flag");
    let before = items(&inbox(&b, true)).len();
    a.wt()
        .args(["mr", "comment", "1", "-m", "@bob are you there?"])
        .ok();
    b.wt()
        .args(["mr", "comment", "1", "-m", "Ignoring for now"])
        .ok();
    wait_notification(&a, "mr_commented", "bob commented on !1");
    poll_until(
        "alice's notification of bob's last comment",
        WAIT,
        POLL,
        || {
            let v = inbox(&a, true);
            items(&v)
                .iter()
                .any(|n| str_field(n, "body").contains("Ignoring for now"))
                .then_some(())
                .ok_or_else(|| pretty(&v))
        },
    );
    assert_eq!(
        items(&inbox(&b, true)).len(),
        before,
        "an ignored tree notifies nothing"
    );
    b.wt()
        .args([
            "inbox",
            "watch",
            &tree_spec,
            "--level",
            "participating",
            "--no-email",
        ])
        .ok();
    // A tenant-wide setting is listed next to the tree's.
    b.wt()
        .args(["inbox", "watch", "--tenant", "alice", "--level", "watching"])
        .ok();
    let watches = b.wt().args(["inbox", "watches"]).json();
    let ws = watches["watches"].as_array().cloned().unwrap_or_default();
    assert!(
        ws.iter()
            .any(|w| w["tree"] == TREE && w["level"] == "participating")
            && ws
                .iter()
                .any(|w| w["tenant"] == "alice" && w["tree"].is_null() && w["level"] == "watching"),
        "{}",
        pretty(&watches)
    );

    // --- bob merges alice's MR: alice is told
    b.wt().args(["mr", "merge", "1"]).ok();
    let merged = wait_notification(&a, "mr_merged", "bob merged !1 Add feature into main");
    assert_eq!(merged["event"], "mr.merged");

    // --- read state: read-all, read by prefix, done by prefix
    let unread = inbox(&a, false);
    assert!(
        unread["unread_count"].as_u64().unwrap_or(0) >= 3,
        "{}",
        pretty(&unread)
    );
    a.wt().args(["inbox", "read", "--all"]).ok();
    let after = inbox(&a, false);
    assert_eq!(after["unread_count"], 0, "{}", pretty(&after));
    assert!(items(&after).is_empty(), "{}", pretty(&after));
    let all = inbox(&a, true);
    assert!(
        items(&all).iter().all(|n| !n["read_at"].is_null()),
        "{}",
        pretty(&all)
    );

    let bob_unread = items(&inbox(&b, false));
    let first = str_field(&bob_unread[0], "id");
    b.wt().args(["inbox", "read", &first[..8]]).ok();
    let n_after = inbox(&b, true);
    let marked = items(&n_after)
        .into_iter()
        .find(|n| n["id"] == first.as_str())
        .expect("still listed");
    assert!(!marked["read_at"].is_null(), "{}", pretty(&marked));
    let total = items(&n_after).len();
    b.wt().args(["inbox", "done", &first[..8]]).ok();
    assert_eq!(items(&inbox(&b, true)).len(), total - 1);
    b.wt().args(["inbox", "read", "zzzzzzzz"]).fails();
    // Human output renders.
    let human = a.wt().args(["inbox", "--all"]).ok();
    assert!(human.stdout.contains("mr merged"), "{}", human.stdout);

    // --- email: bob's mention, bob's watching notification, alice's merge
    let m = smtp.wait_for(
        "bob@example.com",
        &format!("[alice/{TREE}] alice opened !1"),
    );
    let text = m.text();
    assert!(
        text.contains(&format!(
            "http://wt.e2e.test/admin/trees/alice/{TREE}/merge-requests/1"
        )),
        "{text}"
    );
    assert!(text.contains("From: \"W0rkTree\" <wt@e2e.test>"), "{text}");
    assert!(text.contains("multipart/alternative"), "{text}");
    smtp.wait_for("bob@example.com", "alice commented on !1");
    smtp.wait_for("alice@example.com", "bob merged !1");
    assert!(
        smtp.mails().iter().all(|m| m.rcpt.len() == 1),
        "one recipient per email"
    );
}
