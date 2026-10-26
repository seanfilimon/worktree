//! Tenant lifecycle (ENGINEERING_PLAN §16.4): contact email and
//! verification (SMTP off: the code is in the server log), account email
//! verification, slug renames (old slug answers tenant_moved over REST and
//! gRPC, `wt tenant switch`), deletion with a recovery window that
//! suspends the tenant and revokes service-account keys, restore, and the
//! purge job.

use std::time::Duration;

use serde_json::{json, Value};
use worktree_e2e::json::pretty;
use worktree_e2e::{poll_until, require_e2e, TestEnv};

use crate::iam_common::wt_json;

/// Every verification code logged for `name` (subject tenant or account),
/// oldest first.
fn logged_codes(env: &TestEnv, name: &str) -> Vec<String> {
    let s = env.server();
    let mut text = std::fs::read_to_string(&s.stderr_path).unwrap_or_default();
    text.push_str(&std::fs::read_to_string(&s.stdout_path).unwrap_or_default());
    let mut out = Vec::new();
    for line in text.lines() {
        if !line.contains("email verification") || !line.contains(&format!("name={name} ")) {
            continue;
        }
        if let Some(i) = line.find("token=wte_") {
            let code: String = line[i + "token=".len()..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
                .collect();
            out.push(code);
        }
    }
    out
}

#[track_caller]
fn wait_code(env: &TestEnv, name: &str, nth: usize) -> String {
    poll_until(
        &format!("verification code #{nth} for {name} in the server log"),
        Duration::from_secs(10),
        Duration::from_millis(100),
        || {
            let codes = logged_codes(env, name);
            codes
                .get(nth)
                .cloned()
                .ok_or_else(|| format!("{} code(s) so far", codes.len()))
        },
    )
}

#[test]
fn tenant_lifecycle() {
    require_e2e!();
    let env = TestEnv::with_server_args(
        "tenant_lifecycle",
        &[
            "--tenant-deletion-window",
            "4s",
            "--tenant-purge-interval",
            "1s",
        ],
    );
    let url = env.url();
    let olga = env.user("olga");
    olga.register();
    let dave = env.user("dave");
    dave.register();
    olga.wt()
        .args(["tenant", "create", "acme", "--name", "Acme"])
        .ok();
    olga.wt()
        .args([
            "tenant",
            "add-member",
            "acme",
            "dave",
            "--role",
            "developer",
        ])
        .ok();
    let repo = olga.init("proj");
    repo.commit(&[("README.md", "# acme\n")], "initial");
    repo.add_remote("origin", &url, "acme");
    repo.push();

    // --- contact email + verification (code logged: SMTP is off)
    dave.wt()
        .args(["tenant", "email", "set", "acme", "ops@acme.test"])
        .fails();
    let t = wt_json(&olga, &["tenant", "email", "set", "acme", "ops@acme.test"]);
    assert_eq!(t["email"], "ops@acme.test", "{}", pretty(&t));
    assert!(t["email_verified_at"].is_null(), "{}", pretty(&t));
    let code = wait_code(&env, "acme", 0);
    olga.wt()
        .args(["tenant", "email", "verify", "acme", "wte_bogus"])
        .fails();
    let t = wt_json(&olga, &["tenant", "email", "verify", "acme", &code]);
    assert!(t["email_verified_at"].is_string(), "{}", pretty(&t));
    // Single use.
    olga.wt()
        .args(["tenant", "email", "verify", "acme", &code])
        .fails();
    // A verified email names the tenant.
    let by_email = dave.api().get("/api/v1/tenants/ops@acme.test").ok_json();
    assert_eq!(by_email["slug"], "acme", "{}", pretty(&by_email));

    // --- account email verification
    let dave_api = dave.api();
    let sent = dave_api.post_empty("/api/v1/me/email/resend").json();
    assert_eq!(sent["delivery"], "log", "{}", pretty(&sent));
    let acct_code = wait_code(&env, "dave", 0);
    let acct = dave_api
        .post("/api/v1/me/email/verify", json!({ "token": acct_code }))
        .ok_json();
    assert!(acct["email_verified_at"].is_string(), "{}", pretty(&acct));

    // --- rename
    dave.wt()
        .args(["tenant", "rename", "acme", "acme-inc"])
        .fails();
    let renamed = wt_json(&olga, &["tenant", "rename", "acme", "acme-inc"]);
    assert_eq!(
        renamed["tenant"]["slug"],
        "acme-inc",
        "{}",
        pretty(&renamed)
    );
    assert_eq!(
        renamed["reservation"]["slug"],
        "acme",
        "{}",
        pretty(&renamed)
    );
    // REST: the old slug answers 301 tenant_moved with the new slug.
    let moved = olga.api().get("/api/v1/tenants/acme");
    moved.assert_status(301);
    let body = moved.json();
    assert_eq!(body["error"]["code"], "tenant_moved", "{}", pretty(&body));
    assert_eq!(
        body["error"]["details"]["new_slug"],
        "acme-inc",
        "{}",
        pretty(&body)
    );
    let out = olga.wt().args(["tenant", "show", "acme"]).fails();
    assert!(
        out.mentions("renamed to 'acme-inc'") && out.mentions("wt tenant switch acme-inc"),
        "{out}"
    );
    // gRPC: pushing through the old remote tenant fails with the hint.
    repo.commit(&[("NOTES.md", "after rename\n")], "after rename");
    let out = repo.wt().arg("push").fails();
    assert!(
        out.mentions("acme-inc") && out.mentions("wt tenant switch"),
        "{out}"
    );
    let switched = repo.wt().args(["tenant", "switch", "acme-inc"]).json();
    assert_eq!(switched["previous_tenant"], "acme", "{}", pretty(&switched));
    repo.push();
    // The old slug cannot be claimed.
    let out = dave.wt().args(["tenant", "create", "acme"]).fails();
    assert!(out.mentions("reserved"), "{out}");

    // --- delete: suspended, keys revoked, read-only; restore
    let olga_api = olga.api();
    olga_api
        .post(
            "/api/v1/tenants/acme-inc/service-accounts",
            json!({"name": "ci", "role": "developer"}),
        )
        .assert_ok();
    let key = olga_api
        .post(
            "/api/v1/tenants/acme-inc/service-accounts/ci/keys",
            json!({"name": "k"}),
        )
        .ok_json();
    let key = key["token"].as_str().expect("key token").to_string();
    let sa_api = env.server().api().with_token("sa:ci", &key);
    sa_api.get("/api/v1/tenants/acme-inc").assert_ok();

    dave.wt()
        .args(["tenant", "delete", "acme-inc", "--yes"])
        .fails();
    let d = wt_json(&olga, &["tenant", "delete", "acme-inc", "--yes"]);
    assert_eq!(d["slug"], "acme-inc", "{}", pretty(&d));
    assert_eq!(d["revoked_keys"], 1, "{}", pretty(&d));
    sa_api.get("/api/v1/tenants/acme-inc").assert_status(401);
    let shown = olga.wt().args(["tenant", "show", "acme-inc"]).ok();
    assert!(
        shown.mentions("suspended") && shown.mentions("deletion"),
        "{shown}"
    );
    repo.commit(&[("NOTES.md", "while deleted\n")], "while deleted");
    let out = repo.wt().arg("push").fails();
    assert!(out.mentions("suspended") || out.mentions("denied"), "{out}");
    dave.wt().args(["tenant", "restore", "acme-inc"]).fails();
    let restored = wt_json(&olga, &["tenant", "restore", "acme-inc"]);
    assert!(
        restored["status"].is_null() || restored["status"] == "",
        "{}",
        pretty(&restored)
    );
    repo.push();

    // --- delete again and let the purge job run (4 s window, 1 s tick)
    olga.wt()
        .args(["tenant", "delete", "acme-inc", "--yes"])
        .ok();
    let admin = env.server().admin_api();
    poll_until(
        "tenant acme-inc purged",
        Duration::from_secs(30),
        Duration::from_millis(250),
        || {
            let r = admin.get("/api/v1/tenants/acme-inc");
            if r.status == 404 {
                Ok(())
            } else {
                Err(format!("{} {}", r.status, r.text()))
            }
        },
    );
    admin.get("/api/v1/trees/acme-inc/proj").assert_status(404);
    let tenants = wt_json(&olga, &["tenant", "list"]);
    assert!(
        !tenants
            .as_array()
            .map(|a| a.iter().any(|t| t["slug"] == "acme-inc"))
            .unwrap_or(false),
        "{}",
        pretty(&tenants)
    );
    poll_until(
        "tenant.purge audit event",
        Duration::from_secs(10),
        Duration::from_millis(100),
        || {
            let audit = admin.get("/api/v1/audit?action=tenant.purge").ok_json();
            if audit
                .as_array()
                .is_some_and(|a| a.iter().any(|e: &Value| e["action"] == "tenant.purge"))
            {
                Ok(())
            } else {
                Err(pretty(&audit))
            }
        },
    );
    // The purged slug stays reserved.
    let out = dave.wt().args(["tenant", "create", "acme-inc"]).fails();
    assert!(out.mentions("reserved"), "{out}");
}
