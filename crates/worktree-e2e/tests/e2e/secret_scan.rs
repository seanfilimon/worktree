//! Pre-snapshot secret scanning: refusal, `--no-scan`, inline allow marker.

use worktree_e2e::json::messages;
use worktree_e2e::{require_e2e, TestEnv};

/// A syntactically valid AWS access key id (not the well-known EXAMPLE key,
/// which scanners may allow-list). Built at runtime so this source file
/// itself does not trip repository secret scanners.
fn fake_aws_key() -> String {
    format!("{}{}", "AKIA", "Z7Q4W2E8R5T1Y3U6")
}

#[test]
fn secret_scan() {
    require_e2e!();
    let env = TestEnv::new("secret_scan");
    let alice = env.user("alice");
    let repo = alice.init("proj");
    repo.commit(&[("README.md", "hello\n")], "initial");
    let key = fake_aws_key();

    // A new file containing an AWS key is refused.
    repo.write(
        "config/aws.env",
        format!("# credentials\nAWS_ACCESS_KEY_ID={key}\n"),
    );
    let out = repo.wt().args(["snapshot", "-m", "add creds"]).fails();
    assert!(
        out.mentions("config/aws.env"),
        "the offending path must be reported\n{out}"
    );
    assert!(
        !out.combined().contains(&key),
        "the secret must be redacted in the report\n{out}"
    );
    assert_eq!(
        messages(&repo.log(&[])),
        vec!["initial"],
        "no snapshot may be created"
    );
    assert!(
        repo.changes().iter().any(|(p, _)| p == "config/aws.env"),
        "the file is still pending after the refusal"
    );

    // A modified tracked file is scanned too.
    repo.write("README.md", format!("hello\nleaked: {key}\n"));
    let out = repo.wt().args(["snapshot", "-m", "leak in readme"]).fails();
    assert!(out.mentions("README.md"), "{out}");
    repo.wt().args(["restore", "README.md"]).ok();

    // --no-scan bypasses the scan.
    repo.wt()
        .args(["snapshot", "-m", "add creds anyway", "--no-scan"])
        .ok();
    assert_eq!(
        messages(&repo.log(&[])),
        vec!["add creds anyway", "initial"]
    );
    repo.assert_clean();

    // An inline `wt:allow-secret` marker on the same line is honored.
    repo.write(
        "config/allowed.env",
        format!("AWS_ACCESS_KEY_ID={key} # wt:allow-secret\n"),
    );
    repo.wt().args(["snapshot", "-m", "allowed secret"]).ok();
    assert_eq!(messages(&repo.log(&["-n", "1"])), vec!["allowed secret"]);
}
