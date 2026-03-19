use std::path::PathBuf;

use crate::error::Result;
use crate::import::repo::GitRepo;

/// Information about a Git submodule discovered during import.
#[derive(Debug, Clone)]
pub struct SubmoduleInfo {
    /// The name of the submodule as declared in `.gitmodules`.
    pub name: String,
    /// The filesystem path of the submodule relative to the repository root.
    pub path: PathBuf,
    /// The remote URL the submodule points to.
    pub url: String,
}

/// Handles discovery and import of Git submodules, converting them into
/// Worktree nested tree references.
pub struct SubmoduleImporter;

impl SubmoduleImporter {
    /// Scan the given repository for submodules and return information about each one.
    ///
    /// This reads the `.gitmodules` file (via libgit2) and collects the name,
    /// relative path, and remote URL for every registered submodule.
    pub fn import_submodules(repo: &GitRepo) -> Result<Vec<SubmoduleInfo>> {
        let raw = repo.inner();
        let submodules = raw.submodules().map_err(|e| {
            crate::error::GitCompatError::ImportError(format!(
                "failed to enumerate submodules: {}",
                e
            ))
        })?;

        let mut infos = Vec::with_capacity(submodules.len());
        for sm in &submodules {
            let name = sm.name().unwrap_or("").to_string();
            let path = sm.path().to_path_buf();
            let url = sm.url().unwrap_or("").to_string();
            infos.push(SubmoduleInfo { name, path, url });
        }

        Ok(infos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submodule_info_fields() {
        let info = SubmoduleInfo {
            name: "vendor/lib".to_string(),
            path: PathBuf::from("vendor/lib"),
            url: "https://github.com/example/lib.git".to_string(),
        };
        assert_eq!(info.name, "vendor/lib");
        assert_eq!(info.path, PathBuf::from("vendor/lib"));
        assert_eq!(info.url, "https://github.com/example/lib.git");
    }
}
