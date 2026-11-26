//! Stock `git` against the real server through `git-remote-wt`:
//! `git clone wt::http://host/tenant/tree`, commit, `git push` (seen by
//! `wt clone`), `wt push` (seen by `git fetch`), tags, and the refusals —
//! history is append-only and unsupported content is never dropped.

use std::path::Path;

use worktree_e2e::bins;
use worktree_e2e::json::{self, str_field};
use worktree_e2e::{require_e2e, Cmd, Git, TestEnv, User};

use crate::common::{publish_tree, server_tip, TREE};

/// `git` with the helper on `PATH` and `user`'s saved login.
fn git_as(git: &Git, user: &User, dir: &Path) -> Cmd {
    let helper = bins::git_remote_bin();
    let mut paths = vec![helper
        .parent()
        .expect("helper has a parent directory")
        .to_path_buf()];
    if let Some(path) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&path));
    }
    git.cmd(dir)
        .env("PATH", std::env::join_paths(paths).expect("PATH"))
        .env("WT_HOME", &user.home)
        .env_remove("WT_TOKEN")
        // A local time zone, as real users have.
        .env("TZ", "Asia/Tokyo")
}

fn out(git: &Git, user: &User, dir: &Path, args: &[&str]) -> String {
    git_as(git, user, dir)
        .args(args)
        .ok()
        .stdout
        .trim()
        .to_string()
}

#[test]
fn git_remote_helper() {
    require_e2e!();
    assert!(
        Git::available(),
        "the git_remote_helper scenario needs `git` on PATH"
    );
    let env = TestEnv::with_server("git_remote_helper");
    let git = env.git();
    let alice = env.user("alice");
    let wt = publish_tree(
        &env,
        &alice,
        &[("README.md", "# shared\n"), ("src/app.txt", "app v1\n")],
    );
    let api = alice.api();
    let remote = format!("wt::{}/alice/{TREE}", env.url());
    let work_area = env.path("git");
    std::fs::create_dir_all(&work_area).unwrap();

    // ------------------------------------------------ git clone
    git_as(&git, &alice, &work_area)
        .args(["clone", "-q", &remote, "work"])
        .ok();
    let work = work_area.join("work");
    assert_eq!(
        std::fs::read_to_string(work.join("src/app.txt")).unwrap(),
        "app v1\n"
    );
    assert_eq!(out(&git, &alice, &work, &["log", "--format=%s"]), "initial");
    assert_eq!(
        out(&git, &alice, &work, &["symbolic-ref", "--short", "HEAD"]),
        "main"
    );
    let cloned_head = out(&git, &alice, &work, &["rev-parse", "HEAD"]);
    // A second clone computes the same commit id.
    git_as(&git, &alice, &work_area)
        .args(["clone", "-q", &remote, "other"])
        .ok();
    let other = work_area.join("other");
    assert_eq!(
        out(&git, &alice, &other, &["rev-parse", "HEAD"]),
        cloned_head
    );

    // ------------------------------------------------ git commit + push -> wt clone
    std::fs::create_dir_all(work.join("docs")).unwrap();
    std::fs::write(work.join("docs/guide.md"), "# Guide\n").unwrap();
    git_as(&git, &alice, &work).args(["add", "-A"]).ok();
    git_as(&git, &alice, &work)
        .args(["commit", "-q", "-m", "add the guide"])
        .ok();
    let pushed = out(&git, &alice, &work, &["rev-parse", "HEAD"]);
    git_as(&git, &alice, &work)
        .args(["push", "-q", "origin", "main"])
        .ok();
    assert_eq!(
        out(&git, &alice, &work, &["rev-parse", "origin/main"]),
        pushed
    );

    let verify = alice.clone_tree("alice", TREE, "verify");
    assert_eq!(verify.read("docs/guide.md"), "# Guide\n");
    let log = verify.log(&[]);
    assert_eq!(log.len(), 2, "{log:?}");
    assert_eq!(str_field(&log[0], "message"), "add the guide");
    assert!(
        str_field(&log[0], "author").contains("git.author@example.com"),
        "{}",
        json::pretty(&log[0])
    );
    assert_eq!(
        server_tip(&api, "alice", TREE, "main").as_deref(),
        Some(verify.head_id().as_str())
    );
    // The pusher keeps its commit: fetching again changes nothing.
    git_as(&git, &alice, &work)
        .args(["fetch", "-q", "origin"])
        .ok();
    assert_eq!(
        out(&git, &alice, &work, &["rev-parse", "origin/main"]),
        pushed
    );

    // ------------------------------------------------ wt push -> git fetch
    wt.pull();
    wt.commit(&[("src/app.txt", "app v2\n")], "app v2 from wt");
    wt.push();
    git_as(&git, &alice, &work)
        .args(["fetch", "-q", "origin"])
        .ok();
    assert_eq!(
        out(&git, &alice, &work, &["log", "--format=%s", "origin/main"]),
        "app v2 from wt\nadd the guide\ninitial"
    );
    assert_eq!(
        out(&git, &alice, &work, &["rev-parse", "origin/main~1"]),
        pushed
    );
    git_as(&git, &alice, &work)
        .args(["merge", "-q", "--ff-only", "origin/main"])
        .ok();
    assert_eq!(
        std::fs::read_to_string(work.join("src/app.txt")).unwrap(),
        "app v2\n"
    );
    git_as(&git, &alice, &other)
        .args(["pull", "-q", "--ff-only"])
        .ok();
    assert_eq!(
        out(&git, &alice, &other, &["rev-parse", "HEAD^{tree}"]),
        out(&git, &alice, &work, &["rev-parse", "HEAD^{tree}"])
    );

    // ------------------------------------------------ tags
    git_as(&git, &alice, &work)
        .args(["tag", "-a", "v1.0", "-m", "first release"])
        .ok();
    git_as(&git, &alice, &work)
        .args(["push", "-q", "origin", "v1.0"])
        .ok();
    git_as(&git, &alice, &other)
        .args(["fetch", "-q", "--tags", "origin"])
        .ok();
    assert_eq!(
        out(&git, &alice, &other, &["cat-file", "-t", "v1.0"]),
        "tag"
    );
    assert_eq!(
        out(&git, &alice, &other, &["rev-parse", "v1.0^{commit}"]),
        out(&git, &alice, &other, &["rev-parse", "HEAD"])
    );
    wt.pull();
    let tags = wt.wt().args(["tag", "list"]).json();
    assert!(
        json::all_strings(&tags).iter().any(|s| s == "v1.0"),
        "wt sees the tag pushed with git:\n{}",
        json::pretty(&tags)
    );

    // ------------------------------------------------ refusals
    let tip = server_tip(&api, "alice", TREE, "main");

    // A symbolic link is refused, not dropped.
    let blob = out(&git, &alice, &work, &["rev-parse", "HEAD:README.md"]);
    git_as(&git, &alice, &work)
        .args(["update-index", "--add", "--cacheinfo"])
        .arg(format!("120000,{blob},docs/link"))
        .ok();
    git_as(&git, &alice, &work)
        .args(["commit", "-q", "-m", "add a link"])
        .ok();
    let refused = git_as(&git, &alice, &work)
        .args(["push", "origin", "main"])
        .fails();
    assert!(refused.mentions("docs/link (symbolic link)"), "{refused}");
    assert_eq!(server_tip(&api, "alice", TREE, "main"), tip);

    // Rewritten history is refused, with and without --force.
    git_as(&git, &alice, &work)
        .args(["reset", "-q", "--hard", "origin/main"])
        .ok();
    std::fs::write(work.join("README.md"), "# rewritten\n").unwrap();
    git_as(&git, &alice, &work)
        .args(["commit", "-q", "-a", "--amend", "-m", "rewritten"])
        .ok();
    let plain = git_as(&git, &alice, &work)
        .args(["push", "origin", "main"])
        .fails();
    assert!(plain.mentions("rejected"), "{plain}");
    let forced = git_as(&git, &alice, &work)
        .args(["push", "--force", "origin", "main"])
        .fails();
    assert!(forced.mentions("append-only"), "{forced}");
    assert_eq!(server_tip(&api, "alice", TREE, "main"), tip);

    // Without a login nothing can be pushed.
    let nobody = env.user("nobody");
    git_as(&git, &alice, &work)
        .args(["reset", "-q", "--hard", "origin/main"])
        .ok();
    std::fs::write(work.join("anon.txt"), "x\n").unwrap();
    git_as(&git, &alice, &work).args(["add", "-A"]).ok();
    git_as(&git, &alice, &work)
        .args(["commit", "-q", "-m", "anonymous"])
        .ok();
    let anonymous = git_as(&git, &nobody, &work)
        .args(["push", "origin", "main"])
        .fails();
    assert!(
        anonymous.mentions("wt login") || anonymous.mentions("not found"),
        "{anonymous}"
    );
    assert_eq!(server_tip(&api, "alice", TREE, "main"), tip);
}
