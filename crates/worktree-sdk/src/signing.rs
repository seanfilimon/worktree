//! Ed25519 snapshot signing (docs/ENGINEERING_PLAN.md §13.6).
//!
//! The signed payload is the protobuf encoding of the snapshot's
//! `SnapshotRecord` with `signature`, `signing_key_id`, `tree_id` and
//! `branch_name` cleared (the server files records under its own tree id,
//! and the branch is informational). The Go server rebuilds the same bytes
//! (`internal/signing.Payload`); both sides pin a shared test vector.
//!
//! Keys live in `<wt home>/keys/`: `<name>.key` holds the hex-encoded
//! 32-byte seed (mode 0600 on Unix) and `<name>.pub` the public metadata
//! ([`KeyInfo`], TOML) — including the server key id once the key is
//! registered with `wt identity register`. Unregistered keys sign with
//! their fingerprint as key id; the server resolves either.
//!
//! `[signing] key = "<name>"` selects the key ("" = signing disabled unless
//! a snapshot asks for it, which then uses the `default` key) and
//! `auto = true` signs every locally created snapshot.
//!
//! v0.7 (§15.8): tags are signed the same way, over their `TagRecord` with
//! `signature` and `signing_key_id` cleared ([`tag_signed_payload`]);
//! `auto = true` also signs every tag created with [`create_tag`].

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use chrono::{DateTime, Utc};
use ed25519_dalek::{Signature, Signer as _, SigningKey, Verifier as _, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use worktree_protocol::proto::sync as wire;

use crate::config::SigningSection;
use crate::error::{Result, SdkError};
use crate::model::{Snapshot, SnapshotSignature, TagInfo};
use crate::repo::Repository;

/// The only supported signature algorithm.
pub const ALGORITHM: &str = "ed25519";

/// Key name used when none is configured or given.
pub const DEFAULT_KEY: &str = "default";

/// Length of an Ed25519 signature.
pub const SIGNATURE_LEN: usize = 64;

// ---------------------------------------------------------------------------
// Payload, signing and verification

/// Compact verification result used by `wt log --show-signature` / `wt show`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignatureStatus {
    /// The snapshot carries no signature.
    Unsigned,
    /// The signature verifies against a known key.
    Verified { key_id: String },
    /// The signature does not verify (reason).
    Invalid(String),
    /// Signed with a key that is not known locally (key id).
    UnknownKey(String),
}

/// Verify `snapshot`'s signature against the keys in `<wt home>/keys/`
/// (see [`verify_with_local_keys`] for the detailed result).
pub fn verify_local(_repo: &Repository, snapshot: &Snapshot) -> Result<SignatureStatus> {
    Ok(match verify_with_local_keys(snapshot)? {
        Verification::Unsigned => SignatureStatus::Unsigned,
        Verification::Valid { key_id, .. } => SignatureStatus::Verified { key_id },
        Verification::Invalid { key_id, key } => SignatureStatus::Invalid(format!(
            "signature does not verify with key {key} ({key_id})"
        )),
        Verification::UnknownKey { key_id } => SignatureStatus::UnknownKey(key_id),
    })
}

/// The canonical bytes a snapshot signature covers.
pub fn signed_payload(snapshot: &Snapshot) -> Vec<u8> {
    use prost::Message;
    let mut rec = snapshot.to_record();
    rec.signature.clear();
    rec.signing_key_id.clear();
    rec.tree_id = None;
    rec.branch_name.clear();
    rec.encode_to_vec()
}

/// `SHA256:` + unpadded base64 of the SHA-256 of the public key (the
/// server's `SigningKey.fingerprint`).
pub fn fingerprint(public_key: &[u8; 32]) -> String {
    let digest = Sha256::digest(public_key);
    format!(
        "SHA256:{}",
        base64::engine::general_purpose::STANDARD_NO_PAD.encode(digest)
    )
}

/// Decode a standard-base64 32-byte public key (the server's JSON form).
pub fn decode_public_key_base64(text: &str) -> Result<[u8; 32]> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(text.trim())
        .map_err(|e| SdkError::Serialization(format!("public key: {e}")))?;
    <[u8; 32]>::try_from(bytes.as_slice()).map_err(|_| {
        SdkError::Serialization(format!(
            "public key: expected 32 bytes, got {}",
            bytes.len()
        ))
    })
}

/// Whether `snapshot.signature` is a valid signature of its payload by
/// `public_key` (false for unsigned snapshots and malformed keys).
pub fn verify_signature(snapshot: &Snapshot, public_key: &[u8; 32]) -> bool {
    snapshot
        .signature
        .as_ref()
        .is_some_and(|sig| verify_bytes(&signed_payload(snapshot), &sig.signature, public_key))
}

/// Sign `snapshot` in place when signing is configured for the repository
/// (`[signing] key` set and `auto = true`); a no-op otherwise.
pub fn sign_snapshot(repo: &Repository, snapshot: &mut Snapshot) -> Result<()> {
    sign_snapshot_with(repo, snapshot, false)
}

/// Like [`sign_snapshot`]; `force` signs even without `auto` (`wt snapshot
/// --sign`), using the `default` key when none is configured.
pub fn sign_snapshot_with(repo: &Repository, snapshot: &mut Snapshot, force: bool) -> Result<()> {
    sign_configured(
        &repo.config().signing,
        &KeyStore::default_location(),
        snapshot,
        force,
    )
    .map(|_| ())
}

/// Apply a `[signing]` section: returns whether `snapshot` was signed.
/// `InvalidConfig` when signing is wanted but the key does not exist.
pub fn sign_configured(
    section: &SigningSection,
    keys: &KeyStore,
    snapshot: &mut Snapshot,
    force: bool,
) -> Result<bool> {
    match configured_key(section, keys, force)? {
        Some(key) => {
            key.sign(snapshot);
            Ok(true)
        }
        None => Ok(false),
    }
}

/// The key a `[signing]` section selects when signing is wanted (`auto`
/// with a key name, or `force`), `None` otherwise. `InvalidConfig` when
/// the key does not exist.
fn configured_key(
    section: &SigningSection,
    keys: &KeyStore,
    force: bool,
) -> Result<Option<LocalKey>> {
    let configured = section.key.trim();
    if !force && !(section.auto && !configured.is_empty()) {
        return Ok(None);
    }
    let name = if configured.is_empty() {
        DEFAULT_KEY
    } else {
        configured
    };
    keys.load(name).map(Some).map_err(|e| match e {
        SdkError::InvalidArgument(msg) => SdkError::InvalidConfig(msg),
        other => other,
    })
}

/// Result of verifying a snapshot signature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Verification {
    /// The snapshot carries no signature.
    Unsigned,
    /// The signature verifies against a trusted key.
    Valid {
        key_id: String,
        /// Which key: a local key name or `account/name` for server keys.
        key: String,
        fingerprint: String,
    },
    /// The key is known but the signature does not verify (the snapshot or
    /// its signature was altered).
    Invalid { key_id: String, key: String },
    /// No trusted key has this id.
    UnknownKey { key_id: String },
}

impl Verification {
    pub fn is_valid(&self) -> bool {
        matches!(self, Self::Valid { .. })
    }
}

/// A public key trusted for verification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedKey {
    /// Label shown to the user (local key name, `account/name`).
    pub label: String,
    /// Key ids this key answers to (server id and/or fingerprint).
    pub ids: Vec<String>,
    pub public_key: [u8; 32],
}

impl TrustedKey {
    /// A trusted key from local metadata (fingerprint plus registered id).
    pub fn from_info(info: &KeyInfo) -> Result<Self> {
        let mut ids = vec![info.fingerprint.clone()];
        if !info.key_id.is_empty() {
            ids.push(info.key_id.clone());
        }
        Ok(Self {
            label: info.name.clone(),
            ids,
            public_key: info.public_key_bytes()?,
        })
    }
}

/// Verify `snapshot` against `keys`, matching its key id.
pub fn verify_with_keys(snapshot: &Snapshot, keys: &[TrustedKey]) -> Verification {
    verify_payload(
        snapshot.signature.as_ref(),
        || signed_payload(snapshot),
        keys,
    )
}

/// Verify `snapshot` against the keys in `<wt home>/keys/`.
pub fn verify_with_local_keys(snapshot: &Snapshot) -> Result<Verification> {
    KeyStore::default_location().verify(snapshot)
}

/// Match `sig` against `keys` and check it over `payload`.
fn verify_payload(
    sig: Option<&SnapshotSignature>,
    payload: impl FnOnce() -> Vec<u8>,
    keys: &[TrustedKey],
) -> Verification {
    let Some(sig) = sig else {
        return Verification::Unsigned;
    };
    let key_id = sig.key_id.clone();
    match keys.iter().find(|k| k.ids.contains(&key_id)) {
        None => Verification::UnknownKey { key_id },
        Some(k) if verify_bytes(&payload(), &sig.signature, &k.public_key) => Verification::Valid {
            key_id,
            key: k.label.clone(),
            fingerprint: fingerprint(&k.public_key),
        },
        Some(k) => Verification::Invalid {
            key_id,
            key: k.label.clone(),
        },
    }
}

/// Whether `signature` is a valid Ed25519 signature of `payload`.
fn verify_bytes(payload: &[u8], signature: &[u8], public_key: &[u8; 32]) -> bool {
    let Ok(bytes) = <[u8; SIGNATURE_LEN]>::try_from(signature) else {
        return false;
    };
    let Ok(key) = VerifyingKey::from_bytes(public_key) else {
        return false;
    };
    key.verify(payload, &Signature::from_bytes(&bytes)).is_ok()
}

// ---------------------------------------------------------------------------
// v0.7 trust (ENGINEERING_PLAN §15.8): signed tags
//
// The signed payload of a tag is the deterministic protobuf encoding of its
// `TagRecord` with `signature` and `signing_key_id` cleared. An empty
// message is encoded as absent (the server does not distinguish "" from no
// message). The Go server rebuilds the same bytes
// (`internal/signing.TagPayload`); both sides pin a shared test vector.

/// The unsigned wire record of `tag` (signature fields empty).
pub(crate) fn tag_record(tag: &TagInfo) -> wire::TagRecord {
    wire::TagRecord {
        name: tag.name.clone(),
        target: Some(wire::SnapshotId {
            value: tag.target.clone(),
        }),
        message: tag.message.clone().filter(|m| !m.is_empty()),
        tagger: tag.tagger.clone(),
        created_at: Some(prost_types::Timestamp {
            seconds: tag.created_at.timestamp(),
            nanos: tag.created_at.timestamp_subsec_nanos() as i32,
        }),
        signature: Vec::new(),
        signing_key_id: String::new(),
    }
}

/// The canonical bytes a tag signature covers.
pub fn tag_signed_payload(tag: &TagInfo) -> Vec<u8> {
    use prost::Message;
    tag_record(tag).encode_to_vec()
}

/// Whether `tag.signature` is a valid signature by `public_key`.
pub fn verify_tag_signature(tag: &TagInfo, public_key: &[u8; 32]) -> bool {
    tag.signature
        .as_ref()
        .is_some_and(|sig| verify_bytes(&tag_signed_payload(tag), &sig.signature, public_key))
}

/// Verify `tag` against `keys`, matching its key id.
pub fn verify_tag_with_keys(tag: &TagInfo, keys: &[TrustedKey]) -> Verification {
    verify_payload(tag.signature.as_ref(), || tag_signed_payload(tag), keys)
}

/// Sign `tag` in place when `[signing]` asks for it (`auto` with a key) or
/// `force` (`wt tag --sign`); returns whether it was signed.
pub fn sign_tag_configured(
    section: &SigningSection,
    keys: &KeyStore,
    tag: &mut TagInfo,
    force: bool,
) -> Result<bool> {
    match configured_key(section, keys, force)? {
        Some(key) => {
            key.sign_tag(tag);
            Ok(true)
        }
        None => Ok(false),
    }
}

/// Create tag `name` at `rev` (default HEAD) like
/// [`Repository::create_tag`], signed when `[signing] auto` is on or
/// `sign` is set. The key is loaded first, so a missing key fails before
/// the tag exists.
pub fn create_tag(
    repo: &Repository,
    tree: &str,
    name: &str,
    rev: Option<&str>,
    message: Option<&str>,
    sign: bool,
) -> Result<TagInfo> {
    let key = configured_key(&repo.config().signing, &KeyStore::default_location(), sign)?;
    let mut tag = repo.create_tag(tree, name, rev, message)?;
    if let Some(key) = key {
        key.sign_tag(&mut tag);
        repo.put_tag(tree, &tag)?;
    }
    Ok(tag)
}

/// Verify `tag` against the keys in `<wt home>/keys/`.
pub fn verify_tag_with_local_keys(tag: &TagInfo) -> Result<Verification> {
    KeyStore::default_location().verify_tag(tag)
}

// Release integrity helpers (`wt release verify`, §15.8).

/// Lowercase hex SHA-256 of `data`.
pub fn sha256_hex(data: &[u8]) -> String {
    encode_hex(&Sha256::digest(data))
}

/// Standard (padded) base64, as Go encodes `[]byte` in JSON.
pub fn encode_base64(data: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(data)
}

/// Decode standard (padded or unpadded) base64, as Go encodes `[]byte`.
pub fn decode_base64(text: &str) -> Result<Vec<u8>> {
    let text = text.trim();
    base64::engine::general_purpose::STANDARD
        .decode(text)
        .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(text))
        .map_err(|e| SdkError::Serialization(format!("base64: {e}")))
}

/// Parse a SHA256SUMS file (`<hex>  <name>` or `<hex> *<name>` lines) into
/// (name, lowercase hex) pairs; blank lines and `#` comments are skipped,
/// malformed lines are errors.
pub fn parse_checksums(text: &str) -> Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let parsed = line.split_once(' ').and_then(|(sum, rest)| {
            let name = rest
                .strip_prefix(' ')
                .or_else(|| rest.strip_prefix('*'))
                .unwrap_or(rest);
            let ok = sum.len() == 64 && sum.chars().all(|c| c.is_ascii_hexdigit());
            (ok && !name.is_empty()).then(|| (name.to_string(), sum.to_ascii_lowercase()))
        });
        match parsed {
            Some(pair) => out.push(pair),
            None => {
                return Err(SdkError::Serialization(format!(
                    "checksums line {}: expected '<sha256>  <name>'",
                    i + 1
                )))
            }
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Keys

/// Public metadata of a local key (`<name>.pub`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyInfo {
    pub name: String,
    #[serde(default = "default_algorithm")]
    pub algorithm: String,
    /// Hex-encoded 32-byte public key.
    pub public_key: String,
    pub fingerprint: String,
    /// Server key id once registered ("" = unregistered).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub key_id: String,
    /// Server the key is registered with.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub server: String,
    pub created_at: DateTime<Utc>,
}

fn default_algorithm() -> String {
    ALGORITHM.into()
}

impl KeyInfo {
    pub fn public_key_bytes(&self) -> Result<[u8; 32]> {
        decode_hex32(&self.public_key).ok_or_else(|| {
            SdkError::Serialization(format!("key {}: malformed public key", self.name))
        })
    }

    /// Standard (padded) base64 of the public key, as the server expects.
    pub fn public_key_base64(&self) -> Result<String> {
        Ok(base64::engine::general_purpose::STANDARD.encode(self.public_key_bytes()?))
    }

    /// Key id put into signatures: the server id, else the fingerprint.
    pub fn signing_id(&self) -> &str {
        if self.key_id.is_empty() {
            &self.fingerprint
        } else {
            &self.key_id
        }
    }
}

/// A local private key.
pub struct LocalKey {
    pub info: KeyInfo,
    secret: SigningKey,
}

impl std::fmt::Debug for LocalKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalKey")
            .field("info", &self.info)
            .finish_non_exhaustive()
    }
}

impl LocalKey {
    /// A key from a 32-byte seed (metadata freshly derived, unregistered).
    pub fn from_seed(name: &str, seed: &[u8; 32]) -> Self {
        let secret = SigningKey::from_bytes(seed);
        let public = secret.verifying_key().to_bytes();
        Self {
            info: KeyInfo {
                name: name.to_string(),
                algorithm: ALGORITHM.into(),
                public_key: encode_hex(&public),
                fingerprint: fingerprint(&public),
                key_id: String::new(),
                server: String::new(),
                created_at: Utc::now(),
            },
            secret,
        }
    }

    pub fn public_key(&self) -> [u8; 32] {
        self.secret.verifying_key().to_bytes()
    }

    /// Sign `snapshot` in place (replacing any earlier signature).
    pub fn sign(&self, snapshot: &mut Snapshot) {
        snapshot.signature = None;
        let sig = self.secret.sign(&signed_payload(snapshot));
        snapshot.signature = Some(SnapshotSignature {
            key_id: self.info.signing_id().to_string(),
            signature: sig.to_bytes().to_vec(),
        });
    }

    /// Sign `tag` in place (replacing any earlier signature).
    pub fn sign_tag(&self, tag: &mut TagInfo) {
        let sig = self.secret.sign(&tag_signed_payload(tag));
        tag.signature = Some(SnapshotSignature {
            key_id: self.info.signing_id().to_string(),
            signature: sig.to_bytes().to_vec(),
        });
    }
}

/// The directory of local keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyStore {
    dir: PathBuf,
}

impl KeyStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// `<wt home>/keys`.
    pub fn default_location() -> Self {
        Self::new(crate::config::wt_home().join("keys"))
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn key_path(&self, name: &str) -> PathBuf {
        self.dir.join(format!("{name}.key"))
    }

    fn pub_path(&self, name: &str) -> PathBuf {
        self.dir.join(format!("{name}.pub"))
    }

    pub fn exists(&self, name: &str) -> bool {
        self.key_path(name).is_file()
    }

    /// Generate a new random key. `InvalidArgument` if `name` is taken and
    /// not `force`.
    pub fn generate(&self, name: &str, force: bool) -> Result<KeyInfo> {
        let secret = SigningKey::generate(&mut rand_core::OsRng);
        self.import_seed(name, &secret.to_bytes(), force)
    }

    /// Store a key from a known seed (tests, imports).
    pub fn import_seed(&self, name: &str, seed: &[u8; 32], force: bool) -> Result<KeyInfo> {
        validate_key_name(name)?;
        if self.exists(name) && !force {
            return Err(SdkError::InvalidArgument(format!(
                "key '{name}' already exists (use --force to replace it)"
            )));
        }
        create_private_dir(&self.dir)?;
        let key = LocalKey::from_seed(name, seed);
        write_secret(
            &self.key_path(name),
            format!("{}\n", encode_hex(seed)).as_bytes(),
        )?;
        self.write_info(&key.info)?;
        Ok(key.info)
    }

    fn write_info(&self, info: &KeyInfo) -> Result<()> {
        let text = toml::to_string_pretty(info)?;
        crate::repo::fsutil::write_atomic(&self.pub_path(&info.name), text.as_bytes())
    }

    /// Public metadata of `name` (`InvalidArgument` if there is no such key).
    pub fn info(&self, name: &str) -> Result<KeyInfo> {
        validate_key_name(name)?;
        let path = self.pub_path(name);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // A seed without metadata (copied key): derive it.
                return self.load(name).map(|k| k.info);
            }
            Err(e) => return Err(e.into()),
        };
        toml::from_str(&text)
            .map_err(|e| SdkError::Serialization(format!("{}: {}", path.display(), e.message())))
    }

    /// Load the private key `name`.
    pub fn load(&self, name: &str) -> Result<LocalKey> {
        validate_key_name(name)?;
        let path = self.key_path(name);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(SdkError::InvalidArgument(format!(
                    "signing key '{name}' not found in {} (run `wt identity generate {name}`)",
                    self.dir.display()
                )));
            }
            Err(e) => return Err(e.into()),
        };
        let seed = decode_hex32(text.trim()).ok_or_else(|| {
            SdkError::Serialization(format!("{}: malformed key seed", path.display()))
        })?;
        let mut key = LocalKey::from_seed(name, &seed);
        if let Ok(text) = fs::read_to_string(self.pub_path(name)) {
            if let Ok(info) = toml::from_str::<KeyInfo>(&text) {
                // Keep registration and creation time; the public key is
                // always derived from the seed.
                key.info.key_id = info.key_id;
                key.info.server = info.server;
                key.info.created_at = info.created_at;
            }
        }
        Ok(key)
    }

    /// All local keys, sorted by name.
    pub fn list(&self) -> Result<Vec<KeyInfo>> {
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let mut out = Vec::new();
        for entry in entries {
            let path = entry?.path();
            if path.extension().and_then(|e| e.to_str()) != Some("key") {
                continue;
            }
            let Some(name) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if validate_key_name(name).is_ok() {
                out.push(self.info(name)?);
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    /// Record the server registration of `name`.
    pub fn set_registration(&self, name: &str, key_id: &str, server: &str) -> Result<KeyInfo> {
        let mut info = self.load(name)?.info;
        info.key_id = key_id.to_string();
        info.server = server.to_string();
        self.write_info(&info)?;
        Ok(info)
    }

    /// Forget the registration whose server id is `key_id` (after a
    /// revoke); returns the affected key, if any.
    pub fn clear_registration(&self, key_id: &str) -> Result<Option<KeyInfo>> {
        for info in self.list()? {
            if !key_id.is_empty() && info.key_id == key_id {
                return self.set_registration(&info.name, "", "").map(Some);
            }
        }
        Ok(None)
    }

    /// Every local key as a [`TrustedKey`].
    pub fn trusted(&self) -> Result<Vec<TrustedKey>> {
        self.list()?.iter().map(TrustedKey::from_info).collect()
    }

    /// Verify `snapshot` against the local keys.
    pub fn verify(&self, snapshot: &Snapshot) -> Result<Verification> {
        if snapshot.signature.is_none() {
            return Ok(Verification::Unsigned);
        }
        Ok(verify_with_keys(snapshot, &self.trusted()?))
    }

    /// Verify `tag` against the local keys.
    pub fn verify_tag(&self, tag: &TagInfo) -> Result<Verification> {
        if tag.signature.is_none() {
            return Ok(Verification::Unsigned);
        }
        Ok(verify_tag_with_keys(tag, &self.trusted()?))
    }
}

/// Key names: 1-64 of `[A-Za-z0-9._-]`, not starting with '.' or '-'.
pub fn validate_key_name(name: &str) -> Result<()> {
    let ok = !name.is_empty()
        && name.len() <= 64
        && !name.starts_with(['.', '-'])
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if ok {
        Ok(())
    } else {
        Err(SdkError::InvalidArgument(format!(
            "invalid key name {name:?}: use 1-64 letters, digits, '.', '_' or '-'"
        )))
    }
}

fn create_private_dir(dir: &Path) -> Result<()> {
    fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Write a secret file readable by the owner only (replacing `path`).
fn write_secret(path: &Path, data: &[u8]) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    let mut opts = OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts.open(path)?;
    file.write_all(data)?;
    file.flush()?;
    Ok(())
}

fn encode_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(DIGITS[usize::from(b >> 4)] as char);
        out.push(DIGITS[usize::from(b & 0xf)] as char);
    }
    out
}

fn decode_hex32(text: &str) -> Option<[u8; 32]> {
    let bytes = text.as_bytes();
    if bytes.len() != 64 {
        return None;
    }
    let nibble = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = nibble(bytes[2 * i])? << 4 | nibble(bytes[2 * i + 1])?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Dependency;
    use crate::store::ObjectHash;

    /// Seed of the cross-language test vector: bytes 0x00..=0x1f.
    fn vector_seed() -> [u8; 32] {
        std::array::from_fn(|i| i as u8)
    }

    /// The fixed record of the cross-language test vector (mirrored in
    /// services/server-go/internal/signing/signing_test.go).
    fn vector_snapshot() -> Snapshot {
        Snapshot {
            id: "0f8fad5b-d9cb-469f-a165-70867728950e".into(),
            tree_id: "7c9e6679-7425-40de-944b-e07fc1f90ae7".into(),
            branch: "main".into(),
            parents: vec!["6fa459ea-ee8a-4ca4-894e-db77e160355e".into()],
            manifest: ObjectHash::from_bytes([0x11; 32]),
            author: "Ada Lovelace <ada@example.com>".into(),
            message: "Sign the analytical engine\n\nBody line.".into(),
            created_at: DateTime::parse_from_rfc3339("2026-01-02T03:04:05.123456789Z")
                .expect("timestamp")
                .with_timezone(&Utc),
            auto_generated: false,
            dependencies: vec![Dependency {
                tenant: String::new(),
                tree: "docs".into(),
                branch: "main".into(),
                title: "Document the engine".into(),
                details: String::new(),
                priority: "high".into(),
                blocking: true,
                linked: false,
            }],
            reverts: Some("9b2e2b3a-1d4c-4f6e-8a7b-0c1d2e3f4a5b".into()),
            signature: None,
        }
    }

    // Cross-language test vector: the Go server asserts the same values
    // (services/server-go/internal/signing/signing_test.go).
    const VECTOR_PUBLIC_KEY: &str =
        "03a107bff3ce10be1d70dd18e74bc09967e4d6309ba50d5f1ddc8664125531b8";
    const VECTOR_FINGERPRINT: &str = "SHA256:Vkdap1RjR0wChd9dvyvKtz2mUTWIOem3dIGy6rEHcIw";
    const VECTOR_PAYLOAD: &str = concat!(
        "0a260a2430663866616435622d643963622d343639662d613136352d37303836",
        "373732383935306522260a2436666134353965612d656538612d346361342d38",
        "3934652d6462373765313630333535652a220a20111111111111111111111111",
        "1111111111111111111111111111111111111111321e416461204c6f76656c61",
        "6365203c616461406578616d706c652e636f6d3e3a265369676e207468652061",
        "6e616c79746963616c20656e67696e650a0a426f6479206c696e652e420b08a5",
        "ebdcca0610959aef3a52291204646f63731a046d61696e2213446f63756d656e",
        "742074686520656e67696e6532046869676838015a260a243962326532623361",
        "2d316434632d346636652d386137622d306331643265336634613562",
    );
    const VECTOR_SIGNATURE: &str = concat!(
        "8905b39afeb6a0a33d757233f0a2574dcde46b4bef9812ddc72d76a4feadf778",
        "7263261785f4eb5713e59fe3fa8d8f7df3b097a7b92c201031fbe0a374f2e702",
    );

    #[test]
    fn cross_language_vector() {
        let key = LocalKey::from_seed("vector", &vector_seed());
        assert_eq!(encode_hex(&key.public_key()), VECTOR_PUBLIC_KEY);
        assert_eq!(key.info.fingerprint, VECTOR_FINGERPRINT);
        let mut snap = vector_snapshot();
        // A stale signature is not part of the payload.
        snap.signature = Some(SnapshotSignature {
            key_id: "stale".into(),
            signature: vec![1, 2, 3],
        });
        assert_eq!(encode_hex(&signed_payload(&snap)), VECTOR_PAYLOAD);
        key.sign(&mut snap);
        let sig = snap.signature.as_ref().expect("signed");
        assert_eq!(encode_hex(&sig.signature), VECTOR_SIGNATURE);
        assert_eq!(sig.key_id, VECTOR_FINGERPRINT);
    }

    #[test]
    fn payload_ignores_tree_branch_and_signature() {
        let base = signed_payload(&vector_snapshot());
        let mut other = vector_snapshot();
        other.tree_id = "server-tree-id".into();
        other.branch = "feature".into();
        other.signature = Some(SnapshotSignature {
            key_id: "k".into(),
            signature: vec![0; 64],
        });
        assert_eq!(signed_payload(&other), base);
        other.message.push('!');
        assert_ne!(signed_payload(&other), base);
    }

    fn trusted(key: &LocalKey) -> Vec<TrustedKey> {
        vec![TrustedKey::from_info(&key.info).expect("trusted")]
    }

    #[test]
    fn sign_and_verify_round_trip() {
        let key = LocalKey::from_seed("me", &[7; 32]);
        let mut snap = vector_snapshot();
        assert_eq!(
            verify_with_keys(&snap, &trusted(&key)),
            Verification::Unsigned
        );
        key.sign(&mut snap);
        assert!(verify_signature(&snap, &key.public_key()));
        let v = verify_with_keys(&snap, &trusted(&key));
        assert!(v.is_valid(), "{v:?}");
        assert_eq!(
            v,
            Verification::Valid {
                key_id: key.info.fingerprint.clone(),
                key: "me".into(),
                fingerprint: key.info.fingerprint.clone(),
            }
        );
        // Survives the wire record and a JSON round trip.
        let back = Snapshot::from_record(&snap.to_record()).expect("record");
        assert!(verify_signature(&back, &key.public_key()));
        let json: Snapshot =
            serde_json::from_str(&serde_json::to_string(&snap).expect("json")).expect("parse");
        assert!(verify_signature(&json, &key.public_key()));
    }

    #[test]
    fn tampering_is_detected() {
        let key = LocalKey::from_seed("me", &[7; 32]);
        let mut signed = vector_snapshot();
        key.sign(&mut signed);
        let keys = trusted(&key);
        type Tamper = Box<dyn Fn(&mut Snapshot)>;
        let tampers: Vec<Tamper> = vec![
            Box::new(|s| s.message.push('x')),
            Box::new(|s| s.author = "Mallory".into()),
            Box::new(|s| s.parents.clear()),
            Box::new(|s| s.manifest = ObjectHash::from_bytes([0x22; 32])),
            Box::new(|s| s.created_at += chrono::Duration::nanoseconds(1)),
            Box::new(|s| s.dependencies.clear()),
            Box::new(|s| s.reverts = None),
            Box::new(|s| s.auto_generated = true),
            Box::new(|s| s.id = "0f8fad5b-d9cb-469f-a165-70867728950f".into()),
            Box::new(|s| {
                if let Some(sig) = s.signature.as_mut() {
                    sig.signature[0] ^= 1;
                }
            }),
            Box::new(|s| {
                if let Some(sig) = s.signature.as_mut() {
                    sig.signature.truncate(10);
                }
            }),
        ];
        for (i, tamper) in tampers.iter().enumerate() {
            let mut snap = signed.clone();
            tamper(&mut snap);
            assert!(
                matches!(verify_with_keys(&snap, &keys), Verification::Invalid { .. }),
                "tamper #{i} not detected"
            );
        }
        // A different key under the same id does not verify either.
        let other = LocalKey::from_seed("other", &[8; 32]);
        assert!(!verify_signature(&signed, &other.public_key()));
        // An unknown key id.
        let mut snap = signed.clone();
        if let Some(sig) = snap.signature.as_mut() {
            sig.key_id = "nope".into();
        }
        assert_eq!(
            verify_with_keys(&snap, &keys),
            Verification::UnknownKey {
                key_id: "nope".into()
            }
        );
    }

    #[test]
    fn registered_keys_sign_with_their_server_id() {
        let mut key = LocalKey::from_seed("me", &[9; 32]);
        key.info.key_id = "key-123".into();
        let mut snap = vector_snapshot();
        key.sign(&mut snap);
        assert_eq!(snap.signature.as_ref().expect("signed").key_id, "key-123");
        assert!(verify_with_keys(&snap, &trusted(&key)).is_valid());
        // Older snapshots signed with the fingerprint still verify.
        let mut unregistered = LocalKey::from_seed("me", &[9; 32]);
        unregistered.info.key_id.clear();
        let mut old = vector_snapshot();
        unregistered.sign(&mut old);
        assert!(verify_with_keys(&old, &trusted(&key)).is_valid());
    }

    #[test]
    fn key_store_generate_list_load() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = KeyStore::new(dir.path().join("keys"));
        assert!(store.list().expect("list").is_empty());
        let a = store.generate("work", false).expect("generate");
        assert_eq!(a.algorithm, ALGORITHM);
        assert!(a.fingerprint.starts_with("SHA256:"));
        assert_eq!(a.signing_id(), a.fingerprint);
        store
            .import_seed("ci.bot", &vector_seed(), false)
            .expect("import");
        let names: Vec<String> = store
            .list()
            .expect("list")
            .into_iter()
            .map(|k| k.name)
            .collect();
        assert_eq!(names, ["ci.bot", "work"]);
        let loaded = store.load("work").expect("load");
        assert_eq!(encode_hex(&loaded.public_key()), a.public_key);
        assert_eq!(store.info("work").expect("info"), a);
        assert_eq!(
            store.info("ci.bot").expect("info").public_key,
            VECTOR_PUBLIC_KEY
        );
        let b64 = a.public_key_base64().expect("b64");
        assert_eq!(b64.len(), 44);
        assert_eq!(
            encode_hex(&decode_public_key_base64(&b64).expect("decode")),
            a.public_key
        );
        assert!(decode_public_key_base64("AAAA").is_err());
        assert!(decode_public_key_base64("not base64!").is_err());

        // Existing names need --force; force replaces the key.
        let err = store.generate("work", false).expect_err("exists");
        assert!(err.to_string().contains("already exists"), "{err}");
        let b = store.generate("work", true).expect("force");
        assert_ne!(a.public_key, b.public_key);

        // Missing keys and bad names.
        let err = store.load("nope").expect_err("missing");
        assert!(
            err.to_string().contains("wt identity generate nope"),
            "{err}"
        );
        for bad in ["", ".hidden", "-x", "a/b", "a b", &"x".repeat(65)] {
            assert!(store.generate(bad, false).is_err(), "{bad:?}");
        }
        // The seed file holds the hex seed.
        let seed = fs::read_to_string(store.dir().join("ci.bot.key")).expect("seed");
        assert_eq!(seed.trim(), encode_hex(&vector_seed()));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(store.dir().join("work.key"))
                .expect("meta")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn key_store_registration_and_verify() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = KeyStore::new(dir.path());
        store.import_seed("me", &[3; 32], false).expect("import");
        let info = store
            .set_registration("me", "key-1", "http://localhost:7420")
            .expect("register");
        assert_eq!(info.signing_id(), "key-1");
        let key = store.load("me").expect("load");
        assert_eq!(key.info.key_id, "key-1");
        assert_eq!(key.info.server, "http://localhost:7420");

        let mut snap = vector_snapshot();
        assert_eq!(store.verify(&snap).expect("verify"), Verification::Unsigned);
        key.sign(&mut snap);
        assert!(store.verify(&snap).expect("verify").is_valid());

        let cleared = store.clear_registration("key-1").expect("clear");
        assert_eq!(cleared.map(|k| k.key_id), Some(String::new()));
        assert_eq!(store.clear_registration("key-1").expect("clear"), None);
        // Signed with the (now forgotten) server id: unknown locally.
        assert!(matches!(
            store.verify(&snap).expect("verify"),
            Verification::UnknownKey { .. }
        ));
    }

    #[test]
    fn signing_section_controls_signing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = KeyStore::new(dir.path());
        let off = SigningSection::default();
        let mut snap = vector_snapshot();
        assert!(!sign_configured(&off, &store, &mut snap, false).expect("off"));
        assert!(snap.signature.is_none());

        // auto without a key name stays off.
        let auto_no_key = SigningSection {
            key: String::new(),
            auto: true,
        };
        assert!(!sign_configured(&auto_no_key, &store, &mut snap, false).expect("off"));

        // A configured key that does not exist is a configuration error.
        let auto = SigningSection {
            key: "work".into(),
            auto: true,
        };
        let err = sign_configured(&auto, &store, &mut snap, false).expect_err("missing");
        assert!(matches!(err, SdkError::InvalidConfig(_)), "{err:?}");

        store.import_seed("work", &[5; 32], false).expect("import");
        assert!(sign_configured(&auto, &store, &mut snap, false).expect("auto"));
        assert!(store.verify(&snap).expect("verify").is_valid());

        // key set but auto off: only a forced (`--sign`) snapshot is signed.
        let manual = SigningSection {
            key: "work".into(),
            auto: false,
        };
        let mut snap = vector_snapshot();
        assert!(!sign_configured(&manual, &store, &mut snap, false).expect("manual"));
        assert!(sign_configured(&manual, &store, &mut snap, true).expect("forced"));
        assert!(snap.signature.is_some());

        // Forced without a configured key uses the default key.
        let mut snap = vector_snapshot();
        let err = sign_configured(&off, &store, &mut snap, true).expect_err("no default");
        assert!(err.to_string().contains(DEFAULT_KEY), "{err}");
        store
            .import_seed(DEFAULT_KEY, &[6; 32], false)
            .expect("import");
        assert!(sign_configured(&off, &store, &mut snap, true).expect("default"));
        let fp = store.info(DEFAULT_KEY).expect("info").fingerprint;
        assert_eq!(snap.signature.as_ref().expect("signed").key_id, fp);
    }

    #[test]
    fn signing_config_section_parses() {
        let mut cfg = crate::config::WorktreeConfig::new("web", "main");
        let text = toml::to_string_pretty(&cfg).expect("toml");
        assert!(!text.contains("[signing]"), "{text}");
        cfg.signing = SigningSection {
            key: "work".into(),
            auto: true,
        };
        let text = toml::to_string_pretty(&cfg).expect("toml");
        assert!(text.contains("[signing]"), "{text}");
        let back: crate::config::WorktreeConfig = toml::from_str(&text).expect("parse");
        assert_eq!(back.signing, cfg.signing);
        let partial: SigningSection = toml::from_str("key = \"k\"").expect("partial");
        assert_eq!(
            partial,
            SigningSection {
                key: "k".into(),
                auto: false
            }
        );
    }

    // ---- v0.7 trust: signed tags --------------------------------------

    /// The fixed tag of the cross-language tag vector (mirrored in
    /// services/server-go/internal/signing/tags_test.go).
    fn vector_tag() -> TagInfo {
        TagInfo {
            name: "v1.0.0".into(),
            target: "0f8fad5b-d9cb-469f-a165-70867728950e".into(),
            message: Some("Release 1.0.0\n\nSigned.".into()),
            tagger: "Ada Lovelace <ada@example.com>".into(),
            created_at: DateTime::parse_from_rfc3339("2026-01-02T03:04:05.123456789Z")
                .expect("timestamp")
                .with_timezone(&Utc),
            signature: None,
        }
    }

    // Cross-language tag vector (same seed and fingerprint as above); the
    // Go server asserts the same values.
    const TAG_VECTOR_PAYLOAD: &str = concat!(
        "0a0676312e302e3012260a2430663866616435622d643963622d343639662d61",
        "3136352d3730383637373238393530651a1652656c6561736520312e302e300a",
        "0a5369676e65642e221e416461204c6f76656c616365203c616461406578616d",
        "706c652e636f6d3e2a0b08a5ebdcca0610959aef3a",
    );
    const TAG_VECTOR_SIGNATURE: &str = concat!(
        "0dc28f052fa81fe86ff9ad4e67f8a282e4aa97987665958ce5b4cc5d3a3b828a",
        "fbad99d3824577adca5acd0732d9e3f39eb48a3f6e25c93664ccdf9179d93a0a",
    );

    #[test]
    fn tag_cross_language_vector() {
        let key = LocalKey::from_seed("vector", &vector_seed());
        let mut tag = vector_tag();
        tag.signature = Some(SnapshotSignature {
            key_id: "stale".into(),
            signature: vec![1, 2, 3],
        });
        let payload = encode_hex(&tag_signed_payload(&tag));
        key.sign_tag(&mut tag);
        let sig = tag.signature.as_ref().expect("signed");
        let signature = encode_hex(&sig.signature);
        assert_eq!(
            (payload.as_str(), signature.as_str()),
            (TAG_VECTOR_PAYLOAD, TAG_VECTOR_SIGNATURE)
        );
        assert_eq!(sig.key_id, VECTOR_FINGERPRINT);
    }

    #[test]
    fn tag_payload_normalizes_empty_message() {
        let mut a = vector_tag();
        a.message = None;
        let mut b = vector_tag();
        b.message = Some(String::new());
        assert_eq!(tag_signed_payload(&a), tag_signed_payload(&b));
        assert_ne!(tag_signed_payload(&a), tag_signed_payload(&vector_tag()));
    }

    #[test]
    fn tag_sign_verify_and_tamper() {
        let key = LocalKey::from_seed("me", &[7; 32]);
        let keys = trusted(&key);
        let mut tag = vector_tag();
        assert_eq!(verify_tag_with_keys(&tag, &keys), Verification::Unsigned);
        key.sign_tag(&mut tag);
        assert!(verify_tag_signature(&tag, &key.public_key()));
        assert!(verify_tag_with_keys(&tag, &keys).is_valid());
        // JSON round trip (the local tag file).
        let json: TagInfo =
            serde_json::from_str(&serde_json::to_string(&tag).expect("json")).expect("parse");
        assert!(verify_tag_with_keys(&json, &keys).is_valid());
        type Tamper = Box<dyn Fn(&mut TagInfo)>;
        let tampers: Vec<Tamper> = vec![
            Box::new(|t| t.name.push('x')),
            Box::new(|t| t.target = "other".into()),
            Box::new(|t| t.message = None),
            Box::new(|t| t.tagger = "Mallory".into()),
            Box::new(|t| t.created_at += chrono::Duration::nanoseconds(1)),
            Box::new(|t| {
                if let Some(sig) = t.signature.as_mut() {
                    sig.signature[3] ^= 1;
                }
            }),
        ];
        for (i, tamper) in tampers.iter().enumerate() {
            let mut t = tag.clone();
            tamper(&mut t);
            assert!(
                matches!(
                    verify_tag_with_keys(&t, &keys),
                    Verification::Invalid { .. }
                ),
                "tamper #{i} not detected"
            );
        }
        // Unsigned tags serialize without a signature field.
        let text = serde_json::to_string(&vector_tag()).expect("json");
        assert!(!text.contains("signature"), "{text}");
        let old: TagInfo = serde_json::from_str(
            r#"{"name":"v","target":"s","message":null,"tagger":"a","created_at":"2026-01-01T00:00:00Z"}"#,
        )
        .expect("old tag file");
        assert!(old.signature.is_none());
    }

    #[test]
    fn tag_signing_follows_the_signing_section() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = KeyStore::new(dir.path());
        let mut tag = vector_tag();
        let off = SigningSection::default();
        assert!(!sign_tag_configured(&off, &store, &mut tag, false).expect("off"));
        assert!(tag.signature.is_none());
        assert!(sign_tag_configured(&off, &store, &mut tag, true).is_err());
        store.import_seed("work", &[5; 32], false).expect("import");
        let auto = SigningSection {
            key: "work".into(),
            auto: true,
        };
        assert!(sign_tag_configured(&auto, &store, &mut tag, false).expect("auto"));
        assert!(store.verify_tag(&tag).expect("verify").is_valid());
        assert_eq!(
            store.verify_tag(&vector_tag()).expect("verify"),
            Verification::Unsigned
        );
    }

    #[test]
    fn release_integrity_helpers() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(decode_base64("AQID").expect("b64"), vec![1, 2, 3]);
        assert_eq!(decode_base64("AQI").expect("unpadded"), vec![1, 2]);
        assert!(decode_base64("!!").is_err());
        let sum = sha256_hex(b"x");
        let text = format!(
            "# comment\n{sum}  app.zip\n\n{} *b in.bin\r\n",
            sum.to_uppercase()
        );
        assert_eq!(
            parse_checksums(&text).expect("parse"),
            vec![
                ("app.zip".to_string(), sum.clone()),
                ("b in.bin".to_string(), sum.clone())
            ]
        );
        assert!(parse_checksums("nothex  a\n").is_err());
        assert!(parse_checksums(&format!("{sum}\n")).is_err());
    }

    #[test]
    fn hex_helpers() {
        let seed = vector_seed();
        assert_eq!(decode_hex32(&encode_hex(&seed)), Some(seed));
        assert_eq!(decode_hex32(&encode_hex(&seed).to_uppercase()), Some(seed));
        assert_eq!(decode_hex32("abc"), None);
        assert_eq!(decode_hex32(&"zz".repeat(32)), None);
    }
}
