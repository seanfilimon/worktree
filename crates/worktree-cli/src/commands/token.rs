//! `wt token` — Your personal API tokens (§7 `/auth/tokens`, scopes §12.3).

use clap::{Args, Subcommand};
use serde_json::{json, Value};
use worktree_sdk::remote::rest::seg;

use crate::commands::iam_common::{
    call, confirm, reveal_token, show, token_table, ApiToken, CreatedToken, NewToken, YesArgs,
};
use crate::context::Context;
use crate::error::Result;

const TOKENS_PATH: &str = "/api/v1/auth/tokens";

#[derive(Debug, Args)]
pub struct TokenArgs {
    #[command(subcommand)]
    pub command: TokenCommand,
}

#[derive(Debug, Subcommand)]
pub enum TokenCommand {
    /// List your API tokens
    List,
    /// Create an API token; the secret is shown once
    Create {
        /// Token name
        name: String,
        /// Expire after this many hours (default: never)
        #[arg(long, value_name = "N")]
        ttl_hours: Option<u32>,
        /// Restrict the token to an action pattern (`tree:read`, `tree:*`); repeatable
        #[arg(long = "scope", value_name = "SCOPE")]
        scopes: Vec<String>,
    },
    /// Revoke an API token
    Revoke {
        /// Token id
        id: String,
        #[command(flatten)]
        yes: YesArgs,
    },
}

pub fn run(ctx: &Context, args: TokenArgs) -> Result<()> {
    let client = ctx.rest()?;
    match args.command {
        TokenCommand::List => {
            let tokens: Value = call(client.get(TOKENS_PATH))?;
            show(&ctx.out, &tokens, |t, tokens: &Vec<ApiToken>| {
                token_table(tokens).render(t, "No API tokens.");
            })
        }
        TokenCommand::Create {
            name,
            ttl_hours,
            scopes,
        } => {
            let body = NewToken {
                name: &name,
                ttl_hours,
                scopes: &scopes,
            };
            let created: Value = call(client.post(TOKENS_PATH, &body))?;
            show(&ctx.out, &created, |t, c: &CreatedToken| {
                reveal_token(t, "API token", c);
            })
        }
        TokenCommand::Revoke { id, yes } => {
            confirm(yes.yes, &format!("revoke API token {id}"))?;
            call(client.delete(&format!("{TOKENS_PATH}/{}", seg(&id))))?;
            ctx.out.emit(&json!({"id": id, "revoked": true}), |t| {
                t.line(format_args!("Revoked API token {id}"));
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Command;
    use crate::commands::iam_common::parse;

    fn token(args: &[&str]) -> TokenCommand {
        let mut argv = vec!["token"];
        argv.extend_from_slice(args);
        match parse(&argv) {
            Command::Token(a) => a.command,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn parses_subcommands() {
        assert!(matches!(token(&["list"]), TokenCommand::List));
        assert!(matches!(
            token(&["create", "laptop", "--ttl-hours", "8", "--scope", "tree:read"]),
            TokenCommand::Create { name, ttl_hours: Some(8), scopes }
                if name == "laptop" && scopes == ["tree:read"]
        ));
        assert!(matches!(
            token(&["revoke", "t1", "--yes"]),
            TokenCommand::Revoke { id, yes: YesArgs { yes: true } } if id == "t1"
        ));
    }

    #[test]
    fn create_body_omits_unset_ttl() {
        let scopes = vec![];
        let body = NewToken {
            name: "laptop",
            ttl_hours: None,
            scopes: &scopes,
        };
        assert_eq!(
            serde_json::to_value(&body).unwrap(),
            json!({"name": "laptop", "scopes": []})
        );
    }
}
