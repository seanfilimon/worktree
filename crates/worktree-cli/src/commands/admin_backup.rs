//! `wt admin backup` — server backups (docs/ENGINEERING_PLAN.md §15.2):
//! create, list, download and delete `.wtbak` archives. Restoring is
//! offline on the server host: `worktree-server restore --data DIR FILE`.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::{Args, Subcommand};
use colored::Colorize;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use worktree_sdk::remote::rest::seg;
use worktree_sdk::SdkError;

use crate::commands::server_common::{
    bytes, confirm, fields, nullable, or_dash, render, timestamp, when, yes_no, Api, Cell, Table,
};
use crate::context::Context;
use crate::error::{CliError, Result};
use crate::output::format::date;
use crate::output::Text;

/// Backups are written synchronously and downloads can be large: allow
/// long requests.
const LONG_TIMEOUT: Duration = Duration::from_secs(4 * 3600);

const BACKUPS: &str = "/api/v1/admin/backups";

#[derive(Debug, Args)]
pub struct BackupArgs {
    #[command(subcommand)]
    pub command: BackupCommand,
}

#[derive(Debug, Subcommand)]
pub enum BackupCommand {
    /// Back up the server now: a consistent snapshot of the metadata
    /// database plus every object, written to <data>/backups/<id>.wtbak on
    /// the server (waits until the backup is complete)
    Create {
        /// Note stored with the backup, e.g. "before upgrading to 0.8"
        #[arg(long, value_name = "TEXT")]
        note: Option<String>,
    },
    /// List backups with the backup schedule and object store
    List,
    /// Download a backup archive
    Download {
        /// Backup id (from `wt admin backup list`)
        id: String,
        /// Output file (default: ./<id>.wtbak)
        #[arg(short = 'o', long, value_name = "FILE")]
        output: Option<PathBuf>,
    },
    /// Delete a backup from the server
    Delete {
        /// Backup id
        id: String,
        /// Do not ask for confirmation
        #[arg(long, short = 'y')]
        yes: bool,
    },
}

/// `domain.BackupInfo`.
#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct BackupInfo {
    pub id: String,
    pub path: String,
    pub size_bytes: u64,
    pub objects: u64,
    pub created_at: String,
    pub created_by: String,
    pub note: String,
}

/// `GET /api/v1/admin/backups`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct BackupStatus {
    config: BackupConfig,
    store: StoreInfo,
    supported: bool,
    running: bool,
    next_run_at: String,
    last_error: String,
    #[serde(deserialize_with = "nullable")]
    backups: Vec<BackupInfo>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct BackupConfig {
    dir: String,
    interval: String,
    interval_seconds: u64,
    keep: u64,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct StoreInfo {
    kind: String,
    root: String,
    endpoint: String,
    bucket: String,
    prefix: String,
    region: String,
    secure: bool,
    compress: bool,
}

fn backup_path(id: &str) -> String {
    format!("{BACKUPS}/{}", seg(id))
}

pub fn run(ctx: &Context, api: &Api, args: BackupArgs) -> Result<()> {
    let out = &ctx.out;
    match args.command {
        BackupCommand::Create { note } => {
            let body = match note {
                Some(note) => json!({ "note": note }),
                None => json!({}),
            };
            let resp = api.block_on(long_request(
                api,
                reqwest::Method::POST,
                BACKUPS,
                Some(serde_json::to_vec(&body).map_err(|e| CliError::failure(e.to_string()))?),
            ))?;
            let value: Value = serde_json::from_slice(&resp).map_err(|e| {
                CliError::failure(format!("unexpected response from the server: {e}"))
            })?;
            render(out, &value, |t, b: BackupInfo| {
                t.line(format_args!(
                    "Created backup {} ({} objects, {})",
                    b.id.green().bold(),
                    b.objects,
                    bytes(b.size_bytes)
                ));
                t.line(format!("stored on the server at {}", b.path).dimmed());
                t.line(
                    format!(
                        "download it with `wt admin backup download {}`; restore with `worktree-server restore --data DIR FILE`",
                        b.id
                    )
                    .dimmed(),
                );
            })
        }
        BackupCommand::List => {
            let value = api.get(BACKUPS)?;
            render(out, &value, |t, s: BackupStatus| status(t, &s))
        }
        BackupCommand::Download { id, output } => {
            let dest =
                ctx.absolute(&output.unwrap_or_else(|| PathBuf::from(format!("{id}.wtbak"))));
            let size = api.block_on(download(api, &id, &dest))?;
            out.emit(
                &json!({ "id": id, "path": dest.display().to_string(), "size_bytes": size }),
                |t| {
                    t.line(format_args!(
                        "Downloaded backup {} to {} ({})",
                        id.bold(),
                        dest.display(),
                        bytes(size)
                    ));
                },
            )
        }
        BackupCommand::Delete { id, yes } => {
            confirm(yes, &format!("delete backup {id}"))?;
            api.delete(&backup_path(&id), &[])?;
            out.emit(&json!({ "id": id, "deleted": true }), |t| {
                t.line(format_args!("Deleted backup {}", id.bold()));
            })
        }
    }
}

/// The saved bearer token for the API's server (or `WT_TOKEN`).
/// device-auth (§16.6): renewed first when it is about to expire.
async fn token(api: &Api) -> Result<Option<String>> {
    Ok(
        worktree_sdk::remote::fresh_credential(api.client().server())
            .await?
            .map(|c| c.token),
    )
}

fn http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(concat!("wt/", env!("CARGO_PKG_VERSION")))
        .timeout(LONG_TIMEOUT)
        .build()
        .map_err(|e| CliError::failure(format!("cannot create HTTP client: {e}")))
}

fn error_chain(e: &dyn std::error::Error) -> String {
    let mut text = e.to_string();
    let mut source = e.source();
    while let Some(s) = source {
        let part = s.to_string();
        if !text.contains(&part) {
            text.push_str(": ");
            text.push_str(&part);
        }
        source = s.source();
    }
    text
}

/// The SDK's mapping of an error response (401 = not logged in, 403 =
/// permission denied, else the server's message).
fn api_error(server: &str, status: u16, body: &[u8]) -> CliError {
    #[derive(Deserialize, Default)]
    #[serde(default)]
    struct Detail {
        code: String,
        message: String,
    }
    #[derive(Deserialize)]
    struct Envelope {
        error: Detail,
    }
    let detail = serde_json::from_slice::<Envelope>(body)
        .map(|e| e.error)
        .unwrap_or_default();
    let err = if status == 401 || detail.code == "unauthenticated" {
        SdkError::NotLoggedIn(server.to_string())
    } else if status == 403 || detail.code == "forbidden" {
        SdkError::PermissionDenied(if detail.message.is_empty() {
            "forbidden".into()
        } else {
            detail.message
        })
    } else if detail.message.is_empty() {
        SdkError::Remote(format!("{server}: request failed (HTTP {status})"))
    } else {
        SdkError::Remote(format!(
            "{} ({}, HTTP {status})",
            detail.message, detail.code
        ))
    };
    err.into()
}

/// A request with the long timeout; returns the body of a 2xx response.
async fn long_request(
    api: &Api,
    method: reqwest::Method,
    path: &str,
    body: Option<Vec<u8>>,
) -> Result<Vec<u8>> {
    let server = api.client().server().to_string();
    let mut req = http_client()?.request(method, format!("{server}{path}"));
    if let Some(t) = token(api).await? {
        req = req.header(AUTHORIZATION, format!("Bearer {t}"));
    }
    if let Some(body) = body {
        req = req.header(CONTENT_TYPE, "application/json").body(body);
    }
    let resp = req
        .send()
        .await
        .map_err(|e| CliError::failure(format!("cannot reach {server}: {}", error_chain(&e))))?;
    let status = resp.status().as_u16();
    let body = resp
        .bytes()
        .await
        .map_err(|e| CliError::failure(format!("reading the response: {}", error_chain(&e))))?;
    if !(200..300).contains(&status) {
        return Err(api_error(&server, status, &body));
    }
    Ok(body.to_vec())
}

/// Stream backup `id` into `dest` (through `<dest>.part`); returns the size.
async fn download(api: &Api, id: &str, dest: &Path) -> Result<u64> {
    let server = api.client().server().to_string();
    let url = format!("{server}{}/download", backup_path(id));
    let mut req = http_client()?.get(url);
    if let Some(t) = token(api).await? {
        req = req.header(AUTHORIZATION, format!("Bearer {t}"));
    }
    let mut resp = req
        .send()
        .await
        .map_err(|e| CliError::failure(format!("cannot reach {server}: {}", error_chain(&e))))?;
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        let body = resp.bytes().await.unwrap_or_default();
        return Err(api_error(&server, status, &body));
    }
    if let Some(dir) = dest.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir).map_err(|e| {
                CliError::failure(format!("cannot create '{}': {e}", dir.display()))
            })?;
        }
    }
    let mut part = dest.as_os_str().to_owned();
    part.push(".part");
    let part = PathBuf::from(part);
    let result = async {
        let mut file = std::fs::File::create(&part)
            .map_err(|e| CliError::failure(format!("cannot write '{}': {e}", part.display())))?;
        let mut size = 0u64;
        while let Some(chunk) = resp
            .chunk()
            .await
            .map_err(|e| CliError::failure(format!("download interrupted: {}", error_chain(&e))))?
        {
            file.write_all(&chunk).map_err(|e| {
                CliError::failure(format!("cannot write '{}': {e}", part.display()))
            })?;
            size += chunk.len() as u64;
        }
        file.sync_all()
            .map_err(|e| CliError::failure(format!("cannot write '{}': {e}", part.display())))?;
        Ok::<u64, CliError>(size)
    }
    .await;
    let size = match result {
        Ok(size) => size,
        Err(e) => {
            let _ = std::fs::remove_file(&part);
            return Err(e);
        }
    };
    std::fs::rename(&part, dest).map_err(|e| {
        let _ = std::fs::remove_file(&part);
        CliError::failure(format!("cannot write '{}': {e}", dest.display()))
    })?;
    Ok(size)
}

fn schedule(c: &BackupConfig) -> String {
    if c.interval_seconds == 0 {
        "off (--backup-interval)".into()
    } else if c.interval.is_empty() {
        format!("every {}s", c.interval_seconds)
    } else {
        format!("every {}", c.interval)
    }
}

fn store_line(s: &StoreInfo) -> String {
    let compress = if s.compress { ", compressed" } else { "" };
    match s.kind.as_str() {
        "s3" => {
            let scheme = if s.secure { "https" } else { "http" };
            let region = if s.region.is_empty() {
                String::new()
            } else {
                format!(", region {}", s.region)
            };
            format!(
                "s3 {scheme}://{}/{}/{}{region}{compress}",
                s.endpoint, s.bucket, s.prefix
            )
        }
        "disk" => format!("disk {}{compress}", s.root),
        "" => "-".into(),
        other => format!("{other}{compress}"),
    }
}

fn status(t: &mut Text, s: &BackupStatus) {
    let c = &s.config;
    let keep = if c.keep == 0 {
        "all".to_string()
    } else {
        format!("{} scheduled", c.keep)
    };
    let next = timestamp(&s.next_run_at).map_or_else(|| "-".into(), date);
    let mut rows = vec![
        ("object store", store_line(&s.store)),
        ("backup dir", or_dash(&c.dir)),
        ("schedule", schedule(c)),
        ("keep", keep),
        ("next run", next),
        ("running", yes_no(s.running).into()),
    ];
    if !s.supported {
        rows.push((
            "supported",
            "no (this server's stores cannot be backed up)"
                .red()
                .to_string(),
        ));
    }
    if !s.last_error.is_empty() {
        rows.push(("last error", s.last_error.red().to_string()));
    }
    fields(t, &rows);
    t.blank();
    let mut table = Table::new(&["ID", "CREATED", "BY", "SIZE", "OBJECTS", "NOTE"]);
    for b in &s.backups {
        table.row([
            Cell::from(b.id.as_str()).bold(),
            when(&b.created_at).into(),
            or_dash(&b.created_by).into(),
            bytes(b.size_bytes).into(),
            b.objects.to_string().into(),
            or_dash(&b.note).into(),
        ]);
    }
    table.render(
        t,
        "No backups yet. Create one with `wt admin backup create`.",
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_status() {
        colored::control::set_override(false);
        let s: BackupStatus = serde_json::from_value(json!({
            "config": {"dir": "/data/backups", "interval": "24h0m0s", "interval_seconds": 86400, "keep": 7},
            "store": {"kind": "s3", "endpoint": "minio:9000", "bucket": "wt", "prefix": "prod/", "compress": true},
            "supported": true, "running": false,
            "backups": [{"id": "bk-20260101T000000Z-abcdef", "path": "/data/backups/x.wtbak",
                         "size_bytes": 2048, "objects": 12, "created_at": "", "created_by": "admin", "note": "pre"}]
        }))
        .expect("status");
        let mut t = Text::default();
        status(&mut t, &s);
        let text = t.as_str();
        for want in [
            "  object store  s3 http://minio:9000/wt/prod/, compressed\n",
            "  backup dir    /data/backups\n",
            "  schedule      every 24h0m0s\n",
            "  keep          7 scheduled\n",
            "ID                          CREATED  BY     SIZE     OBJECTS  NOTE\n",
            "bk-20260101T000000Z-abcdef  -        admin  2.0 KiB  12       pre\n",
        ] {
            assert!(text.contains(want), "missing {want:?} in\n{text}");
        }

        let empty: BackupStatus = serde_json::from_value(json!({
            "config": {"interval_seconds": 0, "keep": 0},
            "store": {"kind": "disk", "root": "/data"},
            "supported": false, "backups": null, "last_error": "disk full"
        }))
        .expect("empty");
        let mut t = Text::default();
        status(&mut t, &empty);
        let text = t.as_str();
        assert!(text.contains("  object store  disk /data\n"), "{text}");
        assert!(
            text.contains("  schedule      off (--backup-interval)\n"),
            "{text}"
        );
        assert!(text.contains("  keep          all\n"), "{text}");
        assert!(text.contains("  last error    disk full\n"), "{text}");
        assert!(text.contains("no (this server's stores"), "{text}");
        assert!(text.ends_with("No backups yet. Create one with `wt admin backup create`.\n"));
    }

    #[test]
    fn maps_error_responses() {
        let e = api_error(
            "http://s",
            409,
            br#"{"error":{"code":"backup_running","message":"a backup is already running"}}"#,
        );
        assert!(
            e.to_string()
                .contains("a backup is already running (backup_running, HTTP 409)"),
            "{e}"
        );
        assert!(api_error("http://s", 500, b"oops")
            .to_string()
            .contains("HTTP 500"));
        assert_eq!(backup_path("bk-1 x"), "/api/v1/admin/backups/bk-1%20x");
    }
}
