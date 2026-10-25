//! `wt sso` — Single sign-on (OpenID Connect) configuration (§12.3 iam-sso;
//! server admins only).

use clap::{ArgAction, Args, Subcommand};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::commands::auth::first_line;
use crate::commands::iam_common::{call, fields, show, when};
use crate::context::Context;
use crate::error::{CliError, Result};
use crate::output::Text;

const SSO_PATH: &str = "/api/v1/admin/sso";

#[derive(Debug, Args)]
pub struct SsoArgs {
    #[command(subcommand)]
    pub command: SsoCommand,
}

#[derive(Debug, Subcommand)]
pub enum SsoCommand {
    /// Show the SSO configuration
    Show,
    /// Change the SSO configuration (unset options keep their value)
    Set(SetArgs),
}

#[derive(Debug, Args)]
pub struct SetArgs {
    /// Turn SSO login on or off
    #[arg(long, value_name = "BOOL", action = ArgAction::Set)]
    pub enabled: Option<bool>,
    /// Provider name shown on the login button, e.g. "Okta"
    #[arg(long)]
    pub name: Option<String>,
    /// OpenID Connect issuer URL
    #[arg(long, value_name = "URL")]
    pub issuer: Option<String>,
    /// OAuth client id
    #[arg(long, value_name = "ID")]
    pub client_id: Option<String>,
    /// Read the OAuth client secret from the first line of stdin
    #[arg(long)]
    pub client_secret_stdin: bool,
    /// Callback URL registered with the provider (…/api/v1/auth/sso/callback)
    #[arg(long, value_name = "URL")]
    pub redirect_url: Option<String>,
    /// Requested scope; repeatable, replaces the list
    #[arg(long = "scope", value_name = "SCOPE")]
    pub scopes: Vec<String>,
    /// Email domain allowed to sign in; repeatable, replaces the list
    #[arg(long = "allowed-domain", value_name = "DOMAIN")]
    pub allowed_domains: Vec<String>,
    /// Create accounts on first SSO login
    #[arg(long, value_name = "BOOL", action = ArgAction::Set)]
    pub auto_create: Option<bool>,
    /// ID-token claim used as the username
    #[arg(long, value_name = "CLAIM")]
    pub username_claim: Option<String>,
}

/// The configuration as returned by GET and sent (plus the secret) by PUT.
#[derive(Debug, Default, Serialize, Deserialize)]
struct SsoConfig {
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    name: String,
    #[serde(default)]
    issuer: String,
    #[serde(default)]
    client_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    client_secret: Option<String>,
    #[serde(default, skip_serializing)]
    has_client_secret: bool,
    #[serde(default)]
    redirect_url: String,
    #[serde(default)]
    scopes: Vec<String>,
    #[serde(default)]
    allowed_domains: Vec<String>,
    #[serde(default)]
    auto_create: bool,
    #[serde(default)]
    username_claim: String,
    #[serde(default, skip_serializing)]
    updated_at: String,
}

impl SetArgs {
    fn is_empty(&self) -> bool {
        self.enabled.is_none()
            && self.name.is_none()
            && self.issuer.is_none()
            && self.client_id.is_none()
            && !self.client_secret_stdin
            && self.redirect_url.is_none()
            && self.scopes.is_empty()
            && self.allowed_domains.is_empty()
            && self.auto_create.is_none()
            && self.username_claim.is_none()
    }

    /// Overlay the given options on `config`.
    fn apply(self, config: &mut SsoConfig, client_secret: Option<String>) {
        fn set<T>(slot: &mut T, value: Option<T>) {
            if let Some(v) = value {
                *slot = v;
            }
        }
        set(&mut config.enabled, self.enabled);
        set(&mut config.name, self.name);
        set(&mut config.issuer, self.issuer);
        set(&mut config.client_id, self.client_id);
        set(&mut config.redirect_url, self.redirect_url);
        set(&mut config.auto_create, self.auto_create);
        set(&mut config.username_claim, self.username_claim);
        if !self.scopes.is_empty() {
            config.scopes = self.scopes;
        }
        if !self.allowed_domains.is_empty() {
            config.allowed_domains = self.allowed_domains;
        }
        config.client_secret = client_secret;
    }
}

pub fn run(ctx: &Context, args: SsoArgs) -> Result<()> {
    match args.command {
        SsoCommand::Show => {
            let config: Value = call(ctx.rest()?.get(SSO_PATH))?;
            show(&ctx.out, &config, config_text)
        }
        SsoCommand::Set(set) => {
            if set.is_empty() {
                return Err(CliError::usage("nothing to change")
                    .with_hint("see `wt sso set --help` for the options"));
            }
            let secret = if set.client_secret_stdin {
                let secret = first_line(std::io::stdin().lock())?;
                if secret.is_empty() {
                    return Err(CliError::failure("the client secret must not be empty"));
                }
                Some(secret)
            } else {
                None
            };
            let client = ctx.rest()?;
            let current: Value = call(client.get(SSO_PATH))?;
            let mut config: SsoConfig = serde_json::from_value(current).map_err(|e| {
                CliError::failure(format!("unexpected response from the server: {e}"))
            })?;
            set.apply(&mut config, secret);
            let saved: Value = call(client.put(SSO_PATH, &config))?;
            show(&ctx.out, &saved, |t, c: &SsoConfig| {
                t.line("SSO configuration saved.");
                t.blank();
                config_text(t, c);
            })
        }
    }
}

fn config_text(t: &mut Text, c: &SsoConfig) {
    let onoff = |b: bool| if b { "yes" } else { "no" }.to_string();
    fields(
        t,
        &[
            ("enabled", onoff(c.enabled)),
            ("name", c.name.clone()),
            ("issuer", c.issuer.clone()),
            ("client id", c.client_id.clone()),
            (
                "client secret",
                if c.has_client_secret { "(set)" } else { "" }.into(),
            ),
            ("redirect url", c.redirect_url.clone()),
            ("scopes", c.scopes.join(" ")),
            ("allowed domains", c.allowed_domains.join(", ")),
            ("auto-create", onoff(c.auto_create)),
            ("username claim", c.username_claim.clone()),
            ("updated", when(&c.updated_at)),
        ],
    );
}

#[cfg(test)]
mod tests {
    use clap::error::ErrorKind;
    use serde_json::json;

    use super::*;
    use crate::cli::Command;
    use crate::commands::iam_common::{parse, parse_err};

    #[test]
    fn parses_subcommands() {
        let Command::Sso(a) = parse(&["sso", "show"]) else {
            panic!()
        };
        assert!(matches!(a.command, SsoCommand::Show));
        let Command::Sso(a) = parse(&[
            "sso",
            "set",
            "--enabled",
            "true",
            "--name",
            "Okta",
            "--issuer",
            "https://idp",
            "--client-id",
            "c1",
            "--client-secret-stdin",
            "--redirect-url",
            "https://wt/cb",
            "--allowed-domain",
            "a.io",
            "--allowed-domain",
            "b.io",
            "--scope",
            "openid",
            "--auto-create",
            "false",
            "--username-claim",
            "email",
        ]) else {
            panic!()
        };
        let SsoCommand::Set(set) = a.command else {
            panic!()
        };
        assert_eq!(set.enabled, Some(true));
        assert_eq!(set.auto_create, Some(false));
        assert!(set.client_secret_stdin);
        assert_eq!(set.allowed_domains, ["a.io", "b.io"]);
        assert_eq!(set.scopes, ["openid"]);
        assert_eq!(set.username_claim.as_deref(), Some("email"));
        assert_eq!(
            parse_err(&["sso", "set", "--enabled", "maybe"]),
            ErrorKind::InvalidValue
        );
    }

    #[test]
    fn set_overlays_only_given_options() {
        let mut config: SsoConfig = serde_json::from_value(json!({
            "enabled": false, "name": "Okta", "issuer": "https://idp", "client_id": "c1",
            "has_client_secret": true, "redirect_url": "https://wt/cb", "scopes": ["openid"],
            "allowed_domains": ["acme.io"], "auto_create": true, "username_claim": "preferred_username",
            "updated_at": "2026-01-01T00:00:00Z"
        }))
        .unwrap();
        let args = SetArgs {
            enabled: Some(true),
            name: None,
            issuer: None,
            client_id: Some("c2".into()),
            client_secret_stdin: false,
            redirect_url: None,
            scopes: vec![],
            allowed_domains: vec!["a.io".into(), "b.io".into()],
            auto_create: None,
            username_claim: None,
        };
        assert!(!args.is_empty());
        args.apply(&mut config, None);
        let sent = serde_json::to_value(&config).unwrap();
        assert_eq!(
            sent,
            json!({
                "enabled": true, "name": "Okta", "issuer": "https://idp", "client_id": "c2",
                "redirect_url": "https://wt/cb", "scopes": ["openid"],
                "allowed_domains": ["a.io", "b.io"], "auto_create": true,
                "username_claim": "preferred_username"
            })
        );
        config.client_secret = Some("s".into());
        assert_eq!(serde_json::to_value(&config).unwrap()["client_secret"], "s");
    }

    #[test]
    fn renders_config_without_the_secret() {
        let config = SsoConfig {
            enabled: true,
            name: "Okta".into(),
            has_client_secret: true,
            scopes: vec!["openid".into(), "email".into()],
            ..SsoConfig::default()
        };
        let mut t = Text::default();
        config_text(&mut t, &config);
        let out = t.as_str();
        assert!(out.contains("enabled:         yes\n"), "{out}");
        assert!(out.contains("client secret:   (set)\n"), "{out}");
        assert!(out.contains("scopes:          openid email\n"), "{out}");
    }
}
