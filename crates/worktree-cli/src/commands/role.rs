//! `wt role` — Built-in and custom roles of a tenant (§12.3 roles).

use clap::{Args, Subcommand};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use worktree_sdk::remote::rest::seg;

use crate::commands::iam_common::{call, confirm, fields, show, tenant_path, yes, Table, YesArgs};
use crate::context::Context;
use crate::error::{CliError, Result};
use crate::output::Text;

#[derive(Debug, Args)]
pub struct RoleArgs {
    #[command(subcommand)]
    pub command: RoleCommand,
}

#[derive(Debug, Subcommand)]
pub enum RoleCommand {
    /// List built-in and custom roles
    List {
        /// Tenant slug
        tenant: String,
    },
    /// Create a custom role
    Create {
        /// Tenant slug
        tenant: String,
        /// Role name ([a-z0-9-], must not collide with a built-in role)
        name: String,
        /// Granted action (`tree:read`, `area:*`, ...); repeatable
        #[arg(long = "permission", value_name = "ACTION", required = true)]
        permissions: Vec<String>,
        /// Description
        #[arg(long, default_value = "")]
        description: String,
    },
    /// Show a role and its permissions
    Show {
        /// Tenant slug
        tenant: String,
        /// Role name
        name: String,
    },
    /// Change a custom role's permissions or description
    Update {
        /// Tenant slug
        tenant: String,
        /// Role name
        name: String,
        /// Replacement permission list; repeatable
        #[arg(long = "permission", value_name = "ACTION")]
        permissions: Vec<String>,
        /// New description
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete a custom role (refused while it is assigned)
    Delete {
        /// Tenant slug
        tenant: String,
        /// Role name
        name: String,
        #[command(flatten)]
        yes: YesArgs,
    },
}

#[derive(Debug, Deserialize)]
struct Role {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    permissions: Vec<String>,
    #[serde(default)]
    builtin: bool,
}

#[derive(Serialize)]
struct RolePatch<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    permissions: Option<&'a [String]>,
}

fn role_path(tenant: &str, name: &str) -> String {
    tenant_path(tenant, &format!("/roles/{}", seg(name)))
}

pub fn run(ctx: &Context, args: RoleArgs) -> Result<()> {
    let client = ctx.rest()?;
    match args.command {
        RoleCommand::List { tenant } => {
            let roles: Value = call(client.get(&tenant_path(&tenant, "/roles")))?;
            show(&ctx.out, &roles, |t, roles: &Vec<Role>| role_list(t, roles))
        }
        RoleCommand::Create {
            tenant,
            name,
            permissions,
            description,
        } => {
            let body =
                json!({"name": name, "description": description, "permissions": permissions});
            let role: Value = call(client.post(&tenant_path(&tenant, "/roles"), &body))?;
            show(&ctx.out, &role, |t, role: &Role| {
                t.line(format_args!(
                    "Created role {} in {tenant} ({} permissions)",
                    role.name,
                    role.permissions.len()
                ));
            })
        }
        RoleCommand::Show { tenant, name } => {
            let role: Value = call(client.get(&role_path(&tenant, &name)))?;
            show(&ctx.out, &role, role_detail)
        }
        RoleCommand::Update {
            tenant,
            name,
            permissions,
            description,
        } => {
            if permissions.is_empty() && description.is_none() {
                return Err(CliError::usage("nothing to update")
                    .with_hint("pass --permission (repeatable) and/or --description"));
            }
            let patch = RolePatch {
                description: description.as_deref(),
                permissions: (!permissions.is_empty()).then_some(permissions.as_slice()),
            };
            let role: Value = call(client.patch(&role_path(&tenant, &name), &patch))?;
            show(&ctx.out, &role, |t, role: &Role| {
                t.line(format_args!("Updated role {} in {tenant}", role.name));
            })
        }
        RoleCommand::Delete { tenant, name, yes } => {
            confirm(yes.yes, &format!("delete role {name} in {tenant}"))?;
            call(client.delete(&role_path(&tenant, &name)))?;
            ctx.out.emit(
                &json!({"tenant": tenant, "role": name, "deleted": true}),
                |t| t.line(format_args!("Deleted role {name} in {tenant}")),
            )
        }
    }
}

fn role_list(t: &mut Text, roles: &[Role]) {
    let mut table = Table::new(&["name", "built-in", "permissions", "description"]);
    for role in roles {
        table.row([
            role.name.clone(),
            yes(role.builtin).to_string(),
            role.permissions.join(","),
            role.description.clone(),
        ]);
    }
    table.render(t, "No roles.");
}

fn role_detail(t: &mut Text, role: &Role) {
    fields(
        t,
        &[
            ("role", role.name.clone()),
            ("built-in", if role.builtin { "yes" } else { "no" }.into()),
            ("description", role.description.clone()),
        ],
    );
    t.blank();
    t.line("Permissions:");
    if role.permissions.is_empty() {
        t.line("  (none)");
    }
    for p in &role.permissions {
        t.line(format_args!("  {p}"));
    }
}

#[cfg(test)]
mod tests {
    use clap::error::ErrorKind;

    use super::*;
    use crate::cli::Command;
    use crate::commands::iam_common::{parse, parse_err};

    fn role(args: &[&str]) -> RoleCommand {
        let mut argv = vec!["role"];
        argv.extend_from_slice(args);
        match parse(&argv) {
            Command::Role(a) => a.command,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn parses_subcommands() {
        assert!(matches!(role(&["list", "acme"]), RoleCommand::List { .. }));
        assert!(matches!(
            role(&["create", "acme", "auditor", "--permission", "audit:read",
                   "--permission", "tree:read", "--description", "d"]),
            RoleCommand::Create { name, permissions, description, .. }
                if name == "auditor" && permissions == ["audit:read", "tree:read"] && description == "d"
        ));
        assert_eq!(
            parse_err(&["role", "create", "acme", "auditor"]),
            ErrorKind::MissingRequiredArgument
        );
        assert!(matches!(
            role(&["show", "acme", "auditor"]),
            RoleCommand::Show { .. }
        ));
        assert!(matches!(
            role(&["update", "acme", "auditor", "--permission", "tree:*"]),
            RoleCommand::Update { permissions, description: None, .. } if permissions == ["tree:*"]
        ));
        assert!(matches!(
            role(&["delete", "acme", "auditor", "--yes"]),
            RoleCommand::Delete {
                yes: YesArgs { yes: true },
                ..
            }
        ));
    }

    #[test]
    fn renders_roles() {
        let roles: Vec<Role> = serde_json::from_value(json!([
            {"name": "owner", "description": "Full control", "permissions": ["*"], "builtin": true},
            {"name": "auditor", "description": "", "permissions": ["audit:read", "tree:read"], "builtin": false}
        ]))
        .unwrap();
        let mut t = Text::default();
        role_list(&mut t, &roles);
        assert_eq!(
            t.as_str(),
            "NAME     BUILT-IN  PERMISSIONS           DESCRIPTION\n\
             owner    yes       *                     Full control\n\
             auditor  -         audit:read,tree:read  -\n"
        );
        let mut t = Text::default();
        role_detail(&mut t, &roles[1]);
        assert!(t.as_str().contains("built-in:    no\n"), "{}", t.as_str());
        assert!(t.as_str().contains("  audit:read\n  tree:read\n"));
    }

    #[test]
    fn patch_omits_unset_fields() {
        let perms = vec!["tree:read".to_string()];
        let patch = RolePatch {
            description: None,
            permissions: Some(&perms),
        };
        assert_eq!(
            serde_json::to_value(&patch).unwrap(),
            json!({"permissions": ["tree:read"]})
        );
    }
}
