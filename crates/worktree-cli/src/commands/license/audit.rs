//! `wt license audit [TREE] [--ref REF] [--strict]`: the server's license
//! audit of a snapshot (`GET {tree}/compliance/licenses`, ENGINEERING_PLAN
//! §16.7).

use colored::Colorize;
use serde::Deserialize;

use crate::commands::server_common::{
    decode, nullable, or_dash, tree_or_default, Api, Cell, Table, TreeRef,
};
use crate::context::Context;
use crate::error::{CliError, Result};
use crate::output::Text;

/// Paths listed in human output per section (JSON has them all).
const SHOWN: usize = 20;

/// `scan.LicenseUsage`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Usage {
    spdx: String,
    files: u64,
    proprietary: bool,
    #[serde(deserialize_with = "nullable")]
    globs: Vec<String>,
}

/// `scan.ExposedPath`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Exposed {
    path: String,
    spdx: String,
    grantee: String,
    grant: String,
    glob: String,
    expires_at: Option<String>,
}

/// `scan.RuleCheck`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RuleCheck {
    path_glob: String,
    spdx: String,
    grantee: String,
    expired: bool,
    files: u64,
    valid: bool,
    #[serde(deserialize_with = "nullable")]
    problems: Vec<String>,
}

/// `scan.LicenseAudit`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Audit {
    #[serde(rename = "ref")]
    reference: String,
    snapshot_id: String,
    files: u64,
    #[serde(deserialize_with = "nullable")]
    licenses: Vec<Usage>,
    #[serde(deserialize_with = "nullable")]
    unlicensed: Vec<String>,
    unlicensed_count: u64,
    #[serde(deserialize_with = "nullable")]
    exposed: Vec<Exposed>,
    exposed_count: u64,
    #[serde(deserialize_with = "nullable")]
    rules: Vec<RuleCheck>,
    invalid_rules: u64,
    spdx_list_version: String,
}

pub fn run(
    ctx: &Context,
    tree: Option<TreeRef>,
    reference: Option<String>,
    strict: bool,
) -> Result<()> {
    let tree = tree_or_default(ctx, tree)?;
    let api = Api::connect(ctx)?;
    let query: Vec<(&str, String)> = reference.into_iter().map(|r| ("ref", r)).collect();
    let value = api.get_query(&format!("{}/compliance/licenses", tree.api()), &query)?;
    let audit: Audit = decode(&value)?;
    ctx.out.emit(&value, |t| report(t, &audit, &tree))?;
    if strict && (audit.invalid_rules > 0 || audit.exposed_count > 0) {
        return Err(CliError::failure(format!(
            "license audit of {tree}: {} invalid rule(s), {} exposed proprietary path(s)",
            audit.invalid_rules, audit.exposed_count
        )));
    }
    Ok(())
}

fn report(t: &mut Text, a: &Audit, tree: &TreeRef) {
    t.line(format_args!(
        "License audit of {} at {} ({} files, snapshot {})",
        tree.to_string().bold(),
        or_dash(&a.reference),
        a.files,
        crate::output::format::short(&a.snapshot_id)
    ));
    t.blank();

    let mut licenses = Table::new(&["LICENSE", "FILES", "RULES"]);
    for u in &a.licenses {
        let spdx = if u.proprietary {
            Cell::from(format!("{} (proprietary)", u.spdx)).yellow()
        } else {
            Cell::from(u.spdx.as_str()).green()
        };
        licenses.row([
            spdx,
            Cell::from(u.files.to_string()),
            Cell::from(u.globs.join(", ")),
        ]);
    }
    licenses.render(t, "No file is covered by a license rule.");

    t.blank();
    if a.unlicensed_count == 0 {
        t.line("Every file is covered by a license rule.".green());
    } else {
        t.line(format_args!(
            "{} file(s) without a license rule:",
            a.unlicensed_count.to_string().yellow().bold()
        ));
        for p in a.unlicensed.iter().take(SHOWN) {
            t.line(format_args!("  {p}"));
        }
        if a.unlicensed_count as usize > SHOWN {
            t.line(
                format!(
                    "  ... and {} more (--json lists up to 1000)",
                    a.unlicensed_count as usize - SHOWN
                )
                .dimmed(),
            );
        }
    }

    t.blank();
    if a.exposed_count == 0 {
        t.line("No proprietary file is readable by other tenants.".green());
    } else {
        t.line(format_args!(
            "{} proprietary path grant(s) readable by other tenants:",
            a.exposed_count.to_string().red().bold()
        ));
        let mut table = Table::new(&["PATH", "LICENSE", "GRANTEE", "GRANT", "RULE", "EXPIRES"]);
        for e in a.exposed.iter().take(SHOWN) {
            let grantee = if e.grantee.is_empty() {
                "every other tenant".to_string()
            } else {
                e.grantee.clone()
            };
            table.row([
                Cell::from(e.path.as_str()).bold(),
                Cell::from(e.spdx.as_str()),
                Cell::from(grantee),
                Cell::from(e.grant.as_str()),
                Cell::from(e.glob.as_str()),
                Cell::from(or_dash(e.expires_at.as_deref().unwrap_or_default())),
            ]);
        }
        table.render(t, "");
    }

    t.blank();
    let mut rules = Table::new(&["RULE", "SPDX", "GRANTEE", "FILES", "CHECK"]);
    for r in &a.rules {
        let check = if r.valid {
            Cell::from(if r.expired { "ok (expired)" } else { "ok" }).green()
        } else {
            Cell::from(r.problems.join("; ")).red()
        };
        rules.row([
            Cell::from(r.path_glob.as_str()).bold(),
            Cell::from(r.spdx.as_str()),
            Cell::from(or_dash(&r.grantee)),
            Cell::from(r.files.to_string()),
            check,
        ]);
    }
    rules.render(t, "No license rules.");
    t.line(
        format!(
            "{} rule(s) fail the strict SPDX check (SPDX License List {}).",
            a.invalid_rules, a.spdx_list_version
        )
        .dimmed(),
    );
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::cli::Command;
    use crate::commands::license::LicenseCommand;
    use crate::commands::server_common::testing::parse;

    #[test]
    fn parses_audit() {
        let Command::License(a) =
            parse(&["license", "audit", "acme/web", "--ref", "v1", "--strict"])
        else {
            panic!()
        };
        let LicenseCommand::Audit {
            tree,
            reference,
            strict,
        } = a.command
        else {
            panic!()
        };
        assert_eq!(tree.map(|t| t.to_string()).as_deref(), Some("acme/web"));
        assert_eq!(reference.as_deref(), Some("v1"));
        assert!(strict);
        let Command::License(a) = parse(&["license", "audit"]) else {
            panic!()
        };
        assert!(matches!(
            a.command,
            LicenseCommand::Audit {
                tree: None,
                reference: None,
                strict: false
            }
        ));
    }

    #[test]
    fn renders_a_report() {
        let audit: Audit = decode(&json!({
            "tree_id": "t", "ref": "main", "snapshot_id": "0123456789abcdef", "files": 3,
            "licenses": [{ "spdx": "MIT", "files": 2, "proprietary": false, "globs": ["src/**"] },
                         { "spdx": "LicenseRef-Proprietary", "files": 1, "proprietary": true, "globs": null }],
            "unlicensed": ["README.md"], "unlicensed_count": 1,
            "exposed": [{ "path": "vendor/x.c", "spdx": "LicenseRef-Proprietary", "grantee": "",
                          "grant": "read-only", "glob": "vendor/**" }],
            "exposed_count": 1,
            "rules": [{ "path_glob": "tools/**", "spdx": "GPL-2.0", "grant": "", "grantee": "",
                        "expired": false, "files": 1, "valid": false, "problems": ["\"GPL-2.0\" is deprecated"] }],
            "invalid_rules": 1, "spdx_list_version": "3.25", "truncated": false
        }))
        .expect("decode");
        let mut t = Text::default();
        report(
            &mut t,
            &audit,
            &TreeRef {
                tenant: "acme".into(),
                name: "web".into(),
            },
        );
        let s = t.as_str();
        for want in [
            "README.md",
            "vendor/x.c",
            "every other tenant",
            "deprecated",
            "3.25",
        ] {
            assert!(s.contains(want), "missing {want}:\n{s}");
        }
    }
}
