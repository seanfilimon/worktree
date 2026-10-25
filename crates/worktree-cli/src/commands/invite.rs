//! `wt invite` — Tenant invitations (§12.2, §12.3 invitations).

use clap::{Args, Subcommand};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use worktree_sdk::remote::rest::seg;

use crate::commands::iam_common::{call, confirm, fields, show, tenant_path, when, Table, YesArgs};
use crate::context::Context;
use crate::error::Result;
use crate::output::Text;

#[derive(Debug, Args)]
pub struct InviteArgs {
    #[command(subcommand)]
    pub command: InviteCommand,
}

#[derive(Debug, Subcommand)]
pub enum InviteCommand {
    /// List a tenant's invitations
    List {
        /// Tenant slug
        tenant: String,
    },
    /// Invite someone to a tenant; prints a one-time code
    Create {
        /// Tenant slug
        tenant: String,
        #[command(flatten)]
        invitee: Invitee,
        /// Tenant role granted on acceptance
        #[arg(long, default_value = "developer")]
        role: String,
        /// Also add the invitee to this team
        #[arg(long, value_name = "SLUG")]
        team: Option<String>,
    },
    /// Revoke a pending invitation
    Revoke {
        /// Tenant slug
        tenant: String,
        /// Invitation id
        id: String,
        #[command(flatten)]
        yes: YesArgs,
    },
    /// Preview the invitation behind a code
    Show {
        /// Invitation code (`wti_...`)
        code: String,
    },
    /// Accept an invitation as the signed-in account
    Accept {
        /// Invitation code (`wti_...`)
        code: String,
    },
}

/// Exactly one of `--email`, `--user`.
#[derive(Debug, Args, Serialize)]
#[group(required = true, multiple = false)]
pub struct Invitee {
    /// Invite by email address
    #[arg(long, value_name = "EMAIL")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// Invite an existing account by username
    #[arg(long = "user", value_name = "USER")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Invitation {
    id: String,
    #[serde(default)]
    email: String,
    #[serde(default)]
    username: String,
    role: String,
    #[serde(default)]
    team_id: String,
    #[serde(default)]
    invited_by: String,
    #[serde(default)]
    expires_at: String,
    #[serde(default)]
    accepted_by: String,
    #[serde(default)]
    revoked: bool,
}

impl Invitation {
    fn invitee(&self) -> &str {
        if self.username.is_empty() {
            &self.email
        } else {
            &self.username
        }
    }

    fn status(&self) -> String {
        if self.revoked {
            "revoked".into()
        } else if !self.accepted_by.is_empty() {
            format!("accepted by {}", self.accepted_by)
        } else if chrono::DateTime::parse_from_rfc3339(&self.expires_at)
            .is_ok_and(|at| at < chrono::Utc::now())
        {
            "expired".into()
        } else {
            "pending".into()
        }
    }
}

#[derive(Debug, Deserialize)]
struct Created {
    invitation: Invitation,
    code: String,
}

#[derive(Debug, Deserialize)]
struct TenantRef {
    slug: String,
    #[serde(default)]
    name: String,
}

#[derive(Debug, Deserialize)]
struct Preview {
    invitation: Invitation,
    tenant: TenantRef,
}

#[derive(Serialize)]
struct NewInvitation<'a> {
    #[serde(flatten)]
    invitee: &'a Invitee,
    role: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    team: Option<&'a str>,
}

pub fn run(ctx: &Context, args: InviteArgs) -> Result<()> {
    let client = ctx.rest()?;
    match args.command {
        InviteCommand::List { tenant } => {
            let list: Value = call(client.get(&tenant_path(&tenant, "/invitations")))?;
            show(&ctx.out, &list, |t, list: &Vec<Invitation>| {
                invitation_list(t, list)
            })
        }
        InviteCommand::Create {
            tenant,
            invitee,
            role,
            team,
        } => {
            let body = NewInvitation {
                invitee: &invitee,
                role: &role,
                team: team.as_deref(),
            };
            let created: Value = call(client.post(&tenant_path(&tenant, "/invitations"), &body))?;
            show(&ctx.out, &created, |t, c: &Created| {
                created_text(t, &tenant, client.server(), c);
            })
        }
        InviteCommand::Revoke { tenant, id, yes } => {
            confirm(yes.yes, &format!("revoke invitation {id} to {tenant}"))?;
            let path = tenant_path(&tenant, &format!("/invitations/{}", seg(&id)));
            call(client.delete(&path))?;
            ctx.out
                .emit(&json!({"tenant": tenant, "id": id, "revoked": true}), |t| {
                    t.line(format_args!("Revoked invitation {id}"))
                })
        }
        InviteCommand::Show { code } => {
            let preview: Value = call(client.get(&invitation_path(&code, "")))?;
            show(&ctx.out, &preview, preview_text)
        }
        InviteCommand::Accept { code } => {
            let path = invitation_path(&code, "/accept");
            let invitation: Value = call(client.post(&path, &json!({})))?;
            show(&ctx.out, &invitation, |t, inv: &Invitation| {
                t.line(format_args!(
                    "Invitation accepted; you joined as {}",
                    inv.role
                ));
            })
        }
    }
}

fn invitation_path(code: &str, rest: &str) -> String {
    format!("/api/v1/invitations/{}{rest}", seg(code.trim()))
}

fn invitation_list(t: &mut Text, list: &[Invitation]) {
    let mut table = Table::new(&[
        "id",
        "invitee",
        "role",
        "team",
        "status",
        "expires",
        "invited by",
    ]);
    for inv in list {
        table.row([
            inv.id.clone(),
            inv.invitee().to_string(),
            inv.role.clone(),
            inv.team_id.clone(),
            inv.status(),
            when(&inv.expires_at),
            inv.invited_by.clone(),
        ]);
    }
    table.render(t, "No invitations.");
}

fn created_text(t: &mut Text, tenant: &str, server: &str, c: &Created) {
    t.line(format_args!(
        "Invited {} to {tenant} as {} (expires {})",
        c.invitation.invitee(),
        c.invitation.role,
        when(&c.invitation.expires_at)
    ));
    t.blank();
    t.line(format_args!("    {}", c.code));
    t.blank();
    t.line("Send them the code; it works once. To accept, they run:");
    t.line(format_args!(
        "    wt --server {server} invite accept {}",
        c.code
    ));
}

fn preview_text(t: &mut Text, p: &Preview) {
    let tenant = if p.tenant.name.is_empty() || p.tenant.name == p.tenant.slug {
        p.tenant.slug.clone()
    } else {
        format!("{} ({})", p.tenant.slug, p.tenant.name)
    };
    let inv = &p.invitation;
    fields(
        t,
        &[
            ("tenant", tenant),
            ("role", inv.role.clone()),
            ("for", inv.invitee().to_string()),
            ("invited by", inv.invited_by.clone()),
            ("expires", when(&inv.expires_at)),
            ("status", inv.status()),
        ],
    );
}

#[cfg(test)]
mod tests {
    use clap::error::ErrorKind;

    use super::*;
    use crate::cli::Command;
    use crate::commands::iam_common::{parse, parse_err};

    fn invite(args: &[&str]) -> InviteCommand {
        let mut argv = vec!["invite"];
        argv.extend_from_slice(args);
        match parse(&argv) {
            Command::Invite(a) => a.command,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn parses_subcommands() {
        assert!(matches!(
            invite(&["list", "acme"]),
            InviteCommand::List { .. }
        ));
        assert!(matches!(
            invite(&["create", "acme", "--email", "bo@x.io"]),
            InviteCommand::Create { invitee: Invitee { email: Some(_), username: None }, role, team: None, .. }
                if role == "developer"
        ));
        assert!(matches!(
            invite(&["create", "acme", "--user", "bo", "--role", "viewer", "--team", "core"]),
            InviteCommand::Create { invitee: Invitee { username: Some(u), .. }, role, team: Some(t), .. }
                if u == "bo" && role == "viewer" && t == "core"
        ));
        assert_eq!(
            parse_err(&["invite", "create", "acme"]),
            ErrorKind::MissingRequiredArgument
        );
        assert_eq!(
            parse_err(&["invite", "create", "acme", "--email", "e", "--user", "u"]),
            ErrorKind::ArgumentConflict
        );
        assert!(matches!(
            invite(&["revoke", "acme", "i1", "--yes"]),
            InviteCommand::Revoke {
                yes: YesArgs { yes: true },
                ..
            }
        ));
        assert!(
            matches!(invite(&["show", "wti_x"]), InviteCommand::Show { code } if code == "wti_x")
        );
        assert!(matches!(
            invite(&["accept", "wti_x"]),
            InviteCommand::Accept { .. }
        ));
    }

    fn invitation(extra: Value) -> Invitation {
        let mut base = json!({"id": "i1", "role": "developer", "email": "bo@x.io",
                              "invited_by": "ada", "expires_at": "2999-01-01T00:00:00Z"});
        base.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        serde_json::from_value(base).unwrap()
    }

    #[test]
    fn invitation_status() {
        assert_eq!(invitation(json!({})).status(), "pending");
        assert_eq!(invitation(json!({"revoked": true})).status(), "revoked");
        assert_eq!(
            invitation(json!({"accepted_by": "bo"})).status(),
            "accepted by bo"
        );
        assert_eq!(
            invitation(json!({"expires_at": "2000-01-01T00:00:00Z"})).status(),
            "expired"
        );
        assert_eq!(invitation(json!({"username": "bo"})).invitee(), "bo");
    }

    #[test]
    fn created_invitation_prints_code_and_accept_command() {
        let c = Created {
            invitation: invitation(json!({})),
            code: "wti_abc".into(),
        };
        let mut t = Text::default();
        created_text(&mut t, "acme", "http://localhost:7420", &c);
        let out = t.as_str();
        assert!(
            out.contains("Invited bo@x.io to acme as developer"),
            "{out}"
        );
        assert!(out.contains("    wti_abc\n"), "{out}");
        assert!(
            out.contains("wt --server http://localhost:7420 invite accept wti_abc"),
            "{out}"
        );
    }

    #[test]
    fn request_body_has_one_invitee() {
        let invitee = Invitee {
            email: None,
            username: Some("bo".into()),
        };
        let body = NewInvitation {
            invitee: &invitee,
            role: "viewer",
            team: Some("core"),
        };
        assert_eq!(
            serde_json::to_value(&body).unwrap(),
            json!({"username": "bo", "role": "viewer", "team": "core"})
        );
    }

    #[test]
    fn renders_list_and_preview() {
        let mut t = Text::default();
        invitation_list(&mut t, &[invitation(json!({"team_id": "tm1"}))]);
        assert!(
            t.as_str().contains("i1  bo@x.io  developer  tm1   pending"),
            "{}",
            t.as_str()
        );
        let p = Preview {
            invitation: invitation(json!({})),
            tenant: TenantRef {
                slug: "acme".into(),
                name: "Acme Inc".into(),
            },
        };
        let mut t = Text::default();
        preview_text(&mut t, &p);
        assert!(
            t.as_str().contains("tenant:     acme (Acme Inc)"),
            "{}",
            t.as_str()
        );
    }
}
