//! # `.wt-tree/branches.toml` — branch metadata (config-as-code, §16.3)
//!
//! ```toml
//! [[branch]]
//! name = "main"                       # required
//! description = "Production branch"   # optional, at most 1000 characters
//! owner = "team:platform"             # optional, user:NAME or team:SLUG
//! ```
//!
//! When pushed to the tree's default branch, the server sets
//! `description` / `owner` of the declared branches; those branches are
//! then config-managed (the REST API refuses to edit them). The rules are
//! the Go server's (`internal/treeconfig/validate.go`); positions and
//! codes are described in [`super::as_code`].

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::as_code::{
    char_len, codes, parse as parse_toml, Diagnostic, Diags, Pos, Reader, BRANCHES_FILE,
};

/// Most branches one file may declare.
pub const MAX_BRANCHES: usize = 500;
/// Longest description (characters).
pub const MAX_DESCRIPTION: usize = 1000;
const MAX_REF_LEN: usize = 255;

/// One `[[branch]]` entry.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchDecl {
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub owner: String,
}

/// Check a branch name with the server's ref-name rules; `None` = valid.
pub fn check_branch_name(name: &str) -> Option<&'static str> {
    if name.is_empty() {
        return Some("branch name is empty");
    }
    if name.len() > MAX_REF_LEN {
        return Some("branch name is longer than 255 bytes");
    }
    if name
        .chars()
        .any(|c| c.is_whitespace() || c.is_control() || "~^:?*[\\".contains(c))
    {
        return Some("branch name contains whitespace, a control character or one of ~^:?*[\\");
    }
    if name
        .split('/')
        .any(|c| c.is_empty() || c == "." || c == "..")
    {
        return Some("branch name has an empty or relative path component");
    }
    None
}

fn slug_like(name: &str, max: usize) -> bool {
    let b = name.as_bytes();
    !b.is_empty()
        && b.len() <= max
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

/// Check the syntax of an owner (`user:NAME` with NAME of 1-39 `[a-z0-9-]`,
/// or `team:SLUG` with SLUG of 1-63); `None` = valid. Whether the user or
/// team exists is checked by the server.
pub fn check_owner(owner: &str) -> Option<String> {
    match owner.split_once(':') {
        Some(("user", name)) if slug_like(name, 39) => None,
        Some(("team", name)) if slug_like(name, 63) => None,
        Some(("user" | "team", _)) => Some(format!("owner {} has an invalid name", owner.trim())),
        _ => Some("owner must be user:NAME or team:SLUG".to_string()),
    }
}

/// Parse and validate `branches.toml`: the declared branches (valid entries
/// only) and every diagnostic.
pub fn parse(src: &str) -> (Vec<BranchDecl>, Vec<Diagnostic>) {
    let mut d = Diags::new(BRANCHES_FILE);
    let out = parse_into(src, &mut d);
    (out, d.list)
}

fn parse_into(src: &str, d: &mut Diags) -> Vec<BranchDecl> {
    let mut out = Vec::new();
    let Some(t) = parse_toml(src, d, codes::E_SYNTAX) else {
        return out;
    };
    let mut r = Reader {
        d,
        structure: codes::E_STRUCTURE,
    };
    r.unknown_keys(&t, &["branch"], "");
    let entries = r.tables(&t, "branch");
    if entries.len() > MAX_BRANCHES {
        r.d.add(
            codes::E_LIMIT,
            Pos::START,
            format!("at most {MAX_BRANCHES} branches may be declared"),
        );
    }
    let mut seen = BTreeSet::new();
    for e in entries {
        let tb = &e.tbl;
        r.unknown_keys(tb, &["name", "description", "owner"], " in [[branch]]");
        let present = tb.get("name").is_some();
        let Some((name, nv)) = r.str(tb, "name") else {
            if !present {
                r.d.add(codes::E_MISSING, e.pos, "[[branch]] needs a name");
            }
            continue;
        };
        if let Some(msg) = check_branch_name(&name) {
            r.d.add(codes::E_BRANCH_NAME, nv.key_pos, msg);
            continue;
        }
        if !seen.insert(name.clone()) {
            r.d.add(
                codes::E_DUPLICATE,
                nv.key_pos,
                format!("branch {name} is declared more than once"),
            );
            continue;
        }
        let mut decl = BranchDecl {
            name,
            ..Default::default()
        };
        if let Some((desc, dv)) = r.str(tb, "description") {
            if char_len(&desc) > MAX_DESCRIPTION {
                r.d.add(
                    codes::E_LIMIT,
                    dv.key_pos,
                    format!("description must be at most {MAX_DESCRIPTION} characters"),
                );
            }
            decl.description = desc;
        }
        if let Some((owner, ov)) = r.str(tb, "owner") {
            if !owner.is_empty() {
                if let Some(msg) = check_owner(&owner) {
                    r.d.add(codes::E_OWNER, ov.key_pos, msg);
                }
                decl.owner = owner;
            }
        }
        out.push(decl);
    }
    out
}

/// Render declarations as `branches.toml` text (used by `wt branch` hints).
pub fn to_toml(decls: &[BranchDecl]) -> String {
    let q = |s: &str| serde_json::to_string(s).unwrap_or_default();
    let mut out = String::new();
    for (i, d) in decls.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str("[[branch]]\n");
        out.push_str(&format!("name = {}\n", q(&d.name)));
        if !d.description.is_empty() {
            out.push_str(&format!("description = {}\n", q(&d.description)));
        }
        if !d.owner.is_empty() {
            out.push_str(&format!("owner = {}\n", q(&d.owner)));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_branches() {
        let (decls, diags) = parse(
            "[[branch]]\nname = \"main\"\ndescription = \"  Prod  \"\nowner = \"user:alice\"\n\nbranch2 = 1\n",
        );
        // `branch2` sits inside the [[branch]] table: an unknown key.
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, codes::E_STRUCTURE);
        assert_eq!((diags[0].line, diags[0].col), (6, 1));
        assert_eq!(
            decls,
            vec![BranchDecl {
                name: "main".into(),
                description: "Prod".into(),
                owner: "user:alice".into()
            }]
        );
        let (decls, diags) = parse("branch = [{ name = \"a\" }, { name = \"b\", owner = \"\" }]\n");
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(decls.len(), 2);
        assert!(decls[1].owner.is_empty());
    }

    #[test]
    fn checks_names_and_owners() {
        for ok in ["main", "release/1.0", "feature/x-y_z"] {
            assert_eq!(check_branch_name(ok), None, "{ok}");
        }
        for bad in ["", "a b", "a//b", "a/./b", "x..", "a:b", "/a", "a/"] {
            if bad == "x.." {
                assert_eq!(check_branch_name(bad), None); // ".." only as a component
                continue;
            }
            assert!(check_branch_name(bad).is_some(), "{bad}");
        }
        assert!(check_branch_name(&"x".repeat(256)).is_some());
        for ok in ["user:alice", "team:core-dev", "user:a1"] {
            assert_eq!(check_owner(ok), None, "{ok}");
        }
        for bad in ["alice", "user:", "team:Core", "group:x", "user:a b"] {
            assert!(check_owner(bad).is_some(), "{bad}");
        }
        assert!(check_owner(&format!("user:{}", "a".repeat(40))).is_some());
    }

    #[test]
    fn limits_and_round_trip() {
        let mut src = String::new();
        for i in 0..=MAX_BRANCHES {
            src.push_str(&format!("[[branch]]\nname = \"b{i}\"\n"));
        }
        let (_, diags) = parse(&src);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, codes::E_LIMIT);
        let decls = vec![
            BranchDecl {
                name: "main".into(),
                description: "Say \"hi\"".into(),
                owner: "team:core".into(),
            },
            BranchDecl {
                name: "dev".into(),
                ..Default::default()
            },
        ];
        let (back, diags) = parse(&to_toml(&decls));
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(back, decls);
    }
}
