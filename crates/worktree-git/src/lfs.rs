//! Git LFS content during `wt git import`.
//!
//! A Git LFS file is stored in Git as a small *pointer file*:
//!
//! ```text
//! version https://git-lfs.github.com/spec/v1
//! oid sha256:4d7a214614ab2935c943f9e0ff69d22eadbb8f32b1258daaa5e2ca24d17e2393
//! size 12345
//! ```
//!
//! W0rkTree needs no LFS (files above 8 MiB are stored as content-defined
//! chunks), so import replaces each pointer with the content it names:
//!
//! 1. A blob is an LFS pointer only if it parses as one ([`LfsPointer::parse`],
//!    following the parser of the `git-lfs` client) **and** its path has
//!    `filter=lfs` in the commit's `.gitattributes` files. Pointer-shaped
//!    files without the attribute are what a Git checkout shows too, so they
//!    are imported as they are (and listed in [`LfsReport::untracked`]).
//! 2. The content comes from the repository's local LFS store
//!    (`<git dir>/lfs/objects/<aa>/<bb>/<oid>`, or `lfs.storage`), then —
//!    only with [`crate::ImportOptions::lfs_fetch`] — from `git lfs fetch
//!    --all` (run once) and `git lfs smudge` (per object), if `git-lfs` is
//!    installed.
//! 3. Content is used only if its SHA-256 and size match the pointer.
//! 4. Content that is unavailable is handled by [`LfsMode`]: `Auto` keeps the
//!    pointer file and reports it ([`LfsReport::missing`]), `Require` fails
//!    the import, `Skip` never looks for content.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use git2::Oid;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tracing::{info, warn};

use crate::config::gitattributes::LfsRules;
use crate::error::{GitCompatError, Result};

/// Version line of current pointer files.
pub const POINTER_VERSION: &str = "https://git-lfs.github.com/spec/v1";

/// Versions accepted by the `git-lfs` client besides [`POINTER_VERSION`].
const LEGACY_VERSIONS: [&str; 2] = [
    "https://hawser.github.com/spec/v1",
    "http://git-media.io/v/2",
];

/// Blobs of this size or larger are never pointers (the client's cutoff).
pub const MAX_POINTER_SIZE: usize = 1024;

/// SHA-256 of the empty string (the oid of an empty LFS file).
const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

/// What import does with Git LFS files.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LfsMode {
    /// Import the content when it is available; otherwise keep the pointer
    /// file and report it.
    #[default]
    Auto,
    /// Like `Auto`, but unavailable content fails the import.
    Require,
    /// Import pointer files as they are (no lookup).
    Skip,
}

/// A parsed Git LFS pointer file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LfsPointer {
    /// Lowercase hex SHA-256 of the content.
    pub oid: String,
    /// Content size in bytes.
    pub size: u64,
    /// The pointer lists LFS extensions (`ext-N-name` lines); its content
    /// must be produced by the extension programs.
    pub extensions: bool,
}

impl LfsPointer {
    /// Parse `data` as a pointer file, or `None` if it is not one.
    ///
    /// Mirrors the `git-lfs` client: surrounding whitespace and blank lines
    /// are ignored, `\r\n` line ends are accepted, the keys must be
    /// `version`, `oid`, `size` in that order (`ext-N-name` lines may appear
    /// before `oid`), the version must be a known spec URL, the oid must be
    /// `sha256:` plus 64 lowercase hex digits and the size a decimal number.
    /// Anything else — extra keys, other hash algorithms, files of
    /// [`MAX_POINTER_SIZE`] bytes or more — is not a pointer.
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() >= MAX_POINTER_SIZE {
            return None;
        }
        let text = std::str::from_utf8(data).ok()?.trim();
        let mut expected = ["version", "oid", "size"].into_iter().peekable();
        let mut values: [&str; 3] = [""; 3];
        let mut index = 0;
        let mut extensions = false;
        for line in text.split('\n') {
            let line = line.strip_suffix('\r').unwrap_or(line);
            if line.is_empty() {
                continue;
            }
            let (key, value) = line.split_once(' ')?;
            let want = expected.peek().copied()?;
            if key == want {
                values[index] = value;
                index += 1;
                expected.next();
            } else if index > 0 && is_extension_key(key) {
                extensions = true;
            } else {
                return None;
            }
        }
        if index != 3 {
            return None;
        }
        let [version, oid, size] = values;
        if version != POINTER_VERSION && !LEGACY_VERSIONS.contains(&version) {
            return None;
        }
        let oid = oid.strip_prefix("sha256:")?;
        if oid.len() != 64 || !oid.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
            return None;
        }
        if size.is_empty() || !size.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        Some(Self {
            oid: oid.to_owned(),
            size: size.parse().ok()?,
            extensions,
        })
    }

    /// The canonical pointer text (what `git lfs pointer` writes).
    pub fn encode(&self) -> String {
        format!(
            "version {POINTER_VERSION}\noid sha256:{}\nsize {}\n",
            self.oid, self.size
        )
    }

    /// Whether `data` is this pointer's content (size and SHA-256 match).
    pub fn verify(&self, data: &[u8]) -> bool {
        data.len() as u64 == self.size && sha256_hex(data) == self.oid
    }
}

/// `ext-<digit>-<word>`, the key of an LFS extension line.
fn is_extension_key(key: &str) -> bool {
    let Some(rest) = key.strip_prefix("ext-") else {
        return false;
    };
    let mut chars = rest.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_digit())
        && chars.next() == Some('-')
        && chars.as_str().chars().next().is_some()
        && chars
            .as_str()
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_')
}

fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A Git LFS file whose content was not imported (its pointer was).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct LfsMissing {
    pub path: String,
    /// SHA-256 named by the pointer.
    pub oid: String,
    pub size: u64,
    /// Why the content is unavailable.
    pub reason: String,
}

/// What an import did with Git LFS files. Counts are over distinct
/// (path, pointer) pairs in the converted commits.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct LfsReport {
    /// Distinct LFS objects whose verified content replaced the pointer.
    pub resolved: usize,
    /// Pointer files imported as they are because of [`LfsMode::Skip`].
    pub skipped: usize,
    /// Pointer files imported as they are because the content was not
    /// available ([`LfsMode::Auto`]), sorted by path.
    pub missing: Vec<LfsMissing>,
    /// Paths of pointer-shaped files without `filter=lfs` (imported as they
    /// are, like a Git checkout shows them), sorted.
    pub untracked: Vec<String>,
}

/// Locates and verifies LFS content for one Git repository.
pub(crate) struct LfsResolver {
    objects: PathBuf,
    fetch: Option<Fetcher>,
}

/// `git lfs fetch` / `git lfs smudge` in the source repository.
struct Fetcher {
    program: OsString,
    dir: PathBuf,
    state: FetchState,
}

enum FetchState {
    NotTried,
    /// `git lfs fetch --all` ran (its error, if it failed).
    Fetched(Option<String>),
    /// git-lfs cannot be run.
    Unavailable(String),
}

impl LfsResolver {
    /// Resolver for `git`; `fetch` enables `git lfs fetch`/`smudge`.
    pub fn for_repository(git: &git2::Repository, fetch: bool) -> Self {
        let dir = git.workdir().unwrap_or_else(|| git.path()).to_path_buf();
        Self::new(
            lfs_storage(git).join("objects"),
            fetch.then(|| "git".into()),
            dir,
        )
    }

    /// Resolver over the LFS object directory `objects`; `program` (a `git`
    /// executable) enables fetching, run in `dir`.
    pub fn new(objects: PathBuf, program: Option<OsString>, dir: PathBuf) -> Self {
        Self {
            objects,
            fetch: program.map(|program| Fetcher {
                program,
                dir,
                state: FetchState::NotTried,
            }),
        }
    }

    /// Verified content of `pointer` (found at `path`), or why it is not
    /// available.
    pub fn content(
        &mut self,
        pointer: &LfsPointer,
        path: &str,
    ) -> std::result::Result<Vec<u8>, String> {
        if pointer.extensions {
            return Err("the pointer uses Git LFS extensions, which are not supported".into());
        }
        if pointer.size == 0 && pointer.oid == EMPTY_SHA256 {
            return Ok(Vec::new());
        }
        let local = match self.read_local(pointer) {
            Ok(Some(data)) => return Ok(data),
            Ok(None) => "not in the local Git LFS store".to_owned(),
            Err(why) => why,
        };
        let Some(fetcher) = self.fetch.as_mut() else {
            return Err(local);
        };
        let fetched = match fetcher.fetch_all() {
            Err(why) => return Err(format!("{local}; {why}")),
            Ok(fetched) => fetched,
        };
        if let Ok(Some(data)) = self.read_local(pointer) {
            return Ok(data);
        }
        let Some(fetcher) = self.fetch.as_mut() else {
            return Err(local);
        };
        match fetcher.smudge(pointer, path) {
            Ok(data) => Ok(data),
            Err(why) => Err(match fetched {
                Some(fetch_error) => format!("{local}; {fetch_error}; {why}"),
                None => format!("{local}; {why}"),
            }),
        }
    }

    /// Content from the local store: `None` if absent, `Err` if present but
    /// not matching the pointer.
    fn read_local(&self, pointer: &LfsPointer) -> std::result::Result<Option<Vec<u8>>, String> {
        let path = self
            .objects
            .join(&pointer.oid[0..2])
            .join(&pointer.oid[2..4])
            .join(&pointer.oid);
        let data = match fs::read(&path) {
            Ok(data) => data,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(format!("cannot read {}: {e}", path.display())),
        };
        if pointer.verify(&data) {
            Ok(Some(data))
        } else {
            Err(format!(
                "the local Git LFS object {} does not match the pointer (size or SHA-256 differs)",
                path.display()
            ))
        }
    }
}

impl Fetcher {
    fn git(&self) -> Command {
        let mut cmd = Command::new(&self.program);
        cmd.current_dir(&self.dir)
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(Stdio::null());
        cmd
    }

    /// Run `git lfs fetch --all` once. `Ok(Some(error))` if it ran but
    /// failed (some objects may still have arrived), `Err` if git-lfs
    /// cannot be run at all.
    fn fetch_all(&mut self) -> std::result::Result<Option<String>, String> {
        match &self.state {
            FetchState::Fetched(error) => return Ok(error.clone()),
            FetchState::Unavailable(why) => return Err(why.clone()),
            FetchState::NotTried => {}
        }
        let installed = self
            .git()
            .args(["lfs", "version"])
            .output()
            .is_ok_and(|out| out.status.success());
        if !installed {
            let why = "git-lfs is not installed (`git lfs version` failed)".to_owned();
            warn!("{why}; Git LFS content cannot be downloaded");
            self.state = FetchState::Unavailable(why.clone());
            return Err(why);
        }
        info!(dir = %self.dir.display(), "running git lfs fetch --all");
        let error = match self.git().args(["lfs", "fetch", "--all"]).output() {
            Ok(out) if out.status.success() => None,
            Ok(out) => Some(format!(
                "`git lfs fetch --all` failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )),
            Err(e) => Some(format!("cannot run `git lfs fetch --all`: {e}")),
        };
        if let Some(error) = &error {
            warn!("{error}");
        }
        self.state = FetchState::Fetched(error.clone());
        Ok(error)
    }

    /// `git lfs smudge -- <path>` with the pointer on stdin; the output is
    /// verified against the pointer.
    fn smudge(&self, pointer: &LfsPointer, path: &str) -> std::result::Result<Vec<u8>, String> {
        let mut child = self
            .git()
            .args(["lfs", "smudge", "--", path])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("cannot run `git lfs smudge`: {e}"))?;
        if let Some(mut stdin) = child.stdin.take() {
            // A failed write shows up as a failed exit status below.
            let _ = stdin.write_all(pointer.encode().as_bytes());
        }
        let out = child
            .wait_with_output()
            .map_err(|e| format!("`git lfs smudge` failed: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "`git lfs smudge` failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        if pointer.verify(&out.stdout) {
            Ok(out.stdout)
        } else {
            Err("`git lfs smudge` did not produce the content named by the pointer".into())
        }
    }
}

/// The repository's LFS storage directory: `lfs.storage` (relative to the
/// common Git directory) or `<common git dir>/lfs`.
fn lfs_storage(git: &git2::Repository) -> PathBuf {
    let common = common_dir(git.path());
    let configured = git
        .config()
        .ok()
        .and_then(|config| config.get_path("lfs.storage").ok());
    match configured {
        Some(path) if path.is_absolute() => path,
        Some(path) => common.join(path),
        None => common.join("lfs"),
    }
}

/// The common directory of a (possibly linked-worktree) Git directory.
fn common_dir(git_dir: &Path) -> PathBuf {
    match fs::read_to_string(git_dir.join("commondir")) {
        Ok(text) => {
            let path = PathBuf::from(text.trim());
            if path.is_absolute() {
                path
            } else {
                git_dir.join(path)
            }
        }
        Err(_) => git_dir.to_path_buf(),
    }
}

/// How the converter stores one pointer blob at one path.
pub(crate) enum Decision {
    /// Store this verified content in place of the pointer.
    Content(Vec<u8>),
    /// The content was returned by an earlier [`LfsImport::decide`] call
    /// for the same oid (reuse what was stored then).
    Known,
    /// Store the pointer file itself.
    Pointer,
}

/// Per-import LFS state: the resolver, `.gitattributes` rules, caches and
/// the report.
pub(crate) struct LfsImport {
    mode: LfsMode,
    resolver: LfsResolver,
    /// Compiled rules per `.gitattributes` blob.
    rules: HashMap<Oid, LfsRules>,
    /// Content lookups per oid: `Ok(())` once resolved, `Err(reason)` if
    /// unavailable.
    looked_up: HashMap<String, std::result::Result<(), String>>,
    resolved: HashSet<String>,
    skipped: HashSet<(String, String)>,
    missing: HashMap<(String, String), LfsMissing>,
    untracked: HashSet<String>,
}

impl LfsImport {
    pub fn new(mode: LfsMode, resolver: LfsResolver) -> Self {
        Self {
            mode,
            resolver,
            rules: HashMap::new(),
            looked_up: HashMap::new(),
            resolved: HashSet::new(),
            skipped: HashSet::new(),
            missing: HashMap::new(),
            untracked: HashSet::new(),
        }
    }

    /// Decide how to import `pointer` found at `path` in the Git tree
    /// `root`. Fails only in [`LfsMode::Require`] when the content is
    /// unavailable (or on Git errors).
    pub fn decide(
        &mut self,
        git: &git2::Repository,
        root: &git2::Tree<'_>,
        path: &str,
        pointer: &LfsPointer,
    ) -> Result<Decision> {
        if !self.tracked(git, root, path)? {
            if self.untracked.insert(path.to_owned()) {
                warn!(
                    path,
                    "looks like a Git LFS pointer but has no filter=lfs attribute; imported as is"
                );
            }
            return Ok(Decision::Pointer);
        }
        if self.mode == LfsMode::Skip {
            self.skipped.insert((path.to_owned(), pointer.oid.clone()));
            return Ok(Decision::Pointer);
        }
        match self.looked_up.get(&pointer.oid) {
            Some(Ok(())) => return Ok(Decision::Known),
            Some(Err(reason)) => {
                let reason = reason.clone();
                return self.unavailable(path, pointer, reason);
            }
            None => {}
        }
        match self.resolver.content(pointer, path) {
            Ok(data) => {
                self.looked_up.insert(pointer.oid.clone(), Ok(()));
                self.resolved.insert(pointer.oid.clone());
                Ok(Decision::Content(data))
            }
            Err(reason) => {
                self.looked_up
                    .insert(pointer.oid.clone(), Err(reason.clone()));
                self.unavailable(path, pointer, reason)
            }
        }
    }

    fn unavailable(
        &mut self,
        path: &str,
        pointer: &LfsPointer,
        reason: String,
    ) -> Result<Decision> {
        if self.mode == LfsMode::Require {
            return Err(GitCompatError::ImportError(format!(
                "Git LFS content of {path} (sha256:{}, {} bytes) is not available: {reason}",
                pointer.oid, pointer.size
            )));
        }
        let key = (path.to_owned(), pointer.oid.clone());
        if let std::collections::hash_map::Entry::Vacant(slot) = self.missing.entry(key) {
            warn!(
                path,
                oid = %pointer.oid,
                size = pointer.size,
                %reason,
                "Git LFS content not available; imported the pointer file"
            );
            slot.insert(LfsMissing {
                path: path.to_owned(),
                oid: pointer.oid.clone(),
                size: pointer.size,
                reason,
            });
        }
        Ok(Decision::Pointer)
    }

    /// Whether `path` has `filter=lfs` according to the `.gitattributes`
    /// files of its directory and every parent directory in `root` (the
    /// deepest file wins, like Git).
    fn tracked(
        &mut self,
        git: &git2::Repository,
        root: &git2::Tree<'_>,
        path: &str,
    ) -> Result<bool> {
        let mut decision = None;
        let mut dir = String::new();
        let mut rest = path;
        loop {
            let attributes = if dir.is_empty() {
                ".gitattributes".to_owned()
            } else {
                format!("{dir}/.gitattributes")
            };
            if let Ok(entry) = root.get_path(Path::new(&attributes)) {
                if entry.kind() == Some(git2::ObjectType::Blob) {
                    let rules = match self.rules.entry(entry.id()) {
                        std::collections::hash_map::Entry::Occupied(slot) => slot.into_mut(),
                        std::collections::hash_map::Entry::Vacant(slot) => {
                            let blob = git.find_blob(entry.id())?;
                            slot.insert(LfsRules::parse(&String::from_utf8_lossy(blob.content())))
                        }
                    };
                    if let Some(found) = rules.decision(rest) {
                        decision = Some(found);
                    }
                }
            }
            let Some((head, tail)) = rest.split_once('/') else {
                break;
            };
            if dir.is_empty() {
                dir = head.to_owned();
            } else {
                dir = format!("{dir}/{head}");
            }
            rest = tail;
        }
        Ok(decision == Some(true))
    }

    /// The report so far.
    pub fn report(&self) -> LfsReport {
        let mut missing: Vec<LfsMissing> = self.missing.values().cloned().collect();
        missing.sort();
        let mut untracked: Vec<String> = self.untracked.iter().cloned().collect();
        untracked.sort();
        LfsReport {
            resolved: self.resolved.len(),
            skipped: self.skipped.len(),
            missing,
            untracked,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OID: &str = "4d7a214614ab2935c943f9e0ff69d22eadbb8f32b1258daaa5e2ca24d17e2393";

    fn pointer_text(oid: &str, size: u64) -> String {
        format!("version {POINTER_VERSION}\noid sha256:{oid}\nsize {size}\n")
    }

    #[test]
    fn parses_canonical_and_tolerated_pointers() {
        let p = LfsPointer::parse(pointer_text(OID, 12345).as_bytes()).unwrap();
        assert_eq!(p.oid, OID);
        assert_eq!(p.size, 12345);
        assert!(!p.extensions);
        assert_eq!(p.encode(), pointer_text(OID, 12345));

        // CRLF, no trailing newline, blank lines and surrounding whitespace.
        let crlf = format!("version {POINTER_VERSION}\r\noid sha256:{OID}\r\nsize 7\r\n");
        assert_eq!(LfsPointer::parse(crlf.as_bytes()).unwrap().size, 7);
        let bare = format!("version {POINTER_VERSION}\noid sha256:{OID}\nsize 7");
        assert!(LfsPointer::parse(bare.as_bytes()).is_some());
        let spaced = format!("\n  version {POINTER_VERSION}\n\noid sha256:{OID}\nsize 7\n\n");
        assert!(LfsPointer::parse(spaced.as_bytes()).is_some());

        // Legacy spec URL and extension lines.
        let legacy =
            format!("version https://hawser.github.com/spec/v1\noid sha256:{OID}\nsize 1\n");
        assert!(LfsPointer::parse(legacy.as_bytes()).is_some());
        let ext = format!(
            "version {POINTER_VERSION}\next-0-foo sha256:{OID}\noid sha256:{OID}\nsize 1\n"
        );
        assert!(LfsPointer::parse(ext.as_bytes()).unwrap().extensions);
    }

    #[test]
    fn near_misses_are_not_pointers() {
        let v = POINTER_VERSION;
        let upper = OID.to_uppercase();
        let short = &OID[..63];
        let long_text = format!("{}{}", pointer_text(OID, 1), "x".repeat(MAX_POINTER_SIZE));
        let cases: Vec<String> = vec![
            String::new(),
            "hello world\n".into(),
            // Prose about LFS.
            format!("This repo uses git-lfs.\nversion {v}\n"),
            // Missing keys.
            format!("version {v}\noid sha256:{OID}\n"),
            format!("version {v}\nsize 12\n"),
            format!("oid sha256:{OID}\nsize 12\n"),
            // Wrong order.
            format!("version {v}\nsize 12\noid sha256:{OID}\n"),
            format!("oid sha256:{OID}\nversion {v}\nsize 12\n"),
            // Unknown version, hash algorithm, malformed oid or size.
            format!("version https://example.com/spec/v1\noid sha256:{OID}\nsize 12\n"),
            format!("version {v}\noid sha1:{}\nsize 12\n", &OID[..40]),
            format!("version {v}\noid sha256:{upper}\nsize 12\n"),
            format!("version {v}\noid sha256:{short}\nsize 12\n"),
            format!("version {v}\noid sha256:{OID}0\nsize 12\n"),
            format!("version {v}\noid {OID}\nsize 12\n"),
            format!("version {v}\noid sha256:{OID}\nsize -12\n"),
            format!("version {v}\noid sha256:{OID}\nsize 12kb\n"),
            format!("version {v}\noid sha256:{OID}\nsize\n"),
            format!("version {v}\noid sha256:{OID}\nsize \n"),
            // Extra or unknown lines.
            format!("version {v}\noid sha256:{OID}\nsize 12\nsize 12\n"),
            format!("version {v}\noid sha256:{OID}\nsize 12\nmore stuff\n"),
            format!("version {v}\ncomment x\noid sha256:{OID}\nsize 12\n"),
            format!("ext-0-foo sha256:{OID}\nversion {v}\noid sha256:{OID}\nsize 12\n"),
            // Tab instead of space.
            format!("version\t{v}\noid sha256:{OID}\nsize 12\n"),
            // Too large to be a pointer.
            long_text,
        ];
        for case in &cases {
            assert_eq!(LfsPointer::parse(case.as_bytes()), None, "{case:?}");
        }
        // Not UTF-8.
        assert_eq!(LfsPointer::parse(&[0xff, 0xfe, b'v']), None);
    }

    fn store_object(objects: &Path, data: &[u8]) -> String {
        let oid = sha256_hex(data);
        let dir = objects.join(&oid[0..2]).join(&oid[2..4]);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(&oid), data).unwrap();
        oid
    }

    #[test]
    fn local_store_content_is_verified() {
        let dir = tempfile::tempdir().unwrap();
        let objects = dir.path().join("objects");
        let data = b"large binary content".to_vec();
        let oid = store_object(&objects, &data);
        let mut resolver = LfsResolver::new(objects.clone(), None, dir.path().into());

        let good = LfsPointer {
            oid: oid.clone(),
            size: data.len() as u64,
            extensions: false,
        };
        assert_eq!(resolver.content(&good, "a.bin").unwrap(), data);

        // Wrong size in the pointer.
        let wrong_size = LfsPointer {
            size: 3,
            ..good.clone()
        };
        let err = resolver.content(&wrong_size, "a.bin").unwrap_err();
        assert!(err.contains("does not match"), "{err}");

        // Corrupt object in the store (same size, different bytes).
        let path = objects.join(&oid[0..2]).join(&oid[2..4]).join(&oid);
        fs::write(&path, b"LARGE BINARY CONTENT").unwrap();
        let err = resolver.content(&good, "a.bin").unwrap_err();
        assert!(err.contains("does not match"), "{err}");

        // Absent.
        let absent = LfsPointer {
            oid: OID.into(),
            size: 5,
            extensions: false,
        };
        assert_eq!(
            resolver.content(&absent, "b.bin").unwrap_err(),
            "not in the local Git LFS store"
        );

        // Empty files need no store; extensions are refused.
        let empty = LfsPointer {
            oid: EMPTY_SHA256.into(),
            size: 0,
            extensions: false,
        };
        assert_eq!(resolver.content(&empty, "e").unwrap(), Vec::<u8>::new());
        let ext = LfsPointer {
            extensions: true,
            ..good
        };
        assert!(resolver
            .content(&ext, "x")
            .unwrap_err()
            .contains("extensions"));
    }

    #[test]
    fn fetch_without_git_lfs_reports_why() {
        let dir = tempfile::tempdir().unwrap();
        let mut resolver = LfsResolver::new(
            dir.path().join("objects"),
            Some(dir.path().join("no-such-git").into_os_string()),
            dir.path().into(),
        );
        let absent = LfsPointer {
            oid: OID.into(),
            size: 5,
            extensions: false,
        };
        let err = resolver.content(&absent, "a.bin").unwrap_err();
        assert!(err.contains("not in the local Git LFS store"), "{err}");
        assert!(err.contains("git-lfs is not installed"), "{err}");
    }

    mod converter {
        use super::*;
        use crate::import::TreeConverter;
        use crate::test_support::{GitFixture, MemStore, Spec};

        struct Lfs {
            fx: GitFixture,
            tree: Oid,
            present: Vec<u8>,
            present_ptr: String,
            missing_ptr: String,
        }

        /// `a.bin` (content in the local store), `b.bin` (content missing),
        /// a pointer-shaped `docs/c.txt` without the attribute and
        /// `sub/d.bin` whose directory unsets the filter.
        fn fixture() -> Lfs {
            let fx = GitFixture::new();
            let present = b"the real large content\n".to_vec();
            let oid = store_object(&fx.repo.path().join("lfs/objects"), &present);
            let present_ptr = pointer_text(&oid, present.len() as u64);
            let missing_ptr = pointer_text(OID, 99);
            let tree = fx.tree(&[
                Spec::File(
                    ".gitattributes",
                    b"*.bin filter=lfs diff=lfs merge=lfs -text\n",
                ),
                Spec::File("a.bin", present_ptr.as_bytes()),
                Spec::File("b.bin", missing_ptr.as_bytes()),
                Spec::File("docs/c.txt", present_ptr.as_bytes()),
                Spec::File("sub/.gitattributes", b"*.bin -filter\n"),
                Spec::File("sub/d.bin", present_ptr.as_bytes()),
                Spec::File("plain.bin", b"not a pointer"),
            ]);
            Lfs {
                fx,
                tree,
                present,
                present_ptr,
                missing_ptr,
            }
        }

        fn convert(
            lfs: &Lfs,
            mode: LfsMode,
        ) -> (Result<worktree_sdk::Manifest>, MemStore, LfsReport) {
            let store = MemStore::default();
            let import = LfsImport::new(mode, LfsResolver::for_repository(&lfs.fx.repo, false));
            let mut conv = TreeConverter::new(&lfs.fx.repo, &store).with_lfs(import);
            let tree = lfs.fx.repo.find_tree(lfs.tree).unwrap();
            let manifest = conv.manifest(&tree);
            // Converting the same tree again reuses the resolved content.
            if let Ok(first) = &manifest {
                assert_eq!(conv.manifest(&tree).unwrap(), *first);
            }
            let report = conv.lfs_report();
            (manifest, store, report)
        }

        fn content(store: &MemStore, manifest: &worktree_sdk::Manifest, path: &str) -> Vec<u8> {
            let entry = manifest.get(path).unwrap();
            let data = store.get(&entry.hash).unwrap();
            assert_eq!(entry.size, data.len() as u64, "{path}");
            data
        }

        #[test]
        fn auto_substitutes_available_content_and_reports_the_rest() {
            let lfs = fixture();
            let (manifest, store, report) = convert(&lfs, LfsMode::Auto);
            let manifest = manifest.unwrap();
            assert_eq!(content(&store, &manifest, "a.bin"), lfs.present);
            assert_eq!(
                content(&store, &manifest, "b.bin"),
                lfs.missing_ptr.as_bytes()
            );
            assert_eq!(
                content(&store, &manifest, "docs/c.txt"),
                lfs.present_ptr.as_bytes()
            );
            assert_eq!(
                content(&store, &manifest, "sub/d.bin"),
                lfs.present_ptr.as_bytes()
            );
            assert_eq!(content(&store, &manifest, "plain.bin"), b"not a pointer");
            assert_eq!(report.resolved, 1);
            assert_eq!(report.skipped, 0);
            assert_eq!(report.missing.len(), 1);
            assert_eq!(report.missing[0].path, "b.bin");
            assert_eq!(report.missing[0].oid, OID);
            assert_eq!(report.missing[0].size, 99);
            assert_eq!(report.missing[0].reason, "not in the local Git LFS store");
            assert_eq!(report.untracked, vec!["docs/c.txt", "sub/d.bin"]);
        }

        #[test]
        fn require_fails_on_missing_content() {
            let lfs = fixture();
            let (manifest, _, _) = convert(&lfs, LfsMode::Require);
            let err = manifest.unwrap_err();
            assert!(
                matches!(&err, GitCompatError::ImportError(m) if m.contains("b.bin")),
                "{err}"
            );
            assert!(err.to_string().contains(OID), "{err}");
        }

        #[test]
        fn skip_keeps_every_pointer() {
            let lfs = fixture();
            let (manifest, store, report) = convert(&lfs, LfsMode::Skip);
            let manifest = manifest.unwrap();
            assert_eq!(
                content(&store, &manifest, "a.bin"),
                lfs.present_ptr.as_bytes()
            );
            assert_eq!(
                content(&store, &manifest, "b.bin"),
                lfs.missing_ptr.as_bytes()
            );
            assert_eq!(report.resolved, 0);
            assert_eq!(report.skipped, 2);
            assert!(report.missing.is_empty());
        }

        #[test]
        fn without_lfs_handling_pointers_are_plain_files() {
            let lfs = fixture();
            let store = MemStore::default();
            let tree = lfs.fx.repo.find_tree(lfs.tree).unwrap();
            let mut conv = TreeConverter::new(&lfs.fx.repo, &store);
            let manifest = conv.manifest(&tree).unwrap();
            assert_eq!(
                content(&store, &manifest, "a.bin"),
                lfs.present_ptr.as_bytes()
            );
            assert_eq!(conv.lfs_report(), LfsReport::default());
        }
    }

    /// A fake `git` whose `lfs fetch` copies objects from a "server"
    /// directory into the store and whose `lfs smudge` serves one object.
    #[cfg(unix)]
    #[test]
    fn fetch_and_smudge_through_git_lfs() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let objects = dir.path().join("lfs/objects");
        let server = dir.path().join("server");
        let fetched = b"fetched by git lfs fetch".to_vec();
        let smudged = b"downloaded by git lfs smudge".to_vec();
        let fetched_oid = store_object(&server, &fetched);
        let smudged_oid = sha256_hex(&smudged);
        fs::write(dir.path().join("smudge-output"), &smudged).unwrap();
        let script = format!(
            "#!/bin/sh\n\
             [ \"$1\" = lfs ] || exit 2\n\
             case \"$2\" in\n\
               version) echo 'git-lfs/fake'; exit 0;;\n\
               fetch) mkdir -p '{objects}' && cp -R '{server}/.' '{objects}/'; echo fetch >> '{log}'; exit 0;;\n\
               smudge) cat > /dev/null; cat '{out}'; exit 0;;\n\
             esac\n\
             exit 2\n",
            objects = objects.display(),
            server = server.display(),
            log = dir.path().join("log").display(),
            out = dir.path().join("smudge-output").display(),
        );
        let git = dir.path().join("fake-git");
        fs::write(&git, script).unwrap();
        fs::set_permissions(&git, fs::Permissions::from_mode(0o755)).unwrap();

        let mut resolver = LfsResolver::new(
            objects.clone(),
            Some(git.into_os_string()),
            dir.path().into(),
        );
        let from_fetch = LfsPointer {
            oid: fetched_oid,
            size: fetched.len() as u64,
            extensions: false,
        };
        assert_eq!(resolver.content(&from_fetch, "a.bin").unwrap(), fetched);
        let from_smudge = LfsPointer {
            oid: smudged_oid,
            size: smudged.len() as u64,
            extensions: false,
        };
        assert_eq!(resolver.content(&from_smudge, "b.bin").unwrap(), smudged);
        // Smudge output that does not match the pointer is rejected.
        let mismatch = LfsPointer {
            oid: OID.into(),
            size: smudged.len() as u64,
            extensions: false,
        };
        let err = resolver.content(&mismatch, "c.bin").unwrap_err();
        assert!(err.contains("did not produce"), "{err}");
        // `git lfs fetch --all` ran exactly once.
        assert_eq!(
            fs::read_to_string(dir.path().join("log")).unwrap(),
            "fetch\n"
        );
    }
}
