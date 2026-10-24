//! Moving Git repositories to and from Git hosts.
//!
//! [`push`] publishes the branches/tags of a local Git repository (typically
//! one produced by `wt git export`) to a URL; [`fetch_mirror`] mirrors a
//! remote repository into a local bare repository that `wt git import` can
//! then read.

pub mod auth;
pub mod transport;

use std::cell::RefCell;
use std::path::Path;

use git2::{AutotagOption, BranchType, Direction, FetchOptions, Repository};
use tracing::info;

use crate::error::{GitCompatError, Result};
use auth::GitAuth;
use transport::GitTransport;

/// What [`push`] sends.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PushOptions {
    /// Branches to push (default: all local branches).
    pub branches: Vec<String>,
    /// Also push every tag.
    pub include_tags: bool,
    /// Overwrite remote refs even when the update is not a fast-forward.
    pub force: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PushReport {
    /// Full ref names that were pushed.
    pub refs: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FetchReport {
    /// Branches present in the mirror after the fetch.
    pub branches: Vec<String>,
    /// Tags present in the mirror after the fetch.
    pub tags: Vec<String>,
}

fn transport_for(url: &str) -> Result<GitTransport> {
    GitTransport::from_url(url)
        .ok_or_else(|| GitCompatError::RemoteError(format!("unsupported remote URL: {url}")))
}

/// Push refs of the Git repository at `git_path` to `url`.
pub fn push(git_path: &Path, url: &str, opts: &PushOptions, auth: &GitAuth) -> Result<PushReport> {
    let transport = transport_for(url)?;
    let git = Repository::open(git_path)?;

    let mut refs = Vec::new();
    if opts.branches.is_empty() {
        for item in git.branches(Some(BranchType::Local))? {
            let (branch, _) = item?;
            if let Some(name) = branch.get().name() {
                refs.push(name.to_owned());
            }
        }
    } else {
        for name in &opts.branches {
            let branch = git.find_branch(name, BranchType::Local).map_err(|e| {
                if e.code() == git2::ErrorCode::NotFound {
                    GitCompatError::RemoteError(format!("branch not found: {name}"))
                } else {
                    e.into()
                }
            })?;
            if let Some(refname) = branch.get().name() {
                refs.push(refname.to_owned());
            }
        }
    }
    if opts.include_tags {
        for tag in git.tag_names(None)?.iter().flatten() {
            refs.push(format!("refs/tags/{tag}"));
        }
    }
    refs.sort();
    refs.dedup();
    if refs.is_empty() {
        return Ok(PushReport::default());
    }

    let prefix = if opts.force { "+" } else { "" };
    let refspecs: Vec<String> = refs.iter().map(|r| format!("{prefix}{r}:{r}")).collect();
    let rejected = RefCell::new(Vec::new());
    let mut callbacks = auth.callbacks();
    callbacks.push_update_reference(|refname, status| {
        if let Some(reason) = status {
            rejected.borrow_mut().push(format!("{refname} ({reason})"));
        }
        Ok(())
    });
    let mut push_opts = git2::PushOptions::new();
    push_opts.remote_callbacks(callbacks);
    let mut remote = git.remote_anonymous(url)?;
    remote.push(&refspecs, Some(&mut push_opts))?;
    drop(push_opts);

    let rejected = rejected.into_inner();
    if !rejected.is_empty() {
        return Err(GitCompatError::RemoteError(format!(
            "push to {url} rejected: {}",
            rejected.join(", ")
        )));
    }
    info!(%transport, refs = refs.len(), "pushed to Git remote");
    Ok(PushReport { refs })
}

/// Mirror all branches and tags of `url` into the bare repository at
/// `git_path` (created if missing). Local refs are overwritten.
pub fn fetch_mirror(git_path: &Path, url: &str, auth: &GitAuth) -> Result<FetchReport> {
    let transport = transport_for(url)?;
    let git = match Repository::open(git_path) {
        Ok(git) => git,
        Err(e) if e.code() == git2::ErrorCode::NotFound => Repository::init_bare(git_path)?,
        Err(e) => return Err(e.into()),
    };
    if !git.is_bare() {
        return Err(GitCompatError::RemoteError(format!(
            "{} is not a bare repository; mirrors must be bare",
            git_path.display()
        )));
    }

    let mut remote = git.remote_anonymous(url)?;
    let default_branch = {
        let connection = remote.connect_auth(Direction::Fetch, Some(auth.callbacks()), None)?;
        connection
            .default_branch()
            .ok()
            .and_then(|buf| buf.as_str().map(str::to_owned))
    };

    let mut fetch_opts = FetchOptions::new();
    fetch_opts
        .remote_callbacks(auth.callbacks())
        .download_tags(AutotagOption::None);
    remote.fetch(
        &["+refs/heads/*:refs/heads/*", "+refs/tags/*:refs/tags/*"],
        Some(&mut fetch_opts),
        Some("wt git fetch"),
    )?;
    if let Some(head) = default_branch {
        git.set_head(&head)?;
    }

    let mut branches = Vec::new();
    for item in git.branches(Some(BranchType::Local))? {
        let (branch, _) = item?;
        if let Some(name) = branch.name()? {
            branches.push(name.to_owned());
        }
    }
    branches.sort();
    let mut tags: Vec<String> = git
        .tag_names(None)?
        .iter()
        .flatten()
        .map(str::to_owned)
        .collect();
    tags.sort();
    info!(%transport, branches = branches.len(), tags = tags.len(), "fetched Git mirror");
    Ok(FetchReport { branches, tags })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{GitFixture, Spec};

    fn url_of(path: &Path) -> String {
        path.to_string_lossy().into_owned()
    }

    fn source() -> (GitFixture, git2::Oid, git2::Oid) {
        let fx = GitFixture::new();
        let c1 = fx.commit(
            Some("refs/heads/main"),
            &[Spec::File("a", b"1")],
            &[],
            "c1",
            1,
        );
        let c2 = fx.commit(
            Some("refs/heads/dev"),
            &[Spec::File("a", b"2")],
            &[c1],
            "c2",
            2,
        );
        fx.tag_annotated("v1", c1, "one", 3);
        (fx, c1, c2)
    }

    #[test]
    fn push_all_branches_and_tags_to_bare_remote() {
        let (fx, c1, c2) = source();
        let remote = GitFixture::bare();
        let report = push(
            fx.path(),
            &url_of(remote.path()),
            &PushOptions {
                include_tags: true,
                ..PushOptions::default()
            },
            &GitAuth::auto(),
        )
        .unwrap();
        assert_eq!(
            report.refs,
            vec!["refs/heads/dev", "refs/heads/main", "refs/tags/v1"]
        );
        assert_eq!(remote.repo.refname_to_id("refs/heads/main").unwrap(), c1);
        assert_eq!(remote.repo.refname_to_id("refs/heads/dev").unwrap(), c2);
        let tag = remote
            .repo
            .find_reference("refs/tags/v1")
            .unwrap()
            .peel_to_tag()
            .unwrap();
        assert_eq!(tag.target_id(), c1);
    }

    #[test]
    fn push_selected_branch_only() {
        let (fx, c1, _) = source();
        let remote = GitFixture::bare();
        let report = push(
            fx.path(),
            &url_of(remote.path()),
            &PushOptions {
                branches: vec!["main".to_owned()],
                ..PushOptions::default()
            },
            &GitAuth::auto(),
        )
        .unwrap();
        assert_eq!(report.refs, vec!["refs/heads/main"]);
        assert_eq!(remote.repo.refname_to_id("refs/heads/main").unwrap(), c1);
        assert!(remote.repo.find_reference("refs/heads/dev").is_err());
        assert!(remote.repo.find_reference("refs/tags/v1").is_err());

        let err = push(
            fx.path(),
            &url_of(remote.path()),
            &PushOptions {
                branches: vec!["ghost".to_owned()],
                ..PushOptions::default()
            },
            &GitAuth::auto(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("branch not found: ghost"));
    }

    #[test]
    fn non_fast_forward_requires_force() {
        let (fx, c1, _) = source();
        let remote = GitFixture::bare();
        let url = url_of(remote.path());
        push(fx.path(), &url, &PushOptions::default(), &GitAuth::auto()).unwrap();

        // Rewrite main locally to an unrelated commit.
        let other = fx.commit(None, &[Spec::File("b", b"x")], &[], "unrelated", 5);
        fx.repo
            .reference("refs/heads/main", other, true, "rewrite")
            .unwrap();
        let only_main = PushOptions {
            branches: vec!["main".to_owned()],
            ..PushOptions::default()
        };
        assert!(push(fx.path(), &url, &only_main, &GitAuth::auto()).is_err());
        assert_eq!(remote.repo.refname_to_id("refs/heads/main").unwrap(), c1);

        let forced = PushOptions {
            force: true,
            ..only_main
        };
        push(fx.path(), &url, &forced, &GitAuth::auto()).unwrap();
        assert_eq!(remote.repo.refname_to_id("refs/heads/main").unwrap(), other);
    }

    #[test]
    fn fetch_mirror_creates_bare_copy() {
        let (fx, c1, c2) = source();
        let dir = tempfile::tempdir().unwrap();
        let mirror = dir.path().join("mirror.git");
        let report = fetch_mirror(&mirror, &url_of(fx.path()), &GitAuth::auto()).unwrap();
        assert_eq!(report.branches, vec!["dev", "main"]);
        assert_eq!(report.tags, vec!["v1"]);

        let git = Repository::open(&mirror).unwrap();
        assert!(git.is_bare());
        assert_eq!(git.refname_to_id("refs/heads/main").unwrap(), c1);
        assert_eq!(git.refname_to_id("refs/heads/dev").unwrap(), c2);
        assert_eq!(
            git.find_reference("HEAD").unwrap().symbolic_target(),
            Some("refs/heads/main")
        );

        // Re-fetch picks up moved refs.
        let c3 = fx.commit(
            Some("refs/heads/main"),
            &[Spec::File("a", b"3")],
            &[c1],
            "c3",
            4,
        );
        fetch_mirror(&mirror, &url_of(fx.path()), &GitAuth::auto()).unwrap();
        let git = Repository::open(&mirror).unwrap();
        assert_eq!(git.refname_to_id("refs/heads/main").unwrap(), c3);
    }

    #[test]
    fn fetch_mirror_refuses_non_bare_target() {
        let (fx, _, _) = source();
        let target = GitFixture::new();
        let err = fetch_mirror(target.path(), &url_of(fx.path()), &GitAuth::auto()).unwrap_err();
        assert!(err.to_string().contains("not a bare repository"));
    }

    #[test]
    fn unsupported_urls_are_rejected() {
        let (fx, _, _) = source();
        let err = push(
            fx.path(),
            "ftp://example.com/r",
            &PushOptions::default(),
            &GitAuth::auto(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("unsupported remote URL"));
        let dir = tempfile::tempdir().unwrap();
        assert!(fetch_mirror(&dir.path().join("m"), "ftp://x/y", &GitAuth::auto()).is_err());
    }
}
