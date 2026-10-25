//! `wt tenant` — tenants (organizations) and their members on the server.

use clap::{Args, Subcommand, ValueEnum};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use serde_json::json;
use worktree_sdk::remote::rest::seg;

use crate::commands::admin::Account;
use crate::commands::server_common::{fields, render, tenant_path, when, Api, Cell, Table};
use crate::context::Context;
use crate::error::Result;
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
    /// Change a tenant's display name
    Rename { tenant: String, name: String },
    /// Change a tenant's plan (server admins only)
    Plan { tenant: String, plan: Plan },
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
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Member {
    account: Account,
    role: String,
}

pub fn run(ctx: &Context, args: TenantArgs) -> Result<()> {
    let api = Api::connect(ctx)?;
    let out = &ctx.out;
    match args.command {
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
            render(out, &value, |t, tenant: Tenant| show(t, &tenant))
        }
        TenantCommand::Rename { tenant, name } => {
            let value = api.patch(&tenant_path(&tenant), &json!({ "name": name }))?;
            render(out, &value, |t, tenant: Tenant| {
                t.line(format_args!(
                    "Renamed {} to {}",
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

fn show(t: &mut Text, tenant: &Tenant) {
    t.line(format_args!("{}  {}", tenant.slug.bold(), tenant.name));
    fields(
        t,
        &[
            ("kind", tenant.kind.clone()),
            ("plan", tenant.plan.clone()),
            ("id", tenant.id.clone()),
            ("created", when(&tenant.created_at)),
        ],
    );
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
            tenant(&["rename", "acme", "Acme Inc"]),
            TenantCommand::Rename { name, .. } if name == "Acme Inc"
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
    fn plans_serialize_lowercase() {
        assert_eq!(json!(Plan::Pro), json!("pro"));
    }
}
