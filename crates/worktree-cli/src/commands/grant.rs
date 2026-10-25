//! `wt grant` — Per-tree access grants for users, teams, service accounts
//! (§12.3 grants) and, since v0.5, whole tenants (§13.7: every member and
//! service account of another tenant).

use clap::{Args, Subcommand};
use serde_json::{json, Value};
use worktree_sdk::remote::rest::seg;

use crate::commands::iam_common::{call, confirm, grant_table, show, Grant, TreeRef, YesArgs};
use crate::context::Context;
use crate::error::Result;

#[derive(Debug, Args)]
pub struct GrantArgs {
    #[command(subcommand)]
    pub command: GrantCommand,
}

#[derive(Debug, Subcommand)]
pub enum GrantCommand {
    /// List the grants on a tree
    List {
        /// Server tree (TENANT/TREE)
        // Not `tree`: that id belongs to the global `--tree` flag.
        #[arg(id = "server_tree", value_name = "TREE")]
        tree: TreeRef,
    },
    /// Grant a role on a tree (replaces the subject's existing grant)
    Set {
        /// Server tree (TENANT/TREE)
        // Not `tree`: that id belongs to the global `--tree` flag.
        #[arg(id = "server_tree", value_name = "TREE")]
        tree: TreeRef,
        #[command(flatten)]
        subject: SubjectArgs,
        /// Built-in or custom role of the tree's tenant
        #[arg(long)]
        role: String,
    },
    /// Remove a subject's grant from a tree
    Remove {
        /// Server tree (TENANT/TREE)
        // Not `tree`: that id belongs to the global `--tree` flag.
        #[arg(id = "server_tree", value_name = "TREE")]
        tree: TreeRef,
        #[command(flatten)]
        subject: SubjectArgs,
        #[command(flatten)]
        yes: YesArgs,
    },
}

/// Exactly one of `--user`, `--team`, `--sa`.
#[derive(Debug, Args)]
#[group(required = true, multiple = false)]
pub struct SubjectArgs {
    /// A user, by username
    #[arg(long, value_name = "USER")]
    pub user: Option<String>,
    /// A team of the tree's tenant, by slug
    #[arg(long, value_name = "SLUG")]
    pub team: Option<String>,
    /// A service account, by name
    #[arg(long, value_name = "NAME")]
    pub sa: Option<String>,
    /// Every member and service account of another tenant, by slug
    /// (cross-tenant access to this tree)
    #[arg(long, value_name = "SLUG")]
    pub tenant: Option<String>,
}

impl SubjectArgs {
    /// (subject_type, subject) as the API spells them.
    pub fn subject(&self) -> (&'static str, &str) {
        match (&self.user, &self.team, &self.sa, &self.tenant) {
            (Some(user), ..) => ("user", user),
            (_, Some(team), ..) => ("team", team),
            (_, _, Some(sa), _) => ("service_account", sa),
            (_, _, _, Some(tenant)) => ("tenant", tenant),
            // clap enforces exactly one of the four.
            (None, None, None, None) => unreachable!("clap requires a subject"),
        }
    }
}

pub fn run(ctx: &Context, args: GrantArgs) -> Result<()> {
    let client = ctx.rest()?;
    match args.command {
        GrantCommand::List { tree } => {
            let grants: Value = call(client.get(&tree.path("/grants")))?;
            show(&ctx.out, &grants, |t, grants: &Vec<Grant>| {
                grant_table(grants).render(t, &format!("No grants on {tree}."));
            })
        }
        GrantCommand::Set {
            tree,
            subject,
            role,
        } => {
            let (kind, name) = subject.subject();
            let body = json!({"subject_type": kind, "subject": name, "role": role});
            let grant: Value = call(client.put(&tree.path("/grants"), &body))?;
            show(&ctx.out, &grant, |t, g: &Grant| {
                t.line(format_args!(
                    "Granted {} to {} {} on {tree}",
                    g.role, g.subject_type, g.subject
                ));
            })
        }
        GrantCommand::Remove { tree, subject, yes } => {
            let (kind, name) = subject.subject();
            confirm(
                yes.yes,
                &format!("remove the grant of {kind} {name} on {tree}"),
            )?;
            let path = tree.path(&format!(
                "/grants?subject_type={}&subject={}",
                seg(kind),
                seg(name)
            ));
            call(client.delete(&path))?;
            ctx.out.emit(
                &json!({"tree": tree.to_string(), "subject_type": kind, "subject": name, "removed": true}),
                |t| t.line(format_args!("Removed the grant of {kind} {name} on {tree}")),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::error::ErrorKind;

    use super::*;
    use crate::cli::Command;
    use crate::commands::iam_common::{parse, parse_err};

    fn grant(args: &[&str]) -> GrantCommand {
        let mut argv = vec!["grant"];
        argv.extend_from_slice(args);
        match parse(&argv) {
            Command::Grant(a) => a.command,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn parses_subcommands() {
        assert!(matches!(
            grant(&["list", "acme/web"]),
            GrantCommand::List { tree } if tree.tenant == "acme" && tree.name == "web"
        ));
        let GrantCommand::Set { subject, role, .. } =
            grant(&["set", "acme/web", "--team", "core", "--role", "maintainer"])
        else {
            panic!()
        };
        assert_eq!(
            (subject.subject(), role.as_str()),
            (("team", "core"), "maintainer")
        );
        let GrantCommand::Remove { subject, yes, .. } =
            grant(&["remove", "acme/web", "--sa", "ci", "--yes"])
        else {
            panic!()
        };
        assert_eq!(subject.subject(), ("service_account", "ci"));
        assert!(yes.yes);
        let GrantCommand::Set { subject, .. } =
            grant(&["set", "acme/web", "--user", "ada", "--role", "viewer"])
        else {
            panic!()
        };
        assert_eq!(subject.subject(), ("user", "ada"));
        let GrantCommand::Set { subject, .. } =
            grant(&["set", "acme/web", "--tenant", "partner", "--role", "viewer"])
        else {
            panic!()
        };
        assert_eq!(subject.subject(), ("tenant", "partner"));
        assert_eq!(
            parse_err(&["grant", "set", "acme/web", "--tenant", "a", "--sa", "b", "--role", "r"]),
            ErrorKind::ArgumentConflict
        );

        assert_eq!(
            parse_err(&["grant", "set", "acme/web", "--role", "viewer"]),
            ErrorKind::MissingRequiredArgument
        );
        assert_eq!(
            parse_err(&["grant", "set", "acme/web", "--user", "a", "--team", "t", "--role", "r"]),
            ErrorKind::ArgumentConflict
        );
        assert_eq!(
            parse_err(&["grant", "list", "web"]),
            ErrorKind::ValueValidation
        );
    }
}
