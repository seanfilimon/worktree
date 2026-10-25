//! `wt license detect [PATH] [--apply]`: SPDX guesses for the tree's files
//! (`worktree_sdk::scan::detect_license`) and the license rules they
//! suggest (ENGINEERING_PLAN §13.8).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use colored::Colorize;
use serde::{Deserialize, Serialize};
use serde_json::json;
use worktree_sdk::{scan, Repository, TreeInfo};

use super::Grant;
use crate::commands::server_common::{confirm, decode, default_tree, Api, Cell, Table};
use crate::context::Context;
use crate::error::{CliError, Result};

/// Files larger than this are not scanned (license texts are small).
const MAX_SCAN_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Detection {
    /// Tree-relative path.
    pub path: String,
    pub spdx: String,
    /// "license_file" or "header".
    pub kind: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Proposal {
    pub path_glob: String,
    pub spdx: String,
}

#[derive(Serialize)]
struct DetectResult<'a> {
    tree: &'a str,
    detections: &'a [Detection],
    proposals: &'a [Proposal],
    /// Rules created on the server (`--apply`).
    applied: &'a [Proposal],
}

pub fn run(
    ctx: &Context,
    path: Option<PathBuf>,
    apply: bool,
    grant: Grant,
    yes: bool,
) -> Result<()> {
    let (repo, tree) = ctx.open_tree()?;
    let scope = match &path {
        Some(p) => ctx
            .tree_paths(&repo, &tree, std::slice::from_ref(p))?
            .into_iter()
            .next()
            .unwrap_or_default(),
        None => String::new(),
    };
    let detections = scan_tree(&repo, &tree, &scope)?;
    let proposals = propose(&detections);

    let mut applied = Vec::new();
    if apply && !proposals.is_empty() {
        let tree_ref = default_tree(&repo, &tree)?;
        let api = Api::connect(ctx)?;
        let path = format!("{}/licenses", tree_ref.api());
        let existing: Option<Vec<Proposal>> = decode(&api.get(&path)?)?;
        let existing: BTreeSet<(String, String)> = existing
            .unwrap_or_default()
            .into_iter()
            .map(|r| (r.path_glob, r.spdx))
            .collect();
        let new: Vec<&Proposal> = proposals
            .iter()
            .filter(|p| !existing.contains(&(p.path_glob.clone(), p.spdx.clone())))
            .collect();
        if !new.is_empty() {
            confirm(
                yes,
                &format!("create {} license rule(s) on {tree_ref}", new.len()),
            )?;
            for p in new {
                api.put(
                    &path,
                    &json!({ "path_glob": p.path_glob, "spdx": p.spdx, "grant": grant }),
                )?;
                applied.push(p.clone());
            }
        }
    }

    let result = DetectResult {
        tree: &tree.name,
        detections: &detections,
        proposals: &proposals,
        applied: &applied,
    };
    ctx.out.emit(&result, |t| {
        let mut table = Table::new(&["PATH", "SPDX", "SOURCE"]);
        for d in &detections {
            table.row([
                Cell::from(d.path.as_str()).bold(),
                Cell::from(d.spdx.as_str()).green(),
                d.kind.replace('_', " ").into(),
            ]);
        }
        table.render(t, "No licenses detected.");
        if !proposals.is_empty() {
            t.blank();
            t.line("Proposed rules:");
            for p in &proposals {
                t.line(format_args!("  {}  {}", p.path_glob.bold(), p.spdx.green()));
            }
            if apply {
                t.line(format_args!(
                    "Created {} rule(s); {} already existed.",
                    applied.len(),
                    proposals.len() - applied.len()
                ));
            } else {
                t.line("Run `wt license detect --apply` to create them on the server.");
            }
        }
    })
}

/// Detect licenses of the tree's working files under `scope` (a
/// tree-relative file or directory, "" = everything), skipping ignored
/// paths, nested trees and metadata directories.
pub fn scan_tree(repo: &Repository, tree: &TreeInfo, scope: &str) -> Result<Vec<Detection>> {
    let base = repo.tree_dir(&tree.name)?;
    let root_prefix = tree.path.to_string_lossy().replace('\\', "/");
    let start = if scope.is_empty() {
        base.clone()
    } else {
        base.join(scope)
    };
    let mut out = Vec::new();
    let mut visit = |rel: &str, abs: &Path| -> Result<()> {
        let meta = fs::metadata(abs)
            .map_err(|e| CliError::failure(format!("cannot read '{}': {e}", abs.display())))?;
        if meta.len() > MAX_SCAN_BYTES {
            return Ok(());
        }
        let data = fs::read(abs)
            .map_err(|e| CliError::failure(format!("cannot read '{}': {e}", abs.display())))?;
        if let Some(spdx) = scan::detect_license(&data) {
            let kind = if is_license_file(rel) {
                "license_file"
            } else {
                "header"
            };
            out.push(Detection {
                path: rel.to_owned(),
                spdx,
                kind,
            });
        }
        Ok(())
    };
    if start.is_file() {
        visit(scope, &start)?;
    } else if start.is_dir() {
        walk(repo, &root_prefix, &base, &start, &mut visit)?;
    } else {
        return Err(CliError::failure(format!(
            "'{scope}' does not exist in tree '{}'",
            tree.name
        )));
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

fn walk(
    repo: &Repository,
    root_prefix: &str,
    base: &Path,
    dir: &Path,
    visit: &mut dyn FnMut(&str, &Path) -> Result<()>,
) -> Result<()> {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .map_err(|e| CliError::failure(format!("cannot read '{}': {e}", dir.display())))?
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let abs = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if matches!(name.as_str(), ".wt" | ".wt-tree" | ".git") {
            continue;
        }
        let Ok(rel_path) = abs.strip_prefix(base) else {
            continue;
        };
        let rel = rel_path.to_string_lossy().replace('\\', "/");
        let root_rel = if root_prefix.is_empty() {
            rel.clone()
        } else {
            format!("{root_prefix}/{rel}")
        };
        if repo.is_ignored(&root_rel)? {
            continue;
        }
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            // Another tree's directory.
            if abs.join(".wt-tree").is_dir() {
                continue;
            }
            walk(repo, root_prefix, base, &abs, visit)?;
        } else if kind.is_file() {
            visit(&rel, &abs)?;
        }
    }
    Ok(())
}

/// LICENSE, LICENCE, COPYING (any case, any suffix) and UNLICENSE files,
/// except the license texts of a REUSE `LICENSES/` directory.
fn is_license_file(path: &str) -> bool {
    let (dir, name) = path.rsplit_once('/').unwrap_or(("", path));
    if dir == "LICENSES" || dir.ends_with("/LICENSES") {
        return false;
    }
    let upper = name.to_ascii_uppercase();
    ["LICENSE", "LICENCE", "COPYING", "UNLICENSE"]
        .iter()
        .any(|p| upper.starts_with(p))
}

fn dir_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

fn dir_glob(dir: &str) -> String {
    if dir.is_empty() {
        "**".into()
    } else {
        format!("{dir}/**")
    }
}

/// Rules suggested by `detections` (sorted by glob).
pub fn propose(detections: &[Detection]) -> Vec<Proposal> {
    // Directory licenses from license files.
    let mut dirs: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for d in detections.iter().filter(|d| d.kind == "license_file") {
        dirs.entry(dir_of(&d.path)).or_default().insert(&d.spdx);
    }
    let dir_spdx: BTreeMap<&str, String> = dirs
        .iter()
        .map(|(dir, ids)| {
            let ids: Vec<&str> = ids.iter().copied().collect();
            let spdx = if ids.len() == 1 {
                ids[0].to_owned()
            } else {
                ids.iter()
                    .map(|id| {
                        if id.contains(' ') {
                            format!("({id})")
                        } else {
                            (*id).to_owned()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" OR ")
            };
            (*dir, spdx)
        })
        .collect();
    let mut out: Vec<Proposal> = dir_spdx
        .iter()
        .map(|(dir, spdx)| Proposal {
            path_glob: dir_glob(dir),
            spdx: spdx.clone(),
        })
        .collect();

    let headers: Vec<&Detection> = detections.iter().filter(|d| d.kind == "header").collect();
    let distinct: BTreeSet<&str> = headers.iter().map(|d| d.spdx.as_str()).collect();
    if dir_spdx.is_empty() && distinct.len() == 1 && headers.len() > 1 {
        // Consistent headers and no license file: one rule for the tree.
        out.push(Proposal {
            path_glob: "**".into(),
            spdx: headers[0].spdx.clone(),
        });
    } else {
        for d in headers {
            if effective(&dir_spdx, &d.path) != Some(d.spdx.as_str()) {
                out.push(Proposal {
                    path_glob: d.path.clone(),
                    spdx: d.spdx.clone(),
                });
            }
        }
    }
    out.sort_by(|a, b| a.path_glob.cmp(&b.path_glob));
    out
}

/// License of the nearest directory (of `path`) holding a license file.
fn effective<'a>(dirs: &'a BTreeMap<&str, String>, path: &str) -> Option<&'a str> {
    let mut dir = dir_of(path);
    loop {
        if let Some(spdx) = dirs.get(dir) {
            return Some(spdx);
        }
        if dir.is_empty() {
            return None;
        }
        dir = dir_of(dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn det(path: &str, spdx: &str) -> Detection {
        Detection {
            path: path.into(),
            spdx: spdx.into(),
            kind: if is_license_file(path) {
                "license_file"
            } else {
                "header"
            },
        }
    }

    fn globs(p: &[Proposal]) -> Vec<(&str, &str)> {
        p.iter()
            .map(|p| (p.path_glob.as_str(), p.spdx.as_str()))
            .collect()
    }

    #[test]
    fn license_files() {
        assert!(is_license_file("LICENSE"));
        assert!(is_license_file("vendor/x/License.txt"));
        assert!(is_license_file("LICENSE-MIT"));
        assert!(is_license_file("COPYING"));
        assert!(!is_license_file("LICENSES/MIT.txt"));
        assert!(!is_license_file("src/license.rs.bak/x"));
        assert!(!is_license_file("README.md"));
    }

    #[test]
    fn proposals_from_files_and_headers() {
        let d = vec![
            det("LICENSE-APACHE", "Apache-2.0"),
            det("LICENSE-MIT", "MIT"),
            det("src/main.rs", "MIT OR Apache-2.0"),
            det("src/gpl.c", "GPL-2.0-only"),
            det("vendor/lib/LICENSE", "BSD-3-Clause"),
            det("vendor/lib/a.c", "BSD-3-Clause"),
        ];
        assert_eq!(
            globs(&propose(&d)),
            vec![
                ("**", "Apache-2.0 OR MIT"),
                ("src/gpl.c", "GPL-2.0-only"),
                ("src/main.rs", "MIT OR Apache-2.0"),
                ("vendor/lib/**", "BSD-3-Clause"),
            ]
        );
    }

    #[test]
    fn consistent_headers_without_license_file() {
        let d = vec![det("a.rs", "MIT"), det("b/c.rs", "MIT")];
        assert_eq!(globs(&propose(&d)), vec![("**", "MIT")]);
        let d = vec![det("a.rs", "MIT"), det("b/c.rs", "Apache-2.0")];
        assert_eq!(
            globs(&propose(&d)),
            vec![("a.rs", "MIT"), ("b/c.rs", "Apache-2.0")]
        );
        assert!(propose(&[]).is_empty());
    }
}
