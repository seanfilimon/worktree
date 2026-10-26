//! # `.wt-tree/protections.toml` — branch protections (config-as-code, §16.3)
//!
//! ```toml
//! [[protection]]
//! pattern = "main"                 # required; path.Match glob, `**` segments
//! no_direct_push = true
//! required_approvals = 1           # 0-100
//! allowed_pushers = ["alice"]
//! no_delete = true
//! require_ci_pass = true
//! required_checks = ["ci/build"]
//! require_signed = false
//! require_linear = false
//! block_on_todos = false
//! merge_methods = ["squash"]       # fast-forward | merge | squash | rebase
//! no_force_push = true
//! require_owner_approval = false
//! ```
//!
//! When pushed to the tree's default branch, the declared protections
//! replace the protections the configuration created before; protections
//! set through the REST API (`wt protect`) with the same pattern are never
//! overwritten. The rules are the Go server's
//! (`internal/treeconfig/validate.go`); see [`super::as_code`].

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::as_code::{
    codes, parse as parse_toml, Diagnostic, Diags, Pos, Reader, PROTECTIONS_FILE,
};

/// Most protections one file may declare.
pub const MAX_PROTECTIONS: usize = 100;
/// Most entries of `allowed_pushers`.
pub const MAX_PUSHERS: usize = 100;
/// Most entries of `required_checks`.
pub const MAX_REQUIRED_CHECKS: usize = 50;
/// Largest `required_approvals`.
pub const MAX_APPROVALS: i64 = 100;
const MAX_CONTEXT_LEN: usize = 255;
const MAX_PATTERN_LEN: usize = 512;

/// Merge methods in preference order.
pub const MERGE_METHODS: &[&str] = &["fast-forward", "merge", "squash", "rebase"];

const KEYS: &[&str] = &[
    "pattern",
    "no_direct_push",
    "required_approvals",
    "allowed_pushers",
    "no_delete",
    "require_ci_pass",
    "required_checks",
    "require_signed",
    "require_linear",
    "block_on_todos",
    "merge_methods",
    "no_force_push",
    "require_owner_approval",
];

/// One `[[protection]]` entry (the server's `BranchProtection` fields).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Protection {
    pub pattern: String,
    pub no_direct_push: bool,
    pub required_approvals: u32,
    pub allowed_pushers: Vec<String>,
    pub no_delete: bool,
    pub require_ci_pass: bool,
    pub required_checks: Vec<String>,
    pub require_signed: bool,
    pub require_linear: bool,
    pub block_on_todos: bool,
    pub merge_methods: Vec<String>,
    pub no_force_push: bool,
    pub require_owner_approval: bool,
}

/// One end of a character-class range (Go's `getEsc`): a character other
/// than `-` / `]`, possibly escaped, that must not end the pattern.
fn class_char(chars: &[char], i: &mut usize) -> bool {
    if *i >= chars.len() || chars[*i] == '-' || chars[*i] == ']' {
        return false;
    }
    if chars[*i] == '\\' {
        *i += 1;
        if *i >= chars.len() {
            return false;
        }
    }
    *i += 1;
    *i < chars.len()
}

/// Whether `pattern` is a valid Go `path.Match` pattern.
fn valid_glob(pattern: &str) -> bool {
    let chars: Vec<char> = pattern.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\\' => {
                if i + 1 >= chars.len() {
                    return false;
                }
                i += 2;
            }
            '[' => {
                i += 1;
                if i < chars.len() && chars[i] == '^' {
                    i += 1;
                }
                let mut ranges = 0;
                loop {
                    if i < chars.len() && chars[i] == ']' && ranges > 0 {
                        i += 1;
                        break;
                    }
                    if !class_char(&chars, &mut i) {
                        return false;
                    }
                    if chars[i] == '-' {
                        i += 1;
                        if !class_char(&chars, &mut i) {
                            return false;
                        }
                    }
                    ranges += 1;
                }
            }
            _ => i += 1,
        }
    }
    true
}

/// Check a protection pattern like the server (`review.ValidatePattern`
/// plus the REST API's glob check); `None` = valid.
pub fn check_pattern(p: &str) -> Option<String> {
    if p.is_empty() {
        return Some("pattern is empty".into());
    }
    if p.len() > MAX_PATTERN_LEN {
        return Some("pattern is longer than 512 bytes".into());
    }
    if p.chars().any(char::is_control) {
        return Some("pattern contains a control character".into());
    }
    if p.split('/').any(str::is_empty) || !valid_glob(p) {
        return Some(format!(
            "pattern {p} is malformed (empty segment or bad glob)"
        ));
    }
    None
}

/// Parse and validate `protections.toml`: the declared protections (valid
/// entries only) and every diagnostic.
pub fn parse(src: &str) -> (Vec<Protection>, Vec<Diagnostic>) {
    let mut d = Diags::new(PROTECTIONS_FILE);
    let out = parse_into(src, &mut d);
    (out, d.list)
}

fn parse_into(src: &str, d: &mut Diags) -> Vec<Protection> {
    let mut out = Vec::new();
    let Some(t) = parse_toml(src, d, codes::E_SYNTAX) else {
        return out;
    };
    let mut r = Reader {
        d,
        structure: codes::E_STRUCTURE,
    };
    r.unknown_keys(&t, &["protection"], "");
    let entries = r.tables(&t, "protection");
    if entries.len() > MAX_PROTECTIONS {
        r.d.add(
            codes::E_LIMIT,
            Pos::START,
            format!("at most {MAX_PROTECTIONS} protections may be declared"),
        );
    }
    let mut seen = BTreeSet::new();
    for e in entries {
        let tb = &e.tbl;
        r.unknown_keys(tb, KEYS, " in [[protection]]");
        let present = tb.get("pattern").is_some();
        let Some((pattern, pv)) = r.str(tb, "pattern") else {
            if !present {
                r.d.add(codes::E_MISSING, e.pos, "[[protection]] needs a pattern");
            }
            continue;
        };
        if let Some(msg) = check_pattern(&pattern) {
            r.d.add(codes::E_PATTERN, pv.key_pos, msg);
            continue;
        }
        if !seen.insert(pattern.clone()) {
            r.d.add(
                codes::E_DUPLICATE,
                pv.key_pos,
                format!("pattern {pattern} is declared more than once"),
            );
            continue;
        }
        let mut p = Protection {
            pattern,
            ..Default::default()
        };
        let flag = |r: &mut Reader<'_>, key: &str| r.boolean(tb, key).unwrap_or(false);
        p.no_direct_push = flag(&mut r, "no_direct_push");
        p.no_delete = flag(&mut r, "no_delete");
        p.require_ci_pass = flag(&mut r, "require_ci_pass");
        p.require_signed = flag(&mut r, "require_signed");
        p.require_linear = flag(&mut r, "require_linear");
        p.block_on_todos = flag(&mut r, "block_on_todos");
        p.no_force_push = flag(&mut r, "no_force_push");
        p.require_owner_approval = flag(&mut r, "require_owner_approval");
        if let Some((n, nv)) = r.integer(tb, "required_approvals") {
            if !(0..=MAX_APPROVALS).contains(&n) {
                r.d.add(
                    codes::E_VALUE,
                    nv.key_pos,
                    format!("required_approvals must be between 0 and {MAX_APPROVALS}"),
                );
            } else {
                p.required_approvals = n as u32;
            }
        }
        let pushers = r.strings(tb, "allowed_pushers").unwrap_or_default();
        if pushers.len() > MAX_PUSHERS {
            let at = tb
                .get("allowed_pushers")
                .map(|v| v.key_pos)
                .unwrap_or(Pos::START);
            r.d.add(
                codes::E_LIMIT,
                at,
                format!("allowed_pushers lists at most {MAX_PUSHERS} users"),
            );
        }
        for (u, v) in pushers {
            if u.is_empty() || u.len() > 100 || u.chars().any(char::is_whitespace) {
                r.d.add(
                    codes::E_VALUE,
                    v.pos,
                    "allowed_pushers entries must be usernames",
                );
                continue;
            }
            if !p.allowed_pushers.contains(&u) {
                p.allowed_pushers.push(u);
            }
        }
        let checks = r.strings(tb, "required_checks").unwrap_or_default();
        if checks.len() > MAX_REQUIRED_CHECKS {
            let at = tb
                .get("required_checks")
                .map(|v| v.key_pos)
                .unwrap_or(Pos::START);
            r.d.add(
                codes::E_LIMIT,
                at,
                format!("required_checks lists at most {MAX_REQUIRED_CHECKS} contexts"),
            );
        }
        for (c, v) in checks {
            if c.is_empty() || c.len() > MAX_CONTEXT_LEN || c.chars().any(char::is_control) {
                r.d.add(
                    codes::E_VALUE,
                    v.pos,
                    format!(
                        "required_checks entries must be 1-{MAX_CONTEXT_LEN} bytes of printable text"
                    ),
                );
                continue;
            }
            if !p.required_checks.contains(&c) {
                p.required_checks.push(c);
            }
        }
        let methods = r.strings(tb, "merge_methods").unwrap_or_default();
        let mut ok = BTreeSet::new();
        for (m, v) in methods {
            if !MERGE_METHODS.contains(&m.as_str()) {
                r.d.add(
                    codes::E_VALUE,
                    v.pos,
                    format!(
                        "merge method {m:?} is not one of {}",
                        MERGE_METHODS.join(", ")
                    ),
                );
                continue;
            }
            ok.insert(m);
        }
        p.merge_methods = MERGE_METHODS
            .iter()
            .filter(|m| ok.contains(**m))
            .map(|m| m.to_string())
            .collect();
        out.push(p);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_protections() {
        let (ps, diags) = parse(
            "[[protection]]\npattern = \"main\"\nrequired_approvals = 2\nmerge_methods = [\"squash\", \"merge\", \"squash\"]\nallowed_pushers = \"alice\"\nrequired_checks = [\"ci\", \"ci\"]\nno_force_push = true\n",
        );
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(ps.len(), 1);
        let p = &ps[0];
        assert_eq!(p.required_approvals, 2);
        assert_eq!(p.merge_methods, vec!["merge", "squash"]);
        assert_eq!(p.allowed_pushers, vec!["alice"]);
        assert_eq!(p.required_checks, vec!["ci"]);
        assert!(p.no_force_push && !p.no_delete);
    }

    #[test]
    fn checks_patterns() {
        for ok in [
            "main",
            "release/*",
            "release/**",
            "**",
            "a/**/b",
            "v[0-9]*",
            "x\\*",
            "[^a]",
        ] {
            assert_eq!(check_pattern(ok), None, "{ok}");
        }
        for bad in [
            "", "a//b", "/a", "a/", "[", "a[b", "x\\", "[]", "[a-]", "a\u{7}",
        ] {
            assert!(check_pattern(bad).is_some(), "{bad}");
        }
        assert!(check_pattern(&"a".repeat(513)).is_some());
    }

    #[test]
    fn limits() {
        let mut src = String::new();
        for i in 0..=MAX_PROTECTIONS {
            src.push_str(&format!("[[protection]]\npattern = \"p{i}\"\n"));
        }
        let (_, diags) = parse(&src);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, codes::E_LIMIT);
        let users: Vec<String> = (0..=MAX_PUSHERS).map(|i| format!("\"u{i}\"")).collect();
        let (_, diags) = parse(&format!(
            "[[protection]]\npattern = \"main\"\nallowed_pushers = [{}]\nrequired_approvals = -1\nrequired_checks = [\"\"]\n",
            users.join(",")
        ));
        let codes: Vec<&str> = diags.iter().map(|d| d.code.as_str()).collect();
        assert_eq!(codes, vec!["E3007", "E3009", "E3007"], "{diags:?}");
    }
}
