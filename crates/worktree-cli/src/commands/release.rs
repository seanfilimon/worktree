//! `wt release` — releases of a server tree: list, create, show, edit,
//! delete, and release assets (upload, download) (ENGINEERING_PLAN §13.5).

use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};
use colored::Colorize;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use worktree_sdk::remote::rest::seg;

use crate::commands::server_common::{
    bytes, confirm, decode, fields, nullable, or_dash, render, tree_or_default, when, yes_no, Api,
    Cell, Table, TreeRef,
};
use crate::context::Context;
use crate::error::{CliError, Result};
use crate::output::format::short;
use crate::output::Text;

#[derive(Debug, Args)]
pub struct ReleaseArgs {
    #[command(subcommand)]
    pub command: ReleaseCommand,
}

#[derive(Debug, Subcommand)]
pub enum ReleaseCommand {
    /// List releases (newest first; drafts only if you may create releases)
    List {
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: Option<TreeRef>,
    },
    /// Create a release (the tag is created at --snapshot when it does not exist)
    Create {
        tag: String,
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: Option<TreeRef>,
        /// Display name (default: the tag)
        #[arg(long, value_name = "NAME")]
        name: Option<String>,
        /// Release notes
        #[arg(long, value_name = "TEXT", conflicts_with = "notes_file")]
        notes: Option<String>,
        /// Read the release notes from a file
        #[arg(long, value_name = "FILE")]
        notes_file: Option<PathBuf>,
        /// Create it as an unpublished draft
        #[arg(long)]
        draft: bool,
        /// Mark it as a pre-release
        #[arg(long)]
        prerelease: bool,
        /// Snapshot (id, branch or tag) to tag when TAG does not exist yet
        #[arg(long, value_name = "REV")]
        snapshot: Option<String>,
    },
    /// Show a release and its assets
    Show {
        tag: String,
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: Option<TreeRef>,
    },
    /// Edit a release
    Edit {
        tag: String,
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: Option<TreeRef>,
        #[arg(long, value_name = "NAME")]
        name: Option<String>,
        #[arg(long, value_name = "TEXT", conflicts_with = "notes_file")]
        notes: Option<String>,
        #[arg(long, value_name = "FILE")]
        notes_file: Option<PathBuf>,
        /// Turn it back into a draft
        #[arg(long, conflicts_with = "publish")]
        draft: bool,
        /// Publish a draft
        #[arg(long)]
        publish: bool,
        /// Mark it as a pre-release
        #[arg(long, conflicts_with = "no_prerelease")]
        prerelease: bool,
        /// Mark it as a regular release
        #[arg(long)]
        no_prerelease: bool,
    },
    /// Delete a release (its tag is kept)
    Delete {
        tag: String,
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: Option<TreeRef>,
        /// Do not ask for confirmation
        #[arg(long, short = 'y')]
        yes: bool,
    },
    /// Upload files as release assets (replacing assets of the same name)
    Upload {
        tag: String,
        #[arg(required = true, value_name = "FILE")]
        files: Vec<PathBuf>,
        /// Server tree (default: the tree of the current worktree)
        #[arg(id = "tree_ref", long = "in", value_name = "TENANT/TREE")]
        tree: Option<TreeRef>,
    },
    /// Download release assets (all of them without ASSET)
    Download {
        tag: String,
        asset: Option<String>,
        /// Directory to write to
        #[arg(short, long, value_name = "DIR", default_value = ".")]
        output: PathBuf,
        /// Server tree (default: the tree of the current worktree)
        #[arg(id = "tree_ref", long = "in", value_name = "TENANT/TREE")]
        tree: Option<TreeRef>,
    },
}

/// `domain.Release`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Release {
    tag: String,
    snapshot_id: String,
    name: String,
    notes: String,
    draft: bool,
    prerelease: bool,
    #[serde(deserialize_with = "nullable")]
    assets: Vec<Asset>,
    author: String,
    created_at: String,
    published_at: Option<String>,
}

/// `domain.ReleaseAsset`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Asset {
    name: String,
    size: u64,
    content_type: String,
    uploaded_at: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ReleaseList {
    #[serde(deserialize_with = "nullable")]
    releases: Vec<Release>,
}

pub fn run(ctx: &Context, args: ReleaseArgs) -> Result<()> {
    let api = Api::connect(ctx)?;
    let out = &ctx.out;
    match args.command {
        ReleaseCommand::List { tree } => {
            let tree = tree_or_default(ctx, tree)?;
            let value = api.get(&releases_path(&tree))?;
            render(out, &value, |t, list: ReleaseList| {
                list_releases(t, &list.releases)
            })
        }
        ReleaseCommand::Create {
            tag,
            tree,
            name,
            notes,
            notes_file,
            draft,
            prerelease,
            snapshot,
        } => {
            let tree = tree_or_default(ctx, tree)?;
            let notes = read_notes(ctx, notes, notes_file)?;
            let body = json!({
                "tag": tag,
                "name": name.unwrap_or_default(),
                "notes": notes.unwrap_or_default(),
                "draft": draft,
                "prerelease": prerelease,
                "snapshot": snapshot.unwrap_or_default(),
            });
            let value = api.post(&releases_path(&tree), &body)?;
            render(out, &value, |t, r: Release| {
                t.line(format_args!(
                    "Created {}release {} at {} on {tree}",
                    if r.draft { "draft " } else { "" },
                    r.tag.green().bold(),
                    short(&r.snapshot_id).yellow()
                ));
            })
        }
        ReleaseCommand::Show { tag, tree } => {
            let tree = tree_or_default(ctx, tree)?;
            let value = api.get(&release_path(&tree, &tag))?;
            render(out, &value, |t, r: Release| show(t, &r))
        }
        ReleaseCommand::Edit {
            tag,
            tree,
            name,
            notes,
            notes_file,
            draft,
            publish,
            prerelease,
            no_prerelease,
        } => {
            let tree = tree_or_default(ctx, tree)?;
            let notes = read_notes(ctx, notes, notes_file)?;
            let body = edit_body(name, notes, draft, publish, prerelease, no_prerelease);
            if body.is_empty() {
                return Err(CliError::usage("nothing to change").with_hint(
                    "pass --name, --notes, --notes-file, --draft, --publish, --prerelease or --no-prerelease",
                ));
            }
            let value = api.patch(&release_path(&tree, &tag), &Value::Object(body))?;
            render(out, &value, |t, r: Release| {
                t.line(format_args!("Updated release {}", r.tag.bold()));
            })
        }
        ReleaseCommand::Delete { tag, tree, yes } => {
            let tree = tree_or_default(ctx, tree)?;
            confirm(yes, &format!("delete release {tag} of {tree}"))?;
            api.delete(&release_path(&tree, &tag), &[])?;
            out.emit(&json!({ "deleted": tag }), |t| {
                t.line(format_args!(
                    "Deleted release {} (the tag is kept)",
                    tag.bold()
                ));
            })
        }
        ReleaseCommand::Upload { tag, files, tree } => {
            let tree = tree_or_default(ctx, tree)?;
            let mut uploaded = Vec::new();
            for file in &files {
                let path = ctx.absolute(file);
                let name = file_name(&path)?;
                let data = std::fs::read(&path).map_err(|e| {
                    CliError::failure(format!("cannot read '{}': {e}", file.display()))
                })?;
                let value: Value = api.block_on(api.client().put_bytes(
                    &asset_path(&tree, &tag, &name),
                    content_type(&name),
                    data,
                ))?;
                uploaded.push(value);
            }
            let assets: Vec<Asset> = decode(&Value::Array(uploaded.clone()))?;
            out.emit(&uploaded, |t| {
                for a in &assets {
                    t.line(format_args!(
                        "Uploaded {} ({}) to release {}",
                        a.name.green(),
                        bytes(a.size),
                        tag.bold()
                    ));
                }
            })
        }
        ReleaseCommand::Download {
            tag,
            asset,
            output,
            tree,
        } => {
            let tree = tree_or_default(ctx, tree)?;
            let names = match asset {
                Some(name) => vec![name],
                None => {
                    let r: Release = decode(&api.get(&release_path(&tree, &tag))?)?;
                    r.assets.into_iter().map(|a| a.name).collect()
                }
            };
            let dir = ctx.absolute(&output);
            std::fs::create_dir_all(&dir).map_err(|e| {
                CliError::failure(format!("cannot create '{}': {e}", dir.display()))
            })?;
            let mut done = Vec::new();
            for name in &names {
                let local = file_name(Path::new(name))?;
                let data = api.block_on(api.client().get_bytes(&asset_path(&tree, &tag, name)))?;
                let dest = dir.join(&local);
                std::fs::write(&dest, &data).map_err(|e| {
                    CliError::failure(format!("cannot write '{}': {e}", dest.display()))
                })?;
                done.push((name.clone(), dest, data.len()));
            }
            let report: Vec<Value> = done
                .iter()
                .map(|(n, p, s)| json!({ "name": n, "path": p.display().to_string(), "size": s }))
                .collect();
            out.emit(&report, |t| {
                if done.is_empty() {
                    t.line(format!("Release {tag} has no assets.").dimmed());
                }
                for (_, path, size) in &done {
                    t.line(format_args!(
                        "Downloaded {} ({})",
                        path.display().to_string().green(),
                        bytes(*size as u64)
                    ));
                }
            })
        }
    }
}

fn releases_path(tree: &TreeRef) -> String {
    format!("{}/releases", tree.api())
}

/// Tags may contain '/', which [`seg`] encodes as %2F.
fn release_path(tree: &TreeRef, tag: &str) -> String {
    format!("{}/{}", releases_path(tree), seg(tag))
}

fn asset_path(tree: &TreeRef, tag: &str, name: &str) -> String {
    format!("{}/assets/{}", release_path(tree, tag), seg(name))
}

/// The final component of `path` (an asset name never contains separators).
fn file_name(path: &Path) -> Result<String> {
    path.file_name()
        .and_then(|n| n.to_str())
        .filter(|n| !n.is_empty() && *n != "." && *n != "..")
        .map(str::to_string)
        .ok_or_else(|| CliError::usage(format!("'{}' has no usable file name", path.display())))
}

fn read_notes(
    ctx: &Context,
    notes: Option<String>,
    file: Option<PathBuf>,
) -> Result<Option<String>> {
    match file {
        Some(file) => {
            let path = ctx.absolute(&file);
            std::fs::read_to_string(&path)
                .map(Some)
                .map_err(|e| CliError::failure(format!("cannot read '{}': {e}", file.display())))
        }
        None => Ok(notes),
    }
}

/// The PATCH body of `wt release edit` (only the given fields).
fn edit_body(
    name: Option<String>,
    notes: Option<String>,
    draft: bool,
    publish: bool,
    prerelease: bool,
    no_prerelease: bool,
) -> Map<String, Value> {
    let mut body = Map::new();
    if let Some(name) = name {
        body.insert("name".into(), name.into());
    }
    if let Some(notes) = notes {
        body.insert("notes".into(), notes.into());
    }
    if draft || publish {
        body.insert("draft".into(), draft.into());
    }
    if prerelease || no_prerelease {
        body.insert("prerelease".into(), prerelease.into());
    }
    body
}

/// Media type of an asset from its file extension.
fn content_type(name: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    let ext = lower.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    if lower.ends_with(".tar.gz") || ext == "tgz" || ext == "gz" {
        return "application/gzip";
    }
    match ext {
        "zip" => "application/zip",
        "tar" => "application/x-tar",
        "json" => "application/json",
        "txt" | "md" | "sha256" | "asc" | "sig" => "text/plain; charset=utf-8",
        "html" => "text/html; charset=utf-8",
        "pdf" => "application/pdf",
        "png" => "image/png",
        "svg" => "image/svg+xml",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
}

fn kind(r: &Release) -> Cell {
    if r.draft {
        Cell::from("draft").yellow()
    } else if r.prerelease {
        Cell::from("pre-release").cyan()
    } else {
        Cell::from("release").green()
    }
}

fn list_releases(t: &mut Text, releases: &[Release]) {
    let mut table = Table::new(&[
        "TAG", "NAME", "KIND", "SNAPSHOT", "ASSETS", "AUTHOR", "CREATED",
    ]);
    for r in releases {
        table.row([
            Cell::from(r.tag.as_str()).bold(),
            r.name.as_str().into(),
            kind(r),
            Cell::from(short(&r.snapshot_id)).yellow(),
            r.assets.len().to_string().into(),
            or_dash(&r.author).into(),
            when(&r.created_at).into(),
        ]);
    }
    table.render(t, "No releases.");
}

fn show(t: &mut Text, r: &Release) {
    t.line(format_args!(
        "{} {}  [{}]",
        r.tag.bold(),
        r.name,
        kind(r).colored()
    ));
    fields(
        t,
        &[
            ("snapshot", short(&r.snapshot_id).to_string()),
            ("author", or_dash(&r.author)),
            ("created", when(&r.created_at)),
            (
                "published",
                r.published_at
                    .as_deref()
                    .map_or_else(|| "-".to_string(), when),
            ),
            ("pre-release", yes_no(r.prerelease).to_string()),
        ],
    );
    if !r.notes.is_empty() {
        t.blank();
        for line in r.notes.lines() {
            t.line(format_args!("    {line}"));
        }
    }
    t.blank();
    let mut table = Table::new(&["ASSET", "SIZE", "TYPE", "UPLOADED"]);
    for a in &r.assets {
        table.row([
            Cell::from(a.name.as_str()).bold(),
            bytes(a.size).into(),
            a.content_type.as_str().into(),
            when(&a.uploaded_at).into(),
        ]);
    }
    table.render(t, "No assets.");
}

#[cfg(test)]
mod tests {
    use clap::error::ErrorKind;

    use super::*;
    use crate::cli::Command;
    use crate::commands::server_common::testing::{parse, parse_err};

    fn release(args: &[&str]) -> ReleaseCommand {
        let mut full = vec!["release"];
        full.extend_from_slice(args);
        match parse(&full) {
            Command::Release(a) => a.command,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn parses_every_subcommand() {
        assert!(matches!(
            release(&["list"]),
            ReleaseCommand::List { tree: None }
        ));
        let ReleaseCommand::Create {
            tag,
            tree,
            name,
            draft,
            prerelease,
            snapshot,
            ..
        } = release(&[
            "create",
            "v1.0",
            "acme/web",
            "--name",
            "One",
            "--draft",
            "--prerelease",
            "--snapshot",
            "main",
        ])
        else {
            panic!()
        };
        assert_eq!(tag, "v1.0");
        assert!(tree.is_some() && draft && prerelease);
        assert_eq!(
            (name.as_deref(), snapshot.as_deref()),
            (Some("One"), Some("main"))
        );
        assert_eq!(
            parse_err(&[
                "release",
                "create",
                "v1",
                "--notes",
                "x",
                "--notes-file",
                "f"
            ]),
            ErrorKind::ArgumentConflict
        );
        assert!(matches!(
            release(&["show", "v1"]),
            ReleaseCommand::Show { .. }
        ));
        assert!(matches!(
            release(&["edit", "v1", "--publish", "--no-prerelease"]),
            ReleaseCommand::Edit {
                publish: true,
                no_prerelease: true,
                ..
            }
        ));
        assert_eq!(
            parse_err(&["release", "edit", "v1", "--draft", "--publish"]),
            ErrorKind::ArgumentConflict
        );
        assert!(matches!(
            release(&["delete", "v1", "-y"]),
            ReleaseCommand::Delete { yes: true, .. }
        ));
        let ReleaseCommand::Upload { files, tree, .. } =
            release(&["upload", "v1", "a.zip", "b.txt", "--in", "acme/web"])
        else {
            panic!()
        };
        assert_eq!(files.len(), 2);
        assert_eq!(tree.map(|t| t.to_string()).as_deref(), Some("acme/web"));
        assert_eq!(
            parse_err(&["release", "upload", "v1"]),
            ErrorKind::MissingRequiredArgument
        );
        let ReleaseCommand::Download { asset, output, .. } =
            release(&["download", "v1", "a.zip", "-o", "dist"])
        else {
            panic!()
        };
        assert_eq!(asset.as_deref(), Some("a.zip"));
        assert_eq!(output, PathBuf::from("dist"));
        assert!(matches!(
            release(&["download", "v1"]),
            ReleaseCommand::Download { asset: None, .. }
        ));
    }

    #[test]
    fn paths_and_helpers() {
        let tree: TreeRef = "acme/web".parse().expect("tree");
        assert_eq!(
            release_path(&tree, "release/1.0"),
            "/api/v1/trees/acme/web/releases/release%2F1.0"
        );
        assert_eq!(
            asset_path(&tree, "v1", "my app.zip"),
            "/api/v1/trees/acme/web/releases/v1/assets/my%20app.zip"
        );
        assert_eq!(content_type("x.tar.gz"), "application/gzip");
        assert_eq!(content_type("X.ZIP"), "application/zip");
        assert_eq!(content_type("SHA256SUMS.txt"), "text/plain; charset=utf-8");
        assert_eq!(content_type("binary"), "application/octet-stream");
        assert_eq!(file_name(Path::new("dir/a.zip")).expect("name"), "a.zip");
        assert!(file_name(Path::new("..")).is_err());
        let body = edit_body(Some("N".into()), None, false, true, false, true);
        assert_eq!(
            Value::Object(body),
            json!({"name": "N", "draft": false, "prerelease": false})
        );
        assert!(edit_body(None, None, false, false, false, false).is_empty());
    }

    #[test]
    fn renders_list_and_detail() {
        colored::control::set_override(false);
        let r: Release = serde_json::from_value(json!({
            "tag": "v1.0", "name": "One", "snapshot_id": "0123456789abcdef", "notes": "Fixes\nMore",
            "prerelease": true, "author": "ada", "created_at": "",
            "assets": [{"name": "app.zip", "size": 2048, "content_type": "application/zip", "uploaded_at": ""}]
        }))
        .expect("release");
        let mut t = Text::default();
        show(&mut t, &r);
        let s = t.as_str();
        assert!(s.starts_with("v1.0 One  [pre-release]\n"), "{s}");
        assert!(s.contains("    Fixes\n    More\n"), "{s}");
        assert!(s.contains("app.zip  2.0 KiB  application/zip"), "{s}");
        let mut t = Text::default();
        list_releases(&mut t, &[r]);
        assert!(
            t.as_str()
                .contains("v1.0  One   pre-release  01234567  1       ada"),
            "{}",
            t.as_str()
        );
        let mut t = Text::default();
        list_releases(&mut t, &[]);
        assert_eq!(t.as_str(), "No releases.\n");
    }
}
