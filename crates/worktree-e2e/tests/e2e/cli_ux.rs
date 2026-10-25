//! CLI ergonomics and the REST escape hatch (ENGINEERING_PLAN §14.8,
//! §14.10): `wt completions`, aliases from `wt config set alias.NAME`
//! (built-ins cannot be shadowed), `wt doctor --json` against the running
//! server, `wt open --print` URLs, `wt api` with the saved credential and
//! the OpenAPI description served at `GET /api/v1/openapi.yaml`.

use serde_json::Value;
use worktree_e2e::json::{find_objects, pretty, str_field};
use worktree_e2e::{require_e2e, Api, TestEnv};

use crate::common::{publish_tree, TREE};

/// (name, status) of every doctor check.
fn checks(v: &Value) -> Vec<(String, String)> {
    find_objects(v, &["name", "status", "message"])
        .into_iter()
        .map(|c| (str_field(c, "name"), str_field(c, "status")))
        .collect()
}

#[test]
fn cli_ux() {
    require_e2e!();
    let env = TestEnv::with_server("cli_ux");
    let alice = env.user("alice");
    let a = publish_tree(
        &env,
        &alice,
        &[("README.md", "# ux\n"), ("src/lib.rs", "\n")],
    );
    let head = a.head_id();

    // --- completions
    for shell in ["bash", "zsh", "fish", "powershell", "elvish"] {
        let out = alice.wt().args(["completions", shell]).ok();
        assert!(
            out.stdout.len() > 1000 && out.stdout.contains("cherry-pick"),
            "{shell} completions look incomplete ({} bytes)",
            out.stdout.len()
        );
    }
    let bash = alice.wt().args(["completions", "bash"]).ok();
    assert!(bash.stdout.contains("_wt"), "bash completion function");
    alice.wt().args(["completions", "tcsh"]).code(2);

    // --- aliases
    a.wt().args(["config", "set", "alias.st", "status"]).ok();
    a.wt()
        .args(["config", "set", "alias.last", "log -n 1"])
        .ok();
    assert_eq!(
        a.wt()
            .args(["config", "get", "alias.st"])
            .ok()
            .stdout
            .trim(),
        "status"
    );
    // (Compared as JSON: the text output has relative times.)
    let st = a.wt().arg("st").ok();
    assert!(st.stdout.starts_with("On tree proj, branch main"), "{st}");
    let status = a.wt().args(["--json", "status"]).ok().json();
    assert_eq!(
        a.wt().args(["--json", "st"]).ok().json(),
        status,
        "`wt st` runs `wt status`"
    );
    a.write("dirty.txt", "x\n");
    let st = a.wt().args(["--json", "st"]).ok().json();
    assert!(
        st.to_string().contains("dirty.txt"),
        "alias with global flags:\n{}",
        pretty(&st)
    );
    a.remove("dirty.txt");
    let last = a.wt().args(["--json", "last"]).ok().json();
    let snaps = worktree_e2e::json::snapshots(&last);
    assert_eq!(snaps.len(), 1, "{}", pretty(&last));
    assert_eq!(str_field(&snaps[0], "id"), head);
    // Extra arguments are appended to the expansion.
    let short = a.wt().args(["last", "--oneline"]).ok();
    assert!(short.stdout.contains(&head[..7]), "{short}");
    // Built-in commands cannot be shadowed (the alias is stored but ignored;
    // `wt doctor` warns about it below).
    a.wt().args(["config", "set", "alias.status", "log"]).ok();
    let s = a.wt().arg("status").ok();
    assert!(s.stdout.starts_with("On tree proj, branch main"), "{s}");
    assert_eq!(
        a.wt().args(["--json", "status"]).ok().json(),
        status,
        "an alias must not shadow `status`"
    );
    // Aliases appear in completion scripts generated afterwards.
    let bash = a.wt().args(["completions", "bash"]).ok();
    assert!(bash.stdout.contains("last"), "aliases are completed");
    // Unknown commands keep the "did you mean" hint.
    let out = a.wt().arg("statsu").code(2);
    assert!(out.mentions("status"), "{out}");

    // --- doctor against the running server
    let doctor = a.wt().args(["doctor", "--json"]).run();
    let report = doctor.json();
    let all = checks(&report);
    assert_eq!(
        report["errors"].as_u64(),
        Some(0),
        "doctor errors:\n{}",
        pretty(&report)
    );
    assert_eq!(report["ok"], Value::Bool(true), "{}", pretty(&report));
    assert!(doctor.success(), "{doctor}");
    for name in [
        "worktree",
        "config.worktree",
        "user",
        "origin: server",
        "origin: sync",
        "origin: version",
        "origin: clock",
        "origin: credential",
        "origin: token",
    ] {
        assert!(
            all.contains(&(name.to_string(), "ok".to_string())),
            "doctor check `{name}` ok: {all:?}"
        );
    }
    let check = |v: &Value, name: &str| -> Value {
        find_objects(v, &["name", "status", "message"])
            .into_iter()
            .find(|c| c["name"] == name)
            .cloned()
            .unwrap_or_else(|| panic!("no `{name}` check in\n{}", pretty(v)))
    };
    assert!(
        str_field(&check(&report, "origin: credential"), "message").contains("alice"),
        "{}",
        pretty(&report)
    );
    assert!(
        str_field(&check(&report, "origin: server"), "message").contains(&env.url()),
        "{}",
        pretty(&report)
    );
    assert_eq!(
        str_field(&check(&report, "aliases"), "status"),
        "warn",
        "the shadowing alias is reported:\n{}",
        pretty(&report)
    );
    let text = a.wt().arg("doctor").ok();
    assert!(text.stdout.contains('✓'), "{text}");
    a.wt().args(["config", "unset", "alias.status"]).ok();
    // An unreachable remote is an error (exit 1).
    a.wt()
        .args([
            "remote",
            "add",
            "dead",
            "http://127.0.0.1:9",
            "--tenant",
            "alice",
        ])
        .ok();
    let dead = a.wt().args(["doctor", "--json", "--timeout", "2"]).code(1);
    let dead = dead.json();
    assert_eq!(dead["ok"], Value::Bool(false), "{}", pretty(&dead));
    assert_eq!(
        str_field(&check(&dead, "dead: server"), "status"),
        "error",
        "{}",
        pretty(&dead)
    );
    a.wt().args(["remote", "remove", "dead"]).confirmed();
    let report = a.wt().args(["doctor", "--json"]).ok().json();
    assert_eq!(
        report["warnings"].as_u64(),
        Some(1),
        "only the daemon warning:\n{}",
        pretty(&report)
    );
    let offline = a.wt().args(["doctor", "--json", "--offline"]).ok().json();
    assert!(
        checks(&offline).iter().any(|(_, s)| s == "skip"),
        "{}",
        pretty(&offline)
    );

    // --- wt open --print
    let base = format!("{}/admin/trees/alice/{TREE}", env.url());
    let url = a.wt().args(["open", "--print"]).ok();
    assert_eq!(url.stdout.trim(), base, "{url}");
    let mr = a.wt().args(["open", "mr", "7", "--print"]).ok();
    assert!(
        mr.stdout.trim().starts_with(&base) && mr.stdout.contains('7'),
        "{mr}"
    );
    let snap = a.wt().args(["open", "snapshot", "HEAD", "--print"]).ok();
    assert!(
        snap.stdout.trim().starts_with(&base) && snap.stdout.contains(&head),
        "{snap}"
    );
    let file = a.wt().args(["open", "file", "src/lib.rs", "--print"]).ok();
    assert!(
        file.stdout.trim().starts_with(&base) && file.stdout.contains("src/lib.rs"),
        "{file}"
    );
    let json = a.wt().args(["--json", "open", "--print"]).ok().json();
    assert!(json.to_string().contains(&base), "{}", pretty(&json));

    // --- wt api
    let me = a.wt().args(["api", "GET", "auth/me"]).ok().json();
    assert!(me.to_string().contains("\"alice\""), "{}", pretty(&me));
    let me_abs = a.wt().args(["api", "GET", "/api/v1/auth/me"]).ok().json();
    assert_eq!(me, me_abs);
    let branches = a
        .wt()
        .args(["api", "GET", &format!("trees/alice/{TREE}/branches")])
        .ok()
        .json();
    assert!(
        branches.to_string().contains(&head),
        "{}",
        pretty(&branches)
    );
    let out = a
        .wt()
        .args([
            "api",
            "PATCH",
            &format!("trees/alice/{TREE}"),
            "-d",
            r#"{"description":"set through wt api"}"#,
        ])
        .ok();
    assert!(out.mentions("set through wt api"), "{out}");
    let missing = a.wt().args(["api", "GET", "trees/alice/nope"]).code(1);
    assert!(
        missing.mentions("404") || missing.mentions("not_found"),
        "{missing}"
    );
    a.wt()
        .args(["api", "GET", "tenants", "-d", "{not json"])
        .code(2);
    // Without a login the server refuses.
    let carol = env.user("carol");
    let anon = carol
        .wt()
        .args(["--server", &env.url(), "api", "GET", "auth/me"])
        .fails();
    assert!(
        anon.mentions("401") || anon.mentions("unauth") || anon.mentions("login"),
        "{anon}"
    );

    // --- OpenAPI description
    let spec = Api::new(&env.url()).get("/api/v1/openapi.yaml");
    spec.assert_status(200);
    let yaml = spec.text();
    assert!(
        yaml.contains("openapi: 3.1"),
        "{}",
        &yaml[..yaml.len().min(400)]
    );
    assert!(yaml.contains("/api/v1/auth/me") || yaml.contains("/auth/me:"));
    let raw = a.wt().args(["api", "GET", "openapi.yaml", "--raw"]).ok();
    assert_eq!(
        raw.stdout.trim(),
        yaml.trim(),
        "wt api --raw prints the body"
    );
}
