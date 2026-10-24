//! Background daemon: start, watch a worktree, auto-snapshot after
//! inactivity, stop.

use std::time::{Duration, Instant};

use worktree_e2e::json::{self, bool_field, str_field};
use worktree_e2e::{poll_until, require_e2e, TestEnv};

#[test]
fn daemon_autosnapshot() {
    require_e2e!();
    let env = TestEnv::new("daemon_autosnapshot");
    let alice = env.user("alice");
    let repo = alice.init("proj");
    let initial = repo.commit(&[("README.md", "# watched\n")], "initial");
    repo.wt()
        .args(["config", "set", "auto_snapshot.inactivity_secs", "1"])
        .ok();
    assert_eq!(
        repo.wt()
            .args(["config", "get", "auto_snapshot.inactivity_secs"])
            .ok()
            .stdout
            .trim(),
        "1"
    );
    repo.wt()
        .args(["config", "set", "sync.auto_stage", "false"])
        .ok();

    let mut daemon = alice.daemon_start();
    let bg_json = alice.bg_json_path();
    poll_until(
        "daemon to write bg.json",
        Duration::from_secs(15),
        Duration::from_millis(100),
        || {
            if bg_json.is_file() {
                Ok(())
            } else {
                Err(format!("{} missing", bg_json.display()))
            }
        },
    );
    alice.wt().args(["daemon", "status"]).ok();

    repo.wt().args(["daemon", "watch"]).arg(&repo.dir).ok();
    let status = alice.wt().args(["daemon", "status"]).json();
    assert!(
        json::all_strings(&status)
            .iter()
            .any(|s| s.contains("proj")),
        "daemon status lists the watched worktree:\n{}",
        json::pretty(&status)
    );

    // Write files; keep generating events every few seconds in case the
    // watcher registration raced with the first write.
    repo.write("auto/one.txt", "first automatic change\n");
    let mut last_write = Instant::now();
    let mut n = 1;
    let auto = poll_until(
        "an auto-generated snapshot in `wt log`",
        Duration::from_secs(30),
        Duration::from_millis(250),
        || {
            let log = repo.log(&[]);
            if let Some(s) = log.iter().find(|s| bool_field(s, "auto_generated")) {
                return Ok(s.clone());
            }
            if last_write.elapsed() > Duration::from_secs(5) {
                n += 1;
                repo.write(
                    &format!("auto/more-{n}.txt"),
                    format!("change number {n}\n"),
                );
                last_write = Instant::now();
            }
            Err(format!("log: {:?}", json::messages(&log)))
        },
    );
    let msg = str_field(&auto, "message");
    assert!(
        msg.starts_with("auto:"),
        "auto snapshot message {msg:?} should start with `auto:`"
    );
    assert_eq!(str_field(&auto, "branch"), "main");
    let chain = json::ids(&repo.log(&[]));
    assert!(
        chain.contains(&initial),
        "auto snapshots extend the branch history"
    );
    let id = str_field(&auto, "id");
    let show = repo.wt().args(["show", &id, "--stat"]).ok();
    assert!(
        show.stdout.contains("one.txt"),
        "the auto snapshot contains the written file\n{show}"
    );

    // Unwatch + stop.
    repo.wt().args(["daemon", "unwatch"]).arg(&repo.dir).ok();
    daemon.stop();
    poll_until(
        "daemon to remove bg.json on shutdown",
        Duration::from_secs(15),
        Duration::from_millis(100),
        || {
            if bg_json.exists() {
                Err(format!("{} still present", bg_json.display()))
            } else {
                Ok(())
            }
        },
    );
}
