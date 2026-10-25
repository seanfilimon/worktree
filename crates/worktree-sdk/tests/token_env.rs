//! `WT_TOKEN` overrides saved logins of the default credential store.
//! (Its own test binary: it changes process environment variables.)

use worktree_sdk::remote::{Credential, CredentialStore};

#[test]
fn wt_token_overrides_the_default_store_only() {
    let home = tempfile::tempdir().unwrap();
    std::env::set_var("WT_HOME", home.path());
    let store = CredentialStore::default_location();
    store
        .put(
            "http://localhost:7420",
            &Credential {
                username: "alice".into(),
                token: "wt_saved".into(),
            },
        )
        .unwrap();
    assert_eq!(
        store.get("http://localhost:7420").unwrap().unwrap().token,
        "wt_saved"
    );

    std::env::set_var("WT_TOKEN", "wt_from_env");
    std::env::set_var("WT_TOKEN_USER", "sa:acme/ci");
    let cred = store.get("http://other.example:7420").unwrap().unwrap();
    assert_eq!(cred.token, "wt_from_env");
    assert_eq!(cred.username, "sa:acme/ci");

    // Explicit stores (tests, tools) are unaffected.
    let explicit = CredentialStore::new(home.path().join("credentials.toml"));
    assert_eq!(
        explicit
            .get("http://localhost:7420")
            .unwrap()
            .unwrap()
            .token,
        "wt_saved"
    );

    std::env::set_var("WT_TOKEN", "  ");
    assert_eq!(
        store.get("http://localhost:7420").unwrap().unwrap().token,
        "wt_saved"
    );
    std::env::remove_var("WT_TOKEN");
    std::env::remove_var("WT_TOKEN_USER");
}
