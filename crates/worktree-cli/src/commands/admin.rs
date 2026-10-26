//! `wt admin` — server administration: accounts, system information,
//! statistics (server admins only).

use clap::{Args, Subcommand};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map};
use worktree_sdk::remote::rest::seg;

use crate::commands::server_common::{
    bytes, decode, fields, new_password, nullable, render, tenant_path, timestamp, when, yes_no,
    Api, Cell, Table,
};
use crate::context::Context;
use crate::error::{CliError, Result};
use crate::output::format::date;
use crate::output::Text;

// storage-backends (§15.2): `wt admin backup ...`.
#[path = "admin_backup.rs"]
mod backup;

// --- server-ops (§16.11): `wt admin outbox ...`, `wt admin retention ...` ---
#[path = "admin_outbox.rs"]
mod outbox;
#[path = "admin_retention.rs"]
mod retention;
// --- end server-ops ---

#[derive(Debug, Args)]
pub struct AdminArgs {
    #[command(subcommand)]
    pub command: AdminCommand,
}

#[derive(Debug, Subcommand)]
pub enum AdminCommand {
    /// List every account on the server
    Accounts,
    /// Create an account (bypasses the registration setting)
    AccountCreate {
        username: String,
        /// Email address
        #[arg(long, value_name = "EMAIL")]
        email: String,
        /// Make the account a server admin
        #[arg(long)]
        admin: bool,
        /// Read the password from the first line of stdin
        #[arg(long)]
        password_stdin: bool,
    },
    /// Change an account's admin or disabled flag
    AccountSet {
        username: String,
        /// Grant or revoke server admin
        #[arg(long, value_name = "BOOL")]
        admin: Option<bool>,
        /// Disable or re-enable the account
        #[arg(long, value_name = "BOOL")]
        disabled: Option<bool>,
    },
    /// Set an account's password (revokes its tokens)
    Password {
        username: String,
        /// Read the password from the first line of stdin
        #[arg(long)]
        password_stdin: bool,
    },
    /// Issue a one-time password reset code for an account
    ResetCode {
        username: String,
        /// Hours until the code expires (server default: 1)
        #[arg(long, value_name = "N")]
        ttl_hours: Option<u32>,
    },
    /// Show server version, runtime and storage information
    System,
    /// Show object counts across the server
    Stats,
    /// Run server garbage collection now: mark unreachable objects, delete
    /// those past the grace period, expire staged snapshots and audit events
    Gc {
        /// Report what would be marked and deleted without changing anything
        #[arg(long)]
        dry_run: bool,
    },
    /// Show the garbage-collection configuration and recent runs
    GcStatus,
    // --- storage (§14.6) ---
    /// Verify the server's object store: re-hash every object and check
    /// that every snapshot reachable from a branch or tag has its manifest,
    /// blobs and chunks (exits 1 on problems)
    Fsck,
    // --- end storage ---
    // --- storage-backends (§15.2) ---
    /// Create, list, download and delete server backups (restore offline
    /// with `worktree-server restore --data DIR <FILE|ID>`)
    Backup(backup::BackupArgs),
    // --- end storage-backends ---
    // --- server-ops (§16.11) ---
    /// The durable email outbox: list queued, sent and dead-lettered
    /// emails; retry or delete them
    Outbox(outbox::OutboxArgs),
    /// Show the retention settings (policy versions, audit events, webhook
    /// deliveries, backups) and the last run; `run` prunes now
    Retention(retention::RetentionArgs),
    // --- end server-ops ---
    // --- search-v2 (§15.9) ---
    /// Show the code search index (memory, disk, indexed trees and
    /// branches) or rebuild it (`wt admin code-index rebuild [TENANT/TREE]`)
    CodeIndex(crate::commands::search::CodeIndexArgs),
    // --- end search-v2 ---
    /// Show or change a tenant's quotas (unset limits use the server
    /// defaults; 0 unsets a limit)
    Limits {
        /// Tenant slug
        tenant: String,
        /// Maximum number of trees
        #[arg(long, value_name = "N")]
        max_trees: Option<u32>,
        /// Maximum stored bytes, e.g. 500M, 10GiB or 1TB (K/M/G/T and
        /// KiB/MiB/GiB/TiB are powers of 1024, KB/MB/GB/TB powers of 1000)
        #[arg(long, value_name = "SIZE", value_parser = parse_size)]
        max_storage: Option<u64>,
        /// Maximum number of branches per tree
        #[arg(long, value_name = "N")]
        max_branches: Option<u32>,
        /// Maximum number of members
        #[arg(long, value_name = "N")]
        max_members: Option<u32>,
        /// Requests per minute per principal of the tenant
        #[arg(long, value_name = "N")]
        rate: Option<u32>,
        /// Remove every tenant-specific limit (back to the server defaults)
        #[arg(long, conflicts_with_all = ["max_trees", "max_storage", "max_branches", "max_members", "rate"])]
        clear: bool,
    },
    /// Show a tenant's resource usage against its effective limits
    Usage {
        /// Tenant slug
        tenant: String,
    },
}

/// `domain.GCReport`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct GcReport {
    dry_run: bool,
    scanned: u64,
    reachable: u64,
    unreachable: u64,
    marked: u64,
    deleted: u64,
    freed_bytes: u64,
    staged_deleted: u64,
    audit_pruned: u64,
    started_at: String,
    finished_at: String,
    error: String,
}

/// `GET /api/v1/admin/gc`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct GcStatus {
    config: GcConfig,
    running: bool,
    next_run_at: String,
    #[serde(deserialize_with = "nullable")]
    runs: Vec<GcReport>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct GcConfig {
    interval: String,
    interval_seconds: u64,
    grace: String,
    grace_seconds: u64,
    staged_retention: String,
    staged_retention_seconds: u64,
    audit_retention: String,
    audit_retention_seconds: u64,
}

/// `domain.TenantLimits` (zero = unset).
#[derive(Debug, Default, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
struct TenantLimits {
    #[serde(skip_serializing_if = "is_zero")]
    max_trees: u64,
    #[serde(skip_serializing_if = "is_zero")]
    max_storage_bytes: u64,
    #[serde(skip_serializing_if = "is_zero")]
    max_branches_per_tree: u64,
    #[serde(skip_serializing_if = "is_zero")]
    max_members: u64,
    #[serde(skip_serializing_if = "is_zero")]
    requests_per_minute: u64,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

/// `GET /api/v1/tenants/{t}/usage`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Usage {
    trees: u64,
    branches: u64,
    members: u64,
    storage_bytes: u64,
    limits: TenantLimits,
    effective: TenantLimits,
}

/// The limit flags of `wt admin limits`.
#[derive(Debug, Default)]
struct LimitChanges {
    max_trees: Option<u32>,
    max_storage: Option<u64>,
    max_branches: Option<u32>,
    max_members: Option<u32>,
    rate: Option<u32>,
}

impl LimitChanges {
    fn is_empty(&self) -> bool {
        self.max_trees.is_none()
            && self.max_storage.is_none()
            && self.max_branches.is_none()
            && self.max_members.is_none()
            && self.rate.is_none()
    }

    /// `current` with the given flags applied.
    fn apply(&self, current: &TenantLimits) -> TenantLimits {
        let mut l = current.clone();
        if let Some(n) = self.max_trees {
            l.max_trees = n.into();
        }
        if let Some(n) = self.max_storage {
            l.max_storage_bytes = n;
        }
        if let Some(n) = self.max_branches {
            l.max_branches_per_tree = n.into();
        }
        if let Some(n) = self.max_members {
            l.max_members = n.into();
        }
        if let Some(n) = self.rate {
            l.requests_per_minute = n.into();
        }
        l
    }
}

/// Parse a byte size: plain bytes or a number with a K/M/G/T suffix.
/// `K`, `KiB` (and M/G/T alike) are powers of 1024; `KB`, `MB`, `GB`, `TB`
/// powers of 1000. Case-insensitive; fractions are allowed ("1.5G").
fn parse_size(s: &str) -> std::result::Result<u64, String> {
    let t = s.trim();
    let split = t
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(t.len());
    let (num, unit) = t.split_at(split);
    let unit = unit.trim().to_ascii_lowercase();
    let factor: u64 = match unit.as_str() {
        "" | "b" => 1,
        "k" | "kib" => 1 << 10,
        "m" | "mib" => 1 << 20,
        "g" | "gib" => 1 << 30,
        "t" | "tib" => 1 << 40,
        "kb" => 1_000,
        "mb" => 1_000_000,
        "gb" => 1_000_000_000,
        "tb" => 1_000_000_000_000,
        _ => {
            return Err(format!(
                "unknown size unit {unit:?} (use K, M, G, T, KB, MiB, ...)"
            ))
        }
    };
    if num.is_empty() {
        return Err("expected a number, e.g. 500M or 10GiB".into());
    }
    if let Ok(n) = num.parse::<u64>() {
        return n
            .checked_mul(factor)
            .ok_or_else(|| "size is too large".to_string());
    }
    let f: f64 = num.parse().map_err(|_| format!("not a number: {num:?}"))?;
    let bytes = (f * factor as f64).round();
    if !bytes.is_finite() || bytes >= u64::MAX as f64 {
        return Err("size is too large".into());
    }
    Ok(bytes as u64)
}

/// `domain.Account` as the CLI displays it.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Account {
    pub username: String,
    pub email: String,
    pub is_admin: bool,
    pub disabled: bool,
    pub created_at: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ResetCode {
    code: String,
    expires_at: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct SystemInfo {
    version: String,
    go_version: String,
    os: String,
    arch: String,
    started_at: String,
    uptime_seconds: u64,
    allow_registration: bool,
    data_dir: String,
    num_cpu: u64,
    num_goroutine: u64,
    memory: Memory,
    storage: Storage,
    event_bus: EventBus,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Memory {
    alloc_bytes: u64,
    sys_bytes: u64,
    heap_inuse_bytes: u64,
    gc_runs: u64,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Storage {
    objects: u64,
    object_bytes: u64,
    meta_db_bytes: u64,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct EventBus {
    published: u64,
    delivered: u64,
    dropped: u64,
    subscribers: u64,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ServerStats {
    tenants: u64,
    accounts: u64,
    trees: u64,
    branches: u64,
    snapshots: u64,
    staged: u64,
    objects: u64,
    storage_bytes: u64,
}

const ACCOUNTS: &str = "/api/v1/admin/accounts";

fn account_path(username: &str) -> String {
    format!("{ACCOUNTS}/{}", seg(username))
}

pub fn run(ctx: &Context, args: AdminArgs) -> Result<()> {
    let api = Api::connect(ctx)?;
    let out = &ctx.out;
    match args.command {
        AdminCommand::Accounts => {
            let value = api.get(ACCOUNTS)?;
            render(out, &value, |t, accounts: Vec<Account>| {
                account_table(t, &accounts);
            })
        }
        AdminCommand::AccountCreate {
            username,
            email,
            admin,
            password_stdin,
        } => {
            let password = new_password(password_stdin)?;
            let body = json!({
                "username": username,
                "email": email,
                "password": password,
                "is_admin": admin,
            });
            let value = api.post(ACCOUNTS, &body)?;
            render(out, &value, |t, account: Account| {
                let role = if account.is_admin {
                    " (server admin)"
                } else {
                    ""
                };
                t.line(format_args!(
                    "Created account {}{role}",
                    account.username.green().bold()
                ));
            })
        }
        AdminCommand::AccountSet {
            username,
            admin,
            disabled,
        } => {
            let mut body = Map::new();
            if let Some(admin) = admin {
                body.insert("is_admin".into(), admin.into());
            }
            if let Some(disabled) = disabled {
                body.insert("disabled".into(), disabled.into());
            }
            if body.is_empty() {
                return Err(CliError::usage("nothing to change")
                    .with_hint("pass --admin true|false and/or --disabled true|false"));
            }
            let value = api.patch(&account_path(&username), &body)?;
            render(out, &value, |t, account: Account| {
                t.line(format_args!(
                    "Updated {}: admin {}, disabled {}",
                    account.username.bold(),
                    yes_no(account.is_admin),
                    yes_no(account.disabled)
                ));
            })
        }
        AdminCommand::Password {
            username,
            password_stdin,
        } => {
            let password = new_password(password_stdin)?;
            api.post(
                &format!("{}/password", account_path(&username)),
                &json!({ "password": password }),
            )?;
            out.emit(
                &json!({ "username": username, "password_changed": true }),
                |t| {
                    t.line(format_args!(
                        "Changed the password of {}; its tokens were revoked",
                        username.bold()
                    ));
                },
            )
        }
        AdminCommand::ResetCode {
            username,
            ttl_hours,
        } => {
            let mut body = Map::new();
            if let Some(ttl) = ttl_hours {
                body.insert("ttl_hours".into(), ttl.into());
            }
            let value = api.post(&format!("{}/reset-code", account_path(&username)), &body)?;
            render(out, &value, |t, reset: ResetCode| {
                t.line(format_args!(
                    "Password reset code for {}: {}",
                    username.bold(),
                    reset.code.yellow().bold()
                ));
                t.line(format_args!("expires {}", when(&reset.expires_at)));
                t.line(format!("redeem it with `wt password-reset {}`", reset.code).dimmed());
            })
        }
        AdminCommand::System => {
            let value = api.get("/api/v1/admin/system")?;
            render(out, &value, |t, info: SystemInfo| system(t, &info))
        }
        AdminCommand::Stats => {
            let value = api.get("/api/v1/admin/stats")?;
            render(out, &value, |t, s: ServerStats| {
                fields(
                    t,
                    &[
                        ("tenants", s.tenants.to_string()),
                        ("accounts", s.accounts.to_string()),
                        ("trees", s.trees.to_string()),
                        ("branches", s.branches.to_string()),
                        ("snapshots", s.snapshots.to_string()),
                        ("staged", s.staged.to_string()),
                        ("objects", s.objects.to_string()),
                        ("storage", bytes(s.storage_bytes)),
                    ],
                );
            })
        }
        AdminCommand::Gc { dry_run } => {
            let path = format!("{GC}?dry_run={dry_run}");
            let value = api.post(&path, &json!({}))?;
            render(out, &value, |t, report: GcReport| gc_report(t, &report))
        }
        AdminCommand::GcStatus => {
            let value = api.get(GC)?;
            render(out, &value, |t, status: GcStatus| gc_status(t, &status))
        }
        // --- storage (§14.6) ---
        AdminCommand::Fsck => {
            let value = api.post(FSCK, &json!({}))?;
            let report: FsckReport = decode(&value)?;
            render(out, &value, |t, r: FsckReport| fsck_report(t, &r))?;
            if report.is_ok() {
                Ok(())
            } else {
                Err(CliError::failure(format!(
                    "server fsck found {} corrupt and {} missing objects, {} missing snapshots",
                    report.corrupt.len(),
                    report.missing.len(),
                    report.missing_snapshots.len()
                )))
            }
        }
        // --- end storage ---
        AdminCommand::Backup(args) => backup::run(ctx, &api, args), // storage-backends (§15.2)
        // --- server-ops (§16.11) ---
        AdminCommand::Outbox(args) => outbox::run(ctx, &api, args),
        AdminCommand::Retention(args) => retention::run(ctx, &api, args),
        // --- end server-ops ---
        // --- search-v2 (§15.9) ---
        AdminCommand::CodeIndex(args) => crate::commands::search::run_code_index(&api, out, args),
        // --- end search-v2 ---
        AdminCommand::Limits {
            tenant,
            max_trees,
            max_storage,
            max_branches,
            max_members,
            rate,
            clear,
        } => {
            let path = limits_path(&tenant);
            let changes = LimitChanges {
                max_trees,
                max_storage,
                max_branches,
                max_members,
                rate,
            };
            let value = if clear {
                api.put(&path, &json!({}))?
            } else if changes.is_empty() {
                api.get(&path)?
            } else {
                let current: TenantLimits = decode(&api.get(&path)?)?;
                api.put(&path, &changes.apply(&current))?
            };
            let changed = clear || !changes.is_empty();
            render(out, &value, |t, limits: TenantLimits| {
                if changed {
                    t.line(format_args!("Updated the limits of {}", tenant.bold()));
                }
                limits_fields(t, &limits);
            })
        }
        AdminCommand::Usage { tenant } => {
            let value = api.get(&format!("{}/usage", tenant_path(&tenant)))?;
            render(out, &value, |t, usage: Usage| usage_fields(t, &usage))
        }
    }
}

const GC: &str = "/api/v1/admin/gc";

// --- storage (§14.6): `wt admin fsck` ---

const FSCK: &str = "/api/v1/admin/fsck";

/// `POST /api/v1/admin/fsck`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct FsckReport {
    objects_checked: u64,
    snapshots_checked: u64,
    corrupt: Vec<String>,
    missing: Vec<FsckMissing>,
    missing_snapshots: Vec<String>,
    duration_ms: u64,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct FsckMissing {
    snapshot: String,
    object: String,
}

impl FsckReport {
    fn is_ok(&self) -> bool {
        self.corrupt.is_empty() && self.missing.is_empty() && self.missing_snapshots.is_empty()
    }
}

fn fsck_report(t: &mut Text, r: &FsckReport) {
    for h in &r.corrupt {
        t.line(format_args!("{} {h}", "corrupt object:".red().bold()));
    }
    for m in &r.missing {
        t.line(format_args!(
            "{} {} (snapshot {})",
            "missing object:".red().bold(),
            m.object,
            m.snapshot
        ));
    }
    for s in &r.missing_snapshots {
        t.line(format_args!("{} {s}", "missing snapshot:".red().bold()));
    }
    if !r.is_ok() {
        t.blank();
    }
    fields(
        t,
        &[
            ("objects checked", r.objects_checked.to_string()),
            ("snapshots", r.snapshots_checked.to_string()),
            (
                "duration",
                format!("{}.{}s", r.duration_ms / 1000, r.duration_ms % 1000 / 100),
            ),
        ],
    );
    if r.is_ok() {
        t.line("No problems found".green());
    }
}

// --- end storage ---

/// `/api/v1/admin/tenants/{slug}/limits`.
fn limits_path(tenant: &str) -> String {
    format!("/api/v1/admin/tenants/{}/limits", seg(tenant))
}

/// Seconds between two server timestamps ("1.2s"), "-" when unknown.
fn duration(start: &str, end: &str) -> String {
    match (timestamp(start), timestamp(end)) {
        (Some(s), Some(e)) if e >= s => {
            let ms = (e - s).num_milliseconds();
            format!("{}.{}s", ms / 1000, ms % 1000 / 100)
        }
        _ => "-".into(),
    }
}

fn gc_report(t: &mut Text, r: &GcReport) {
    if r.dry_run {
        t.line("Dry run: nothing was marked or deleted".yellow());
    }
    let mut rows = vec![
        ("dry run", yes_no(r.dry_run).to_string()),
        ("scanned", r.scanned.to_string()),
        ("reachable", r.reachable.to_string()),
        ("unreachable", r.unreachable.to_string()),
        ("newly marked", r.marked.to_string()),
        ("deleted", r.deleted.to_string()),
        ("freed", bytes(r.freed_bytes)),
        ("staged deleted", r.staged_deleted.to_string()),
        ("audit pruned", r.audit_pruned.to_string()),
        ("started", when(&r.started_at)),
        ("duration", duration(&r.started_at, &r.finished_at)),
    ];
    if !r.error.is_empty() {
        rows.push(("error", r.error.red().to_string()));
    }
    fields(t, &rows);
}

/// A retention or interval setting; `zero` names what 0 means.
fn period(text: &str, seconds: u64, zero: &str) -> String {
    if seconds == 0 {
        zero.to_string()
    } else if text.is_empty() {
        format!("{seconds}s")
    } else {
        text.to_string()
    }
}

fn gc_status(t: &mut Text, s: &GcStatus) {
    let c = &s.config;
    let next = timestamp(&s.next_run_at).map_or_else(|| "-".into(), date);
    fields(
        t,
        &[
            (
                "interval",
                period(&c.interval, c.interval_seconds, "disabled"),
            ),
            ("grace", period(&c.grace, c.grace_seconds, "none")),
            (
                "staged retention",
                period(
                    &c.staged_retention,
                    c.staged_retention_seconds,
                    "keep forever",
                ),
            ),
            (
                "audit retention",
                period(
                    &c.audit_retention,
                    c.audit_retention_seconds,
                    "keep forever",
                ),
            ),
            ("running", yes_no(s.running).into()),
            ("next run", next),
        ],
    );
    t.blank();
    let mut table = Table::new(&[
        "STARTED",
        "MODE",
        "SCANNED",
        "UNREACHABLE",
        "MARKED",
        "DELETED",
        "FREED",
    ]);
    for r in &s.runs {
        let mode = if r.dry_run {
            Cell::from("dry-run").yellow()
        } else {
            Cell::from("live")
        };
        table.row([
            when(&r.started_at).into(),
            mode,
            r.scanned.to_string().into(),
            r.unreachable.to_string().into(),
            r.marked.to_string().into(),
            r.deleted.to_string().into(),
            bytes(r.freed_bytes).into(),
        ]);
    }
    table.render(t, "No garbage-collection runs yet.");
}

/// A count limit; 0 = `zero`.
fn limit(n: u64, zero: &str) -> String {
    if n == 0 {
        zero.into()
    } else {
        n.to_string()
    }
}

fn limits_fields(t: &mut Text, l: &TenantLimits) {
    const DEFAULT: &str = "server default";
    let storage = if l.max_storage_bytes == 0 {
        DEFAULT.into()
    } else {
        bytes(l.max_storage_bytes)
    };
    fields(
        t,
        &[
            ("max trees", limit(l.max_trees, DEFAULT)),
            ("max storage", storage),
            ("max branches/tree", limit(l.max_branches_per_tree, DEFAULT)),
            ("max members", limit(l.max_members, DEFAULT)),
            ("requests/minute", limit(l.requests_per_minute, DEFAULT)),
        ],
    );
}

fn usage_fields(t: &mut Text, u: &Usage) {
    const UNLIMITED: &str = "unlimited";
    let e = &u.effective;
    let used = |n: u64, max: u64| format!("{n} / {}", limit(max, UNLIMITED));
    let storage = if e.max_storage_bytes == 0 {
        format!("{} / {UNLIMITED}", bytes(u.storage_bytes))
    } else {
        format!(
            "{} / {}",
            bytes(u.storage_bytes),
            bytes(e.max_storage_bytes)
        )
    };
    fields(
        t,
        &[
            ("trees", used(u.trees, e.max_trees)),
            ("branches", u.branches.to_string()),
            (
                "max branches/tree",
                limit(e.max_branches_per_tree, UNLIMITED),
            ),
            ("members", used(u.members, e.max_members)),
            ("storage", storage),
            ("requests/minute", limit(e.requests_per_minute, UNLIMITED)),
        ],
    );
    if u.limits != TenantLimits::default() {
        t.line("  (tenant-specific limits override the server defaults)".dimmed());
    }
}

/// USER EMAIL ADMIN DISABLED CREATED.
pub fn account_table(t: &mut Text, accounts: &[Account]) {
    let mut table = Table::new(&["USER", "EMAIL", "ADMIN", "DISABLED", "CREATED"]);
    for a in accounts {
        let admin = Cell::from(yes_no(a.is_admin));
        let disabled = Cell::from(yes_no(a.disabled));
        table.row([
            Cell::from(a.username.as_str()).bold(),
            a.email.as_str().into(),
            if a.is_admin { admin.yellow() } else { admin },
            if a.disabled { disabled.red() } else { disabled },
            when(&a.created_at).into(),
        ]);
    }
    table.render(t, "No accounts.");
}

fn system(t: &mut Text, info: &SystemInfo) {
    let started = timestamp(&info.started_at).map_or_else(|| "-".into(), date);
    fields(
        t,
        &[
            ("version", info.version.clone()),
            ("go", info.go_version.clone()),
            ("platform", format!("{}/{}", info.os, info.arch)),
            ("started", started),
            ("uptime", uptime(info.uptime_seconds)),
            ("registration", yes_no(info.allow_registration).into()),
            ("data dir", info.data_dir.clone()),
            ("cpus", info.num_cpu.to_string()),
            ("goroutines", info.num_goroutine.to_string()),
            (
                "memory",
                format!(
                    "{} allocated, {} heap in use, {} from the OS, {} GC runs",
                    bytes(info.memory.alloc_bytes),
                    bytes(info.memory.heap_inuse_bytes),
                    bytes(info.memory.sys_bytes),
                    info.memory.gc_runs
                ),
            ),
            (
                "storage",
                format!(
                    "{} objects ({}), metadata {}",
                    info.storage.objects,
                    bytes(info.storage.object_bytes),
                    bytes(info.storage.meta_db_bytes)
                ),
            ),
            (
                "event bus",
                format!(
                    "{} published, {} delivered, {} dropped, {} subscribers",
                    info.event_bus.published,
                    info.event_bus.delivered,
                    info.event_bus.dropped,
                    info.event_bus.subscribers
                ),
            ),
        ],
    );
}

/// "3d 4h 5m", "12m 30s".
fn uptime(secs: u64) -> String {
    let (d, h, m, s) = (secs / 86_400, secs / 3600 % 24, secs / 60 % 60, secs % 60);
    match (d, h) {
        (0, 0) => format!("{m}m {s}s"),
        (0, _) => format!("{h}h {m}m"),
        _ => format!("{d}d {h}h {m}m"),
    }
}

#[cfg(test)]
mod tests {
    use clap::error::ErrorKind;

    use super::*;
    use crate::cli::Command;
    use crate::commands::server_common::testing::{parse, parse_err};

    fn admin(args: &[&str]) -> AdminCommand {
        let mut full = vec!["admin"];
        full.extend_from_slice(args);
        match parse(&full) {
            Command::Admin(a) => a.command,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn parses_every_subcommand() {
        assert!(matches!(admin(&["accounts"]), AdminCommand::Accounts));
        assert!(matches!(admin(&["system"]), AdminCommand::System));
        assert!(matches!(admin(&["stats"]), AdminCommand::Stats));
        let AdminCommand::AccountCreate {
            username,
            email,
            admin: is_admin,
            password_stdin,
        } = admin(&[
            "account-create",
            "bob",
            "--email",
            "bob@example.com",
            "--admin",
            "--password-stdin",
        ])
        else {
            panic!()
        };
        assert_eq!(
            (username.as_str(), email.as_str()),
            ("bob", "bob@example.com")
        );
        assert!(is_admin && password_stdin);
        let AdminCommand::AccountSet {
            username,
            admin: is_admin,
            disabled,
        } = admin(&[
            "account-set",
            "bob",
            "--admin",
            "false",
            "--disabled",
            "true",
        ])
        else {
            panic!()
        };
        assert_eq!(
            (username.as_str(), is_admin, disabled),
            ("bob", Some(false), Some(true))
        );
        assert!(matches!(
            admin(&["password", "bob", "--password-stdin"]),
            AdminCommand::Password {
                password_stdin: true,
                ..
            }
        ));
        assert!(matches!(
            admin(&["reset-code", "bob", "--ttl-hours", "2"]),
            AdminCommand::ResetCode {
                ttl_hours: Some(2),
                ..
            }
        ));
        assert_eq!(
            parse_err(&["admin", "account-create", "bob"]),
            ErrorKind::MissingRequiredArgument
        );
        assert_eq!(
            parse_err(&["admin", "account-set", "bob", "--admin", "maybe"]),
            ErrorKind::InvalidValue
        );
    }

    #[test]
    fn parses_ops_subcommands() {
        assert!(matches!(
            admin(&["gc"]),
            AdminCommand::Gc { dry_run: false }
        ));
        assert!(matches!(
            admin(&["gc", "--dry-run"]),
            AdminCommand::Gc { dry_run: true }
        ));
        assert!(matches!(admin(&["gc-status"]), AdminCommand::GcStatus));
        // storage (§14.6)
        assert!(matches!(admin(&["fsck"]), AdminCommand::Fsck));
        let AdminCommand::Limits {
            tenant,
            max_trees,
            max_storage,
            max_branches,
            max_members,
            rate,
            clear,
        } = admin(&[
            "limits",
            "acme",
            "--max-trees",
            "10",
            "--max-storage",
            "5GiB",
            "--max-branches",
            "50",
            "--max-members",
            "0",
            "--rate",
            "600",
        ])
        else {
            panic!()
        };
        assert_eq!(tenant, "acme");
        assert_eq!(
            (
                max_trees,
                max_storage,
                max_branches,
                max_members,
                rate,
                clear
            ),
            (Some(10), Some(5 << 30), Some(50), Some(0), Some(600), false)
        );
        assert!(matches!(
            admin(&["limits", "acme", "--clear"]),
            AdminCommand::Limits { clear: true, .. }
        ));
        assert!(matches!(
            admin(&["limits", "acme"]),
            AdminCommand::Limits {
                max_trees: None,
                clear: false,
                ..
            }
        ));
        assert_eq!(
            parse_err(&["admin", "limits", "acme", "--clear", "--max-trees", "3"]),
            ErrorKind::ArgumentConflict
        );
        assert_eq!(
            parse_err(&["admin", "limits", "acme", "--max-storage", "lots"]),
            ErrorKind::ValueValidation
        );
        assert_eq!(
            parse_err(&["admin", "limits"]),
            ErrorKind::MissingRequiredArgument
        );
        let AdminCommand::Usage { tenant } = admin(&["usage", "acme"]) else {
            panic!()
        };
        assert_eq!(tenant, "acme");
        assert_eq!(limits_path("a b"), "/api/v1/admin/tenants/a%20b/limits");
    }

    #[test]
    fn parses_sizes() {
        for (input, want) in [
            ("0", 0),
            ("1024", 1024),
            ("1K", 1024),
            ("1k", 1024),
            ("1KiB", 1024),
            ("1KB", 1000),
            ("2M", 2 << 20),
            ("2MB", 2_000_000),
            ("2 MiB", 2 << 20),
            ("3G", 3 << 30),
            ("3gb", 3_000_000_000),
            ("1T", 1 << 40),
            ("1TB", 1_000_000_000_000),
            ("1.5G", 3 << 29),
            ("10b", 10),
        ] {
            assert_eq!(parse_size(input), Ok(want), "{input}");
        }
        for bad in ["", "G", "1X", "1..5M", "-1", "99999999999T"] {
            assert!(parse_size(bad).is_err(), "{bad} should not parse");
        }
    }

    #[test]
    fn limit_changes_overlay_the_current_limits() {
        let current = TenantLimits {
            max_trees: 5,
            max_members: 20,
            ..TenantLimits::default()
        };
        let changes = LimitChanges {
            max_trees: Some(0),
            max_storage: Some(1 << 30),
            ..LimitChanges::default()
        };
        assert!(!changes.is_empty());
        assert!(LimitChanges::default().is_empty());
        let next = changes.apply(&current);
        assert_eq!(
            serde_json::to_value(&next).expect("json"),
            json!({"max_storage_bytes": 1 << 30, "max_members": 20})
        );
    }

    #[test]
    fn renders_gc_reports_and_status() {
        colored::control::set_override(false);
        let report: GcReport = serde_json::from_value(json!({
            "dry_run": true, "scanned": 10, "reachable": 7, "unreachable": 3,
            "marked": 2, "deleted": 1, "freed_bytes": 2048, "staged_deleted": 4,
            "audit_pruned": 5, "started_at": "2026-01-01T00:00:00Z",
            "finished_at": "2026-01-01T00:00:01.5Z"
        }))
        .expect("report");
        let mut t = Text::default();
        gc_report(&mut t, &report);
        let text = t.as_str();
        assert!(
            text.starts_with("Dry run: nothing was marked or deleted\n"),
            "{text}"
        );
        for want in [
            "  scanned         10\n",
            "  unreachable     3\n",
            "  newly marked    2\n",
            "  freed           2.0 KiB\n",
            "  staged deleted  4\n",
            "  audit pruned    5\n",
            "  duration        1.5s\n",
        ] {
            assert!(text.contains(want), "missing {want:?} in\n{text}");
        }

        let status: GcStatus = serde_json::from_value(json!({
            "config": {"interval": "24h0m0s", "interval_seconds": 86400,
                       "grace": "720h0m0s", "grace_seconds": 2592000,
                       "staged_retention": "0s", "staged_retention_seconds": 0,
                       "audit_retention": "0s", "audit_retention_seconds": 0},
            "running": false,
            "runs": [{"dry_run": false, "scanned": 9, "unreachable": 1, "marked": 1,
                      "deleted": 0, "freed_bytes": 0, "started_at": ""}]
        }))
        .expect("status");
        let mut t = Text::default();
        gc_status(&mut t, &status);
        assert_eq!(
            t.as_str(),
            "  interval          24h0m0s\n  grace             720h0m0s\n  staged retention  keep forever\n  audit retention   keep forever\n  running           no\n  next run          -\n\nSTARTED  MODE  SCANNED  UNREACHABLE  MARKED  DELETED  FREED\n-        live  9        1            1       0        0 B\n"
        );
        let empty: GcStatus = serde_json::from_value(json!({
            "config": {"interval_seconds": 0}, "runs": null
        }))
        .expect("empty status");
        let mut t = Text::default();
        gc_status(&mut t, &empty);
        assert!(t.as_str().contains("  interval          disabled\n"));
        assert!(t.as_str().ends_with("No garbage-collection runs yet.\n"));
    }

    #[test]
    fn renders_limits_and_usage() {
        colored::control::set_override(false);
        let mut t = Text::default();
        limits_fields(
            &mut t,
            &TenantLimits {
                max_trees: 3,
                max_storage_bytes: 1 << 30,
                ..TenantLimits::default()
            },
        );
        assert_eq!(
            t.as_str(),
            "  max trees          3\n  max storage        1.0 GiB\n  max branches/tree  server default\n  max members        server default\n  requests/minute    server default\n"
        );
        let usage: Usage = serde_json::from_value(json!({
            "trees": 2, "branches": 5, "members": 4, "storage_bytes": 1536,
            "limits": {"max_trees": 3},
            "effective": {"max_trees": 3, "max_members": 10, "requests_per_minute": 1200}
        }))
        .expect("usage");
        let mut t = Text::default();
        usage_fields(&mut t, &usage);
        assert_eq!(
            t.as_str(),
            "  trees              2 / 3\n  branches           5\n  max branches/tree  unlimited\n  members            4 / 10\n  storage            1.5 KiB / unlimited\n  requests/minute    1200\n  (tenant-specific limits override the server defaults)\n"
        );
    }

    // storage (§14.6)
    #[test]
    fn renders_fsck_reports() {
        colored::control::set_override(false);
        let bad: FsckReport = serde_json::from_value(json!({
            "objects_checked": 12, "snapshots_checked": 3, "duration_ms": 1530,
            "corrupt": ["aa"], "missing": [{"snapshot": "s1", "object": "bb"}],
            "missing_snapshots": ["s9"]
        }))
        .expect("report");
        assert!(!bad.is_ok());
        let mut t = Text::default();
        fsck_report(&mut t, &bad);
        assert_eq!(
            t.as_str(),
            "corrupt object: aa\nmissing object: bb (snapshot s1)\nmissing snapshot: s9\n\n  objects checked  12\n  snapshots        3\n  duration         1.5s\n"
        );
        let good: FsckReport =
            serde_json::from_value(json!({"objects_checked": 1, "corrupt": [], "missing": []}))
                .expect("report");
        assert!(good.is_ok());
        let mut t = Text::default();
        fsck_report(&mut t, &good);
        assert!(t.as_str().ends_with("No problems found\n"));
    }

    // storage-backends (§15.2)
    #[test]
    fn parses_backup_subcommands() {
        use backup::BackupCommand as B;
        let sub = |args: &[&str]| match admin(args) {
            AdminCommand::Backup(b) => b.command,
            other => panic!("{other:?}"),
        };
        assert!(matches!(
            sub(&["backup", "create"]),
            B::Create { note: None }
        ));
        let B::Create { note } = sub(&["backup", "create", "--note", "pre-upgrade"]) else {
            panic!()
        };
        assert_eq!(note.as_deref(), Some("pre-upgrade"));
        assert!(matches!(sub(&["backup", "list"]), B::List));
        let B::Download { id, output } = sub(&["backup", "download", "bk-1", "-o", "x.wtbak"])
        else {
            panic!()
        };
        assert_eq!(
            (id.as_str(), output.as_deref()),
            ("bk-1", Some(std::path::Path::new("x.wtbak")))
        );
        assert!(matches!(
            sub(&["backup", "download", "bk-1"]),
            B::Download { output: None, .. }
        ));
        assert!(matches!(
            sub(&["backup", "delete", "bk-1", "--yes"]),
            B::Delete { yes: true, .. }
        ));
        assert_eq!(
            parse_err(&["admin", "backup", "download"]),
            ErrorKind::MissingRequiredArgument
        );
        assert_eq!(
            parse_err(&["admin", "backup", "restore"]),
            ErrorKind::InvalidSubcommand
        );
    }

    // --- server-ops (§16.11) ---
    #[test]
    fn parses_outbox_and_retention_subcommands() {
        use outbox::{EmailStatus, OutboxCommand as O};
        use retention::RetentionCommand as R;
        let ob = |args: &[&str]| match admin(args) {
            AdminCommand::Outbox(o) => o.command,
            other => panic!("{other:?}"),
        };
        assert!(matches!(
            ob(&["outbox", "list"]),
            O::List {
                status: None,
                limit: None
            }
        ));
        assert!(matches!(
            ob(&["outbox", "list", "--status", "dead", "--limit", "5"]),
            O::List {
                status: Some(EmailStatus::Dead),
                limit: Some(5)
            }
        ));
        let O::Retry { id, all } = ob(&["outbox", "retry", "em-1"]) else {
            panic!()
        };
        assert_eq!((id.as_deref(), all), (Some("em-1"), false));
        assert!(matches!(
            ob(&["outbox", "retry", "--all"]),
            O::Retry {
                id: None,
                all: true
            }
        ));
        assert!(matches!(
            ob(&["outbox", "delete", "em-1", "-y"]),
            O::Delete { yes: true, .. }
        ));
        assert_eq!(
            parse_err(&["admin", "outbox", "retry"]),
            ErrorKind::MissingRequiredArgument
        );
        assert_eq!(
            parse_err(&["admin", "outbox", "retry", "em-1", "--all"]),
            ErrorKind::ArgumentConflict
        );
        assert_eq!(
            parse_err(&["admin", "outbox", "list", "--status", "lost"]),
            ErrorKind::InvalidValue
        );
        assert_eq!(
            parse_err(&["admin", "outbox", "delete"]),
            ErrorKind::MissingRequiredArgument
        );

        let rt = |args: &[&str]| match admin(args) {
            AdminCommand::Retention(r) => r.command,
            other => panic!("{other:?}"),
        };
        assert!(rt(&["retention"]).is_none());
        assert!(matches!(rt(&["retention", "run"]), Some(R::Run)));
        assert_eq!(
            parse_err(&["admin", "retention", "prune"]),
            ErrorKind::InvalidSubcommand
        );
    }
    // --- end server-ops ---

    #[test]
    fn uptimes() {
        assert_eq!(uptime(75), "1m 15s");
        assert_eq!(uptime(3 * 3600 + 120), "3h 2m");
        assert_eq!(uptime(2 * 86_400 + 3600), "2d 1h 0m");
    }

    #[test]
    fn account_tables() {
        colored::control::set_override(false);
        let mut t = Text::default();
        account_table(
            &mut t,
            &[Account {
                username: "admin".into(),
                email: "a@x".into(),
                is_admin: true,
                ..Account::default()
            }],
        );
        assert_eq!(
            t.as_str(),
            "USER   EMAIL  ADMIN  DISABLED  CREATED\nadmin  a@x    yes    no        -\n"
        );
    }
}
