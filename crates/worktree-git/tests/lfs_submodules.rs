//! Git LFS and submodule fidelity of `wt git import` / `wt git export`,
//! over fixture repositories built with the `git` CLI.

use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

use sha2::{Digest, Sha256};
use tempfile::TempDir;
use worktree_git::ident::snapshot_id_for_commit;
use worktree_git::submodule::{decode_pins, PINS_FILE};
use worktree_git::{
    export_repository, import_into_tree, import_repository, ExportOptions, GitMap, ImportOptions,
    LfsMode, SubmoduleMode,
};
use worktree_sdk::Repository;

fn isolate_wt_home() {
    static HOME: OnceLock<TempDir> = OnceLock::new();
    let home = HOME.get_or_init(|| tempfile::tempdir().expect("WT_HOME tempdir"));
    std::env::set_var("WT_HOME", home.path());
}

/// Run `git` in `dir` with a fixed identity, UTC dates and no user config.
fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args([
            "-c",
            "protocol.file.allow=always",
            "-c",
            "core.autocrlf=false",
        ])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Ada Lovelace")
        .env("GIT_AUTHOR_EMAIL", "ada@example.com")
        .env("GIT_COMMITTER_NAME", "Ada Lovelace")
        .env("GIT_COMMITTER_EMAIL", "ada@example.com")
        .env("GIT_AUTHOR_DATE", "1600000000 +0000")
        .env("GIT_COMMITTER_DATE", "1600000000 +0000")
        .output()
        .expect("run git");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

fn init_repo(dir: &Path) {
    fs::create_dir_all(dir).unwrap();
    git(dir, &["init", "-q", "-b", "main"]);
}

fn commit_all(dir: &Path, message: &str) -> String {
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", message]);
    git(dir, &["rev-parse", "HEAD"])
}

fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn pointer(data: &[u8]) -> String {
    format!(
        "version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize {}\n",
        sha256_hex(data),
        data.len()
    )
}

/// Put `data` into the repository's local LFS store.
fn store_lfs_object(repo: &Path, data: &[u8]) {
    let oid = sha256_hex(data);
    let dir = repo
        .join(".git/lfs/objects")
        .join(&oid[0..2])
        .join(&oid[2..4]);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(oid), data).unwrap();
}

const BIG: &[u8] = b"\x00\x01binary asset content that lives in Git LFS\xff\n";
const BIG2: &[u8] = b"second version of the asset\n";

/// A Git repository where `assets/logo.bin` is an LFS file (committed as a
/// pointer, as `git lfs` does). Returns (dir, first commit).
fn lfs_repo(root: &Path) -> (std::path::PathBuf, String) {
    let dir = root.join("src");
    init_repo(&dir);
    fs::write(
        dir.join(".gitattributes"),
        "*.bin filter=lfs diff=lfs merge=lfs -text\n",
    )
    .unwrap();
    fs::create_dir_all(dir.join("assets")).unwrap();
    fs::write(dir.join("assets/logo.bin"), pointer(BIG)).unwrap();
    fs::write(dir.join("README.md"), "# LFS\n").unwrap();
    let c1 = commit_all(&dir, "Add logo");
    (dir, c1)
}

fn opts(lfs: LfsMode) -> ImportOptions {
    ImportOptions {
        include_tags: true,
        lfs,
        ..ImportOptions::default()
    }
}

fn manifest_hash(repo: &Repository, commit: &str) -> String {
    let oid = git2::Oid::from_str(commit).unwrap();
    repo.get_snapshot(&snapshot_id_for_commit(oid))
        .unwrap()
        .manifest
        .to_hex()
}

fn file_at(repo: &Repository, commit: &str, path: &str) -> Vec<u8> {
    let oid = git2::Oid::from_str(commit).unwrap();
    let snap = repo.get_snapshot(&snapshot_id_for_commit(oid)).unwrap();
    repo.read_file_at(&snap, path).unwrap()
}

#[test]
fn lfs_content_is_imported_and_exported_as_plain_blobs() {
    isolate_wt_home();
    let tmp = tempfile::tempdir().unwrap();
    let (src, c1) = lfs_repo(tmp.path());
    store_lfs_object(&src, BIG);

    let dest = tmp.path().join("wt");
    let report = import_repository(&src, &dest, &opts(LfsMode::Auto)).unwrap();
    assert_eq!(report.lfs.resolved, 1);
    assert!(report.lfs.missing.is_empty());
    assert_eq!(fs::read(dest.join("assets/logo.bin")).unwrap(), BIG);
    let repo = Repository::open(&dest).unwrap();
    assert!(repo.status(&report.tree).unwrap().is_clean());
    let entry = repo
        .manifest_of(
            &repo
                .get_snapshot(&snapshot_id_for_commit(git2::Oid::from_str(&c1).unwrap()))
                .unwrap(),
        )
        .unwrap()
        .get("assets/logo.bin")
        .cloned()
        .unwrap();
    assert_eq!(entry.size, BIG.len() as u64);

    // Export writes the content as an ordinary blob (no pointer).
    let out = tmp.path().join("out.git");
    export_repository(
        &repo,
        &out,
        &ExportOptions {
            bare: true,
            ..ExportOptions::default()
        },
    )
    .unwrap();
    let out = git2::Repository::open(&out).unwrap();
    let tip = out.refname_to_id("refs/heads/main").unwrap();
    let tree = out.find_commit(tip).unwrap().tree().unwrap();
    let blob = tree.get_path(Path::new("assets/logo.bin")).unwrap();
    assert_eq!(out.find_blob(blob.id()).unwrap().content(), BIG);
    assert_ne!(
        tip.to_string(),
        c1,
        "content instead of pointer changes the SHA"
    );
}

#[test]
fn missing_lfs_content_in_each_mode() {
    isolate_wt_home();
    let tmp = tempfile::tempdir().unwrap();
    let (src, _) = lfs_repo(tmp.path());

    // auto: the pointer is imported and reported.
    let dest = tmp.path().join("auto");
    let report = import_repository(&src, &dest, &opts(LfsMode::Auto)).unwrap();
    assert_eq!(report.lfs.resolved, 0);
    assert_eq!(report.lfs.missing.len(), 1);
    let missing = &report.lfs.missing[0];
    assert_eq!(missing.path, "assets/logo.bin");
    assert_eq!(missing.oid, sha256_hex(BIG));
    assert_eq!(missing.size, BIG.len() as u64);
    assert_eq!(
        fs::read_to_string(dest.join("assets/logo.bin")).unwrap(),
        pointer(BIG)
    );

    // require: the import fails and leaves no worktree behind.
    let dest = tmp.path().join("require");
    let err = import_repository(&src, &dest, &opts(LfsMode::Require)).unwrap_err();
    assert!(err.to_string().contains("assets/logo.bin"), "{err}");
    assert!(!dest.exists());

    // skip: pointers are kept even when the content is available.
    store_lfs_object(&src, BIG);
    let dest = tmp.path().join("skip");
    let report = import_repository(&src, &dest, &opts(LfsMode::Skip)).unwrap();
    assert_eq!(report.lfs.skipped, 1);
    assert_eq!(report.lfs.resolved, 0);
    assert!(report.lfs.missing.is_empty());
    assert_eq!(
        fs::read_to_string(dest.join("assets/logo.bin")).unwrap(),
        pointer(BIG)
    );
}

#[test]
fn reimport_keeps_snapshot_ids_whether_or_not_content_was_available() {
    isolate_wt_home();
    let tmp = tempfile::tempdir().unwrap();
    let (src, c1) = lfs_repo(tmp.path());

    // A: imported while the content is missing (pointer kept).
    let a = tmp.path().join("a");
    import_repository(&src, &a, &opts(LfsMode::Auto)).unwrap();
    // B and C: imported with the content available.
    store_lfs_object(&src, BIG);
    let b = tmp.path().join("b");
    let c = tmp.path().join("c");
    import_repository(&src, &b, &opts(LfsMode::Auto)).unwrap();
    import_repository(&src, &c, &opts(LfsMode::Auto)).unwrap();
    let (ra, rb, rc) = (
        Repository::open(&a).unwrap(),
        Repository::open(&b).unwrap(),
        Repository::open(&c).unwrap(),
    );

    // Same snapshot ids and mapping everywhere.
    let map = |r: &Repository| {
        GitMap::load(&r.wt_dir())
            .unwrap()
            .iter()
            .map(|(s, c)| (s.to_owned(), c.to_owned()))
            .collect::<Vec<_>>()
    };
    assert_eq!(map(&ra), map(&rb));
    // Identical content gives identical snapshots; the pointer import differs.
    assert_eq!(manifest_hash(&rb, &c1), manifest_hash(&rc, &c1));
    assert_ne!(manifest_hash(&ra, &c1), manifest_hash(&rb, &c1));

    // Incremental re-import into A: the existing snapshot is immutable (it
    // keeps the pointer); the new commit gets content.
    fs::write(src.join("assets/logo.bin"), pointer(BIG2)).unwrap();
    store_lfs_object(&src, BIG2);
    let c2 = commit_all(&src, "Update logo");
    let tree = ra.root_tree().unwrap();
    let report = import_into_tree(&ra, &tree.name, &src, &opts(LfsMode::Auto)).unwrap();
    assert_eq!(report.snapshots, 2);
    assert_eq!(report.lfs.resolved, 1);
    assert_eq!(
        file_at(&ra, &c1, "assets/logo.bin"),
        pointer(BIG).as_bytes()
    );
    assert_eq!(file_at(&ra, &c2, "assets/logo.bin"), BIG2);
    assert_eq!(fs::read(a.join("assets/logo.bin")).unwrap(), BIG2);
    let map_a = GitMap::load(&ra.wt_dir()).unwrap();
    assert_eq!(
        map_a.commit_for(&snapshot_id_for_commit(git2::Oid::from_str(&c1).unwrap())),
        Some(c1.as_str())
    );
}

/// A library repository and a superproject that pins it at `vendor/lib`
/// via `git submodule add`. Returns (superproject dir, lib commit, super commit).
fn submodule_repos(root: &Path) -> (std::path::PathBuf, String, String) {
    let lib = root.join("lib");
    init_repo(&lib);
    fs::write(lib.join("lib.rs"), "pub fn lib() {}\n").unwrap();
    let lib_commit = commit_all(&lib, "Lib");

    let sup = root.join("super");
    init_repo(&sup);
    fs::write(sup.join("main.rs"), "fn main() {}\n").unwrap();
    commit_all(&sup, "Initial");
    let url = lib.to_string_lossy().into_owned();
    git(&sup, &["submodule", "add", "-q", &url, "vendor/lib"]);
    let sup_commit = commit_all(&sup, "Add lib submodule");
    (sup, lib_commit, sup_commit)
}

#[test]
fn submodule_pins_are_recorded_and_exported_as_gitlinks() {
    isolate_wt_home();
    let tmp = tempfile::tempdir().unwrap();
    let (sup, lib_commit, sup_commit) = submodule_repos(tmp.path());

    let dest = tmp.path().join("wt");
    let report = import_repository(&sup, &dest, &ImportOptions::default()).unwrap();
    assert_eq!(report.submodules.len(), 1);
    let sub = &report.submodules[0];
    assert_eq!(sub.path, "vendor/lib");
    assert_eq!(sub.commit, lib_commit);
    assert!(sub.nested_tree.is_none());
    assert!(sub.note.as_deref().unwrap().contains("not imported"));

    let pins = decode_pins(&fs::read(dest.join(PINS_FILE)).unwrap()).unwrap();
    assert_eq!(pins.len(), 1);
    assert_eq!(pins[0].path, "vendor/lib");
    assert_eq!(pins[0].commit, lib_commit);
    assert_eq!(pins[0].name.as_deref(), Some("vendor/lib"));
    assert!(pins[0].url.as_deref().unwrap().ends_with("lib"));
    assert!(dest.join(".gitmodules").is_file());
    assert!(!dest.join("vendor/lib").exists());

    // Exporting into a fresh repository reproduces the original commits
    // byte for byte (same tree with gitlink, same SHA).
    let repo = Repository::open(&dest).unwrap();
    let out = tmp.path().join("out.git");
    let exported = export_repository(
        &repo,
        &out,
        &ExportOptions {
            bare: true,
            ..ExportOptions::default()
        },
    )
    .unwrap();
    assert_eq!(exported.gitlinks, 1);
    assert!(exported.warnings.is_empty());
    let out = git2::Repository::open(&out).unwrap();
    let tip = out.refname_to_id("refs/heads/main").unwrap();
    assert_eq!(tip.to_string(), sup_commit);
    let tree = out.find_commit(tip).unwrap().tree().unwrap();
    let link = tree.get_path(Path::new("vendor/lib")).unwrap();
    assert_eq!(link.filemode(), i32::from(git2::FileMode::Commit));
    assert_eq!(link.id().to_string(), lib_commit);
    assert!(tree.get_path(Path::new(PINS_FILE)).is_err());
}

#[test]
fn nested_mode_imports_local_submodules_as_nested_trees() {
    isolate_wt_home();
    let tmp = tempfile::tempdir().unwrap();
    let (sup, lib_commit, _) = submodule_repos(tmp.path());
    // The superproject's own clone of the submodule (.git/modules/vendor/lib)
    // has the pinned commit checked out (detached), on branch main.
    assert!(sup.join(".git/modules/vendor/lib").is_dir());

    let dest = tmp.path().join("wt");
    let report = import_repository(
        &sup,
        &dest,
        &ImportOptions {
            submodules: SubmoduleMode::Nested,
            ..ImportOptions::default()
        },
    )
    .unwrap();
    let sub = &report.submodules[0];
    assert_eq!(sub.nested_tree.as_deref(), Some("vendor-lib"));
    assert!(sub.at_pin, "{sub:?}");
    assert_eq!(sub.note, None);

    let repo = Repository::open(&dest).unwrap();
    assert_eq!(
        fs::read_to_string(dest.join("vendor/lib/lib.rs")).unwrap(),
        "pub fn lib() {}\n"
    );
    let pinned = snapshot_id_for_commit(git2::Oid::from_str(&lib_commit).unwrap());
    let current = repo.current_branch("vendor-lib").unwrap();
    assert_eq!(
        repo.branch_tip("vendor-lib", &current).unwrap(),
        Some(pinned)
    );
    let root = repo.root_tree().unwrap();
    assert!(repo.status(&root.name).unwrap().is_clean());
    assert!(repo.status("vendor-lib").unwrap().is_clean());

    // A submodule without a local repository is reported, not fatal.
    let bare = tmp.path().join("bare.git");
    git(
        tmp.path(),
        &["clone", "-q", "--bare", &sup.to_string_lossy(), "bare.git"],
    );
    let report = import_repository(
        &bare,
        &tmp.path().join("wt2"),
        &ImportOptions {
            submodules: SubmoduleMode::Nested,
            ..ImportOptions::default()
        },
    )
    .unwrap();
    assert!(report.submodules[0].nested_tree.is_none());
    assert!(report.submodules[0]
        .note
        .as_deref()
        .unwrap()
        .contains("not available locally"));
}
