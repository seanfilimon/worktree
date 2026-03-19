use git2::Oid;

use super::repo::GitRepo;
use crate::error::Result;

/// Walks the commit graph of a Git repository in topological order.
///
/// `CommitWalker` wraps a `git2::Revwalk` and yields commit OIDs starting
/// from the repository HEAD, traversing parents before children (topological
/// ordering). This is the natural order for importing a Git history into
/// Worktree's snapshot model.
pub struct CommitWalker {
    /// Collected commit OIDs in topological order.
    oids: Vec<Oid>,
    /// Current position in the `oids` vector.
    index: usize,
}

impl CommitWalker {
    /// Create a new `CommitWalker` that traverses all commits reachable from HEAD.
    ///
    /// The walker performs a topological sort so that parent commits are always
    /// yielded before their children, which is required for correct import
    /// ordering (a snapshot's parents must already exist before the snapshot
    /// itself is created).
    pub fn new(repo: &GitRepo) -> Result<Self> {
        let inner = repo.inner();
        let mut revwalk = inner.revwalk()?;

        // Sort topologically (parents before children) and break ties by time.
        revwalk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::TIME)?;

        // Start from HEAD.
        revwalk.push_head()?;

        let mut oids = Vec::new();
        for oid_result in revwalk {
            let oid = oid_result?;
            oids.push(oid);
        }

        // Reverse so that the oldest ancestors come first (parents before children).
        oids.reverse();

        Ok(Self { oids, index: 0 })
    }

    /// Create a `CommitWalker` starting from a specific branch reference.
    ///
    /// This is useful when importing only a single branch rather than the
    /// entire history reachable from HEAD.
    pub fn from_branch(repo: &GitRepo, branch_name: &str) -> Result<Self> {
        let inner = repo.inner();
        let mut revwalk = inner.revwalk()?;

        revwalk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::TIME)?;

        let reference = inner.find_branch(branch_name, git2::BranchType::Local)?;
        let target = reference
            .get()
            .target()
            .ok_or_else(|| {
                crate::error::GitCompatError::ImportError(format!(
                    "branch '{}' does not point to a valid commit",
                    branch_name
                ))
            })?;

        revwalk.push(target)?;

        let mut oids = Vec::new();
        for oid_result in revwalk {
            let oid = oid_result?;
            oids.push(oid);
        }

        // Reverse so that the oldest ancestors come first.
        oids.reverse();

        Ok(Self { oids, index: 0 })
    }

    /// Returns the total number of commits discovered during the walk.
    pub fn total_commits(&self) -> usize {
        self.oids.len()
    }

    /// Returns the number of commits remaining to be yielded.
    pub fn remaining(&self) -> usize {
        self.oids.len().saturating_sub(self.index)
    }

    /// Reset the walker back to the beginning so it can be iterated again.
    pub fn reset(&mut self) {
        self.index = 0;
    }
}

impl Iterator for CommitWalker {
    type Item = Oid;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.oids.len() {
            let oid = self.oids[self.index];
            self.index += 1;
            Some(oid)
        } else {
            None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.remaining();
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for CommitWalker {}
