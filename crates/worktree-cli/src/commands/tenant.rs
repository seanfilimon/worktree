//! `wt tenant` — tenants (organizations) and their members on the server,
//! plus the v0.8 lifecycle (§16.4): contact email and verification, slug
//! renames, deletion with a recovery window, restore and `wt tenant switch`.

use clap::{Args, Subcommand, ValueEnum};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use serde_json::json;
use worktree_sdk::remote::rest::seg;

use crate::commands::admin::Account;
use crate::commands::remote::remote_name;
use crate::commands::server_common::{
    confirm, fields, render, tenant_path, when, Api, Cell, Table,
};
use crate::context::Context;
use crate::error::{CliError, Result};
use crate::output::Text;

#[derive(Debug, Args)]
pub struct TenantArgs {
    #[command(subcommand)]
    pub command: TenantCommand,
}

#[derive(Debug, Subcommand)]
pub enum TenantCommand {
    /// List the tenants you can see
    List,
    /// Create an organization tenant (you become its owner)
    Create {
        slug: String,
        /// Display name (default: the slug)
        #[arg(long, value_name = "N")]
        name: Option<String>,
    },
    /// Show a tenant
    Show { tenant: String },
    /// Change a tenant's slug (owners). The old slug stays reserved for 90
    /// days and answers "tenant moved" with the new slug; update worktree
    /// remotes with `wt tenant switch NEW_SLUG`
    Rename {
        tenant: String,
        /// The new slug (lowercase letters, digits and '-')
        new_slug: String,
    },
    /// Change a tenant's display name
    SetName { tenant: String, name: String },
    /// Change a tenant's plan (server admins only)
    Plan { tenant: String, plan: Plan },
    /// Delete an organization tenant (owners). It is suspended at once and
    /// purged after a 30-day recovery window; `wt tenant restore` cancels it
    Delete {
        tenant: String,
        /// Do not ask for confirmation
        #[arg(long, short = 'y')]
        yes: bool,
    },
    /// Cancel a scheduled deletion during the recovery window (owners)
    Restore { tenant: String },
    /// The tenant's contact email and its verification
    Email(EmailArgs),
    /// Point the current worktree remote at another tenant (sets
    /// `remote.<name>.tenant` in .wt/config.toml), e.g. after a rename
    Switch {
        slug: String,
        /// Remote to change (default: [sync].remote, usually origin)
        #[arg(long, value_name = "NAME")]
        remote: Option<String>,
    },
    /// List a tenant's members
    Members { tenant: String },
    /// Add a member to a tenant
    AddMember {
        tenant: String,
        username: String,
        /// Role of the new member
        #[arg(long, value_name = "R", default_value = "developer")]
        role: String,
    },
    /// Change a member's role
    SetRole {
        tenant: String,
        username: String,
        role: String,
    },
    /// Remove a member from a tenant
    RemoveMember { tenant: String, username: String },
    /// Suspend a tenant: it becomes read-only and its service-account keys
    /// are revoked (server admins only)
    Suspend {
        tenant: String,
        /// Why, shown to members whose writes are refused
        #[arg(long, value_name = "TEXT")]
        reason: Option<String>,
    },
    /// Lift a suspension or deactivation (server admins only)
    Resume { tenant: String },
    /// Deactivate a tenant: all access is refused (server admins only)
    Deactivate {
        tenant: String,
        /// Why, shown to members whose requests are refused
        #[arg(long, value_name = "TEXT")]
        reason: Option<String>,
    },
}

#[derive(Debug, Args)]
pub struct EmailArgs {
    #[command(subcommand)]
    pub command: EmailCommand,
}

#[derive(Debug, Subcommand)]
pub enum EmailCommand {
    /// Set the contact email; a single-use verification code (valid 24 h)
    /// is mailed to it (or written to the server log without SMTP)
    Set { tenant: String, email: String },
    /// Verify the contact email with the code from the verification email
    Verify { tenant: String, code: String },
    /// Send a new verification code (earlier codes stop working)
    Resend { tenant: String },
    /// Remove the contact email
    Clear { tenant: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Plan {
    Free,
    Pro,
    Enterprise,
}

/// `domain.Tenant`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Tenant {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub kind: String,
    pub plan: String,
    pub created_at: String,
    /// "" = active, "suspended", "deactivated" (v0.5).
    pub status: String,
    pub status_reason: String,
    /// Contact email and when it was verified (v0.8).
    pub email: String,
    pub email_verified_at: Option<String>,
}

impl Tenant {
    /// The status, with "" spelled out as "active".
    pub fn status_label(&self) -> &str {
        if self.status.is_empty() {
            "active"
        } else {
            &self.status
        }
    }

    /// "ops@acme.io (verified)", "ops@acme.io (unverified)" or "-".
    fn email_label(&self) -> String {
        match (self.email.as_str(), &self.email_verified_at) {
            ("", _) => "-".into(),
            (e, Some(_)) => format!("{e} (verified)"),
            (e, None) => format!("{e} (unverified)"),
        }
    }
}

/// `iam.TenantDeletion`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Deletion {
    slug: String,
    requested_by: String,
    purge_at: String,
    revoked_keys: u64,
}

/// `iam.SlugReservation` inside the rename response.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Renamed {
    tenant: Tenant,
    reservation: Reservation,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Reservation {
    slug: String,
    expires_at: String,
}

/// `iam.VerificationSent`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Sent {
    email: String,
    delivery: String,
    expires_at: String,
}

/// PATCH /admin/tenants/{t} body for `suspend` / `resume` / `deactivate`.
fn status_body(status: &str, reason: Option<String>) -> serde_json::Value {
    match reason {
        Some(r) => json!({ "status": status, "status_reason": r }),
        None if status == "active" => json!({ "status": status, "status_reason": "" }),
        None => json!({ "status": status }),
    }
}

/// Organization slug rules (the server checks them too).
fn valid_slug(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= 63
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
        && b[0] != b'-'
        && b[b.len() - 1] != b'-'
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Member {
    account: Account,
    role: String,
}

pub fn run(ctx: &Context, args: TenantArgs) -> Result<()> {
    // `switch` edits the worktree only and needs no server.
    if let TenantCommand::Switch { slug, remote } = args.command {
        return switch(ctx, &slug, remote);
    }
    let api = Api::connect(ctx)?;
    let out = &ctx.out;
    match args.command {
        TenantCommand::Switch { .. } => unreachable!("handled above"),
        TenantCommand::List => {
            let value = api.get("/api/v1/tenants")?;
            render(out, &value, |t, tenants: Vec<Tenant>| {
                tenant_table(t, &tenants);
            })
        }
        TenantCommand::Create { slug, name } => {
            let body = json!({ "slug": slug, "name": name.unwrap_or_default() });
            let value = api.post("/api/v1/tenants", &body)?;
            render(out, &value, |t, tenant: Tenant| {
                t.line(format_args!(
                    "Created tenant {} ({})",
                    tenant.slug.green().bold(),
                    tenant.name
                ));
            })
        }
        TenantCommand::Show { tenant } => {
            let value = api.get(&tenant_path(&tenant))?;
            let deletion = if value["status"].as_str() == Some("suspended") {
                api.get(&format!("{}/deletion", tenant_path(&tenant))).ok()
            } else {
                None
            };
            let deletion: Option<Deletion> = deletion.and_then(|d| serde_json::from_value(d).ok());
            render(out, &value, |t, tenant: Tenant| {
                show(t, &tenant, deletion.as_ref())
            })
        }
        TenantCommand::Rename { tenant, new_slug } => {
            if !valid_slug(&new_slug) {
                return Err(CliError::usage(format!(
                    "'{new_slug}' is not a valid slug (lowercase letters, digits and '-')"
                ))
                .with_hint(format!(
                    "to change the display name, run `wt tenant set-name {tenant} \"{new_slug}\"`"
                )));
            }
            let path = format!("{}/rename", tenant_path(&tenant));
            let value = api.post(&path, &json!({ "slug": new_slug }))?;
            render(out, &value, |t, r: Renamed| {
                t.line(format_args!(
                    "Renamed {} to {}",
                    r.reservation.slug.bold(),
                    r.tenant.slug.green().bold()
                ));
                t.line(format_args!(
                    "The old slug is reserved until {} and answers \"tenant moved\".",
                    when(&r.reservation.expires_at)
                ));
                t.line(format_args!(
                    "Update worktree remotes with `wt tenant switch {}`.",
                    r.tenant.slug
                ));
            })
        }
        TenantCommand::SetName { tenant, name } => {
            let value = api.patch(&tenant_path(&tenant), &json!({ "name": name }))?;
            render(out, &value, |t, tenant: Tenant| {
                t.line(format_args!(
                    "{} is now called {}",
                    tenant.slug.bold(),
                    tenant.name.green()
                ));
            })
        }
        TenantCommand::Plan { tenant, plan } => {
            let value = api.patch(&tenant_path(&tenant), &json!({ "plan": plan }))?;
            render(out, &value, |t, tenant: Tenant| {
                t.line(format_args!(
                    "{} is now on the {} plan",
                    tenant.slug.bold(),
                    tenant.plan.green()
                ));
            })
        }
        TenantCommand::Delete { tenant, yes } => {
            confirm(
                yes,
                &format!("delete tenant {tenant} (purged after the recovery window)"),
            )?;
            api.delete(&tenant_path(&tenant), &[])?;
            let value = api.get(&format!("{}/deletion", tenant_path(&tenant)))?;
            render(out, &value, |t, d: Deletion| {
                t.line(format_args!(
                    "Tenant {} is scheduled for deletion on {}",
                    d.slug.bold(),
                    when(&d.purge_at).red()
                ));
                t.line("It is read-only until then; `wt tenant restore` cancels the deletion.");
                if d.revoked_keys > 0 {
                    t.line(format_args!(
                        "{} service-account key(s) were revoked.",
                        d.revoked_keys
                    ));
                }
            })
        }
        TenantCommand::Restore { tenant } => {
            let path = format!("{}/restore", tenant_path(&tenant));
            let value = api.post(&path, &json!({}))?;
            render(out, &value, |t, tenant: Tenant| {
                t.line(format_args!(
                    "Restored {} (status {})",
                    tenant.slug.green().bold(),
                    tenant.status_label()
                ));
            })
        }
        TenantCommand::Email(EmailArgs { command }) => email(&api, ctx, command),
        TenantCommand::Members { tenant } => {
            let value = api.get(&format!("{}/members", tenant_path(&tenant)))?;
            render(out, &value, |t, members: Vec<Member>| {
                let mut table = Table::new(&["USER", "EMAIL", "ROLE"]);
                for m in &members {
                    table.row([
                        Cell::from(m.account.username.as_str()).bold(),
                        m.account.email.as_str().into(),
                        Cell::from(m.role.as_str()).cyan(),
                    ]);
                }
                table.render(t, "No members.");
            })
        }
        TenantCommand::AddMember {
            tenant,
            username,
            role,
        }
        | TenantCommand::SetRole {
            tenant,
            username,
            role,
        } => {
            let path = member_path(&tenant, &username);
            let value = api.put(&path, &json!({ "role": role }))?;
            render(out, &value, |t, m: Member| {
                t.line(format_args!(
                    "{}: {} is now {}",
                    tenant.bold(),
                    m.account.username.green().bold(),
                    m.role.cyan()
                ));
            })
        }
        TenantCommand::Suspend { tenant, reason } => {
            set_status(&api, ctx, &tenant, "suspended", reason)
        }
        TenantCommand::Resume { tenant } => set_status(&api, ctx, &tenant, "active", None),
        TenantCommand::Deactivate { tenant, reason } => {
            set_status(&api, ctx, &tenant, "deactivated", reason)
        }
        TenantCommand::RemoveMember { tenant, username } => {
            api.delete(&member_path(&tenant, &username), &[])?;
            out.emit(
                &json!({ "tenant": tenant, "username": username, "removed": true }),
                |t| {
                    t.line(format_args!(
                        "Removed {} from {}",
                        username.bold(),
                        tenant.bold()
                    ));
                },
            )
        }
    }
}

fn email(api: &Api, ctx: &Context, command: EmailCommand) -> Result<()> {
    let out = &ctx.out;
    match command {
        EmailCommand::Set { tenant, email } => {
            let value = api.patch(&tenant_path(&tenant), &json!({ "email": email }))?;
            render(out, &value, |t, tenant: Tenant| {
                t.line(format_args!(
                    "{} contact email: {}",
                    tenant.slug.bold(),
                    tenant.email_label()
                ));
                if tenant.email_verified_at.is_none() && !tenant.email.is_empty() {
                    t.line(format_args!(
                        "A verification code was sent; run `wt tenant email verify {} CODE`.",
                        tenant.slug
                    ));
                }
            })
        }
        EmailCommand::Verify { tenant, code } => {
            let path = format!("{}/email/verify", tenant_path(&tenant));
            let value = api.post(&path, &json!({ "token": code.trim() }))?;
            render(out, &value, |t, tenant: Tenant| {
                t.line(format_args!(
                    "Verified {} for {}",
                    tenant.email.green().bold(),
                    tenant.slug.bold()
                ));
            })
        }
        EmailCommand::Resend { tenant } => {
            let path = format!("{}/email/resend", tenant_path(&tenant));
            let value = api.post(&path, &json!({}))?;
            render(out, &value, |t, s: Sent| {
                let how = if s.delivery == "log" {
                    "written to the server log (SMTP is not configured)"
                } else {
                    "emailed"
                };
                t.line(format_args!(
                    "A new code for {} was {how}; it expires {}.",
                    s.email.bold(),
                    when(&s.expires_at)
                ));
            })
        }
        EmailCommand::Clear { tenant } => {
            let value = api.patch(&tenant_path(&tenant), &json!({ "email": "" }))?;
            render(out, &value, |t, tenant: Tenant| {
                t.line(format_args!(
                    "Removed the contact email of {}",
                    tenant.slug.bold()
                ));
            })
        }
    }
}

#[derive(Serialize)]
struct Switched<'a> {
    remote: &'a str,
    tenant: &'a str,
    previous_tenant: &'a str,
}

/// `wt tenant switch SLUG`: set `remote.<name>.tenant`.
fn switch(ctx: &Context, slug: &str, remote: Option<String>) -> Result<()> {
    if !valid_slug(slug) {
        return Err(CliError::usage(format!(
            "'{slug}' is not a valid tenant slug"
        )));
    }
    let mut repo = ctx.open()?;
    let name = remote_name(&repo, remote);
    let mut config = repo.config().clone();
    let Some(entry) = config.remote.get_mut(&name) else {
        return Err(worktree_sdk::SdkError::RemoteNotConfigured(name).into());
    };
    let previous = std::mem::replace(&mut entry.tenant, slug.to_string());
    repo.set_config(config)?;
    ctx.out.emit(
        &Switched {
            remote: &name,
            tenant: slug,
            previous_tenant: &previous,
        },
        |t| {
            if previous == slug {
                t.line(format_args!(
                    "Remote {} already uses tenant {}",
                    name.bold(),
                    slug.cyan()
                ));
            } else {
                t.line(format_args!(
                    "Remote {} now uses tenant {} (was {})",
                    name.bold(),
                    slug.green().bold(),
                    previous
                ));
            }
        },
    )
}

fn set_status(
    api: &Api,
    ctx: &Context,
    tenant: &str,
    status: &str,
    reason: Option<String>,
) -> Result<()> {
    let path = format!("/api/v1/admin/tenants/{}", seg(tenant));
    let value = api.patch(&path, &status_body(status, reason))?;
    render(&ctx.out, &value, |t, tenant: Tenant| {
        let label = tenant.status_label();
        let colored = match label {
            "active" => label.green(),
            "suspended" => label.yellow(),
            _ => label.red(),
        };
        t.line(format_args!(
            "{} is now {}",
            tenant.slug.bold(),
            colored.bold()
        ));
        if !tenant.status_reason.is_empty() {
            t.line(format_args!("reason: {}", tenant.status_reason));
        }
    })
}

fn member_path(tenant: &str, username: &str) -> String {
    format!("{}/members/{}", tenant_path(tenant), seg(username))
}

/// SLUG NAME KIND PLAN CREATED.
pub fn tenant_table(t: &mut Text, tenants: &[Tenant]) {
    let mut table = Table::new(&["SLUG", "NAME", "KIND", "PLAN", "CREATED"]);
    for tenant in tenants {
        table.row([
            Cell::from(tenant.slug.as_str()).bold(),
            tenant.name.as_str().into(),
            tenant.kind.as_str().into(),
            tenant.plan.as_str().into(),
            when(&tenant.created_at).into(),
        ]);
    }
    table.render(t, "No tenants.");
}

fn show(t: &mut Text, tenant: &Tenant, deletion: Option<&Deletion>) {
    t.line(format_args!("{}  {}", tenant.slug.bold(), tenant.name));
    let mut rows = vec![
        ("kind", tenant.kind.clone()),
        ("plan", tenant.plan.clone()),
        ("status", {
            let mut s = tenant.status_label().to_string();
            if !tenant.status_reason.is_empty() {
                s.push_str(&format!(" ({})", tenant.status_reason));
            }
            s
        }),
        ("email", tenant.email_label()),
    ];
    if let Some(d) = deletion {
        rows.push((
            "deletion",
            format!(
                "purged {} (requested by {}; `wt tenant restore {}` cancels it)",
                when(&d.purge_at),
                d.requested_by,
                tenant.slug
            ),
        ));
    }
    rows.push(("id", tenant.id.clone()));
    rows.push(("created", when(&tenant.created_at)));
    fields(t, &rows);
}

#[cfg(test)]
mod tests {
    use clap::error::ErrorKind;

    use super::*;
    use crate::cli::Command;
    use crate::commands::server_common::testing::{parse, parse_err};

    fn tenant(args: &[&str]) -> TenantCommand {
        let mut full = vec!["tenant"];
        full.extend_from_slice(args);
        match parse(&full) {
            Command::Tenant(a) => a.command,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn parses_every_subcommand() {
        assert!(matches!(tenant(&["list"]), TenantCommand::List));
        assert!(matches!(
            tenant(&["create", "acme", "--name", "Acme"]),
            TenantCommand::Create { slug, name: Some(n) } if slug == "acme" && n == "Acme"
        ));
        assert!(matches!(
            tenant(&["create", "acme"]),
            TenantCommand::Create { name: None, .. }
        ));
        assert!(
            matches!(tenant(&["show", "acme"]), TenantCommand::Show { tenant } if tenant == "acme")
        );
        assert!(matches!(
            tenant(&["set-name", "acme", "Acme Inc"]),
            TenantCommand::SetName { name, .. } if name == "Acme Inc"
        ));
        assert!(matches!(
            tenant(&["plan", "acme", "enterprise"]),
            TenantCommand::Plan {
                plan: Plan::Enterprise,
                ..
            }
        ));
        assert!(matches!(
            tenant(&["members", "acme"]),
            TenantCommand::Members { .. }
        ));
        assert!(matches!(
            tenant(&["add-member", "acme", "bob"]),
            TenantCommand::AddMember { role, .. } if role == "developer"
        ));
        assert!(matches!(
            tenant(&["add-member", "acme", "bob", "--role", "viewer"]),
            TenantCommand::AddMember { role, .. } if role == "viewer"
        ));
        assert!(matches!(
            tenant(&["set-role", "acme", "bob", "admin"]),
            TenantCommand::SetRole { username, role, .. } if username == "bob" && role == "admin"
        ));
        assert!(matches!(
            tenant(&["remove-member", "acme", "bob"]),
            TenantCommand::RemoveMember { .. }
        ));
        assert_eq!(
            parse_err(&["tenant", "plan", "acme", "gold"]),
            ErrorKind::InvalidValue
        );
        assert_eq!(
            parse_err(&["tenant", "set-role", "acme", "bob"]),
            ErrorKind::MissingRequiredArgument
        );
    }

    #[test]
    fn parses_lifecycle_subcommands() {
        assert!(matches!(
            tenant(&["rename", "acme", "acme-inc"]),
            TenantCommand::Rename { tenant, new_slug } if tenant == "acme" && new_slug == "acme-inc"
        ));
        assert!(matches!(
            tenant(&["delete", "acme"]),
            TenantCommand::Delete { yes: false, .. }
        ));
        assert!(matches!(
            tenant(&["delete", "acme", "--yes"]),
            TenantCommand::Delete { yes: true, .. }
        ));
        assert!(matches!(
            tenant(&["restore", "acme"]),
            TenantCommand::Restore { tenant } if tenant == "acme"
        ));
        assert!(matches!(
            tenant(&["email", "set", "acme", "ops@acme.io"]),
            TenantCommand::Email(EmailArgs { command: EmailCommand::Set { email, .. } }) if email == "ops@acme.io"
        ));
        assert!(matches!(
            tenant(&["email", "verify", "acme", "wte_x"]),
            TenantCommand::Email(EmailArgs { command: EmailCommand::Verify { code, .. } }) if code == "wte_x"
        ));
        assert!(matches!(
            tenant(&["email", "resend", "acme"]),
            TenantCommand::Email(EmailArgs {
                command: EmailCommand::Resend { .. }
            })
        ));
        assert!(matches!(
            tenant(&["email", "clear", "acme"]),
            TenantCommand::Email(EmailArgs {
                command: EmailCommand::Clear { .. }
            })
        ));
        assert!(matches!(
            tenant(&["switch", "acme-inc"]),
            TenantCommand::Switch { slug, remote: None } if slug == "acme-inc"
        ));
        assert!(matches!(
            tenant(&["switch", "acme-inc", "--remote", "upstream"]),
            TenantCommand::Switch { remote: Some(r), .. } if r == "upstream"
        ));
        assert_eq!(
            parse_err(&["tenant", "rename", "acme"]),
            ErrorKind::MissingRequiredArgument
        );
    }

    #[test]
    fn parses_status_subcommands() {
        assert!(matches!(
            tenant(&["suspend", "acme", "--reason", "unpaid"]),
            TenantCommand::Suspend { tenant, reason: Some(r) } if tenant == "acme" && r == "unpaid"
        ));
        assert!(matches!(
            tenant(&["deactivate", "acme"]),
            TenantCommand::Deactivate { reason: None, .. }
        ));
        assert!(matches!(
            tenant(&["resume", "acme"]),
            TenantCommand::Resume { tenant } if tenant == "acme"
        ));
        assert_eq!(
            parse_err(&["tenant", "resume", "acme", "--reason", "x"]),
            ErrorKind::UnknownArgument
        );
    }

    #[test]
    fn status_bodies_and_labels() {
        assert_eq!(
            status_body("suspended", Some("unpaid".into())),
            json!({"status": "suspended", "status_reason": "unpaid"})
        );
        assert_eq!(
            status_body("deactivated", None),
            json!({"status": "deactivated"})
        );
        assert_eq!(
            status_body("active", None),
            json!({"status": "active", "status_reason": ""})
        );
        let t: Tenant = serde_json::from_value(json!({"slug": "acme"})).unwrap();
        assert_eq!(t.status_label(), "active");
        assert_eq!(t.email_label(), "-");
        let t: Tenant = serde_json::from_value(
            json!({"slug": "acme", "status": "suspended", "email": "ops@acme.io"}),
        )
        .unwrap();
        assert_eq!(t.status_label(), "suspended");
        assert_eq!(t.email_label(), "ops@acme.io (unverified)");
        let t: Tenant = serde_json::from_value(
            json!({"slug": "acme", "email": "ops@acme.io", "email_verified_at": "2026-10-01T00:00:00Z"}),
        )
        .unwrap();
        assert_eq!(t.email_label(), "ops@acme.io (verified)");
    }

    #[test]
    fn slug_rules() {
        for ok in ["acme", "acme-inc", "a", "a1-b2"] {
            assert!(valid_slug(ok), "{ok}");
        }
        for bad in [
            "",
            "Acme",
            "acme inc",
            "-acme",
            "acme-",
            "a_b",
            &"a".repeat(64),
        ] {
            assert!(!valid_slug(bad), "{bad}");
        }
    }

    #[test]
    fn plans_serialize_lowercase() {
        assert_eq!(json!(Plan::Pro), json!("pro"));
    }
}
