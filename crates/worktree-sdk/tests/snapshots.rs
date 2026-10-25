//! Status/snapshot cycles, ignore rules, history and revision resolution.

mod common;

use chrono::Utc;
use common::{opts, Fixture};
use worktree_sdk::{ChangeKind, FileChange, SdkError, Snapshot, SnapshotOptions, TagInfo};

fn changes(fx: &Fixture) -> Vec<(String, ChangeKind)> {
    fx.repo
        .status(&fx.tree)
        .unwrap()
        .changes
        .into_iter()
        .map(|c| (c.path, c.kind))
        .collect()
}

#[test]
fn snapshot_status_cycle() {
    let fx = Fixture::new();
    let status = fx.repo.status(&fx.tree).unwrap();
    assert!(status.is_clean());
    assert_eq!(status.head, None);
    assert_eq!(status.branch, "main");
    assert!(matches!(
        fx.repo.snapshot(&fx.tree, opts("empty")),
        Err(SdkError::NoChanges)
    ));

    fx.write("README.md", "hello\n");
    fx.write("src/main.rs", "fn main() {}\n");
    assert_eq!(
        changes(&fx),
        [
            ("README.md".to_string(), ChangeKind::Added),
            ("src/main.rs".to_string(), ChangeKind::Added)
        ]
    );
    let first = fx.snap("first");
    assert!(first.parents.is_empty());
    assert_eq!(first.branch, "main");
    assert_eq!(first.tree_id, fx.repo.root_tree().unwrap().id);
    assert!(fx.repo.status(&fx.tree).unwrap().is_clean());
    assert_eq!(
        fx.repo.branch_tip(&fx.tree, "main").unwrap(),
        Some(first.id.clone())
    );
    assert!(matches!(
        fx.repo.snapshot(&fx.tree, opts("again")),
        Err(SdkError::NoChanges)
    ));

    fx.write("README.md", "hello world\n");
    fx.remove("src/main.rs");
    fx.write("src/lib.rs", "pub fn x() {}\n");
    assert_eq!(
        changes(&fx),
        [
            ("README.md".to_string(), ChangeKind::Modified),
            ("src/lib.rs".to_string(), ChangeKind::Added),
            ("src/main.rs".to_string(), ChangeKind::Deleted),
        ]
    );
    let second = fx.snap("second");
    assert_eq!(second.parents, vec![first.id.clone()]);
    assert!(fx.repo.status(&fx.tree).unwrap().is_clean());

    let empty = fx
        .repo
        .snapshot(
            &fx.tree,
            SnapshotOptions {
                allow_empty: true,
                ..opts("checkpoint")
            },
        )
        .unwrap();
    assert_eq!(empty.manifest, second.manifest);
    assert_eq!(empty.parents, vec![second.id.clone()]);

    let manifest = fx.repo.manifest_of(&second).unwrap();
    let paths: Vec<_> = manifest.entries.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(paths, ["README.md", "src/lib.rs"]);
    assert_eq!(
        fx.repo.read_file_at(&second, "README.md").unwrap(),
        b"hello world\n"
    );
    assert_eq!(
        fx.repo.read_file_at(&first, "src/main.rs").unwrap(),
        b"fn main() {}\n"
    );
    assert!(fx.repo.read_file_at(&second, "src/main.rs").is_err());
}

#[test]
fn same_size_rewrite_is_detected_despite_index() {
    let fx = Fixture::new();
    fx.write("a.txt", "aaaa");
    fx.snap("one");
    assert!(fx.repo.status(&fx.tree).unwrap().is_clean());
    // Same size, written immediately: mtime may be identical at coarse
    // timestamp resolution; the racy-entry rule must still catch it.
    fx.write("a.txt", "bbbb");
    assert_eq!(changes(&fx), [("a.txt".to_string(), ChangeKind::Modified)]);
    fx.write("a.txt", "aaaa");
    assert!(fx.repo.status(&fx.tree).unwrap().is_clean());
}

#[test]
fn snapshot_options_are_honored() {
    let fx = Fixture::new();
    fx.write("x.txt", "x");
    let s = fx
        .repo
        .snapshot(
            &fx.tree,
            SnapshotOptions {
                message: "auto: 1 files changed".into(),
                author: Some("bot".into()),
                auto_generated: true,
                allow_empty: false,
                skip_secret_scan: true,
                dependencies: Vec::new(),
                skip_hooks: false,
            },
        )
        .unwrap();
    assert_eq!(s.author, "bot");
    assert!(s.auto_generated);
    let log = fx.repo.reflog(&fx.tree, Some(1)).unwrap();
    assert_eq!(log[0].action, "auto-snapshot");
    assert_eq!(log[0].new.as_deref(), Some(s.id.as_str()));
}

#[test]
fn ignore_rules_apply_to_status() {
    let fx = Fixture::new();
    fx.write("keep.txt", "k");
    fx.write("target/debug/out.bin", "x");
    fx.write("web/node_modules/pkg/index.js", "x");
    fx.write(".git/HEAD", "ref");
    fx.write("notes.swp", "x");
    fx.write("app.log", "log");
    fx.write("logs/keep.log", "log");
    fx.write("logs/.wtignore", "!keep.log\n");
    fx.write(".wtignore", "*.log\n");
    fx.repo.add_ignore_pattern("secret/").unwrap();
    fx.repo.add_ignore_pattern("secret/").unwrap(); // idempotent
    fx.write("secret/key.pem", "x");

    let paths: Vec<_> = changes(&fx).into_iter().map(|(p, _)| p).collect();
    // §13.9: root `.wtignore` patterns cannot be re-included by a nested
    // `!pattern`, so logs/keep.log stays ignored.
    assert_eq!(paths, [".wtignore", "keep.txt", "logs/.wtignore"]);

    assert!(fx.repo.is_ignored("app.log").unwrap());
    assert!(fx.repo.is_ignored("logs/keep.log").unwrap());
    assert!(fx.repo.is_ignored("secret/key.pem").unwrap());
    assert!(fx.repo.is_ignored("target/debug/out.bin").unwrap());
    assert!(fx.repo.is_ignored(".wt/config.toml").unwrap());
    assert!(!fx.repo.is_ignored("keep.txt").unwrap());
    assert!(!fx.repo.is_ignored("").unwrap());

    let patterns = fx.repo.ignore_patterns().unwrap();
    for expected in [
        ".wt/",
        ".wt-tree/",
        "target/",
        "secret/",
        "*.log",
        "!logs/**/keep.log",
    ] {
        assert!(
            patterns.iter().any(|p| p == expected),
            "missing {expected} in {patterns:?}"
        );
    }
    assert_eq!(patterns.iter().filter(|p| *p == "secret/").count(), 1);
    assert!(fx.repo.add_ignore_pattern("   ").is_err());

    fx.snap("with ignores");
    assert!(fx.repo.status(&fx.tree).unwrap().is_clean());
}

#[test]
fn log_resolve_and_prefixes() {
    let fx = Fixture::new();
    let mut ids = Vec::new();
    for i in 0..4 {
        fx.write("f.txt", &format!("v{i}"));
        ids.push(fx.snap(&format!("s{i}")).id);
    }
    let log = fx.repo.log(&fx.tree, None, None).unwrap();
    let logged: Vec<_> = log.iter().map(|s| s.id.clone()).rev().collect();
    assert_eq!(logged, ids);
    assert_eq!(fx.repo.log(&fx.tree, None, Some(2)).unwrap().len(), 2);
    assert_eq!(
        fx.repo.log(&fx.tree, Some("HEAD~2"), None).unwrap()[0].id,
        ids[1]
    );

    let r = |rev: &str| fx.repo.resolve(&fx.tree, rev).map(|s| s.id);
    assert_eq!(r("HEAD").unwrap(), ids[3]);
    assert_eq!(r("@").unwrap(), ids[3]);
    assert_eq!(r("HEAD~1").unwrap(), ids[2]);
    assert_eq!(r("main~3").unwrap(), ids[0]);
    assert_eq!(r("HEAD^").unwrap(), ids[2]);
    assert_eq!(r("HEAD~1^1").unwrap(), ids[1]);
    assert_eq!(r(&ids[1]).unwrap(), ids[1]);
    assert_eq!(r(&ids[1][..8]).unwrap(), ids[1]);
    assert!(matches!(r("HEAD~4"), Err(SdkError::InvalidRevision(_))));
    assert!(matches!(r("HEAD^2"), Err(SdkError::InvalidRevision(_))));
    assert!(matches!(
        r("no-such-branch"),
        Err(SdkError::InvalidRevision(_))
    ));

    assert_eq!(fx.repo.get_snapshot(&ids[2][..6]).unwrap().id, ids[2]);
    assert!(matches!(
        fx.repo.get_snapshot(&ids[2][..3]),
        Err(SdkError::SnapshotNotFound(_))
    ));
    assert!(matches!(
        fx.repo.get_snapshot("ffffffff-none"),
        Err(SdkError::SnapshotNotFound(_))
    ));
    assert!(fx.repo.has_snapshot(&ids[0]));
    assert!(!fx.repo.has_snapshot(&ids[0][..8]));
    assert!(!fx.repo.has_snapshot("../../etc"));

    // Ancestry: parents before children; is_ancestor is reflexive.
    let anc: Vec<_> = fx
        .repo
        .ancestry(&ids[3])
        .unwrap()
        .into_iter()
        .map(|s| s.id)
        .collect();
    assert_eq!(anc, ids);
    assert!(fx.repo.is_ancestor(&ids[0], &ids[3]).unwrap());
    assert!(fx.repo.is_ancestor(&ids[3], &ids[3]).unwrap());
    assert!(!fx.repo.is_ancestor(&ids[3], &ids[0]).unwrap());
}

#[test]
fn ambiguous_prefix_is_reported() {
    let fx = Fixture::new();
    fx.write("f.txt", "x");
    let base = fx.snap("base");
    for suffix in ["1111", "2222"] {
        let snap = Snapshot {
            id: format!("abcd{suffix}-0000-4000-8000-000000000000"),
            ..base.clone()
        };
        fx.repo.write_snapshot(&snap).unwrap();
        fx.repo.write_snapshot(&snap).unwrap(); // idempotent
    }
    assert!(matches!(
        fx.repo.get_snapshot("abcd"),
        Err(SdkError::AmbiguousId(_))
    ));
    assert_eq!(
        fx.repo.get_snapshot("abcd1").unwrap().id,
        "abcd1111-0000-4000-8000-000000000000"
    );
    assert!(fx
        .repo
        .write_snapshot(&Snapshot {
            id: "../evil".into(),
            ..base
        })
        .is_err());
}

#[test]
fn tags_and_remote_refs_resolve() {
    let fx = Fixture::new();
    fx.write("f.txt", "1");
    let one = fx.snap("one");
    fx.write("f.txt", "2");
    let two = fx.snap("two");

    let tag = fx
        .repo
        .create_tag(&fx.tree, "v1.0", Some("HEAD~1"), Some("first release"))
        .unwrap();
    assert_eq!(tag.target, one.id);
    assert_eq!(tag.message.as_deref(), Some("first release"));
    fx.repo
        .create_tag(&fx.tree, "release/latest", None, None)
        .unwrap();
    assert!(matches!(
        fx.repo.create_tag(&fx.tree, "v1.0", None, None),
        Err(SdkError::TagExists(_))
    ));
    assert!(fx.repo.create_tag(&fx.tree, "bad tag", None, None).is_err());

    let names: Vec<_> = fx
        .repo
        .tags(&fx.tree)
        .unwrap()
        .into_iter()
        .map(|t| t.name)
        .collect();
    assert_eq!(names, ["release/latest", "v1.0"]);
    assert_eq!(fx.repo.resolve(&fx.tree, "v1.0").unwrap().id, one.id);
    assert_eq!(
        fx.repo.resolve(&fx.tree, "release/latest").unwrap().id,
        two.id
    );
    assert_eq!(
        fx.repo.resolve(&fx.tree, "release/latest~1").unwrap().id,
        one.id
    );

    fx.repo.delete_tag(&fx.tree, "release/latest").unwrap();
    assert!(matches!(
        fx.repo.delete_tag(&fx.tree, "release/latest"),
        Err(SdkError::TagNotFound(_))
    ));
    assert_eq!(fx.repo.tags(&fx.tree).unwrap().len(), 1);

    let imported = TagInfo {
        name: "imported".into(),
        target: two.id.clone(),
        message: None,
        tagger: "git".into(),
        created_at: Utc::now(),
    };
    fx.repo.put_tag(&fx.tree, &imported).unwrap();
    assert_eq!(fx.repo.resolve(&fx.tree, "imported").unwrap().id, two.id);

    fx.repo
        .set_remote_tip(&fx.tree, "origin", "main", Some(&one.id))
        .unwrap();
    fx.repo
        .set_remote_tip(&fx.tree, "origin", "feature/x", Some(&two.id))
        .unwrap();
    assert_eq!(
        fx.repo.remote_tip(&fx.tree, "origin", "main").unwrap(),
        Some(one.id.clone())
    );
    assert_eq!(fx.repo.resolve(&fx.tree, "origin/main").unwrap().id, one.id);
    assert_eq!(
        fx.repo.resolve(&fx.tree, "origin/feature/x").unwrap().id,
        two.id
    );
    assert_eq!(
        fx.repo.remote_branches(&fx.tree, "origin").unwrap(),
        vec![
            ("feature/x".to_string(), two.id.clone()),
            ("main".to_string(), one.id.clone())
        ]
    );
    let main = fx.repo.branches(&fx.tree, false).unwrap().remove(0);
    assert_eq!(main.upstream_tip, Some(one.id.clone()));
    fx.repo
        .set_remote_tip(&fx.tree, "origin", "feature/x", None)
        .unwrap();
    assert_eq!(
        fx.repo.remote_tip(&fx.tree, "origin", "feature/x").unwrap(),
        None
    );
    assert_eq!(
        fx.repo.remote_tip(&fx.tree, "upstream", "main").unwrap(),
        None
    );
}

#[test]
fn reflog_is_newest_first_and_limited() {
    let fx = Fixture::new();
    fx.write("a", "1");
    let s1 = fx.snap("one");
    fx.repo.create_branch(&fx.tree, "dev", None).unwrap();
    fx.repo.create_tag(&fx.tree, "t", None, None).unwrap();
    let log = fx.repo.reflog(&fx.tree, None).unwrap();
    let actions: Vec<_> = log.iter().map(|e| e.action.as_str()).collect();
    assert_eq!(actions, ["tag.create", "branch.create", "snapshot", "init"]);
    assert_eq!(log[2].new.as_deref(), Some(s1.id.as_str()));
    assert_eq!(log[2].message, "one");
    assert_eq!(fx.repo.reflog(&fx.tree, Some(2)).unwrap().len(), 2);

    // Torn trailing lines are skipped rather than failing the whole read.
    let path = fx
        .repo
        .wt_dir()
        .join("trees")
        .join(&fx.tree)
        .join("reflog.jsonl");
    let mut text = std::fs::read_to_string(&path).unwrap();
    text.push_str("{\"broken\n");
    std::fs::write(&path, text).unwrap();
    assert_eq!(fx.repo.reflog(&fx.tree, None).unwrap().len(), 4);
    assert!(fx.repo.reflog("missing-tree", None).is_err());
}

#[test]
fn manifest_plumbing_roundtrip() {
    let fx = Fixture::new();
    fx.write("a.txt", "alpha");
    fx.write("dir/b.txt", "beta");
    let manifest = fx.repo.manifest_from_working(&fx.tree).unwrap();
    assert_eq!(manifest.entries.len(), 2);
    for e in &manifest.entries {
        assert!(fx.repo.store().has(&e.hash), "blob for {} stored", e.path);
    }
    let hash = fx.repo.write_manifest(&manifest).unwrap();
    assert_eq!(hash, manifest.hash());
    assert_eq!(fx.repo.read_manifest(&hash).unwrap(), manifest);
    assert!(fx.repo.store().has(&hash));

    // A hand-built snapshot + set_branch_tip behaves like a real snapshot.
    let snap = Snapshot {
        id: uuid::Uuid::new_v4().to_string(),
        tree_id: fx.repo.root_tree().unwrap().id,
        branch: "main".into(),
        parents: vec![],
        manifest: hash,
        author: "import".into(),
        message: "imported".into(),
        created_at: Utc::now(),
        auto_generated: false,
        dependencies: Vec::new(),
        reverts: None,
        signature: None,
    };
    assert!(matches!(
        fx.repo.set_branch_tip(&fx.tree, "main", Some(&snap.id)),
        Err(SdkError::SnapshotNotFound(_))
    ));
    fx.repo.write_snapshot(&snap).unwrap();
    fx.repo
        .set_branch_tip(&fx.tree, "main", Some(&snap.id))
        .unwrap();
    assert!(fx.repo.status(&fx.tree).unwrap().is_clean());
    let back = Snapshot::from_record(&snap.to_record()).unwrap();
    assert_eq!(back, snap);
}

#[test]
fn status_reports_tree_and_branch() {
    let fx = Fixture::new();
    fx.write("a", "1");
    let report = fx.repo.status(&fx.tree).unwrap();
    assert_eq!(report.tree, "demo");
    assert_eq!(
        report.changes,
        vec![FileChange {
            path: "a".into(),
            kind: ChangeKind::Added
        }]
    );
    assert!(report.merge.is_none());
    assert!(matches!(
        fx.repo.status("nope"),
        Err(SdkError::TreeNotFound(_))
    ));
}

#[cfg(unix)]
#[test]
fn executable_bit_is_tracked() {
    use std::os::unix::fs::PermissionsExt;
    let fx = Fixture::new();
    fx.write("run.sh", "#!/bin/sh\n");
    let p = fx.path("run.sh");
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    let s1 = fx.snap("exec");
    assert!(
        fx.repo
            .manifest_of(&s1)
            .unwrap()
            .get("run.sh")
            .unwrap()
            .executable
    );
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(changes(&fx), [("run.sh".to_string(), ChangeKind::Modified)]);
    fx.snap("not exec");
    fx.repo.checkout(&fx.tree, &s1.id, true).unwrap();
    let mode = std::fs::metadata(&p).unwrap().permissions().mode();
    assert_ne!(mode & 0o111, 0);
}

#[test]
fn secret_scan_blocks_snapshot() {
    let fx = Fixture::new();
    fx.write("config.env", "AWS_ACCESS_KEY_ID=AKIAZ4Q7RJ2KXW3MPLQ9\n");
    let err = fx
        .repo
        .snapshot(
            &fx.tree,
            SnapshotOptions {
                message: "leak".into(),
                ..SnapshotOptions::default()
            },
        )
        .unwrap_err();
    assert!(
        matches!(err, SdkError::SecretsDetected(ref m) if m.contains("config.env")),
        "{err}"
    );
    assert_eq!(fx.repo.branch_tip(&fx.tree, "main").unwrap(), None);
    // Skipping the scan lets it through.
    fx.snap("forced");
}

#[test]
fn secret_scan_passes_clean_files() {
    let fx = Fixture::new();
    fx.write("main.rs", "fn main() { println!(\"hi\"); }\n");
    fx.repo
        .snapshot(
            &fx.tree,
            SnapshotOptions {
                message: "clean".into(),
                ..SnapshotOptions::default()
            },
        )
        .unwrap();
}
