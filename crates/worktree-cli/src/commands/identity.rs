//! `wt identity` — Snapshot signing keys: generate, show, register, list,
//! revoke, verify (ENGINEERING_PLAN §13.6).
//!
//! Private keys live in `<wt home>/keys/` (see `worktree_sdk::signing`);
//! the server keeps the public halves under `/api/v1/auth/keys`.

use clap::{Args, Subcommand};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use worktree_sdk::remote::rest::seg;
use worktree_sdk::signing::{
    self, KeyInfo, KeyStore, TrustedKey, Verification, ALGORITHM, DEFAULT_KEY,
};

use crate::commands::iam_common::{call, confirm, fields, Table, YesArgs};
use crate::context::Context;
use crate::error::{CliError, Result};
use crate::output::Text;

const KEYS_PATH: &str = "/api/v1/auth/keys";

#[derive(Debug, Args)]
pub struct IdentityArgs {
    #[command(subcommand)]
    pub command: IdentityCommand,
}

#[derive(Debug, Subcommand)]
pub enum IdentityCommand {
    /// Generate a new Ed25519 signing key in <wt home>/keys
    Generate {
        /// Key name (default: "default")
        name: Option<String>,
        /// Replace an existing key of the same name
        #[arg(long)]
        force: bool,
    },
    /// List local signing keys
    List,
    /// Show a key's fingerprint and public key
    Show {
        /// Key name (default: [signing] key, else "default")
        name: Option<String>,
    },
    /// Upload a key's public half to the server and record its key id
    Register {
        /// Key name (default: [signing] key, else "default")
        name: Option<String>,
    },
    /// Revoke a registered key on the server
    Revoke {
        /// Server key id (or the name of a registered local key)
        id: String,
        #[command(flatten)]
        yes: YesArgs,
    },
    /// Verify a snapshot's signature against local (and optionally an account's registered) keys
    Verify {
        /// Snapshot to verify (default: HEAD)
        snapshot: Option<String>,
        /// Also trust the registered keys of this server account
        #[arg(long, value_name = "USERNAME")]
        account: Option<String>,
    },
}

pub fn run(ctx: &Context, args: IdentityArgs) -> Result<()> {
    let keys = KeyStore::default_location();
    match args.command {
        IdentityCommand::Generate { name, force } => {
            let name = name.unwrap_or_else(|| DEFAULT_KEY.to_string());
            let info = keys.generate(&name, force)?;
            ctx.out.emit(&info, |t| {
                t.line(format_args!("Generated signing key {}", info.name));
                key_fields(t, &info);
                t.line(format_args!(
                    "Sign snapshots with `wt snapshot --sign`, or set [signing] key = \"{}\" and auto = true in .wt/config.toml; run `wt identity register {}` to upload it.",
                    info.name, info.name
                ));
            })
        }
        IdentityCommand::List => {
            let list = keys.list()?;
            ctx.out.emit(&list, |t| {
                let mut table = Table::new(&["name", "fingerprint", "key id", "server"]);
                for k in &list {
                    table.row([
                        k.name.clone(),
                        k.fingerprint.clone(),
                        k.key_id.clone(),
                        k.server.clone(),
                    ]);
                }
                table.render(t, "No signing keys (run `wt identity generate`).");
            })
        }
        IdentityCommand::Show { name } => {
            let info = keys.info(&key_name(ctx, name))?;
            ctx.out.emit(&info, |t| key_fields(t, &info))
        }
        IdentityCommand::Register { name } => {
            let name = key_name(ctx, name);
            let info = keys.info(&name)?;
            let client = ctx.rest()?;
            let body = NewKey {
                name: &info.name,
                algorithm: ALGORITHM,
                public_key: info.public_key_base64()?,
            };
            let created: Value = call(client.post(KEYS_PATH, &body)).map_err(|e| {
                if e.to_string().contains("already registered") {
                    e.with_hint("a public key can be registered once; generate a new key with `wt identity generate NAME`")
                } else {
                    e
                }
            })?;
            let server_key: ServerKey = decode(&created)?;
            let info = keys.set_registration(&name, &server_key.id, client.server())?;
            ctx.out.emit(&created, |t| {
                t.line(format_args!(
                    "Registered signing key {} as {} on {}",
                    info.name, server_key.id, info.server
                ));
            })
        }
        IdentityCommand::Revoke { id, yes } => {
            // A registered local key may be named instead of its id.
            let id = match keys.info(&id) {
                Ok(info) if !info.key_id.is_empty() => info.key_id,
                _ => id,
            };
            confirm(yes.yes, &format!("revoke signing key {id}"))?;
            let client = ctx.rest()?;
            call(client.delete(&format!("{KEYS_PATH}/{}", seg(&id))))?;
            let local = keys.clear_registration(&id)?;
            ctx.out.emit(
                &json!({"id": id, "revoked": true, "local_key": local.as_ref().map(|k| &k.name)}),
                |t| {
                    t.line(format_args!("Revoked signing key {id}"));
                    if let Some(k) = &local {
                        t.line(format_args!(
                            "Local key {} is no longer registered (its private key is kept).",
                            k.name
                        ));
                    }
                },
            )
        }
        IdentityCommand::Verify { snapshot, account } => {
            let (repo, tree) = ctx.open_tree()?;
            let snap = repo.resolve(&tree.name, snapshot.as_deref().unwrap_or("HEAD"))?;
            let mut trusted = keys.trusted()?;
            if let Some(account) = account {
                trusted.extend(account_keys(ctx, &account)?);
            }
            let result = signing::verify_with_keys(&snap, &trusted);
            let report = VerifyReport {
                snapshot: snap.id.clone(),
                verification: result.clone(),
            };
            ctx.out.emit(&report, |t| {
                t.line(format_args!(
                    "snapshot {}: {}",
                    snap.short_id(),
                    describe(&result)
                ));
            })?;
            if result.is_valid() {
                Ok(())
            } else {
                Err(CliError::failure(format!(
                    "snapshot {} is not verified",
                    snap.short_id()
                )))
            }
        }
    }
}

/// Explicit name, else `[signing] key` of the current worktree, else
/// `default`.
fn key_name(ctx: &Context, explicit: Option<String>) -> String {
    explicit
        .or_else(|| {
            ctx.open()
                .ok()
                .map(|repo| repo.config().signing.key.trim().to_string())
                .filter(|k| !k.is_empty())
        })
        .unwrap_or_else(|| DEFAULT_KEY.to_string())
}

fn key_fields(t: &mut Text, info: &KeyInfo) {
    fields(
        t,
        &[
            ("name", info.name.clone()),
            ("algorithm", info.algorithm.clone()),
            ("fingerprint", info.fingerprint.clone()),
            ("public key", info.public_key_base64().unwrap_or_default()),
            ("key id", info.key_id.clone()),
            ("server", info.server.clone()),
            ("created", info.created_at.to_rfc3339()),
        ],
    );
}

fn describe(v: &Verification) -> String {
    match v {
        Verification::Unsigned => "unsigned".into(),
        Verification::Valid {
            key, fingerprint, ..
        } => format!("good signature by {key} ({fingerprint})"),
        Verification::Invalid { key, key_id } => {
            format!("BAD signature (key {key}, id {key_id})")
        }
        Verification::UnknownKey { key_id } => format!("signed by unknown key {key_id}"),
    }
}

/// Unrevoked registered keys of `username` as trusted keys.
fn account_keys(ctx: &Context, username: &str) -> Result<Vec<TrustedKey>> {
    let client = ctx.rest()?;
    let value: Value = call(client.get(&format!("/api/v1/accounts/{}/keys", seg(username))))?;
    let list: Vec<ServerKey> = decode(&value)?;
    list.into_iter()
        .filter(|k| k.revoked_at.is_none())
        .map(|k| {
            Ok(TrustedKey {
                label: format!("{username}/{}", k.name),
                public_key: signing::decode_public_key_base64(&k.public_key)?,
                ids: vec![k.id, k.fingerprint],
            })
        })
        .collect()
}

fn decode<T: for<'de> Deserialize<'de>>(value: &Value) -> Result<T> {
    serde_json::from_value(value.clone())
        .map_err(|e| CliError::failure(format!("unexpected response from the server: {e}")))
}

/// Body of `POST /api/v1/auth/keys`.
#[derive(Debug, Serialize)]
struct NewKey<'a> {
    name: &'a str,
    algorithm: &'a str,
    /// Standard base64.
    public_key: String,
}

/// A server `SigningKey`.
#[derive(Debug, Deserialize)]
struct ServerKey {
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    public_key: String,
    #[serde(default)]
    fingerprint: String,
    #[serde(default)]
    revoked_at: Option<String>,
}

#[derive(Debug, Serialize)]
struct VerifyReport {
    snapshot: String,
    #[serde(flatten)]
    verification: Verification,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Command;
    use crate::commands::iam_common::parse;

    fn identity(args: &[&str]) -> IdentityCommand {
        let mut argv = vec!["identity"];
        argv.extend_from_slice(args);
        match parse(&argv) {
            Command::Identity(a) => a.command,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn parses_subcommands() {
        assert!(matches!(
            identity(&["generate"]),
            IdentityCommand::Generate {
                name: None,
                force: false
            }
        ));
        assert!(matches!(
            identity(&["generate", "work", "--force"]),
            IdentityCommand::Generate { name: Some(n), force: true } if n == "work"
        ));
        assert!(matches!(identity(&["list"]), IdentityCommand::List));
        assert!(matches!(
            identity(&["show", "work"]),
            IdentityCommand::Show { name: Some(n) } if n == "work"
        ));
        assert!(matches!(
            identity(&["register"]),
            IdentityCommand::Register { name: None }
        ));
        assert!(matches!(
            identity(&["revoke", "k1", "-y"]),
            IdentityCommand::Revoke { id, yes: YesArgs { yes: true } } if id == "k1"
        ));
        assert!(matches!(
            identity(&["verify", "HEAD~1", "--account", "ada"]),
            IdentityCommand::Verify { snapshot: Some(s), account: Some(a) } if s == "HEAD~1" && a == "ada"
        ));
    }

    #[test]
    fn verify_report_is_flat() {
        let report = VerifyReport {
            snapshot: "s1".into(),
            verification: Verification::UnknownKey { key_id: "k".into() },
        };
        assert_eq!(
            serde_json::to_value(&report).unwrap(),
            json!({"snapshot": "s1", "status": "unknown_key", "key_id": "k"})
        );
    }
}
