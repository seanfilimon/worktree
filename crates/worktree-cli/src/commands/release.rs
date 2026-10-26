//! `wt release` — releases of a server tree: list, create, show, edit,
//! delete, and release assets (upload, download) (ENGINEERING_PLAN §13.5).

use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use worktree_sdk::remote::rest::seg;
use worktree_sdk::signing::{self, KeyStore, Verification};
use worktree_sdk::{SnapshotSignature, TagInfo};

use crate::commands::identity::{account_keys, describe};
use crate::commands::server_common::{
    bytes, confirm, decode, fields, nullable, or_dash, render, timestamp, tree_or_default, when,
    yes_no, Api, Cell, Table, TreeRef,
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
        /// Also generate an SPDX 2.3 SBOM of the snapshot and attach it as sbom.spdx.json
        #[arg(long)]
        sbom: bool,
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
    // v0.7 trust (ENGINEERING_PLAN §15.8).
    /// Print the SHA256SUMS of a release's assets
    Checksums {
        tag: String,
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: Option<TreeRef>,
    },
    /// Generate an SPDX 2.3 SBOM of the release's snapshot and attach it as sbom.spdx.json
    Sbom {
        tag: String,
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: Option<TreeRef>,
    },
    /// Check downloaded assets against the release checksums and verify the tag signature
    Verify {
        tag: String,
        /// Directory holding the downloaded assets
        // (id differs from the global -C `dir`)
        #[arg(
            id = "assets_dir",
            short = 'd',
            long = "dir",
            value_name = "DIR",
            default_value = "."
        )]
        dir: PathBuf,
        /// Server tree (default: the tree of the current worktree)
        #[arg(id = "tree_ref", long = "in", value_name = "TENANT/TREE")]
        tree: Option<TreeRef>,
        /// Also trust the registered keys of this account (the signer's are always trusted)
        #[arg(long, value_name = "USERNAME")]
        account: Option<String>,
        /// Accept a release whose tag is not signed (checksums are still checked)
        #[arg(long)]
        allow_unsigned: bool,
        /// Skip assets that were not downloaded instead of failing
        #[arg(long)]
        ignore_missing: bool,
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
    // v0.7 trust: computed by the server from the tag.
    verified: bool,
    signer: String,
}

/// `domain.ReleaseAsset`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Asset {
    name: String,
    size: u64,
    content_type: String,
    uploaded_at: String,
    /// v0.7: hex SHA-256 ("" for assets uploaded before v0.7).
    sha256: String,
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
            sbom,
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
            let mut value = api.post(&releases_path(&tree), &body)?;
            let sbom_asset = if sbom {
                let asset = api.post(&format!("{}/sbom", release_path(&tree, &tag)), &json!({}))?;
                if let Value::Object(map) = &mut value {
                    map.insert("sbom".into(), asset.clone());
                }
                Some(decode::<Asset>(&asset)?)
            } else {
                None
            };
            render(out, &value, |t, r: Release| {
                t.line(format_args!(
                    "Created {}release {} at {} on {tree}{}",
                    if r.draft { "draft " } else { "" },
                    r.tag.green().bold(),
                    short(&r.snapshot_id).yellow(),
                    if r.verified { " (verified tag)" } else { "" }
                ));
                if let Some(a) = &sbom_asset {
                    t.line(format_args!(
                        "Attached SBOM {} ({})",
                        a.name.green(),
                        bytes(a.size)
                    ));
                }
            })
        }
        ReleaseCommand::Checksums { tag, tree } => {
            let tree = tree_or_default(ctx, tree)?;
            let text = checksums(&api, &tree, &tag)?;
            let pairs = signing::parse_checksums(&text)?;
            let report: Vec<Value> = pairs
                .iter()
                .map(|(n, s)| json!({ "name": n, "sha256": s }))
                .collect();
            out.emit(&report, |t| {
                if pairs.is_empty() {
                    t.line(format!("Release {tag} has no assets.").dimmed());
                }
                for line in text.lines() {
                    t.line(line);
                }
            })
        }
        ReleaseCommand::Sbom { tag, tree } => {
            let tree = tree_or_default(ctx, tree)?;
            let value = api.post(&format!("{}/sbom", release_path(&tree, &tag)), &json!({}))?;
            render(out, &value, |t, a: Asset| {
                t.line(format_args!(
                    "Attached SBOM {} ({}) to release {}",
                    a.name.green(),
                    bytes(a.size),
                    tag.bold()
                ));
            })
        }
        ReleaseCommand::Verify {
            tag,
            dir,
            tree,
            account,
            allow_unsigned,
            ignore_missing,
        } => {
            let tree = tree_or_default(ctx, tree)?;
            let report = verify_release(
                ctx,
                &api,
                &tree,
                &tag,
                &ctx.absolute(&dir),
                account.as_deref(),
            )?;
            let failures = report.failures(allow_unsigned, ignore_missing);
            out.emit(&report, |t| render_verify(t, &report))?;
            if failures.is_empty() {
                Ok(())
            } else {
                Err(CliError::failure(format!(
                    "release {tag} did not verify: {}",
                    failures.join("; ")
                )))
            }
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

// ---- v0.7 trust (ENGINEERING_PLAN §15.8): checksums and verification ----

/// The SHA256SUMS text of a release.
fn checksums(api: &Api, tree: &TreeRef, tag: &str) -> Result<String> {
    let data = api.block_on(
        api.client()
            .get_bytes(&format!("{}/checksums", release_path(tree, tag))),
    )?;
    String::from_utf8(data)
        .map_err(|_| CliError::failure("the server sent checksums that are not UTF-8"))
}

/// `domain.Tag` as the REST API returns it.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ServerTag {
    name: String,
    target: String,
    message: String,
    tagger: String,
    created_at: String,
    signature: String,
    signing_key_id: String,
    verified: bool,
    signer: String,
    signed_tagger: String,
}

impl ServerTag {
    /// The tag record its signature covers.
    fn to_tag_info(&self) -> Result<TagInfo> {
        let created_at = timestamp(&self.created_at).ok_or_else(|| {
            CliError::failure(format!("tag {} has a malformed created_at", self.name))
        })?;
        let signature = if self.signature.is_empty() && self.signing_key_id.is_empty() {
            None
        } else {
            Some(SnapshotSignature {
                key_id: self.signing_key_id.clone(),
                signature: signing::decode_base64(&self.signature)?,
            })
        };
        let tagger = if signature.is_some() && !self.signed_tagger.is_empty() {
            self.signed_tagger.clone()
        } else {
            self.tagger.clone()
        };
        Ok(TagInfo {
            name: self.name.clone(),
            target: self.target.clone(),
            message: (!self.message.is_empty()).then(|| self.message.clone()),
            tagger,
            created_at,
            signature,
        })
    }
}

/// One asset checked by `wt release verify`.
#[derive(Debug, Serialize)]
struct AssetCheck {
    name: String,
    expected: String,
    actual: Option<String>,
    /// "ok" | "mismatch" | "missing" | "unlisted"
    status: &'static str,
}

/// The `wt release verify` report.
#[derive(Debug, Serialize)]
struct VerifyReport {
    tag: String,
    snapshot_id: String,
    tag_target: String,
    signer: String,
    server_verified: bool,
    signature: Verification,
    assets: Vec<AssetCheck>,
}

impl VerifyReport {
    /// Why the release does not verify (empty = it does).
    fn failures(&self, allow_unsigned: bool, ignore_missing: bool) -> Vec<String> {
        let mut out = Vec::new();
        match &self.signature {
            Verification::Valid { .. } => {}
            Verification::Unsigned if allow_unsigned => {}
            Verification::Unsigned => out.push("the tag is not signed".to_string()),
            Verification::UnknownKey { key_id } => {
                out.push(format!("the tag is signed by unknown key {key_id}"))
            }
            Verification::Invalid { .. } => out.push("the tag signature is invalid".to_string()),
        }
        if self.tag_target != self.snapshot_id {
            out.push(format!(
                "the tag points at {} but the release at {}",
                short(&self.tag_target),
                short(&self.snapshot_id)
            ));
        }
        for a in &self.assets {
            match a.status {
                "ok" => {}
                "missing" if ignore_missing => {}
                status => out.push(format!("{} {status}", a.name)),
            }
        }
        out
    }
}

fn verify_release(
    ctx: &Context,
    api: &Api,
    tree: &TreeRef,
    tag: &str,
    dir: &Path,
    account: Option<&str>,
) -> Result<VerifyReport> {
    let release: Release = decode(&api.get(&release_path(tree, tag))?)?;
    let sums = signing::parse_checksums(&checksums(api, tree, tag)?)?;
    let tags: Vec<ServerTag> = decode(&api.get(&format!("{}/tags", tree.api()))?)?;
    let server_tag = tags
        .into_iter()
        .find(|t| t.name == tag)
        .ok_or_else(|| CliError::failure(format!("tag {tag} does not exist on {tree}")))?;
    let info = server_tag.to_tag_info()?;

    // Trust local keys plus the registered keys of the signer (and --account).
    let mut trusted = KeyStore::default_location().trusted()?;
    let mut accounts: Vec<&str> = Vec::new();
    if !server_tag.signer.is_empty() {
        accounts.push(&server_tag.signer);
    }
    if let Some(a) = account {
        if !accounts.contains(&a) {
            accounts.push(a);
        }
    }
    if info.signature.is_some() {
        for a in accounts {
            trusted.extend(account_keys(ctx, a)?);
        }
    }
    let signature = signing::verify_tag_with_keys(&info, &trusted);

    let mut assets = Vec::new();
    for (name, expected) in &sums {
        let local = file_name(Path::new(name))
            .ok()
            .map(|n| dir.join(n))
            .filter(|p| p.is_file());
        let check = match local {
            None => AssetCheck {
                name: name.clone(),
                expected: expected.clone(),
                actual: None,
                status: "missing",
            },
            Some(path) => {
                let data = std::fs::read(&path).map_err(|e| {
                    CliError::failure(format!("cannot read '{}': {e}", path.display()))
                })?;
                let actual = signing::sha256_hex(&data);
                let status = if &actual == expected {
                    "ok"
                } else {
                    "mismatch"
                };
                AssetCheck {
                    name: name.clone(),
                    expected: expected.clone(),
                    actual: Some(actual),
                    status,
                }
            }
        };
        assets.push(check);
    }
    // Assets the release lists without a checksum line are suspicious.
    for a in &release.assets {
        if !sums.iter().any(|(n, _)| n == &a.name) {
            assets.push(AssetCheck {
                name: a.name.clone(),
                expected: a.sha256.clone(),
                actual: None,
                status: "unlisted",
            });
        }
    }
    Ok(VerifyReport {
        tag: release.tag,
        snapshot_id: release.snapshot_id,
        tag_target: info.target,
        signer: server_tag.signer,
        server_verified: server_tag.verified,
        signature,
        assets,
    })
}

fn render_verify(t: &mut Text, r: &VerifyReport) {
    t.line(format_args!(
        "release {} ({})",
        r.tag.bold(),
        short(&r.snapshot_id).yellow()
    ));
    let sig = describe(&r.signature);
    t.line(format_args!(
        "  tag signature: {}",
        if r.signature.is_valid() {
            sig.green()
        } else {
            sig.red()
        }
    ));
    for a in &r.assets {
        let status = match a.status {
            "ok" => "OK".green(),
            other => other.to_uppercase().red(),
        };
        t.line(format_args!("  {status}  {}", a.name));
    }
    if r.assets.is_empty() {
        t.line("  (no assets)".dimmed());
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
    let verified = if r.verified { ", verified" } else { "" };
    if r.draft {
        Cell::from(format!("draft{verified}")).yellow()
    } else if r.prerelease {
        Cell::from(format!("pre-release{verified}")).cyan()
    } else {
        Cell::from(format!("release{verified}")).green()
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
            (
                "verified",
                if r.verified {
                    format!("yes (tag signed by {})", or_dash(&r.signer))
                } else {
                    "no".to_string()
                },
            ),
        ],
    );
    if !r.notes.is_empty() {
        t.blank();
        for line in r.notes.lines() {
            t.line(format_args!("    {line}"));
        }
    }
    t.blank();
    let mut table = Table::new(&["ASSET", "SIZE", "TYPE", "UPLOADED", "SHA256"]);
    for a in &r.assets {
        table.row([
            Cell::from(a.name.as_str()).bold(),
            bytes(a.size).into(),
            a.content_type.as_str().into(),
            when(&a.uploaded_at).into(),
            or_dash(a.sha256.get(..16).unwrap_or(&a.sha256)).into(),
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

    // ---- v0.7 trust ----

    #[test]
    fn parses_trust_subcommands() {
        assert!(matches!(
            release(&["create", "v1", "--sbom"]),
            ReleaseCommand::Create { sbom: true, .. }
        ));
        assert!(matches!(
            release(&["checksums", "v1", "acme/web"]),
            ReleaseCommand::Checksums { tree: Some(_), .. }
        ));
        assert!(matches!(
            release(&["sbom", "v1"]),
            ReleaseCommand::Sbom { .. }
        ));
        let ReleaseCommand::Verify {
            tag,
            dir,
            account,
            allow_unsigned,
            ignore_missing,
            ..
        } = release(&[
            "verify",
            "v1",
            "-d",
            "dist",
            "--account",
            "ada",
            "--allow-unsigned",
            "--ignore-missing",
        ])
        else {
            panic!()
        };
        assert_eq!(tag, "v1");
        assert_eq!(dir, PathBuf::from("dist"));
        assert_eq!(account.as_deref(), Some("ada"));
        assert!(allow_unsigned && ignore_missing);
        assert!(matches!(
            release(&["verify", "v1"]),
            ReleaseCommand::Verify { dir, .. } if dir == Path::new(".")
        ));
    }

    #[test]
    fn server_tags_rebuild_the_signed_record() {
        let key = signing::LocalKey::from_seed("k", &[3; 32]);
        let mut info = TagInfo {
            name: "v1".into(),
            target: "s1".into(),
            message: Some("Release v1".into()),
            tagger: "Ada <ada@example.com>".into(),
            created_at: timestamp("2026-01-02T03:04:05.000000123Z").expect("ts"),
            signature: None,
        };
        key.sign_tag(&mut info);
        let sig = info.signature.clone().expect("signed");
        // Go's encoding/json writes []byte as standard base64.
        let b64 = signing::encode_base64(&sig.signature);
        let server: ServerTag = serde_json::from_value(json!({
            "name": "v1", "target": "s1", "message": "Release v1", "tagger": "ada",
            "created_at": "2026-01-02T03:04:05.000000123Z", "signature": b64,
            "signing_key_id": sig.key_id, "verified": true, "signer": "ada",
            "signed_tagger": "Ada <ada@example.com>"
        }))
        .expect("tag");
        let rebuilt = server.to_tag_info().expect("info");
        assert_eq!(rebuilt, info);
        assert!(signing::verify_tag_signature(&rebuilt, &key.public_key()));

        let plain: ServerTag = serde_json::from_value(json!({
            "name": "v0", "target": "s0", "message": "", "tagger": "bob",
            "created_at": "2026-01-02T03:04:05Z", "verified": false
        }))
        .expect("plain");
        let plain = plain.to_tag_info().expect("info");
        assert!(plain.signature.is_none() && plain.message.is_none());
        assert_eq!(plain.tagger, "bob");
    }

    #[test]
    fn verify_report_failures() {
        let ok = |name: &str| AssetCheck {
            name: name.into(),
            expected: "e".into(),
            actual: Some("e".into()),
            status: "ok",
        };
        let mut r = VerifyReport {
            tag: "v1".into(),
            snapshot_id: "s1".into(),
            tag_target: "s1".into(),
            signer: "ada".into(),
            server_verified: true,
            signature: Verification::Valid {
                key_id: "k".into(),
                key: "ada/laptop".into(),
                fingerprint: "SHA256:x".into(),
            },
            assets: vec![ok("a.zip")],
        };
        assert!(r.failures(false, false).is_empty());
        r.assets.push(AssetCheck {
            name: "b.bin".into(),
            expected: "e".into(),
            actual: None,
            status: "missing",
        });
        assert_eq!(r.failures(false, false), ["b.bin missing"]);
        assert!(r.failures(false, true).is_empty());
        r.assets[0].status = "mismatch";
        r.signature = Verification::Unsigned;
        r.tag_target = "s2".into();
        let f = r.failures(false, true);
        assert_eq!(f.len(), 3, "{f:?}");
        assert!(
            f[0].contains("not signed") && f[1].contains("points at") && f[2] == "a.zip mismatch"
        );
        assert_eq!(r.failures(true, true).len(), 2);

        colored::control::set_override(false);
        let mut t = Text::default();
        render_verify(&mut t, &r);
        assert!(t.as_str().contains("MISMATCH  a.zip"), "{}", t.as_str());
        let v = serde_json::to_value(&r).expect("json");
        assert_eq!(v["signature"]["status"], "unsigned");
        assert_eq!(v["assets"][1]["status"], "missing");
    }
}
