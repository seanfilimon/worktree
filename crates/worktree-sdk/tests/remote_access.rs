//! `worktree_sdk::remote::access` against the in-process fake server:
//! SyncAccessConfig (plan / apply, tenant and tree documents) and
//! GetAccessConfig (pull).

mod remote_fake_server;

use remote_fake_server::FakeServer;
use worktree_protocol::proto::sync as wire;
use worktree_sdk::remote::access::{self, declarative, AccessTarget};
use worktree_sdk::remote::SyncClient;
use worktree_sdk::SdkError;

const ROLES: &str =
    "[[role]]\nname = \"release-manager\"\npermissions = [\"tree:read\", \"release:*\"]\n";
const POLICIES: &str = "[[policy]]\nname = \"protect-main\"\n[[policy.statement]]\neffect = \"deny\"\nactions = [\"branch:push\"]\nresources = [\"wt:branch/alice/web/main\"]\nprincipals = [\"role:developer\"]\n";

fn compiled() -> String {
    declarative::compile(Some(ROLES), Some(POLICIES), &declarative::Scope::Tenant)
        .expect("valid document")
        .json
}

#[tokio::test]
async fn plan_apply_and_pull_tenant_document() {
    let server = FakeServer::start().await;
    let token = server.add_account("alice", "pw");
    let mut c = SyncClient::connect(&server.url, Some(&token))
        .await
        .unwrap();
    let target = AccessTarget::tenant("alice");
    let json = compiled();

    let plan = access::sync_with(&mut c, &target, &json, true)
        .await
        .unwrap();
    assert!(plan.accepted && plan.dry_run);
    assert_eq!(
        plan.changes,
        vec![
            "role release-manager: create",
            "policy protect-main: create"
        ]
    );
    {
        let st = server.state();
        let req = st.last_access_sync.as_ref().unwrap();
        assert!(req.dry_run);
        assert_eq!(req.config_type, wire::AccessConfigType::Policies as i32);
        assert_eq!(req.tenant_id.as_ref().unwrap().value, "alice");
        assert!(req.tree_id.is_none());
        assert_eq!(
            req.config_hash.as_ref().unwrap().value,
            blake3::hash(json.as_bytes()).as_bytes().to_vec()
        );
        assert!(st.access_docs.is_empty(), "a plan must not store");
    }
    assert!(access::pull_with(&mut c, &target).await.unwrap().is_none());

    let applied = access::sync_with(&mut c, &target, &json, false)
        .await
        .unwrap();
    assert!(applied.accepted && !applied.dry_run);
    let pulled = access::pull_with(&mut c, &target).await.unwrap().unwrap();
    assert_eq!(pulled.json, json);
    assert_eq!(pulled.updated_by, "alice");
    assert!(pulled.updated_at.is_some());
    assert_eq!(pulled.document.roles[0].name, "release-manager");
    // The pulled document decompiles to TOML that compiles back identically.
    let roles = declarative::roles_to_toml(&pulled.document.roles);
    let pols = declarative::policies_to_toml(&pulled.document.policies);
    let again =
        declarative::compile(Some(&roles), Some(&pols), &declarative::Scope::Tenant).unwrap();
    assert_eq!(again.json, json);
}

#[tokio::test]
async fn tree_documents_resolve_the_tree_and_strip_prefixes() {
    let server = FakeServer::start().await;
    let token = server.add_account("alice", "pw");
    let tree = server.add_tree("alice", "web");
    let mut c = SyncClient::connect(&server.url, Some(&token))
        .await
        .unwrap();
    let target = AccessTarget::tree("alice", "web");
    assert_eq!(target.to_string(), "tree alice/web");
    let json = r#"{"roles":[],"policies":[{"name":"web/freeze","description":"","enabled":true,"statements":[]}]}"#;
    access::sync_with(&mut c, &target, json, false)
        .await
        .unwrap();
    assert_eq!(
        server
            .state()
            .last_access_sync
            .as_ref()
            .unwrap()
            .tree_id
            .as_ref()
            .unwrap()
            .value,
        tree
    );
    let pulled = access::pull_with(&mut c, &target).await.unwrap().unwrap();
    assert_eq!(pulled.document.policies[0].name, "freeze");
    // The tenant document is separate.
    assert!(access::pull_with(&mut c, &AccessTarget::tenant("alice"))
        .await
        .unwrap()
        .is_none());
    // Unknown trees are reported as such.
    let err = access::sync_with(&mut c, &AccessTarget::tree("alice", "nope"), json, true)
        .await
        .unwrap_err();
    assert!(matches!(err, SdkError::TreeNotFound(_)), "{err:?}");
}

#[tokio::test]
async fn refusals_and_authentication() {
    let server = FakeServer::start().await;
    let token = server.add_account("alice", "pw");
    let mut c = SyncClient::connect(&server.url, Some(&token))
        .await
        .unwrap();
    let plan = access::sync_with(&mut c, &AccessTarget::tenant("alice"), "[1]", false)
        .await
        .unwrap();
    assert!(!plan.accepted);
    assert_eq!(plan.errors, vec!["content is not a JSON object"]);

    let mut anon = SyncClient::connect(&server.url, None).await.unwrap();
    for err in [
        access::sync_with(&mut anon, &AccessTarget::tenant("alice"), "{}", true)
            .await
            .unwrap_err(),
        access::pull_with(&mut anon, &AccessTarget::tenant("alice"))
            .await
            .unwrap_err(),
    ] {
        assert!(matches!(err, SdkError::NotLoggedIn(_)), "{err:?}");
    }
}
