//! `wt policy` — Access policies (allow/deny statements) of a tenant
//! (§12.1, §12.3 policies).
//!
//! Policy files are JSON documents `{name, description, enabled, statements}`
//! — exactly what `wt policy show --json` prints, so a policy can be
//! exported, edited and fed back with `wt policy update T ID --file -`.

use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use worktree_sdk::remote::rest::seg;

use crate::commands::iam_common::{
    call, confirm, fields, read_input, show, tenant_path, when, yes, Table, YesArgs,
};
use crate::context::Context;
use crate::error::{CliError, Result};
use crate::output::Text;

#[derive(Debug, Args)]
pub struct PolicyArgs {
    #[command(subcommand)]
    pub command: PolicyCommand,
}

#[derive(Debug, Subcommand)]
pub enum PolicyCommand {
    /// List the policies of a tenant
    List {
        /// Tenant slug
        tenant: String,
    },
    /// Show a policy and its statements
    Show {
        /// Tenant slug
        tenant: String,
        /// Policy id
        id: String,
    },
    /// Create a policy from a JSON file
    Create {
        /// Tenant slug
        tenant: String,
        /// Policy document (`-` = stdin)
        #[arg(long, short = 'f', value_name = "FILE")]
        file: PathBuf,
    },
    /// Replace a policy with the contents of a JSON file
    Update {
        /// Tenant slug
        tenant: String,
        /// Policy id
        id: String,
        /// Policy document (`-` = stdin)
        #[arg(long, short = 'f', value_name = "FILE")]
        file: PathBuf,
    },
    /// Delete a policy
    Delete {
        /// Tenant slug
        tenant: String,
        /// Policy id
        id: String,
        #[command(flatten)]
        yes: YesArgs,
    },
    /// Check a policy file's statements without saving anything
    Validate {
        /// Tenant slug
        tenant: String,
        /// Policy document (`-` = stdin)
        #[arg(long, short = 'f', value_name = "FILE")]
        file: PathBuf,
    },
    /// Enable a policy
    Enable {
        /// Tenant slug
        tenant: String,
        /// Policy id
        id: String,
    },
    /// Disable a policy (kept, but not evaluated)
    Disable {
        /// Tenant slug
        tenant: String,
        /// Policy id
        id: String,
    },
}

/// A policy document as sent to the server. Statements are passed through
/// untouched so every field the server understands survives a round trip.
#[derive(Debug, Serialize, Deserialize)]
struct PolicyDocument {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default = "enabled_by_default")]
    enabled: bool,
    statements: Vec<Value>,
}

fn enabled_by_default() -> bool {
    true
}

impl PolicyDocument {
    fn parse(text: &str, source: &Path) -> Result<Self> {
        serde_json::from_str(text).map_err(|e| {
            CliError::failure(format!("invalid policy file '{}': {e}", source.display())).with_hint(
                "expected JSON {\"name\", \"description\", \"enabled\", \"statements\": [...]}",
            )
        })
    }

    fn read(path: &Path) -> Result<Self> {
        Self::parse(&read_input(path)?, path)
    }
}

#[derive(Debug, Deserialize)]
struct Policy {
    id: String,
    name: String,
    #[serde(default)]
    description: String,
    enabled: bool,
    #[serde(default)]
    statements: Vec<Statement>,
    #[serde(default)]
    version: i64,
    #[serde(default)]
    created_by: String,
    #[serde(default)]
    updated_at: String,
}

#[derive(Debug, Deserialize)]
struct Statement {
    #[serde(default)]
    sid: String,
    effect: String,
    #[serde(default)]
    actions: Vec<String>,
    #[serde(default)]
    resources: Vec<String>,
    #[serde(default)]
    principals: Vec<String>,
    #[serde(default)]
    conditions: Value,
}

#[derive(Debug, Deserialize)]
struct Validation {
    valid: bool,
    #[serde(default)]
    errors: Vec<ValidationError>,
}

#[derive(Debug, Deserialize)]
struct ValidationError {
    statement: i64,
    #[serde(default)]
    field: String,
    message: String,
}

fn policy_path(tenant: &str, id: &str) -> String {
    tenant_path(tenant, &format!("/policies/{}", seg(id)))
}

pub fn run(ctx: &Context, args: PolicyArgs) -> Result<()> {
    match args.command {
        PolicyCommand::List { tenant } => {
            let policies: Value = call(ctx.rest()?.get(&tenant_path(&tenant, "/policies")))?;
            show(&ctx.out, &policies, |t, p: &Vec<Policy>| policy_list(t, p))
        }
        PolicyCommand::Show { tenant, id } => {
            let policy: Value = call(ctx.rest()?.get(&policy_path(&tenant, &id)))?;
            show(&ctx.out, &policy, policy_detail)
        }
        PolicyCommand::Create { tenant, file } => {
            let doc = PolicyDocument::read(&file)?;
            let path = tenant_path(&tenant, "/policies");
            let policy: Value = call(ctx.rest()?.post(&path, &doc))?;
            show(&ctx.out, &policy, |t, p: &Policy| {
                t.line(format_args!("Created policy {} ({})", p.name, p.id));
            })
        }
        PolicyCommand::Update { tenant, id, file } => {
            let doc = PolicyDocument::read(&file)?;
            let policy: Value = call(ctx.rest()?.put(&policy_path(&tenant, &id), &doc))?;
            show(&ctx.out, &policy, |t, p: &Policy| {
                t.line(format_args!(
                    "Updated policy {} ({}), now version {}",
                    p.name, p.id, p.version
                ));
            })
        }
        PolicyCommand::Validate { tenant, file } => {
            let doc = PolicyDocument::read(&file)?;
            let body = json!({ "statements": doc.statements });
            let path = tenant_path(&tenant, "/policies/validate");
            let result: Value = call(ctx.rest()?.post(&path, &body))?;
            show(&ctx.out, &result, |t, v: &Validation| validation(t, v))?;
            if result["valid"].as_bool() == Some(true) {
                Ok(())
            } else {
                Err(CliError::failure(format!(
                    "policy file '{}' is invalid",
                    file.display()
                )))
            }
        }
        PolicyCommand::Delete { tenant, id, yes } => {
            confirm(yes.yes, &format!("delete policy {id} in {tenant}"))?;
            call(ctx.rest()?.delete(&policy_path(&tenant, &id)))?;
            ctx.out
                .emit(&json!({"tenant": tenant, "id": id, "deleted": true}), |t| {
                    t.line(format_args!("Deleted policy {id}"))
                })
        }
        PolicyCommand::Enable { tenant, id } => set_enabled(ctx, &tenant, &id, true),
        PolicyCommand::Disable { tenant, id } => set_enabled(ctx, &tenant, &id, false),
    }
}

/// PUT is a full replace, so enabling/disabling re-sends the current policy.
fn set_enabled(ctx: &Context, tenant: &str, id: &str, enabled: bool) -> Result<()> {
    let client = ctx.rest()?;
    let path = policy_path(tenant, id);
    let current: Value = call(client.get(&path))?;
    let mut doc: PolicyDocument = serde_json::from_value(current)
        .map_err(|e| CliError::failure(format!("unexpected response from the server: {e}")))?;
    doc.enabled = enabled;
    let policy: Value = call(client.put(&path, &doc))?;
    show(&ctx.out, &policy, |t, p: &Policy| {
        let state = if p.enabled { "Enabled" } else { "Disabled" };
        t.line(format_args!("{state} policy {} ({})", p.name, p.id));
    })
}

fn policy_list(t: &mut Text, policies: &[Policy]) {
    let mut table = Table::new(&[
        "id",
        "name",
        "enabled",
        "statements",
        "version",
        "description",
    ]);
    for p in policies {
        table.row([
            p.id.clone(),
            p.name.clone(),
            yes(p.enabled).to_string(),
            p.statements.len().to_string(),
            p.version.to_string(),
            p.description.clone(),
        ]);
    }
    table.render(t, "No policies.");
}

fn policy_detail(t: &mut Text, p: &Policy) {
    fields(
        t,
        &[
            ("policy", format!("{} ({})", p.name, p.id)),
            ("enabled", if p.enabled { "yes" } else { "no" }.into()),
            ("version", p.version.to_string()),
            ("description", p.description.clone()),
            ("created by", p.created_by.clone()),
            ("updated", when(&p.updated_at)),
        ],
    );
    for (i, s) in p.statements.iter().enumerate() {
        t.blank();
        let sid = if s.sid.is_empty() {
            String::new()
        } else {
            format!(" ({})", s.sid)
        };
        t.line(format_args!(
            "Statement {}{sid}: {}",
            i + 1,
            s.effect.to_uppercase()
        ));
        let mut pairs = vec![
            ("  actions", s.actions.join(", ")),
            ("  resources", s.resources.join(", ")),
            ("  principals", s.principals.join(", ")),
        ];
        if s.conditions.as_object().is_some_and(|c| !c.is_empty()) {
            pairs.push(("  conditions", s.conditions.to_string()));
        }
        fields(t, &pairs);
    }
}

fn validation(t: &mut Text, v: &Validation) {
    if v.valid {
        t.line("Policy is valid.");
        return;
    }
    let mut table = Table::new(&["statement", "field", "message"]);
    for e in &v.errors {
        table.row([e.statement.to_string(), e.field.clone(), e.message.clone()]);
    }
    table.render(t, "Policy is invalid.");
}

#[cfg(test)]
mod tests {
    use clap::error::ErrorKind;

    use super::*;
    use crate::cli::Command;
    use crate::commands::iam_common::{parse, parse_err};

    fn policy(args: &[&str]) -> PolicyCommand {
        let mut argv = vec!["policy"];
        argv.extend_from_slice(args);
        match parse(&argv) {
            Command::Policy(a) => a.command,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn parses_subcommands() {
        assert!(matches!(
            policy(&["list", "acme"]),
            PolicyCommand::List { .. }
        ));
        assert!(
            matches!(policy(&["show", "acme", "p1"]), PolicyCommand::Show { id, .. } if id == "p1")
        );
        assert!(matches!(
            policy(&["create", "acme", "--file", "p.json"]),
            PolicyCommand::Create { file, .. } if file == Path::new("p.json")
        ));
        assert!(matches!(
            policy(&["update", "acme", "p1", "-f", "-"]),
            PolicyCommand::Update { file, .. } if file == Path::new("-")
        ));
        assert!(matches!(
            policy(&["validate", "acme", "-f", "x"]),
            PolicyCommand::Validate { .. }
        ));
        assert!(matches!(
            policy(&["delete", "acme", "p1", "--yes"]),
            PolicyCommand::Delete {
                yes: YesArgs { yes: true },
                ..
            }
        ));
        assert!(matches!(
            policy(&["enable", "acme", "p1"]),
            PolicyCommand::Enable { .. }
        ));
        assert!(matches!(
            policy(&["disable", "acme", "p1"]),
            PolicyCommand::Disable { .. }
        ));
        assert_eq!(
            parse_err(&["policy", "create", "acme"]),
            ErrorKind::MissingRequiredArgument
        );
    }

    const DOC: &str = r#"{
        "name": "no-friday-deploys",
        "statements": [{"effect": "deny", "actions": ["branch:push"],
                        "resources": ["wt:branch/acme/*/main"], "principals": ["*"],
                        "conditions": {"time_window": {"days": ["fri"], "start": "12:00", "end": "23:59"}}}]
    }"#;

    #[test]
    fn policy_documents_default_and_round_trip() {
        let doc = PolicyDocument::parse(DOC, Path::new("p.json")).unwrap();
        assert!(doc.enabled, "enabled defaults to true");
        assert_eq!(doc.description, "");
        let sent = serde_json::to_value(&doc).unwrap();
        assert_eq!(
            sent["statements"][0]["conditions"]["time_window"]["days"][0],
            "fri"
        );
        // Output of `wt policy show --json` is accepted as input.
        let shown = json!({"id": "p1", "tenant_id": "t", "name": "x", "description": "d",
                           "enabled": false, "statements": [], "version": 3});
        let doc: PolicyDocument = serde_json::from_value(shown).unwrap();
        assert!(!doc.enabled);
        let err = PolicyDocument::parse("{\"statements\": []}", Path::new("bad.json")).unwrap_err();
        assert!(err.to_string().contains("bad.json"), "{err}");
    }

    #[test]
    fn renders_policies() {
        let policy: Policy = serde_json::from_value(json!({
            "id": "p1", "name": "no-friday-deploys", "description": "", "enabled": true,
            "version": 2, "created_by": "ada", "updated_at": "2026-01-01T00:00:00Z",
            "statements": [{"sid": "fri", "effect": "deny", "actions": ["branch:push"],
                            "resources": ["wt:branch/acme/*/main"], "principals": ["*"],
                            "conditions": {"require_mfa": true}},
                           {"effect": "allow", "actions": ["tree:read"], "resources": ["*"],
                            "principals": ["authenticated"], "conditions": {}}]
        }))
        .unwrap();
        let mut t = Text::default();
        policy_list(&mut t, std::slice::from_ref(&policy));
        assert!(
            t.as_str()
                .contains("p1  no-friday-deploys  yes      2           2"),
            "{}",
            t.as_str()
        );
        let mut t = Text::default();
        policy_detail(&mut t, &policy);
        let out = t.as_str();
        assert!(out.contains("Statement 1 (fri): DENY\n"), "{out}");
        assert!(
            out.contains("  conditions: {\"require_mfa\":true}"),
            "{out}"
        );
        assert!(out.contains("Statement 2: ALLOW\n"), "{out}");
        assert_eq!(out.matches("conditions").count(), 1, "{out}");
    }

    #[test]
    fn renders_validation_results() {
        let mut t = Text::default();
        validation(
            &mut t,
            &Validation {
                valid: true,
                errors: vec![],
            },
        );
        assert_eq!(t.as_str(), "Policy is valid.\n");
        let v: Validation = serde_json::from_value(json!({"valid": false, "errors": [
            {"statement": 0, "field": "effect", "message": "must be allow or deny"}]}))
        .unwrap();
        let mut t = Text::default();
        validation(&mut t, &v);
        assert!(
            t.as_str()
                .contains("0          effect  must be allow or deny"),
            "{}",
            t.as_str()
        );
    }
}
