//! Server-side secret scanning and license compliance (ENGINEERING_PLAN
//! §16.7): pushes are scanned with the client rule set; the default warn
//! mode records findings and lets the push through; block mode rejects a
//! developer's push with located, redacted findings while the owner's
//! branch:bypass_protection lets hers through; an allowlisted fingerprint
//! passes; the local and server scanners report the same fingerprint;
//! `wt license audit` reports unlicensed, exposed and non-SPDX paths; the
//! admin summary sees it all.

use serde_json::Value;
use worktree_e2e::json::{find_objects, pretty, str_field};
use worktree_e2e::{require_e2e, TestEnv};

use crate::common::{add_member, publish_tree, server_tip, TREE};

/// Secrets are assembled at runtime so that this file holds no literal one.
fn aws_key() -> String {
    format!("{}{}", "AKIA", "Z7Q4W2E8R5T1Y3U6")
}

fn gh_token() -> String {
    format!("{}{}", "gh", "p_Xk3Lm9Qw2Er5Ty8Ui1Op4As7Df0Gh6Jk2Zx9")
}

/// Findings of `wt compliance findings --status all --json`.
fn findings(v: &Value) -> Vec<Value> {
    find_objects(v, &["fingerprint", "path", "line"])
        .into_iter()
        .cloned()
        .collect()
}

#[test]
fn server_secret_scan() {
    require_e2e!();
    let env = TestEnv::with_server("server_secret_scan");
    let alice = env.user("alice");
    let a = publish_tree(&env, &alice, &[("README.md", "# scanned\n")]);
    let bob = env.user("bob");
    bob.register();
    add_member(&alice.api(), "alice", "bob", "developer");
    let b = bob.clone_tree("alice", TREE, TREE);
    let aws = aws_key();
    let gh = gh_token();

    // --- warn (default): the push goes through, the finding is recorded
    let settings = a.wt().args(["compliance", "settings"]).json();
    assert_eq!(
        str_field(&settings, "mode"),
        "warn",
        "{}",
        pretty(&settings)
    );
    b.write(
        "config/aws.env",
        format!("# creds\nAWS_ACCESS_KEY_ID={aws}\n"),
    );
    b.wt()
        .args(["snapshot", "-m", "add creds", "--no-scan"])
        .ok();
    b.push();
    let tip = b.head_id();
    assert_eq!(server_tip(&alice.api(), "alice", TREE, "main"), Some(tip));
    let list = b.wt().args(["compliance", "findings"]).json();
    let found = findings(&list);
    assert_eq!(found.len(), 1, "{}", pretty(&list));
    let aws_fp = str_field(&found[0], "fingerprint");
    assert_eq!(str_field(&found[0], "path"), "config/aws.env");
    assert_eq!(found[0]["line"].as_u64(), Some(2));
    assert_eq!(str_field(&found[0], "rule"), "aws-access-key");
    assert_eq!(str_field(&found[0], "action"), "warned");
    assert!(
        !list.to_string().contains(&aws),
        "findings must be redacted:\n{}",
        pretty(&list)
    );
    let text = b.wt().args(["compliance", "findings"]).ok();
    assert!(
        text.mentions("config/aws.env:2") && text.mentions(&aws_fp),
        "{text}"
    );

    // --- the local scanner reports the very same fingerprint
    let local = b.wt().args(["--json", "compliance", "scan"]).fails();
    let local = local.json();
    let local_found = find_objects(&local, &["fingerprint", "rule"]);
    assert!(
        local_found
            .iter()
            .any(|f| f["fingerprint"] == aws_fp.as_str()),
        "local scan:\n{}",
        pretty(&local)
    );
    // ... and so does an on-demand server scan of the branch (tree:admin).
    b.wt().args(["compliance", "scan", "--server"]).fails();
    let remote = a
        .wt()
        .args(["--json", "compliance", "scan", "--server"])
        .fails();
    let remote = remote.json();
    assert!(
        find_objects(&remote, &["fingerprint", "allowed"])
            .iter()
            .any(|m| m["fingerprint"] == aws_fp.as_str() && m["allowed"] == Value::Bool(false)),
        "server scan:\n{}",
        pretty(&remote)
    );

    // --- block: a developer's push is rejected with the located finding
    b.wt()
        .args(["compliance", "settings", "--mode", "block"])
        .fails(); // tree:admin only
    a.wt()
        .args(["compliance", "settings", "--mode", "block"])
        .ok();
    let before = server_tip(&alice.api(), "alice", TREE, "main");
    b.write(
        "src/app.rs",
        format!("fn main() {{}}\nconst T: &str = \"{gh}\";\n"),
    );
    b.wt()
        .args(["snapshot", "-m", "add token", "--no-scan"])
        .ok();
    let out = b.wt().arg("push").fails();
    assert!(
        out.mentions("secret scanning blocked")
            && out.mentions("src/app.rs:2")
            && out.mentions("github-token"),
        "{out}"
    );
    assert!(
        out.mentions("ghp_****2Zx9"),
        "the excerpt is redacted\n{out}"
    );
    assert!(
        !out.combined().contains(&gh),
        "the secret must not be echoed\n{out}"
    );
    assert_eq!(
        server_tip(&alice.api(), "alice", TREE, "main"),
        before,
        "main unchanged"
    );
    let list = b
        .wt()
        .args(["compliance", "findings", "--status", "all"])
        .json();
    let blocked: Vec<Value> = findings(&list)
        .into_iter()
        .filter(|f| f["path"] == "src/app.rs")
        .collect();
    assert_eq!(blocked.len(), 1, "{}", pretty(&list));
    assert_eq!(str_field(&blocked[0], "action"), "blocked");
    let gh_fp = str_field(&blocked[0], "fingerprint");

    // --- an allowlisted fingerprint lets the push through
    b.wt()
        .args(["compliance", "allow", &gh_fp, "--reason", "test fixture"])
        .fails(); // tree:admin only
    a.wt()
        .args(["compliance", "allow", &gh_fp, "--reason", "test fixture"])
        .ok();
    let al = b.wt().args(["compliance", "allow", "--list"]).json();
    assert!(al.to_string().contains(&gh_fp), "{}", pretty(&al));
    b.push();
    assert_eq!(
        server_tip(&alice.api(), "alice", TREE, "main"),
        Some(b.head_id())
    );
    let resolved = b
        .wt()
        .args(["compliance", "findings", "--status", "resolved"])
        .json();
    assert!(
        findings(&resolved)
            .iter()
            .any(|f| f["fingerprint"] == gh_fp.as_str() && f["resolution"] == "false_positive"),
        "{}",
        pretty(&resolved)
    );

    // --- the owner holds branch:bypass_protection: her push goes through
    a.pull();
    a.write("deploy/token.txt", format!("token: {gh}x\nkey {aws}\n"));
    a.wt()
        .args(["snapshot", "-m", "owner push", "--no-scan"])
        .ok();
    a.push();
    let list = a.wt().args(["compliance", "findings"]).json();
    assert!(
        findings(&list)
            .iter()
            .any(|f| f["path"] == "deploy/token.txt" && f["action"] == "bypassed"),
        "{}",
        pretty(&list)
    );

    // --- resolving a finding
    let open = a.wt().args(["compliance", "findings"]).json();
    let first = findings(&open)
        .into_iter()
        .find(|f| f["path"] == "config/aws.env")
        .expect("aws finding open");
    let id = str_field(&first, "id");
    let r = a
        .wt()
        .args([
            "compliance",
            "resolve",
            &id,
            "--as",
            "revoked",
            "--note",
            "rotated",
        ])
        .json();
    assert_eq!(str_field(&r, "status"), "resolved", "{}", pretty(&r));

    // --- license audit
    a.commit(
        &[
            ("vendor/blob.c", "/* vendor */\n"),
            ("tools/run.sh", "echo hi\n"),
        ],
        "vendor and tools",
    );
    a.push();
    a.wt()
        .args([
            "license",
            "set",
            "vendor/**",
            "LicenseRef-Proprietary",
            "--grant",
            "read-only",
        ])
        .ok();
    a.wt().args(["license", "set", "tools/**", "GPL-2.0"]).ok();
    let audit = a.wt().args(["license", "audit"]).json();
    assert!(
        find_objects(&audit, &["path", "grant", "glob"])
            .iter()
            .any(|e| e["path"] == "vendor/blob.c" && e["grant"] == "read-only"),
        "exposed:\n{}",
        pretty(&audit)
    );
    assert!(
        audit["unlicensed"].to_string().contains("README.md"),
        "{}",
        pretty(&audit)
    );
    assert_eq!(
        audit["invalid_rules"].as_u64(),
        Some(1),
        "{}",
        pretty(&audit)
    );
    let text = a.wt().args(["license", "audit"]).ok();
    assert!(
        text.mentions("deprecated") && text.mentions("vendor/blob.c"),
        "{text}"
    );
    a.wt().args(["license", "audit", "--strict"]).fails();

    // --- the admin summary
    let admin = env.admin_user();
    admin.login();
    let sum = admin.api().get("/api/v1/admin/compliance").ok_json();
    assert_eq!(sum["modes"]["block"].as_u64(), Some(1), "{}", pretty(&sum));
    assert!(
        sum["open_findings"].as_u64().unwrap_or(0) >= 1,
        "{}",
        pretty(&sum)
    );
    assert_eq!(sum["invalid_rules"].as_u64(), Some(1), "{}", pretty(&sum));

    // --- off: nothing is scanned any more
    a.wt()
        .args(["compliance", "settings", "--mode", "off"])
        .ok();
    a.write("more.env", format!("K={aws}\n"));
    a.wt()
        .args(["snapshot", "-m", "unscanned", "--no-scan"])
        .ok();
    a.push();
    let list = a
        .wt()
        .args(["compliance", "findings", "--status", "all"])
        .json();
    assert!(
        !findings(&list).iter().any(|f| f["path"] == "more.env"),
        "{}",
        pretty(&list)
    );
}
