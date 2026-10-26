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

/// Absolute paths for one project.
///
/// The CLI installs one process default during bootstrap. Tests and other
/// callers that need independent projects can construct values directly and
/// pass them to the path-aware I/O methods instead of mutating the process cwd.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectPaths {
    root: PathBuf,
}

static PROJECT_PATHS: OnceLock<ProjectPaths> = OnceLock::new();

impl ProjectPaths {
    /// Address a project rooted at `root`, which must be absolute.
    pub fn new(root: PathBuf) -> Result<Self> {
        if !root.is_absolute() {
            return Err(crate::Error::Config(format!(
                "project root must be absolute: {}",
                root.display()
            )));
        }
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn aic_dir(&self) -> PathBuf {
        self.root.join(".aic")
    }

    pub fn config_path(&self) -> PathBuf {
        self.aic_dir().join("config.toml")
    }

    pub fn settings_path(&self) -> PathBuf {
        self.aic_dir().join("settings.toml")
    }

    pub fn wraps_path(&self) -> PathBuf {
        self.aic_dir().join("wraps.toml")
    }

    pub fn current_context_path(&self) -> PathBuf {
        self.aic_dir().join("current-context")
    }

    pub fn workspace_dir(&self) -> PathBuf {
        self.root.join("workspace")
    }

    pub fn workspace_tree(&self, tenant: &str) -> PathBuf {
        self.workspace_dir().join(tenant)
    }

    pub fn aic_sync_dir(&self, tenant: &str) -> PathBuf {
        self.workspace_tree(tenant).join(".aic-sync")
    }

    pub fn backups_dir(&self) -> PathBuf {
        self.aic_dir().join("backups")
    }

    pub fn undo_log_path(&self) -> PathBuf {
        self.aic_dir().join("undo.log")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.aic_dir().join("logs")
    }

    pub fn idmstore_dir(&self) -> PathBuf {
        self.aic_dir().join("idmstore")
    }

    pub fn agent_socket_path(&self) -> PathBuf {
        self.aic_dir().join("agent.sock")
    }

    pub fn agent_pid_path(&self) -> PathBuf {
        self.aic_dir().join("agent.pid")
    }

    pub fn agent_log_path(&self) -> PathBuf {
        self.aic_dir().join("agent.log")
    }

    fn artifact_path(&self, artifact: VaultArtifact, suffix: &str) -> PathBuf {
        self.aic_dir()
            .join(format!("{}{suffix}", artifact.file_stem()))
    }

    pub(crate) fn artifact_enc_path(&self, artifact: VaultArtifact) -> PathBuf {
        self.artifact_path(artifact, ".enc")
    }

    pub(crate) fn artifact_plain_path(&self, artifact: VaultArtifact) -> PathBuf {
        self.artifact_path(artifact, ".plain")
    }

    pub(crate) fn write_gitignore(&self) -> Result<()> {
        ProjectConfig::write_gitignore_to(&self.aic_dir())
    }
}

/// Install the immutable project root used by the process-default path APIs.
/// Repeating the same root is harmless; selecting another root is an error.
pub fn set_project_root(root: PathBuf) -> Result<()> {
    let paths = ProjectPaths::new(root)?;
    if let Some(current) = PROJECT_PATHS.get() {
        if current == &paths {
            return Ok(());
        }
        return Err(crate::Error::Config(format!(
            "project root is already set to {}",
            current.root().display()
        )));
    }
    PROJECT_PATHS
        .set(paths)
        .map_err(|_| crate::Error::Config("project root was initialized concurrently".into()))
}

pub(crate) fn project_paths() -> &'static ProjectPaths {
    PROJECT_PATHS.get().expect(
        "project paths are installed by cli::bootstrap_project_root; \
             a test that reaches this must build its own ProjectPaths instead",
    )
}

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

/// Proof that a tenant's credentials may be written: the lifecycle lock is
/// held and the tenant was confirmed present and unchanged under it. Minted
/// only by [`hold_tenant`]; every per-tenant credential store requires one.
#[derive(Debug)]
pub struct TenantHold {
    _lock: fs::File,
    tenant: String,
}

impl TenantHold {
    pub fn tenant(&self) -> &str {
        &self.tenant
    }
}

/// See [`ProjectPaths::hold_tenant`].
pub fn hold_tenant(expected: &Tenant) -> Result<TenantHold> {
    project_paths().hold_tenant(expected)
}

/// How a lock is held: many readers of a state, or the one changing it.
#[derive(Debug, Clone, Copy)]
enum Hold {
    Shared,
    Exclusive,
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
    project_paths().load_artifact_bytes(artifact, dek)
}

impl ProjectPaths {
    pub(crate) fn load_artifact_bytes(
        &self,
        artifact: VaultArtifact,
        dek: Option<&crypto::Dek>,
    ) -> Result<Option<Vec<u8>>> {
        let enc_path = self.artifact_enc_path(artifact);
        let plain_path = self.artifact_plain_path(artifact);
        match dek {
            Some(dek) => match load_optional_file(&enc_path)? {
                Some(data) => Ok(Some(crypto::decrypt_data(&data, dek)?)),
                None => Ok(None),
            },
            None => load_optional_file(&plain_path),
        }
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
    project_paths().save_artifact_bytes(artifact, bytes, dek)
}

impl ProjectPaths {
    pub(crate) fn save_artifact_bytes(
        &self,
        artifact: VaultArtifact,
        bytes: &[u8],
        dek: Option<&crypto::Dek>,
    ) -> Result<()> {
        match dek {
            Some(dek) => {
                let enc = crypto::encrypt_data(bytes, dek)?;
                save_private_file(&self.artifact_enc_path(artifact), &enc)
            }
            None => save_private_file(&self.artifact_plain_path(artifact), bytes),
        }
    }

    /// Read-modify-write one artifact's per-tenant map as a transaction: an
    /// exclusive lock on `<stem>.lock` is held across the read, `change` and
    /// the write, and `change` sees the map **as it is on disk under that
    /// lock**. A writer that saved its in-memory copy instead would drop
    /// every entry another `aic` — or the daemon — added since that copy was
    /// read. Returns the map as written, so a caller can refresh its copy.
    ///
    /// Only writers that come through here are serialised. Same non-re-entrant
    /// rule as `update_config`: `change` must not take this lock again.
    ///
    /// The caller chose plain or encrypted — and, encrypted, which key —
    /// before it got here, possibly long before (a TUI open for hours); a
    /// mode switch since is refused rather than written into the form the
    /// vault no longer reads, and so is a key the vault no longer uses. The
    /// mode is read from `settings.toml` under a shared hold of the vault
    /// lock, which a mode switch holds exclusively throughout — so it is
    /// authoritative for every artifact, including one that had no file in
    /// either form when the switch ran. A copy left in the other form by a
    /// switch that died after its commit point is removed once this write
    /// has landed.
    pub(crate) fn update_artifact_map<V, T>(
        &self,
        artifact: VaultArtifact,
        dek: Option<&crypto::Dek>,
        change: impl FnOnce(&mut HashMap<String, V>) -> Result<T>,
    ) -> Result<(HashMap<String, V>, T)>
    where
        V: serde::de::DeserializeOwned + serde::Serialize,
    {
        let _vault = self.lock_vault(Hold::Shared)?;
        let _guard = self.lock_artifact(artifact)?;
        self.vault_mode_is(dek)?;
        self.not_only_in_other_form(artifact, dek)?;
        let mut map = decode_json_map(self.load_artifact_bytes(artifact, dek)?)?;
        let out = change(&mut map)?;
        self.save_artifact_bytes(artifact, &serde_json::to_vec(&map)?, dek)?;
        self.discard_stale_form(artifact, dek.is_some())?;
        Ok((map, out))
    }

    /// Read one artifact's map bytes as the vault holds them: under a shared
    /// hold of the vault lock, in the mode — and under the key — `dek`
    /// assumes. A mismatch is an error, never an absent file: a reader in
    /// the old mode would otherwise report every credential as missing, and
    /// an offboarding probe would take that as "nothing to remove".
    pub(crate) fn read_artifact_bytes(
        &self,
        artifact: VaultArtifact,
        dek: Option<&crypto::Dek>,
    ) -> Result<Option<Vec<u8>>> {
        let _vault = self.lock_vault(Hold::Shared)?;
        self.vault_mode_is(dek)?;
        self.not_only_in_other_form(artifact, dek)?;
        self.load_artifact_bytes(artifact, dek)
    }

    /// Whether `dek` is this vault's key, checked under a shared hold of the
    /// vault lock that is **returned**: the daemon installs the key while
    /// holding it, so no transition can replace the key between the check
    /// and the install — an unlock result that arrives after a key change
    /// cannot install the old key.
    pub(crate) fn hold_vault_key(&self, dek: &crypto::Dek) -> Result<fs::File> {
        let vault = self.lock_vault(Hold::Shared)?;
        self.vault_mode_is(Some(dek))?;
        Ok(vault)
    }

    /// No file in the form the vault reads, but one in the other: the only
    /// copy, left by an older version's interrupted transition (which
    /// deleted each source as it went). Reading it as an empty map reports
    /// every credential absent; writing would create the current form and
    /// let the stale-form discard delete the original. `settle` keeps such a
    /// file, so every reader and writer must too.
    fn not_only_in_other_form(
        &self,
        artifact: VaultArtifact,
        dek: Option<&crypto::Dek>,
    ) -> Result<()> {
        let (current, other) = if dek.is_some() {
            (
                self.artifact_enc_path(artifact),
                self.artifact_plain_path(artifact),
            )
        } else {
            (
                self.artifact_plain_path(artifact),
                self.artifact_enc_path(artifact),
            )
        };
        if current.exists() || !other.exists() {
            return Ok(());
        }
        let name = |p: &Path| {
            p.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        };
        Err(crate::Error::Crypto(format!(
            "{} is the only copy of this map but the vault is {}: an encryption change \
             was interrupted by an older aic. Nothing was changed. Recovering needs every \
             vault file in one form — the other files in .aic/ may be split the same way — \
             so check which form each map is in before changing anything; back up .aic/ first",
            name(&other),
            if dek.is_some() { "encrypted" } else { "plain" },
        )))
    }
}

/// The directory the operator ran the command in, recorded before the CLI
/// moves the process into the project root.
///
/// Only [`display_path`] consumes it. Every path the program computes is
/// absolute (see [`ProjectPaths`]), which is what makes them safe to use, but
/// printing one raw is needlessly long from the project root — where most
/// commands are run.
static INVOCATION_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Record the invocation directory. First call wins; later ones are ignored,
/// so a test or a second bootstrap cannot move it under a running command.
pub fn set_invocation_dir(dir: PathBuf) {
    let _ = INVOCATION_DIR.set(dir);
}

/// Render `path` the way the operator would type it: relative to the directory
/// they invoked the command in, when it lies beneath that directory, and
/// absolute otherwise.
///
/// Absolute is the fallback rather than a `../..` chain because a path outside
/// the operator's directory is one they will copy, paste or `cd` to, and a
/// relative form only works from the one place they already are.
pub fn display_path(path: &Path) -> String {
    relative_to(INVOCATION_DIR.get().map(PathBuf::as_path), path)
}

/// The pure half of [`display_path`], so the unset and outside cases are
/// testable without a process-global.
fn relative_to(base: Option<&Path>, path: &Path) -> String {
    base.and_then(|base| path.strip_prefix(base).ok())
        .filter(|rel| !rel.as_os_str().is_empty())
        .unwrap_or(path)
        .display()
        .to_string()
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
    /// Which directory the context was read from. [`detect_workspace_context`]
    /// leaves it `None`; the CLI bootstrap records it, so a command can say
    /// why it chose the tenant it did.
    pub origin: Option<WorkspaceOrigin>,
}

/// The directory a [`WorkspaceContext`] was inferred from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceOrigin {
    /// The process's working directory.
    Cwd,
    /// An explicit search start: `--project` or `AIC_PROJECT`, named here.
    Explicit(&'static str),
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
    project_paths().decrypt_keys_file(dek)
}

/// Encrypt + persist the JWK map using the in-memory DEK.
pub fn save_jwk_map(map: &HashMap<String, serde_json::Value>, dek: &crypto::Dek) -> Result<()> {
    project_paths().save_jwk_map(map, dek)
}

impl ProjectPaths {
    pub(crate) fn decrypt_keys_file(
        &self,
        dek: &crypto::Dek,
    ) -> Result<HashMap<String, serde_json::Value>> {
        decode_json_map(self.load_artifact_bytes(VaultArtifact::Jwks, Some(dek))?)
    }

    pub(crate) fn save_jwk_map(
        &self,
        map: &HashMap<String, serde_json::Value>,
        dek: &crypto::Dek,
    ) -> Result<()> {
        self.save_artifact_bytes(VaultArtifact::Jwks, &serde_json::to_vec(map)?, Some(dek))
    }
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
/// flip `settings.toml` to `encrypt_keys = true`, then delete the plaintext
/// files. Called by the Settings screen when the user adds the first auth
/// factor while encryption is disabled.
///
/// The flag is the commit point. Nothing the old mode reads is removed
/// before it flips, so a transition that dies part way leaves the vault
/// working in the old mode, and a retry — under any key — overwrites the
/// partial copies (removing them first: [`ProjectPaths::settle`]). Copies
/// left in the old form after the flip are removed by the next write of
/// that artifact or the next unlock.
///
/// `wraps.toml` is not touched here — callers must have already saved the
/// wrap that protects this DEK before calling this function, otherwise
/// `keys.enc` would be unreadable on next launch.
///
/// `expected` is the vault mode the caller decided from (`None`: no
/// `settings.toml`, first run); `save_wrap` persists the wrap of `dek`. Both
/// run under the exclusive vault lock, the mode rechecked first, so a second
/// TUI finishing its own setup in the meantime is refused here instead of
/// having its wrap replaced by one for a DEK its files were not written with.
pub fn enable_encryption(
    dek: &crypto::Dek,
    expected: Option<bool>,
    save_wrap: impl FnOnce() -> Result<()>,
) -> Result<()> {
    project_paths().enable_encryption(dek, expected, save_wrap)
}

/// First run without encryption: record `encrypt_keys = false` and make sure
/// `keys.plain` exists, under the vault lock and only if no other `aic` has
/// set the vault up since `expected` was read.
pub fn set_up_plain(expected: Option<bool>) -> Result<()> {
    project_paths().set_up_plain(expected)
}

impl ProjectPaths {
    pub(crate) fn enable_encryption(
        &self,
        dek: &crypto::Dek,
        expected: Option<bool>,
        save_wrap: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        let _vault = self.lock_vault(Hold::Exclusive)?;
        self.expect_vault_mode(expected)?;
        self.settle()?;
        // Refuse before the wrap is replaced, not after: a destination this
        // DEK cannot read means the files belong to some other setup.
        for &artifact in VaultArtifact::ALL {
            self.migration_preflight(artifact, None, Some(dek))?;
        }
        save_wrap()?;
        for &artifact in VaultArtifact::ALL {
            // The JWK map must always exist encrypted after this: first-run
            // installs never wrote keys.plain (no factors added yet), so an
            // empty map stands in — decrypting empty bytes would fail the
            // round-trip in `decrypt_keys_file`. Other artifacts are optional:
            // only a present plaintext file is migrated.
            self.migrate_artifact(artifact, None, Some(dek), artifact == VaultArtifact::Jwks)?;
        }
        self.set_vault_mode(true)?;
        self.settle_after_commit();
        Ok(())
    }

    pub(crate) fn disable_encryption(&self, dek: &crypto::Dek) -> Result<()> {
        let _vault = self.lock_vault(Hold::Exclusive)?;
        self.expect_vault_mode(Some(true))?;
        self.key_is_current(dek)?;
        self.settle()?;
        for &artifact in VaultArtifact::ALL {
            self.migration_preflight(artifact, Some(dek), None)?;
        }
        for &artifact in VaultArtifact::ALL {
            // The JWK map always lands as plaintext (even if empty) — the app
            // expects keys.plain to exist in no-encryption mode. Other
            // artifacts round-trip only when they had an encrypted file.
            self.migrate_artifact(artifact, Some(dek), None, artifact == VaultArtifact::Jwks)?;
        }
        // `keys.enc` and `wraps.toml` stay until the flag has flipped, so a
        // disable that dies before it leaves a vault that still unlocks.
        self.set_vault_mode(false)?;
        self.settle_after_commit();
        Ok(())
    }

    pub(crate) fn set_up_plain(&self, expected: Option<bool>) -> Result<()> {
        if expected == Some(true) {
            // Leaving encryption is `disable_encryption`, which decrypts
            // the files first; flipping the flag alone would strand them.
            return Err(crate::Error::Crypto(
                "the vault is encrypted; disable encryption from Auth Settings instead".into(),
            ));
        }
        let _vault = self.lock_vault(Hold::Exclusive)?;
        self.expect_vault_mode(expected)?;
        self.migration_preflight(VaultArtifact::Jwks, None, None)?;
        if load_optional_file(&self.artifact_plain_path(VaultArtifact::Jwks))?.is_none() {
            self.save_artifact_bytes(VaultArtifact::Jwks, b"{}", None)?;
        }
        self.set_vault_mode(false)
    }

    /// Remove whatever the committed mode does not read: the other form of
    /// an artifact whose current form exists, and — plain — a `wraps.toml`
    /// once no encrypted file is left for it to open. Such copies are a
    /// transition's partial work (before its flag flipped) or its leftover
    /// (after); a plaintext leftover is a cleartext credential the operator
    /// believes encrypted. An artifact found **only** in the other form is
    /// left alone: no transition of this version writes that, and it may be
    /// the only copy. Nothing to do before first-run setup. Callers hold the
    /// vault lock exclusively.
    fn settle(&self) -> Result<()> {
        self.discard_abandoned_sidecars()?;
        let Some(settings) = self.load_settings()? else {
            return Ok(());
        };
        for &artifact in VaultArtifact::ALL {
            self.discard_stale_form(artifact, settings.encrypt_keys)?;
        }
        if !settings.encrypt_keys
            && VaultArtifact::ALL
                .iter()
                .all(|&a| !self.artifact_enc_path(a).exists())
        {
            remove_if_present(&self.wraps_path())?;
        }
        Ok(())
    }

    /// `save_private_file` writes `<name>.new.<uuid>` and renames it into
    /// place; a process that died between the two left the sidecar, holding
    /// whatever it was writing — a cleartext credential map, possibly. Every
    /// writer of these files holds the vault lock, so under the exclusive
    /// hold none is in flight and any sidecar is abandoned.
    fn discard_abandoned_sidecars(&self) -> Result<()> {
        let mut names: Vec<String> = vec!["settings.toml".into(), "wraps.toml".into()];
        for &artifact in VaultArtifact::ALL {
            for path in [
                self.artifact_plain_path(artifact),
                self.artifact_enc_path(artifact),
            ] {
                names.push(
                    path.file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        }
        let entries = match fs::read_dir(self.aic_dir()) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        for entry in entries {
            let entry = entry?;
            let file_name = entry.file_name();
            let file_name = file_name.to_string_lossy();
            let abandoned = names.iter().any(|name| {
                file_name
                    .strip_prefix(name.as_str())
                    .and_then(|rest| rest.strip_prefix(".new."))
                    .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok())
            });
            if abandoned && entry.file_type()?.is_file() {
                remove_if_present(&entry.path())?;
            }
        }
        Ok(())
    }

    /// After the flag flipped the transition has happened, whatever the
    /// cleanup does: reporting a failure here would make the caller undo its
    /// in-memory state for a vault that is already in the new mode. The
    /// leftovers are retried by the next write or unlock.
    fn settle_after_commit(&self) {
        if let Err(e) = self.settle() {
            tracing::warn!(error = %e, "vault transition committed; leftover cleanup failed");
        }
    }

    /// Remove `artifact`'s copy in the form `encrypted` does not read, if
    /// the form it does read exists. Callers hold the vault lock (either
    /// way) and `encrypted` is the mode read under it.
    fn discard_stale_form(&self, artifact: VaultArtifact, encrypted: bool) -> Result<()> {
        let (current, stale) = if encrypted {
            (
                self.artifact_enc_path(artifact),
                self.artifact_plain_path(artifact),
            )
        } else {
            (
                self.artifact_plain_path(artifact),
                self.artifact_enc_path(artifact),
            )
        };
        if current.exists() {
            remove_if_present(&stale)?;
        }
        Ok(())
    }

    /// Callers hold the vault lock exclusively.
    fn set_vault_mode(&self, encrypt: bool) -> Result<()> {
        let mut settings = self.load_settings()?.unwrap_or_default();
        settings.encrypt_keys = encrypt;
        self.write_settings(&settings)
    }
}

/// Transition from "encrypted" to "no encryption": decrypt `keys.enc` and
/// any `log-keys.enc` with `dek`, write the cleartext files at mode 600,
/// then delete the encrypted files and `wraps.toml`, and flip
/// `settings.toml` to `encrypt_keys = false`. Called by the last-factor
/// guard in the Settings screen.
pub fn disable_encryption(dek: &crypto::Dek) -> Result<()> {
    project_paths().disable_encryption(dek)
}

/// Finish what an interrupted vault transition left behind, under the
/// exclusive vault lock (see [`ProjectPaths::settle`]). Called after an
/// unlock, so a cleartext leftover does not wait for the next write of its
/// artifact.
pub fn settle_vault() -> Result<()> {
    let paths = project_paths();
    let _vault = paths.lock_vault(Hold::Exclusive)?;
    paths.settle()
}

/// An encrypted vault with no `settings.toml`: a first-run setup that died
/// after writing `keys.enc` and its wrap but before the flag (or a vault
/// whose settings file was lost). Both are recovered the same way — by
/// unlocking with the wrap, which proves the key, then finishing the setup.
pub fn interrupted_first_run() -> bool {
    project_paths().interrupted_first_run()
}

impl ProjectPaths {
    /// Encrypted files and a wrap, no settings, and **no plaintext form of
    /// anything**: a first run never writes one, so a plaintext copy means
    /// this is something else (a lost settings file over an older leftover)
    /// and which copy is current cannot be told from here.
    pub(crate) fn interrupted_first_run(&self) -> bool {
        !self.settings_path().exists()
            && self.wraps_path().exists()
            && self.artifact_enc_path(VaultArtifact::Jwks).exists()
            && VaultArtifact::ALL
                .iter()
                .all(|&a| !self.artifact_plain_path(a).exists())
    }

    /// Record `encrypt_keys = true` over the encrypted files as they are —
    /// nothing is copied, so nothing can be overwritten — once `dek` is
    /// shown to open every one of them, under the exclusive vault lock.
    pub(crate) fn finish_first_run(&self, dek: &crypto::Dek) -> Result<()> {
        let _vault = self.lock_vault(Hold::Exclusive)?;
        self.expect_vault_mode(None)?;
        if !self.interrupted_first_run() {
            return Err(crate::Error::Crypto(
                "this is not an interrupted first-run setup (a plaintext copy or the \
                 wrap is present or missing); nothing was changed"
                    .into(),
            ));
        }
        for &artifact in VaultArtifact::ALL {
            if self.artifact_enc_path(artifact).exists() {
                decode_json_map::<serde_json::Value>(
                    self.load_artifact_bytes(artifact, Some(dek))?,
                )?;
            }
        }
        self.set_vault_mode(true)
    }
}

/// Finish an [`interrupted_first_run`] under the key the operator just
/// unlocked with: see [`ProjectPaths::finish_first_run`].
pub fn finish_first_run(dek: &crypto::Dek) -> Result<()> {
    project_paths().finish_first_run(dek)
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
    project_paths().current_context_path()
}

pub fn read_current_context() -> Result<Option<String>> {
    project_paths().read_current_context()
}

pub fn write_current_context(name: &str) -> Result<()> {
    project_paths().write_current_context(name)
}

impl ProjectPaths {
    pub fn read_current_context(&self) -> Result<Option<String>> {
        let path = self.current_context_path();
        if !path.exists() {
            return Ok(None);
        }
        let s = fs::read_to_string(path)?;
        Ok(Some(s.trim().to_string()))
    }

    pub fn write_current_context(&self, name: &str) -> Result<()> {
        fs::create_dir_all(self.aic_dir())?;
        fs::write(self.current_context_path(), name)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
        project_paths().settings_path()
    }

    pub fn load() -> Result<Option<Self>> {
        project_paths().load_settings()
    }

    pub fn save(&self) -> Result<()> {
        project_paths().save_settings(self)
    }

    fn load_from(path: &Path) -> Result<Self> {
        let body = fs::read_to_string(path)?;
        // An empty file parses as every default — `encrypt_keys = false` —
        // and would name the plain form for a vault whose files are
        // encrypted. Nothing this version writes is empty, so it is damage.
        if body.trim().is_empty() {
            return Err(crate::Error::Config(format!(
                "{} is empty; restore it (it records whether the vault is encrypted)",
                path.display()
            )));
        }
        let settings: Self = toml::from_str(&body)?;
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

    /// Replaced, never truncated in place: a reader racing a truncating
    /// write would see an empty file, which is the vault mode's only record.
    fn save_to(&self, path: &Path) -> Result<()> {
        save_private_file(path, toml::to_string_pretty(self)?.as_bytes())
    }
}

impl ProjectPaths {
    pub fn load_settings(&self) -> Result<Option<Settings>> {
        let path = self.settings_path();
        if !path.exists() {
            return Ok(None);
        }
        Ok(Some(Settings::load_from(&path)?))
    }

    /// Save every field but the vault mode. `encrypt_keys` belongs to the
    /// vault transitions, which flip it under the exclusive vault lock as
    /// their last step; a settings edit that loaded the file before a
    /// transition and saved after it would otherwise put the flag back, and
    /// the vault would then name a form whose files are gone. So the flag on
    /// disk wins whenever there is one.
    pub fn save_settings(&self, settings: &Settings) -> Result<()> {
        let _vault = self.lock_vault(Hold::Exclusive)?;
        let mut settings = settings.clone();
        if let Some(on_disk) = self.load_settings()? {
            settings.encrypt_keys = on_disk.encrypt_keys;
        }
        self.write_settings(&settings)
    }

    /// Callers hold the vault lock exclusively.
    ///
    /// The settings file is written last: in a vault transition it is the
    /// commit point, and a fallible step after it would report "failed"
    /// for a switch that has already happened.
    fn write_settings(&self, settings: &Settings) -> Result<()> {
        fs::create_dir_all(self.aic_dir())?;
        self.write_gitignore()?;
        settings.save_to(&self.settings_path())
    }
}

impl ProjectConfig {
    pub fn dir() -> PathBuf {
        project_paths().aic_dir()
    }

    pub fn keys_path() -> PathBuf {
        project_paths().artifact_enc_path(VaultArtifact::Jwks)
    }

    pub fn wraps_path() -> PathBuf {
        project_paths().wraps_path()
    }

    pub fn keys_plain_path() -> PathBuf {
        project_paths().artifact_plain_path(VaultArtifact::Jwks)
    }

    pub fn config_path() -> PathBuf {
        project_paths().config_path()
    }

    /// Root of the script-sync workspace (sibling of `.aic/`). Trees are
    /// namespaced per tenant + realm so multiple tenants never share a tree.
    pub fn workspace_dir() -> PathBuf {
        project_paths().workspace_dir()
    }

    /// `workspace/<tenant>/` — the per-tenant tree. AM scripts live under
    /// `am/<realm>/<type>/` (realm-scoped); IDM endpoints under
    /// `idm/endpoint/` (tenant-global). Also holds configs + `.aic-sync/`.
    pub fn workspace_tree(tenant: &str) -> PathBuf {
        project_paths().workspace_tree(tenant)
    }

    /// `workspace/<tenant>/.aic-sync/` — our sync state for the whole tenant
    /// (snapshots for both realms + IDM, applied-templates version).
    /// Gitignored; never holds secrets.
    pub fn aic_sync_dir(tenant: &str) -> PathBuf {
        project_paths().aic_sync_dir(tenant)
    }

    pub fn load() -> Result<Option<Self>> {
        project_paths().load_config()
    }

    pub fn save(&self) -> Result<()> {
        project_paths().save_config(self)
    }

    pub fn write_gitignore() -> Result<()> {
        project_paths().write_gitignore()
    }

    /// Create `dir` and write the vault `.gitignore` into it.
    ///
    /// Callers must pass a stable path. Project-owned callers should prefer a
    /// [`ProjectPaths`] method, whose paths are absolute by construction.
    pub fn write_gitignore_to(dir: &Path) -> Result<()> {
        if !dir.is_absolute() {
            return Err(crate::Error::Config(format!(
                "gitignore directory must be absolute: {}",
                dir.display()
            )));
        }
        fs::create_dir_all(dir)?;
        fs::write(dir.join(".gitignore"), Self::gitignore_content())?;
        Ok(())
    }

    pub(crate) fn gitignore_content() -> String {
        // Every vault artifact's .enc/.plain pair, plus wraps.toml — which
        // holds the (encrypted) DEK envelope and the FIDO2 credential id for
        // any enrolled security_keys (opaque but device-specific). Never
        // check any of these in.
        let mut content = String::new();
        for &artifact in VaultArtifact::ALL {
            content.push_str(&format!(
                "{stem}.enc\n{stem}.plain\n{stem}.lock\n{stem}.enc.new*\n{stem}.plain.new*\n",
                stem = artifact.file_stem()
            ));
        }
        content.push_str(
            "wraps.toml\n\
             wraps.toml.new*\n\
             # Machine-local, per-person settings — not project content.\n\
             settings.toml\n\
             settings.toml.new*\n\
             local-config/\n\
             backups/\n\
             *.log\n\
             # Which ESV secret version holds which SAML certificate. No key\n\
             # material, but it names the tenant, its realms and its entities.\n\
             # Three names, not one: a change takes the lock on the first\n\
             # sidecar and is assembled in the second before being renamed\n\
             # into place (`saml::rotate::journal`).\n\
             saml-rotations.json\n\
             saml-rotations.json.lock\n\
             saml-rotations.json.new\n\
             # config.toml's update lock and its rename-into-place sidecar.\n\
             config.toml.lock\n\
             # Onboarding/offboarding's tenant-lifecycle lock.\n\
             tenants.lock\n\
             vault.lock\n\
             config.toml.new*\n",
        );
        content
    }

    /// Save the encrypted JWK map (Argon2id + AES-256-GCM) at mode 600.
    pub fn save_keys_enc(data: &[u8]) -> Result<()> {
        project_paths().save_keys_enc(data)
    }

    pub fn load_keys_enc() -> Result<Option<Vec<u8>>> {
        project_paths().load_keys_enc()
    }

    /// Save unencrypted JWK map (mode 600). Used when the user opts out of
    /// master-password protection.
    pub fn save_keys_plain(data: &[u8]) -> Result<()> {
        project_paths().save_keys_plain(data)
    }

    pub fn load_keys_plain() -> Result<Option<Vec<u8>>> {
        project_paths().load_keys_plain()
    }
}

impl ProjectPaths {
    pub fn load_config(&self) -> Result<Option<ProjectConfig>> {
        let path = self.config_path();
        if !path.exists() {
            return Ok(None);
        }
        let contents = fs::read_to_string(&path)?;
        let config: ProjectConfig = toml::from_str(&contents)?;
        Ok(Some(config))
    }

    pub fn save_config(&self, config: &ProjectConfig) -> Result<()> {
        let _guard = self.lock_config()?;
        self.install_config(config)
    }

    /// Read-modify-write `config.toml` as one transaction: an exclusive lock
    /// on `config.toml.lock` is held across the read, `change` and the
    /// rename, so two writers cannot both read the same file and the later
    /// silently drop the earlier's edit. `change` sees the file as it is
    /// *under the lock* — validate against that, not an earlier read. Only
    /// writers that come through here are serialised; a `load` → `save` pair
    /// is still a lost-update window (its `save` is merely atomic).
    ///
    /// **`change` must not call `save_config` or `update_config`**: the lock
    /// is not re-entrant, so a nested save blocks on the lock this call holds
    /// and the process deadlocks against itself. Mutate the `&mut` it is
    /// given; that is what gets written.
    ///
    /// A `change` that leaves the document as it was writes nothing — not
    /// even to normalise a hand-edited file's formatting or comments.
    pub fn update_config<T>(
        &self,
        change: impl FnOnce(&mut ProjectConfig) -> Result<T>,
    ) -> Result<T> {
        self.update_or_create_config(
            || Err(crate::Error::Config("no .aic/config.toml here".into())),
            change,
        )
    }

    /// `update_config` for a writer that may be the first: with no
    /// `config.toml`, `create` supplies the document `change` then edits —
    /// decided under the same lock, so two first onboardings cannot both see
    /// "absent" and the later overwrite the earlier.
    pub fn update_or_create_config<T>(
        &self,
        create: impl FnOnce() -> Result<ProjectConfig>,
        change: impl FnOnce(&mut ProjectConfig) -> Result<T>,
    ) -> Result<T> {
        let _guard = self.lock_config()?;
        let loaded = self.load_config()?;
        let mut config = match &loaded {
            Some(config) => config.clone(),
            None => create()?,
        };
        let out = change(&mut config)?;
        // Compared as parsed documents, not bytes: a hand-edited file
        // (comments, its own key order) re-serialises differently, and a
        // change that altered nothing must not rewrite it into ours.
        if loaded.as_ref() != Some(&config) {
            self.install_config(&config)?;
        }
        Ok(out)
    }

    /// The lock lives on a sidecar whose inode nothing replaces (the config
    /// itself is renamed over). `flock` underneath, released by the kernel
    /// when the descriptor closes, so a killed `aic` leaves no stale lock.
    fn lock_config(&self) -> Result<fs::File> {
        self.lock_sidecar("config.toml.lock")
    }

    fn lock_artifact(&self, artifact: VaultArtifact) -> Result<fs::File> {
        self.lock_sidecar(&format!("{}.lock", artifact.file_stem()))
    }

    /// The vault lock: held exclusively by a mode switch (and by settings
    /// saves), shared by every artifact writer.
    fn lock_vault(&self, hold: Hold) -> Result<fs::File> {
        let dir = self.aic_dir();
        fs::create_dir_all(&dir)?;
        let handle = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(dir.join("vault.lock"))?;
        match hold {
            Hold::Shared => handle.lock_shared()?,
            Hold::Exclusive => handle.lock()?,
        }
        Ok(handle)
    }

    /// The vault mode on disk, under the vault lock, against the one `dek`
    /// assumes — and, encrypted, that `dek` is the vault's key. No
    /// `settings.toml` is no vault: nothing set up the mode a credential
    /// would be written in.
    ///
    /// The key matters as much as the mode. A session unlocked before a
    /// disable and a re-enable holds the **old** key while the flag reads
    /// encrypted again; checking only `dek.is_some()` lets it write a map no
    /// later unlock can open.
    fn vault_mode_is(&self, dek: Option<&crypto::Dek>) -> Result<()> {
        let assumed = dek.is_some();
        match self.load_settings()? {
            Some(settings) if settings.encrypt_keys == assumed => match dek {
                Some(dek) => self.key_is_current(dek),
                None => Ok(()),
            },
            Some(_) => Err(crate::Error::Crypto(format!(
                "the vault is no longer {}; re-run `aic login` and retry",
                if assumed { "encrypted" } else { "plain" }
            ))),
            None => Err(crate::Error::Crypto(
                "the vault is not set up yet; run `aic` to set it up".into(),
            )),
        }
    }

    /// Whether `dek` opens `keys.enc`, the one file an encrypted vault always
    /// has. Callers hold the vault lock.
    fn key_is_current(&self, dek: &crypto::Dek) -> Result<()> {
        let stale = || {
            crate::Error::Crypto(
                "the vault's key changed since this session unlocked; re-run `aic login` and retry"
                    .into(),
            )
        };
        let data =
            load_optional_file(&self.artifact_enc_path(VaultArtifact::Jwks))?.ok_or_else(stale)?;
        crypto::decrypt_data(&data, dek)
            .map(drop)
            .map_err(|_| stale())
    }

    /// Whether the vault mode is still the one a setup step decided from —
    /// `None` for no `settings.toml`. Anything else means another `aic`
    /// changed it in the meantime.
    fn expect_vault_mode(&self, expected: Option<bool>) -> Result<()> {
        let current = self.load_settings()?.map(|s| s.encrypt_keys);
        if current == expected {
            return Ok(());
        }
        let describe = |mode: Option<bool>| match mode {
            None => "not set up",
            Some(true) => "encrypted",
            Some(false) => "plain",
        };
        Err(crate::Error::Crypto(format!(
            "another aic changed the vault (it was {}, it is now {}); restart aic and retry",
            describe(expected),
            describe(current)
        )))
    }

    /// Move one artifact between forms — `from` and `to` are the DEK of each
    /// side, `None` for plain. Copies only: the old form is what the vault
    /// reads until the caller flips the flag, and is removed after that by
    /// [`ProjectPaths::settle`]. Callers hold the vault lock exclusively,
    /// which excludes every `update_artifact_map`.
    ///
    /// Idempotent, so a transition that died part way can be run again: a
    /// source that is gone while the destination exists is an artifact an
    /// earlier run (or an earlier version, which deleted per artifact)
    /// already moved, and the destination is kept — after checking it reads
    /// under `to`, because one this DEK cannot open belongs to some other
    /// setup. `required` writes an empty map only when
    /// **neither** form exists (the JWK map must always exist); writing it
    /// over a migrated destination would erase every credential in it.
    pub(crate) fn migrate_artifact(
        &self,
        artifact: VaultArtifact,
        from: Option<&crypto::Dek>,
        to: Option<&crypto::Dek>,
        required: bool,
    ) -> Result<()> {
        let _guard = self.lock_artifact(artifact)?;
        self.migration_preflight(artifact, from, to)?;
        let bytes = match self.load_artifact_bytes(artifact, from)? {
            Some(bytes) if !bytes.is_empty() => Some(bytes),
            Some(_) => Some(b"{}".to_vec()),
            None if required && !self.artifact_form_path(artifact, to).exists() => {
                Some(b"{}".to_vec())
            }
            None => None,
        };
        if let Some(bytes) = bytes {
            self.save_artifact_bytes(artifact, &bytes, to)?;
        }
        Ok(())
    }

    /// The one condition under which a migration keeps what it finds at the
    /// destination — source gone, destination present — requires that
    /// destination to read under `to`.
    fn migration_preflight(
        &self,
        artifact: VaultArtifact,
        from: Option<&crypto::Dek>,
        to: Option<&crypto::Dek>,
    ) -> Result<()> {
        if self.artifact_form_path(artifact, from).exists()
            || !self.artifact_form_path(artifact, to).exists()
        {
            return Ok(());
        }
        decode_json_map::<serde_json::Value>(self.load_artifact_bytes(artifact, to)?)
            .map(drop)
            .map_err(|e| {
                crate::Error::Crypto(format!(
                    "{} exists but cannot be read with this vault's key ({e}); \
                     it was written by another setup, so nothing was changed",
                    self.artifact_form_path(artifact, to)
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                ))
            })
    }

    /// The form `dek` selects: `<stem>.enc` under a key, else `<stem>.plain`.
    fn artifact_form_path(&self, artifact: VaultArtifact, dek: Option<&crypto::Dek>) -> PathBuf {
        match dek {
            Some(_) => self.artifact_enc_path(artifact),
            None => self.artifact_plain_path(artifact),
        }
    }

    /// The tenant-lifecycle lock: held by onboarding from its first credential
    /// write until the config row is recorded, and by offboarding from its
    /// identity check until the row is removed. Both key everything by tenant
    /// **name**, so without it an onboarding that has stored a replacement's
    /// key but not yet its row passes offboarding's identity check, and the
    /// replacement's key is deleted. Taken with `try_lock` — neither side
    /// waits on the other (offboarding holds it across a network call); the
    /// loser is told to retry.
    pub(crate) fn try_lock_lifecycle(&self) -> Result<fs::File> {
        let dir = self.aic_dir();
        fs::create_dir_all(&dir)?;
        let handle = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(dir.join("tenants.lock"))?;
        match handle.try_lock() {
            Ok(()) => Ok(handle),
            Err(fs::TryLockError::WouldBlock) => Err(crate::Error::Config(
                "another aic is adding or removing a tenant in this project; retry when it finishes"
                    .into(),
            )),
            Err(fs::TryLockError::Error(e)) => Err(e.into()),
        }
    }

    /// The lifecycle lock, held for a credential write to `expected` once
    /// the config shows that tenant is still there **and still the same
    /// one**. Offboarding and onboarding hold the same lock, so while a
    /// [`TenantHold`] lives neither can remove the row, replace it, or delete
    /// credentials keyed by its name — and a write that lost the race is
    /// refused rather than stored under a name that now means something else,
    /// or nothing.
    pub(crate) fn hold_tenant(&self, expected: &Tenant) -> Result<TenantHold> {
        let lock = self.try_lock_lifecycle()?;
        let current = self
            .load_config()?
            .and_then(|config| config.tenants.into_iter().find(|t| t.name == expected.name));
        match current {
            Some(current) if current.same_identity(expected) => Ok(TenantHold {
                _lock: lock,
                tenant: expected.name.clone(),
            }),
            Some(_) => Err(crate::Error::Config(format!(
                "tenant `{}` was replaced since this command read it (its base URL or service \
                 account changed); nothing was stored — re-run against the current entry",
                expected.name
            ))),
            None => Err(crate::Error::Config(format!(
                "tenant `{}` was removed since this command read it; nothing was stored",
                expected.name
            ))),
        }
    }

    /// An exclusive `flock` on `.aic/<name>`, held until the handle drops.
    fn lock_sidecar(&self, name: &str) -> Result<fs::File> {
        let dir = self.aic_dir();
        fs::create_dir_all(&dir)?;
        let handle = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(dir.join(name))?;
        handle.lock()?;
        Ok(handle)
    }

    /// Callers hold `lock_config`. Written to a sidecar, synced, then renamed,
    /// so a reader — the daemon stamps the file by content — never sees half
    /// a file. The sidecar is created fresh (`create_new`, a unique name), so
    /// a symlink or hard link planted at a predictable path cannot redirect
    /// the write, and it carries the existing file's mode — a `0600` config
    /// stays `0600` across the rename; a new one is `0600`. Content identical
    /// to what is on disk is not rewritten, so a no-op does not wake the
    /// daemon's content stamp or touch the mtime.
    fn install_config(&self, config: &ProjectConfig) -> Result<()> {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let contents = toml::to_string_pretty(config)?;
        let target = self.config_path();
        let existing = match fs::read(&target) {
            Ok(bytes) => Some(bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };
        if existing.as_deref() == Some(contents.as_bytes()) {
            return Ok(());
        }
        let mode = match fs::metadata(&target) {
            Ok(metadata) => metadata.permissions().mode() & 0o7777,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => 0o600,
            Err(e) => return Err(e.into()),
        };
        let temp = self
            .aic_dir()
            .join(format!("config.toml.new.{}", uuid::Uuid::new_v4()));
        let written = (|| -> Result<()> {
            let mut handle = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(mode)
                .open(&temp)?;
            // `mode` at open is filtered by the umask; set it exactly.
            handle.set_permissions(fs::Permissions::from_mode(mode))?;
            handle.write_all(contents.as_bytes())?;
            handle.sync_all()?;
            fs::rename(&temp, &target)?;
            Ok(())
        })();
        if written.is_err() {
            let _ = fs::remove_file(&temp);
        }
        written?;
        self.write_gitignore()?;
        Ok(())
    }

    pub(crate) fn save_keys_enc(&self, data: &[u8]) -> Result<()> {
        save_private_file(&self.artifact_enc_path(VaultArtifact::Jwks), data)
    }

    pub(crate) fn load_keys_enc(&self) -> Result<Option<Vec<u8>>> {
        load_optional_file(&self.artifact_enc_path(VaultArtifact::Jwks))
    }

    pub(crate) fn save_keys_plain(&self, data: &[u8]) -> Result<()> {
        save_private_file(&self.artifact_plain_path(VaultArtifact::Jwks), data)
    }

    pub(crate) fn load_keys_plain(&self) -> Result<Option<Vec<u8>>> {
        load_optional_file(&self.artifact_plain_path(VaultArtifact::Jwks))
    }
}

/// Written to a fresh `0600` sidecar (`create_new`, unique name), synced and
/// renamed into place, so a concurrent reader sees the old file or the new
/// one — never a truncated one, which the daemon would read as an empty map
/// and an offboarding probe as "no credential".
fn save_private_file(path: &Path, data: &[u8]) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let parent = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let temp = parent.join(format!("{name}.new.{}", uuid::Uuid::new_v4()));
    let written = (|| -> Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)?;
        file.write_all(data)?;
        file.sync_all()?;
        fs::rename(&temp, path)?;
        Ok(())
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temp);
    }
    written
}

fn remove_if_present(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
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

    /// `display_path` shortens a path the operator can reach from where they
    /// are, and leaves every other path whole. The discriminating case is the
    /// third: a sibling of the invocation directory shares a prefix with it
    /// textually but is not beneath it, and printing it relative would send
    /// the reader somewhere that does not exist.
    #[test]
    fn relative_to_shortens_only_paths_under_the_invocation_dir() {
        let base = Path::new("/home/dave/w/proj");

        assert_eq!(
            relative_to(Some(base), Path::new("/home/dave/w/proj/workspace/sandbox")),
            "workspace/sandbox"
        );
        assert_eq!(
            relative_to(Some(base), Path::new("/home/dave/w/other/file.js")),
            "/home/dave/w/other/file.js"
        );
        assert_eq!(
            relative_to(Some(base), Path::new("/home/dave/w/proj-old/file.js")),
            "/home/dave/w/proj-old/file.js"
        );
        // The invocation directory itself: "" would print as nothing at all.
        assert_eq!(relative_to(Some(base), base), "/home/dave/w/proj");
        // No bootstrap (a library caller, or a test): absolute, never a panic.
        assert_eq!(
            relative_to(None, Path::new("/home/dave/w/proj/workspace")),
            "/home/dave/w/proj/workspace"
        );
    }

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
    fn write_gitignore_to_uses_the_given_directory() {
        let dir = TestDir::new();
        let aic = dir.path(".aic");
        ProjectConfig::write_gitignore_to(&aic).unwrap();
        assert_eq!(
            fs::read_to_string(aic.join(".gitignore")).unwrap(),
            ProjectConfig::gitignore_content()
        );
    }

    /// Two `update_config` writers: the second blocks while the first holds
    /// the transaction, then sees its edit. Without the lock it would read
    /// the pre-edit file and its save would drop the first edit.
    /// The discriminating case for a credential save: an entry another
    /// process stored after this one loaded its copy survives, because the
    /// change is applied to the file under the lock, not to that copy.
    /// Record the vault mode directly, as a finished setup would have.
    fn vault_in(paths: &ProjectPaths, encrypt: bool) {
        paths
            .write_settings(&Settings {
                encrypt_keys: encrypt,
                ..Settings::default()
            })
            .unwrap();
    }

    #[test]
    fn an_artifact_update_merges_into_the_map_on_disk() {
        let dir = TestDir::new();
        let paths = ProjectPaths::new(dir.path("p")).unwrap();
        let dek = crypto::Dek::random();
        vault_in(&paths, true);
        // An encrypted vault's key is the one `keys.enc` opens under.
        paths
            .save_artifact_bytes(VaultArtifact::Jwks, b"{}", Some(&dek))
            .unwrap();
        paths
            .save_artifact_bytes(VaultArtifact::LogKeys, br#"{"other":{"k":1}}"#, Some(&dek))
            .unwrap();
        let (map, ()) = paths
            .update_artifact_map(VaultArtifact::LogKeys, Some(&dek), |map| {
                map.insert("mine".to_string(), serde_json::json!({"k": 2}));
                Ok(())
            })
            .unwrap();
        assert_eq!(map.len(), 2);
        let on_disk: HashMap<String, serde_json::Value> = decode_json_map(
            paths
                .load_artifact_bytes(VaultArtifact::LogKeys, Some(&dek))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(on_disk, map);
        // Absent and plain both work: no file is an empty map.
        vault_in(&paths, false);
        let (plain, ()) = paths
            .update_artifact_map(VaultArtifact::JwtBearerKeys, None, |map| {
                map.insert("t".to_string(), serde_json::json!(1));
                Ok(())
            })
            .unwrap();
        assert_eq!(plain.len(), 1);
    }

    /// A credential store for a tenant is held against the lifecycle lock
    /// and the config as it is under it: refused once the tenant is gone or
    /// another one holds its name, and, while held, offboarding and
    /// onboarding cannot start.
    #[test]
    fn a_tenant_hold_requires_the_same_tenant_and_blocks_the_lifecycle() {
        let dir = TestDir::new();
        let paths = ProjectPaths::new(dir.path("p")).unwrap();
        let tenant = Tenant {
            name: "uat".into(),
            base_url: "https://tenant.example.com".into(),
            theme: TenantTheme::Sandbox,
            sa_id: Some("sa-1".into()),
            scopes: Vec::new(),
            provenance: Provenance::default(),
        };
        let write = |tenants: Vec<Tenant>| {
            paths
                .update_or_create_config(
                    || {
                        Ok(ProjectConfig {
                            project: "t".into(),
                            default_tenant: String::new(),
                            tenants: Vec::new(),
                        })
                    },
                    |config| {
                        config.tenants = tenants;
                        Ok(())
                    },
                )
                .unwrap()
        };
        assert!(paths.hold_tenant(&tenant).is_err(), "absent tenant");
        write(vec![tenant.clone()]);
        let hold = paths.hold_tenant(&tenant).unwrap();
        assert_eq!(hold.tenant(), "uat");
        assert!(paths.try_lock_lifecycle().is_err());
        drop(hold);
        let mut replaced = tenant.clone();
        replaced.sa_id = Some("sa-2".into());
        write(vec![replaced]);
        assert!(paths.hold_tenant(&tenant).is_err(), "replaced tenant");
    }

    /// One lifecycle step at a time, and the loser is refused rather than
    /// made to wait; the lock is free again once the holder drops it.
    #[test]
    fn the_lifecycle_lock_refuses_a_second_holder() {
        let dir = TestDir::new();
        let paths = ProjectPaths::new(dir.path("p")).unwrap();
        let held = paths.try_lock_lifecycle().unwrap();
        assert!(paths.try_lock_lifecycle().is_err());
        drop(held);
        assert!(paths.try_lock_lifecycle().is_ok());
    }

    /// A writer that chose its mode before a migration is refused, not
    /// written into the form nothing reads — both directions.
    #[test]
    fn a_writer_in_the_old_vault_mode_is_refused() {
        let dir = TestDir::new();
        let paths = ProjectPaths::new(dir.path("p")).unwrap();
        let dek = crypto::Dek::random();
        paths.set_up_plain(None).unwrap();
        paths
            .save_artifact_bytes(VaultArtifact::Jwks, br#"{"a":1}"#, None)
            .unwrap();
        paths
            .enable_encryption(&dek, Some(false), || Ok(()))
            .unwrap();
        assert!(!paths.artifact_plain_path(VaultArtifact::Jwks).exists());
        let stale = paths.update_artifact_map(VaultArtifact::Jwks, None, |m| {
            m.insert("b".to_string(), serde_json::json!(2));
            Ok(())
        });
        assert!(stale.is_err());
        assert!(!paths.artifact_plain_path(VaultArtifact::Jwks).exists());
        // Control: the current mode writes, and the migrated entry is there.
        let (map, ()) = paths
            .update_artifact_map(VaultArtifact::Jwks, Some(&dek), |m| {
                m.insert("b".to_string(), serde_json::json!(2));
                Ok(())
            })
            .unwrap();
        assert_eq!(map.len(), 2);
        // And back: an encrypted writer after a disable is refused.
        paths.disable_encryption(&dek).unwrap();
        assert!(
            paths
                .update_artifact_map(
                    VaultArtifact::Jwks,
                    Some(&dek),
                    |_: &mut HashMap<String, serde_json::Value>| Ok(())
                )
                .is_err()
        );
    }

    /// The discriminating case for a per-artifact mode check: an artifact
    /// with no file in **either** form when encryption was enabled. Nothing
    /// on disk says the plain form is stale, so only the vault-wide mode can
    /// refuse a writer that chose plain before the switch.
    #[test]
    fn a_stale_writer_is_refused_for_an_artifact_that_never_existed() {
        let dir = TestDir::new();
        let paths = ProjectPaths::new(dir.path("p")).unwrap();
        let dek = crypto::Dek::random();
        paths.set_up_plain(None).unwrap();
        paths
            .enable_encryption(&dek, Some(false), || Ok(()))
            .unwrap();
        assert!(!paths.artifact_enc_path(VaultArtifact::LogKeys).exists());
        let stale = paths.update_artifact_map(VaultArtifact::LogKeys, None, |m| {
            m.insert("t".to_string(), serde_json::json!(1));
            Ok(())
        });
        assert!(stale.is_err());
        assert!(!paths.artifact_plain_path(VaultArtifact::LogKeys).exists());
    }

    /// A transition that died after moving the JWK map but before flipping
    /// the flag is run again: the source is gone, and the moved map must
    /// survive rather than be replaced by the empty map `required` writes
    /// for a vault with no keys at all.
    #[test]
    fn retrying_a_finished_migration_keeps_the_moved_map() {
        let dir = TestDir::new();
        let paths = ProjectPaths::new(dir.path("p")).unwrap();
        let dek = crypto::Dek::random();
        paths.set_up_plain(None).unwrap();
        paths
            .save_artifact_bytes(VaultArtifact::Jwks, br#"{"a":1}"#, None)
            .unwrap();
        paths
            .migrate_artifact(VaultArtifact::Jwks, None, Some(&dek), true)
            .unwrap();
        // An earlier version deleted each source as it went.
        fs::remove_file(paths.artifact_plain_path(VaultArtifact::Jwks)).unwrap();
        // The flag never flipped; the operator retries.
        paths
            .enable_encryption(&dek, Some(false), || Ok(()))
            .unwrap();
        let map: HashMap<String, serde_json::Value> = decode_json_map(
            paths
                .load_artifact_bytes(VaultArtifact::Jwks, Some(&dek))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(map.len(), 1, "{map:?}");
    }

    fn jwk_map(
        paths: &ProjectPaths,
        dek: Option<&crypto::Dek>,
    ) -> HashMap<String, serde_json::Value> {
        decode_json_map(paths.load_artifact_bytes(VaultArtifact::Jwks, dek).unwrap()).unwrap()
    }

    /// An enable that died after copying but before its flag flipped leaves
    /// the plain vault working — its files are all still there — and a
    /// retry under a **different** key goes through. Deleting each source as
    /// it was copied made the first false and the second impossible: the
    /// copy under the lost key blocked every later attempt.
    #[test]
    fn an_enable_that_died_before_its_flag_is_retried_under_a_new_key() {
        let dir = TestDir::new();
        let paths = ProjectPaths::new(dir.path("p")).unwrap();
        paths.set_up_plain(None).unwrap();
        paths
            .save_artifact_bytes(VaultArtifact::Jwks, br#"{"a":1}"#, None)
            .unwrap();
        paths
            .save_artifact_bytes(VaultArtifact::LogKeys, br#"{"t":1}"#, None)
            .unwrap();
        let lost = crypto::Dek::random();
        for &artifact in VaultArtifact::ALL {
            paths
                .migrate_artifact(artifact, None, Some(&lost), artifact == VaultArtifact::Jwks)
                .unwrap();
        }
        // The plain vault still reads and writes.
        assert_eq!(jwk_map(&paths, None).len(), 1);
        paths
            .update_artifact_map(VaultArtifact::Jwks, None, |m| {
                m.insert("b".into(), serde_json::json!(2));
                Ok(())
            })
            .unwrap();
        let retry = crypto::Dek::random();
        paths
            .enable_encryption(&retry, Some(false), || Ok(()))
            .unwrap();
        assert_eq!(jwk_map(&paths, Some(&retry)).len(), 2);
        for &artifact in VaultArtifact::ALL {
            assert!(
                !paths.artifact_plain_path(artifact).exists(),
                "{artifact:?}"
            );
        }
    }

    /// A disable that died before its flag flipped leaves a vault that still
    /// unlocks — `keys.enc` and `wraps.toml` are removed only after the
    /// flag — and the cleartext copy it made is removed by the next write
    /// rather than left beside an "encrypted" vault.
    #[test]
    fn a_disable_that_died_before_its_flag_still_unlocks() {
        let dir = TestDir::new();
        let paths = ProjectPaths::new(dir.path("p")).unwrap();
        let dek = crypto::Dek::random();
        paths.set_up_plain(None).unwrap();
        paths
            .save_artifact_bytes(VaultArtifact::Jwks, br#"{"a":1}"#, None)
            .unwrap();
        paths
            .enable_encryption(&dek, Some(false), || {
                paths.save_wraps(&wraps::WrapsFile::default())
            })
            .unwrap();
        paths
            .migrate_artifact(VaultArtifact::Jwks, Some(&dek), None, true)
            .unwrap();
        assert!(paths.artifact_plain_path(VaultArtifact::Jwks).exists());
        assert!(paths.load_wraps().unwrap().is_some());
        assert_eq!(jwk_map(&paths, Some(&dek)).len(), 1);
        paths
            .update_artifact_map(
                VaultArtifact::Jwks,
                Some(&dek),
                |_: &mut HashMap<String, serde_json::Value>| Ok(()),
            )
            .unwrap();
        assert!(!paths.artifact_plain_path(VaultArtifact::Jwks).exists());

        // Completed, it removes the encrypted side and the wraps.
        paths.disable_encryption(&dek).unwrap();
        assert_eq!(jwk_map(&paths, None).len(), 1);
        assert!(!paths.artifact_enc_path(VaultArtifact::Jwks).exists());
        assert!(paths.load_wraps().unwrap().is_none());
    }

    /// A session unlocked before a disable and re-enable holds the old key
    /// while the flag reads encrypted again. Its writes, reads and wrap
    /// edits are refused; the current key's go through.
    #[test]
    fn a_session_holding_a_replaced_key_is_refused() {
        let dir = TestDir::new();
        let paths = ProjectPaths::new(dir.path("p")).unwrap();
        let old = crypto::Dek::random();
        let new = crypto::Dek::random();
        paths.set_up_plain(None).unwrap();
        paths
            .enable_encryption(&old, Some(false), || Ok(()))
            .unwrap();
        paths.disable_encryption(&old).unwrap();
        paths
            .enable_encryption(&new, Some(false), || {
                paths.save_wraps(&wraps::WrapsFile::default())
            })
            .unwrap();
        let write = |dek: &crypto::Dek| {
            paths.update_artifact_map(VaultArtifact::LogKeys, Some(dek), |m| {
                m.insert("t".into(), serde_json::json!(1));
                Ok(())
            })
        };
        assert!(write(&old).is_err());
        assert!(!paths.artifact_enc_path(VaultArtifact::LogKeys).exists());
        assert!(
            paths
                .read_artifact_bytes(VaultArtifact::Jwks, Some(&old))
                .is_err()
        );
        assert!(paths.update_wraps(&old, |_| Ok(())).is_err());
        assert!(paths.disable_encryption(&old).is_err());
        write(&new).unwrap();
        assert!(
            paths
                .read_artifact_bytes(VaultArtifact::LogKeys, Some(&new))
                .unwrap()
                .is_some()
        );
        paths.update_wraps(&new, |_| Ok(())).unwrap();
    }

    /// A reader in the old mode gets an error, not an absent file: the
    /// daemon reported that as "no credential", and offboarding as nothing
    /// to remove.
    #[test]
    fn a_read_in_the_old_mode_is_an_error_not_an_absence() {
        let dir = TestDir::new();
        let paths = ProjectPaths::new(dir.path("p")).unwrap();
        let dek = crypto::Dek::random();
        paths.set_up_plain(None).unwrap();
        paths
            .save_artifact_bytes(VaultArtifact::Jwks, br#"{"a":1}"#, None)
            .unwrap();
        assert!(
            paths
                .read_artifact_bytes(VaultArtifact::Jwks, None)
                .unwrap()
                .is_some()
        );
        paths
            .enable_encryption(&dek, Some(false), || Ok(()))
            .unwrap();
        assert!(
            paths
                .read_artifact_bytes(VaultArtifact::Jwks, None)
                .is_err()
        );
    }

    /// An empty `settings.toml` parses as every default — plain — so it is
    /// refused rather than read as a plain vault over encrypted files.
    #[test]
    fn an_empty_settings_file_is_refused() {
        let dir = TestDir::new();
        let paths = ProjectPaths::new(dir.path("p")).unwrap();
        paths.set_up_plain(None).unwrap();
        fs::write(paths.settings_path(), " \n").unwrap();
        assert!(paths.load_settings().is_err());
        assert!(
            paths
                .update_artifact_map(
                    VaultArtifact::Jwks,
                    None,
                    |_: &mut HashMap<String, serde_json::Value>| Ok(())
                )
                .is_err()
        );
    }

    /// Two sessions edit the wraps from their own copies: each edit applies
    /// to the file as it is, so neither loses the other's factor, and
    /// removing what is now the last factor is refused.
    #[test]
    fn wrap_edits_apply_to_the_file_on_disk() {
        let dir = TestDir::new();
        let paths = ProjectPaths::new(dir.path("p")).unwrap();
        let dek = crypto::Dek::random();
        let key = |id: &str| wraps::Wrap::SecurityKey {
            label: None,
            credential_id: id.into(),
            rp_id: "aic".into(),
            nonce: String::new(),
            ciphertext: String::new(),
        };
        let mut start = wraps::WrapsFile::default();
        start.add_factor(key("a"), Some("salt")).unwrap();
        paths.set_up_plain(None).unwrap();
        paths
            .enable_encryption(&dek, Some(false), || paths.save_wraps(&start))
            .unwrap();

        // Session one adds b; session two, from its older copy, renames a.
        paths
            .update_wraps(&dek, |w| w.add_factor(key("b"), Some("salt")))
            .unwrap();
        let after = paths
            .update_wraps(&dek, |w| w.rename_factor("a", "renamed"))
            .unwrap();
        assert_eq!(after.wraps.len(), 2);
        assert_eq!(after.wraps[0].label(), "renamed");

        // A key enrolled against another salt would never unlock.
        assert!(
            paths
                .update_wraps(&dek, |w| w.add_factor(key("c"), Some("other")))
                .is_err()
        );

        // Both sessions remove a different key; the second is refused.
        paths
            .update_wraps(&dek, |w| w.remove_factor(&key("a")))
            .unwrap();
        assert!(
            paths
                .update_wraps(&dek, |w| w.remove_factor(&key("b")))
                .is_err()
        );
        assert_eq!(paths.load_wraps().unwrap().unwrap().wraps.len(), 1);
    }

    /// The legacy sole copy `settle` keeps: the vault reads plain, but the
    /// map exists only encrypted. A write would create `keys.plain` and the
    /// stale-form discard would then delete the original; a read would call
    /// every credential absent. Both are refused, and the file survives.
    #[test]
    fn a_map_only_in_the_other_form_is_neither_read_nor_overwritten() {
        let dir = TestDir::new();
        let paths = ProjectPaths::new(dir.path("p")).unwrap();
        let dek = crypto::Dek::random();
        paths.set_up_plain(None).unwrap();
        fs::remove_file(paths.artifact_plain_path(VaultArtifact::Jwks)).unwrap();
        paths
            .save_artifact_bytes(VaultArtifact::Jwks, br#"{"a":1}"#, Some(&dek))
            .unwrap();
        let write = paths.update_artifact_map(VaultArtifact::Jwks, None, |m| {
            m.insert("b".into(), serde_json::json!(2));
            Ok(())
        });
        assert!(write.is_err());
        assert!(
            paths
                .read_artifact_bytes(VaultArtifact::Jwks, None)
                .is_err()
        );
        assert_eq!(jwk_map(&paths, Some(&dek)).len(), 1);
        assert!(!paths.artifact_plain_path(VaultArtifact::Jwks).exists());
        // Control: neither form is an empty map, not an error.
        assert!(
            paths
                .read_artifact_bytes(VaultArtifact::LogKeys, None)
                .unwrap()
                .is_none()
        );
    }

    /// A save that died between writing its sidecar and renaming it left a
    /// `<name>.new.<uuid>` — for `keys.plain`, a cleartext map. Settling
    /// removes exactly those, and nothing that merely resembles one.
    #[test]
    fn settling_removes_abandoned_sidecars_only() {
        let dir = TestDir::new();
        let paths = ProjectPaths::new(dir.path("p")).unwrap();
        paths.set_up_plain(None).unwrap();
        let id = uuid::Uuid::new_v4();
        let aic = paths.aic_dir();
        let abandoned = [
            aic.join(format!("keys.plain.new.{id}")),
            aic.join(format!("settings.toml.new.{id}")),
        ];
        let kept = [
            aic.join("keys.plain.new.not-a-uuid"),
            aic.join(format!("config.toml.new.{id}")),
        ];
        for path in abandoned.iter().chain(&kept) {
            fs::write(path, "x").unwrap();
        }
        {
            let _vault = paths.lock_vault(Hold::Exclusive).unwrap();
            paths.settle().unwrap();
        }
        for path in &abandoned {
            assert!(!path.exists(), "{}", path.display());
        }
        for path in &kept {
            assert!(path.exists(), "{}", path.display());
        }
    }

    /// The flag is the last thing a transition writes. A `.gitignore` that
    /// cannot be written used to fail **after** the flag flipped, so a
    /// disable reported failure for a vault already plain, and the caller
    /// kept its encrypted session. Now it fails before, and nothing moved.
    #[test]
    fn a_transition_that_reports_failure_has_not_committed() {
        let dir = TestDir::new();
        let paths = ProjectPaths::new(dir.path("p")).unwrap();
        let dek = crypto::Dek::random();
        paths.set_up_plain(None).unwrap();
        paths
            .enable_encryption(&dek, Some(false), || Ok(()))
            .unwrap();
        let gitignore = paths.aic_dir().join(".gitignore");
        fs::remove_file(&gitignore).unwrap();
        fs::create_dir(&gitignore).unwrap();
        assert!(paths.disable_encryption(&dek).is_err());
        assert_eq!(
            paths.load_settings().unwrap().map(|s| s.encrypt_keys),
            Some(true)
        );
        assert_eq!(jwk_map(&paths, Some(&dek)).len(), 0);
        paths.hold_vault_key(&dek).unwrap();
    }

    /// The daemon's gate on a `PutDek`: the vault's key, and only while the
    /// vault is encrypted under it.
    #[test]
    fn only_the_vaults_key_is_accepted() {
        let dir = TestDir::new();
        let paths = ProjectPaths::new(dir.path("p")).unwrap();
        let dek = crypto::Dek::random();
        paths.set_up_plain(None).unwrap();
        assert!(paths.hold_vault_key(&dek).is_err());
        paths
            .enable_encryption(&dek, Some(false), || Ok(()))
            .unwrap();
        paths.hold_vault_key(&dek).unwrap();
        assert!(paths.hold_vault_key(&crypto::Dek::random()).is_err());
    }

    /// A first run that died after `keys.enc` and its wrap but before
    /// `settings.toml` is recognised and finished in place with the key the
    /// wrap opens; any other key is refused. A plaintext copy beside it is
    /// not a first run — which copy is current cannot be told — so it is
    /// neither recognised nor finished, and nothing is copied over the
    /// encrypted map.
    #[test]
    fn an_interrupted_first_run_is_finished_with_its_own_key() {
        let dir = TestDir::new();
        let paths = ProjectPaths::new(dir.path("p")).unwrap();
        let dek = crypto::Dek::random();
        assert!(!paths.interrupted_first_run());
        paths.save_wraps(&wraps::WrapsFile::default()).unwrap();
        paths
            .save_artifact_bytes(VaultArtifact::Jwks, br#"{"a":1,"b":2}"#, Some(&dek))
            .unwrap();
        assert!(paths.interrupted_first_run());

        paths
            .save_artifact_bytes(VaultArtifact::LogKeys, b"{}", None)
            .unwrap();
        assert!(!paths.interrupted_first_run());
        assert!(paths.finish_first_run(&dek).is_err());
        fs::remove_file(paths.artifact_plain_path(VaultArtifact::LogKeys)).unwrap();

        assert!(paths.finish_first_run(&crypto::Dek::random()).is_err());
        assert_eq!(paths.load_settings().unwrap().map(|s| s.encrypt_keys), None);
        paths.finish_first_run(&dek).unwrap();
        assert!(!paths.interrupted_first_run());
        assert_eq!(
            paths.load_settings().unwrap().map(|s| s.encrypt_keys),
            Some(true)
        );
        assert_eq!(jwk_map(&paths, Some(&dek)).len(), 2);
    }

    /// The daemon installs a key while holding the lock it checked it
    /// under, so a transition cannot replace the key in between: one that
    /// starts during the hold waits for it.
    #[test]
    fn a_held_vault_key_holds_off_a_transition() {
        let dir = TestDir::new();
        let paths = ProjectPaths::new(dir.path("p")).unwrap();
        let dek = crypto::Dek::random();
        paths.set_up_plain(None).unwrap();
        paths
            .enable_encryption(&dek, Some(false), || Ok(()))
            .unwrap();
        let guard = paths.hold_vault_key(&dek).unwrap();
        let disable = {
            let (paths, dek) = (paths.clone(), dek.clone());
            std::thread::spawn(move || paths.disable_encryption(&dek))
        };
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert_eq!(
            paths.load_settings().unwrap().map(|s| s.encrypt_keys),
            Some(true)
        );
        drop(guard);
        disable.join().unwrap().unwrap();
        assert_eq!(
            paths.load_settings().unwrap().map(|s| s.encrypt_keys),
            Some(false)
        );
    }

    /// A destination this key cannot read is some other setup's, and is
    /// refused before the wrap is replaced — the step that would make it
    /// unreadable for good.
    #[test]
    fn a_destination_under_another_key_stops_the_transition_before_the_wrap() {
        let dir = TestDir::new();
        let paths = ProjectPaths::new(dir.path("p")).unwrap();
        let theirs = crypto::Dek::random();
        let mine = crypto::Dek::random();
        paths
            .save_artifact_bytes(VaultArtifact::Jwks, br#"{"a":1}"#, Some(&theirs))
            .unwrap();
        let mut wrapped = false;
        let refused = paths.enable_encryption(&mine, None, || {
            wrapped = true;
            Ok(())
        });
        assert!(refused.is_err());
        assert!(!wrapped);
        assert_eq!(paths.load_settings().unwrap().map(|s| s.encrypt_keys), None);
    }

    /// Two first-run screens: the second to commit finds the vault already
    /// set up and is refused before its wrap is written, either way round.
    #[test]
    fn a_second_first_run_setup_is_refused() {
        let dir = TestDir::new();
        let paths = ProjectPaths::new(dir.path("p")).unwrap();
        let first = crypto::Dek::random();
        paths.enable_encryption(&first, None, || Ok(())).unwrap();
        let mut wrapped = false;
        let second = paths.enable_encryption(&crypto::Dek::random(), None, || {
            wrapped = true;
            Ok(())
        });
        assert!(second.is_err());
        assert!(!wrapped);
        assert!(paths.set_up_plain(None).is_err());
        assert!(paths.decrypt_keys_file(&first).is_ok());
        // The case the destination check cannot see: the other setup chose
        // plain, so there is nothing under a foreign key to trip over —
        // only the recorded mode says this screen's "not set up" is stale.
        let other = TestDir::new();
        let plain_first = ProjectPaths::new(other.path("p")).unwrap();
        plain_first.set_up_plain(None).unwrap();
        let mut wrapped = false;
        let late = plain_first.enable_encryption(&crypto::Dek::random(), None, || {
            wrapped = true;
            Ok(())
        });
        assert!(late.is_err());
        assert!(!wrapped);
        assert!(
            plain_first
                .artifact_plain_path(VaultArtifact::Jwks)
                .exists()
        );
        // Nor does naming the mode it is in make a plain setup legal: that
        // would flip the flag over files still encrypted.
        assert!(paths.set_up_plain(Some(true)).is_err());
        // Control: a vault nothing set up takes a plain setup.
        let other = TestDir::new();
        let fresh = ProjectPaths::new(other.path("p")).unwrap();
        fresh.set_up_plain(None).unwrap();
        assert_eq!(
            fresh.load_settings().unwrap().map(|s| s.encrypt_keys),
            Some(false)
        );
    }

    /// A settings edit loaded before a vault transition and saved after it
    /// leaves the transition's flag alone.
    #[test]
    fn a_settings_save_does_not_revert_the_vault_mode() {
        let dir = TestDir::new();
        let paths = ProjectPaths::new(dir.path("p")).unwrap();
        paths.set_up_plain(None).unwrap();
        let mut stale = paths.load_settings().unwrap().unwrap();
        paths
            .enable_encryption(&crypto::Dek::random(), Some(false), || Ok(()))
            .unwrap();
        stale.agent_idle_timeout_secs = Some(5);
        paths.save_settings(&stale).unwrap();
        let now = paths.load_settings().unwrap().unwrap();
        assert!(now.encrypt_keys);
        assert_eq!(now.agent_idle_timeout_secs, Some(5));
    }

    /// The migration waits for a writer holding the map, and carries its
    /// entry over instead of publishing the map it read before it.
    #[test]
    fn a_migration_carries_over_a_concurrent_write() {
        use std::sync::mpsc;
        use std::time::Duration;
        let dir = TestDir::new();
        let paths = ProjectPaths::new(dir.path("p")).unwrap();
        let dek = crypto::Dek::random();
        paths.set_up_plain(None).unwrap();
        paths
            .save_artifact_bytes(VaultArtifact::LogKeys, br#"{"a":1}"#, None)
            .unwrap();
        let (inside_tx, inside_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let writer = {
            let paths = paths.clone();
            std::thread::spawn(move || {
                paths
                    .update_artifact_map(VaultArtifact::LogKeys, None, |m| {
                        inside_tx.send(()).unwrap();
                        release_rx.recv().unwrap();
                        m.insert("b".to_string(), serde_json::json!(2));
                        Ok(())
                    })
                    .unwrap();
            })
        };
        inside_rx.recv().unwrap();
        let (done_tx, done_rx) = mpsc::channel();
        let migration = {
            let paths = paths.clone();
            let dek = dek.clone();
            std::thread::spawn(move || {
                paths
                    .enable_encryption(&dek, Some(false), || Ok(()))
                    .unwrap();
                done_tx.send(()).unwrap();
            })
        };
        assert!(
            done_rx.recv_timeout(Duration::from_millis(300)).is_err(),
            "the migration ran while a writer held the map"
        );
        release_tx.send(()).unwrap();
        writer.join().unwrap();
        migration.join().unwrap();
        let map: HashMap<String, serde_json::Value> = decode_json_map(
            paths
                .load_artifact_bytes(VaultArtifact::LogKeys, Some(&dek))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(map.len(), 2, "{map:?}");
    }

    /// A private file is replaced by rename: the old inode is never
    /// truncated in place, and no sidecar is left behind.
    #[test]
    fn a_private_file_is_replaced_not_truncated() {
        use std::os::unix::fs::MetadataExt;
        let dir = TestDir::new();
        let path = dir.path("secret");
        save_private_file(&path, b"old").unwrap();
        let before = fs::metadata(&path).unwrap().ino();
        let reader = fs::File::open(&path).unwrap();
        save_private_file(&path, b"new").unwrap();
        assert_ne!(fs::metadata(&path).unwrap().ino(), before);
        assert_eq!(reader.metadata().unwrap().len(), 3);
        assert_eq!(fs::read(&path).unwrap(), b"new");
        assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
        let litter: Vec<_> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".new."))
            .collect();
        assert!(litter.is_empty());
    }

    /// Two writers of one map, the first holding the transaction open: the
    /// second waits, then sees and keeps the first's entry.
    #[test]
    fn artifact_updates_serialise_and_lose_no_entry() {
        use std::sync::mpsc;
        use std::time::Duration;
        let dir = TestDir::new();
        let paths = ProjectPaths::new(dir.path("p")).unwrap();
        vault_in(&paths, false);
        let (inside_tx, inside_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let first = {
            let paths = paths.clone();
            std::thread::spawn(move || {
                paths
                    .update_artifact_map(VaultArtifact::Jwks, None, |map| {
                        inside_tx.send(()).unwrap();
                        release_rx.recv().unwrap();
                        map.insert("a".to_string(), serde_json::json!(1));
                        Ok(())
                    })
                    .unwrap();
            })
        };
        inside_rx.recv().unwrap();
        let (done_tx, done_rx) = mpsc::channel();
        let second = {
            let paths = paths.clone();
            std::thread::spawn(move || {
                paths
                    .update_artifact_map(VaultArtifact::Jwks, None, |map| {
                        map.insert("b".to_string(), serde_json::json!(2));
                        Ok(())
                    })
                    .unwrap();
                done_tx.send(()).unwrap();
            })
        };
        assert!(
            done_rx.recv_timeout(Duration::from_millis(300)).is_err(),
            "the second writer finished while the first held the transaction"
        );
        release_tx.send(()).unwrap();
        first.join().unwrap();
        second.join().unwrap();
        let map: HashMap<String, serde_json::Value> = decode_json_map(
            paths
                .load_artifact_bytes(VaultArtifact::Jwks, None)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(map.len(), 2, "{map:?}");
    }

    #[test]
    fn update_config_serialises_writers_and_loses_no_edit() {
        use std::sync::mpsc;
        use std::time::Duration;
        let dir = TestDir::new();
        let paths = ProjectPaths::new(dir.path("p")).unwrap();
        paths
            .save_config(&ProjectConfig {
                project: "x".into(),
                default_tenant: "t".into(),
                tenants: Vec::new(),
            })
            .unwrap();

        let (inside_tx, inside_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let first = {
            let paths = paths.clone();
            std::thread::spawn(move || {
                paths
                    .update_config(|cfg| {
                        inside_tx.send(()).unwrap();
                        release_rx.recv().unwrap();
                        cfg.project.push('a');
                        Ok(())
                    })
                    .unwrap();
            })
        };
        inside_rx.recv().unwrap();
        let (done_tx, done_rx) = mpsc::channel();
        let second = {
            let paths = paths.clone();
            std::thread::spawn(move || {
                paths
                    .update_config(|cfg| {
                        cfg.project.push('b');
                        Ok(())
                    })
                    .unwrap();
                done_tx.send(()).unwrap();
            })
        };
        assert!(
            done_rx.recv_timeout(Duration::from_millis(300)).is_err(),
            "the second writer finished while the first held the transaction"
        );
        release_tx.send(()).unwrap();
        first.join().unwrap();
        second.join().unwrap();
        assert_eq!(paths.load_config().unwrap().unwrap().project, "xab");
        let litter: Vec<_> = fs::read_dir(paths.aic_dir())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("config.toml.new")
            })
            .collect();
        assert!(litter.is_empty(), "sidecars left behind: {litter:?}");
    }

    /// The rename must not widen a private config: the replacement carries
    /// the existing file's mode.
    #[test]
    fn a_save_keeps_the_config_files_mode() {
        let tmp = TestDir::new();
        let paths = ProjectPaths::new(tmp.path("p")).unwrap();
        let mut config = ProjectConfig {
            project: "x".into(),
            default_tenant: "t".into(),
            tenants: Vec::new(),
        };
        paths.save_config(&config).unwrap();
        fs::set_permissions(paths.config_path(), fs::Permissions::from_mode(0o640)).unwrap();
        config.project = "changed".into();
        paths.save_config(&config).unwrap();
        let mode = fs::metadata(paths.config_path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o640);
    }

    /// A no-op update writes nothing: the inode is the same afterwards.
    #[test]
    fn an_unchanged_update_does_not_rewrite_the_config() {
        use std::os::unix::fs::MetadataExt;
        let tmp = TestDir::new();
        let paths = ProjectPaths::new(tmp.path("p")).unwrap();
        paths
            .save_config(&ProjectConfig {
                project: "x".into(),
                default_tenant: "t".into(),
                tenants: Vec::new(),
            })
            .unwrap();
        let before = fs::metadata(paths.config_path()).unwrap().ino();
        paths.update_config(|_| Ok(())).unwrap();
        assert_eq!(fs::metadata(paths.config_path()).unwrap().ino(), before);
        // Control: a real change replaces the file.
        paths
            .update_config(|c| {
                c.project = "y".into();
                Ok(())
            })
            .unwrap();
        assert_ne!(fs::metadata(paths.config_path()).unwrap().ino(), before);
    }

    /// The discriminating case for comparing parsed documents: a hand-edited
    /// file whose bytes are not ours keeps its comment through a no-op.
    #[test]
    fn a_no_op_update_keeps_a_hand_edited_config() {
        let tmp = TestDir::new();
        let paths = ProjectPaths::new(tmp.path("p")).unwrap();
        fs::create_dir_all(paths.aic_dir()).unwrap();
        let hand = "# my notes\ndefault_tenant = \"t\"\nproject = \"x\"\ntenants = []\n";
        fs::write(paths.config_path(), hand).unwrap();
        paths.update_config(|_| Ok(())).unwrap();
        assert_eq!(fs::read_to_string(paths.config_path()).unwrap(), hand);
    }

    /// A link planted at a sidecar path is not followed: the sidecar name is
    /// unique and created fresh, so the planted target stays untouched.
    #[test]
    fn a_planted_sidecar_link_is_not_written_through() {
        let tmp = TestDir::new();
        let paths = ProjectPaths::new(tmp.path("p")).unwrap();
        fs::create_dir_all(paths.aic_dir()).unwrap();
        let victim = tmp.path("victim");
        fs::write(&victim, "untouched").unwrap();
        std::os::unix::fs::symlink(&victim, paths.aic_dir().join("config.toml.new")).unwrap();
        paths
            .save_config(&ProjectConfig {
                project: "x".into(),
                default_tenant: "t".into(),
                tenants: Vec::new(),
            })
            .unwrap();
        assert_eq!(fs::read_to_string(&victim).unwrap(), "untouched");
    }

    #[test]
    fn project_paths_isolate_concurrent_multi_step_writes() {
        // Regression: each writer used to resolve `.aic` again for every
        // syscall, so a concurrent cwd change could split one logical write
        // across projects. Explicit absolute roots must stay independent.
        let left_dir = TestDir::new();
        let right_dir = TestDir::new();
        let left = ProjectPaths::new(left_dir.path("left")).unwrap();
        let right = ProjectPaths::new(right_dir.path("right")).unwrap();

        assert!(ProjectPaths::new(PathBuf::from("relative")).is_err());
        assert!(left.aic_dir().is_absolute());
        assert!(right.workspace_dir().is_absolute());

        let write = |paths: ProjectPaths, name: &'static str| {
            std::thread::spawn(move || {
                let config = ProjectConfig {
                    project: name.into(),
                    default_tenant: name.into(),
                    tenants: Vec::new(),
                };
                let settings = Settings {
                    operator: Operator {
                        name: Some(name.into()),
                        host: None,
                    },
                    ..Settings::default()
                };
                for _ in 0..16 {
                    paths.save_config(&config).unwrap();
                    paths.save_settings(&settings).unwrap();
                    paths.save_wraps(&wraps::WrapsFile::default()).unwrap();
                    paths.write_current_context(name).unwrap();
                    paths.save_keys_plain(name.as_bytes()).unwrap();
                }
            })
        };

        let left_write = write(left.clone(), "left");
        let right_write = write(right.clone(), "right");
        left_write.join().unwrap();
        right_write.join().unwrap();

        assert_eq!(left.load_config().unwrap().unwrap().project, "left");
        assert_eq!(right.load_config().unwrap().unwrap().project, "right");
        assert_eq!(
            left.read_current_context().unwrap().as_deref(),
            Some("left")
        );
        assert_eq!(
            right.read_current_context().unwrap().as_deref(),
            Some("right")
        );
        assert_eq!(left.load_keys_plain().unwrap().unwrap(), b"left");
        assert_eq!(right.load_keys_plain().unwrap().unwrap(), b"right");
        assert!(left.load_wraps().unwrap().is_some());
        assert!(right.load_wraps().unwrap().is_some());
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
        // Same class, and the one that got missed: `aic saml rotate` writes
        // .aic/saml-rotations.json, which names the tenant, its realms and its
        // entity ids. This repo's own top-level .gitignore has `.aic/`, so the
        // gap is invisible here — it opens in a scaffolded project whose
        // .gitignore does not, where this file is the only cover. Any new
        // runtime file written under ProjectConfig::dir() needs a line here.
        // Derived from the journal module's own path helpers rather than
        // spelled again here, so renaming a sidecar cannot leave this test
        // asserting the old name while production writes the new one.
        use crate::saml::rotate::journal::{JOURNAL_FILE, lock_path, temp_path};
        let journal = Path::new(JOURNAL_FILE);
        for runtime_file in [
            journal.to_path_buf(),
            lock_path(journal),
            temp_path(journal),
        ] {
            let name = runtime_file.to_string_lossy().into_owned();
            assert!(
                content.lines().any(|line| line == name),
                "a new .aic/ runtime file must be added to gitignore_content: {name}"
            );
        }
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
