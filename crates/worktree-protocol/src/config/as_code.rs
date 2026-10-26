//! # Config-as-code validation (`.wt-tree/`)
//!
//! A tree's shared `.wt-tree/` files configure the server when they are
//! pushed to the tree's default branch (ENGINEERING_PLAN §16.3,
//! `specs/dot-wt-tree/ConfigAsCode.md`):
//!
//! - `.wt-tree/config.toml` — client settings (`[auto_snapshot]`,
//!   `[secret_scanning]`, `[hooks]`), validated only;
//! - `.wt-tree/branches.toml` — branch descriptions and owners
//!   ([`super::branches`]);
//! - `.wt-tree/protections.toml` — branch protections
//!   ([`super::protections`]);
//! - `.wt-tree/access/policies.toml` — the tree's deny policies
//!   ([`crate::access::declarative`]).
//!
//! The rules, the diagnostic codes ([`codes`]) and the positions
//! (line:column of the key, array element or `[[table]]` header a problem
//! is about; columns count characters) are the same as the Go server's
//! (`services/server-go/internal/treeconfig`); both run the conformance
//! cases in `testdata/config_as_code/`. The server additionally checks
//! that owners exist and that the pusher may change `access/` (iam:admin).

use std::fmt;
use std::ops::Range;

use serde::Serialize;

use crate::access::declarative;

/// Directory of the tree's shared configuration.
pub const DIR: &str = ".wt-tree";
/// `.wt-tree/config.toml`.
pub const CONFIG_FILE: &str = ".wt-tree/config.toml";
/// `.wt-tree/branches.toml`.
pub const BRANCHES_FILE: &str = ".wt-tree/branches.toml";
/// `.wt-tree/protections.toml`.
pub const PROTECTIONS_FILE: &str = ".wt-tree/protections.toml";
/// `.wt-tree/access/` (with the trailing slash).
pub const ACCESS_DIR: &str = ".wt-tree/access/";
/// `.wt-tree/access/policies.toml`.
pub const POLICIES_FILE: &str = ".wt-tree/access/policies.toml";
/// `.wt-tree/access/roles.toml` (not allowed at tree level).
pub const ROLES_FILE: &str = ".wt-tree/access/roles.toml";

/// Largest config file accepted (bytes).
pub const MAX_FILE_SIZE: usize = 256 << 10;

/// Diagnostic codes, shared with the Go server. Errors are `E30xx`,
/// warnings `W31xx`; access files keep the `E20xx`/`W21xx` codes of the
/// declarative access compiler. Codes marked "server" are only produced by
/// the server.
pub mod codes {
    /// The file is not valid TOML.
    pub const E_SYNTAX: &str = "E3001";
    /// An unknown key (branches/protections) or a value of the wrong type.
    pub const E_STRUCTURE: &str = "E3002";
    /// A required field is missing or empty.
    pub const E_MISSING: &str = "E3003";
    /// An invalid branch name.
    pub const E_BRANCH_NAME: &str = "E3004";
    /// An invalid protection pattern.
    pub const E_PATTERN: &str = "E3005";
    /// Two entries for the same branch / pattern.
    pub const E_DUPLICATE: &str = "E3006";
    /// A value out of range or not allowed.
    pub const E_VALUE: &str = "E3007";
    /// An owner that is not `user:NAME` / `team:SLUG` (server: or does not exist).
    pub const E_OWNER: &str = "E3008";
    /// Too many entries, or a text that is too long.
    pub const E_LIMIT: &str = "E3009";
    /// (server) The pusher may not change this configuration (access/ needs iam:admin).
    pub const E_PERMISSION: &str = "E3010";
    /// (server) The access document was refused by the server.
    pub const E_ACCESS: &str = "E3011";
    /// (server) A protection pattern is managed through the API; the entry is ignored.
    pub const W_MANAGED_BY_API: &str = "W3101";
    /// (server) A declared branch does not exist yet.
    pub const W_NO_SUCH_BRANCH: &str = "W3102";
    /// A `config.toml` section that is not read at tree level.
    pub const W_SECTION: &str = "W3103";
    /// An unknown key in `config.toml` (ignored).
    pub const W_UNKNOWN_KEY: &str = "W3104";
    /// Roles in a tree-level access directory.
    pub const E_TREE_ROLES: &str = "E2013";
}

/// Whether a tree-relative path is one of the config-as-code files.
pub fn is_config_path(path: &str) -> bool {
    matches!(path, CONFIG_FILE | BRANCHES_FILE | PROTECTIONS_FILE) || is_access_path(path)
}

/// Whether a tree-relative path lies below `.wt-tree/access/`.
pub fn is_access_path(path: &str) -> bool {
    path.len() > ACCESS_DIR.len() && path.starts_with(ACCESS_DIR)
}

// ---------------------------------------------------------------------------
// Diagnostics

/// Error or warning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

/// A 1-based line and column (characters); `0:0` = no position.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Pos {
    pub line: usize,
    pub col: usize,
}

impl Pos {
    pub const START: Pos = Pos { line: 1, col: 1 };
}

/// One problem found in a `.wt-tree` file (the JSON shape of the server's
/// `ConfigDiagnostic`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Diagnostic {
    pub code: String,
    pub severity: Severity,
    pub file: String,
    pub line: usize,
    pub col: usize,
    pub message: String,
}

impl Diagnostic {
    pub fn new(code: &str, file: &str, pos: Pos, message: impl Into<String>) -> Self {
        let severity = if code.starts_with('W') {
            Severity::Warning
        } else {
            Severity::Error
        };
        Self {
            code: code.to_string(),
            severity,
            file: file.to_string(),
            line: pos.line,
            col: pos.col,
            message: message.into(),
        }
    }

    /// Whether this is an error (not a warning).
    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }
}

impl fmt::Display for Diagnostic {
    /// `file:line:col: CODE message` (`file: CODE message` without a position).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.file)?;
        if self.line > 0 {
            write!(f, ":{}:{}", self.line, self.col)?;
        }
        write!(f, ": {} {}", self.code, self.message)
    }
}

/// Collects the diagnostics of one file.
pub(crate) struct Diags {
    pub file: String,
    pub list: Vec<Diagnostic>,
}

impl Diags {
    pub fn new(file: &str) -> Self {
        Self {
            file: file.to_string(),
            list: Vec::new(),
        }
    }

    pub fn add(&mut self, code: &str, pos: Pos, message: impl Into<String>) {
        self.list
            .push(Diagnostic::new(code, &self.file, pos, message));
    }
}

/// Sort by file, line, column, code (the server's order).
pub fn sort_diagnostics(list: &mut [Diagnostic]) {
    list.sort_by(|a, b| (&a.file, a.line, a.col, &a.code).cmp(&(&b.file, b.line, b.col, &b.code)));
}

// ---------------------------------------------------------------------------
// A positioned TOML tree (from toml_edit spans)

/// Value kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    String,
    Integer,
    Float,
    Bool,
    Datetime,
    Array,
    Table,
}

impl Kind {
    /// Article + name, as in the server's messages.
    pub fn type_name(self) -> &'static str {
        match self {
            Kind::String => "a string",
            Kind::Integer => "an integer",
            Kind::Float => "a float",
            Kind::Bool => "a boolean",
            Kind::Datetime => "a date-time",
            Kind::Array => "an array",
            Kind::Table => "a table",
        }
    }
}

/// A TOML value with the position of the value and of the key (or table
/// header / array element) that defined it.
#[derive(Debug, Clone)]
pub(crate) struct Val {
    pub kind: Kind,
    pub pos: Pos,
    pub key_pos: Pos,
    pub s: String,
    pub i: i64,
    pub b: bool,
    pub arr: Vec<Val>,
    pub tbl: Tbl,
}

/// A table with keys in definition order.
#[derive(Debug, Clone, Default)]
pub(crate) struct Tbl {
    pub entries: Vec<(String, Val)>,
}

impl Tbl {
    pub fn get(&self, key: &str) -> Option<&Val> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }
}

/// Maps byte offsets to positions.
pub(crate) struct Lines<'a> {
    text: &'a str,
    starts: Vec<usize>,
}

impl<'a> Lines<'a> {
    pub fn new(text: &'a str) -> Self {
        let mut starts = vec![0];
        starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
        Self { text, starts }
    }

    pub fn pos(&self, offset: usize) -> Pos {
        let offset = offset.min(self.text.len());
        let line = self.starts.partition_point(|&s| s <= offset);
        let start = self.starts[line - 1];
        let col = self
            .text
            .get(start..offset)
            .map(|s| s.chars().count())
            .unwrap_or(0)
            + 1;
        Pos { line, col }
    }

    fn span(&self, span: Option<Range<usize>>) -> Pos {
        span.map(|r| self.pos(r.start)).unwrap_or(Pos::START)
    }
}

fn empty(kind: Kind, pos: Pos, key_pos: Pos) -> Val {
    Val {
        kind,
        pos,
        key_pos,
        s: String::new(),
        i: 0,
        b: false,
        arr: Vec::new(),
        tbl: Tbl::default(),
    }
}

fn conv_value(lines: &Lines<'_>, v: &toml_edit::Value, key_pos: Option<Pos>) -> Val {
    let pos = lines.span(v.span());
    let key_pos = key_pos.unwrap_or(pos);
    match v {
        toml_edit::Value::String(s) => Val {
            s: s.value().clone(),
            ..empty(Kind::String, pos, key_pos)
        },
        toml_edit::Value::Integer(i) => Val {
            i: *i.value(),
            ..empty(Kind::Integer, pos, key_pos)
        },
        toml_edit::Value::Float(_) => empty(Kind::Float, pos, key_pos),
        toml_edit::Value::Boolean(b) => Val {
            b: *b.value(),
            ..empty(Kind::Bool, pos, key_pos)
        },
        toml_edit::Value::Datetime(d) => Val {
            s: d.value().to_string(),
            ..empty(Kind::Datetime, pos, key_pos)
        },
        toml_edit::Value::Array(a) => Val {
            arr: a.iter().map(|e| conv_value(lines, e, None)).collect(),
            ..empty(Kind::Array, pos, key_pos)
        },
        toml_edit::Value::InlineTable(t) => {
            let mut tbl = Tbl::default();
            for (k, val) in t.iter() {
                let kp = t.key(k).and_then(|k| k.span()).map(|r| lines.pos(r.start));
                tbl.entries
                    .push((k.to_string(), conv_value(lines, val, kp)));
            }
            Val {
                tbl,
                ..empty(Kind::Table, pos, key_pos)
            }
        }
    }
}

fn conv_table(lines: &Lines<'_>, t: &toml_edit::Table) -> Tbl {
    let mut out = Tbl::default();
    for (k, item) in t.iter() {
        let key_span = t.key(k).and_then(|k| k.span());
        let kp = key_span.clone().map(|r| lines.pos(r.start));
        let val = match item {
            toml_edit::Item::None => continue,
            toml_edit::Item::Value(v) => conv_value(lines, v, kp),
            toml_edit::Item::Table(sub) => {
                // A [header] table is positioned at its header; dotted and
                // implicit tables at their key.
                let pos = if sub.is_dotted() || sub.is_implicit() {
                    kp.unwrap_or(Pos::START)
                } else {
                    lines.span(sub.span().or(key_span))
                };
                Val {
                    tbl: conv_table(lines, sub),
                    ..empty(Kind::Table, pos, pos)
                }
            }
            toml_edit::Item::ArrayOfTables(aot) => {
                let elems: Vec<Val> = aot
                    .iter()
                    .map(|sub| {
                        let pos = lines.span(sub.span());
                        Val {
                            tbl: conv_table(lines, sub),
                            ..empty(Kind::Table, pos, pos)
                        }
                    })
                    .collect();
                let pos = elems.first().map(|e| e.pos).unwrap_or(Pos::START);
                Val {
                    arr: elems,
                    ..empty(Kind::Array, pos, pos)
                }
            }
        };
        out.entries.push((k.to_string(), val));
    }
    out
}

/// Parse `src`; a syntax error is reported with `code`.
pub(crate) fn parse(src: &str, d: &mut Diags, code: &str) -> Option<Tbl> {
    let lines = Lines::new(src);
    match toml_edit::ImDocument::parse(src) {
        Ok(doc) => Some(conv_table(&lines, doc.as_table())),
        Err(e) => {
            let pos = lines.span(e.span());
            d.add(code, pos, e.message().trim().replace('\n', " "));
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Typed accessors (the same shape as the server's `reader`)

pub(crate) struct Reader<'d> {
    pub d: &'d mut Diags,
    pub structure: &'static str,
}

impl Reader<'_> {
    pub fn unknown_keys(&mut self, t: &Tbl, allowed: &[&str], place: &str) {
        for (k, v) in &t.entries {
            if !allowed.contains(&k.as_str()) {
                self.d.add(
                    self.structure,
                    v.key_pos,
                    format!(
                        "unknown key '{k}'{place} (expected one of: {})",
                        allowed.join(", ")
                    ),
                );
            }
        }
    }

    pub fn str<'t>(&mut self, t: &'t Tbl, key: &str) -> Option<(String, &'t Val)> {
        let v = t.get(key)?;
        if v.kind != Kind::String {
            self.d.add(
                self.structure,
                v.key_pos,
                format!("{key} must be a string, found {}", v.kind.type_name()),
            );
            return None;
        }
        Some((v.s.trim().to_string(), v))
    }

    pub fn boolean(&mut self, t: &Tbl, key: &str) -> Option<bool> {
        let v = t.get(key)?;
        if v.kind != Kind::Bool {
            self.d.add(
                self.structure,
                v.key_pos,
                format!("{key} must be true or false, found {}", v.kind.type_name()),
            );
            return None;
        }
        Some(v.b)
    }

    pub fn integer<'t>(&mut self, t: &'t Tbl, key: &str) -> Option<(i64, &'t Val)> {
        let v = t.get(key)?;
        if v.kind != Kind::Integer {
            self.d.add(
                self.structure,
                v.key_pos,
                format!("{key} must be an integer, found {}", v.kind.type_name()),
            );
            return None;
        }
        Some((v.i, v))
    }

    /// An array of strings (a single string is accepted).
    pub fn strings<'t>(&mut self, t: &'t Tbl, key: &str) -> Option<Vec<(String, &'t Val)>> {
        let v = t.get(key)?;
        match v.kind {
            Kind::String => Some(vec![(v.s.trim().to_string(), v)]),
            Kind::Array => {
                let mut out = Vec::new();
                for e in &v.arr {
                    if e.kind != Kind::String {
                        self.d.add(
                            self.structure,
                            e.pos,
                            format!(
                                "{key} entries must be strings, found {}",
                                e.kind.type_name()
                            ),
                        );
                        continue;
                    }
                    out.push((e.s.trim().to_string(), e));
                }
                Some(out)
            }
            _ => {
                self.d.add(
                    self.structure,
                    v.key_pos,
                    format!(
                        "{key} must be an array of strings, found {}",
                        v.kind.type_name()
                    ),
                );
                None
            }
        }
    }

    /// An array of tables (`[[key]]` or `key = [{...}]`).
    pub fn tables<'t>(&mut self, t: &'t Tbl, key: &str) -> Vec<&'t Val> {
        let Some(v) = t.get(key) else {
            return Vec::new();
        };
        if v.kind != Kind::Array {
            self.d.add(
                self.structure,
                v.key_pos,
                format!(
                    "{key} must be an array of tables ([[{key}]]), found {}",
                    v.kind.type_name()
                ),
            );
            return Vec::new();
        }
        let mut out = Vec::new();
        for e in &v.arr {
            if e.kind != Kind::Table {
                self.d.add(
                    self.structure,
                    e.pos,
                    format!("{key} entries must be tables, found {}", e.kind.type_name()),
                );
                continue;
            }
            out.push(e);
        }
        out
    }
}

pub(crate) fn char_len(s: &str) -> usize {
    s.chars().count()
}

// ---------------------------------------------------------------------------
// config.toml

/// Sections read from a tree-level `config.toml`, their keys and kinds.
const CONFIG_SCHEMA: &[(&str, &[(&str, &str)])] = &[
    (
        "auto_snapshot",
        &[
            ("enabled", "bool"),
            ("inactivity_secs", "uint"),
            ("inactivity_timeout_secs", "uint"),
            ("max_changed_files", "uint"),
            ("max_changed_bytes", "size"),
            ("message_format", "string"),
            ("on_branch_switch", "enum:refuse,snapshot,stash"),
        ],
    ),
    ("hooks", &[("enabled", "bool"), ("timeout_secs", "uint")]),
    (
        "secret_scanning",
        &[
            ("enabled", "bool"),
            ("block_on_match", "bool"),
            ("exclude", "strings"),
            ("patterns", "patterns"),
        ],
    ),
];

/// Pre-v0.7 identity keys (moved to `local.toml` by the client; ignored).
const LEGACY_CONFIG_KEYS: &[&str] = &["name", "id"];

/// `50MB`, `1.5 GiB`, `100` (the server's size syntax).
fn is_size(s: &str) -> bool {
    let s = s.trim().to_ascii_lowercase();
    let digits_end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    if digits_end == 0 {
        return false;
    }
    let mut rest = &s[digits_end..];
    if let Some(frac) = rest.strip_prefix('.') {
        let n = frac
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(frac.len());
        if n == 0 {
            return false;
        }
        rest = &frac[n..];
    }
    let unit = rest.trim();
    matches!(
        unit,
        "" | "b"
            | "kb"
            | "kib"
            | "mb"
            | "mib"
            | "gb"
            | "gib"
            | "tb"
            | "tib"
            | "k"
            | "m"
            | "g"
            | "t"
    )
}

/// Validate a tree-level `config.toml`.
pub fn validate_config(src: &str) -> Vec<Diagnostic> {
    let mut d = Diags::new(CONFIG_FILE);
    validate_config_into(src, &mut d);
    d.list
}

fn validate_config_into(src: &str, d: &mut Diags) {
    let Some(t) = parse(src, d, codes::E_SYNTAX) else {
        return;
    };
    let sections: Vec<&str> = CONFIG_SCHEMA.iter().map(|(s, _)| *s).collect();
    for (k, v) in &t.entries {
        let schema = CONFIG_SCHEMA.iter().find(|(s, _)| s == k).map(|(_, f)| *f);
        match schema {
            _ if LEGACY_CONFIG_KEYS.contains(&k.as_str()) && v.kind != Kind::Table => {}
            None if v.kind == Kind::Table => d.add(
                codes::W_SECTION,
                v.key_pos,
                format!(
                    "[{k}] is not read at tree level (allowed: {})",
                    sections.join(", ")
                ),
            ),
            None => d.add(
                codes::W_UNKNOWN_KEY,
                v.key_pos,
                format!("unknown key '{k}' (ignored)"),
            ),
            Some(_) if v.kind != Kind::Table => d.add(
                codes::E_STRUCTURE,
                v.key_pos,
                format!("{k} must be a table ([{k}]), found {}", v.kind.type_name()),
            ),
            Some(fields) => {
                for (key, kv) in &v.tbl.entries {
                    match fields.iter().find(|(f, _)| f == key) {
                        None => d.add(
                            codes::W_UNKNOWN_KEY,
                            kv.key_pos,
                            format!("unknown key '{k}.{key}' (ignored)"),
                        ),
                        Some((_, kind)) => check_config_value(d, &format!("{k}.{key}"), kind, kv),
                    }
                }
            }
        }
    }
}

fn check_config_value(d: &mut Diags, key: &str, kind: &str, v: &Val) {
    let bad = |d: &mut Diags, want: &str| {
        d.add(
            codes::E_STRUCTURE,
            v.key_pos,
            format!("{key} must be {want}, found {}", v.kind.type_name()),
        )
    };
    match kind {
        "bool" => {
            if v.kind != Kind::Bool {
                bad(d, "true or false");
            }
        }
        "uint" => {
            if v.kind != Kind::Integer {
                bad(d, "an integer");
            } else if v.i < 0 {
                d.add(
                    codes::E_VALUE,
                    v.key_pos,
                    format!("{key} must not be negative"),
                );
            }
        }
        "string" => {
            if v.kind != Kind::String {
                bad(d, "a string");
            }
        }
        "size" => match v.kind {
            Kind::Integer if v.i < 0 => d.add(
                codes::E_VALUE,
                v.key_pos,
                format!("{key} must not be negative"),
            ),
            Kind::Integer => {}
            Kind::String if !is_size(&v.s) => d.add(
                codes::E_VALUE,
                v.key_pos,
                format!("{key}: {:?} is not a size (e.g. 50MB)", v.s),
            ),
            Kind::String => {}
            _ => bad(d, "a size (integer or string such as \"50MB\")"),
        },
        "strings" => {
            if v.kind != Kind::Array {
                bad(d, "an array of strings");
                return;
            }
            for e in &v.arr {
                if e.kind != Kind::String {
                    d.add(
                        codes::E_STRUCTURE,
                        e.pos,
                        format!(
                            "{key} entries must be strings, found {}",
                            e.kind.type_name()
                        ),
                    );
                }
            }
        }
        "patterns" => {
            if v.kind != Kind::Array {
                bad(d, "an array of tables");
                return;
            }
            for e in &v.arr {
                if e.kind != Kind::Table {
                    d.add(
                        codes::E_STRUCTURE,
                        e.pos,
                        format!("{key} entries must be tables, found {}", e.kind.type_name()),
                    );
                    continue;
                }
                let mut r = Reader {
                    d,
                    structure: codes::E_STRUCTURE,
                };
                r.unknown_keys(&e.tbl, &["name", "regex"], &format!(" in {key}"));
                for f in ["name", "regex"] {
                    let present = e.tbl.get(f).is_some();
                    let got = r.str(&e.tbl, f);
                    if !present || got.is_some_and(|(s, _)| s.is_empty()) {
                        r.d.add(
                            codes::E_MISSING,
                            e.pos,
                            format!("{key} entries need a non-empty {f}"),
                        );
                    }
                }
            }
        }
        _ => {
            if let Some(allowed) = kind.strip_prefix("enum:") {
                let allowed: Vec<&str> = allowed.split(',').collect();
                if v.kind != Kind::String {
                    bad(d, "a string");
                } else if !allowed.contains(&v.s.as_str()) {
                    d.add(
                        codes::E_VALUE,
                        v.key_pos,
                        format!("{key} must be one of {}, got {:?}", allowed.join(", "), v.s),
                    );
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Access files

/// Position of an access diagnostic location such as
/// `policy[0] (name).statement[1].actions[0]`: the `[[policy]]` or
/// `[[policy.statement]]` header it refers to.
fn locate_access(t: &Tbl, location: &str) -> Pos {
    let index = |s: &str, prefix: &str| -> Option<usize> {
        let rest = s.strip_prefix(prefix)?;
        rest[..rest.find(']')?].parse().ok()
    };
    let Some(i) = index(location, "policy[") else {
        return Pos::START;
    };
    let Some(policy) = t
        .get("policy")
        .and_then(|v| v.arr.get(i))
        .filter(|v| v.kind == Kind::Table)
    else {
        return Pos::START;
    };
    if let Some(at) = location.find(".statement[") {
        if let Some(j) = index(&location[at + 1..], "statement[") {
            if let Some(st) = policy.tbl.get("statement").and_then(|v| v.arr.get(j)) {
                return st.pos;
            }
        }
    }
    policy.pos
}

fn validate_policies(src: &str, out: &mut Vec<Diagnostic>) {
    let mut d = Diags::new(POLICIES_FILE);
    let Some(tree) = parse(src, &mut d, declarative::codes::E_SYNTAX) else {
        out.extend(d.list);
        return;
    };
    let map = |diag: declarative::Diagnostic| {
        let message = if diag.location.is_empty() {
            diag.message
        } else {
            format!("{}: {}", diag.location, diag.message)
        };
        Diagnostic::new(
            diag.code,
            POLICIES_FILE,
            locate_access(&tree, &diag.location),
            message,
        )
    };
    match declarative::parse_policies(src) {
        Err(errs) => out.extend(errs.into_iter().map(map)),
        Ok(parsed) => {
            out.extend(parsed.warnings.into_iter().map(map));
            let scope = declarative::Scope::Tree {
                tenant: None,
                tree: None,
            };
            out.extend(
                declarative::validate(&[], &parsed.value, &scope)
                    .into_iter()
                    .map(map),
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Everything

/// Validate the config-as-code files among `files` (tree-relative path →
/// content; other paths are ignored). Diagnostics are sorted.
pub fn validate_files<'a, I>(files: I) -> Vec<Diagnostic>
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    let mut out = Vec::new();
    for (path, src) in files {
        if !is_config_path(path) {
            continue;
        }
        if src.len() > MAX_FILE_SIZE {
            out.push(Diagnostic::new(
                codes::E_LIMIT,
                path,
                Pos::START,
                format!("the file is larger than {MAX_FILE_SIZE} bytes"),
            ));
            continue;
        }
        match path {
            CONFIG_FILE => out.extend(validate_config(src)),
            BRANCHES_FILE => out.extend(super::branches::parse(src).1),
            PROTECTIONS_FILE => out.extend(super::protections::parse(src).1),
            POLICIES_FILE => validate_policies(src, &mut out),
            ROLES_FILE => out.push(Diagnostic::new(
                codes::E_TREE_ROLES,
                path,
                Pos::START,
                "tree-level access files cannot define roles; define them in .wt/access/roles.toml",
            )),
            _ => {}
        }
    }
    sort_diagnostics(&mut out);
    out
}

/// Whether `list` holds at least one error.
pub fn has_errors(list: &[Diagnostic]) -> bool {
    list.iter().any(Diagnostic::is_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn diag_lines(list: &[Diagnostic]) -> Vec<String> {
        list.iter()
            .map(|d| format!("{}:{} {}", d.line, d.col, d.code))
            .collect()
    }

    fn matches_expected(got: &[String], want: &[String]) -> bool {
        got.len() == want.len()
            && got.iter().zip(want).all(|(g, w)| {
                let (gp, gc) = g.split_once(' ').unwrap();
                let (wp, wc) = w.split_once(' ').unwrap();
                gc == wc
                    && gp
                        .split(':')
                        .zip(wp.split(':'))
                        .all(|(g, w)| w == "?" || g == w)
            })
    }

    /// The cases shared with the Go server (see testdata/config_as_code/README.md).
    #[test]
    fn conformance_cases() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/config_as_code");
        let mut n = 0;
        let mut entries: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let name = entry.file_name().to_string_lossy().to_string();
            let Some(stem) = name.strip_suffix(".toml") else {
                continue;
            };
            let (case, kind) = stem.split_once('.').unwrap();
            let src = std::fs::read_to_string(entry.path()).unwrap();
            let want: Vec<String> = std::fs::read_to_string(dir.join(format!("{case}.expected")))
                .unwrap()
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(|l| l.trim().to_string())
                .collect();
            let path = format!("{DIR}/{kind}.toml");
            let got = validate_files([(path.as_str(), src.as_str())]);
            assert!(got.iter().all(|d| d.file == path), "{case}: {got:?}");
            let lines = diag_lines(&got);
            assert!(
                matches_expected(&lines, &want),
                "{case}: got {lines:?}, want {want:?}\n{got:#?}"
            );
            n += 1;
        }
        assert!(n >= 5, "only {n} cases");
    }

    #[test]
    fn positions_count_characters() {
        let lines = Lines::new("ab\né = [1, \"ü\", 2]\r\nx");
        assert_eq!(lines.pos(0), Pos { line: 1, col: 1 });
        assert_eq!(lines.pos(3), Pos { line: 2, col: 1 });
        let src = "\"é\" = [1, \"ü\", 2]\n[[t]]\n  k = 1\n";
        let mut d = Diags::new("f");
        let t = parse(src, &mut d, codes::E_SYNTAX).unwrap();
        let arr = &t.get("é").unwrap().arr;
        assert_eq!(arr[2].pos, Pos { line: 1, col: 16 });
        assert_eq!(t.get("é").unwrap().key_pos, Pos::START);
        let tables = &t.get("t").unwrap();
        assert_eq!(tables.key_pos, Pos { line: 2, col: 1 });
        assert_eq!(
            tables.arr[0].tbl.get("k").unwrap().key_pos,
            Pos { line: 3, col: 3 }
        );
    }

    #[test]
    fn config_sizes_and_display() {
        for ok in ["50MB", "1.5 GiB", "100", " 7k "] {
            assert!(is_size(ok), "{ok}");
        }
        for bad in ["", "MB", "1.MB", "5 parsecs", "-1"] {
            assert!(!is_size(bad), "{bad}");
        }
        let d = Diagnostic::new(codes::E_OWNER, BRANCHES_FILE, Pos { line: 3, col: 7 }, "x");
        assert_eq!(d.to_string(), ".wt-tree/branches.toml:3:7: E3008 x");
        let w = Diagnostic::new(codes::W_SECTION, CONFIG_FILE, Pos::default(), "w");
        assert_eq!(w.to_string(), ".wt-tree/config.toml: W3103 w");
        assert!(!w.is_error());
        let json = serde_json::to_value(&d).unwrap();
        assert_eq!(json["severity"], "error");
        assert_eq!(json["line"], 3);
    }

    #[test]
    fn access_files_are_located() {
        let src = "[[policy]]\nname = \"p\"\n[[policy.statement]]\neffect = \"allow\"\nprincipals = [\"*\"]\nactions = [\"sync:push\"]\nresources = [\"*\"]\n";
        let got = validate_files([(POLICIES_FILE, src)]);
        let codes: Vec<&str> = got.iter().map(|d| d.code.as_str()).collect();
        assert!(codes.contains(&"E2008"), "{got:?}"); // tree-level allow
        assert!(codes.contains(&"W2101"), "{got:?}"); // alias
        let allow = got.iter().find(|d| d.code == "E2008").unwrap();
        assert_eq!((allow.line, allow.col), (3, 1), "{allow:?}");
        let syntax = validate_files([(POLICIES_FILE, "[[policy]\n")]);
        assert_eq!(syntax.len(), 1);
        assert_eq!(syntax[0].code, "E2001");
        assert_eq!(syntax[0].line, 1);
        let roles = validate_files([(ROLES_FILE, "")]);
        assert_eq!(roles[0].code, codes::E_TREE_ROLES);
        let ignored = validate_files([(".wt-tree/ignore", "["), ("README.md", "[")]);
        assert!(ignored.is_empty());
        let big = "#".repeat(MAX_FILE_SIZE + 1);
        assert_eq!(
            validate_files([(CONFIG_FILE, big.as_str())])[0].code,
            codes::E_LIMIT
        );
        assert!(is_config_path(".wt-tree/access/x/y.toml"));
        assert!(!is_config_path(ACCESS_DIR));
        assert!(!is_config_path(".wt-tree/hooks/x"));
    }
}
