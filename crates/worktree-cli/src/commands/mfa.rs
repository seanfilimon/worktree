//! `wt mfa` — Two-factor authentication (TOTP) and recovery codes
//! (§12.2, §12.3 MFA).

use std::io::Write;

use clap::{Args, Subcommand};
use qrcode::render::unicode::Dense1x2;
use qrcode::QrCode;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::commands::auth::{prompt_line, read_password};
use crate::commands::iam_common::{call, fields, show, when};
use crate::context::Context;
use crate::error::{CliError, Result};
use crate::output::Text;

const MFA_PATH: &str = "/api/v1/auth/mfa";

#[derive(Debug, Args)]
pub struct MfaArgs {
    #[command(subcommand)]
    pub command: MfaCommand,
}

#[derive(Debug, Subcommand)]
pub enum MfaCommand {
    /// Show whether two-factor authentication is enabled
    Status,
    /// Enroll an authenticator app (shows a QR code, then asks for a code)
    Enable,
    /// Turn two-factor authentication off (asks for your password)
    Disable {
        /// Read the password from the first line of stdin
        #[arg(long)]
        password_stdin: bool,
    },
    /// Replace your recovery codes (asks for a current code)
    RecoveryCodes,
}

#[derive(Debug, Deserialize)]
struct MfaState {
    totp_enabled: bool,
    #[serde(default)]
    enabled_at: Option<String>,
    #[serde(default)]
    recovery_codes_left: u32,
}

#[derive(Debug, Serialize, Deserialize)]
struct Enrollment {
    secret: String,
    otpauth_url: String,
}

#[derive(Debug, Deserialize)]
struct RecoveryCodes {
    recovery_codes: Vec<String>,
}

pub fn run(ctx: &Context, args: MfaArgs) -> Result<()> {
    let client = ctx.rest()?;
    match args.command {
        MfaCommand::Status => {
            let state: Value = call(client.get(MFA_PATH))?;
            show(&ctx.out, &state, status)
        }
        MfaCommand::Enable => {
            let enrollment: Value =
                call(client.post(&format!("{MFA_PATH}/totp/enroll"), &json!({})))?;
            let parsed: Enrollment = serde_json::from_value(enrollment).map_err(|e| {
                CliError::failure(format!("unexpected response from the server: {e}"))
            })?;
            let mut text = Text::default();
            enrollment_text(&mut text, &parsed);
            // The enrollment must be visible before the prompt; with --json it
            // goes to stderr so stdout stays one JSON document.
            if ctx.out.is_json() {
                let mut stderr = std::io::stderr();
                stderr.write_all(text.as_str().as_bytes())?;
            } else {
                ctx.out
                    .emit(&parsed, |t| t.line(text.as_str().trim_end()))?;
            }
            let code = totp_code(&prompt_line(
                "Enter the 6-digit code from your authenticator app: ",
            )?)?;
            let path = format!("{MFA_PATH}/totp/confirm");
            let codes: Value = call(client.post(&path, &json!({ "code": code })))?;
            let parsed_codes: RecoveryCodes =
                serde_json::from_value(codes.clone()).map_err(|e| {
                    CliError::failure(format!("unexpected response from the server: {e}"))
                })?;
            let result = json!({
                "secret": parsed.secret,
                "otpauth_url": parsed.otpauth_url,
                "recovery_codes": codes["recovery_codes"],
            });
            ctx.out.emit(&result, |t| {
                t.blank();
                t.line("Two-factor authentication is enabled.");
                recovery_codes(t, &parsed_codes);
            })
        }
        MfaCommand::Disable { password_stdin } => {
            let password = read_password(password_stdin, "Password: ", false)?;
            let body = json!({ "password": password });
            call(client.send(
                Method::DELETE,
                &format!("{MFA_PATH}/totp"),
                &[],
                Some(&body),
            ))?;
            ctx.out.emit(&json!({"totp_enabled": false}), |t| {
                t.line("Two-factor authentication is disabled.");
            })
        }
        MfaCommand::RecoveryCodes => {
            let code = prompt_line("Enter a current code from your authenticator app: ")?;
            let path = format!("{MFA_PATH}/recovery-codes");
            let codes: Value = call(client.post(&path, &json!({ "code": code.trim() })))?;
            show(&ctx.out, &codes, |t, c: &RecoveryCodes| {
                t.line("New recovery codes (the old ones no longer work):");
                recovery_codes(t, c);
            })
        }
    }
}

/// Normalize a typed TOTP code ("123 456" → "123456"); it must be 6 digits.
fn totp_code(input: &str) -> Result<String> {
    let code: String = input.chars().filter(|c| !c.is_whitespace()).collect();
    if code.len() == 6 && code.bytes().all(|b| b.is_ascii_digit()) {
        Ok(code)
    } else {
        Err(CliError::failure("the code must be 6 digits"))
    }
}

/// The otpauth URL as a terminal QR code (light modules drawn as blocks,
/// for dark-background terminals).
fn qr_code(data: &str) -> Option<String> {
    let code = QrCode::new(data.as_bytes()).ok()?;
    Some(
        code.render::<Dense1x2>()
            .dark_color(Dense1x2::Light)
            .light_color(Dense1x2::Dark)
            .quiet_zone(true)
            .build(),
    )
}

fn enrollment_text(t: &mut Text, e: &Enrollment) {
    t.line("Scan this QR code with your authenticator app:");
    t.blank();
    if let Some(qr) = qr_code(&e.otpauth_url) {
        t.line(qr);
        t.blank();
    }
    t.line("Or enter the secret manually:");
    fields(
        t,
        &[
            ("  secret", e.secret.clone()),
            ("  url", e.otpauth_url.clone()),
        ],
    );
    t.blank();
}

fn recovery_codes(t: &mut Text, codes: &RecoveryCodes) {
    t.blank();
    for code in &codes.recovery_codes {
        t.line(format_args!("    {code}"));
    }
    t.blank();
    t.line("Store these recovery codes somewhere safe. Each works once, in place of a");
    t.line("TOTP code, if you lose your authenticator. They are not shown again.");
}

fn status(t: &mut Text, s: &MfaState) {
    if !s.totp_enabled {
        t.line("Two-factor authentication is off. Turn it on with `wt mfa enable`.");
        return;
    }
    fields(
        t,
        &[
            ("two-factor", "enabled (TOTP)".into()),
            (
                "since",
                s.enabled_at.as_deref().map(when).unwrap_or_default(),
            ),
            ("recovery codes left", s.recovery_codes_left.to_string()),
        ],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Command;
    use crate::commands::iam_common::parse;

    #[test]
    fn parses_subcommands() {
        let mfa = |args: &[&str]| {
            let mut argv = vec!["mfa"];
            argv.extend_from_slice(args);
            match parse(&argv) {
                Command::Mfa(a) => a.command,
                other => panic!("{other:?}"),
            }
        };
        assert!(matches!(mfa(&["status"]), MfaCommand::Status));
        assert!(matches!(mfa(&["enable"]), MfaCommand::Enable));
        assert!(matches!(
            mfa(&["disable", "--password-stdin"]),
            MfaCommand::Disable {
                password_stdin: true
            }
        ));
        assert!(matches!(
            mfa(&["recovery-codes"]),
            MfaCommand::RecoveryCodes
        ));
    }

    #[test]
    fn totp_codes_are_normalized() {
        assert_eq!(totp_code(" 123 456\n").unwrap(), "123456");
        for bad in ["12345", "1234567", "abcdef", ""] {
            assert!(totp_code(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn enrollment_shows_qr_secret_and_url() {
        let e = Enrollment {
            secret: "JBSWY3DPEHPK3PXP".into(),
            otpauth_url: "otpauth://totp/W0rkTree:ada?secret=JBSWY3DPEHPK3PXP&issuer=W0rkTree"
                .into(),
        };
        let mut t = Text::default();
        enrollment_text(&mut t, &e);
        let out = t.as_str();
        assert!(out.contains("secret: JBSWY3DPEHPK3PXP"), "{out}");
        assert!(out.contains("url:    otpauth://totp/W0rkTree:ada"), "{out}");
        assert!(
            out.contains('█') && out.contains('▀'),
            "QR code missing:\n{out}"
        );
        let qr = qr_code(&e.otpauth_url).unwrap();
        let rows: Vec<&str> = qr.lines().collect();
        let width = rows[0].chars().count();
        assert!(rows.iter().all(|r| r.chars().count() == width), "ragged QR");
        // Two modules per character row: roughly square.
        assert!(
            rows.len() * 2 >= width && rows.len() * 2 <= width + 2,
            "{width}x{}",
            rows.len()
        );
    }

    #[test]
    fn renders_status_and_codes() {
        let mut t = Text::default();
        status(
            &mut t,
            &MfaState {
                totp_enabled: false,
                enabled_at: None,
                recovery_codes_left: 0,
            },
        );
        assert!(t.as_str().contains("wt mfa enable"));
        let mut t = Text::default();
        status(
            &mut t,
            &MfaState {
                totp_enabled: true,
                enabled_at: None,
                recovery_codes_left: 7,
            },
        );
        assert!(
            t.as_str().contains("recovery codes left: 7"),
            "{}",
            t.as_str()
        );
        let mut t = Text::default();
        recovery_codes(
            &mut t,
            &RecoveryCodes {
                recovery_codes: vec!["abcd-efgh".into(), "ijkl-mnop".into()],
            },
        );
        assert!(t.as_str().contains("    abcd-efgh\n    ijkl-mnop\n"));
    }
}
