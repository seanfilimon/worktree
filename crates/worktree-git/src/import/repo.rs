use std::path::Path;

use git2::Repository;

use crate::error::Result;

/// A thin wrapper around [`git2::Repository`] that provides
/// Worktree-oriented convenience methods for inspecting a Git repository.
pub struct GitRepo {
    /// The underlying libgit2 repository handle.
    repo: Repository,
}

impl GitRepo {
    /// Open an existing Git repository at the given path.
    ///
    /// The path may point to either a bare repository or a working directory
    /// that contains a `.git` folder.
    pub fn open(path: &Path) -> Result<Self> {
        let repo = Repository::open(path)?;
        Ok(Self { repo })
    }

    /// Return a reference to the underlying [`git2::Repository`].
    pub fn inner(&self) -> &Repository {
        &self.repo
    }

    /// List the names of all local branches in the repository.
    pub fn branches(&self) -> Result<Vec<String>> {
        let mut names = Vec::new();
        let branches = self.repo.branches(Some(git2::BranchType::Local))?;
        for branch_result in branches {
            let (branch, _branch_type) = branch_result?;
            if let Some(name) = branch.name()? {
                names.push(name.to_owned());
            }
        }
        Ok(names)
    }

    /// Return the name of the branch that HEAD currently points to.
    ///
    /// Returns an error if HEAD is detached or unborn.
    pub fn head_branch(&self) -> Result<String> {
        let head = self.repo.head()?;
        let shorthand = head
            .shorthand()
            .ok_or_else(|| crate::error::GitCompatError::ImportError(
                "HEAD is not a valid UTF-8 reference".to_string(),
            ))?;
        Ok(shorthand.to_owned())
    }

    /// Count the total number of commits reachable from HEAD.
    ///
    /// Walks the entire commit graph starting from HEAD in topological order.
    pub fn commit_count(&self) -> Result<usize> {
        let head = self.repo.head()?;
        let head_oid = head.target().ok_or_else(|| {
            crate::error::GitCompatError::ImportError(
                "HEAD does not point to a valid object".to_string(),
            )
        })?;

        let mut revwalk = self.repo.revwalk()?;
        revwalk.set_sorting(git2::Sort::TOPOLOGICAL)?;
        revwalk.push(head_oid)?;

        let mut count: usize = 0;
        for oid_result in revwalk {
            let _oid = oid_result?;
            count += 1;
        }
        Ok(count)
    }
}
