//! `git-remote-wt`: a Git remote helper for W0rkTree servers.
//!
//! With the `git-remote-wt` binary on `PATH`, stock Git can use a server
//! tree as a remote:
//!
//! ```text
//! git clone wt::https://wt.example.com/acme/web
//! git fetch origin
//! git push origin main
//! ```
//!
//! # How it works
//!
//! Git starts `git-remote-wt <remote> <url>` and talks to it over
//! stdin/stdout ([`protocol`], gitremote-helpers(7)). The helper announces
//! the `fetch`, `push` and `option` capabilities: it reads and writes the
//! Git object database directly through libgit2, with the same tree and
//! commit conversion as `wt git import` / `wt git export`. (The
//! alternative `import`/`export` capabilities would have Git compute
//! commit ids from a fast-import stream and need a stream parser; the
//! conversions of this crate work on object databases.)
//!
//! * **Cache.** `$GIT_DIR/wt/<remote>/` holds a W0rkTree worktree without
//!   checked-out files. The SDK fetches the server tree into it
//!   incrementally and pushes from it.
//! * **`list` / `fetch`.** The helper fetches into the cache, writes the
//!   commits (and annotated tag objects) of every server branch and tag
//!   that Git does not have yet, and advertises them. Snapshot -> commit
//!   is deterministic, so every clone computes the same commit ids.
//! * **`push`.** New commits become snapshots in the cache (ids as in
//!   `wt git import`), then the SDK pushes the branch. History is
//!   append-only: a push that is not a fast-forward is refused, with or
//!   without `--force`, and so are deleting refs and moving tags. Commits
//!   containing submodules or non-UTF-8 file names are
//!   refused rather than pushed without that content.
//! * **Authentication** is the SDK's: the login saved by `wt login` in
//!   `$WT_HOME`, or `WT_TOKEN`. Public trees can be read anonymously.
//!
//! A commit pushed through the helper keeps its id where it was pushed and
//! wherever the repository already has it. Elsewhere it is rebuilt from
//! the snapshot, which does not keep Git's committer, time zones or
//! signatures; see `docs/git-compatibility.md` for when ids agree.

mod cache;
pub mod protocol;
mod target;
mod to_git;
mod to_wt;

use std::path::PathBuf;

use git2::{ObjectType, Oid};
use worktree_sdk::remote::{self, CredentialStore, SyncClient};
use worktree_sdk::{SdkError, TagInfo};

use self::cache::{Cache, TagObjects, ORIGIN};
use self::protocol::{Backend, FetchSpec, ListedRef, PushResult, PushSpec};
use self::to_git::Exported;
use self::to_wt::CommitConverter;
use crate::error::{GitCompatError, Result};
use crate::ident;
use crate::map::GitMap;

pub use self::target::Target;

/// Name of the helper binary; Git derives it from the `wt::` URL prefix.
pub const BINARY_NAME: &str = "git-remote-wt";

/// A remote ref the helper can push to.
enum Destination<'a> {
    Branch(&'a str),
    Tag(&'a str),
}

fn destination(dst: &str) -> std::result::Result<Destination<'_>, String> {
    if let Some(name) = dst.strip_prefix("refs/heads/").filter(|n| !n.is_empty()) {
        Ok(Destination::Branch(name))
    } else if let Some(name) = dst.strip_prefix("refs/tags/").filter(|n| !n.is_empty()) {
        Ok(Destination::Tag(name))
    } else {
        Err(format!(
            "W0rkTree has branches and tags only; cannot push to {dst}"
        ))
    }
}

/// The reason shown next to a ref Git reports as rejected.
fn reason(err: GitCompatError) -> String {
    match err {
        GitCompatError::SdkError(SdkError::PushRejected(message)) => message,
        GitCompatError::SdkError(SdkError::NotLoggedIn(server)) => {
            format!("not logged in to {server}; run `wt login {server}` or set WT_TOKEN")
        }
        GitCompatError::ImportError(message) => message,
        other => other.to_string(),
    }
}

fn non_fast_forward(branch: &str, force: bool) -> String {
    if force {
        format!(
            "W0rkTree history is append-only: --force cannot replace snapshots of '{branch}'; \
             fetch, merge the remote branch and push again"
        )
    } else {
        format!(
            "non-fast-forward: the server has snapshots of '{branch}' this push does not contain; \
             fetch, merge the remote branch and push again"
        )
    }
}

/// The repository's common directory: the Git directory itself, or for a
/// linked work tree (`git worktree add`) the main repository's, which its
/// `commondir` file names.
fn common_dir(git: &git2::Repository) -> PathBuf {
    let git_dir = git.path();
    match std::fs::read_to_string(git_dir.join("commondir")) {
        Ok(relative) => git_dir.join(relative.trim()),
        Err(_) => git_dir.to_path_buf(),
    }
}

/// What a refresh learned about the server tree.
struct State {
    cache: Cache,
    exported: Exported,
    default_branch: Option<String>,
}

/// The helper for one remote of one Git repository; implements
/// [`Backend`] on top of the SDK and libgit2.
pub struct Helper {
    git: git2::Repository,
    cache_dir: PathBuf,
    target: Target,
    runtime: tokio::runtime::Runtime,
    verbosity: u32,
    state: Option<State>,
    /// Keeps the scratch repository alive when Git runs the helper outside
    /// a repository (`git ls-remote wt::...`).
    _scratch: Option<tempfile::TempDir>,
}

impl Helper {
    /// A helper for the remote Git calls `alias`, at `address`
    /// (see [`Target::parse`]).
    ///
    /// `git_dir` is the repository Git runs the helper for (`$GIT_DIR`);
    /// `None` uses a temporary repository that is deleted afterwards.
    /// Errors when the address is invalid or the repository cannot be
    /// opened.
    pub fn new(alias: &str, address: &str, git_dir: Option<PathBuf>) -> Result<Self> {
        let target = Target::parse(address)?;
        let (git, cache_root, scratch) = match git_dir {
            Some(dir) => {
                let git = git2::Repository::open(&dir)?;
                // Linked work trees share the main repository's cache.
                let root = common_dir(&git).join("wt");
                (git, root, None)
            }
            None => {
                let scratch = tempfile::tempdir()?;
                let git = git2::Repository::init_bare(scratch.path().join("git"))?;
                (git, scratch.path().join("wt"), Some(scratch))
            }
        };
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;
        Ok(Self {
            git,
            cache_dir: cache_root.join(cache::dir_name(alias, address)),
            target,
            runtime,
            verbosity: 1,
            state: None,
            _scratch: scratch,
        })
    }

    fn say(&self, message: std::fmt::Arguments<'_>) {
        if self.verbosity >= 1 {
            eprintln!("wt: {message}");
        }
    }

    /// Bring the cache up to date with the server and write the Git
    /// objects of everything it has.
    fn refresh(&mut self, for_push: bool) -> Result<()> {
        let cache = cache::open_or_create(&self.cache_dir, &self.target)?;
        let fetched = self
            .runtime
            .block_on(remote::fetch(&cache.repo, &cache.tree, ORIGIN));
        let exists = match fetched {
            Ok(report) => {
                if report.snapshots_received > 0 {
                    self.say(format_args!(
                        "received {} snapshot(s) of {}",
                        report.snapshots_received,
                        self.target.spec()
                    ));
                }
                true
            }
            // The first push creates the tree.
            Err(SdkError::TreeNotFound(_)) if for_push => false,
            Err(SdkError::TreeNotFound(_)) => {
                return Err(GitCompatError::RemoteError(format!(
                    "tree {} not found on {} (or it is not visible to you: \
                     run `wt login {}` or set WT_TOKEN)",
                    self.target.spec(),
                    self.target.server,
                    self.target.server
                )));
            }
            Err(e) => return Err(e.into()),
        };
        let exported = to_git::export_remote(&cache, &self.git)?;
        let default_branch = if exists { self.default_branch() } else { None };
        self.state = Some(State {
            cache,
            exported,
            default_branch,
        });
        Ok(())
    }

    /// The server tree's default branch; `None` if it cannot be read
    /// (`HEAD` is then guessed from the branch names).
    fn default_branch(&self) -> Option<String> {
        let target = &self.target;
        let resolved = self.runtime.block_on(async {
            let store = CredentialStore::default_location();
            let mut client = SyncClient::connect_with_store(&store, &target.server).await?;
            client
                .resolve_tree(&target.tenant, &target.tree, false)
                .await
        });
        match resolved {
            Ok(tree) => Some(tree.default_branch).filter(|b| !b.is_empty()),
            Err(e) => {
                tracing::warn!(error = %e, "could not read the default branch");
                None
            }
        }
    }

    fn state(&mut self, for_push: bool) -> Result<&State> {
        if self.state.is_none() {
            self.refresh(for_push)?;
        }
        self.state
            .as_ref()
            .ok_or_else(|| GitCompatError::RemoteError("the remote was not read".into()))
    }

    fn push_branch(&self, state: &State, spec: &PushSpec, branch: &str) -> Result<()> {
        let Some(src) = &spec.src else {
            return Err(GitCompatError::ImportError(format!(
                "W0rkTree history is append-only: branches are not deleted through Git \
                 (use `wt branch archive {branch}` instead)"
            )));
        };
        let repo = &state.cache.repo;
        let tree = repo.tree(&state.cache.tree)?;
        let tip = self.git.revparse_single(src)?.peel_to_commit()?.id();

        let wt_dir = repo.wt_dir();
        let mut map = GitMap::load(&wt_dir)?;
        let (snapshot, written) = CommitConverter::new(repo, &self.git, &tree.id).convert(
            tip,
            branch,
            &state.exported.tips,
            &mut map,
        )?;
        if written > 0 {
            map.save(&wt_dir)?;
        }

        if let Some(server_tip) = repo.remote_tip(&tree.name, ORIGIN, branch)? {
            if server_tip == snapshot {
                return Ok(());
            }
            if !repo.is_ancestor(&server_tip, &snapshot)? {
                return Err(GitCompatError::ImportError(non_fast_forward(
                    branch, spec.force,
                )));
            }
        }
        repo.set_branch_tip(&tree.name, branch, Some(&snapshot))?;
        let report = self
            .runtime
            .block_on(remote::push(repo, &tree.name, ORIGIN, Some(branch)))?;
        if !report.up_to_date {
            self.say(format_args!(
                "pushed {} snapshot(s) to {} ({branch})",
                report.snapshots_sent,
                self.target.spec()
            ));
        }
        Ok(())
    }

    fn push_tag(&self, state: &State, spec: &PushSpec, name: &str) -> Result<()> {
        let refused = GitCompatError::ImportError;
        let Some(src) = &spec.src else {
            return Err(refused(format!(
                "tags are not deleted through Git (use `wt tag delete {name} --remote`)"
            )));
        };
        let repo = &state.cache.repo;
        let tree = &state.cache.tree;
        let object = self.git.revparse_single(src)?;
        let commit = object
            .peel(ObjectType::Commit)
            .map_err(|_| refused(format!("tag {name} does not point at a commit")))?;
        let wt_dir = repo.wt_dir();
        let map = GitMap::load(&wt_dir)?;
        let on_server = |snapshot: &str| -> Result<bool> {
            for (_, tip) in repo.remote_branches(tree, ORIGIN)? {
                if repo.is_ancestor(snapshot, &tip)? {
                    return Ok(true);
                }
            }
            Ok(false)
        };
        let target = match map.snapshot_for(&commit.id().to_string()) {
            Some(id) if repo.has_snapshot(id) && on_server(id)? => id.to_owned(),
            _ => {
                return Err(refused(format!(
                    "the commit tag {name} points at is not on a branch of the server; \
                     push its branch first (or together with the tag)"
                )));
            }
        };

        if let Some(existing) = repo.tags(tree)?.into_iter().find(|t| t.name == name) {
            if existing.target == target {
                return Ok(());
            }
            return Err(refused(format!(
                "tag {name} already exists on the server and points elsewhere; \
                 W0rkTree tags are not moved through Git"
            )));
        }

        let annotated = object.as_tag();
        let tagger = annotated.and_then(|t| t.tagger());
        let author = commit
            .as_commit()
            .map(|c| c.author().to_owned())
            .ok_or_else(|| refused(format!("tag {name} does not point at a commit")))?;
        let time = match (&tagger, commit.as_commit()) {
            (Some(tagger), _) => tagger.when(),
            (None, Some(c)) => c.time(),
            (None, None) => git2::Time::new(0, 0),
        };
        let info = TagInfo {
            name: name.to_owned(),
            target: target.clone(),
            message: annotated
                .map(|t| ident::message_from_git(t.message_bytes().unwrap_or_default())),
            tagger: ident::author_of(tagger.as_ref().unwrap_or(&author)),
            created_at: ident::git_time_to_utc(time),
            signature: None,
        };
        repo.put_tag(tree, &info)?;
        if let Err(e) = self
            .runtime
            .block_on(remote::push_tag(repo, tree, ORIGIN, name))
        {
            // The cache mirrors the server: forget the tag it refused.
            if let Err(cleanup) = repo.delete_tag(tree, name) {
                tracing::warn!(tag = name, error = %cleanup, "could not remove the refused tag from the cache");
            }
            return Err(e.into());
        }
        if let Some(tag) = annotated {
            let mut pushed = TagObjects::load(&wt_dir)?;
            pushed.record(name, &tag.id().to_string(), &target);
            pushed.save(&wt_dir)?;
        }
        Ok(())
    }
}

impl Backend for Helper {
    fn option(&mut self, name: &str, value: &str) -> bool {
        match name {
            "verbosity" => match value.parse() {
                Ok(level) => {
                    self.verbosity = level;
                    true
                }
                Err(_) => false,
            },
            // Accepted; the helper prints no progress meter.
            "progress" => matches!(value, "true" | "false"),
            _ => false,
        }
    }

    fn list(&mut self, for_push: bool) -> Result<Vec<ListedRef>> {
        self.refresh(for_push)?;
        let state = self.state(for_push)?;
        let exported = &state.exported;
        let mut refs = Vec::with_capacity(exported.branches.len() + exported.tags.len() + 1);
        let has = |name: &str| exported.branches.iter().any(|(branch, _)| branch == name);
        let head = state
            .default_branch
            .as_deref()
            .filter(|name| has(name))
            .or_else(|| Some("main").filter(|name| has(name)))
            .or_else(|| exported.branches.first().map(|(name, _)| name.as_str()));
        if let Some(head) = head {
            refs.push(ListedRef::Symbolic {
                name: "HEAD".to_owned(),
                target: format!("refs/heads/{head}"),
            });
        }
        for (name, oid) in &exported.branches {
            refs.push(ListedRef::Object {
                name: format!("refs/heads/{name}"),
                sha: oid.to_string(),
            });
        }
        for (name, oid) in &exported.tags {
            refs.push(ListedRef::Object {
                name: format!("refs/tags/{name}"),
                sha: oid.to_string(),
            });
        }
        Ok(refs)
    }

    fn fetch(&mut self, wants: &[FetchSpec]) -> Result<()> {
        // `list` already wrote every advertised object; make sure of it.
        self.state(false)?;
        let odb = self.git.odb()?;
        for want in wants {
            let present = Oid::from_str(&want.sha).is_ok_and(|oid| odb.exists(oid));
            if !present {
                return Err(GitCompatError::RemoteError(format!(
                    "{} ({}) is not on the server; fetch again",
                    want.name, want.sha
                )));
            }
        }
        Ok(())
    }

    fn push(&mut self, specs: &[PushSpec]) -> Result<Vec<PushResult>> {
        self.state(true)?;
        let Some(state) = self.state.as_ref() else {
            return Err(GitCompatError::RemoteError(
                "the remote was not read".into(),
            ));
        };
        let mut outcomes: Vec<Option<std::result::Result<(), String>>> = vec![None; specs.len()];
        // Branches first: a tag can only name a snapshot the server has.
        for tags in [false, true] {
            for (spec, outcome) in specs.iter().zip(outcomes.iter_mut()) {
                let result = match destination(&spec.dst) {
                    Ok(Destination::Branch(name)) if !tags => {
                        self.push_branch(state, spec, name).map_err(reason)
                    }
                    Ok(Destination::Tag(name)) if tags => {
                        self.push_tag(state, spec, name).map_err(reason)
                    }
                    Ok(_) => continue,
                    Err(why) => Err(why),
                };
                *outcome = Some(result);
            }
        }
        Ok(specs
            .iter()
            .zip(outcomes)
            .map(|(spec, outcome)| PushResult {
                dst: spec.dst.clone(),
                outcome: outcome.unwrap_or_else(|| Err("not processed".to_owned())),
            })
            .collect())
    }
}

/// Run the helper as Git invokes it: `args` are the command-line arguments
/// after the program name (`<remote> [<url>]`), the conversation is on
/// stdin/stdout, and `$GIT_DIR` names the repository.
///
/// Errors are for the caller to print on stderr before exiting non-zero.
pub fn run(args: &[String]) -> Result<()> {
    let (alias, address) = match args {
        [alias, address] => (alias.as_str(), address.as_str()),
        // Git passes only the URL when the remote has no name.
        [address] => (address.as_str(), address.as_str()),
        _ => {
            return Err(GitCompatError::RemoteError(format!(
                "usage: {BINARY_NAME} <remote> <url> — Git runs this for remotes like \
                 wt::https://<host>/<tenant>/<tree>"
            )));
        }
    };
    let git_dir = std::env::var_os("GIT_DIR")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from);
    let mut helper = Helper::new(alias, address, git_dir)?;
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    protocol::serve(&mut helper, &mut stdin.lock(), &mut stdout.lock())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn destinations() {
        assert!(matches!(
            destination("refs/heads/feature/x"),
            Ok(Destination::Branch("feature/x"))
        ));
        assert!(matches!(
            destination("refs/tags/v1.0"),
            Ok(Destination::Tag("v1.0"))
        ));
        for bad in [
            "refs/notes/commits",
            "refs/heads/",
            "HEAD",
            "refs/pull/1/head",
        ] {
            let why = destination(bad).err().expect(bad);
            assert!(why.contains(bad), "{why}");
        }
    }

    #[test]
    fn reasons_are_actionable() {
        assert_eq!(
            reason(SdkError::PushRejected("branch main is protected".into()).into()),
            "branch main is protected"
        );
        let login = reason(SdkError::NotLoggedIn("http://localhost:7420".into()).into());
        assert!(
            login.contains("wt login http://localhost:7420") && login.contains("WT_TOKEN"),
            "{login}"
        );
        assert!(non_fast_forward("main", true).contains("append-only"));
        assert!(non_fast_forward("main", true).contains("--force"));
        assert!(non_fast_forward("main", false).contains("non-fast-forward"));
    }

    #[test]
    fn usage_and_bad_urls_are_errors() {
        assert!(matches!(run(&[]), Err(GitCompatError::RemoteError(m)) if m.contains("usage")));
        let err = Helper::new("origin", "https://wt.example.com/only-tenant", None)
            .err()
            .expect("bad url");
        assert!(err.to_string().contains("<tenant>/<tree>"), "{err}");
    }

    #[test]
    fn unknown_options_are_unsupported() {
        let mut helper = Helper::new("origin", "https://wt.example.com/acme/web", None).unwrap();
        assert!(helper.option("verbosity", "0"));
        assert_eq!(helper.verbosity, 0);
        assert!(!helper.option("verbosity", "loud"));
        assert!(helper.option("progress", "true"));
        for name in ["depth", "dry-run", "followtags", "atomic"] {
            assert!(!helper.option(name, "true"), "{name}");
        }
    }
}
