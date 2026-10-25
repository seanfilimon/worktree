//! `wt access` — declarative access configuration (ENGINEERING_PLAN §13.7).
//!
//! Tenant-level roles and policies live in `.wt/access/roles.toml` and
//! `.wt/access/policies.toml` at the worktree root; a tree may restrict
//! itself further with deny statements in `<tree>/.wt-tree/access/policies.toml`
//! (`--tree-doc`). `validate` checks the files locally, `plan` asks the
//! server what `apply` would change, `apply` makes the server's
//! config-managed roles and policies match the files, and `pull` writes the
//! last applied document back as TOML.

use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};
use colored::Colorize;
use serde::Serialize;
use serde_json::json;
use worktree_sdk::remote::access::{self, declarative, AccessPlan, AccessTarget};
use worktree_sdk::{Repository, SdkError};

use self::declarative::{Compiled, Diagnostic, Scope};
use crate::commands::server_common::{confirm, cwd_origin_tenant, default_tree};
use crate::context::{block_on, Context};
use crate::error::{CliError, Result};
use crate::output::Text;

#[derive(Debug, Args)]
pub struct AccessArgs {
    #[command(subcommand)]
    pub command: AccessCommand,
}

/// Which files a command works on.
#[derive(Debug, Args, Default)]
pub struct Location {
    /// Directory holding roles.toml / policies.toml (default: .wt/access at
    /// the worktree root, or the tree's .wt-tree/access with --tree-doc)
    // Not `dir`: that id belongs to the global `-C` flag.
    #[arg(long = "dir", id = "access_dir", value_name = "DIR")]
    pub dir: Option<PathBuf>,
    /// Use the tree-level document (<tree>/.wt-tree/access/policies.toml:
    /// deny statements only) of the selected tree instead of the tenant's
    #[arg(long)]
    pub tree_doc: bool,
}

/// The server-side target.
#[derive(Debug, Args, Default)]
pub struct TargetArgs {
    /// Tenant slug (default: the tenant of this worktree's origin remote)
    #[arg(long, value_name = "T")]
    pub tenant: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum AccessCommand {
    /// Check the access files locally (syntax, actions, principals, resources)
    Validate {
        #[command(flatten)]
        location: Location,
    },
    /// Show what `wt access apply` would change on the server
    Plan {
        #[command(flatten)]
        location: Location,
        #[command(flatten)]
        target: TargetArgs,
    },
    /// Make the server's config-managed roles and policies match the files
    Apply {
        #[command(flatten)]
        location: Location,
        #[command(flatten)]
        target: TargetArgs,
        /// Do not ask for confirmation
        #[arg(short = 'y', long)]
        yes: bool,
    },
    /// Write the last applied document from the server as TOML files
    Pull {
        #[command(flatten)]
        location: Location,
        #[command(flatten)]
        target: TargetArgs,
        /// Overwrite local files that differ without asking
        #[arg(short = 'y', long)]
        yes: bool,
    },
}

/// Resolved local files plus the scope they compile in.
#[derive(Debug)]
struct Files {
    dir: PathBuf,
    scope: Scope,
    /// Server tree name for tree documents.
    tree: Option<String>,
    /// Origin tenant of the worktree, if known.
    origin_tenant: Option<String>,
}

impl Files {
    fn roles_path(&self) -> PathBuf {
        self.dir.join(declarative::ROLES_FILE)
    }

    fn policies_path(&self) -> PathBuf {
        self.dir.join(declarative::POLICIES_FILE)
    }
}

fn locate(ctx: &Context, location: &Location, tenant: Option<&str>) -> Result<Files> {
    let repo = match Repository::open(ctx.cwd()) {
        Ok(repo) => Some(repo),
        Err(SdkError::NotAWorktree) => None,
        Err(e) => return Err(e.into()),
    };
    if !location.tree_doc {
        let dir = match (&location.dir, &repo) {
            (Some(dir), _) => ctx.absolute(dir),
            (None, Some(repo)) => repo.root().join(declarative::TENANT_ACCESS_DIR),
            (None, None) => {
                return Err(CliError::usage("not inside a worktree").with_hint(
                    "pass --dir DIR (the directory holding roles.toml and policies.toml)",
                ))
            }
        };
        return Ok(Files {
            dir,
            scope: Scope::Tenant,
            tree: None,
            origin_tenant: cwd_origin_tenant(ctx),
        });
    }
    let repo = repo.ok_or_else(|| {
        CliError::usage(
            "--tree-doc needs a worktree (the tree is taken from the current directory or --tree)",
        )
    })?;
    let info = ctx.select_tree(&repo)?;
    let server_tree = default_tree(&repo, &info).ok();
    let dir = match &location.dir {
        Some(dir) => ctx.absolute(dir),
        None => repo
            .tree_dir(&info.name)?
            .join(declarative::TREE_ACCESS_DIR),
    };
    let name = server_tree
        .as_ref()
        .map(|t| t.name.clone())
        .unwrap_or_else(|| {
            if info.remote_name.is_empty() {
                info.name.clone()
            } else {
                info.remote_name.clone()
            }
        });
    let origin = server_tree.map(|t| t.tenant);
    let tenant = tenant.map(str::to_string).or_else(|| origin.clone());
    Ok(Files {
        dir,
        scope: Scope::Tree {
            tenant,
            tree: Some(name.clone()),
        },
        tree: Some(name),
        origin_tenant: origin,
    })
}

fn read_optional(path: &Path) -> Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(CliError::failure(format!(
            "cannot read '{}': {e}",
            path.display()
        ))),
    }
}

/// Compile the files; `Err` lists the diagnostics.
fn compile_files(files: &Files) -> Result<std::result::Result<Compiled, Vec<Diagnostic>>> {
    // Tree documents have no roles; a stray roles.toml there is reported
    // by the compiler (E2013).
    let roles = read_optional(&files.roles_path())?;
    let policies = read_optional(&files.policies_path())?;
    if roles.is_none() && policies.is_none() {
        return Err(CliError::failure(format!(
            "no access files in '{}' (expected {} and/or {})",
            files.dir.display(),
            declarative::ROLES_FILE,
            declarative::POLICIES_FILE
        )));
    }
    Ok(declarative::compile(
        roles.as_deref(),
        policies.as_deref(),
        &files.scope,
    ))
}

fn print_warnings(ctx: &Context, warnings: &[Diagnostic]) {
    if ctx.out.is_json() {
        return;
    }
    for w in warnings {
        eprintln!("{} {w}", "warning:".yellow().bold());
    }
}

fn diagnostics_text(t: &mut Text, diags: &[Diagnostic]) {
    for d in diags {
        t.line(format_args!("{} {d}", "error:".red().bold()));
    }
}

/// Compile, failing with every error listed.
fn compile_or_fail(ctx: &Context, files: &Files) -> Result<Compiled> {
    match compile_files(files)? {
        Ok(c) => {
            print_warnings(ctx, &c.warnings);
            Ok(c)
        }
        Err(errors) => {
            ctx.out.emit(
                &json!({"valid": false, "dir": files.dir, "errors": errors}),
                |t| diagnostics_text(t, &errors),
            )?;
            Err(CliError::failure(format!(
                "{} error(s) in the access files in '{}'",
                errors.len(),
                files.dir.display()
            )))
        }
    }
}

fn target_for(files: &Files, args: &TargetArgs) -> Result<AccessTarget> {
    let tenant = args
        .tenant
        .clone()
        .or_else(|| files.origin_tenant.clone())
        .ok_or_else(|| {
            CliError::usage("no tenant given and the worktree has no origin remote")
                .with_hint("pass --tenant SLUG")
        })?;
    Ok(AccessTarget {
        tenant,
        tree: files.tree.clone(),
    })
}

/// JSON output of plan / apply.
#[derive(Debug, Serialize)]
struct PlanOutput<'a> {
    target: String,
    #[serde(flatten)]
    plan: &'a AccessPlan,
    warnings: &'a [Diagnostic],
}

fn plan_text(t: &mut Text, target: &AccessTarget, plan: &AccessPlan, applied: bool) {
    if !plan.accepted {
        t.line(format_args!(
            "The server refused the access document for {target}:"
        ));
        for e in &plan.errors {
            t.line(format_args!("  {} {e}", "error:".red().bold()));
        }
        return;
    }
    if plan.changes.is_empty() {
        t.line(format_args!(
            "No changes: {target} matches the access files."
        ));
        return;
    }
    let verb = if applied { "Applied" } else { "Plan for" };
    t.line(format_args!("{verb} {target}:"));
    for c in &plan.changes {
        let marker = if c.ends_with(": delete") {
            "-".red()
        } else if c.contains(": create") {
            "+".green()
        } else {
            "~".yellow()
        };
        t.line(format_args!("  {marker} {c}"));
    }
    if !applied {
        t.blank();
        t.line(format_args!(
            "{} change(s). Run `wt access apply` to make them.",
            plan.changes.len()
        ));
    }
}

fn server(ctx: &Context) -> Result<String> {
    Ok(ctx.rest()?.server().to_string())
}

pub fn run(ctx: &Context, args: AccessArgs) -> Result<()> {
    match args.command {
        AccessCommand::Validate { location } => {
            let files = locate(ctx, &location, None)?;
            let compiled = compile_or_fail(ctx, &files)?;
            let doc = &compiled.document;
            ctx.out.emit(
                &json!({"valid": true, "dir": files.dir, "warnings": compiled.warnings,
                        "document": doc}),
                |t| {
                    t.line(format_args!(
                        "Access files in '{}' are valid: {} role(s), {} polic{}.",
                        files.dir.display(),
                        doc.roles.len(),
                        doc.policies.len(),
                        if doc.policies.len() == 1 { "y" } else { "ies" }
                    ));
                },
            )
        }
        AccessCommand::Plan { location, target } => {
            let files = locate(ctx, &location, target.tenant.as_deref())?;
            let compiled = compile_or_fail(ctx, &files)?;
            let target = target_for(&files, &target)?;
            let server = server(ctx)?;
            let plan = block_on(access::plan(&server, &target, &compiled.json))??;
            emit_plan(ctx, &target, &plan, &compiled.warnings, false)
        }
        AccessCommand::Apply {
            location,
            target,
            yes,
        } => {
            let files = locate(ctx, &location, target.tenant.as_deref())?;
            let compiled = compile_or_fail(ctx, &files)?;
            let target = target_for(&files, &target)?;
            let server = server(ctx)?;
            let plan = block_on(access::plan(&server, &target, &compiled.json))??;
            if !plan.accepted {
                return emit_plan(ctx, &target, &plan, &compiled.warnings, false);
            }
            if !plan.changes.is_empty() {
                if !ctx.out.is_json() {
                    let mut t = Text::default();
                    plan_text(&mut t, &target, &plan, false);
                    eprint!("{}", t.as_str());
                }
                confirm(
                    yes,
                    &format!("apply {} change(s) to {target}", plan.changes.len()),
                )?;
            }
            let applied = block_on(access::apply(&server, &target, &compiled.json))??;
            emit_plan(ctx, &target, &applied, &compiled.warnings, true)
        }
        AccessCommand::Pull {
            location,
            target,
            yes,
        } => {
            let files = locate(ctx, &location, target.tenant.as_deref())?;
            let target = target_for(&files, &target)?;
            let server = server(ctx)?;
            let pulled = block_on(access::pull(&server, &target))??.ok_or_else(|| {
                CliError::failure(format!("no access document has been applied to {target}"))
                    .with_hint("write the TOML files and run `wt access apply`")
            })?;
            let mut written = Vec::new();
            let mut outputs = vec![(
                files.policies_path(),
                declarative::policies_to_toml(&pulled.document.policies),
            )];
            if files.tree.is_none() {
                outputs.insert(
                    0,
                    (
                        files.roles_path(),
                        declarative::roles_to_toml(&pulled.document.roles),
                    ),
                );
            }
            for (path, text) in &outputs {
                let current = read_optional(path)?;
                if current.as_deref() == Some(text.as_str()) {
                    continue;
                }
                if current.is_some() {
                    confirm(yes, &format!("overwrite '{}'", path.display()))?;
                }
                write_file(path, text)?;
                written.push(path.clone());
            }
            ctx.out.emit(
                &json!({"target": target.to_string(), "written": written,
                        "updated_by": pulled.updated_by, "updated_at": pulled.updated_at,
                        "document": pulled.document}),
                |t| {
                    if written.is_empty() {
                        t.line(format_args!("Access files already match {target}."));
                    }
                    for p in &written {
                        t.line(format_args!("Wrote {}", p.display()));
                    }
                    if !pulled.updated_by.is_empty() {
                        t.line(format_args!("(last applied by {})", pulled.updated_by));
                    }
                },
            )
        }
    }
}

fn emit_plan(
    ctx: &Context,
    target: &AccessTarget,
    plan: &AccessPlan,
    warnings: &[Diagnostic],
    applied: bool,
) -> Result<()> {
    ctx.out.emit(
        &PlanOutput {
            target: target.to_string(),
            plan,
            warnings,
        },
        |t| plan_text(t, target, plan, applied),
    )?;
    if plan.accepted {
        Ok(())
    } else {
        Err(CliError::failure(format!(
            "the server refused the access document for {target}"
        )))
    }
}

fn write_file(path: &Path, text: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| CliError::failure(format!("cannot create '{}': {e}", parent.display())))?;
    }
    std::fs::write(path, text)
        .map_err(|e| CliError::failure(format!("cannot write '{}': {e}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Command;
    use crate::commands::server_common::testing::parse;

    fn access(args: &[&str]) -> AccessCommand {
        let mut argv = vec!["access"];
        argv.extend_from_slice(args);
        match parse(&argv) {
            Command::Access(a) => a.command,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn parses_subcommands() {
        assert!(matches!(
            access(&["validate", "--dir", "x"]),
            AccessCommand::Validate { location: Location { dir: Some(d), tree_doc: false } } if d == Path::new("x")
        ));
        assert!(matches!(
            access(&["plan", "--tenant", "acme", "--tree-doc"]),
            AccessCommand::Plan { location: Location { tree_doc: true, .. }, target: TargetArgs { tenant: Some(t) } } if t == "acme"
        ));
        assert!(matches!(
            access(&["apply", "--yes"]),
            AccessCommand::Apply { yes: true, .. }
        ));
        assert!(matches!(
            access(&["pull", "--tenant", "acme"]),
            AccessCommand::Pull { yes: false, .. }
        ));
    }

    #[test]
    fn plan_rendering() {
        let target = AccessTarget::tenant("acme");
        let plan = AccessPlan {
            accepted: true,
            errors: vec![],
            changes: vec![
                "role r1: create (1 permission)".into(),
                "policy p: update (2 statements)".into(),
                "policy old: delete".into(),
            ],
            dry_run: true,
        };
        colored::control::set_override(false);
        let mut t = Text::default();
        plan_text(&mut t, &target, &plan, false);
        let out = t.as_str();
        assert!(out.starts_with("Plan for tenant acme:\n"), "{out}");
        assert!(
            out.contains("  + role r1: create (1 permission)\n"),
            "{out}"
        );
        assert!(
            out.contains("  ~ policy p: update (2 statements)\n"),
            "{out}"
        );
        assert!(out.contains("  - policy old: delete\n"), "{out}");
        assert!(out.contains("3 change(s)"), "{out}");

        let mut t = Text::default();
        plan_text(
            &mut t,
            &target,
            &AccessPlan {
                changes: vec![],
                ..plan.clone()
            },
            true,
        );
        assert_eq!(
            t.as_str(),
            "No changes: tenant acme matches the access files.\n"
        );

        let mut t = Text::default();
        plan_text(
            &mut t,
            &AccessTarget::tree("acme", "web"),
            &AccessPlan {
                accepted: false,
                errors: vec!["role x grants tree:admin".into()],
                changes: vec![],
                dry_run: true,
            },
            false,
        );
        assert!(t
            .as_str()
            .contains("refused the access document for tree acme/web"));
        assert!(t.as_str().contains("error: role x grants tree:admin"));
    }

    #[test]
    fn target_defaults_to_the_origin_tenant() {
        let files = Files {
            dir: PathBuf::from("."),
            scope: Scope::Tenant,
            tree: None,
            origin_tenant: Some("acme".into()),
        };
        assert_eq!(
            target_for(&files, &TargetArgs::default()).unwrap(),
            AccessTarget::tenant("acme")
        );
        assert_eq!(
            target_for(
                &files,
                &TargetArgs {
                    tenant: Some("other".into())
                }
            )
            .unwrap()
            .tenant,
            "other"
        );
        let none = Files {
            origin_tenant: None,
            ..files
        };
        assert!(target_for(&none, &TargetArgs::default()).is_err());
    }
}
