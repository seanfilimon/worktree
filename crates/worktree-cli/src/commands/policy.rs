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
    call, confirm, fields, read_input, show, tenant_path, when, yes, PrincipalRef, Table, YesArgs,
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
    /// Dry-run a policy change: evaluate a request with and without the
    /// policies of a file (nothing is saved)
    ///
    /// By default the file's policies replace the tenant's policies of the
    /// same name and are added otherwise; with --only they are the only
    /// policies evaluated.
    Simulate {
        /// Tenant slug
        tenant: String,
        /// One policy, an array of policies, or {"policies": [...]} (`-` = stdin)
        #[arg(long, short = 'f', value_name = "FILE")]
        file: PathBuf,
        /// Evaluate only the file's policies
        #[arg(long)]
        only: bool,
        /// `user:<name>` or `sa:<tenant>/<name>`
        #[arg(long)]
        principal: PrincipalRef,
        /// Action, e.g. `tree:write` or `branch:push`
        #[arg(long)]
        action: String,
        /// Tree name (or TENANT/TREE) of the resource
        // Shares the global `--tree` id, exactly like `wt iam simulate`.
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

/// Policies of a simulation file: one policy, an array of policies, or
/// `{"policies": [...]}` (the canonical access document shape).
fn candidate_policies(text: &str, source: &Path) -> Result<Vec<Value>> {
    let value: Value = serde_json::from_str(text).map_err(|e| {
        CliError::failure(format!("invalid policy file '{}': {e}", source.display()))
    })?;
    let items = match value {
        Value::Array(items) => items,
        Value::Object(ref map)
            if map.contains_key("policies") && !map.contains_key("statements") =>
        {
            map["policies"].as_array().cloned().unwrap_or_default()
        }
        other => vec![other],
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        // Validate the shape; send the document as written.
        let doc: PolicyDocument = serde_json::from_value(item.clone()).map_err(|e| {
            CliError::failure(format!("invalid policy in '{}': {e}", source.display()))
        })?;
        out.push(serde_json::to_value(doc).unwrap_or(item));
    }
    Ok(out)
}

/// The tenant's current policies with `candidates` replacing same-named
/// ones (case-insensitive) and the rest appended.
fn merge_candidates(current: Vec<Value>, candidates: Vec<Value>) -> Vec<Value> {
    let name = |v: &Value| v["name"].as_str().unwrap_or_default().to_lowercase();
    let mut out: Vec<Value> = current
        .into_iter()
        .filter(|p| !candidates.iter().any(|c| name(c) == name(p)))
        .collect();
    out.extend(candidates);
    out
}

#[derive(Debug, Deserialize)]
struct SimReason {
    source: String,
    #[serde(default)]
    detail: String,
    #[serde(default)]
    effect: String,
}

#[derive(Debug, Deserialize)]
struct SimDecision {
    allowed: bool,
    #[serde(default)]
    effect: String,
    #[serde(default)]
    reasons: Vec<SimReason>,
}

fn simulation_text(t: &mut Text, request: &str, current: &SimDecision, candidate: &SimDecision) {
    let verdict = |d: &SimDecision| {
        let word = if d.allowed { "ALLOWED" } else { "DENIED" };
        if d.effect.is_empty() {
            word.to_string()
        } else {
            format!("{word} ({})", d.effect)
        }
    };
    t.line(format_args!("{request}"));
    t.line(format_args!("  now:                {}", verdict(current)));
    t.line(format_args!("  with the change:    {}", verdict(candidate)));
    if current.allowed != candidate.allowed {
        t.line("  The change flips this decision.");
    }
    t.blank();
    let mut table = Table::new(&["source", "effect", "detail"]);
    for r in &candidate.reasons {
        table.row([r.source.clone(), r.effect.clone(), r.detail.clone()]);
    }
    table.render(t, "No reasons given.");
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
        PolicyCommand::Simulate {
            tenant,
            file,
            only,
            principal,
            action,
            tree,
            branch,
            path,
            ip,
            mfa,
        } => {
            let client = ctx.rest()?;
            let candidates = candidate_policies(&read_input(&file)?, &file)?;
            let candidates = if only {
                candidates
            } else {
                let current: Vec<Value> = call(client.get(&tenant_path(&tenant, "/policies")))?;
                merge_candidates(current, candidates)
            };
            let tree = match tree.as_deref().map(|t| t.split_once('/')) {
                Some(Some((t, name))) if t == tenant => Some(name.to_string()),
                Some(Some(_)) => {
                    return Err(CliError::usage(format!(
                        "--tree is not a tree of tenant '{tenant}'"
                    )))
                }
                Some(None) => tree,
                None => None,
            };
            let (kind, name, sa_tenant) = match &principal {
                PrincipalRef::User(name) => ("user", name.as_str(), None),
                PrincipalRef::ServiceAccount { tenant, name } => {
                    ("service_account", name.as_str(), Some(tenant.as_str()))
                }
            };
            let mut body = json!({
                "principal": {"type": kind, "name": name, "tenant": sa_tenant},
                "action": action,
                "tenant": tenant,
                "tree": tree,
                "branch": branch,
                "path": path,
                "context": {"ip": ip, "mfa": mfa},
            });
            strip_nulls(&mut body);
            let current: Value = call(client.post("/api/v1/iam/simulate", &body))?;
            body["candidate_policies"] = Value::Array(candidates);
            let candidate: Value = call(client.post("/api/v1/iam/simulate", &body))?;
            let output = json!({"current": current, "candidate": candidate});
            let parse = |v: &Value| -> Result<SimDecision> {
                serde_json::from_value(v.clone()).map_err(|e| {
                    CliError::failure(format!("unexpected response from the server: {e}"))
                })
            };
            let (cur, cand) = (parse(&current)?, parse(&candidate)?);
            let mut resource = match &tree {
                Some(tree) => format!("{tenant}/{tree}"),
                None => format!("tenant {tenant}"),
            };
            if let Some(b) = &branch {
                resource.push_str(&format!(" branch {b}"));
            }
            if let Some(p) = &path {
                resource.push_str(&format!(" path {p}"));
            }
            let request = format!("{principal} {action} on {resource}");
            ctx.out
                .emit(&output, |t| simulation_text(t, &request, &cur, &cand))
        }
        PolicyCommand::Enable { tenant, id } => set_enabled(ctx, &tenant, &id, true),
        PolicyCommand::Disable { tenant, id } => set_enabled(ctx, &tenant, &id, false),
    }
}

/// Remove `null` members of JSON objects (recursively) so optional request
/// fields are simply absent.
fn strip_nulls(v: &mut Value) {
    if let Value::Object(map) = v {
        map.retain(|_, x| !x.is_null());
        for x in map.values_mut() {
            strip_nulls(x);
        }
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
    fn parses_simulate() {
        let cmd = policy(&[
            "simulate",
            "acme",
            "--file",
            "p.json",
            "--principal",
            "user:ada",
            "--action",
            "branch:push",
            "--tree",
            "web",
            "--branch",
            "main",
            "--mfa",
        ]);
        assert!(matches!(
            cmd,
            PolicyCommand::Simulate { only: false, mfa: true, principal: PrincipalRef::User(u),
                                      tree: Some(t), branch: Some(b), .. }
                if u == "ada" && t == "web" && b == "main"
        ));
        assert!(matches!(
            policy(&[
                "simulate",
                "acme",
                "-f",
                "-",
                "--only",
                "--principal",
                "sa:acme/ci",
                "--action",
                "tree:read"
            ]),
            PolicyCommand::Simulate { only: true, .. }
        ));
        assert_eq!(
            parse_err(&[
                "policy",
                "simulate",
                "acme",
                "--principal",
                "user:a",
                "--action",
                "x"
            ]),
            ErrorKind::MissingRequiredArgument
        );
    }

    #[test]
    fn candidate_files_and_merging() {
        let one = candidate_policies(DOC, Path::new("p.json")).unwrap();
        assert_eq!(one.len(), 1);
        assert_eq!(one[0]["name"], "no-friday-deploys");
        assert_eq!(one[0]["enabled"], true);
        let many = candidate_policies(
            r#"[{"name": "a", "statements": []}, {"name": "b", "statements": []}]"#,
            Path::new("x"),
        )
        .unwrap();
        assert_eq!(many.len(), 2);
        let doc = candidate_policies(
            r#"{"roles": [], "policies": [{"name": "c", "statements": []}]}"#,
            Path::new("x"),
        )
        .unwrap();
        assert_eq!(doc[0]["name"], "c");
        assert!(candidate_policies("{\"statements\": []}", Path::new("bad.json")).is_err());
        assert!(candidate_policies("nope", Path::new("bad.json")).is_err());

        let current = vec![
            json!({"id": "1", "name": "Keep", "statements": []}),
            json!({"id": "2", "name": "a", "statements": [{"effect": "deny"}]}),
        ];
        let merged = merge_candidates(current, many);
        let names: Vec<&str> = merged.iter().map(|p| p["name"].as_str().unwrap()).collect();
        assert_eq!(names, vec!["Keep", "a", "b"]);
        assert_eq!(merged[1]["statements"], json!([]), "candidate replaced 'a'");
    }

    #[test]
    fn renders_simulations_and_strips_nulls() {
        let allow = SimDecision {
            allowed: true,
            effect: "allow".into(),
            reasons: vec![],
        };
        let deny = SimDecision {
            allowed: false,
            effect: "deny".into(),
            reasons: vec![SimReason {
                source: "policy".into(),
                detail: "policy \"x\" denies".into(),
                effect: "deny".into(),
            }],
        };
        let mut t = Text::default();
        simulation_text(&mut t, "user:ada tree:write on acme/web", &allow, &deny);
        let out = t.as_str();
        assert!(out.contains("now:                ALLOWED (allow)"), "{out}");
        assert!(out.contains("with the change:    DENIED (deny)"), "{out}");
        assert!(out.contains("flips this decision"), "{out}");
        assert!(out.contains("policy  deny"), "{out}");

        let mut v = json!({"a": null, "b": {"c": null, "d": 1}, "e": [null]});
        strip_nulls(&mut v);
        assert_eq!(v, json!({"b": {"d": 1}, "e": [null]}));
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
