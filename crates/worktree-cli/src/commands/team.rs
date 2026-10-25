//! `wt team` — Teams within a tenant and their members (§12.3 teams).

use clap::{Args, Subcommand};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use worktree_sdk::remote::rest::seg;

use crate::commands::iam_common::{
    call, confirm, fields, grant_table, show, tenant_path, when, AccountRef, Grant, Table, YesArgs,
};
use crate::context::Context;
use crate::error::{CliError, Result};
use crate::output::Text;

#[derive(Debug, Args)]
pub struct TeamArgs {
    #[command(subcommand)]
    pub command: TeamCommand,
}

#[derive(Debug, Subcommand)]
pub enum TeamCommand {
    /// List the teams of a tenant
    List {
        /// Tenant slug
        tenant: String,
    },
    /// Create a team
    Create {
        /// Tenant slug
        tenant: String,
        /// Team slug ([a-z0-9-])
        slug: String,
        /// Display name (default: the slug)
        #[arg(long)]
        name: Option<String>,
        /// Description
        #[arg(long, default_value = "")]
        description: String,
    },
    /// Show a team with its members and tree grants
    Show {
        /// Tenant slug
        tenant: String,
        /// Team slug
        slug: String,
    },
    /// Change a team's name or description
    Update {
        /// Tenant slug
        tenant: String,
        /// Team slug
        slug: String,
        /// New display name
        #[arg(long)]
        name: Option<String>,
        /// New description
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete a team (its memberships and grants go with it)
    Delete {
        /// Tenant slug
        tenant: String,
        /// Team slug
        slug: String,
        #[command(flatten)]
        yes: YesArgs,
    },
    /// Add a member to a team (or change their team role)
    Add {
        /// Tenant slug
        tenant: String,
        /// Team slug
        slug: String,
        /// Username
        user: String,
        /// Make the member a team maintainer (may manage the team's membership)
        #[arg(long)]
        maintainer: bool,
    },
    /// Remove a member from a team
    Remove {
        /// Tenant slug
        tenant: String,
        /// Team slug
        slug: String,
        /// Username
        user: String,
        #[command(flatten)]
        yes: YesArgs,
    },
}

#[derive(Debug, Deserialize)]
struct Team {
    slug: String,
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    created_at: String,
}

#[derive(Debug, Deserialize)]
struct Member {
    account: AccountRef,
    role: String,
}

#[derive(Debug, Deserialize)]
struct TeamDetail {
    team: Team,
    #[serde(default)]
    members: Vec<Member>,
    #[serde(default)]
    grants: Vec<Grant>,
}

#[derive(Serialize)]
struct TeamPatch<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<&'a str>,
}

fn team_path(tenant: &str, slug: &str, rest: &str) -> String {
    tenant_path(tenant, &format!("/teams/{}{rest}", seg(slug)))
}

pub fn run(ctx: &Context, args: TeamArgs) -> Result<()> {
    let client = ctx.rest()?;
    match args.command {
        TeamCommand::List { tenant } => {
            let teams: Value = call(client.get(&tenant_path(&tenant, "/teams")))?;
            show(&ctx.out, &teams, |t, teams: &Vec<Team>| team_list(t, teams))
        }
        TeamCommand::Create {
            tenant,
            slug,
            name,
            description,
        } => {
            let body = json!({
                "slug": slug,
                "name": name.as_deref().unwrap_or(&slug),
                "description": description,
            });
            let team: Value = call(client.post(&tenant_path(&tenant, "/teams"), &body))?;
            show(&ctx.out, &team, |t, team: &Team| {
                t.line(format_args!("Created team {tenant}/{}", team.slug));
            })
        }
        TeamCommand::Show { tenant, slug } => {
            let detail: Value = call(client.get(&team_path(&tenant, &slug, "")))?;
            show(&ctx.out, &detail, team_detail)
        }
        TeamCommand::Update {
            tenant,
            slug,
            name,
            description,
        } => {
            if name.is_none() && description.is_none() {
                return Err(CliError::usage("nothing to update")
                    .with_hint("pass --name and/or --description"));
            }
            let patch = TeamPatch {
                name: name.as_deref(),
                description: description.as_deref(),
            };
            let team: Value = call(client.patch(&team_path(&tenant, &slug, ""), &patch))?;
            show(&ctx.out, &team, |t, team: &Team| {
                t.line(format_args!("Updated team {tenant}/{}", team.slug));
            })
        }
        TeamCommand::Delete { tenant, slug, yes } => {
            confirm(yes.yes, &format!("delete team {tenant}/{slug}"))?;
            call(client.delete(&team_path(&tenant, &slug, "")))?;
            ctx.out.emit(
                &json!({"tenant": tenant, "team": slug, "deleted": true}),
                |t| t.line(format_args!("Deleted team {tenant}/{slug}")),
            )
        }
        TeamCommand::Add {
            tenant,
            slug,
            user,
            maintainer,
        } => {
            let role = if maintainer { "maintainer" } else { "member" };
            let path = team_path(&tenant, &slug, &format!("/members/{}", seg(&user)));
            let member: Value = call(client.put(&path, &json!({ "role": role })))?;
            ctx.out.emit(&member, |t| {
                t.line(format_args!(
                    "Added {user} to team {tenant}/{slug} as {role}"
                ));
            })
        }
        TeamCommand::Remove {
            tenant,
            slug,
            user,
            yes,
        } => {
            confirm(yes.yes, &format!("remove {user} from team {tenant}/{slug}"))?;
            let path = team_path(&tenant, &slug, &format!("/members/{}", seg(&user)));
            call(client.delete(&path))?;
            ctx.out.emit(
                &json!({"tenant": tenant, "team": slug, "username": user, "removed": true}),
                |t| t.line(format_args!("Removed {user} from team {tenant}/{slug}")),
            )
        }
    }
}

fn team_list(t: &mut Text, teams: &[Team]) {
    let mut table = Table::new(&["slug", "name", "description", "created"]);
    for team in teams {
        table.row([
            team.slug.clone(),
            team.name.clone(),
            team.description.clone(),
            when(&team.created_at),
        ]);
    }
    table.render(t, "No teams.");
}

fn team_detail(t: &mut Text, detail: &TeamDetail) {
    let team = &detail.team;
    fields(
        t,
        &[
            ("team", team.slug.clone()),
            ("name", team.name.clone()),
            ("description", team.description.clone()),
            ("created", when(&team.created_at)),
        ],
    );
    t.blank();
    t.line("Members:");
    let mut members = Table::new(&["username", "role"]);
    for m in &detail.members {
        members.row([m.account.username.clone(), m.role.clone()]);
    }
    members.render(t, "(none)");
    t.blank();
    t.line("Tree grants:");
    grant_table(&detail.grants).render(t, "(none)");
}

#[cfg(test)]
mod tests {
    use clap::error::ErrorKind;

    use super::*;
    use crate::cli::Command;
    use crate::commands::iam_common::{parse, parse_err};

    fn team(args: &[&str]) -> TeamCommand {
        let mut argv = vec!["team"];
        argv.extend_from_slice(args);
        match parse(&argv) {
            Command::Team(a) => a.command,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn parses_subcommands() {
        assert!(
            matches!(team(&["list", "acme"]), TeamCommand::List { tenant } if tenant == "acme")
        );
        assert!(matches!(
            team(&["create", "acme", "core", "--name", "Core", "--description", "d"]),
            TeamCommand::Create { tenant, slug, name: Some(n), description }
                if tenant == "acme" && slug == "core" && n == "Core" && description == "d"
        ));
        assert!(matches!(
            team(&["create", "acme", "core"]),
            TeamCommand::Create { name: None, description, .. } if description.is_empty()
        ));
        assert!(matches!(
            team(&["show", "acme", "core"]),
            TeamCommand::Show { .. }
        ));
        assert!(matches!(
            team(&["update", "acme", "core", "--description", "x"]),
            TeamCommand::Update { name: None, description: Some(d), .. } if d == "x"
        ));
        assert!(matches!(
            team(&["delete", "acme", "core", "--yes"]),
            TeamCommand::Delete {
                yes: YesArgs { yes: true },
                ..
            }
        ));
        assert!(matches!(
            team(&["add", "acme", "core", "ada", "--maintainer"]),
            TeamCommand::Add { user, maintainer: true, .. } if user == "ada"
        ));
        assert!(matches!(
            team(&["remove", "acme", "core", "ada", "-y"]),
            TeamCommand::Remove {
                yes: YesArgs { yes: true },
                ..
            }
        ));
        assert_eq!(
            parse_err(&["team", "add", "acme", "core"]),
            ErrorKind::MissingRequiredArgument
        );
    }

    #[test]
    fn renders_team_list_and_detail() {
        let teams: Vec<Team> = serde_json::from_value(json!([
            {"id": "t1", "slug": "core", "name": "Core", "description": "", "created_at": "2026-01-02T03:04:05Z"}
        ]))
        .unwrap();
        let mut t = Text::default();
        team_list(&mut t, &teams);
        assert!(
            t.as_str().starts_with("SLUG  NAME  DESCRIPTION  CREATED\n"),
            "{}",
            t.as_str()
        );
        assert!(t.as_str().contains("core  Core  -"), "{}", t.as_str());

        let detail: TeamDetail = serde_json::from_value(json!({
            "team": {"slug": "core", "name": "Core"},
            "members": [{"account": {"id": "a1", "username": "ada"}, "role": "maintainer"}],
            "grants": [{"subject_type": "team", "subject": "core", "role": "developer", "tree_id": "x"}]
        }))
        .unwrap();
        let mut t = Text::default();
        team_detail(&mut t, &detail);
        let out = t.as_str();
        assert!(out.contains("ada       maintainer"), "{out}");
        assert!(out.contains("core     team  developer"), "{out}");
    }

    #[test]
    fn team_paths_are_encoded() {
        assert_eq!(
            team_path("acme", "a b", "/members/x"),
            "/api/v1/tenants/acme/teams/a%20b/members/x"
        );
    }
}
