//! Smoke test of every server-side command group with `--json` (§12.4):
//! each invocation must exit 0 and print exactly one valid JSON document.
//! Failures are collected so one run reports every broken command.

use serde_json::{json, Value};
use worktree_e2e::json::{find_objects, pretty};
use worktree_e2e::{require_e2e, TestEnv, User};

use crate::common::{publish_tree, TREE};
use crate::iam_common::{json_file, tree_ref};

#[derive(Default)]
struct Smoke {
    ran: usize,
    failures: Vec<String>,
}

impl Smoke {
    /// `wt --json <args>` as `user`; Some(JSON) on success.
    fn run(&mut self, user: &User, args: &[&str]) -> Option<Value> {
        self.ran += 1;
        let out = user.wt().arg("--json").args(args).run();
        if !out.success() {
            self.failures.push(format!("exit code\n{out}"));
            return None;
        }
        match serde_json::from_str::<Value>(out.stdout.trim()) {
            Ok(v) => Some(v),
            Err(e) => {
                self.failures
                    .push(format!("stdout is not one JSON document ({e})\n{out}"));
                None
            }
        }
    }

    /// Like `run`, plus a structural check on the JSON.
    fn check(
        &mut self,
        user: &User,
        args: &[&str],
        what: &str,
        ok: impl Fn(&Value) -> bool,
    ) -> Option<Value> {
        let v = self.run(user, args)?;
        if !ok(&v) {
            self.failures.push(format!(
                "`wt --json {}`: expected {what}, got\n{}",
                args.join(" "),
                pretty(&v)
            ));
        }
        Some(v)
    }

    fn finish(self) {
        assert!(
            self.failures.is_empty(),
            "{} of {} CLI invocations failed:\n\n{}",
            self.failures.len(),
            self.ran,
            self.failures.join("\n\n==========\n\n")
        );
    }
}

fn has(v: &Value, keys: &[&str]) -> bool {
    !find_objects(v, keys).is_empty()
}

#[test]
fn cli_everything() {
    require_e2e!();
    let env = TestEnv::with_server("cli_everything");
    let alice = env.user("alice");
    publish_tree(
        &env,
        &alice,
        &[("README.md", "# smoke\n"), ("src/a.txt", "a\n")],
    );
    let bob = env.user("bob");
    bob.register();
    let admin = env.admin_user();
    admin.login();
    let tree = tree_ref("alice");
    let t = tree.as_str();
    let mut s = Smoke::default();

    // --- tenants
    s.check(&alice, &["tenant", "list"], "tenant alice", |v| {
        find_objects(v, &["slug"])
            .iter()
            .any(|x| x["slug"] == "alice")
    });
    s.check(&alice, &["tenant", "show", "alice"], "a tenant", |v| {
        has(v, &["slug", "kind"])
    });
    s.check(&alice, &["tenant", "members", "alice"], "members", |v| {
        has(v, &["account", "role"])
    });
    s.check(
        &alice,
        &["tenant", "create", "acme-smoke", "--name", "Acme Smoke"],
        "the new tenant",
        |v| {
            find_objects(v, &["slug"])
                .iter()
                .any(|x| x["slug"] == "acme-smoke")
        },
    );

    // --- trees
    s.check(
        &alice,
        &["trees", "list", "--tenant", "alice"],
        "tree proj",
        |v| find_objects(v, &["name"]).iter().any(|x| x["name"] == TREE),
    );
    s.check(&alice, &["trees", "show", t], "a tree summary", |v| {
        has(v, &["branch_count"])
    });
    s.check(&alice, &["trees", "branches", t], "branch main", |v| {
        find_objects(v, &["name", "tip"])
            .iter()
            .any(|b| b["name"] == "main")
    });
    s.run(&alice, &["trees", "tags", t]);
    s.check(&alice, &["trees", "log", t, "-n", "5"], "snapshots", |v| {
        has(v, &["id", "parents"])
    });
    s.run(&alice, &["trees", "stats", t]);
    s.run(&alice, &["trees", "staged", t]);

    // --- review, protection, licenses, audit, search
    s.run(&alice, &["mr", "list", t, "--state", "all"]);
    s.run(&alice, &["protect", "list", t]);
    s.run(&alice, &["license", "list", t]);
    s.run(&alice, &["audit", "--tenant", "alice", "-n", "20"]);
    s.run(&alice, &["search", TREE]);

    // --- teams
    s.check(
        &alice,
        &["team", "create", "alice", "core", "--name", "Core"],
        "team core",
        |v| {
            find_objects(v, &["slug"])
                .iter()
                .any(|x| x["slug"] == "core")
        },
    );
    s.run(&alice, &["team", "list", "alice"]);
    s.check(
        &alice,
        &["team", "show", "alice", "core"],
        "{team, members, grants}",
        |v| v.get("team").is_some() && v.get("members").is_some(),
    );

    // --- roles
    s.check(&alice, &["role", "list", "alice"], "built-in roles", |v| {
        find_objects(v, &["name", "builtin"])
            .iter()
            .any(|r| r["name"] == "owner")
    });
    s.run(
        &alice,
        &[
            "role",
            "create",
            "alice",
            "auditor",
            "--permission",
            "tree:read",
            "--permission",
            "tenant:read",
        ],
    );
    s.run(&alice, &["role", "show", "alice", "auditor"]);

    // --- policies
    let policy = json_file(
        &env,
        "smoke-policy.json",
        &json!({
            "name": "smoke",
            "description": "smoke test",
            "enabled": false,
            "statements": [{
                "effect": "deny",
                "actions": ["tree:write"],
                "resources": [format!("wt:tree/alice/{TREE}")],
                "principals": ["user:nobody"],
                "conditions": {}
            }]
        }),
    );
    let policy = policy.to_str().expect("utf-8 path").to_string();
    s.check(
        &alice,
        &["policy", "validate", "alice", "--file", &policy],
        "{valid: true}",
        |v| {
            find_objects(v, &["valid"])
                .iter()
                .any(|x| x["valid"] == true)
        },
    );
    let created = s.run(&alice, &["policy", "create", "alice", "--file", &policy]);
    s.run(&alice, &["policy", "list", "alice"]);
    if let Some(id) = created.as_ref().and_then(|v| {
        find_objects(v, &["id", "statements"])
            .first()
            .and_then(|p| p["id"].as_str().map(String::from))
    }) {
        s.run(&alice, &["policy", "show", "alice", &id]);
    }

    // --- grants
    s.run(
        &alice,
        &["grant", "set", t, "--user", "bob", "--role", "viewer"],
    );
    s.check(&alice, &["grant", "list", t], "bob's grant", |v| {
        find_objects(v, &["subject_type", "subject"])
            .iter()
            .any(|g| g["subject"] == "bob")
    });

    // --- service accounts
    s.run(
        &alice,
        &["sa", "create", "alice", "bot", "--role", "viewer"],
    );
    s.run(&alice, &["sa", "list", "alice"]);
    s.check(
        &alice,
        &["sa", "show", "alice", "bot"],
        "{service_account, keys}",
        |v| v.get("service_account").is_some() && v.get("keys").is_some(),
    );
    s.check(
        &alice,
        &["sa", "key-create", "alice", "bot", "--name", "k1"],
        "{token, api_token}",
        |v| v.get("token").is_some() && v.get("api_token").is_some(),
    );

    // --- personal tokens, sessions, MFA
    s.check(
        &alice,
        &["token", "create", "smoke", "--ttl-hours", "1"],
        "{token, api_token}",
        |v| v.get("token").is_some() && v.get("api_token").is_some(),
    );
    s.run(&alice, &["token", "list"]);
    s.check(&alice, &["session", "list"], "a current session", |v| {
        find_objects(v, &["current"])
            .iter()
            .any(|x| x["current"] == true)
    });
    s.check(&alice, &["mfa", "status"], "mfa state", |v| {
        has(v, &["totp_enabled"])
    });

    // --- IAM introspection
    s.check(&alice, &["iam", "actions"], "the action catalog", |v| {
        let acts = find_objects(v, &["action", "area"]);
        ["tree:read", "branch:push", "iam:admin"]
            .iter()
            .all(|a| acts.iter().any(|x| x["action"] == *a))
    });
    s.check(
        &alice,
        &["iam", "effective", "--tenant", "alice", "--tree", t],
        "permissions",
        |v| has(v, &["permissions"]),
    );
    s.check(
        &alice,
        &[
            "iam",
            "effective",
            "--tenant",
            "alice",
            "--tree",
            t,
            "--principal",
            "user:bob",
        ],
        "bob's read-only permissions",
        |v| {
            let perms: Vec<&str> = find_objects(v, &["permissions"])
                .first()
                .and_then(|o| o["permissions"].as_array())
                .map(|a| a.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            perms.contains(&"tree:read") && !perms.contains(&"tree:write")
        },
    );
    s.check(
        &alice,
        &[
            "iam",
            "simulate",
            "--principal",
            "user:bob",
            "--action",
            "tree:read",
            "--tenant",
            "alice",
            "--tree",
            t,
        ],
        "an allowed Decision",
        |v| {
            find_objects(v, &["allowed", "effect"])
                .iter()
                .any(|d| d["allowed"] == true)
        },
    );

    // --- invitations
    s.check(
        &alice,
        &[
            "invite", "create", "alice", "--user", "bob", "--role", "viewer",
        ],
        "{invitation, code}",
        |v| v.get("invitation").is_some() && v.get("code").is_some(),
    );
    s.run(&alice, &["invite", "list", "alice"]);

    // --- server administration (admin)
    s.check(&admin, &["admin", "accounts"], "accounts", |v| {
        find_objects(v, &["username"])
            .iter()
            .any(|a| a["username"] == "alice")
    });
    s.run(&admin, &["admin", "system"]);
    s.check(&admin, &["admin", "stats"], "server stats", |v| {
        has(v, &["tenants", "accounts"])
    });
    s.run(&admin, &["audit", "-n", "5"]);
    s.check(&admin, &["sso", "show"], "an SSO config", |v| {
        has(v, &["enabled"])
    });

    s.finish();
}
