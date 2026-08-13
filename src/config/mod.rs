//! Project configuration + the local credential vault.
//!
//! # Adding a new encrypted per-tenant artifact
//!
//! Encrypted per-tenant material (the service-account JWK map, log API keys, …)
//! lives in `.aic/` as a `<stem>.enc` / `<stem>.plain` pair — AES-256-GCM
//! under the in-memory DEK when encryption is on, or a mode-600 plaintext file
//! when the user opted out. The pair semantics, permissions, and gitignore
//! coverage are handled generically here so a new artifact does **not** repeat
//! the plumbing. To add one:
//!
//! 1. Add a [`VaultArtifact`] variant + its row in [`VaultArtifact::ALL`]
//!    (the file stem, e.g. `"log-keys"`). [`ProjectConfig::write_gitignore`],
//!    [`enable_encryption`], and [`disable_encryption`] iterate `ALL`, so the
//!    new stem is covered automatically.
//! 2. Define a typed, secret-redacting wrapper in your feature dir (see
//!    `crate::logs::LogKeyPair`) and serialise your per-tenant map to JSON
//!    bytes; the registry treats the payload as opaque
//!    ([`load_artifact_bytes`] / [`save_artifact_bytes`]).
//! 3. Reach it through the generic agent secret verbs
//!    (`Request::{PutSecret,GetSecret,RemoveSecret}` keyed by the artifact's
//!    [`VaultArtifact::kind`]) rather than adding a new verb triple.
//!
//! The JWK map (`keys`) predates this and keeps its bespoke `PutDek` /
//! `UnlockPlain` agent path (the DEK is derived client-side, not round-tripped
//! as an opaque secret), but still registers here for gitignore + the
//! enable/disable transitions.

pub mod crypto;
pub mod operator;
pub mod tenant;
pub mod wraps;

use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::Result;
pub use tenant::{CredentialSource, Provenance, Tenant, TenantTheme, tenant_file_name};

/// An encrypted per-tenant artifact stored as a `<stem>.enc` / `<stem>.plain`
/// pair in `.aic/`. The registry ([`VaultArtifact::ALL`]) drives gitignore
/// coverage and the encrypt/decrypt transitions so each new artifact is one
/// entry here plus feature-local typed code — see the module header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VaultArtifact {
    /// The service-account JWK map (`keys.enc` / `keys.plain`). Reached via the
    /// bespoke `PutDek` / `UnlockPlain` agent path, not the generic verbs.
    Jwks,
    /// The per-tenant log API key map (`log-keys.enc` / `log-keys.plain`).
    LogKeys,
    /// Per-tenant Trusted JWT Issuer private key records.
    JwtBearerKeys,
}

impl VaultArtifact {
    /// Every registered artifact. Iterated by gitignore + the encrypt/decrypt
    /// transitions so adding a variant here is all it takes for coverage.
    pub const ALL: &'static [VaultArtifact] = &[
        VaultArtifact::Jwks,
        VaultArtifact::LogKeys,
        VaultArtifact::JwtBearerKeys,
    ];

    /// The wire name used by the generic agent secret verbs (`kind` field).
    pub fn kind(self) -> &'static str {
        match self {
            VaultArtifact::Jwks => "keys",
            VaultArtifact::LogKeys => "log-keys",
            VaultArtifact::JwtBearerKeys => "jwt-bearer-keys",
        }
    }

    /// The on-disk file stem; the pair is `<stem>.enc` / `<stem>.plain`.
    pub fn file_stem(self) -> &'static str {
        match self {
            VaultArtifact::Jwks => "keys",
            VaultArtifact::LogKeys => "log-keys",
            VaultArtifact::JwtBearerKeys => "jwt-bearer-keys",
        }
    }

    /// Resolve a `kind` name (as sent over the agent protocol) back to its
    /// artifact.
    pub fn from_kind(kind: &str) -> Option<Self> {
        VaultArtifact::ALL
            .iter()
            .copied()
            .find(|artifact| artifact.kind() == kind)
    }

    fn enc_path(self) -> PathBuf {
        ProjectConfig::dir().join(format!("{}.enc", self.file_stem()))
    }

    fn plain_path(self) -> PathBuf {
        ProjectConfig::dir().join(format!("{}.plain", self.file_stem()))
    }
}

/// Load an artifact's cleartext map bytes from whichever on-disk form matches
/// the vault mode. Returns `None` when nothing has been stored yet.
///
/// - `Encrypted { dek }` decrypts `<stem>.enc`.
/// - `Plain` reads `<stem>.plain` verbatim.
///
/// The payload is opaque JSON bytes — callers own (de)serialisation of the
/// concrete per-tenant map.
pub fn load_artifact_bytes(
    artifact: VaultArtifact,
    dek: Option<&crypto::Dek>,
) -> Result<Option<Vec<u8>>> {
    match dek {
        Some(dek) => match load_optional_file(&artifact.enc_path())? {
            Some(data) => Ok(Some(crypto::decrypt_data(&data, dek)?)),
            None => Ok(None),
        },
        None => load_optional_file(&artifact.plain_path()),
    }
}

/// Persist an artifact's cleartext map bytes in the form the vault mode
/// dictates (encrypted `<stem>.enc` under `dek`, else plaintext `<stem>.plain`),
/// always at mode 600. The payload is treated as opaque JSON bytes.
pub fn save_artifact_bytes(
    artifact: VaultArtifact,
    bytes: &[u8],
    dek: Option<&crypto::Dek>,
) -> Result<()> {
    match dek {
        Some(dek) => {
            let enc = crypto::encrypt_data(bytes, dek)?;
            save_private_file(&artifact.enc_path(), &enc)
        }
        None => save_private_file(&artifact.plain_path(), bytes),
    }
}

/// Tenant + realm inferred from the directory a command was invoked in, when
/// that directory is inside `workspace/<tenant>/…`. Populated once at CLI
/// startup ([`crate::cli`]); consulted so `cd`-ing into a workspace subtree
/// targets the right tenant (and supplies the script `namespace` for a bare
/// name). Explicit args still win.
#[derive(Debug, Default, Clone)]
pub struct WorkspaceContext {
    pub tenant: Option<String>,
    /// The script namespace implied by the cwd, if inside one: `alpha`/`bravo`
    /// (AM realm) or `endpoint`/`schedule` (IDM kind).
    pub namespace: Option<String>,
}

static WORKSPACE_CONTEXT: OnceLock<WorkspaceContext> = OnceLock::new();

/// The detected workspace context (empty if not inside a workspace tree).
pub fn workspace_context() -> WorkspaceContext {
    WORKSPACE_CONTEXT.get().cloned().unwrap_or_default()
}

pub fn set_workspace_context(ctx: WorkspaceContext) {
    let _ = WORKSPACE_CONTEXT.set(ctx);
}

/// Walk up from `start` to the first ancestor containing a `.aic/`
/// directory — the project root. Lets commands run from any subdirectory.
pub fn find_project_root(start: &Path) -> Option<PathBuf> {
    let mut cur = start.canonicalize().ok()?;
    loop {
        if cur.join(".aic").is_dir() {
            return Some(cur);
        }
        if !cur.pop() {
            return None;
        }
    }
}

/// Infer tenant + script namespace from `cwd`'s position under
/// `<root>/workspace/…`: `workspace/<tenant>/…` → tenant;
/// `…/am/<realm>/…` → namespace `<realm>`; `…/idm/{endpoint,schedule}/…` →
/// namespace `endpoint`/`schedule`.
pub fn detect_workspace_context(root: &Path, cwd: &Path) -> WorkspaceContext {
    let mut ctx = WorkspaceContext::default();
    let (root, cwd) = match (root.canonicalize(), cwd.canonicalize()) {
        (Ok(r), Ok(c)) => (r, c),
        _ => return ctx,
    };
    if let Ok(rel) = cwd.strip_prefix(&root) {
        let mut comps = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned());
        if comps.next().as_deref() == Some("workspace") {
            ctx.tenant = comps.next().filter(|s| !s.is_empty());
            ctx.namespace = match comps.next().as_deref() {
                // am/<realm>/… → the realm is the namespace
                Some("am") => comps.next().filter(|r| r == "alpha" || r == "bravo"),
                // idm/{endpoint,schedule}/… → the IDM kind is the namespace
                Some("idm") => comps.next().filter(|k| k == "endpoint" || k == "schedule"),
                _ => None,
            };
        }
    }
    ctx
}

/// Decrypt the DEK with the user's password (via the password wrap recorded
/// in `wraps.toml`), then decrypt `keys.enc` with the DEK and parse the JWK
/// map. Returns `(dek, jwks)`. An empty map is yielded when `keys.enc`
/// doesn't exist yet — that's the "just after first-run setup" state.
pub fn unlock_with_password(
    password: &str,
) -> Result<(crypto::Dek, HashMap<String, serde_json::Value>)> {
    let wraps_file = wraps::WrapsFile::load()?
        .ok_or_else(|| crate::Error::Crypto("no wraps.toml on disk".into()))?;
    let pw_wrap = wraps_file
        .password_wrap()
        .ok_or_else(|| crate::Error::Crypto("no password wrap on file".into()))?;
    let (salt_b64, nonce_b64, ct_b64) = match pw_wrap {
        wraps::Wrap::Password {
            salt,
            nonce,
            ciphertext,
        } => (salt, nonce, ciphertext),
        _ => unreachable!(),
    };
    let salt: [u8; 16] = wraps::b64_decode(salt_b64)?
        .as_slice()
        .try_into()
        .map_err(|_| crate::Error::Crypto("password wrap: salt length".into()))?;
    let nonce: [u8; 12] = wraps::b64_decode(nonce_b64)?
        .as_slice()
        .try_into()
        .map_err(|_| crate::Error::Crypto("password wrap: nonce length".into()))?;
    let ct = wraps::b64_decode(ct_b64)?;
    let dek = crypto::unwrap_dek_with_password(password, &salt, &nonce, &ct)?;
    let jwks = decrypt_keys_file(&dek)?;
    Ok((dek, jwks))
}

pub fn decrypt_keys_file(dek: &crypto::Dek) -> Result<HashMap<String, serde_json::Value>> {
    decode_json_map(load_artifact_bytes(VaultArtifact::Jwks, Some(dek))?)
}

/// Encrypt + persist the JWK map using the in-memory DEK.
pub fn save_jwk_map(map: &HashMap<String, serde_json::Value>, dek: &crypto::Dek) -> Result<()> {
    save_artifact_bytes(VaultArtifact::Jwks, &serde_json::to_vec(map)?, Some(dek))
}

/// Deserialise an artifact's opaque map bytes into a `HashMap`, tolerating both
/// "nothing stored" (`None`) and an empty plaintext file as an empty map.
fn decode_json_map<V: serde::de::DeserializeOwned>(
    data: Option<Vec<u8>>,
) -> Result<HashMap<String, V>> {
    match data {
        Some(data) if !data.is_empty() => Ok(serde_json::from_slice(&data)?),
        Some(_) | None => Ok(HashMap::new()),
    }
}

/// Transition from "no encryption" to "encrypted": read `keys.plain`,
/// encrypt it and any `log-keys.plain`, write the parallel encrypted files,
/// delete the plaintext files, and flip `settings.toml` to
/// `encrypt_keys = true`. Called by the Settings screen when the user adds
/// the first auth factor while encryption is disabled.
///
/// `wraps.toml` is not touched here — callers must have already saved the
/// wrap that protects this DEK before calling this function, otherwise
/// `keys.enc` would be unreadable on next launch.
pub fn enable_encryption(dek: &crypto::Dek) -> Result<()> {
    for &artifact in VaultArtifact::ALL {
        match artifact {
            // The JWK map must always exist encrypted after this: first-run
            // installs never wrote keys.plain (no factors added yet), so
            // substitute an empty JSON map. Decrypting empty bytes would fail
            // the round-trip in `decrypt_keys_file`.
            VaultArtifact::Jwks => {
                let plain = match load_optional_file(&artifact.plain_path())? {
                    Some(bytes) if !bytes.is_empty() => bytes,
                    _ => b"{}".to_vec(),
                };
                save_artifact_bytes(artifact, &plain, Some(dek))?;
            }
            // Other artifacts are optional — only migrate a plaintext file if
            // one is present, leaving nothing behind if the feature was unused.
            _ => {
                if let Some(bytes) = load_optional_file(&artifact.plain_path())? {
                    let bytes = if bytes.is_empty() {
                        b"{}".to_vec()
                    } else {
                        bytes
                    };
                    save_artifact_bytes(artifact, &bytes, Some(dek))?;
                }
            }
        }
        let _ = fs::remove_file(artifact.plain_path());
    }
    let mut settings = Settings::load()?.unwrap_or_default();
    settings.encrypt_keys = true;
    settings.save()?;
    Ok(())
}

/// Transition from "encrypted" to "no encryption": decrypt `keys.enc` and
/// any `log-keys.enc` with `dek`, write the cleartext files at mode 600,
/// then delete the encrypted files and `wraps.toml`, and flip
/// `settings.toml` to `encrypt_keys = false`. Called by the last-factor
/// guard in the Settings screen.
pub fn disable_encryption(dek: &crypto::Dek) -> Result<()> {
    for &artifact in VaultArtifact::ALL {
        match artifact {
            // The JWK map always lands as plaintext (even if empty) — the app
            // expects keys.plain to exist in no-encryption mode.
            VaultArtifact::Jwks => {
                let bytes =
                    load_artifact_bytes(artifact, Some(dek))?.unwrap_or_else(|| b"{}".to_vec());
                save_artifact_bytes(artifact, &bytes, None)?;
            }
            // Other artifacts round-trip only when they had an encrypted file.
            _ => {
                if let Some(bytes) = load_artifact_bytes(artifact, Some(dek))? {
                    save_artifact_bytes(artifact, &bytes, None)?;
                }
            }
        }
        let _ = fs::remove_file(artifact.enc_path());
    }
    let _ = fs::remove_file(ProjectConfig::wraps_path());
    let mut settings = Settings::load()?.unwrap_or_default();
    settings.encrypt_keys = false;
    settings.save()?;
    Ok(())
}

/// Unlock via any enrolled security key in `wraps_file`. Builds an allowList
/// from every enrolled credential id and makes a single `getAssertion` call
/// (one PIN check, one tap) — the device returns the credential it actually
/// holds, and we use that to pick the right wrap to unwrap.
///
/// Errors if `wraps_file` has security-key wraps but no `security_key_hmac_salt`
/// (old schema — user needs to re-enrol).
pub fn unlock_with_security_key(
    wraps_file: &wraps::WrapsFile,
    pin: Option<&str>,
) -> Result<(crypto::Dek, HashMap<String, serde_json::Value>)> {
    let salt_b64 = wraps_file.security_key_hmac_salt.as_ref().ok_or_else(|| {
        crate::Error::Crypto(
            "wraps.toml has security keys enrolled but no security_key_hmac_salt — \
                 the salt was moved file-level; please remove and re-enrol your keys"
                .into(),
        )
    })?;
    let hmac_salt: [u8; crate::vault::security_key::HMAC_SALT_LEN] = wraps::b64_decode(salt_b64)?
        .as_slice()
        .try_into()
        .map_err(|_| crate::Error::Crypto("security_key_hmac_salt length".into()))?;

    let security_wraps: Vec<&wraps::Wrap> = wraps_file.security_key_wraps().collect();
    if security_wraps.is_empty() {
        return Err(crate::Error::Crypto(
            "no security key wraps enrolled".into(),
        ));
    }
    let credential_ids: Vec<Vec<u8>> = security_wraps
        .iter()
        .map(|w| match w {
            wraps::Wrap::SecurityKey { credential_id, .. } => wraps::b64_decode(credential_id),
            _ => unreachable!("filtered above"),
        })
        .collect::<Result<_>>()?;

    let (matched_id, hmac) =
        crate::vault::security_key::unlock_any(&credential_ids, &hmac_salt, pin)?;

    // Find the wrap whose credential_id matches what the device returned.
    let matched_wrap = security_wraps
        .iter()
        .find(|w| match w {
            wraps::Wrap::SecurityKey { credential_id, .. } => wraps::b64_decode(credential_id)
                .map(|b| b == matched_id)
                .unwrap_or(false),
            _ => false,
        })
        .ok_or_else(|| {
            crate::Error::Crypto(
                "security key returned a credential id that doesn't match any enrolled wrap".into(),
            )
        })?;
    let (nonce_b64, ct_b64) = match matched_wrap {
        wraps::Wrap::SecurityKey {
            nonce, ciphertext, ..
        } => (nonce, ciphertext),
        _ => unreachable!("filtered above"),
    };
    let nonce: [u8; 12] = wraps::b64_decode(nonce_b64)?
        .as_slice()
        .try_into()
        .map_err(|_| crate::Error::Crypto("security key wrap: nonce length".into()))?;
    let ct = wraps::b64_decode(ct_b64)?;
    let dek = crypto::unwrap_dek_with_kek(&hmac, &nonce, &ct)?;
    let jwks = decrypt_keys_file(&dek)?;
    Ok((dek, jwks))
}

/// Path to the per-project "currently-selected tenant" pointer used by the CLI.
pub fn current_context_path() -> PathBuf {
    ProjectConfig::dir().join("current-context")
}

pub fn read_current_context() -> Result<Option<String>> {
    let path = current_context_path();
    if !path.exists() {
        return Ok(None);
    }
    let s = fs::read_to_string(path)?;
    Ok(Some(s.trim().to_string()))
}

pub fn write_current_context(name: &str) -> Result<()> {
    fs::create_dir_all(ProjectConfig::dir())?;
    fs::write(current_context_path(), name)?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub project: String,
    pub default_tenant: String,
    #[serde(rename = "tenant", default)]
    pub tenants: Vec<Tenant>,
}

/// Latest `settings.toml` schema understood by this binary.
pub const SETTINGS_VERSION: u32 = 1;

fn default_settings_version() -> u32 {
    SETTINGS_VERSION
}

/// Project-local settings persisted in `.aic/settings.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Schema version of settings.toml. Bump when a field changes meaning or
    /// disappears, and add the corresponding migration step; a `version` that
    /// predates a rename is the only thing that lets a later `aic` tell "the
    /// user never set this" from "this used to be called something else".
    #[serde(default = "default_settings_version")]
    pub version: u32,
    /// Whether vault artifacts are encrypted at rest. Change this only through
    /// [`enable_encryption`] or [`disable_encryption`].
    #[serde(default)]
    pub encrypt_keys: bool,
    /// Agent auto-lock timeout. `None` selects the one-hour daemon default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_idle_timeout_secs: Option<u64>,
    /// Explicit or derived-at-runtime operator identity components.
    #[serde(default)]
    pub operator: Operator,
}

/// Stored operator identity. Missing fields are derived for a run but are not
/// persisted until a human or onboarding establishes them.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Operator {
    /// How this install identifies its human. `None` means "never
    /// established" — that is the trigger for the first-run prompt, so do NOT
    /// persist a derived fallback here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Machine component of the key id. Same rule: `None` means derive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            encrypt_keys: false,
            agent_idle_timeout_secs: None,
            operator: Operator::default(),
        }
    }
}

impl Settings {
    pub fn path() -> std::path::PathBuf {
        ProjectConfig::dir().join("settings.toml")
    }

    pub fn load() -> Result<Option<Self>> {
        let path = Self::path();
        if !path.exists() {
            return Ok(None);
        }
        Ok(Some(Self::load_from(&path)?))
    }

    pub fn save(&self) -> Result<()> {
        fs::create_dir_all(ProjectConfig::dir())?;
        self.save_to(&Self::path())?;
        ProjectConfig::write_gitignore()?;
        Ok(())
    }

    fn load_from(path: &Path) -> Result<Self> {
        let settings: Self = toml::from_str(&fs::read_to_string(path)?)?;
        match settings.version {
            SETTINGS_VERSION => {}
            version if version > SETTINGS_VERSION => {
                return Err(crate::Error::Config(format!(
                    "settings.toml is version {version}; this aic understands up to \
                     {SETTINGS_VERSION} — upgrade aic"
                )));
            }
            version => {
                return Err(crate::Error::Config(format!(
                    "settings.toml version {version} is unsupported"
                )));
            }
        }
        Ok(settings)
    }

    fn save_to(&self, path: &Path) -> Result<()> {
        fs::write(path, toml::to_string_pretty(self)?)?;
        Ok(())
    }
}

impl ProjectConfig {
    pub fn dir() -> PathBuf {
        PathBuf::from(".aic")
    }

    pub fn keys_path() -> PathBuf {
        VaultArtifact::Jwks.enc_path()
    }

    pub fn wraps_path() -> PathBuf {
        Self::dir().join("wraps.toml")
    }

    pub fn keys_plain_path() -> PathBuf {
        VaultArtifact::Jwks.plain_path()
    }

    pub fn config_path() -> PathBuf {
        Self::dir().join("config.toml")
    }

    /// Root of the script-sync workspace (sibling of `.aic/`). Trees are
    /// namespaced per tenant + realm so multiple tenants never share a tree.
    pub fn workspace_dir() -> PathBuf {
        PathBuf::from("workspace")
    }

    /// `workspace/<tenant>/` — the per-tenant tree. AM scripts live under
    /// `am/<realm>/<type>/` (realm-scoped); IDM endpoints under
    /// `idm/endpoint/` (tenant-global). Also holds configs + `.aic-sync/`.
    pub fn workspace_tree(tenant: &str) -> PathBuf {
        Self::workspace_dir().join(tenant)
    }

    /// `workspace/<tenant>/.aic-sync/` — our sync state for the whole tenant
    /// (snapshots for both realms + IDM, applied-templates version).
    /// Gitignored; never holds secrets.
    pub fn aic_sync_dir(tenant: &str) -> PathBuf {
        Self::workspace_tree(tenant).join(".aic-sync")
    }

    pub fn load() -> Result<Option<Self>> {
        let path = Self::config_path();
        if !path.exists() {
            return Ok(None);
        }
        let contents = fs::read_to_string(&path)?;
        let config: ProjectConfig = toml::from_str(&contents)?;
        Ok(Some(config))
    }

    pub fn save(&self) -> Result<()> {
        let dir = Self::dir();
        fs::create_dir_all(&dir)?;
        let contents = toml::to_string_pretty(self)?;
        fs::write(Self::config_path(), contents)?;
        Self::write_gitignore()?;
        Ok(())
    }

    pub fn write_gitignore() -> Result<()> {
        fs::create_dir_all(Self::dir())?;
        let path = Self::dir().join(".gitignore");
        fs::write(path, Self::gitignore_content())?;
        Ok(())
    }

    fn gitignore_content() -> String {
        // Every vault artifact's .enc/.plain pair, plus wraps.toml — which
        // holds the (encrypted) DEK envelope and the FIDO2 credential id for
        // any enrolled security_keys (opaque but device-specific). Never
        // check any of these in.
        let mut content = String::new();
        for &artifact in VaultArtifact::ALL {
            content.push_str(&format!(
                "{stem}.enc\n{stem}.plain\n",
                stem = artifact.file_stem()
            ));
        }
        content.push_str(
            "wraps.toml\n\
             # Machine-local, per-person settings — not project content.\n\
             settings.toml\n\
             local-config/\n\
             backups/\n\
             *.log\n",
        );
        content
    }

    /// Save the encrypted JWK map (Argon2id + AES-256-GCM) at mode 600.
    pub fn save_keys_enc(data: &[u8]) -> Result<()> {
        save_private_file(&Self::keys_path(), data)
    }

    pub fn load_keys_enc() -> Result<Option<Vec<u8>>> {
        load_optional_file(&Self::keys_path())
    }

    /// Save unencrypted JWK map (mode 600). Used when the user opts out of
    /// master-password protection.
    pub fn save_keys_plain(data: &[u8]) -> Result<()> {
        save_private_file(&Self::keys_plain_path(), data)
    }

    pub fn load_keys_plain() -> Result<Option<Vec<u8>>> {
        load_optional_file(&Self::keys_plain_path())
    }
}

fn save_private_file(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, data)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

fn load_optional_file(path: &Path) -> Result<Option<Vec<u8>>> {
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(fs::read(path)?))
}

/// A scratch directory that cleans itself up on drop, shared by the tests in
/// this module and in [`operator`]. RAII rather than a trailing
/// `remove_dir_all` so a failing assertion doesn't leak the directory.
#[cfg(test)]
pub(crate) struct TestDir(PathBuf);

#[cfg(test)]
impl TestDir {
    pub(crate) fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("pingone-aic-manager-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    pub(crate) fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

#[cfg(test)]
impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in per-tenant map — the registry treats artifact payloads as
    /// opaque JSON bytes, so any serialisable map exercises the generic path.
    fn sample_map() -> HashMap<String, String> {
        HashMap::from([("sandbox".to_string(), "opaque-secret".to_string())])
    }

    #[test]
    fn artifact_bytes_plain_save_load_round_trip() {
        let dir = TestDir::new();
        let path = dir.path("log-keys.plain");
        let expected = sample_map();

        // Mirror `save_artifact_bytes(_, _, None)`: plaintext at mode 600.
        save_private_file(&path, &serde_json::to_vec(&expected).unwrap()).unwrap();
        let actual: HashMap<String, String> =
            decode_json_map(load_optional_file(&path).unwrap()).unwrap();

        assert_eq!(actual, expected);
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn artifact_bytes_encrypted_save_load_round_trip() {
        let dir = TestDir::new();
        let path = dir.path("log-keys.enc");
        let expected = sample_map();
        let dek = crypto::Dek::random();

        // Mirror `save_artifact_bytes(_, _, Some(dek))`: AES-256-GCM at mode 600.
        let encrypted =
            crypto::encrypt_data(&serde_json::to_vec(&expected).unwrap(), &dek).unwrap();
        save_private_file(&path, &encrypted).unwrap();
        let plaintext =
            crypto::decrypt_data(&load_optional_file(&path).unwrap().unwrap(), &dek).unwrap();
        let actual: HashMap<String, String> = decode_json_map(Some(plaintext)).unwrap();

        assert_eq!(actual, expected);
        assert_ne!(
            fs::read(&path).unwrap(),
            serde_json::to_vec(&expected).unwrap()
        );
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn missing_artifact_bytes_decode_as_empty_map() {
        let empty: HashMap<String, String> = decode_json_map(None).unwrap();
        assert!(empty.is_empty());
    }

    #[test]
    fn old_settings_file_loads_and_round_trips_with_new_defaults() {
        let dir = TestDir::new();
        let path = dir.path("settings.toml");
        fs::write(&path, "encrypt_keys = true\n").unwrap();

        let settings = Settings::load_from(&path).unwrap();
        assert_eq!(settings.version, SETTINGS_VERSION);
        assert!(settings.encrypt_keys);
        assert_eq!(settings.agent_idle_timeout_secs, None);
        assert_eq!(settings.operator, Operator::default());

        settings.save_to(&path).unwrap();
        let saved = fs::read_to_string(&path).unwrap();
        assert!(saved.contains("version = 1"));
        assert!(saved.contains("[operator]"));
        let reloaded = Settings::load_from(&path).unwrap();
        assert_eq!(reloaded.version, SETTINGS_VERSION);
        assert!(reloaded.encrypt_keys);
        assert_eq!(reloaded.agent_idle_timeout_secs, None);
        assert_eq!(reloaded.operator, Operator::default());
    }

    #[test]
    fn future_settings_version_is_a_clean_error() {
        let dir = TestDir::new();
        let path = dir.path("settings.toml");
        fs::write(&path, "version = 99\nencrypt_keys = true\n").unwrap();

        let error = Settings::load_from(&path).unwrap_err();

        assert_eq!(
            error.to_string(),
            "Config error: settings.toml is version 99; this aic understands up to 1 — upgrade aic"
        );
    }

    #[test]
    fn gitignore_covers_every_artifact_stem() {
        let content = ProjectConfig::gitignore_content();
        for &artifact in VaultArtifact::ALL {
            for suffix in [".enc", ".plain"] {
                let pattern = format!("{}{suffix}", artifact.file_stem());
                assert!(content.lines().any(|line| line == pattern));
            }
        }
        // A shared settings.toml silently assigns one teammate's operator name
        // to everyone else using the project.
        assert!(content.lines().any(|line| line == "settings.toml"));
        // Every tenant snapshot written below ProjectConfig::dir() is private
        // project state; a write_gitignore() call alone does not cover it.
        assert!(content.lines().any(|line| line == "backups/"));
        // Both known stems must resolve back from their wire `kind`.
        assert_eq!(VaultArtifact::from_kind("keys"), Some(VaultArtifact::Jwks));
        assert_eq!(
            VaultArtifact::from_kind("log-keys"),
            Some(VaultArtifact::LogKeys)
        );
        assert_eq!(VaultArtifact::from_kind("nope"), None);
    }

    #[test]
    fn config_tenant_without_sa_id_round_trips_as_none() {
        let input = r#"
project = "test"
default_tenant = "logs"

[[tenant]]
name = "logs"
base_url = "https://logs.example"
theme = "sandbox"
scopes = []
"#;

        let config: ProjectConfig = toml::from_str(input).unwrap();
        assert_eq!(config.tenants[0].sa_id, None);
        assert!(config.tenants[0].provenance.is_unknown());

        let serialized = toml::to_string(&config).unwrap();
        assert!(!serialized.contains("sa_id"));
        assert!(
            !serialized.contains("provenance"),
            "legacy configs must not grow an empty provenance table: {serialized}"
        );
        let reparsed: ProjectConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(reparsed.tenants[0].sa_id, None);
        assert!(reparsed.tenants[0].provenance.is_unknown());
    }

    #[test]
    fn config_tenant_with_sa_id_round_trips_as_some() {
        let input = r#"
project = "test"
default_tenant = "sandbox"

[[tenant]]
name = "sandbox"
base_url = "https://sandbox.example"
theme = "sandbox"
sa_id = "service-account-id"
scopes = ["fr:idm:*"]
"#;

        let config: ProjectConfig = toml::from_str(input).unwrap();
        assert_eq!(
            config.tenants[0].sa_id.as_deref(),
            Some("service-account-id")
        );
        assert!(config.tenants[0].provenance.is_unknown());

        let serialized = toml::to_string(&config).unwrap();
        assert!(
            !serialized.contains("provenance"),
            "unknown provenance must stay omitted: {serialized}"
        );
        let reparsed: ProjectConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(
            reparsed.tenants[0].sa_id.as_deref(),
            Some("service-account-id")
        );
        assert!(reparsed.tenants[0].provenance.is_unknown());
    }

    #[test]
    fn provenance_block_round_trips_after_every_scalar() {
        // Moving `provenance` above `scopes` (or dropping skip_serializing_if)
        // either loses the array on write or injects an empty table into every
        // existing config.toml.
        let input = r#"
project = "test"
default_tenant = "sandbox"

[[tenant]]
name = "sandbox"
base_url = "https://sandbox.example"
theme = "sandbox"
sa_id = "service-account-id"
scopes = ["fr:idm:*"]

[tenant.provenance]
service_account = "created"
log_key = "external"
"#;

        let config: ProjectConfig = toml::from_str(input).unwrap();
        assert_eq!(
            config.tenants[0].provenance.service_account,
            Some(CredentialSource::Created)
        );
        assert_eq!(
            config.tenants[0].provenance.log_key,
            Some(CredentialSource::External)
        );

        let serialized = toml::to_string(&config).unwrap();
        let scopes = serialized
            .find("scopes")
            .expect("scopes stays a scalar in the tenant table");
        let provenance = serialized
            .find("[tenant.provenance]")
            .expect("provenance emits as a nested table");
        assert!(
            provenance > scopes,
            "nested provenance table must follow every scalar:\n{serialized}"
        );
        assert!(serialized.contains("service_account = \"created\""));
        assert!(serialized.contains("log_key = \"external\""));

        let reparsed: ProjectConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(reparsed.tenants[0].provenance, config.tenants[0].provenance);
    }
}
