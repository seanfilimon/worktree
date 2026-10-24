//! Single-user, offline workflow over the whole local CLI surface (§5).
//!
//! Note: every content modification changes the file size, so tests never
//! depend on mtime granularity of the stat cache.

use serde_json::Value;
use worktree_e2e::json::{self, assert_keys, bool_field, find_objects, parents, str_field};
use worktree_e2e::{fsutil, require_e2e, TestEnv};

fn ch(path: &str, kind: &str) -> (String, String) {
    (path.to_string(), kind.to_string())
}

const SNAPSHOT_KEYS: &[&str] = &[
    "id",
    "tree_id",
    "branch",
    "parents",
    "manifest",
    "author",
    "message",
    "created_at",
    "auto_generated",
];

#[test]
fn local_workflow() {
    require_e2e!();
    let env = TestEnv::new("local_workflow");
    let alice = env.user("alice");
    let repo = alice.init("proj");

    // ---------------------------------------------------------------- init
    assert!(repo.path(".wt").is_dir(), ".wt/ missing after init");
    assert!(
        repo.path(".wt/config.toml").is_file(),
        ".wt/config.toml missing after init"
    );
    alice.wt().arg("init").arg(&repo.dir).fails(); // already initialized
    let st = repo.status();
    assert_keys(
        &st,
        &["tree", "branch", "head", "changes", "merge"],
        "status --json",
    );
    assert_eq!(str_field(&st, "tree"), "proj");
    assert_eq!(str_field(&st, "branch"), "main");
    assert!(
        st["head"].is_null(),
        "fresh tree should have no head:\n{}",
        json::pretty(&st)
    );
    repo.assert_clean();
    assert_eq!(
        repo.wt()
            .args(["config", "get", "user.name"])
            .ok()
            .stdout
            .trim(),
        "Alice",
        "config get user.name"
    );

    // ------------------------------------------------------ first snapshot
    repo.write("README.md", "# proj\n");
    repo.write("src/lib.rs", "pub fn answer() -> u32 { 42 }\n");
    assert_eq!(
        repo.changes(),
        vec![ch("README.md", "added"), ch("src/lib.rs", "added")]
    );
    repo.snapshot("initial import");
    repo.assert_clean();
    let log = repo.log(&[]);
    assert_eq!(json::messages(&log), vec!["initial import"]);
    let first = &log[0];
    assert_keys(first, SNAPSHOT_KEYS, "snapshot record");
    assert!(parents(first).is_empty(), "root snapshot has parents");
    assert_eq!(str_field(first, "branch"), "main");
    assert!(!bool_field(first, "auto_generated"));
    assert!(
        str_field(first, "author").contains("Alice"),
        "author should come from user.name"
    );
    assert_eq!(
        str_field(first, "manifest").len(),
        64,
        "manifest hash should be 64 hex chars"
    );
    let s_initial = str_field(first, "id");
    assert_eq!(
        json::str_field(&repo.status(), "head"),
        s_initial,
        "status head == log head"
    );

    // nothing changed -> refused unless --allow-empty
    repo.wt().args(["snapshot", "-m", "noop"]).fails();
    repo.wt()
        .args(["snapshot", "-m", "empty", "--allow-empty"])
        .ok();
    let s_empty = repo.head_id();
    assert_ne!(s_empty, s_initial);
    assert_eq!(
        str_field(&repo.head(), "manifest"),
        str_field(first, "manifest"),
        "empty snapshot keeps the manifest"
    );

    // ------------------------------------------- dirty: modify/add/delete
    repo.write("README.md", "# proj\n\nA test project.\n");
    repo.write("docs/guide.md", "guide\n");
    repo.remove("src/lib.rs");
    assert_eq!(
        repo.changes(),
        vec![
            ch("README.md", "modified"),
            ch("docs/guide.md", "added"),
            ch("src/lib.rs", "deleted")
        ]
    );
    repo.snapshot("second");
    repo.assert_clean();
    let log = repo.log(&[]);
    assert_eq!(
        json::messages(&log),
        vec!["second", "empty", "initial import"],
        "log is newest first"
    );
    assert_eq!(parents(&log[0]), vec![s_empty.clone()]);
    assert_eq!(parents(&log[1]), vec![s_initial.clone()]);
    let s_second = str_field(&log[0], "id");
    assert_eq!(repo.log(&["-n", "1"]).len(), 1, "log -n 1");
    let oneline = repo.wt().args(["--no-color", "log", "--oneline"]).ok();
    assert_eq!(
        oneline.lines().len(),
        3,
        "log --oneline prints one line per snapshot\n{oneline}"
    );
    assert!(
        oneline.stdout.contains("second") && oneline.stdout.contains(&s_second[..8]),
        "{oneline}"
    );
    let show = repo.wt().args(["show", "HEAD", "--stat"]).ok();
    assert!(
        show.stdout.contains("README.md"),
        "show --stat lists changed files\n{show}"
    );
    let show_json = repo.wt().args(["show", "HEAD"]).json();
    assert!(
        json::all_strings(&show_json).contains(&s_second),
        "show --json mentions the id:\n{}",
        json::pretty(&show_json)
    );
    assert_eq!(
        str_field(&repo.log(&["HEAD~1", "-n", "1"])[0], "id"),
        s_empty,
        "HEAD~1 resolves"
    );
    assert_eq!(
        str_field(&repo.log(&[&s_second[..8], "-n", "1"])[0], "id"),
        s_second,
        "id prefix resolves"
    );

    // ------------------------------------------------- branches & switch
    repo.wt().args(["branch", "create", "feature"]).ok();
    let branches = repo.wt().args(["branch", "list"]).json();
    let names = json::names(&branches, &["is_current"]);
    assert!(
        names.contains(&"main".into()) && names.contains(&"feature".into()),
        "branches: {names:?}"
    );
    assert!(bool_field(
        json::named(&branches, "main", &["is_current"]).unwrap(),
        "is_current"
    ));
    assert!(!bool_field(
        json::named(&branches, "feature", &["is_current"]).unwrap(),
        "is_current"
    ));
    repo.wt().args(["switch", "feature"]).ok();
    assert_eq!(repo.current_branch(), "feature");
    repo.write("feature.txt", "feature work\n");
    repo.write("README.md", "# proj\n\nA test project.\nFeature notes.\n");
    repo.snapshot("feature work");
    let s_feature = repo.head_id();
    assert_eq!(str_field(&repo.head(), "branch"), "feature");

    repo.wt().args(["switch", "main"]).ok();
    assert_eq!(repo.current_branch(), "main");
    assert!(
        !repo.exists("feature.txt"),
        "switch must remove files absent on the target branch"
    );
    assert_eq!(
        repo.read("README.md"),
        "# proj\n\nA test project.\n",
        "switch must rewrite files"
    );
    assert_eq!(repo.head_id(), s_second);
    repo.assert_clean();

    // dirty switch refused; restore discards the edit
    repo.write("README.md", "uncommitted edit that is longer than before\n");
    repo.wt().args(["switch", "feature"]).fails();
    assert_eq!(repo.current_branch(), "main");
    assert_eq!(
        repo.read("README.md"),
        "uncommitted edit that is longer than before\n",
        "failed switch must not touch files"
    );
    repo.wt().args(["restore", "README.md"]).ok();
    assert_eq!(repo.read("README.md"), "# proj\n\nA test project.\n");
    repo.assert_clean();

    // ------------------------------------------------ fast-forward merge
    repo.wt().args(["merge", "feature"]).ok();
    assert_eq!(
        repo.head_id(),
        s_feature,
        "fast-forward moves main to feature's tip"
    );
    assert_eq!(repo.read("feature.txt"), "feature work\n");
    assert!(repo.read("README.md").contains("Feature notes."));
    repo.assert_clean();

    // ------------------------------------------------- clean 3-way merge
    let ten: Vec<String> = (1..=10).map(|i| format!("line {i}")).collect();
    repo.write("lines.txt", ten.join("\n") + "\n");
    repo.snapshot("add lines");
    repo.wt().args(["switch", "-c", "topic"]).ok();
    assert_eq!(repo.current_branch(), "topic");
    let mut topic_lines = ten.clone();
    topic_lines[1] = "line 2 (changed on topic)".into();
    repo.write("lines.txt", topic_lines.join("\n") + "\n");
    repo.write("topic.txt", "topic only\n");
    repo.snapshot("topic change");
    let s_topic = repo.head_id();
    repo.wt().args(["switch", "main"]).ok();
    let mut main_lines = ten.clone();
    main_lines[8] = "line 9 (changed on main)".into();
    repo.write("lines.txt", main_lines.join("\n") + "\n");
    repo.write("main.txt", "main only\n");
    repo.snapshot("main change");
    let s_main = repo.head_id();
    repo.wt().args(["merge", "topic", "-m", "merge topic"]).ok();
    let merged = repo.head();
    assert_eq!(str_field(&merged, "message"), "merge topic");
    let mp = parents(&merged);
    assert_eq!(
        mp.len(),
        2,
        "merge snapshot has two parents:\n{}",
        json::pretty(&merged)
    );
    assert_eq!(mp[0], s_main, "first parent is the target branch");
    assert_eq!(mp[1], s_topic, "second parent is the merged branch");
    let mut expect = ten.clone();
    expect[1] = topic_lines[1].clone();
    expect[8] = main_lines[8].clone();
    assert_eq!(
        repo.read("lines.txt"),
        expect.join("\n") + "\n",
        "both sides' hunks merged"
    );
    assert!(repo.exists("topic.txt") && repo.exists("main.txt"));
    repo.assert_clean();

    // ---------------------------------------------------- merge conflict
    repo.write("conflict.txt", "alpha\nshared line\nomega\n");
    repo.snapshot("add conflict.txt");
    repo.wt().args(["switch", "-c", "clash"]).ok();
    repo.write("conflict.txt", "alpha\nclash side\nomega\n");
    repo.snapshot("clash edit");
    let s_clash = repo.head_id();
    repo.wt().args(["switch", "main"]).ok();
    repo.write("conflict.txt", "alpha\nmain side!!\nomega\n");
    repo.snapshot("main edit");
    let s_main = repo.head_id();

    let out = repo.wt().args(["merge", "clash"]).code(3);
    assert!(
        out.mentions("conflict.txt"),
        "conflicting path reported\n{out}"
    );
    let text = repo.read("conflict.txt");
    for marker in ["<<<<<<<", "=======", ">>>>>>>", "clash side", "main side!!"] {
        assert!(
            text.contains(marker),
            "conflict file lacks {marker:?}:\n{text}"
        );
    }
    let st = repo.status();
    assert!(
        !st["merge"].is_null(),
        "status must report the merge in progress:\n{}",
        json::pretty(&st)
    );
    repo.wt().args(["snapshot", "-m", "with markers"]).fails();
    assert_eq!(repo.head_id(), s_main, "no snapshot while markers remain");

    repo.wt().args(["merge", "--abort"]).ok();
    assert_eq!(
        repo.read("conflict.txt"),
        "alpha\nmain side!!\nomega\n",
        "abort restores pre-merge content"
    );
    assert_eq!(repo.head_id(), s_main);
    assert!(
        repo.status()["merge"].is_null(),
        "merge state cleared by --abort"
    );
    repo.assert_clean();

    repo.wt().args(["merge", "clash"]).code(3);
    repo.write("conflict.txt", "alpha\nresolved: both sides\nomega\n");
    repo.wt()
        .args(["merge", "--continue", "-m", "resolve clash"])
        .ok();
    let resolved = repo.head();
    assert_eq!(str_field(&resolved, "message"), "resolve clash");
    assert_eq!(parents(&resolved), vec![s_main.clone(), s_clash.clone()]);
    assert_eq!(
        repo.read("conflict.txt"),
        "alpha\nresolved: both sides\nomega\n"
    );
    assert!(repo.status()["merge"].is_null());
    repo.assert_clean();

    // ------------------------------------------------------------ revert
    repo.write("revert-me.txt", "temporary\n");
    repo.snapshot("add revert-me");
    let s_rm = repo.head_id();
    repo.wt().args(["revert", "HEAD"]).ok();
    let reverted = repo.head();
    assert_ne!(
        str_field(&reverted, "id"),
        s_rm,
        "revert creates a new snapshot"
    );
    assert_eq!(
        parents(&reverted),
        vec![s_rm.clone()],
        "revert is append-only"
    );
    assert!(
        !repo.exists("revert-me.txt"),
        "revert removes the added file"
    );
    assert!(
        json::ids(&repo.log(&[])).contains(&s_rm),
        "reverted snapshot stays in history"
    );
    repo.assert_clean();

    // ----------------------------------------------------------- restore
    let readme_v2 = repo.read("README.md");
    repo.write(
        "README.md",
        "# proj v3\n\nRewritten readme, third version.\n",
    );
    repo.snapshot("readme v3");
    let s_v3 = repo.head_id();
    repo.wt()
        .args(["restore", "README.md", "--from", "HEAD~1"])
        .ok();
    assert_eq!(repo.read("README.md"), readme_v2, "restore --from HEAD~1");
    assert_eq!(repo.changes(), vec![ch("README.md", "modified")]);
    repo.wt().args(["restore", "README.md"]).ok();
    repo.remove("lines.txt");
    repo.write("main.txt", "scribbled over, longer than before\n");
    repo.wt().arg("restore").ok(); // all paths
    assert_eq!(
        repo.read("lines.txt"),
        expect.join("\n") + "\n",
        "restore brings back deleted files"
    );
    assert_eq!(repo.read("main.txt"), "main only\n");
    repo.assert_clean();
    assert_eq!(repo.head_id(), s_v3, "restore never moves head");

    // -------------------------------------------------------------- tags
    repo.wt().args(["tag", "create", "v1.0"]).ok();
    repo.wt()
        .args(["tag", "create", "v0.9", "HEAD~1", "-m", "previous"])
        .ok();
    let tags = repo.wt().args(["tag", "list"]).json();
    let t1 = json::named(&tags, "v1.0", &["target"])
        .unwrap_or_else(|| panic!("v1.0 missing:\n{}", json::pretty(&tags)));
    assert_eq!(str_field(t1, "target"), s_v3);
    let t09 = json::named(&tags, "v0.9", &["target"])
        .unwrap_or_else(|| panic!("v0.9 missing:\n{}", json::pretty(&tags)));
    assert_eq!(str_field(t09, "target"), str_field(&reverted, "id"));
    assert_eq!(json::opt_str(t09, "message").as_deref(), Some("previous"));
    assert_eq!(
        str_field(&repo.log(&["v0.9", "-n", "1"])[0], "id"),
        str_field(&reverted, "id"),
        "tags resolve as revisions"
    );
    repo.wt().args(["tag", "create", "v1.0"]).fails(); // duplicate
    repo.wt().args(["tag", "delete", "v1.0"]).ok();
    let tags = repo.wt().args(["tag", "list"]).json();
    assert_eq!(json::names(&tags, &["target"]), vec!["v0.9".to_string()]);

    // -------------------------------------------------------------- diff
    repo.write(
        "README.md",
        "# proj v3\n\nRewritten readme, third version.\nextra diff line\n",
    );
    let d = repo.wt().arg("diff").ok();
    assert!(
        d.stdout.contains("+extra diff line"),
        "unified diff shows the added line\n{d}"
    );
    assert!(d.stdout.contains("README.md"), "{d}");
    let names_only = repo.wt().args(["diff", "--name-only"]).ok();
    assert_eq!(
        names_only.lines(),
        vec!["README.md"],
        "diff --name-only\n{names_only}"
    );
    repo.wt().args(["diff", "--stat"]).ok();
    let dj = repo.wt().arg("diff").json();
    let files = find_objects(&dj, &["path", "kind"]);
    assert_eq!(files.len(), 1, "diff --json:\n{}", json::pretty(&dj));
    assert_eq!(str_field(files[0], "path"), "README.md");
    assert_eq!(str_field(files[0], "kind"), "modified");
    repo.wt().args(["restore", "README.md"]).ok();
    let between = repo
        .wt()
        .args(["diff", "HEAD~1", "HEAD", "--name-only"])
        .ok();
    assert_eq!(
        between.lines(),
        vec!["README.md"],
        "diff REV REV2\n{between}"
    );
    let scoped = repo
        .wt()
        .args(["diff", "HEAD~3", "HEAD", "--name-only", "README.md"])
        .ok();
    assert_eq!(
        scoped.lines(),
        vec!["README.md"],
        "diff PATH filter\n{scoped}"
    );

    // ------------------------------------------------------------ ignore
    repo.wt().args(["ignore", "add", "*.log"]).ok();
    let patterns = repo.wt().args(["ignore", "list"]).json();
    assert!(
        json::all_strings(&patterns).iter().any(|p| p == "*.log"),
        "ignore list:\n{}",
        json::pretty(&patterns)
    );
    repo.write("debug.log", "noise\n");
    repo.write("target/out.bin", "build output\n");
    repo.write("node_modules/dep/index.js", "module.exports = 1;\n");
    repo.write(".wtignore", "secret-dir/\n");
    repo.write("secret-dir/a.txt", "hidden\n");
    repo.write("notes.txt", "tracked notes\n");
    assert_eq!(
        repo.changes(),
        vec![ch(".wtignore", "added"), ch("notes.txt", "added")],
        "ignored paths must not show up"
    );
    let check = repo.wt().args(["ignore", "check", "debug.log"]).json();
    if let Some(ignored) = json::find_bool(&check, "ignored") {
        assert!(
            ignored,
            "debug.log should be ignored:\n{}",
            json::pretty(&check)
        );
    }
    let not_ignored = repo
        .wt()
        .args(["--json", "ignore", "check", "notes.txt"])
        .run();
    if not_ignored.success() {
        if let Some(ignored) = json::find_bool(&not_ignored.json(), "ignored") {
            assert!(!ignored, "notes.txt must not be ignored\n{not_ignored}");
        }
    } else {
        not_ignored.assert_code(1);
    }
    repo.snapshot("add notes");
    repo.assert_clean();
    assert!(repo.exists("debug.log"), "ignored files are left alone");
    let s_notes = repo.head_id();

    // ------------------------------------------------------ nested trees
    repo.wt().args(["tree", "add", "lib", "libs/core"]).ok();
    assert!(
        repo.path("libs/core/.wt-tree").is_dir(),
        "tree add writes the .wt-tree marker"
    );
    let trees = repo.wt().args(["tree", "list"]).json();
    let tree_names = json::names(&trees, &["current_branch"]);
    assert!(
        tree_names.contains(&"proj".into()) && tree_names.contains(&"lib".into()),
        "trees: {tree_names:?}"
    );
    repo.write("libs/core/core.txt", "core v1\n");
    repo.assert_clean(); // nested tree paths are excluded from the root tree
    let lib = repo.nested("lib");
    assert_eq!(
        lib.changes(),
        vec![ch("core.txt", "added")],
        "nested tree paths are relative to its root"
    );
    let auto = repo
        .user
        .wt_in(&repo.path("libs/core"))
        .arg("status")
        .json();
    assert_eq!(
        str_field(json::status_report(&auto, Some("lib")), "tree"),
        "lib",
        "cwd inside the nested tree selects it"
    );
    lib.snapshot("lib v1");
    lib.assert_clean();
    let lib_log = lib.log(&[]);
    assert_eq!(json::messages(&lib_log), vec!["lib v1"]);
    assert_ne!(
        str_field(&lib_log[0], "tree_id"),
        str_field(&repo.head(), "tree_id"),
        "trees have distinct ids"
    );
    assert_eq!(
        repo.head_id(),
        s_notes,
        "snapshotting the nested tree leaves the root tree alone"
    );
    repo.assert_clean();

    // ----------------------------------------------------------- archive
    let tgz = env.path("out/proj.tar.gz");
    std::fs::create_dir_all(tgz.parent().unwrap()).unwrap();
    repo.wt().arg("archive").arg("-o").arg(&tgz).ok();
    let entries = fsutil::tar_gz_entries(&tgz);
    let readme = entries
        .iter()
        .find(|(k, _)| *k == "README.md" || k.ends_with("/README.md"))
        .unwrap_or_else(|| {
            panic!(
                "README.md not in archive: {:?}",
                entries.keys().collect::<Vec<_>>()
            )
        });
    assert_eq!(readme.1.as_slice(), repo.read("README.md").as_bytes());
    assert!(entries.keys().any(|k| k.ends_with("notes.txt")));
    assert!(
        !entries
            .keys()
            .any(|k| k.contains(".wt/") || k.ends_with("debug.log")),
        "archive leaks metadata/ignored files: {:?}",
        entries.keys()
    );
    let zip = env.path("out/proj.zip");
    repo.wt()
        .args(["archive", "HEAD~1", "--format", "zip", "-o"])
        .arg(&zip)
        .ok();
    let bytes = fsutil::read(&zip);
    assert!(bytes.starts_with(b"PK\x03\x04"), "zip archive magic");
    assert!(
        bytes.windows(9).any(|w| w == b"README.md"),
        "zip contains README.md"
    );

    // ------------------------------------------------------------ reflog
    let reflog = repo.wt().arg("reflog").json();
    let entries = find_objects(&reflog, &["action", "branch"]);
    let actions: Vec<String> = entries.iter().map(|e| str_field(e, "action")).collect();
    for a in ["init", "snapshot", "switch", "merge", "revert"] {
        assert!(
            actions.iter().any(|x| x == a),
            "reflog lacks {a:?}: {actions:?}"
        );
    }
    assert!(
        actions.iter().any(|x| x == "fast-forward"),
        "reflog lacks the fast-forward: {actions:?}"
    );
    assert_eq!(
        actions.last().map(String::as_str),
        Some("init"),
        "reflog is newest first: {actions:?}"
    );
    let short = repo.wt().args(["reflog", "-n", "2"]).json();
    assert_eq!(
        find_objects(&short, &["action", "branch"]).len(),
        2,
        "reflog -n 2"
    );

    // ------------------------------------------------- -C and usage errors
    let elsewhere = alice.wt().arg("-C").arg(&repo.dir).arg("log").json();
    assert_eq!(
        str_field(&json::snapshots(&elsewhere)[0], "id"),
        s_notes,
        "-C <path>"
    );
    alice.wt().arg("log").fails(); // not inside a worktree
    let usage = repo.wt().arg("definitely-not-a-command").run();
    usage.assert_code(2);

    // branch archive / restore
    repo.wt().args(["branch", "archive", "topic"]).ok();
    let listed = json::names(&repo.wt().args(["branch", "list"]).json(), &["is_current"]);
    assert!(
        !listed.contains(&"topic".into()),
        "archived branches are hidden: {listed:?}"
    );
    let all: Value = repo.wt().args(["branch", "list", "-a"]).json();
    assert!(
        json::names(&all, &["is_current"]).contains(&"topic".into()),
        "branch list -a shows archived"
    );
    repo.wt().args(["branch", "archive", "main"]).fails(); // current branch
    repo.wt().args(["branch", "restore", "topic"]).ok();
    assert!(
        json::names(&repo.wt().args(["branch", "list"]).json(), &["is_current"])
            .contains(&"topic".into())
    );
}
