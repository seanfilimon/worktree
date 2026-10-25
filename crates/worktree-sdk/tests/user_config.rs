//! The per-user config layer (`<wt home>/config.toml`). Its own test binary
//! because it sets `WT_HOME` for the whole process.

mod common;

use common::Fixture;
use worktree_sdk::config::{user_config_path, ConfigLevel, ConfigScope};

#[test]
fn user_config_supplies_defaults_below_the_worktree() {
    let home = tempfile::tempdir().unwrap();
    std::env::set_var("WT_HOME", home.path());
    std::fs::write(
        home.path().join("config.toml"),
        "[user]\nname = \"Ada\"\nemail = \"ada@example.com\"\n\n[auto_snapshot]\ninactivity_secs = 1\n\n[secret_scanning]\nexclude = [\"a/\"]\n",
    )
    .unwrap();

    let mut fx = Fixture::new();
    // `[user]` from the user file (the worktree file's empty strings are unset).
    assert_eq!(fx.repo.author(), "Ada <ada@example.com>");
    let stack = fx.repo.config_stack(None).unwrap();
    let name = stack.get("user.name").unwrap().unwrap();
    assert_eq!(name.value, "Ada");
    assert_eq!(name.origin.level, ConfigLevel::User);
    assert_eq!(
        name.origin.path.as_deref(),
        Some(user_config_path().as_path())
    );
    // Sections the user file may not set are ignored.
    let eff = stack.effective().unwrap();
    assert_eq!(eff.auto_snapshot.inactivity_secs, 30);

    // Worktree values win; additive arrays accumulate.
    fx.repo
        .set_config_value(&ConfigScope::Worktree, "user.name", "Bob")
        .unwrap();
    fx.repo
        .set_config_value(&ConfigScope::Worktree, "secret_scanning.exclude", "b/")
        .unwrap();
    let eff = fx.repo.effective_config(None).unwrap();
    assert_eq!(eff.user.name, "Bob");
    assert_eq!(eff.secret_scanning.exclude, vec!["a/", "b/"]);
    assert_eq!(fx.repo.author(), "Bob <ada@example.com>");

    // --global writes the user file and keeps its other keys.
    fx.repo
        .set_config_value(
            &ConfigScope::User,
            "secret_scanning.block_on_match",
            "false",
        )
        .unwrap();
    let text = std::fs::read_to_string(user_config_path()).unwrap();
    assert!(text.contains("block_on_match = false"), "{text}");
    assert!(
        text.contains("inactivity_secs = 1"),
        "unknown-at-level keys kept: {text}"
    );
    assert!(
        !fx.repo
            .effective_config(None)
            .unwrap()
            .secret_scanning
            .block_on_match
    );
    assert!(fx
        .repo
        .set_config_value(&ConfigScope::User, "hooks.enabled", "false")
        .is_err());
}
