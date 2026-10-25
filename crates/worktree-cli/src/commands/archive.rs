//! `wt archive`: write a snapshot's files to a tar.gz or zip archive.

use std::fs::File;
use std::io::{BufWriter, Seek, Write};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Datelike, Timelike, Utc};
use clap::{Args, ValueEnum};
use colored::Colorize;
use flate2::write::GzEncoder;
use flate2::Compression;
use serde::Serialize;
use worktree_git::license::{self, LicensePolicy, LicenseRule, NOTICE_PATH};
use worktree_sdk::remote::ORIGIN;
use worktree_sdk::{Repository, TreeInfo};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::commands::server_common::{decode, default_tree, Api};
use crate::context::Context;
use crate::error::{CliError, Result};
use crate::output::format::plural;

#[derive(Debug, Args)]
pub struct ArchiveArgs {
    /// Snapshot to archive (default: HEAD)
    #[arg(value_name = "REV")]
    pub rev: Option<String>,

    /// Archive file to write
    #[arg(short, long, value_name = "FILE")]
    pub output: PathBuf,

    /// Archive format (default: from the file extension, else tar.gz)
    #[arg(long, value_enum)]
    pub format: Option<ArchiveFormat>,

    #[command(flatten)]
    pub license: LicenseFlags,
}

/// License handling of `wt archive`, `wt git export` and `wt git push`.
///
/// The tree's license rules are fetched from the server when the worktree
/// has an `origin` remote (and cached in `.wt/trees/<tree>/licenses.json`;
/// the cache is used offline). Paths whose rule forbids redistribution are
/// skipped with a warning and a `LICENSES/NOTICE` listing the SPDX id of
/// every exported path is added.
#[derive(Debug, Clone, Default, Args)]
pub struct LicenseFlags {
    /// Ignore license rules: export every path and add no LICENSES/NOTICE
    #[arg(long)]
    pub no_license: bool,
    /// Keep paths whose license forbids redistribution
    #[arg(long, conflicts_with = "no_license")]
    pub include_restricted: bool,
}

/// Where the rules of a license-aware export came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleSource {
    /// `--no-license`.
    Disabled,
    /// Fetched from the server just now.
    Server,
    /// `.wt/trees/<tree>/licenses.json`.
    Cache,
    /// No rules known for the tree.
    None,
}

/// Print a warning on stderr.
pub fn warn(message: impl std::fmt::Display) {
    eprintln!("{} {message}", "warning:".yellow().bold());
}

/// The license policy for exporting `tree` (see [`LicenseFlags`]); `None`
/// when no rule applies.
pub fn license_policy(
    ctx: &Context,
    repo: &Repository,
    tree: &TreeInfo,
    flags: &LicenseFlags,
) -> Result<(Option<LicensePolicy>, RuleSource)> {
    if flags.no_license {
        return Ok((None, RuleSource::Disabled));
    }
    let wt_dir = repo.wt_dir();
    let mut source = RuleSource::None;
    let mut rules = None;
    if repo.config().remote.contains_key(ORIGIN) {
        match fetch_rules(ctx, repo, tree) {
            Ok(fetched) => {
                if let Err(e) = license::save_cached_rules(&wt_dir, &tree.name, &fetched) {
                    warn(format_args!("cannot cache the license rules: {e}"));
                }
                rules = Some(fetched);
                source = RuleSource::Server;
            }
            Err(e) => warn(format_args!(
                "cannot fetch the license rules of tree '{}': {e}; using the cached rules",
                tree.name
            )),
        }
    }
    if rules.is_none() {
        if let Some(cached) = license::load_cached_rules(&wt_dir, &tree.name)? {
            rules = Some(cached);
            source = RuleSource::Cache;
        }
    }
    let active = license::active_rules(&rules.unwrap_or_default(), Utc::now());
    if active.is_empty() {
        return Ok((None, source));
    }
    Ok((
        Some(LicensePolicy::new(active, flags.include_restricted)),
        source,
    ))
}

fn fetch_rules(ctx: &Context, repo: &Repository, tree: &TreeInfo) -> Result<Vec<LicenseRule>> {
    let tree_ref = default_tree(repo, tree)?;
    let api = Api::connect(ctx)?;
    let value = api.get(&format!("{}/licenses", tree_ref.api()))?;
    let rules: Option<Vec<LicenseRule>> = decode(&value)?;
    Ok(rules.unwrap_or_default())
}

/// Warn about each path a license-aware export left out.
pub fn warn_restricted(restricted: &[String]) {
    for path in restricted {
        warn(format_args!(
            "skipped {path}: its license forbids redistribution (use --include-restricted to keep it)"
        ));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize)]
pub enum ArchiveFormat {
    #[value(name = "tar.gz", alias = "tgz")]
    #[serde(rename = "tar.gz")]
    TarGz,
    #[value(name = "zip")]
    #[serde(rename = "zip")]
    Zip,
}

impl ArchiveFormat {
    fn from_path(path: &Path) -> Self {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if name.ends_with(".zip") {
            Self::Zip
        } else {
            Self::TarGz
        }
    }
}

/// One file to put in an archive.
pub struct ArchiveEntry {
    pub path: String,
    pub data: Vec<u8>,
    pub executable: bool,
}

#[derive(Serialize)]
struct ArchiveResult<'a> {
    output: &'a Path,
    format: ArchiveFormat,
    snapshot: &'a str,
    files: usize,
    /// Paths left out because their license forbids redistribution.
    restricted: &'a [String],
    /// Whether a LICENSES/NOTICE was added.
    notice: bool,
    license_rules: RuleSource,
}

pub fn run(ctx: &Context, args: ArchiveArgs) -> Result<()> {
    let (repo, tree) = ctx.open_tree()?;
    let snapshot = repo.resolve(&tree.name, args.rev.as_deref().unwrap_or("HEAD"))?;
    let mut manifest = repo.manifest_of(&snapshot)?;
    let format = args
        .format
        .unwrap_or_else(|| ArchiveFormat::from_path(&args.output));
    let output = ctx.absolute(&args.output);

    let (policy, rule_source) = license_policy(ctx, &repo, &tree, &args.license)?;
    let mut restricted = Vec::new();
    let mut notice = None;
    if let Some(policy) = &policy {
        let header = vec![
            format!("Tree: {}", tree.name),
            format!("Snapshot: {}", snapshot.id),
        ];
        let licensed = policy.apply(&manifest, &header);
        manifest = licensed.manifest;
        restricted = licensed.restricted;
        notice = licensed.notice;
        warn_restricted(&restricted);
    }

    let store = repo.store();
    let entries = manifest
        .entries
        .iter()
        .map(|entry| -> Result<ArchiveEntry> {
            let data = match &notice {
                Some(text) if entry.path == NOTICE_PATH => text.clone(),
                // read_entry reassembles chunked files (§13.2).
                _ => store.read_entry(entry)?,
            };
            Ok(ArchiveEntry {
                path: entry.path.clone(),
                data,
                executable: entry.executable,
            })
        });
    let file = File::create(&output)
        .map_err(|e| CliError::failure(format!("cannot create '{}': {e}", output.display())))?;
    let written = write_archive(format, BufWriter::new(file), snapshot.created_at, entries);
    let count = match written {
        Ok(count) => count,
        Err(err) => {
            // Do not leave a truncated archive behind; the write error is what matters.
            let _ = std::fs::remove_file(&output);
            return Err(err);
        }
    };

    let result = ArchiveResult {
        output: &output,
        format,
        snapshot: &snapshot.id,
        files: count,
        restricted: &restricted,
        notice: notice.is_some(),
        license_rules: rule_source,
    };
    ctx.out.emit(&result, |t| {
        t.line(format_args!(
            "Wrote {} from {} to {}",
            plural(count, "file", "files"),
            snapshot.short_id().yellow(),
            output.display()
        ));
        if result.notice {
            t.line(format_args!(
                "Added {NOTICE_PATH} ({} skipped by license)",
                plural(result.restricted.len(), "path", "paths")
            ));
        }
    })
}

/// Write `entries` as an archive with every file stamped `mtime`.
/// Returns the number of files written.
pub fn write_archive<W: Write + Seek>(
    format: ArchiveFormat,
    writer: W,
    mtime: DateTime<Utc>,
    entries: impl IntoIterator<Item = Result<ArchiveEntry>>,
) -> Result<usize> {
    match format {
        ArchiveFormat::TarGz => write_tar_gz(writer, mtime, entries),
        ArchiveFormat::Zip => write_zip(writer, mtime, entries),
    }
}

fn mode(executable: bool) -> u32 {
    if executable {
        0o755
    } else {
        0o644
    }
}

fn write_tar_gz<W: Write>(
    writer: W,
    mtime: DateTime<Utc>,
    entries: impl IntoIterator<Item = Result<ArchiveEntry>>,
) -> Result<usize> {
    let mut builder = tar::Builder::new(GzEncoder::new(writer, Compression::default()));
    let mtime = u64::try_from(mtime.timestamp()).unwrap_or(0);
    let mut count = 0;
    for entry in entries {
        let entry = entry?;
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_size(entry.data.len() as u64);
        header.set_mode(mode(entry.executable));
        header.set_mtime(mtime);
        builder.append_data(&mut header, &entry.path, entry.data.as_slice())?;
        count += 1;
    }
    builder.into_inner()?.finish()?.flush()?;
    Ok(count)
}

fn write_zip<W: Write + Seek>(
    writer: W,
    mtime: DateTime<Utc>,
    entries: impl IntoIterator<Item = Result<ArchiveEntry>>,
) -> Result<usize> {
    let zip_err =
        |e: zip::result::ZipError| CliError::failure(format!("cannot write zip archive: {e}"));
    let mut zip = ZipWriter::new(writer);
    let base = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .last_modified_time(zip_time(mtime));
    let mut count = 0;
    for entry in entries {
        let entry = entry?;
        let options = base
            .unix_permissions(mode(entry.executable))
            .large_file(entry.data.len() >= u32::MAX as usize);
        zip.start_file(entry.path.as_str(), options)
            .map_err(zip_err)?;
        zip.write_all(&entry.data)?;
        count += 1;
    }
    zip.finish().map_err(zip_err)?.flush()?;
    Ok(count)
}

/// Zip timestamps are DOS dates (1980..=2107); clamp anything outside.
fn zip_time(at: DateTime<Utc>) -> zip::DateTime {
    let date = |y: i32| {
        u16::try_from(y).ok().and_then(|year| {
            zip::DateTime::from_date_and_time(
                year,
                u8::try_from(at.month()).ok()?,
                u8::try_from(at.day()).ok()?,
                u8::try_from(at.hour()).ok()?,
                u8::try_from(at.minute()).ok()?,
                u8::try_from(at.second()).ok()?,
            )
            .ok()
        })
    };
    date(at.year()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Read};

    use chrono::TimeZone;

    use super::*;

    fn entries() -> Vec<Result<ArchiveEntry>> {
        vec![
            Ok(ArchiveEntry {
                path: "README.md".into(),
                data: b"# hello\n".to_vec(),
                executable: false,
            }),
            Ok(ArchiveEntry {
                path: "bin/run.sh".into(),
                data: b"#!/bin/sh\necho hi\n".to_vec(),
                executable: true,
            }),
            Ok(ArchiveEntry {
                path: "empty.txt".into(),
                data: Vec::new(),
                executable: false,
            }),
        ]
    }

    fn mtime() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 24, 12, 30, 44)
            .single()
            .expect("valid time")
    }

    #[test]
    fn format_from_extension() {
        assert_eq!(
            ArchiveFormat::from_path(Path::new("out.zip")),
            ArchiveFormat::Zip
        );
        assert_eq!(
            ArchiveFormat::from_path(Path::new("OUT.ZIP")),
            ArchiveFormat::Zip
        );
        assert_eq!(
            ArchiveFormat::from_path(Path::new("out.tar.gz")),
            ArchiveFormat::TarGz
        );
        assert_eq!(
            ArchiveFormat::from_path(Path::new("out.tgz")),
            ArchiveFormat::TarGz
        );
        assert_eq!(
            ArchiveFormat::from_path(Path::new("out")),
            ArchiveFormat::TarGz
        );
    }

    #[test]
    fn format_serializes_as_cli_name() {
        assert_eq!(
            serde_json::to_string(&ArchiveFormat::TarGz).expect("json"),
            "\"tar.gz\""
        );
        assert_eq!(
            serde_json::to_string(&ArchiveFormat::Zip).expect("json"),
            "\"zip\""
        );
    }

    #[test]
    fn tar_gz_round_trip() {
        let mut buf = Cursor::new(Vec::new());
        let count =
            write_archive(ArchiveFormat::TarGz, &mut buf, mtime(), entries()).expect("write");
        assert_eq!(count, 3);

        let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(buf.get_ref().as_slice()));
        let mut seen = Vec::new();
        for entry in archive.entries().expect("entries") {
            let mut entry = entry.expect("entry");
            let path = entry
                .path()
                .expect("path")
                .to_string_lossy()
                .replace('\\', "/");
            let mode = entry.header().mode().expect("mode");
            let time = entry.header().mtime().expect("mtime");
            let mut data = Vec::new();
            entry.read_to_end(&mut data).expect("read");
            seen.push((path, mode, time, data));
        }
        let ts = u64::try_from(mtime().timestamp()).expect("positive");
        assert_eq!(
            seen,
            vec![
                ("README.md".to_string(), 0o644, ts, b"# hello\n".to_vec()),
                (
                    "bin/run.sh".to_string(),
                    0o755,
                    ts,
                    b"#!/bin/sh\necho hi\n".to_vec()
                ),
                ("empty.txt".to_string(), 0o644, ts, Vec::new()),
            ]
        );
    }

    #[test]
    fn zip_round_trip() {
        let mut buf = Cursor::new(Vec::new());
        let count = write_archive(ArchiveFormat::Zip, &mut buf, mtime(), entries()).expect("write");
        assert_eq!(count, 3);

        let mut archive = zip::ZipArchive::new(Cursor::new(buf.into_inner())).expect("zip");
        assert_eq!(archive.len(), 3);
        let mut file = archive.by_name("bin/run.sh").expect("entry");
        assert_eq!(file.unix_mode().map(|m| m & 0o777), Some(0o755));
        let modified = file.last_modified().expect("mtime");
        assert_eq!(
            (modified.year(), modified.month(), modified.day()),
            (2026, 10, 24)
        );
        let mut data = String::new();
        file.read_to_string(&mut data).expect("read");
        assert_eq!(data, "#!/bin/sh\necho hi\n");
        drop(file);
        let readme = archive.by_name("README.md").expect("entry");
        assert_eq!(readme.unix_mode().map(|m| m & 0o777), Some(0o644));
    }

    #[test]
    fn entry_errors_abort_the_archive() {
        let failing = vec![
            Ok(ArchiveEntry {
                path: "a".into(),
                data: b"a".to_vec(),
                executable: false,
            }),
            Err(CliError::failure("object not found")),
        ];
        let mut buf = Cursor::new(Vec::new());
        let err =
            write_archive(ArchiveFormat::Zip, &mut buf, mtime(), failing).expect_err("must fail");
        assert!(err.to_string().contains("object not found"));
    }

    #[test]
    fn zip_time_clamps_pre_1980() {
        let old = Utc
            .with_ymd_and_hms(1970, 1, 1, 0, 0, 0)
            .single()
            .expect("valid");
        assert_eq!(zip_time(old), zip::DateTime::default());
        let now = zip_time(mtime());
        assert_eq!((now.year(), now.hour(), now.second()), (2026, 12, 44));
    }
}
