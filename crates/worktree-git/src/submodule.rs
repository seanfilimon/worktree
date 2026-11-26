//! Git submodules (gitlinks) in `wt git import` / `wt git export`.
//!
//! W0rkTree has no gitlink entry, so import records every gitlink of a
//! commit in a tracked file at the root of the imported tree,
//! [`PINS_FILE`] (TOML):
//!
//! ```toml
//! # Git submodule pins recorded by `wt git import` (one per gitlink).
//! # `wt git export` writes each entry back as a gitlink; edit `commit` to move a pin.
//! version = 1
//!
//! [[submodule]]
//! path = "vendor/lib"
//! commit = "0123456789abcdef0123456789abcdef01234567"
//! name = "lib"
//! url = "https://example.com/lib.git"
//! ```
//!
//! `path` and `commit` are the gitlink; `name` and `url` come from the
//! commit's `.gitmodules` (which is imported as an ordinary file as well)
//! and are informational. Export removes the file from the Git tree and
//! writes a gitlink per entry, so a commit with submodules round-trips to
//! the same Git tree.
//!
//! With [`SubmoduleMode::Nested`], a new-worktree import also imports each
//! submodule whose repository is available locally (`.git/modules/<name>`
//! or an initialised checkout) into a nested tree at its path.

use std::collections::HashSet;
use std::path::PathBuf;

use git2::{FileMode, Oid, TreeWalkMode, TreeWalkResult};
use serde::{Deserialize, Serialize};
use tracing::warn;
use worktree_sdk::{Repository, SwitchOptions, TreeInfo};

use crate::bridge::ImportOptions;
use crate::error::{GitCompatError, Result};
use crate::ident;

/// Tracked file (at the root of a tree) holding the submodule pins.
pub const PINS_FILE: &str = ".wtsubmodules";

const PINS_VERSION: u32 = 1;

const PINS_HEADER: &str = "\
# Git submodule pins recorded by `wt git import` (one per gitlink).
# `wt git export` writes each entry back as a gitlink; edit `commit` to move a pin.
";

/// How import maps Git submodules.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SubmoduleMode {
    /// Record each gitlink in [`PINS_FILE`]; submodule content is not
    /// imported.
    #[default]
    Record,
    /// Record pins and, for a new worktree, import every submodule whose
    /// repository is available locally into a nested tree at its path.
    Nested,
}

/// One gitlink: a submodule pinned at a commit.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SubmodulePin {
    /// `/`-separated path relative to the tree root.
    pub path: String,
    /// Pinned commit (hex SHA).
    pub commit: String,
    /// Submodule name from `.gitmodules`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Submodule URL from `.gitmodules`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct PinsFile {
    version: u32,
    #[serde(default, rename = "submodule")]
    submodules: Vec<SubmodulePin>,
}

/// What an import did with one submodule of the checked-out commit.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct SubmoduleReport {
    pub path: String,
    pub commit: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Nested tree the submodule's history was imported into.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nested_tree: Option<String>,
    /// Whether the nested tree's checked-out branch tip is the pinned
    /// commit.
    pub at_pin: bool,
    /// What was not imported, if anything.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// A `[submodule "name"]` section of `.gitmodules`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitModule {
    pub name: String,
    pub path: Option<String>,
    pub url: Option<String>,
}

/// Parse the `[submodule "<name>"]` sections of a `.gitmodules` file
/// (git-config syntax: case-insensitive section and key names, `#`/`;`
/// comments, quoted values with `\"`, `\\`, `\t`, `\n` escapes). Other
/// sections and keys are ignored; later values of a key win.
pub fn parse_gitmodules(text: &str) -> Vec<GitModule> {
    let mut modules: Vec<GitModule> = Vec::new();
    let mut current: Option<usize> = None;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(rest) = line.strip_prefix('[') {
            current = None;
            let Some(header) = rest.split(']').next() else {
                continue;
            };
            let header = header.trim();
            let Some((section, sub)) = header.split_once(char::is_whitespace) else {
                continue;
            };
            if !section.eq_ignore_ascii_case("submodule") {
                continue;
            }
            let sub = sub.trim();
            let Some(name) = sub.strip_prefix('"').and_then(|s| s.strip_suffix('"')) else {
                continue;
            };
            let name = unescape(name);
            current = Some(match modules.iter().position(|m| m.name == name) {
                Some(i) => i,
                None => {
                    modules.push(GitModule {
                        name,
                        ..GitModule::default()
                    });
                    modules.len() - 1
                }
            });
            continue;
        }
        let Some(index) = current else {
            continue;
        };
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = config_value(value);
        match key.trim().to_ascii_lowercase().as_str() {
            "path" => modules[index].path = Some(value),
            "url" => modules[index].url = Some(value),
            _ => {}
        }
    }
    modules
}

/// A git-config value: comments outside quotes removed, quotes and
/// escapes resolved, surrounding whitespace trimmed.
fn config_value(raw: &str) -> String {
    let mut out = String::new();
    let mut quoted = false;
    let mut chars = raw.trim().chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => quoted = !quoted,
            '\\' => match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some(other) => out.push(other),
                None => {}
            },
            '#' | ';' if !quoted => break,
            _ => out.push(c),
        }
    }
    out.trim().to_owned()
}

fn unescape(raw: &str) -> String {
    let mut out = String::new();
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(next) = chars.next() {
                out.push(next);
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Pins for `gitlinks` (path, commit), with name and URL from the
/// `.gitmodules` text when it declares the path. Sorted by path.
pub fn pins_for(gitlinks: &[(String, Oid)], gitmodules: Option<&str>) -> Vec<SubmodulePin> {
    let modules = gitmodules.map(parse_gitmodules).unwrap_or_default();
    let mut pins: Vec<SubmodulePin> = gitlinks
        .iter()
        .map(|(path, commit)| {
            let module = modules
                .iter()
                .rev()
                .find(|m| m.path.as_deref() == Some(path.as_str()));
            SubmodulePin {
                path: path.clone(),
                commit: commit.to_string(),
                name: module.map(|m| m.name.clone()),
                url: module.and_then(|m| m.url.clone()),
            }
        })
        .collect();
    pins.sort();
    pins
}

/// Serialize pins as the content of [`PINS_FILE`] (deterministic).
pub fn encode_pins(pins: &[SubmodulePin]) -> Result<Vec<u8>> {
    let mut sorted = pins.to_vec();
    sorted.sort();
    let body = toml::to_string(&PinsFile {
        version: PINS_VERSION,
        submodules: sorted,
    })
    .map_err(|e| GitCompatError::ConfigError(format!("{PINS_FILE}: {e}")))?;
    Ok(format!("{PINS_HEADER}{body}").into_bytes())
}

/// Parse the content of [`PINS_FILE`]. Fails on malformed TOML, an
/// unknown version, invalid paths or commit ids.
pub fn decode_pins(data: &[u8]) -> Result<Vec<SubmodulePin>> {
    let bad = |why: String| GitCompatError::ConfigError(format!("{PINS_FILE}: {why}"));
    let text = std::str::from_utf8(data).map_err(|e| bad(e.to_string()))?;
    let file: PinsFile = toml::from_str(text).map_err(|e| bad(e.message().to_owned()))?;
    if file.version != PINS_VERSION {
        return Err(bad(format!(
            "unsupported version {} (expected {PINS_VERSION})",
            file.version
        )));
    }
    let mut seen = HashSet::new();
    for pin in &file.submodules {
        let valid_path = !pin.path.is_empty()
            && pin
                .path
                .split('/')
                .all(|c| !c.is_empty() && c != "." && c != ".." && !c.eq_ignore_ascii_case(".git"));
        if !valid_path {
            return Err(bad(format!("invalid submodule path {:?}", pin.path)));
        }
        if !seen.insert(pin.path.as_str()) {
            return Err(bad(format!(
                "submodule path {:?} is listed twice",
                pin.path
            )));
        }
        if pin.commit.len() != 40 || Oid::from_str(&pin.commit).is_err() {
            return Err(bad(format!(
                "invalid commit {:?} for {}",
                pin.commit, pin.path
            )));
        }
    }
    Ok(file.submodules)
}

/// Pins of the Git tree `tree` (every gitlink at any depth, with
/// `.gitmodules` details).
pub(crate) fn pins_of_tree(
    git: &git2::Repository,
    tree: &git2::Tree<'_>,
) -> Result<Vec<SubmodulePin>> {
    let mut gitlinks = Vec::new();
    tree.walk(TreeWalkMode::PreOrder, |dir, entry| {
        if entry.filemode() == i32::from(FileMode::Commit) {
            if let Some(name) = entry.name() {
                gitlinks.push((format!("{dir}{name}"), entry.id()));
            }
        }
        TreeWalkResult::Ok
    })?;
    let gitmodules = match tree.get_name(".gitmodules") {
        Some(entry) if entry.kind() == Some(git2::ObjectType::Blob) => {
            Some(String::from_utf8_lossy(git.find_blob(entry.id())?.content()).into_owned())
        }
        _ => None,
    };
    Ok(pins_for(&gitlinks, gitmodules.as_deref()))
}

/// Report entries for `pins` when nothing beyond the pin is imported.
pub(crate) fn recorded(pins: &[SubmodulePin]) -> Vec<SubmoduleReport> {
    pins.iter()
        .map(|pin| SubmoduleReport {
            path: pin.path.clone(),
            commit: pin.commit.clone(),
            url: pin.url.clone(),
            nested_tree: None,
            at_pin: false,
            note: Some(format!(
                "pin recorded in {PINS_FILE}; submodule content not imported"
            )),
        })
        .collect()
}

/// Local repository of a submodule: `<common git dir>/modules/<name>`,
/// else an initialised checkout at `<work dir>/<path>`.
fn local_repository(git: &git2::Repository, pin: &SubmodulePin) -> Option<PathBuf> {
    let name = pin.name.as_deref().unwrap_or(&pin.path);
    let mut candidates = vec![git.path().join("modules").join(name)];
    if let Some(workdir) = git.workdir() {
        candidates.push(workdir.join(&pin.path));
    }
    let own = std::fs::canonicalize(git.path()).ok();
    candidates.into_iter().find(|candidate| {
        let Ok(sub) = git2::Repository::open(candidate) else {
            return false;
        };
        // An uninitialised submodule directory opens the superproject.
        std::fs::canonicalize(sub.path()).ok() != own
    })
}

/// A tree name for `path` that is valid and not taken: the path with
/// unsupported characters replaced, suffixed `-2`, `-3`... if needed.
fn tree_name_for(repo: &Repository, path: &str) -> Result<String> {
    let taken: HashSet<String> = repo.trees()?.into_iter().map(|t| t.name).collect();
    let mut base: String = path
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '-'
            } else {
                c
            }
        })
        .collect();
    base = base
        .trim_matches(|c| c == '.' || c == ' ' || c == '-')
        .to_owned();
    base.truncate(90);
    if base.is_empty() {
        base = "submodule".to_owned();
    }
    let mut name = base.clone();
    let mut n = 2;
    while taken.contains(&name) {
        name = format!("{base}-{n}");
        n += 1;
    }
    Ok(name)
}

/// Import each pinned submodule whose repository is available locally into
/// a nested tree at its path below `parent` ([`SubmoduleMode::Nested`]).
/// Problems with one submodule are reported, not fatal.
pub(crate) fn import_nested(
    git: &git2::Repository,
    repo: &Repository,
    parent: &TreeInfo,
    pins: &[SubmodulePin],
    opts: &ImportOptions,
) -> Vec<SubmoduleReport> {
    let mut reports = recorded(pins);
    for (pin, report) in pins.iter().zip(reports.iter_mut()) {
        match import_one(git, repo, parent, pin, opts) {
            Ok((tree, at_pin, note)) => {
                report.nested_tree = Some(tree);
                report.at_pin = at_pin;
                report.note = note;
            }
            Err(why) => {
                warn!(path = %pin.path, "submodule not imported as a nested tree: {why}");
                report.note = Some(format!(
                    "pin recorded in {PINS_FILE}; not imported as a nested tree: {why}"
                ));
            }
        }
    }
    reports
}

fn import_one(
    git: &git2::Repository,
    repo: &Repository,
    parent: &TreeInfo,
    pin: &SubmodulePin,
    opts: &ImportOptions,
) -> std::result::Result<(String, bool, Option<String>), String> {
    let source = local_repository(git, pin)
        .ok_or_else(|| "its repository is not available locally".to_owned())?;
    let rel = if parent.path.as_os_str().is_empty() {
        PathBuf::from(&pin.path)
    } else {
        parent.path.join(&pin.path)
    };
    let name = tree_name_for(repo, &pin.path).map_err(|e| e.to_string())?;
    repo.add_tree(&name, &rel).map_err(|e| e.to_string())?;
    let sub_opts = ImportOptions {
        branches: Vec::new(),
        all_branches: true,
        name: None,
        include_tags: opts.include_tags,
        lfs: opts.lfs,
        lfs_fetch: opts.lfs_fetch,
        // Submodules of submodules are recorded as pins in the nested tree.
        submodules: SubmoduleMode::Record,
    };
    if let Err(e) = crate::import::import_into_tree(repo, &name, &source, &sub_opts) {
        // Unregister the half-imported tree (its files are kept).
        let _ = repo.remove_tree(&name);
        return Err(e.to_string());
    }
    let pinned = Oid::from_str(&pin.commit)
        .map(ident::snapshot_id_for_commit)
        .map_err(|e| e.to_string())?;
    if !repo.has_snapshot(&pinned) {
        return Ok((
            name,
            false,
            Some(format!(
                "pinned commit {} is not on any branch of the submodule repository",
                pin.commit
            )),
        ));
    }
    let current = repo.current_branch(&name).map_err(|e| e.to_string())?;
    if repo.branch_tip(&name, &current).ok().flatten().as_deref() == Some(pinned.as_str()) {
        return Ok((name, true, None));
    }
    // Switch to a branch whose tip is the pin, if there is one.
    let at_pin = repo
        .branches(&name, true)
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|b| b.tip.as_deref() == Some(pinned.as_str()));
    if let Some(branch) = at_pin {
        let switch = SwitchOptions {
            create: false,
            force: false,
        };
        if repo.switch_branch(&name, &branch.name, switch).is_ok() {
            return Ok((name, true, None));
        }
    }
    Ok((
        name,
        false,
        Some(format!(
            "the nested tree is on branch {current}, not at the pinned commit {}",
            pin.commit
        )),
    ))
}

/// Split [`PINS_FILE`] out of `manifest` for export: the manifest without
/// it and the gitlinks it lists. A file that cannot be parsed stays in the
/// manifest (exported as an ordinary file) and is reported in `warnings`.
pub(crate) fn split_pins(
    manifest: worktree_sdk::Manifest,
    read: impl FnOnce(&worktree_sdk::ManifestEntry) -> Result<Vec<u8>>,
    warnings: &mut Vec<String>,
) -> Result<(worktree_sdk::Manifest, Vec<(String, Oid)>)> {
    let Some(index) = manifest.entries.iter().position(|e| e.path == PINS_FILE) else {
        return Ok((manifest, Vec::new()));
    };
    let data = read(&manifest.entries[index])?;
    match decode_pins(&data) {
        Ok(pins) => {
            let mut manifest = manifest;
            manifest.entries.remove(index);
            let mut links = Vec::with_capacity(pins.len());
            for pin in pins {
                let covered = manifest.entries.iter().any(|e| {
                    e.path == pin.path
                        || e.path.starts_with(&format!("{}/", pin.path))
                        || pin.path.starts_with(&format!("{}/", e.path))
                });
                if covered {
                    warnings.push(format!(
                        "submodule {} not exported as a gitlink: files exist at that path",
                        pin.path
                    ));
                    continue;
                }
                if let Ok(oid) = Oid::from_str(&pin.commit) {
                    links.push((pin.path, oid));
                }
            }
            Ok((manifest, links))
        }
        Err(e) => {
            warnings.push(format!("{e}; exported as an ordinary file"));
            Ok((manifest, Vec::new()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const C1: &str = "1111111111111111111111111111111111111111";
    const C2: &str = "2222222222222222222222222222222222222222";

    #[test]
    fn parses_gitmodules() {
        let text = r#"
# comment
[core]
	path = not-a-submodule
[submodule "lib"]
	path = vendor/lib
	url = https://example.com/lib.git ; trailing comment
[Submodule "odd \"name\""]
	PATH = "with space/dir"
	url = "git@example.com:x#y.git"
[submodule "lib"]
	branch = main
	url = https://example.com/lib2.git
"#;
        let modules = parse_gitmodules(text);
        assert_eq!(
            modules,
            vec![
                GitModule {
                    name: "lib".into(),
                    path: Some("vendor/lib".into()),
                    url: Some("https://example.com/lib2.git".into()),
                },
                GitModule {
                    name: "odd \"name\"".into(),
                    path: Some("with space/dir".into()),
                    url: Some("git@example.com:x#y.git".into()),
                },
            ]
        );
    }

    #[test]
    fn pins_round_trip_deterministically() {
        let gitlinks = vec![
            ("z/last".to_owned(), Oid::from_str(C2).unwrap()),
            ("vendor/lib".to_owned(), Oid::from_str(C1).unwrap()),
        ];
        let gitmodules = "[submodule \"lib\"]\n\tpath = vendor/lib\n\turl = ../lib.git\n";
        let pins = pins_for(&gitlinks, Some(gitmodules));
        assert_eq!(pins[0].path, "vendor/lib");
        assert_eq!(pins[0].name.as_deref(), Some("lib"));
        assert_eq!(pins[0].url.as_deref(), Some("../lib.git"));
        assert_eq!(pins[1].path, "z/last");
        assert_eq!(pins[1].name, None);

        let data = encode_pins(&pins).unwrap();
        let text = String::from_utf8(data.clone()).unwrap();
        assert!(text.starts_with("# Git submodule pins"), "{text}");
        assert!(text.contains("[[submodule]]"), "{text}");
        assert_eq!(decode_pins(&data).unwrap(), pins);
        // Input order does not matter.
        let reversed: Vec<_> = pins.iter().rev().cloned().collect();
        assert_eq!(encode_pins(&reversed).unwrap(), data);
    }

    mod conversion {
        use super::*;
        use crate::export::{BlobSource, TreeWriter};
        use crate::import::TreeConverter;
        use crate::test_support::{GitFixture, MemStore, Spec};

        #[test]
        fn gitlinks_become_a_pins_file_and_round_trip_to_the_same_tree() {
            let fx = GitFixture::new();
            let lib = fx.commit(None, &[Spec::File("lib.rs", b"x")], &[], "lib", 1);
            let tool = fx.commit(None, &[Spec::File("t", b"t")], &[], "tool", 2);
            let gitmodules: &[u8] =
                b"[submodule \"lib\"]\n\tpath = vendor/lib\n\turl = https://example.com/lib.git\n";
            let tree_oid = fx.tree(&[
                Spec::File(".gitmodules", gitmodules),
                Spec::File("README.md", b"hi\n"),
                Spec::Gitlink("vendor/lib", lib),
                Spec::Gitlink("tools/t", tool),
            ]);
            let tree = fx.repo.find_tree(tree_oid).unwrap();
            let store = MemStore::default();
            let manifest = TreeConverter::new(&fx.repo, &store)
                .recording_gitlinks()
                .manifest(&tree)
                .unwrap();
            let paths: Vec<_> = manifest.entries.iter().map(|e| e.path.as_str()).collect();
            assert_eq!(paths, vec![".gitmodules", PINS_FILE, "README.md"]);

            let pins =
                decode_pins(&store.read_blob(manifest.get(PINS_FILE).unwrap()).unwrap()).unwrap();
            assert_eq!(
                pins,
                vec![
                    SubmodulePin {
                        path: "tools/t".into(),
                        commit: tool.to_string(),
                        name: None,
                        url: None,
                    },
                    SubmodulePin {
                        path: "vendor/lib".into(),
                        commit: lib.to_string(),
                        name: Some("lib".into()),
                        url: Some("https://example.com/lib.git".into()),
                    },
                ]
            );
            assert_eq!(pins_of_tree(&fx.repo, &tree).unwrap(), pins);

            // Export: the pins file turns back into gitlinks.
            let mut warnings = Vec::new();
            let (files, links) =
                split_pins(manifest, |e| store.read_blob(e), &mut warnings).unwrap();
            assert!(warnings.is_empty());
            assert!(files.get(PINS_FILE).is_none());
            let out = GitFixture::bare();
            let rebuilt = TreeWriter::new(&out.repo, &store)
                .write_with_gitlinks(&files, &links)
                .unwrap();
            assert_eq!(rebuilt, tree_oid);
        }

        #[test]
        fn a_git_pins_file_next_to_gitlinks_is_an_error() {
            let fx = GitFixture::new();
            let lib = fx.commit(None, &[Spec::File("x", b"x")], &[], "lib", 1);
            let tree_oid = fx.tree(&[
                Spec::File(PINS_FILE, b"user content"),
                Spec::Gitlink("lib", lib),
            ]);
            let store = MemStore::default();
            let err = TreeConverter::new(&fx.repo, &store)
                .recording_gitlinks()
                .manifest(&fx.repo.find_tree(tree_oid).unwrap())
                .unwrap_err();
            assert!(err.to_string().contains(PINS_FILE), "{err}");
        }

        #[test]
        fn unusable_pins_are_reported_on_export() {
            use worktree_sdk::{Manifest, ManifestEntry};
            let store = MemStore::default();
            let entry = |path: &str, data: &[u8]| ManifestEntry {
                path: path.into(),
                hash: crate::import::BlobSink::put_blob(&store, data).unwrap(),
                size: data.len() as u64,
                executable: false,
                chunked: false,
            };
            let pins = encode_pins(&[SubmodulePin {
                path: "lib".into(),
                commit: C1.into(),
                name: None,
                url: None,
            }])
            .unwrap();
            // A file inside the pinned path: the gitlink cannot be written.
            let manifest = Manifest {
                entries: vec![entry(PINS_FILE, &pins), entry("lib/file", b"x")],
            };
            let mut warnings = Vec::new();
            let (files, links) =
                split_pins(manifest, |e| store.read_blob(e), &mut warnings).unwrap();
            assert!(links.is_empty());
            assert_eq!(files.entries.len(), 1);
            assert!(
                warnings[0].contains("not exported as a gitlink"),
                "{warnings:?}"
            );

            // A malformed pins file stays an ordinary file.
            let manifest = Manifest {
                entries: vec![entry(PINS_FILE, b"version = [")],
            };
            let mut warnings = Vec::new();
            let (files, links) =
                split_pins(manifest, |e| store.read_blob(e), &mut warnings).unwrap();
            assert!(links.is_empty());
            assert!(files.get(PINS_FILE).is_some());
            assert!(
                warnings[0].contains("exported as an ordinary file"),
                "{warnings:?}"
            );
        }
    }

    #[test]
    fn invalid_pins_are_rejected() {
        for bad in [
            "not toml [",
            "version = 2\n",
            "version = 1\n[[submodule]]\npath = \"../x\"\ncommit = \"1111111111111111111111111111111111111111\"\n",
            "version = 1\n[[submodule]]\npath = \"x\"\ncommit = \"abc\"\n",
            "version = 1\n[[submodule]]\npath = \"x\"\n",
            "version = 1\n[[submodule]]\npath = \"x\"\ncommit = \"1111111111111111111111111111111111111111\"\n[[submodule]]\npath = \"x\"\ncommit = \"2222222222222222222222222222222222222222\"\n",
        ] {
            assert!(decode_pins(bad.as_bytes()).is_err(), "{bad:?}");
        }
        assert_eq!(decode_pins(b"version = 1\n").unwrap(), Vec::new());
    }
}
