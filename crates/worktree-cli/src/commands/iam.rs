//! `wt iam` — Inspect access: the action catalog, effective permissions and
//! the access simulator (§12.1, §12.3).

use clap::{Args, Subcommand};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::commands::iam_common::{
    call, fields, grant_table, show, Grant, PrincipalRef, Table, TreeRef,
};
use crate::context::Context;
use crate::error::{CliError, Result};
use crate::output::Text;

#[derive(Debug, Args)]
pub struct IamArgs {
    #[command(subcommand)]
    pub command: IamCommand,
}

#[derive(Debug, Subcommand)]
pub enum IamCommand {
    /// List every action roles and policies can refer to
    Actions,
    /// Show the effective permissions of a principal (default: you)
    #[command(group = clap::ArgGroup::new("scope").required(true).multiple(true).args(["tenant", "tree"]))]
    Effective {
        /// Tenant slug
        #[arg(long)]
        tenant: Option<String>,
        /// Server tree (TENANT/TREE). A plain string: the `tree` id is shared
        /// with the global `--tree` flag, which this option replaces here.
        #[arg(long, value_name = "TREE")]
        tree: Option<String>,
        /// `user:<name>` or `sa:<tenant>/<name>`
        #[arg(long)]
        principal: Option<PrincipalRef>,
    },
    /// Evaluate an access request and explain the decision
    Simulate {
        /// `user:<name>` or `sa:<tenant>/<name>`
        #[arg(long)]
        principal: PrincipalRef,
        /// Action, e.g. `tree:write` or `branch:push`
        #[arg(long)]
        action: String,
        /// Tenant slug of the resource
        #[arg(long)]
        tenant: String,
        /// Tree name (or TENANT/TREE) of the resource
        #[arg(long)]
        tree: Option<String>,
        /// Branch of the resource
        #[arg(long, requires = "tree")]
        branch: Option<String>,
        /// Path within the tree
        #[arg(long, requires = "tree")]
        path: Option<String>,
        /// Request IP address, for ip_ranges conditions
        #[arg(long)]
        ip: Option<String>,
        /// Treat the session as MFA-verified, for require_mfa conditions
        #[arg(long)]
        mfa: bool,
    },
}

#[derive(Debug, Deserialize)]
struct ActionInfo {
    action: String,
    #[serde(default)]
    area: String,
    #[serde(default)]
    description: String,
}

#[derive(Debug, Deserialize)]
struct TeamRole {
    team: String,
    role: String,
}

#[derive(Debug, Deserialize)]
struct Effective {
    #[serde(default)]
    role: String,
    #[serde(default)]
    team_roles: Vec<TeamRole>,
    #[serde(default)]
    grants: Vec<Grant>,
    #[serde(default)]
    permissions: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Reason {
    source: String,
    #[serde(default)]
    detail: String,
    #[serde(default)]
    effect: String,
}

#[derive(Debug, Deserialize)]
struct Decision {
    allowed: bool,
    #[serde(default)]
    effect: String,
    #[serde(default)]
    reasons: Vec<Reason>,
}

#[derive(Debug, Serialize)]
struct SimPrincipal<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    tenant: Option<&'a str>,
}

impl<'a> From<&'a PrincipalRef> for SimPrincipal<'a> {
    fn from(p: &'a PrincipalRef) -> Self {
        match p {
            PrincipalRef::User(name) => Self {
                kind: "user",
                name,
                tenant: None,
            },
            PrincipalRef::ServiceAccount { tenant, name } => Self {
                kind: "service_account",
                name,
                tenant: Some(tenant),
            },
        }
    }
}

#[derive(Debug, Default, Serialize)]
struct SimContext<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    ip: Option<&'a str>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    mfa: bool,
}

#[derive(Debug, Serialize)]
struct SimRequest<'a> {
    principal: SimPrincipal<'a>,
    action: &'a str,
    tenant: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    tree: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    branch: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<&'a str>,
    context: SimContext<'a>,
}

/// The tree name of `--tree NAME` or `--tree TENANT/NAME` (whose tenant
/// must match `--tenant`).
fn tree_in_tenant<'a>(tenant: &str, tree: &'a str) -> Result<&'a str> {
    match tree.split_once('/') {
        None => Ok(tree),
        Some((t, name)) if t == tenant && !name.is_empty() && !name.contains('/') => Ok(name),
        Some(_) => Err(CliError::usage(format!(
            "--tree '{tree}' is not a tree of tenant '{tenant}'"
        ))),
    }
}

pub fn run(ctx: &Context, args: IamArgs) -> Result<()> {
    let client = ctx.rest()?;
    match args.command {
        IamCommand::Actions => {
            let actions: Value = call(client.get("/api/v1/iam/actions"))?;
            show(&ctx.out, &actions, |t, a: &Vec<ActionInfo>| {
                action_list(t, a)
            })
        }
        IamCommand::Effective {
            tenant,
            tree,
            principal,
        } => {
            let tree = tree
                .map(|t| t.parse::<TreeRef>().map_err(CliError::usage))
                .transpose()?;
            let tenant = match (tenant, &tree) {
                (Some(t), Some(tree)) if t != tree.tenant => {
                    return Err(CliError::usage(format!(
                        "--tree {tree} is not in tenant '{t}'"
                    )));
                }
                (_, Some(tree)) => tree.tenant.clone(),
                (Some(t), None) => t,
                (None, None) => unreachable!("clap requires --tenant or --tree"),
            };
            let query = [
                ("tenant", tenant),
                ("tree", tree.map(|t| t.name).unwrap_or_default()),
                (
                    "principal",
                    principal.map(|p| p.to_string()).unwrap_or_default(),
                ),
            ];
            let effective: Value = call(client.get_query("/api/v1/iam/effective", &query))?;
            show(&ctx.out, &effective, effective_text)
        }
        IamCommand::Simulate {
            principal,
            action,
            tenant,
            tree,
            branch,
            path,
            ip,
            mfa,
        } => {
            let tree = tree
                .as_deref()
                .map(|t| tree_in_tenant(&tenant, t))
                .transpose()?;
            let request = SimRequest {
                principal: SimPrincipal::from(&principal),
                action: &action,
                tenant: &tenant,
                tree,
                branch: branch.as_deref(),
                path: path.as_deref(),
                context: SimContext {
                    ip: ip.as_deref(),
                    mfa,
                },
            };
            let decision: Value = call(client.post("/api/v1/iam/simulate", &request))?;
            let resource = resource_label(&tenant, tree, branch.as_deref(), path.as_deref());
            show(&ctx.out, &decision, |t, d: &Decision| {
                decision_text(t, &principal, &action, &resource, d);
            })
        }
    }
}

fn action_list(t: &mut Text, actions: &[ActionInfo]) {
    let mut table = Table::new(&["action", "area", "description"]);
    for a in actions {
        table.row([a.action.clone(), a.area.clone(), a.description.clone()]);
    }
    table.render(t, "No actions.");
}

fn effective_text(t: &mut Text, e: &Effective) {
    let teams = e
        .team_roles
        .iter()
        .map(|tr| format!("{} ({})", tr.team, tr.role))
        .collect::<Vec<_>>()
        .join(", ");
    fields(t, &[("tenant role", e.role.clone()), ("team roles", teams)]);
    if !e.grants.is_empty() {
        t.blank();
        t.line("Tree grants:");
        grant_table(&e.grants).render(t, "");
    }
    t.blank();
    t.line("Permissions:");
    if e.permissions.is_empty() {
        t.line("  (none)");
    }
    for p in &e.permissions {
        t.line(format_args!("  {p}"));
    }
}

/// "acme/web branch main path src/" style resource description.
fn resource_label(
    tenant: &str,
    tree: Option<&str>,
    branch: Option<&str>,
    path: Option<&str>,
) -> String {
    let mut label = match tree {
        Some(tree) => format!("{tenant}/{tree}"),
        None => format!("tenant {tenant}"),
    };
    if let Some(branch) = branch {
        label.push_str(&format!(" branch {branch}"));
    }
    if let Some(path) = path {
        label.push_str(&format!(" path {path}"));
    }
    label
}

fn decision_text(
    t: &mut Text,
    principal: &PrincipalRef,
    action: &str,
    resource: &str,
    d: &Decision,
) {
    let verdict = if d.allowed {
        "ALLOWED".green().bold()
    } else {
        "DENIED".red().bold()
    };
    let effect = if d.effect.is_empty() {
        String::new()
    } else {
        format!(" ({})", d.effect)
    };
    t.line(format_args!(
        "{verdict}{effect}: {principal} {action} on {resource}"
    ));
    t.blank();
    let mut table = Table::new(&["source", "effect", "detail"]);
    for r in &d.reasons {
        table.row([r.source.clone(), r.effect.clone(), r.detail.clone()]);
    }
    table.render(t, "No reasons given.");
}

#[cfg(test)]
mod tests {
    use clap::error::ErrorKind;
    use serde_json::json;

    use super::*;
    use crate::cli::Command;
    use crate::commands::iam_common::{parse, parse_err};

    fn iam(args: &[&str]) -> IamCommand {
        let mut argv = vec!["iam"];
        argv.extend_from_slice(args);
        match parse(&argv) {
            Command::Iam(a) => a.command,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn parses_subcommands() {
        assert!(matches!(iam(&["actions"]), IamCommand::Actions));
        assert!(matches!(
            iam(&[
                "effective",
                "--tree",
                "acme/web",
                "--principal",
                "sa:acme/ci"
            ]),
            IamCommand::Effective {
                tenant: None,
                tree: Some(_),
                principal: Some(PrincipalRef::ServiceAccount { .. })
            }
        ));
        assert!(matches!(
            iam(&["effective", "--tenant", "acme"]),
            IamCommand::Effective {
                tenant: Some(_),
                tree: None,
                principal: None
            }
        ));
        assert_eq!(
            parse_err(&["iam", "effective"]),
            ErrorKind::MissingRequiredArgument
        );
        assert_eq!(
            parse_err(&["iam", "effective", "--tenant", "a", "--principal", "bob"]),
            ErrorKind::ValueValidation
        );
        assert!(matches!(
            iam(&["simulate", "--principal", "user:ada", "--action", "branch:push",
                  "--tenant", "acme", "--tree", "web", "--branch", "main",
                  "--path", "src/x", "--ip", "10.0.0.1", "--mfa"]),
            IamCommand::Simulate { principal: PrincipalRef::User(u), branch: Some(_), path: Some(_),
                                   ip: Some(_), mfa: true, .. } if u == "ada"
        ));
        assert_eq!(
            parse_err(&["iam", "simulate", "--principal", "user:a", "--action", "x"]),
            ErrorKind::MissingRequiredArgument
        );
        assert_eq!(
            parse_err(&[
                "iam",
                "simulate",
                "--principal",
                "user:a",
                "--action",
                "x",
                "--tenant",
                "t",
                "--branch",
                "main"
            ]),
            ErrorKind::MissingRequiredArgument
        );
    }

    #[test]
    fn simulate_request_shape() {
        let principal: PrincipalRef = "sa:acme/ci".parse().unwrap();
        let request = SimRequest {
            principal: SimPrincipal::from(&principal),
            action: "branch:push",
            tenant: "acme",
            tree: Some("web"),
            branch: Some("main"),
            path: None,
            context: SimContext {
                ip: Some("10.0.0.1"),
                mfa: false,
            },
        };
        assert_eq!(
            serde_json::to_value(&request).unwrap(),
            json!({
                "principal": {"type": "service_account", "name": "ci", "tenant": "acme"},
                "action": "branch:push", "tenant": "acme", "tree": "web", "branch": "main",
                "context": {"ip": "10.0.0.1"}
            })
        );
        let user = PrincipalRef::User("ada".into());
        let ctx = SimContext {
            ip: None,
            mfa: true,
        };
        assert_eq!(
            serde_json::to_value(SimPrincipal::from(&user)).unwrap(),
            json!({"type": "user", "name": "ada"})
        );
        assert_eq!(serde_json::to_value(ctx).unwrap(), json!({"mfa": true}));
    }

    #[test]
    fn trees_within_the_tenant() {
        assert_eq!(tree_in_tenant("acme", "web").unwrap(), "web");
        assert_eq!(tree_in_tenant("acme", "acme/web").unwrap(), "web");
        assert!(tree_in_tenant("acme", "other/web").is_err());
    }

    #[test]
    fn renders_decisions_with_reasons() {
        let d: Decision = serde_json::from_value(json!({
            "allowed": false, "effect": "deny",
            "reasons": [{"source": "tenant_role", "detail": "developer grants tree:write", "effect": "allow"},
                        {"source": "policy", "detail": "no-friday-deploys/fri", "effect": "deny"}]
        }))
        .unwrap();
        let mut t = Text::default();
        let resource = resource_label("acme", Some("web"), Some("main"), None);
        decision_text(
            &mut t,
            &PrincipalRef::User("ada".into()),
            "branch:push",
            &resource,
            &d,
        );
        let out = t.as_str();
        assert!(out.contains("DENIED"), "{out}");
        assert!(
            out.contains(" (deny): user:ada branch:push on acme/web branch main"),
            "{out}"
        );
        assert!(out.contains("SOURCE       EFFECT  DETAIL"), "{out}");
        assert!(
            out.contains("policy       deny    no-friday-deploys/fri"),
            "{out}"
        );
        assert_eq!(resource_label("acme", None, None, None), "tenant acme");
    }

    #[test]
    fn renders_effective_permissions() {
        let e: Effective = serde_json::from_value(json!({
            "role": "developer", "team_roles": [{"team": "core", "role": "maintainer"}],
            "grants": [], "permissions": ["tree:read", "tree:write"]
        }))
        .unwrap();
        let mut t = Text::default();
        effective_text(&mut t, &e);
        let out = t.as_str();
        assert!(
            out.contains("tenant role: developer\nteam roles:  core (maintainer)\n"),
            "{out}"
        );
        assert!(out.contains("  tree:read\n  tree:write\n"), "{out}");
        assert!(!out.contains("Tree grants"), "{out}");
    }
}
