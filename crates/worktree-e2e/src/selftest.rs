//! Self-tests for the harness itself. These need no external components
//! and always run (they are not gated on `WT_E2E`).

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::time::Duration;

use serde_json::json;

use crate::api::Api;
use crate::cmd::{strip_ansi, Cmd};
use crate::json;

#[test]
fn strip_ansi_removes_escapes() {
    assert_eq!(strip_ansi("\u{1b}[1;32mok\u{1b}[0m done"), "ok done");
    assert_eq!(strip_ansi("plain"), "plain");
}

#[test]
fn random_hex_is_random_and_sized() {
    let a = crate::random_hex(40);
    let b = crate::random_hex(40);
    assert_eq!(a.len(), 40);
    assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    assert_ne!(a, b);
}

#[test]
fn find_objects_is_envelope_agnostic() {
    let bare = json!([{"id": "a", "parents": [], "message": "m1"}, {"id": "b", "parents": ["a"], "message": "m2"}]);
    let wrapped = json!({"snapshots": bare.clone(), "count": 2});
    assert_eq!(json::ids(&json::snapshots(&bare)), vec!["a", "b"]);
    assert_eq!(json::ids(&json::snapshots(&wrapped)), vec!["a", "b"]);

    // Conflicts nested in a status report are not mistaken for changes.
    let status = json!({
        "tree": "proj", "branch": "main", "head": null,
        "changes": [{"path": "x", "kind": "modified"}],
        "merge": {"conflicts": [{"path": "x", "kind": "content"}]}
    });
    let report = json::status_report(&status, None);
    assert_eq!(
        json::changes(report),
        vec![("x".to_string(), "modified".to_string())]
    );

    let multi = json!([
        {"tree": "proj", "branch": "main", "changes": []},
        {"tree": "lib", "branch": "main", "changes": [{"path": "c", "kind": "added"}]}
    ]);
    assert_eq!(
        json::changes(json::status_report(&multi, Some("lib"))).len(),
        1
    );
    assert_eq!(
        json::find_bool(&json!({"result": {"ignored": true}}), "ignored"),
        Some(true)
    );
}

#[test]
fn cmd_captures_output_and_exit_code() {
    let dir = tempfile::tempdir().unwrap();
    let exe = std::env::current_exe().unwrap();
    let out = Cmd::new(&exe, dir.path())
        .args(["--list", "--exact", "selftest::strip_ansi_removes_escapes"])
        .ok();
    assert!(out.stdout.contains("strip_ansi_removes_escapes"), "{out}");
    let bad = Cmd::new(&exe, dir.path())
        .arg("--definitely-not-a-flag")
        .run();
    assert!(!bad.success(), "{bad}");
    assert!(Cmd::new(dir.path().join("missing-binary"), dir.path())
        .try_run()
        .is_err());
}

/// Child half of `running_process_is_interactive` (a no-op unless spawned
/// by it): prints a prompt, reads one line, echoes it.
#[test]
fn interactive_child() {
    if std::env::var("WT_E2E_SELFTEST_CHILD").as_deref() != Ok("1") {
        return;
    }
    println!("READY otpauth://totp/W0rkTree:x?secret=GEZDGNBV&issuer=W0rkTree");
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).unwrap();
    println!("got: {}", line.trim());
}

#[test]
fn running_process_is_interactive() {
    let dir = tempfile::tempdir().unwrap();
    let exe = std::env::current_exe().unwrap();
    let mut child = Cmd::new(&exe, dir.path())
        .args([
            "--exact",
            "selftest::interactive_child",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("WT_E2E_SELFTEST_CHILD", "1")
        .spawn();
    let secret = child.wait_for(Duration::from_secs(30), crate::totp::secret_from_otpauth);
    assert_eq!(secret, "GEZDGNBV");
    child.write_stdin("hello\n");
    let out = child.wait();
    assert!(out.success(), "{out}");
    assert!(out.stdout.contains("got: hello"), "{out}");
}

/// Minimal HTTP server: answers each connection with the next canned response.
fn fake_http(responses: Vec<String>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for (stream, resp) in listener.incoming().zip(responses) {
            let mut stream = stream.unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            // Consume the request head (and any small body).
            let mut content_length = 0usize;
            loop {
                line.clear();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    content_length = v.trim().parse().unwrap_or(0);
                }
            }
            let mut body = vec![0u8; content_length];
            let _ = std::io::Read::read_exact(&mut reader, &mut body);
            stream.write_all(resp.as_bytes()).unwrap();
            stream.flush().unwrap();
            // Keep SSE connections open briefly so the client reads the stream.
            std::thread::sleep(Duration::from_millis(300));
        }
    });
    format!("http://{addr}")
}

#[test]
fn api_parses_json_and_errors() {
    let ok_body = r#"{"token":"t0k","account":{"username":"u"}}"#;
    let err_body = r#"{"error":{"code":"forbidden","message":"nope"}}"#;
    let base = fake_http(vec![
        format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{ok_body}", ok_body.len()),
        format!("HTTP/1.1 403 Forbidden\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{err_body}", err_body.len()),
    ]);
    let api = Api::login(&base, "u", "p");
    assert_eq!(api.token(), Some("t0k"));
    api.get("/api/v1/admin/stats")
        .assert_error(403, "forbidden");
}

#[test]
fn sse_stream_delivers_typed_events() {
    let base = fake_http(vec![
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n\
         : heartbeat\n\n\
         event: snapshot.staged\ndata: {\"type\":\"snapshot.staged\"}\n\n\
         event: branch.pushed\ndata: {\"type\":\"branch.pushed\",\"tenant_id\":\"t1\"}\n\n"
            .to_string(),
    ]);
    let mut sse = Api::new(&base).sse("/api/v1/events");
    assert!(sse.wait_connected(Duration::from_secs(5)));
    let ev = sse.wait_for("branch.pushed", Duration::from_secs(5));
    assert_eq!(ev.event, "branch.pushed");
    assert_eq!(ev.json()["tenant_id"], json!("t1"));
}
