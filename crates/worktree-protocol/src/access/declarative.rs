//! # Declarative access configuration
//!
//! Compiles the TOML access files of a worktree into the canonical JSON
//! document the server applies (`SyncAccessConfig`, ENGINEERING_PLAN §13.7):
//!
//! - `.wt/access/roles.toml` and `.wt/access/policies.toml` — tenant scope;
//! - `<tree>/.wt-tree/access/policies.toml` — tree scope, **deny statements
//!   only**, every resource inside the tree.
//!
//! ```toml
//! [[role]]
//! name = "release-manager"
//! description = "Cut releases"
//! permissions = ["tree:read", "release:*", "mr:merge"]
//!
//! [[policy]]
//! name = "protect-main"
//! description = "Only maintainers push to main"
//! enabled = true
//! [[policy.statement]]
//! sid = "no-direct-main"
//! effect = "deny"            # allow | deny
//! principals = ["role:developer", "team:contractors", "user:bob", "tenant:partner"]
//! actions = ["branch:push"]
//! resources = ["wt:branch/acme/web/main"]
//! conditions = { mfa = true }
//! ```
//!
//! Conditions: `mfa` (alias `require_mfa`), `ip_ranges` (CIDRs),
//! `not_before` / `not_after` (RFC 3339, TOML date-times accepted) and
//! `time_window = { days = ["mon"], start = "09:00", end = "18:00",
//! timezone = "Europe/Berlin" }` and (v0.8) ABAC attribute conditions,
//! `[[policy.statement.conditions.attributes]]` tables with `key`
//! (`principal.<k>`, `tree.<k>`, `tenant.<k>`), `op` and `value` / `values`
//! (see [`super::attributes`]).
//!
//! The canonical JSON is `{"roles":[CustomRole],"policies":[Policy]}` with
//! the Go server's JSON field names (`internal/domain/iam.go`). Validation
//! mirrors the server's (`authz.ValidateStatements`,
//! `authz.ValidatePermissions`); references to users, teams and so on are
//! only checked by the server. Problems are reported as [`Diagnostic`]s
//! with stable codes (see [`codes`]).
//!
//! Permission aliases are accepted and translated, with a warning
//! ([`codes::W_ALIAS`]): `sync:push`→`branch:push`,
//! `snapshot:create`→`tree:write`, `branch:merge`→`mr:merge`,
//! `tree:delete`→`tree:admin`.

use std::collections::{BTreeSet, HashMap};
use std::fmt;

use serde::{Deserialize, Serialize};

use super::attributes::{self, AttributeCondition};

/// Directory of the tenant-scope access files, relative to the worktree root.
pub const TENANT_ACCESS_DIR: &str = ".wt/access";
/// Directory of the tree-scope access file, relative to a tree's directory.
pub const TREE_ACCESS_DIR: &str = ".wt-tree/access";
/// Roles file name.
pub const ROLES_FILE: &str = "roles.toml";
/// Policies file name.
pub const POLICIES_FILE: &str = "policies.toml";

/// Diagnostic codes. Errors are `E20xx`, warnings `W21xx`; the numbers are
/// stable and documented in the access spec.
pub mod codes {
    /// The file is not valid TOML.
    pub const E_SYNTAX: &str = "E2001";
    /// An unknown key, or a value of the wrong type.
    pub const E_STRUCTURE: &str = "E2002";
    /// A required field is missing or empty.
    pub const E_MISSING: &str = "E2003";
    /// An action or permission is not in the action catalog.
    pub const E_UNKNOWN_ACTION: &str = "E2004";
    /// A principal is malformed.
    pub const E_BAD_PRINCIPAL: &str = "E2005";
    /// A resource is malformed.
    pub const E_BAD_RESOURCE: &str = "E2006";
    /// Two roles, policies or statement ids share a name.
    pub const E_DUPLICATE: &str = "E2007";
    /// An allow statement in a tree-level document.
    pub const E_TREE_ALLOW: &str = "E2008";
    /// A role or policy name is invalid (syntax, built-in role name).
    pub const E_BAD_NAME: &str = "E2009";
    /// A statement effect other than allow / deny.
    pub const E_BAD_EFFECT: &str = "E2010";
    /// A condition is invalid (CIDR, time, day, window, time zone).
    pub const E_BAD_CONDITION: &str = "E2011";
    /// A tree-level resource that lies outside the tree.
    pub const E_TREE_RESOURCE: &str = "E2012";
    /// Roles defined in a tree-level document.
    pub const E_TREE_ROLES: &str = "E2013";
    /// A permission a role may not grant (`*`, `server:*`).
    pub const E_ROLE_PERMISSION: &str = "E2014";
    /// A limit was exceeded (too many roles, policies, statements...).
    pub const E_LIMIT: &str = "E2015";
    /// A permission alias was translated.
    pub const W_ALIAS: &str = "W2101";
    /// A policy without statements (it has no effect).
    pub const W_EMPTY_POLICY: &str = "W2102";
}

/// The server's action catalog (`authz.Actions()`), in display order.
pub const ACTIONS: &[&str] = &[
    "tenant:read",
    "tenant:admin",
    "tree:read",
    "tree:write",
    "tree:create",
    "tree:admin",
    "tree:fork",
    "branch:create",
    "branch:delete",
    "branch:push",
    "branch:force_push",
    "branch:bypass_protection",
    "mr:create",
    "mr:approve",
    "mr:merge",
    "release:create",
    "release:delete",
    "ci:report",
    "iam:read",
    "iam:admin",
    "server:admin",
];

/// Built-in roles (custom roles may not reuse these names).
pub const BUILTIN_ROLES: &[&str] = &["owner", "admin", "maintainer", "developer", "viewer"];

/// Permission aliases accepted in TOML: (alias, canonical action).
pub const ALIASES: &[(&str, &str)] = &[
    ("sync:push", "branch:push"),
    ("snapshot:create", "tree:write"),
    ("branch:merge", "mr:merge"),
    ("tree:delete", "tree:admin"),
];

const MAX_ROLES: usize = 200;
const MAX_POLICIES: usize = 200;
const MAX_STATEMENTS: usize = 100;
const MAX_PERMISSIONS: usize = 128;
const MAX_SID: usize = 128;

// ---------------------------------------------------------------------------
// Canonical document (Go JSON field names)

/// A custom role (`domain.CustomRole`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Role {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub permissions: Vec<String>,
}

/// A policy (`domain.Policy`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Policy {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "enabled_default")]
    pub enabled: bool,
    #[serde(default)]
    pub statements: Vec<Statement>,
}

fn enabled_default() -> bool {
    true
}

/// A policy statement (`domain.PolicyStatement`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Statement {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub sid: String,
    pub effect: String,
    #[serde(default)]
    pub actions: Vec<String>,
    #[serde(default)]
    pub resources: Vec<String>,
    #[serde(default)]
    pub principals: Vec<String>,
    #[serde(default)]
    pub conditions: Conditions,
}

/// Statement conditions (`domain.PolicyConditions`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conditions {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ip_ranges: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub require_mfa: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_before: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_after: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_window: Option<TimeWindow>,
    /// v0.8 ABAC attribute conditions (all must hold).
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "null_as_empty_vec"
    )]
    pub attributes: Vec<AttributeCondition>,
}

impl Conditions {
    /// Whether no condition is set.
    pub fn is_empty(&self) -> bool {
        self == &Conditions::default()
    }
}

/// A weekly time window (`domain.TimeWindow`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeWindow {
    #[serde(default, deserialize_with = "null_as_empty")]
    pub days: Vec<String>,
    pub start: String,
    pub end: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub timezone: String,
}

fn null_as_empty<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    Ok(Option::<Vec<String>>::deserialize(d)?.unwrap_or_default())
}

/// The canonical access document sent to the server.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Document {
    #[serde(default, deserialize_with = "null_as_empty_vec")]
    pub roles: Vec<Role>,
    #[serde(default, deserialize_with = "null_as_empty_vec")]
    pub policies: Vec<Policy>,
}

fn null_as_empty_vec<'de, D, T>(d: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Option::<Vec<T>>::deserialize(d)?.unwrap_or_default())
}

impl Document {
    /// Canonical JSON (compact, field order fixed by the structs).
    pub fn to_json(&self) -> String {
        // Serializing plain strings, bools and vectors cannot fail.
        serde_json::to_string(self).unwrap_or_else(|_| String::from("{}"))
    }

    /// Parse a canonical JSON document (as returned by `GetAccessConfig`).
    /// Server-only fields (ids, versions, timestamps) are ignored.
    pub fn from_json(json: &[u8]) -> Result<Self, String> {
        serde_json::from_slice(json).map_err(|e| format!("invalid access document: {e}"))
    }
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

/// One problem found in an access file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Diagnostic {
    /// Stable code, e.g. `E2004` ([`codes`]).
    pub code: &'static str,
    pub severity: Severity,
    /// File name (`roles.toml`, `policies.toml`), when known.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub file: String,
    /// Location inside the file, e.g. `policy[0] (protect-main).statement[1].actions[0]`.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub location: String,
    pub message: String,
}

impl Diagnostic {
    fn new(code: &'static str, location: impl Into<String>, message: impl Into<String>) -> Self {
        let severity = if code.starts_with('W') {
            Severity::Warning
        } else {
            Severity::Error
        };
        Self {
            code,
            severity,
            file: String::new(),
            location: location.into(),
            message: message.into(),
        }
    }

    /// Whether this is an error (not a warning).
    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.code)?;
        if !self.file.is_empty() {
            write!(f, " {}", self.file)?;
        }
        if !self.location.is_empty() {
            write!(f, " {}", self.location)?;
        }
        write!(f, ": {}", self.message)
    }
}

/// A parsed value plus the warnings found while parsing it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed<T> {
    pub value: T,
    pub warnings: Vec<Diagnostic>,
}

/// Where a document applies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// `.wt/access/` of a tenant.
    Tenant,
    /// `<tree>/.wt-tree/access/` of one tree. With the tenant slug and the
    /// tree's server name known, resources are checked to lie inside it.
    Tree {
        tenant: Option<String>,
        tree: Option<String>,
    },
}

/// Result of [`compile`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Compiled {
    pub document: Document,
    /// Canonical JSON of `document`.
    pub json: String,
    pub warnings: Vec<Diagnostic>,
}

// ---------------------------------------------------------------------------
// Parsing

/// Collects diagnostics while walking a TOML table.
#[derive(Default)]
struct Sink {
    errors: Vec<Diagnostic>,
    warnings: Vec<Diagnostic>,
}

impl Sink {
    fn push(&mut self, d: Diagnostic) {
        if d.is_error() {
            self.errors.push(d);
        } else {
            self.warnings.push(d);
        }
    }

    fn err(&mut self, code: &'static str, at: &str, msg: impl Into<String>) {
        self.push(Diagnostic::new(code, at, msg));
    }

    fn finish<T>(self, value: T) -> Result<Parsed<T>, Vec<Diagnostic>> {
        if self.errors.is_empty() {
            Ok(Parsed {
                value,
                warnings: self.warnings,
            })
        } else {
            Err(self.errors)
        }
    }
}

fn type_name(v: &toml::Value) -> &'static str {
    match v {
        toml::Value::String(_) => "a string",
        toml::Value::Integer(_) => "an integer",
        toml::Value::Float(_) => "a float",
        toml::Value::Boolean(_) => "a boolean",
        toml::Value::Datetime(_) => "a date-time",
        toml::Value::Array(_) => "an array",
        toml::Value::Table(_) => "a table",
    }
}

fn parse_table(src: &str, sink: &mut Sink) -> Option<toml::Table> {
    match src.parse::<toml::Table>() {
        Ok(t) => Some(t),
        Err(e) => {
            sink.err(codes::E_SYNTAX, "", e.to_string().trim().replace('\n', " "));
            None
        }
    }
}

/// Read the array of tables `key` (e.g. `[[role]]`) from `table`.
fn table_array<'a>(
    table: &'a toml::Table,
    key: &str,
    at: &str,
    sink: &mut Sink,
) -> Vec<&'a toml::Table> {
    match table.get(key) {
        None => Vec::new(),
        Some(toml::Value::Array(items)) => {
            let mut out = Vec::new();
            for (i, item) in items.iter().enumerate() {
                match item {
                    toml::Value::Table(t) => out.push(t),
                    other => sink.err(
                        codes::E_STRUCTURE,
                        &format!("{at}{key}[{i}]"),
                        format!("expected a table, found {}", type_name(other)),
                    ),
                }
            }
            out
        }
        Some(other) => {
            sink.err(
                codes::E_STRUCTURE,
                &format!("{at}{key}"),
                format!(
                    "expected an array of tables ([[{key}]]), found {}",
                    type_name(other)
                ),
            );
            Vec::new()
        }
    }
}

fn unknown_keys(table: &toml::Table, allowed: &[&str], at: &str, sink: &mut Sink) {
    for key in table.keys() {
        if !allowed.contains(&key.as_str()) {
            sink.err(
                codes::E_STRUCTURE,
                &join(at, key),
                format!(
                    "unknown key '{key}' (expected one of: {})",
                    allowed.join(", ")
                ),
            );
        }
    }
}

fn join(at: &str, key: &str) -> String {
    if at.is_empty() {
        key.to_string()
    } else {
        format!("{at}.{key}")
    }
}

fn get_string(table: &toml::Table, key: &str, at: &str, sink: &mut Sink) -> Option<String> {
    match table.get(key) {
        None => None,
        Some(toml::Value::String(s)) => Some(s.trim().to_string()),
        Some(other) => {
            sink.err(
                codes::E_STRUCTURE,
                &join(at, key),
                format!("expected a string, found {}", type_name(other)),
            );
            None
        }
    }
}

fn get_bool(table: &toml::Table, key: &str, at: &str, sink: &mut Sink) -> Option<bool> {
    match table.get(key) {
        None => None,
        Some(toml::Value::Boolean(b)) => Some(*b),
        Some(other) => {
            sink.err(
                codes::E_STRUCTURE,
                &join(at, key),
                format!("expected true or false, found {}", type_name(other)),
            );
            None
        }
    }
}

fn get_strings(table: &toml::Table, key: &str, at: &str, sink: &mut Sink) -> Option<Vec<String>> {
    match table.get(key) {
        None => None,
        // A single string is accepted where a list is expected.
        Some(toml::Value::String(s)) => Some(vec![s.trim().to_string()]),
        Some(toml::Value::Array(items)) => {
            let mut out = Vec::with_capacity(items.len());
            for (i, item) in items.iter().enumerate() {
                match item {
                    toml::Value::String(s) => out.push(s.trim().to_string()),
                    other => sink.err(
                        codes::E_STRUCTURE,
                        &format!("{}[{i}]", join(at, key)),
                        format!("expected a string, found {}", type_name(other)),
                    ),
                }
            }
            Some(out)
        }
        Some(other) => {
            sink.err(
                codes::E_STRUCTURE,
                &join(at, key),
                format!("expected an array of strings, found {}", type_name(other)),
            );
            None
        }
    }
}

/// Translate an alias; returns the canonical action.
fn translate(action: &str, at: &str, sink: &mut Sink) -> String {
    match ALIASES.iter().find(|(alias, _)| *alias == action) {
        Some((alias, canonical)) => {
            sink.push(Diagnostic::new(
                codes::W_ALIAS,
                at,
                format!("'{alias}' is an alias; translated to '{canonical}'"),
            ));
            (*canonical).to_string()
        }
        None => action.to_string(),
    }
}

/// Parse `roles.toml`: `[[role]]` tables with `name`, `description` and
/// `permissions`. Structural and syntax errors fail the parse; semantic
/// checks happen in [`validate`] / [`compile`].
pub fn parse_roles(src: &str) -> Result<Parsed<Vec<Role>>, Vec<Diagnostic>> {
    let mut sink = Sink::default();
    let mut roles = Vec::new();
    if let Some(table) = parse_table(src, &mut sink) {
        unknown_keys(&table, &["role"], "", &mut sink);
        for (i, t) in table_array(&table, "role", "", &mut sink)
            .into_iter()
            .enumerate()
        {
            let at = format!("role[{i}]");
            unknown_keys(t, &["name", "description", "permissions"], &at, &mut sink);
            let name = get_string(t, "name", &at, &mut sink).unwrap_or_default();
            let at = label(&at, &name);
            let permissions = get_strings(t, "permissions", &at, &mut sink)
                .unwrap_or_default()
                .into_iter()
                .enumerate()
                .map(|(j, p)| translate(&p, &format!("{at}.permissions[{j}]"), &mut sink))
                .collect();
            roles.push(Role {
                name,
                description: get_string(t, "description", &at, &mut sink).unwrap_or_default(),
                permissions,
            });
        }
    }
    sink.finish(roles)
}

fn label(at: &str, name: &str) -> String {
    if name.is_empty() {
        at.to_string()
    } else {
        format!("{at} ({name})")
    }
}

/// Parse `policies.toml`: `[[policy]]` tables with `name`, `description`,
/// `enabled` (default true) and `[[policy.statement]]` entries.
pub fn parse_policies(src: &str) -> Result<Parsed<Vec<Policy>>, Vec<Diagnostic>> {
    let mut sink = Sink::default();
    let mut policies = Vec::new();
    if let Some(table) = parse_table(src, &mut sink) {
        unknown_keys(&table, &["policy"], "", &mut sink);
        for (i, t) in table_array(&table, "policy", "", &mut sink)
            .into_iter()
            .enumerate()
        {
            let at = format!("policy[{i}]");
            unknown_keys(
                t,
                &["name", "description", "enabled", "statement"],
                &at,
                &mut sink,
            );
            let name = get_string(t, "name", &at, &mut sink).unwrap_or_default();
            let at = label(&at, &name);
            let statements = table_array(t, "statement", &format!("{at}."), &mut sink)
                .into_iter()
                .enumerate()
                .map(|(j, st)| parse_statement(st, &format!("{at}.statement[{j}]"), &mut sink))
                .collect();
            policies.push(Policy {
                name,
                description: get_string(t, "description", &at, &mut sink).unwrap_or_default(),
                enabled: get_bool(t, "enabled", &at, &mut sink).unwrap_or(true),
                statements,
            });
        }
    }
    sink.finish(policies)
}

fn parse_statement(t: &toml::Table, at: &str, sink: &mut Sink) -> Statement {
    unknown_keys(
        t,
        &[
            "sid",
            "effect",
            "principals",
            "actions",
            "resources",
            "conditions",
        ],
        at,
        sink,
    );
    let actions = get_strings(t, "actions", at, sink)
        .unwrap_or_default()
        .into_iter()
        .enumerate()
        .map(|(k, a)| translate(&a, &format!("{at}.actions[{k}]"), sink))
        .collect();
    let conditions = match t.get("conditions") {
        None => Conditions::default(),
        Some(toml::Value::Table(c)) => parse_conditions(c, &join(at, "conditions"), sink),
        Some(other) => {
            sink.err(
                codes::E_STRUCTURE,
                &join(at, "conditions"),
                format!("expected a table, found {}", type_name(other)),
            );
            Conditions::default()
        }
    };
    Statement {
        sid: get_string(t, "sid", at, sink).unwrap_or_default(),
        effect: get_string(t, "effect", at, sink)
            .unwrap_or_default()
            .to_ascii_lowercase(),
        actions,
        resources: get_strings(t, "resources", at, sink).unwrap_or_default(),
        principals: get_strings(t, "principals", at, sink).unwrap_or_default(),
        conditions,
    }
}

fn parse_conditions(t: &toml::Table, at: &str, sink: &mut Sink) -> Conditions {
    unknown_keys(
        t,
        &[
            "mfa",
            "require_mfa",
            "ip_ranges",
            "not_before",
            "not_after",
            "time_window",
            "attributes",
        ],
        at,
        sink,
    );
    let mfa = get_bool(t, "mfa", at, sink).unwrap_or(false)
        || get_bool(t, "require_mfa", at, sink).unwrap_or(false);
    let time = |key: &str, sink: &mut Sink| -> Option<String> {
        match t.get(key) {
            None => None,
            Some(toml::Value::String(s)) => Some(s.trim().to_string()),
            Some(toml::Value::Datetime(d)) => Some(d.to_string()),
            Some(other) => {
                sink.err(
                    codes::E_STRUCTURE,
                    &join(at, key),
                    format!("expected an RFC 3339 date-time, found {}", type_name(other)),
                );
                None
            }
        }
    };
    let time_window = match t.get("time_window") {
        None => None,
        Some(toml::Value::Table(w)) => {
            let wat = join(at, "time_window");
            unknown_keys(w, &["days", "start", "end", "timezone"], &wat, sink);
            Some(TimeWindow {
                days: get_strings(w, "days", &wat, sink).unwrap_or_default(),
                start: get_string(w, "start", &wat, sink).unwrap_or_default(),
                end: get_string(w, "end", &wat, sink).unwrap_or_default(),
                timezone: get_string(w, "timezone", &wat, sink).unwrap_or_default(),
            })
        }
        Some(other) => {
            sink.err(
                codes::E_STRUCTURE,
                &join(at, "time_window"),
                format!("expected a table, found {}", type_name(other)),
            );
            None
        }
    };
    Conditions {
        ip_ranges: get_strings(t, "ip_ranges", at, sink).unwrap_or_default(),
        require_mfa: mfa,
        not_before: time("not_before", sink),
        not_after: time("not_after", sink),
        time_window,
        attributes: parse_attribute_conditions(t, at, sink),
    }
}

/// `attributes = [{ key, op, value | values }]` (v0.8 ABAC): structure
/// only; the rules are checked by [`check_conditions`].
fn parse_attribute_conditions(
    t: &toml::Table,
    at: &str,
    sink: &mut Sink,
) -> Vec<AttributeCondition> {
    let aat = join(at, "");
    table_array(t, "attributes", &aat, sink)
        .into_iter()
        .enumerate()
        .map(|(i, c)| {
            let cat = format!("{aat}attributes[{i}]");
            unknown_keys(c, &["key", "op", "value", "values"], &cat, sink);
            let mut values = get_strings(c, "values", &cat, sink).unwrap_or_default();
            if let Some(v) = get_string(c, "value", &cat, sink) {
                if c.contains_key("values") {
                    sink.err(
                        codes::E_STRUCTURE,
                        &join(&cat, "value"),
                        "use either value or values, not both",
                    );
                }
                values.insert(0, v);
            }
            AttributeCondition {
                key: get_string(c, "key", &cat, sink).unwrap_or_default(),
                op: get_string(c, "op", &cat, sink)
                    .unwrap_or_default()
                    .to_ascii_lowercase(),
                values,
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Validation

fn is_action(a: &str) -> bool {
    ACTIONS.contains(&a)
}

fn area_of(a: &str) -> &str {
    a.split_once(':').map(|(area, _)| area).unwrap_or(a)
}

fn is_area(area: &str) -> bool {
    ACTIONS.iter().any(|a| area_of(a) == area)
}

/// Check an action pattern of a statement: a catalog action, `<area>:*` or `*`.
fn check_action(a: &str) -> Result<(), String> {
    if a.is_empty() {
        return Err("action is empty".into());
    }
    if a == "*" {
        return Ok(());
    }
    if let Some(area) = a.strip_suffix(":*") {
        return if is_area(area) {
            Ok(())
        } else {
            Err(format!("unknown action area \"{area}\""))
        };
    }
    if is_action(a) {
        Ok(())
    } else {
        Err(format!(
            "unknown action \"{a}\" (see `wt iam actions`; aliases: {})",
            ALIASES
                .iter()
                .map(|(a, c)| format!("{a}→{c}"))
                .collect::<Vec<_>>()
                .join(", ")
        ))
    }
}

/// Check a statement principal.
pub fn check_principal(p: &str) -> Result<(), String> {
    match p {
        "*" | "authenticated" | "anonymous" => return Ok(()),
        "" => return Err("principal is empty".into()),
        _ => {}
    }
    for prefix in ["user:", "team:", "role:", "sa:", "tenant:"] {
        if let Some(name) = p.strip_prefix(prefix) {
            if name.is_empty() {
                return Err(format!("principal \"{p}\" has no name after \"{prefix}\""));
            }
            if name.chars().any(char::is_whitespace) {
                return Err(format!("principal \"{p}\" contains whitespace"));
            }
            if prefix == "sa:" {
                if let Some((t, n)) = name.split_once('/') {
                    if t.is_empty() || n.is_empty() || n.contains('/') {
                        return Err(format!(
                            "principal \"{p}\" must be sa:<name> or sa:<tenant>/<name>"
                        ));
                    }
                }
            } else if name.contains('/') {
                return Err(format!("principal \"{p}\" must not contain '/'"));
            }
            return Ok(());
        }
    }
    Err(format!(
        "unknown principal \"{p}\" (want user:, team:, role:, sa:, tenant:, authenticated, anonymous or *)"
    ))
}

/// Check a statement resource (same rules as the server).
pub fn check_resource(r: &str) -> Result<(), String> {
    if r == "*" {
        return Ok(());
    }
    let rest = r
        .strip_prefix("wt:")
        .ok_or_else(|| format!("resource \"{r}\" must be \"*\" or start with \"wt:\""))?;
    let (kind, body) = rest
        .split_once('/')
        .filter(|(_, b)| !b.is_empty())
        .ok_or_else(|| format!("resource \"{r}\" has no path after the kind"))?;
    let (lo, hi): (usize, Option<usize>) = match kind {
        "tenant" => (1, Some(1)),
        "tree" => (2, Some(2)),
        "branch" | "path" => (3, None),
        other => {
            return Err(format!(
                "resource \"{r}\" has unknown kind \"{other}\" (want tenant, tree, branch or path)"
            ))
        }
    };
    let mut fixed = 0;
    let mut globstar = false;
    for seg in body.split('/') {
        if seg.is_empty() {
            return Err(format!("resource \"{r}\" has an empty segment"));
        }
        if seg == "**" {
            globstar = true;
        } else if seg.contains("**") {
            return Err(format!("resource \"{r}\": \"**\" must be a whole segment"));
        } else {
            fixed += 1;
        }
    }
    if let Some(hi) = hi {
        if fixed > hi {
            return Err(format!(
                "resource \"{r}\": a {kind} resource has at most {hi} segment(s)"
            ));
        }
    }
    if !globstar && fixed < lo {
        return Err(format!(
            "resource \"{r}\": a {kind} resource needs at least {lo} segment(s)"
        ));
    }
    Ok(())
}

fn valid_role_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    (2..=40).contains(&bytes.len())
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

fn check_clock(s: &str) -> Result<u32, String> {
    let bad = || format!("time \"{s}\" must be HH:MM between 00:00 and 24:00");
    let (h, m) = s.split_once(':').ok_or_else(bad)?;
    if h.len() != 2 || m.len() != 2 {
        return Err(bad());
    }
    let (h, m): (u32, u32) = (h.parse().map_err(|_| bad())?, m.parse().map_err(|_| bad())?);
    if m > 59 || h > 24 || (h == 24 && m != 0) {
        return Err(bad());
    }
    Ok(h * 60 + m)
}

fn check_cidr(s: &str) -> bool {
    let (addr, bits) = match s.split_once('/') {
        Some((a, b)) => (a, Some(b)),
        None => (s, None),
    };
    let Ok(ip) = addr.parse::<std::net::IpAddr>() else {
        return false;
    };
    match bits {
        None => true,
        Some(b) => b
            .parse::<u8>()
            .is_ok_and(|n| n <= if ip.is_ipv4() { 32 } else { 128 }),
    }
}

fn check_conditions(c: &Conditions, at: &str, sink: &mut Sink) {
    for (i, r) in c.ip_ranges.iter().enumerate() {
        if !check_cidr(r) {
            sink.err(
                codes::E_BAD_CONDITION,
                &format!("{at}.ip_ranges[{i}]"),
                format!("invalid CIDR \"{r}\""),
            );
        }
    }
    let mut times = [None, None];
    for (slot, (key, value)) in [("not_before", &c.not_before), ("not_after", &c.not_after)]
        .into_iter()
        .enumerate()
    {
        if let Some(v) = value {
            match chrono::DateTime::parse_from_rfc3339(v) {
                Ok(t) => times[slot] = Some(t),
                Err(_) => sink.err(
                    codes::E_BAD_CONDITION,
                    &join(at, key),
                    format!("\"{v}\" is not an RFC 3339 date-time with a time zone"),
                ),
            }
        }
    }
    if let [Some(nb), Some(na)] = times {
        if nb >= na {
            sink.err(
                codes::E_BAD_CONDITION,
                &join(at, "not_after"),
                "must be after not_before",
            );
        }
    }
    if let Some(w) = &c.time_window {
        let wat = join(at, "time_window");
        for (i, d) in w.days.iter().enumerate() {
            if !["mon", "tue", "wed", "thu", "fri", "sat", "sun"]
                .contains(&d.to_ascii_lowercase().as_str())
            {
                sink.err(
                    codes::E_BAD_CONDITION,
                    &format!("{wat}.days[{i}]"),
                    format!("unknown day \"{d}\" (want mon, tue, wed, thu, fri, sat or sun)"),
                );
            }
        }
        let start = check_clock(&w.start);
        let end = check_clock(&w.end);
        if let Err(e) = &start {
            sink.err(codes::E_BAD_CONDITION, &join(&wat, "start"), e.clone());
        }
        if let Err(e) = &end {
            sink.err(codes::E_BAD_CONDITION, &join(&wat, "end"), e.clone());
        }
        match (start, end) {
            (Ok(1440), _) => sink.err(
                codes::E_BAD_CONDITION,
                &join(&wat, "start"),
                "cannot be 24:00",
            ),
            (Ok(s), Ok(e)) if s == e => sink.err(
                codes::E_BAD_CONDITION,
                &join(&wat, "end"),
                "must differ from start",
            ),
            _ => {}
        }
        if w.timezone.chars().any(char::is_whitespace) {
            sink.err(
                codes::E_BAD_CONDITION,
                &join(&wat, "timezone"),
                format!("\"{}\" is not an IANA time zone name", w.timezone),
            );
        }
    }
    // v0.8 ABAC attribute conditions (same rules as authz.ValidateStatements).
    if c.attributes.len() > attributes::MAX_CONDITIONS {
        sink.err(
            codes::E_LIMIT,
            &join(at, "attributes"),
            format!(
                "at most {} attribute conditions are allowed",
                attributes::MAX_CONDITIONS
            ),
        );
    }
    for (i, a) in c.attributes.iter().enumerate() {
        for (field, msg) in attributes::check_condition(a) {
            sink.err(
                codes::E_BAD_CONDITION,
                &format!("{at}.attributes[{i}].{field}"),
                msg,
            );
        }
    }
}

/// Validate roles and policies for `scope`, returning every problem
/// (errors and warnings). The server repeats these checks and additionally
/// verifies that referenced identities exist.
pub fn validate(roles: &[Role], policies: &[Policy], scope: &Scope) -> Vec<Diagnostic> {
    let mut sink = Sink::default();
    let mut role_diags = Sink::default();
    validate_roles(roles, scope, &mut role_diags);
    let mut policy_diags = Sink::default();
    validate_policies(policies, scope, &mut policy_diags);
    for d in role_diags.errors.into_iter().chain(role_diags.warnings) {
        sink.push(Diagnostic {
            file: ROLES_FILE.into(),
            ..d
        });
    }
    for d in policy_diags.errors.into_iter().chain(policy_diags.warnings) {
        sink.push(Diagnostic {
            file: POLICIES_FILE.into(),
            ..d
        });
    }
    sink.errors.into_iter().chain(sink.warnings).collect()
}

fn validate_roles(roles: &[Role], scope: &Scope, sink: &mut Sink) {
    if matches!(scope, Scope::Tree { .. }) && !roles.is_empty() {
        sink.err(
            codes::E_TREE_ROLES,
            "",
            "tree-level access files cannot define roles; define them in .wt/access/roles.toml",
        );
    }
    if roles.len() > MAX_ROLES {
        sink.err(
            codes::E_LIMIT,
            "",
            format!("at most {MAX_ROLES} roles are allowed"),
        );
    }
    let mut seen: HashMap<&str, usize> = HashMap::new();
    for (i, r) in roles.iter().enumerate() {
        let at = label(&format!("role[{i}]"), &r.name);
        if r.name.is_empty() {
            sink.err(codes::E_MISSING, &at, "name is required");
        } else if !valid_role_name(&r.name) {
            sink.err(
                codes::E_BAD_NAME,
                &at,
                "name must be 2-40 characters of lowercase letters, digits and '-', starting with a letter or digit",
            );
        } else if BUILTIN_ROLES.contains(&r.name.as_str()) {
            sink.err(
                codes::E_BAD_NAME,
                &at,
                format!("\"{}\" is a built-in role", r.name),
            );
        }
        if let Some(j) = seen.insert(r.name.as_str(), i) {
            if !r.name.is_empty() {
                sink.err(
                    codes::E_DUPLICATE,
                    &at,
                    format!("duplicate role name (also role[{j}])"),
                );
            }
        }
        if r.description.chars().count() > 2000 {
            sink.err(
                codes::E_LIMIT,
                &at,
                "description must be at most 2000 characters",
            );
        }
        if r.permissions.len() > MAX_PERMISSIONS {
            sink.err(
                codes::E_LIMIT,
                &at,
                format!("at most {MAX_PERMISSIONS} permissions are allowed"),
            );
        }
        for (j, p) in r.permissions.iter().enumerate() {
            let pat = format!("{at}.permissions[{j}]");
            if p == "*" {
                sink.err(
                    codes::E_ROLE_PERMISSION,
                    &pat,
                    "\"*\" is not allowed in a role; list areas (\"tree:*\") instead",
                );
            } else if let Err(e) = check_action(p) {
                sink.err(codes::E_UNKNOWN_ACTION, &pat, e);
            } else if area_of(p) == "server" {
                sink.err(
                    codes::E_ROLE_PERMISSION,
                    &pat,
                    "server permissions cannot be granted by a role",
                );
            }
        }
    }
}

fn validate_policies(policies: &[Policy], scope: &Scope, sink: &mut Sink) {
    if policies.len() > MAX_POLICIES {
        sink.err(
            codes::E_LIMIT,
            "",
            format!("at most {MAX_POLICIES} policies are allowed"),
        );
    }
    let tree_base = match scope {
        Scope::Tree {
            tenant: Some(t),
            tree: Some(n),
        } => Some(format!("{t}/{n}")),
        _ => None,
    };
    let mut seen: HashMap<String, usize> = HashMap::new();
    for (i, p) in policies.iter().enumerate() {
        let at = label(&format!("policy[{i}]"), &p.name);
        if p.name.is_empty() {
            sink.err(codes::E_MISSING, &at, "name is required");
        } else if p.name.chars().count() > 100 || p.name.contains(['/', '\n', '\r', '\0']) {
            sink.err(
                codes::E_BAD_NAME,
                &at,
                "name must be at most 100 characters without '/' or line breaks",
            );
        }
        if let Some(j) = seen.insert(p.name.to_lowercase(), i) {
            if !p.name.is_empty() {
                sink.err(
                    codes::E_DUPLICATE,
                    &at,
                    format!("duplicate policy name (also policy[{j}])"),
                );
            }
        }
        if p.description.chars().count() > 2000 {
            sink.err(
                codes::E_LIMIT,
                &at,
                "description must be at most 2000 characters",
            );
        }
        if p.statements.is_empty() {
            sink.err(
                codes::W_EMPTY_POLICY,
                &at,
                "policy has no statements and has no effect",
            );
        }
        if p.statements.len() > MAX_STATEMENTS {
            sink.err(
                codes::E_LIMIT,
                &at,
                format!("a policy may have at most {MAX_STATEMENTS} statements"),
            );
        }
        let mut sids: BTreeSet<&str> = BTreeSet::new();
        for (j, st) in p.statements.iter().enumerate() {
            let sat = format!("{at}.statement[{j}]");
            if !st.sid.is_empty() && !sids.insert(st.sid.as_str()) {
                sink.err(
                    codes::E_DUPLICATE,
                    &sat,
                    format!("duplicate sid \"{}\"", st.sid),
                );
            }
            if st.sid.chars().count() > MAX_SID {
                sink.err(
                    codes::E_LIMIT,
                    &sat,
                    format!("sid must be at most {MAX_SID} characters"),
                );
            }
            validate_statement(st, &sat, scope, tree_base.as_deref(), sink);
        }
    }
}

fn validate_statement(
    st: &Statement,
    at: &str,
    scope: &Scope,
    tree_base: Option<&str>,
    sink: &mut Sink,
) {
    match st.effect.as_str() {
        "deny" => {}
        "allow" => {
            if matches!(scope, Scope::Tree { .. }) {
                sink.err(
                    codes::E_TREE_ALLOW,
                    &join(at, "effect"),
                    "tree-level access files may only contain deny statements",
                );
            }
        }
        "" => sink.err(
            codes::E_MISSING,
            &join(at, "effect"),
            "effect is required (allow or deny)",
        ),
        other => sink.err(
            codes::E_BAD_EFFECT,
            &join(at, "effect"),
            format!("effect must be allow or deny, got \"{other}\""),
        ),
    }
    for (field, values) in [
        ("actions", &st.actions),
        ("resources", &st.resources),
        ("principals", &st.principals),
    ] {
        if values.is_empty() {
            let hint = match field {
                "resources" => " (use \"*\" for all)",
                "principals" => " (use \"*\" for everyone)",
                _ => "",
            };
            sink.err(
                codes::E_MISSING,
                &join(at, field),
                format!("at least one entry is required{hint}"),
            );
        }
    }
    for (k, a) in st.actions.iter().enumerate() {
        if let Err(e) = check_action(a) {
            sink.err(codes::E_UNKNOWN_ACTION, &format!("{at}.actions[{k}]"), e);
        }
    }
    for (k, r) in st.resources.iter().enumerate() {
        let rat = format!("{at}.resources[{k}]");
        if let Err(e) = check_resource(r) {
            sink.err(codes::E_BAD_RESOURCE, &rat, e);
        } else if let Some(base) = tree_base {
            let inside = r == &format!("wt:tree/{base}")
                || r.starts_with(&format!("wt:branch/{base}/"))
                || r.starts_with(&format!("wt:path/{base}/"));
            if !inside {
                sink.err(
                    codes::E_TREE_RESOURCE,
                    &rat,
                    format!("\"{r}\" is outside tree {base} (use wt:tree/{base}, wt:branch/{base}/... or wt:path/{base}/...)"),
                );
            }
        } else if matches!(scope, Scope::Tree { .. }) && r == "*" {
            sink.err(
                codes::E_TREE_RESOURCE,
                &rat,
                "\"*\" reaches outside the tree; name the tree's resources",
            );
        }
    }
    for (k, p) in st.principals.iter().enumerate() {
        if let Err(e) = check_principal(p) {
            sink.err(codes::E_BAD_PRINCIPAL, &format!("{at}.principals[{k}]"), e);
        }
    }
    check_conditions(&st.conditions, &join(at, "conditions"), sink);
}

// ---------------------------------------------------------------------------
// Compilation

/// Parse, validate and compile access files into the canonical document.
/// `roles` / `policies` are the TOML sources (`None` = file absent). Every
/// error of every file is returned; warnings come back in
/// [`Compiled::warnings`].
pub fn compile(
    roles: Option<&str>,
    policies: Option<&str>,
    scope: &Scope,
) -> Result<Compiled, Vec<Diagnostic>> {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let with_file = |file: &str, ds: Vec<Diagnostic>| -> Vec<Diagnostic> {
        ds.into_iter()
            .map(|d| Diagnostic {
                file: file.into(),
                ..d
            })
            .collect()
    };
    let mut doc = Document::default();
    if let Some(src) = roles {
        match parse_roles(src) {
            Ok(p) => {
                doc.roles = p.value;
                warnings.extend(with_file(ROLES_FILE, p.warnings));
            }
            Err(e) => errors.extend(with_file(ROLES_FILE, e)),
        }
    }
    if let Some(src) = policies {
        match parse_policies(src) {
            Ok(p) => {
                doc.policies = p.value;
                warnings.extend(with_file(POLICIES_FILE, p.warnings));
            }
            Err(e) => errors.extend(with_file(POLICIES_FILE, e)),
        }
    }
    if errors.is_empty() {
        for d in validate(&doc.roles, &doc.policies, scope) {
            if d.is_error() {
                errors.push(d);
            } else {
                warnings.push(d);
            }
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    // Canonical form: role permissions deduplicated and sorted (the server
    // stores them that way, so plans stay stable).
    for r in &mut doc.roles {
        let set: BTreeSet<String> = r.permissions.drain(..).collect();
        r.permissions = set.into_iter().collect();
    }
    let json = doc.to_json();
    Ok(Compiled {
        document: doc,
        json,
        warnings,
    })
}

// ---------------------------------------------------------------------------
// Decompilation (for `wt access pull`)

fn toml_str(s: &str) -> String {
    toml::Value::String(s.to_string()).to_string()
}

fn toml_list(items: &[String]) -> String {
    let parts: Vec<String> = items.iter().map(|s| toml_str(s)).collect();
    format!("[{}]", parts.join(", "))
}

fn conditions_toml(c: &Conditions) -> String {
    let mut parts = Vec::new();
    if c.require_mfa {
        parts.push("mfa = true".to_string());
    }
    if !c.ip_ranges.is_empty() {
        parts.push(format!("ip_ranges = {}", toml_list(&c.ip_ranges)));
    }
    if let Some(t) = &c.not_before {
        parts.push(format!("not_before = {}", toml_str(t)));
    }
    if let Some(t) = &c.not_after {
        parts.push(format!("not_after = {}", toml_str(t)));
    }
    if let Some(w) = &c.time_window {
        let mut wp = Vec::new();
        if !w.days.is_empty() {
            wp.push(format!("days = {}", toml_list(&w.days)));
        }
        wp.push(format!("start = {}", toml_str(&w.start)));
        wp.push(format!("end = {}", toml_str(&w.end)));
        if !w.timezone.is_empty() {
            wp.push(format!("timezone = {}", toml_str(&w.timezone)));
        }
        parts.push(format!("time_window = {{ {} }}", wp.join(", ")));
    }
    if !c.attributes.is_empty() {
        let items: Vec<String> = c
            .attributes
            .iter()
            .map(|a| {
                let mut ap = vec![
                    format!("key = {}", toml_str(&a.key)),
                    format!("op = {}", toml_str(&a.op)),
                ];
                if !a.values.is_empty() {
                    ap.push(format!("values = {}", toml_list(&a.values)));
                }
                format!("{{ {} }}", ap.join(", "))
            })
            .collect();
        parts.push(format!("attributes = [{}]", items.join(", ")));
    }
    format!("{{ {} }}", parts.join(", "))
}

/// Render roles as `roles.toml`.
pub fn roles_to_toml(roles: &[Role]) -> String {
    let mut out = String::from("# Custom roles of the tenant (wt access apply).\n");
    for r in roles {
        out.push_str("\n[[role]]\n");
        out.push_str(&format!("name = {}\n", toml_str(&r.name)));
        if !r.description.is_empty() {
            out.push_str(&format!("description = {}\n", toml_str(&r.description)));
        }
        out.push_str(&format!("permissions = {}\n", toml_list(&r.permissions)));
    }
    out
}

/// Render policies as `policies.toml`.
pub fn policies_to_toml(policies: &[Policy]) -> String {
    let mut out = String::from("# Access policies (wt access apply).\n");
    for p in policies {
        out.push_str("\n[[policy]]\n");
        out.push_str(&format!("name = {}\n", toml_str(&p.name)));
        if !p.description.is_empty() {
            out.push_str(&format!("description = {}\n", toml_str(&p.description)));
        }
        out.push_str(&format!("enabled = {}\n", p.enabled));
        for st in &p.statements {
            out.push_str("\n[[policy.statement]]\n");
            if !st.sid.is_empty() {
                out.push_str(&format!("sid = {}\n", toml_str(&st.sid)));
            }
            out.push_str(&format!("effect = {}\n", toml_str(&st.effect)));
            out.push_str(&format!("principals = {}\n", toml_list(&st.principals)));
            out.push_str(&format!("actions = {}\n", toml_list(&st.actions)));
            out.push_str(&format!("resources = {}\n", toml_list(&st.resources)));
            if !st.conditions.is_empty() {
                out.push_str(&format!(
                    "conditions = {}\n",
                    conditions_toml(&st.conditions)
                ));
            }
        }
    }
    out
}

/// Tree-level policy names are stored on the server as `<tree>/<name>`;
/// strip that prefix (pulling a tree document yields the names as written).
pub fn strip_tree_prefix(policies: &mut [Policy], tree: &str) {
    let prefix = format!("{tree}/");
    for p in policies {
        if let Some(rest) = p.name.strip_prefix(&prefix) {
            p.name = rest.to_string();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROLES: &str = r#"
[[role]]
name = "release-manager"
description = "Cut releases"
permissions = ["tree:read", "release:*", "mr:merge"]
"#;

    const POLICIES: &str = r#"
[[policy]]
name = "protect-main"
description = "Only maintainers push to main"
enabled = true
[[policy.statement]]
sid = "no-direct-main"
effect = "deny"            # allow | deny
principals = ["role:developer", "team:contractors", "user:bob", "tenant:partner"]
actions = ["branch:push"]
resources = ["wt:branch/acme/web/main"]
conditions = { mfa = true }
"#;

    fn codes_of(ds: &[Diagnostic]) -> Vec<&'static str> {
        ds.iter().map(|d| d.code).collect()
    }

    fn policy_errors(src: &str, scope: &Scope) -> Vec<Diagnostic> {
        compile(None, Some(src), scope).expect_err("expected errors")
    }

    fn one_statement(body: &str) -> String {
        format!("[[policy]]\nname = \"p\"\n[[policy.statement]]\n{body}\n")
    }

    const OK_STMT: &str =
        "effect = \"deny\"\nactions = [\"tree:read\"]\nresources = [\"*\"]\nprincipals = [\"*\"]";

    #[test]
    fn compiles_the_spec_example() {
        let c = compile(Some(ROLES), Some(POLICIES), &Scope::Tenant).unwrap();
        assert!(c.warnings.is_empty(), "{:?}", c.warnings);
        assert_eq!(c.document.roles.len(), 1);
        assert_eq!(
            c.document.roles[0].permissions,
            vec!["mr:merge", "release:*", "tree:read"]
        );
        let v: serde_json::Value = serde_json::from_str(&c.json).unwrap();
        assert_eq!(v["roles"][0]["name"], "release-manager");
        assert_eq!(v["roles"][0]["description"], "Cut releases");
        let st = &v["policies"][0]["statements"][0];
        assert_eq!(v["policies"][0]["enabled"], true);
        assert_eq!(st["sid"], "no-direct-main");
        assert_eq!(st["effect"], "deny");
        assert_eq!(st["actions"][0], "branch:push");
        assert_eq!(st["principals"][3], "tenant:partner");
        assert_eq!(st["conditions"], serde_json::json!({"require_mfa": true}));
        // Round trip through JSON.
        assert_eq!(Document::from_json(c.json.as_bytes()).unwrap(), c.document);
    }

    #[test]
    fn empty_and_missing_files_compile_to_an_empty_document() {
        let c = compile(None, None, &Scope::Tenant).unwrap();
        assert_eq!(c.json, r#"{"roles":[],"policies":[]}"#);
        let c = compile(Some(""), Some("# nothing\n"), &Scope::Tenant).unwrap();
        assert!(c.document.roles.is_empty() && c.document.policies.is_empty());
    }

    #[test]
    fn full_conditions_compile() {
        let src = one_statement(
            r#"effect = "allow"
actions = ["tree:*", "*"]
resources = ["wt:path/acme/web/src/**", "wt:tenant/acme", "wt:tree/acme/*", "wt:branch/acme/**"]
principals = ["sa:ci", "sa:acme/deploy", "authenticated", "user:a*"]
conditions = { ip_ranges = ["10.0.0.0/8", "::1"], not_before = 2026-01-01T00:00:00Z, not_after = "2027-01-01T00:00:00+02:00", time_window = { days = ["mon", "FRI"], start = "09:00", end = "18:00", timezone = "Europe/Berlin" } }"#,
        );
        let c = compile(None, Some(&src), &Scope::Tenant).unwrap();
        let cond = &c.document.policies[0].statements[0].conditions;
        assert_eq!(cond.ip_ranges, vec!["10.0.0.0/8", "::1"]);
        assert_eq!(cond.not_before.as_deref(), Some("2026-01-01T00:00:00Z"));
        assert_eq!(cond.time_window.as_ref().unwrap().timezone, "Europe/Berlin");
        let v: serde_json::Value = serde_json::from_str(&c.json).unwrap();
        assert_eq!(
            v["policies"][0]["statements"][0]["conditions"]["time_window"]["days"],
            serde_json::json!(["mon", "FRI"])
        );
    }

    #[test]
    fn aliases_are_translated_with_warnings() {
        let roles =
            "[[role]]\nname = \"pusher\"\npermissions = [\"sync:push\", \"snapshot:create\"]\n";
        let pols = one_statement(
            "effect = \"deny\"\nactions = [\"branch:merge\", \"tree:delete\"]\nresources = [\"*\"]\nprincipals = [\"*\"]",
        );
        let c = compile(Some(roles), Some(&pols), &Scope::Tenant).unwrap();
        assert_eq!(
            c.document.roles[0].permissions,
            vec!["branch:push", "tree:write"]
        );
        assert_eq!(
            c.document.policies[0].statements[0].actions,
            vec!["mr:merge", "tree:admin"]
        );
        assert_eq!(codes_of(&c.warnings), vec![codes::W_ALIAS; 4]);
        assert!(c.warnings[0].to_string().starts_with(
            "W2101 roles.toml role[0] (pusher).permissions[0]: 'sync:push' is an alias"
        ));
        for (alias, canonical) in ALIASES {
            assert!(ACTIONS.contains(canonical), "{alias}");
        }
    }

    #[test]
    fn every_error_code() {
        let tenant = Scope::Tenant;
        let tree = Scope::Tree {
            tenant: Some("acme".into()),
            tree: Some("web".into()),
        };
        let cases: Vec<(&str, Vec<Diagnostic>)> = vec![
            (
                codes::E_SYNTAX,
                policy_errors("[[policy]\nname = ", &tenant),
            ),
            (
                codes::E_STRUCTURE,
                policy_errors("[[policy]]\nname = \"p\"\nstatements = 3\n", &tenant),
            ),
            (
                codes::E_STRUCTURE,
                policy_errors(&one_statement(&format!("{OK_STMT}\nactions_ = 1")), &tenant),
            ),
            (
                codes::E_STRUCTURE,
                policy_errors("[[policy]]\nname = 3\n", &tenant),
            ),
            (
                codes::E_MISSING,
                policy_errors(
                    &one_statement("effect = \"deny\"\nresources = [\"*\"]\nprincipals = [\"*\"]"),
                    &tenant,
                ),
            ),
            (
                codes::E_MISSING,
                policy_errors(&format!("[[policy]]\n[[policy.statement]]\n{OK_STMT}"), &tenant),
            ),
            (
                codes::E_UNKNOWN_ACTION,
                policy_errors(
                    &one_statement("effect = \"deny\"\nactions = [\"tree:fly\"]\nresources = [\"*\"]\nprincipals = [\"*\"]"),
                    &tenant,
                ),
            ),
            (
                codes::E_BAD_PRINCIPAL,
                policy_errors(
                    &one_statement("effect = \"deny\"\nactions = [\"tree:read\"]\nresources = [\"*\"]\nprincipals = [\"group:x\"]"),
                    &tenant,
                ),
            ),
            (
                codes::E_BAD_RESOURCE,
                policy_errors(
                    &one_statement("effect = \"deny\"\nactions = [\"tree:read\"]\nresources = [\"wt:tree/acme\"]\nprincipals = [\"*\"]"),
                    &tenant,
                ),
            ),
            (
                codes::E_DUPLICATE,
                policy_errors(
                    &format!("{}{}", one_statement(OK_STMT), one_statement(OK_STMT).replace("\"p\"", "\"P\"")),
                    &tenant,
                ),
            ),
            (
                codes::E_TREE_ALLOW,
                policy_errors(
                    &one_statement("effect = \"allow\"\nactions = [\"tree:read\"]\nresources = [\"wt:tree/acme/web\"]\nprincipals = [\"*\"]"),
                    &tree,
                ),
            ),
            (
                codes::E_BAD_NAME,
                policy_errors("[[policy]]\nname = \"a/b\"\n", &tenant),
            ),
            (
                codes::E_BAD_EFFECT,
                policy_errors(&one_statement(&OK_STMT.replace("deny", "maybe")), &tenant),
            ),
            (
                codes::E_BAD_CONDITION,
                policy_errors(
                    &one_statement(&format!("{OK_STMT}\nconditions = {{ ip_ranges = [\"10.0.0.0/33\"] }}")),
                    &tenant,
                ),
            ),
            (
                codes::E_TREE_RESOURCE,
                policy_errors(
                    &one_statement("effect = \"deny\"\nactions = [\"tree:read\"]\nresources = [\"wt:tree/acme/docs\"]\nprincipals = [\"*\"]"),
                    &tree,
                ),
            ),
            (
                codes::E_TREE_ROLES,
                compile(Some("[[role]]\nname = \"r1\"\n"), None, &tree).unwrap_err(),
            ),
            (
                codes::E_ROLE_PERMISSION,
                compile(Some("[[role]]\nname = \"r1\"\npermissions = [\"server:admin\"]\n"), None, &tenant)
                    .unwrap_err(),
            ),
            (
                codes::E_LIMIT,
                policy_errors(&one_statement(&format!("{OK_STMT}\nsid = \"{}\"", "x".repeat(200))), &tenant),
            ),
        ];
        for (code, diags) in &cases {
            assert!(
                codes_of(diags).contains(code),
                "expected {code}, got {diags:?}"
            );
            assert!(diags.iter().all(Diagnostic::is_error));
            assert!(diags.iter().all(|d| !d.file.is_empty()), "{diags:?}");
        }
        // Every declared error code is exercised above.
        for code in [
            codes::E_SYNTAX,
            codes::E_STRUCTURE,
            codes::E_MISSING,
            codes::E_UNKNOWN_ACTION,
            codes::E_BAD_PRINCIPAL,
            codes::E_BAD_RESOURCE,
            codes::E_DUPLICATE,
            codes::E_TREE_ALLOW,
            codes::E_BAD_NAME,
            codes::E_BAD_EFFECT,
            codes::E_BAD_CONDITION,
            codes::E_TREE_RESOURCE,
            codes::E_TREE_ROLES,
            codes::E_ROLE_PERMISSION,
            codes::E_LIMIT,
        ] {
            assert!(cases.iter().any(|(c, _)| *c == code), "{code} untested");
        }
    }

    #[test]
    fn role_validation() {
        let errs = compile(
            Some(
                r#"
[[role]]
name = "admin"
[[role]]
name = "Bad Name"
[[role]]
name = "ok-role"
permissions = ["*", "tree:fly", "server:*", "nope:*"]
[[role]]
name = "ok-role"
[[role]]
description = "no name"
"#,
            ),
            None,
            &Scope::Tenant,
        )
        .unwrap_err();
        let text: Vec<String> = errs.iter().map(ToString::to_string).collect();
        let all = text.join("\n");
        assert!(
            all.contains("E2009 roles.toml role[0] (admin): \"admin\" is a built-in role"),
            "{all}"
        );
        assert!(all.contains("E2009 roles.toml role[1] (Bad Name)"), "{all}");
        assert!(
            all.contains("E2014 roles.toml role[2] (ok-role).permissions[0]"),
            "{all}"
        );
        assert!(
            all.contains(
                "E2004 roles.toml role[2] (ok-role).permissions[1]: unknown action \"tree:fly\""
            ),
            "{all}"
        );
        assert!(
            all.contains("E2014 roles.toml role[2] (ok-role).permissions[2]"),
            "{all}"
        );
        assert!(all.contains("unknown action area \"nope\""), "{all}");
        assert!(
            all.contains("E2007 roles.toml role[3] (ok-role): duplicate role name"),
            "{all}"
        );
        assert!(
            all.contains("E2003 roles.toml role[4]: name is required"),
            "{all}"
        );
    }

    #[test]
    fn condition_validation() {
        let bad = [
            "not_before = \"yesterday\"",
            "not_before = 2026-01-01T00:00:00",
            "not_before = \"2027-01-01T00:00:00Z\", not_after = \"2026-01-01T00:00:00Z\"",
            "time_window = { days = [\"funday\"], start = \"09:00\", end = \"17:00\" }",
            "time_window = { start = \"9am\", end = \"17:00\" }",
            "time_window = { start = \"09:00\", end = \"09:00\" }",
            "time_window = { start = \"24:00\", end = \"09:00\" }",
            "time_window = { start = \"09:00\", end = \"17:00\", timezone = \"Not A Zone\" }",
            "ip_ranges = [\"300.0.0.1\"]",
        ];
        for cond in bad {
            let src = one_statement(&format!("{OK_STMT}\nconditions = {{ {cond} }}"));
            let errs = compile(None, Some(&src), &Scope::Tenant).unwrap_err();
            assert!(
                codes_of(&errs).contains(&codes::E_BAD_CONDITION),
                "{cond}: {errs:?}"
            );
        }
        let src = one_statement(&format!("{OK_STMT}\nconditions = {{ colour = \"red\" }}"));
        assert_eq!(
            codes_of(&compile(None, Some(&src), &Scope::Tenant).unwrap_err()),
            vec![codes::E_STRUCTURE]
        );
    }

    #[test]
    fn attribute_conditions_compile() {
        let src = one_statement(&format!(
            "{OK_STMT}\n\
             [[policy.statement.conditions.attributes]]\n\
             key = \"tree.classification\"\n\
             op = \"EQUALS\"\n\
             value = \"secret\"\n\
             [[policy.statement.conditions.attributes]]\n\
             key = \"principal.clearance\"\n\
             op = \"not_in\"\n\
             values = [\"secret\", \"top-secret\"]\n\
             [[policy.statement.conditions.attributes]]\n\
             key = \"tenant.region\"\n\
             op = \"exists\"\n"
        ));
        let c = compile(None, Some(&src), &Scope::Tenant).unwrap();
        let attrs = &c.document.policies[0].statements[0].conditions.attributes;
        assert_eq!(attrs.len(), 3);
        assert_eq!(attrs[0].op, "equals");
        assert_eq!(attrs[0].values, vec!["secret"]);
        let v: serde_json::Value = serde_json::from_str(&c.json).unwrap();
        assert_eq!(
            v["policies"][0]["statements"][0]["conditions"]["attributes"],
            serde_json::json!([
                {"key": "tree.classification", "op": "equals", "values": ["secret"]},
                {"key": "principal.clearance", "op": "not_in", "values": ["secret", "top-secret"]},
                {"key": "tenant.region", "op": "exists"}
            ])
        );
        // The inline form is equivalent and decompiles to TOML that compiles
        // back to the same document.
        let inline = one_statement(&format!(
            "{OK_STMT}\nconditions = {{ mfa = true, attributes = [{{ key = \"tree.classification\", op = \"equals\", values = [\"secret\"] }}] }}"
        ));
        let c = compile(None, Some(&inline), &Scope::Tenant).unwrap();
        let toml = policies_to_toml(&c.document.policies);
        assert!(toml.contains("attributes = [{ key = \"tree.classification\", op = \"equals\", values = [\"secret\"] }]"), "{toml}");
        assert_eq!(
            compile(None, Some(&toml), &Scope::Tenant).unwrap().document,
            c.document
        );
        // Tree-scope deny documents may use attribute conditions too.
        let tree_doc = "[[policy]]\nname = \"p\"\n[[policy.statement]]\neffect = \"deny\"\nactions = [\"tree:read\"]\nresources = [\"wt:tree/acme/web\"]\nprincipals = [\"*\"]\nconditions = { attributes = [{ key = \"principal.clearance\", op = \"not_exists\" }] }\n";
        let scope = Scope::Tree {
            tenant: Some("acme".into()),
            tree: Some("web".into()),
        };
        assert!(compile(None, Some(tree_doc), &scope).is_ok());
    }

    #[test]
    fn attribute_condition_validation() {
        let cases = [
            (
                "{ key = \"department\", op = \"equals\", value = \"eng\" }",
                codes::E_BAD_CONDITION,
                "attributes[0].key",
            ),
            (
                "{ key = \"account.x\", op = \"exists\" }",
                codes::E_BAD_CONDITION,
                "attributes[0].key",
            ),
            (
                "{ key = \"tree.x\", op = \"like\", value = \"a\" }",
                codes::E_BAD_CONDITION,
                "attributes[0].op",
            ),
            (
                "{ key = \"tree.x\", op = \"equals\" }",
                codes::E_BAD_CONDITION,
                "attributes[0].values",
            ),
            (
                "{ key = \"tree.x\", op = \"exists\", value = \"a\" }",
                codes::E_BAD_CONDITION,
                "attributes[0].values",
            ),
            (
                "{ key = \"tree.x\", op = \"in\", values = [\"\"] }",
                codes::E_BAD_CONDITION,
                "attributes[0].values[0]",
            ),
            (
                "{ key = \"tree.x\", op = \"in\", colour = \"red\" }",
                codes::E_STRUCTURE,
                "attributes[0].colour",
            ),
            (
                "{ key = \"tree.x\", op = \"in\", value = \"a\", values = [\"b\"] }",
                codes::E_STRUCTURE,
                "attributes[0].value",
            ),
            (
                "{ key = 1, op = \"exists\" }",
                codes::E_STRUCTURE,
                "attributes[0].key",
            ),
        ];
        for (cond, code, at) in cases {
            let src = one_statement(&format!(
                "{OK_STMT}\nconditions = {{ attributes = [{cond}] }}"
            ));
            let errs = compile(None, Some(&src), &Scope::Tenant).unwrap_err();
            assert!(
                errs.iter()
                    .any(|d| d.code == code && d.location.ends_with(at)),
                "{cond}: {errs:?}"
            );
        }
        let src = one_statement(&format!("{OK_STMT}\nconditions = {{ attributes = \"x\" }}"));
        assert_eq!(
            codes_of(&compile(None, Some(&src), &Scope::Tenant).unwrap_err()),
            vec![codes::E_STRUCTURE]
        );
        let many = vec!["{ key = \"tree.x\", op = \"exists\" }"; 33].join(", ");
        let src = one_statement(&format!(
            "{OK_STMT}\nconditions = {{ attributes = [{many}] }}"
        ));
        assert_eq!(
            codes_of(&compile(None, Some(&src), &Scope::Tenant).unwrap_err()),
            vec![codes::E_LIMIT]
        );
        // Server documents with attribute conditions parse.
        let json = br#"{"policies":[{"name":"p","statements":[{"effect":"deny","actions":["tree:read"],"resources":["*"],"principals":["*"],
            "conditions":{"attributes":[{"key":"tree.c","op":"exists","values":null}]}}]}]}"#;
        let doc = Document::from_json(json).unwrap();
        assert_eq!(
            doc.policies[0].statements[0].conditions.attributes[0].key,
            "tree.c"
        );
    }

    #[test]
    fn principal_and_resource_rules() {
        for ok in [
            "*",
            "authenticated",
            "anonymous",
            "user:bob",
            "team:core",
            "role:dev*",
            "sa:ci",
            "sa:acme/ci",
            "tenant:partner",
        ] {
            assert!(check_principal(ok).is_ok(), "{ok}");
        }
        for bad in [
            "", "user:", "team:a/b", "sa:/x", "sa:a/b/c", "user:a b", "bob", "tenant:",
        ] {
            assert!(check_principal(bad).is_err(), "{bad}");
        }
        for ok in [
            "*",
            "wt:tenant/acme",
            "wt:tree/acme/web",
            "wt:tree/acme/*",
            "wt:branch/acme/web/release/v1",
            "wt:branch/acme/**",
            "wt:path/acme/web/src/**",
        ] {
            assert!(check_resource(ok).is_ok(), "{ok}");
        }
        for bad in [
            "acme",
            "wt:tenant/a/b",
            "wt:tree/acme",
            "wt:branch/acme/web",
            "wt:repo/x",
            "wt:tree/acme//web",
            "wt:path/a/b/c**",
            "wt:tree/",
        ] {
            assert!(check_resource(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn tree_scope_rules() {
        let tree = Scope::Tree {
            tenant: Some("acme".into()),
            tree: Some("web".into()),
        };
        let ok = one_statement(
            "effect = \"deny\"\nactions = [\"branch:push\"]\nresources = [\"wt:branch/acme/web/main\", \"wt:path/acme/web/secret/**\", \"wt:tree/acme/web\"]\nprincipals = [\"*\"]",
        );
        assert!(compile(None, Some(&ok), &tree).is_ok());
        // Without names only "*" is rejected for being outside the tree.
        let unknown = Scope::Tree {
            tenant: None,
            tree: None,
        };
        assert!(compile(None, Some(&ok), &unknown).is_ok());
        let star = one_statement(OK_STMT);
        assert_eq!(
            codes_of(&compile(None, Some(&star), &unknown).unwrap_err()),
            vec![codes::E_TREE_RESOURCE]
        );
        // A prefix that only looks similar is outside.
        let sneaky = one_statement(
            "effect = \"deny\"\nactions = [\"tree:read\"]\nresources = [\"wt:tree/acme/web2\", \"wt:branch/acme/webx/main\"]\nprincipals = [\"*\"]",
        );
        assert_eq!(
            codes_of(&compile(None, Some(&sneaky), &tree).unwrap_err()),
            vec![codes::E_TREE_RESOURCE; 2]
        );
    }

    #[test]
    fn warnings_do_not_fail_and_errors_collect_across_files() {
        let c = compile(None, Some("[[policy]]\nname = \"empty\"\n"), &Scope::Tenant).unwrap();
        assert_eq!(codes_of(&c.warnings), vec![codes::W_EMPTY_POLICY]);
        assert_eq!(c.warnings[0].severity, Severity::Warning);
        // Both files' syntax errors are reported.
        let errs = compile(Some("[[role"), Some("[[policy"), &Scope::Tenant).unwrap_err();
        assert_eq!(errs.len(), 2);
        assert_eq!(errs[0].file, ROLES_FILE);
        assert_eq!(errs[1].file, POLICIES_FILE);
    }

    #[test]
    fn policy_defaults_and_single_strings() {
        let src = "[[policy]]\nname = \"p\"\n[[policy.statement]]\neffect = \"DENY\"\nactions = \"tree:read\"\nresources = \"*\"\nprincipals = \"*\"\n";
        let c = compile(None, Some(src), &Scope::Tenant).unwrap();
        let p = &c.document.policies[0];
        assert!(p.enabled);
        assert_eq!(p.statements[0].effect, "deny");
        assert_eq!(p.statements[0].actions, vec!["tree:read"]);
        let c = compile(
            None,
            Some("[[policy]]\nname = \"p\"\nenabled = false\n"),
            &Scope::Tenant,
        )
        .unwrap();
        assert!(!c.document.policies[0].enabled);
    }

    #[test]
    fn decompiles_to_equivalent_toml() {
        let full = format!(
            "{POLICIES}\n[[policy.statement]]\neffect = \"allow\"\nactions = [\"tree:read\"]\nresources = [\"*\"]\nprincipals = [\"authenticated\"]\nconditions = {{ ip_ranges = [\"10.0.0.0/8\"], not_after = \"2027-01-01T00:00:00Z\", time_window = {{ days = [\"mon\"], start = \"09:00\", end = \"17:00\", timezone = \"UTC\" }} }}\n"
        );
        let c = compile(Some(ROLES), Some(&full), &Scope::Tenant).unwrap();
        let roles = roles_to_toml(&c.document.roles);
        let pols = policies_to_toml(&c.document.policies);
        let again = compile(Some(&roles), Some(&pols), &Scope::Tenant).unwrap();
        assert_eq!(again.document, c.document);
        assert_eq!(again.json, c.json);
        // Quotes and newlines survive.
        let tricky = vec![Role {
            name: "r1".into(),
            description: "say \"hi\"\nthen leave".into(),
            permissions: vec![],
        }];
        let back = parse_roles(&roles_to_toml(&tricky)).unwrap().value;
        assert_eq!(back, tricky);
    }

    #[test]
    fn server_documents_parse_and_tree_prefixes_strip() {
        // A document as stored by the server (extra fields are ignored).
        let json = br#"{"roles":null,"policies":[{"id":"x","name":"web/freeze","enabled":true,"version":3,
            "statements":[{"effect":"deny","actions":["tree:write"],"resources":["wt:tree/acme/web"],"principals":["*"],
            "conditions":{"time_window":{"days":null,"start":"09:00","end":"10:00","timezone":""}}}]}]}"#;
        let mut doc = Document::from_json(json).unwrap();
        assert!(doc.roles.is_empty());
        strip_tree_prefix(&mut doc.policies, "web");
        assert_eq!(doc.policies[0].name, "freeze");
        assert!(Document::from_json(b"[1]").is_err());
    }

    #[test]
    fn action_catalog_matches_areas() {
        assert!(check_action("release:*").is_ok());
        assert!(check_action("ci:report").is_ok());
        assert!(check_action("*").is_ok());
        assert!(check_action("").is_err());
        assert!(valid_role_name("ab") && !valid_role_name("a") && !valid_role_name("-ab"));
    }
}
