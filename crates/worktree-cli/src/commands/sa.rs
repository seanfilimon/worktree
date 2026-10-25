//! `wt sa` — Service accounts and their keys (§12.3 service accounts).

use clap::{Args, Subcommand};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use worktree_sdk::remote::rest::seg;

use crate::commands::iam_common::{
    call, confirm, fields, reveal_token, show, tenant_path, token_table, when, ApiToken,
    CreatedToken, NewToken, Table, YesArgs,
};
use crate::context::Context;
use crate::error::{CliError, Result};
use crate::output::Text;

#[derive(Debug, Args)]
pub struct SaArgs {
    #[command(subcommand)]
    pub command: SaCommand,
}

#[derive(Debug, Subcommand)]
pub enum SaCommand {
    /// List the service accounts of a tenant
    List {
        /// Tenant slug
        tenant: String,
    },
    /// Create a service account
    Create {
        /// Tenant slug
        tenant: String,
        /// Name ([a-z0-9-])
        name: String,
        /// Role the account acts with inside the tenant
        #[arg(long)]
        role: String,
        /// Description
        #[arg(long, default_value = "")]
        description: String,
    },
    /// Show a service account and its keys
    Show {
        /// Tenant slug
        tenant: String,
        /// Service account name
        name: String,
    },
    /// Change a service account's role or description, or disable/enable it
    Update {
        /// Tenant slug
        tenant: String,
        /// Service account name
        name: String,
        /// New role
        #[arg(long)]
        role: Option<String>,
        /// New description
        #[arg(long)]
        description: Option<String>,
        /// Disable the account (its keys stop working)
        #[arg(long, conflicts_with = "enable")]
        disable: bool,
        /// Enable a disabled account
        #[arg(long)]
        enable: bool,
    },
    /// Delete a service account with its keys and grants
    Delete {
        /// Tenant slug
        tenant: String,
        /// Service account name
        name: String,
        #[command(flatten)]
        yes: YesArgs,
    },
    /// Create a key (bearer token) for a service account; shown once
    KeyCreate {
        /// Tenant slug
        tenant: String,
        /// Service account name
        name: String,
        /// Key name
        #[arg(long = "name", value_name = "NAME", default_value = "key")]
        key_name: String,
        /// Expire after this many hours (default: never)
        #[arg(long, value_name = "N")]
        ttl_hours: Option<u32>,
        /// Restrict the key to an action pattern (`tree:read`, `tree:*`); repeatable
        #[arg(long = "scope", value_name = "SCOPE")]
        scopes: Vec<String>,
    },
    /// Revoke a service-account key
    KeyDelete {
        /// Tenant slug
        tenant: String,
        /// Service account name
        name: String,
        /// Key id
        key_id: String,
        #[command(flatten)]
        yes: YesArgs,
    },
}

#[derive(Debug, Deserialize)]
struct ServiceAccount {
    name: String,
    #[serde(default)]
    description: String,
    role: String,
    #[serde(default)]
    disabled: bool,
    #[serde(default)]
    created_by: String,
    #[serde(default)]
    created_at: String,
}

impl ServiceAccount {
    fn status(&self) -> &'static str {
        if self.disabled {
            "disabled"
        } else {
            "active"
        }
    }
}

#[derive(Debug, Deserialize)]
struct ServiceAccountDetail {
    service_account: ServiceAccount,
    #[serde(default)]
    keys: Vec<ApiToken>,
}

#[derive(Serialize)]
struct SaPatch<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    role: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    disabled: Option<bool>,
}

fn sa_path(tenant: &str, name: &str, rest: &str) -> String {
    tenant_path(tenant, &format!("/service-accounts/{}{rest}", seg(name)))
}

pub fn run(ctx: &Context, args: SaArgs) -> Result<()> {
    let client = ctx.rest()?;
    match args.command {
        SaCommand::List { tenant } => {
            let list: Value = call(client.get(&tenant_path(&tenant, "/service-accounts")))?;
            show(&ctx.out, &list, |t, list: &Vec<ServiceAccount>| {
                sa_list(t, list)
            })
        }
        SaCommand::Create {
            tenant,
            name,
            role,
            description,
        } => {
            let body = json!({"name": name, "description": description, "role": role});
            let sa: Value = call(client.post(&tenant_path(&tenant, "/service-accounts"), &body))?;
            show(&ctx.out, &sa, |t, sa: &ServiceAccount| {
                t.line(format_args!(
                    "Created service account {tenant}/{} with role {}",
                    sa.name, sa.role
                ));
                t.line(format_args!(
                    "Create a key with `wt sa key-create {tenant} {}`",
                    sa.name
                ));
            })
        }
        SaCommand::Show { tenant, name } => {
            let detail: Value = call(client.get(&sa_path(&tenant, &name, "")))?;
            show(&ctx.out, &detail, sa_detail)
        }
        SaCommand::Update {
            tenant,
            name,
            role,
            description,
            disable,
            enable,
        } => {
            let patch = SaPatch {
                description: description.as_deref(),
                role: role.as_deref(),
                disabled: (disable || enable).then_some(disable),
            };
            if patch.description.is_none() && patch.role.is_none() && patch.disabled.is_none() {
                return Err(CliError::usage("nothing to update")
                    .with_hint("pass --role, --description, --disable or --enable"));
            }
            let sa: Value = call(client.patch(&sa_path(&tenant, &name, ""), &patch))?;
            show(&ctx.out, &sa, |t, sa: &ServiceAccount| {
                t.line(format_args!(
                    "Updated service account {tenant}/{} (role {}, {})",
                    sa.name,
                    sa.role,
                    sa.status()
                ));
            })
        }
        SaCommand::Delete { tenant, name, yes } => {
            confirm(yes.yes, &format!("delete service account {tenant}/{name}"))?;
            call(client.delete(&sa_path(&tenant, &name, "")))?;
            ctx.out.emit(
                &json!({"tenant": tenant, "service_account": name, "deleted": true}),
                |t| t.line(format_args!("Deleted service account {tenant}/{name}")),
            )
        }
        SaCommand::KeyCreate {
            tenant,
            name,
            key_name,
            ttl_hours,
            scopes,
        } => {
            let body = NewToken {
                name: &key_name,
                ttl_hours,
                scopes: &scopes,
            };
            let created: Value = call(client.post(&sa_path(&tenant, &name, "/keys"), &body))?;
            show(&ctx.out, &created, |t, c: &CreatedToken| {
                reveal_token(t, &format!("key for {tenant}/{name}"), c);
            })
        }
        SaCommand::KeyDelete {
            tenant,
            name,
            key_id,
            yes,
        } => {
            confirm(yes.yes, &format!("revoke key {key_id} of {tenant}/{name}"))?;
            let path = sa_path(&tenant, &name, &format!("/keys/{}", seg(&key_id)));
            call(client.delete(&path))?;
            ctx.out.emit(
                &json!({"tenant": tenant, "service_account": name, "key_id": key_id, "deleted": true}),
                |t| t.line(format_args!("Revoked key {key_id} of {tenant}/{name}")),
            )
        }
    }
}

fn sa_list(t: &mut Text, list: &[ServiceAccount]) {
    let mut table = Table::new(&["name", "role", "status", "description", "created"]);
    for sa in list {
        table.row([
            sa.name.clone(),
            sa.role.clone(),
            sa.status().to_string(),
            sa.description.clone(),
            when(&sa.created_at),
        ]);
    }
    table.render(t, "No service accounts.");
}

fn sa_detail(t: &mut Text, detail: &ServiceAccountDetail) {
    let sa = &detail.service_account;
    fields(
        t,
        &[
            ("service account", sa.name.clone()),
            ("role", sa.role.clone()),
            ("status", sa.status().to_string()),
            ("description", sa.description.clone()),
            ("created by", sa.created_by.clone()),
            ("created", when(&sa.created_at)),
        ],
    );
    t.blank();
    t.line("Keys:");
    token_table(&detail.keys).render(t, "(none)");
}

#[cfg(test)]
mod tests {
    use clap::error::ErrorKind;

    use super::*;
    use crate::cli::Command;
    use crate::commands::iam_common::{parse, parse_err};

    fn sa(args: &[&str]) -> SaCommand {
        let mut argv = vec!["sa"];
        argv.extend_from_slice(args);
        match parse(&argv) {
            Command::Sa(a) => a.command,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn parses_subcommands() {
        assert!(matches!(sa(&["list", "acme"]), SaCommand::List { .. }));
        assert!(matches!(
            sa(&["create", "acme", "ci", "--role", "developer"]),
            SaCommand::Create { name, role, .. } if name == "ci" && role == "developer"
        ));
        assert_eq!(
            parse_err(&["sa", "create", "acme", "ci"]),
            ErrorKind::MissingRequiredArgument
        );
        assert!(matches!(
            sa(&["show", "acme", "ci"]),
            SaCommand::Show { .. }
        ));
        assert!(matches!(
            sa(&["update", "acme", "ci", "--role", "viewer", "--disable"]),
            SaCommand::Update { role: Some(r), disable: true, enable: false, .. } if r == "viewer"
        ));
        assert_eq!(
            parse_err(&["sa", "update", "acme", "ci", "--disable", "--enable"]),
            ErrorKind::ArgumentConflict
        );
        assert!(matches!(
            sa(&["delete", "acme", "ci", "--yes"]),
            SaCommand::Delete {
                yes: YesArgs { yes: true },
                ..
            }
        ));
        assert!(matches!(
            sa(&["key-create", "acme", "ci", "--name", "deploy", "--ttl-hours", "24",
                 "--scope", "tree:read", "--scope", "tree:write"]),
            SaCommand::KeyCreate { name, key_name, ttl_hours: Some(24), scopes, .. }
                if name == "ci" && key_name == "deploy" && scopes == ["tree:read", "tree:write"]
        ));
        assert!(matches!(
            sa(&["key-create", "acme", "ci"]),
            SaCommand::KeyCreate { key_name, ttl_hours: None, scopes, .. }
                if key_name == "key" && scopes.is_empty()
        ));
        assert!(matches!(
            sa(&["key-delete", "acme", "ci", "k1", "-y"]),
            SaCommand::KeyDelete { key_id, yes: YesArgs { yes: true }, .. } if key_id == "k1"
        ));
    }

    #[test]
    fn renders_service_accounts() {
        let detail: ServiceAccountDetail = serde_json::from_value(json!({
            "service_account": {"id": "s1", "name": "ci", "role": "developer", "disabled": true,
                                "description": "CI bot", "created_by": "ada"},
            "keys": [{"id": "k1", "name": "deploy", "scopes": ["tree:*"], "kind": "service"}]
        }))
        .unwrap();
        let mut t = Text::default();
        sa_detail(&mut t, &detail);
        let out = t.as_str();
        assert!(out.contains("status:          disabled\n"), "{out}");
        assert!(out.contains("k1  deploy  tree:*"), "{out}");

        let mut t = Text::default();
        sa_list(&mut t, std::slice::from_ref(&detail.service_account));
        assert!(
            t.as_str().contains("ci    developer  disabled  CI bot"),
            "{}",
            t.as_str()
        );
    }

    #[test]
    fn patch_sets_disabled_only_when_asked() {
        let patch = SaPatch {
            description: None,
            role: Some("viewer"),
            disabled: None,
        };
        assert_eq!(
            serde_json::to_value(&patch).unwrap(),
            json!({"role": "viewer"})
        );
    }
}
