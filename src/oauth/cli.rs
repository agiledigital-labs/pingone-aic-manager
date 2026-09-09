//! `aic oauth` parser and command implementation.

use std::io::ErrorKind;
use std::path::{Path, PathBuf, is_separator};

use base64::Engine as _;
use clap::{Args, Subcommand};
use rand::RngCore;
use serde_json::Value;

use crate::cli::diff::show_diff;
use crate::cli::{print_json, print_table, prod_hint, read_password_line, realm_arg, tenant_for};
use crate::config::ProjectConfig;
use crate::oauth::{api, spec};
use crate::{Error, Result};

#[derive(Args, Debug)]
pub struct CreateArgs {
    /// Seed the template from a JSON object (including `aic oauth pull` output).
    #[arg(long, value_name = "FILE")]
    from: Option<PathBuf>,
    /// coreOAuth2ClientConfig.clientName (one array value).
    #[arg(long)]
    name: Option<String>,
    /// advancedOAuth2ClientConfig.descriptions (one array value).
    #[arg(long)]
    description: Option<String>,
    /// coreOAuth2ClientConfig.clientType. Defaults to Confidential unless
    /// --from supplies a value.
    #[arg(long, value_name = "TYPE")]
    client_type: Option<String>,
    /// Read coreOAuth2ClientConfig.userpassword as one line from stdin. The
    /// write-only value reads back as null and cannot be recovered.
    #[arg(long, conflicts_with = "generate_secret")]
    secret_stdin: bool,
    /// Generate a 256-bit userpassword and print it once after success. The
    /// write-only value reads back as null and cannot be recovered.
    #[arg(long)]
    generate_secret: bool,
    /// coreOAuth2ClientConfig.scopes (repeatable).
    #[arg(long)]
    scope: Vec<String>,
    /// coreOAuth2ClientConfig.defaultScopes (repeatable).
    #[arg(long)]
    default_scope: Vec<String>,
    /// coreOAuth2ClientConfig.redirectionUris (repeatable).
    #[arg(long)]
    redirect_uri: Vec<String>,
    /// advancedOAuth2ClientConfig.grantTypes (repeatable; live-schema validated).
    #[arg(long)]
    grant: Vec<String>,
    /// advancedOAuth2ClientConfig.responseTypes (repeatable).
    #[arg(long)]
    response_type: Vec<String>,
    /// advancedOAuth2ClientConfig.tokenEndpointAuthMethod (live-schema validated).
    #[arg(long, value_name = "METHOD")]
    token_endpoint_auth_method: Option<String>,
    /// advancedOAuth2ClientConfig.subjectType (live-schema validated).
    #[arg(long, value_name = "TYPE")]
    subject_type: Option<String>,
    /// Set advancedOAuth2ClientConfig.isConsentImplied to true.
    #[arg(long)]
    implied_consent: bool,
    /// coreOAuth2ClientConfig.accessTokenLifetime in seconds (0 inherits).
    #[arg(long, value_name = "SECONDS")]
    access_token_lifetime: Option<u64>,
    /// coreOAuth2ClientConfig.refreshTokenLifetime in seconds (0 inherits).
    #[arg(long, value_name = "SECONDS")]
    refresh_token_lifetime: Option<u64>,
    /// coreOAuth2ClientConfig.authorizationCodeLifetime in seconds (0 inherits).
    #[arg(long, value_name = "SECONDS")]
    authorization_code_lifetime: Option<u64>,
    /// Replace an existing client with the same id.
    #[arg(long)]
    force: bool,
    #[arg(long)]
    realm: Option<String>,
    #[arg(long)]
    tenant: Option<String>,
    /// Confirm creation or replacement on a production-themed tenant.
    #[arg(long)]
    yes: bool,
}

#[derive(Args, Debug)]
pub struct GrantChangeArgs {
    /// OAuth2 client id.
    id: String,
    /// Grant types to add or remove.
    #[arg(required = true, num_args = 1..)]
    grant: Vec<String>,
    #[arg(long)]
    realm: Option<String>,
    #[arg(long)]
    tenant: Option<String>,
    /// Confirm the write on a production-themed tenant.
    #[arg(long)]
    yes: bool,
}

#[derive(Subcommand, Debug)]
pub enum GrantCommand {
    /// List the grant types enabled on an OAuth2 client.
    List {
        /// OAuth2 client id.
        id: String,
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
    },
    /// Add one or more grant types to an OAuth2 client.
    Add(GrantChangeArgs),
    /// Remove one or more grant types from an OAuth2 client.
    Remove(GrantChangeArgs),
}

#[derive(Subcommand, Debug)]
pub enum ProviderCommand {
    /// Show the realm-wide OAuth2 / OIDC provider configuration.
    Get {
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
        /// Print the unmodified provider document as JSON.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum OauthCommand {
    /// List OAuth2 clients in a realm.
    List {
        /// Only clients whose id or client name contains TEXT.
        #[arg(long, value_name = "TEXT")]
        filter: Option<String>,
        /// Hide clients whose id is a bare UUID, which is what AM mints for a
        /// dynamic registration. Tests the id, not the provenance — the count
        /// of what it hid is printed.
        #[arg(long = "no-dynamic")]
        no_dynamic: bool,
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long, help = "Print client ids as JSON")]
        json: bool,
    },
    /// Show one OAuth2 client. Reads only — writes nothing to the workspace.
    Get {
        /// OAuth2 client id.
        id: String,
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
        /// Print the unmodified client document as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Create an OAuth2 client from the tenant's live template.
    Create {
        /// OAuth2 client id.
        id: String,
        #[command(flatten)]
        options: Box<CreateArgs>,
    },
    /// List or change an OAuth2 client's grant types.
    Grant {
        #[command(subcommand)]
        command: GrantCommand,
    },
    /// Inspect the realm-wide OAuth2 / OIDC provider service.
    Provider {
        #[command(subcommand)]
        command: ProviderCommand,
    },
    /// Pull an OAuth2 client into the workspace as JSON.
    Pull {
        /// OAuth2 client id.
        id: String,
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
    },
    /// Push a workspace OAuth2 client JSON file back to AIC.
    Push {
        /// OAuth2 client id.
        id: String,
        #[arg(long)]
        force: bool,
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
    },
    /// Diff an OAuth2 client (colored, via `git diff`). Default compares the
    /// tenant against your local workspace file.
    Diff {
        /// OAuth2 client id.
        id: String,
        /// Diff your local file against the last-synced snapshot (your edits
        /// only). Makes no tenant request.
        #[arg(long, conflicts_with = "snapshot_vs_remote")]
        local_vs_snapshot: bool,
        /// Diff the last-synced snapshot against the tenant (remote drift) —
        /// the comparison `push` refuses on.
        #[arg(long)]
        snapshot_vs_remote: bool,
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
    },
    /// Delete an OAuth2 client from AIC. Requires --force.
    Delete {
        /// OAuth2 client id.
        id: String,
        #[arg(long)]
        force: bool,
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
    },
}

/// Script ids to names, for the config documents that reference scripts by
/// bare UUID.
///
/// Best-effort on purpose. Failing a read-only `get` because a *second*,
/// cosmetic request did not come back would be the wrong trade — the UUIDs
/// are still printed, and a warning says why they were not resolved.
async fn script_names(tenant: &str, realm: &str) -> spec::ScriptNames {
    // `id_to_name`, not `list`: `list` is the *sync* inventory and drops Groovy
    // and product-internal scripts, so an override legitimately pointing at
    // one would print as an unresolved UUID and read as a missing script.
    match crate::scripts::am::id_to_name(tenant, realm).await {
        Ok(entries) => spec::ScriptNames::new(entries),
        Err(error) => {
            eprintln!("warning: could not resolve script ids to names: {error}");
            spec::ScriptNames::default()
        }
    }
}

/// A field the tenant does not set, as the `-` the other tables here use.
///
/// Presentation only: `ClientRow` keeps the empty string, so `--filter -` does
/// not match every unnamed client. Some clients really have `status: null` —
/// `AicEdit` on the sandbox does — and a blank cell reads as a bug.
fn absent_cell(value: &str) -> String {
    if value.is_empty() {
        "-".to_string()
    } else {
        value.to_string()
    }
}

/// What the listing did, in one line.
///
/// A filtered list that only says how many rows it printed is the version that
/// gets misread: `--no-dynamic` on a tenant where a real client happens to
/// have a UUID id hides it, and the operator has no way to notice. Say how
/// many were hidden and by which flag.
fn list_tally(total: usize, kept: usize, filter: &Option<String>, no_dynamic: bool) -> String {
    if kept == total {
        return format!("{total} oauth clients");
    }
    let mut by = Vec::new();
    if let Some(needle) = filter {
        by.push(format!("--filter {needle:?}"));
    }
    if no_dynamic {
        by.push("--no-dynamic".to_string());
    }
    format!(
        "{kept} of {total} oauth clients ({} hidden by {})",
        total - kept,
        by.join(" and ")
    )
}

fn validate_client_id(id: &str) -> Result<()> {
    if id.chars().any(is_separator) {
        return Err(Error::Config(format!(
            "oauth client id {id:?} contains a path separator"
        )));
    }
    Ok(())
}

fn export_path(tenant: &str, realm: &str, id: &str) -> Result<PathBuf> {
    validate_client_id(id)?;
    Ok(ProjectConfig::workspace_tree(tenant)
        .join("oauth")
        .join(realm)
        .join(format!("{id}.json")))
}

fn snapshot_path(tenant: &str, realm: &str, id: &str) -> Result<PathBuf> {
    validate_client_id(id)?;
    Ok(ProjectConfig::workspace_tree(tenant)
        .join("oauth")
        .join(realm)
        .join(".snapshots")
        .join(format!("{id}.json")))
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PushBlockReason {
    MissingSnapshot,
    RemoteDrift,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PushDecision {
    Push,
    Blocked(PushBlockReason),
    NothingToDo,
}

fn push_decision(
    local: &Value,
    remote: &Value,
    snapshot: Option<&Value>,
    force: bool,
) -> PushDecision {
    if api::content_equal(local, remote) {
        return PushDecision::NothingToDo;
    }

    let Some(snapshot) = snapshot else {
        return if force {
            PushDecision::Push
        } else {
            PushDecision::Blocked(PushBlockReason::MissingSnapshot)
        };
    };

    if api::content_equal(remote, snapshot) || force {
        PushDecision::Push
    } else {
        PushDecision::Blocked(PushBlockReason::RemoteDrift)
    }
}

/// Which two versions `aic oauth diff` compares. Same three modes and the same
/// flag names as `aic script diff`, because the question is the same one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiffMode {
    /// Your edits only. Makes no tenant request.
    LocalVsSnapshot,
    /// Tenant drift only — the comparison `push` refuses on.
    SnapshotVsRemote,
    /// Current tenant content against your local file.
    RemoteVsLocal,
}

/// One of the three places a client document lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Local,
    Snapshot,
    Remote,
}

impl Side {
    fn label(self) -> &'static str {
        match self {
            Side::Local => "local",
            Side::Snapshot => "snapshot",
            Side::Remote => "tenant",
        }
    }
}

impl DiffMode {
    fn from_flags(local_vs_snapshot: bool, snapshot_vs_remote: bool) -> Self {
        if local_vs_snapshot {
            DiffMode::LocalVsSnapshot
        } else if snapshot_vs_remote {
            DiffMode::SnapshotVsRemote
        } else {
            DiffMode::RemoteVsLocal
        }
    }

    /// `-` is the left (older) side, `+` is the right (newer) side.
    fn sides(self) -> (Side, Side) {
        match self {
            DiffMode::LocalVsSnapshot => (Side::Snapshot, Side::Local),
            DiffMode::SnapshotVsRemote => (Side::Snapshot, Side::Remote),
            DiffMode::RemoteVsLocal => (Side::Remote, Side::Local),
        }
    }

    /// Does this comparison read that side at all?
    ///
    /// Worth asking rather than loading all three: it keeps
    /// `--local-vs-snapshot` offline, and it stops an unparseable file the
    /// comparison never looks at from failing it. A corrupt local export used
    /// to be able to break `--snapshot-vs-remote`, which is the one mode you
    /// reach for when the local copy is the thing you distrust.
    fn uses(self, side: Side) -> bool {
        let (left, right) = self.sides();
        left == side || right == side
    }
}

/// Two absent sides are equal, and reporting them as "identical" is a false
/// reassurance — it is the answer a mistyped client id gets, at exit 0. Refuse
/// instead, and say that nothing was compared.
fn nothing_to_compare(
    id: &str,
    sides: &DiffSides,
    pull: Option<&str>,
    artefacts: LocalArtefacts,
) -> Option<String> {
    (sides.left_text.is_none() && sides.right_text.is_none()).then(|| {
        let remedy = match pull {
            Some(pull) => format!("; run `{pull}` if the client exists on the tenant"),
            // The two compared sides are both gone, but a *third* file can
            // still exist — `--snapshot-vs-remote` never looks at the local
            // export, and the default mode never looks at the snapshot. Saying
            // "neither side has it" and stopping there reads as nothing left
            // at all, right before someone reaches for `pull`.
            None => format!(
                "; the {} is still there, and `aic oauth pull` would overwrite it",
                pull_cost(artefacts)
            ),
        };
        format!(
            "nothing to compare for oauth client {id}: neither {} nor {} has it{remedy}",
            sides.left.label(),
            sides.right.label()
        )
    })
}

/// Quote a value for a command line we are telling someone to run.
///
/// Tenant names are not restricted to a safe character set — `tenant_file_name`
/// maps the unsafe ones for paths rather than rejecting them — so a name with a
/// space produces a suggestion that runs as two arguments when pasted.
fn shell_quote(value: &str) -> String {
    if !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/'))
    {
        return value.to_string();
    }
    format!("'{}'", value.replace('\'', r"'\''"))
}

/// The files a `pull` would destroy, named. The caller only asks in the branch
/// where at least one exists.
fn pull_cost(artefacts: LocalArtefacts) -> String {
    match (artefacts.local, artefacts.snapshot) {
        (true, true) => "local file and snapshot".to_string(),
        (true, false) => "local file".to_string(),
        (false, true) => "snapshot".to_string(),
        (false, false) => "nothing".to_string(),
    }
}

fn pull_command(id: &str, tenant: &str, realm: &str) -> String {
    format!(
        "aic oauth pull {} --tenant {} --realm {}",
        shell_quote(id),
        shell_quote(tenant),
        shell_quote(realm)
    )
}

/// What `pull` would overwrite: the two files it writes, whether or not this
/// comparison reads them.
///
/// Presence, not content — an existence check, no parse and no network — so
/// asking about a side the mode deliberately did not load stays cheap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LocalArtefacts {
    local: bool,
    snapshot: bool,
}

impl LocalArtefacts {
    fn on_disk(tenant: &str, realm: &str, id: &str) -> Result<Self> {
        // `try_exists`, not `exists`: the latter answers `false` for a path it
        // could not stat at all, so an unreadable directory would read as
        // "nothing here to lose" and the note would offer a `pull` over it.
        let present = |path: &Path| -> Result<bool> {
            path.try_exists()
                .map_err(|error| Error::Config(format!("check {}: {error}", path.display())))
        };
        Ok(Self {
            local: present(&export_path(tenant, realm, id)?)?,
            snapshot: present(&snapshot_path(tenant, realm, id)?)?,
        })
    }

    #[cfg(test)]
    fn none() -> Self {
        Self {
            local: false,
            snapshot: false,
        }
    }

    fn any(self) -> bool {
        self.local || self.snapshot
    }
}

/// The `pull` that would restore a missing side — or nothing, when `pull` is
/// the wrong advice.
///
/// `pull` writes **both** the local export and the snapshot from the tenant.
/// So it is safe advice exactly when neither of those files exists — the
/// tenant does not count, because `pull` never writes to the tenant. With a
/// snapshot present and the local file gone it does not restore the local file
/// from the snapshot, it replaces the snapshot too and throws away the drift
/// signal; with local edits present and the snapshot gone it overwrites the
/// edits.
///
/// Two earlier versions of this rule were wrong, both by looking at the wrong
/// thing. The first asked whether *either compared side* survived, which
/// suppressed the suggestion in the common tenant-exists-locally-missing case
/// and only ever fired for the both-absent case the renderer refuses first —
/// unreachable. The second still read the compared sides, so it could not see
/// a file the mode had not loaded: default `remote-vs-local` never looks at
/// the snapshot, and `--snapshot-vs-remote` never looks at the local file, so
/// each would happily suggest a `pull` that overwrote the other.
fn safe_pull_suggestion(
    artefacts: LocalArtefacts,
    id: &str,
    tenant: &str,
    realm: &str,
) -> Option<String> {
    (!artefacts.any()).then(|| pull_command(id, tenant, realm))
}

/// A side that does not exist renders as empty, which in a diff is
/// indistinguishable from a side that exists and is empty. Say which it was.
///
/// Any command suggested carries the coordinates the diff itself used: both
/// flags default, so a bare `aic oauth pull <id>` is only right when you are
/// on the default tenant and realm — advising it after
/// `--tenant staging --realm bravo` sends you to pull a different client.
fn absent_side_note(
    id: &str,
    side: Side,
    tenant: &str,
    realm: &str,
    pull: Option<&str>,
    cost: &str,
) -> String {
    let remedy = match pull {
        Some(pull) => format!("run `{pull}`"),
        // Naming what `pull` would cost is more use than naming `pull` — and
        // naming *which* file, because the one it would destroy is often not
        // the side this comparison is missing.
        None => format!("`aic oauth pull` would overwrite the {cost} you still have"),
    };
    match side {
        Side::Local => {
            format!("note: no local export for oauth client {id} — {remedy}; shown as empty")
        }
        Side::Snapshot => {
            format!("note: no snapshot for oauth client {id} — {remedy}; shown as empty")
        }
        Side::Remote => {
            format!("note: oauth client {id} does not exist on {tenant}/{realm}; shown as empty")
        }
    }
}

/// Which comparison explains a refusal.
///
/// Drift is a change on the tenant, so the useful view is what the tenant did
/// since you pulled — not your own edits, which you already know about. With
/// no snapshot there is nothing to have drifted from, so the useful view is
/// what `--force` would overwrite.
fn blocked_push_diff_mode(reason: &PushBlockReason) -> DiffMode {
    match reason {
        PushBlockReason::RemoteDrift => DiffMode::SnapshotVsRemote,
        PushBlockReason::MissingSnapshot => DiffMode::RemoteVsLocal,
    }
}

fn push_block_message(id: &str, reason: &PushBlockReason, tenant: &str, realm: &str) -> String {
    // Coordinates included for the same reason the diff notes carry them: both
    // flags default, so a bare `pull` after `--realm bravo` re-pulls a
    // different client and reports success.
    let pull = format!(
        "aic oauth pull {} --tenant {} --realm {}",
        shell_quote(id),
        shell_quote(tenant),
        shell_quote(realm)
    );
    match reason {
        PushBlockReason::MissingSnapshot => format!(
            "no snapshot for oauth client {id:?}; run `{pull}` first (it overwrites your local file) or pass --force"
        ),
        PushBlockReason::RemoteDrift => format!(
            "remote oauth client {id} changed since you last pulled; re-pull with `{pull}` (it overwrites your local file) or pass --force"
        ),
    }
}

/// The two sides of a rendered comparison. `None` text means the side does not
/// exist at all — no local export, no snapshot, or no such client on the
/// tenant — which the renderer reports rather than passing off as empty.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DiffSides {
    left: Side,
    left_text: Option<String>,
    right: Side,
    right_text: Option<String>,
}

/// Pick the two sides `mode` names out of the three documents.
///
/// Pure, and takes all three, so `push` can render its refusal from the values
/// it already fetched rather than re-reading the tenant — a second read could
/// disagree with the one the refusal was decided on.
fn diff_sides_from(
    mode: DiffMode,
    local: Option<&Value>,
    snapshot: Option<&Value>,
    remote: Option<&Value>,
) -> DiffSides {
    let text = |side: Side| -> Option<String> {
        match side {
            Side::Local => local,
            Side::Snapshot => snapshot,
            Side::Remote => remote,
        }
        .map(api::content_text)
    };
    let (left, right) = mode.sides();
    DiffSides {
        left,
        left_text: text(left),
        right,
        right_text: text(right),
    }
}

fn render_diff_sides(
    id: &str,
    tenant: &str,
    realm: &str,
    sides: &DiffSides,
    artefacts: LocalArtefacts,
) -> Result<()> {
    // Ahead of the per-side notes: when neither side exists they would say
    // twice, at length, what the refusal says once.
    let pull = safe_pull_suggestion(artefacts, id, tenant, realm);
    let cost = pull_cost(artefacts);
    if let Some(message) = nothing_to_compare(id, sides, pull.as_deref(), artefacts) {
        return Err(Error::Config(message));
    }
    for (side, text) in [
        (sides.left, &sides.left_text),
        (sides.right, &sides.right_text),
    ] {
        if text.is_none() {
            eprintln!(
                "{}",
                absent_side_note(id, side, tenant, realm, pull.as_deref(), &cost)
            );
        }
    }
    show_diff(
        id,
        sides.left.label(),
        sides.left_text.as_deref().unwrap_or_default(),
        sides.right.label(),
        sides.right_text.as_deref().unwrap_or_default(),
    )
}

/// `read_export`, but a missing file is a side that does not exist rather than
/// an error — a diff against nothing is the honest answer, and it is what shows
/// you the file you deleted.
fn read_export_opt(path: &Path, id: &str, tenant: &str, realm: &str) -> Result<Option<Value>> {
    match std::fs::metadata(path) {
        Ok(_) => read_export(path, id, tenant, realm).map(Some),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(Error::Config(format!(
            "read oauth client export {}: {error}",
            path.display()
        ))),
    }
}

fn write_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, bytes)?;
    Ok(())
}

fn write_snapshot(tenant: &str, realm: &str, id: &str, client: &Value) -> Result<()> {
    let path = snapshot_path(tenant, realm, id)?;
    write_bytes(&path, &serde_json::to_vec_pretty(client)?)
}

fn parse_client_value(value: Value, label: &str) -> Result<Value> {
    if value.is_object() {
        Ok(value)
    } else {
        Err(Error::Config(format!(
            "oauth client JSON {label} is not an object"
        )))
    }
}

fn read_seed(path: &Path) -> Result<Value> {
    let bytes = std::fs::read(path).map_err(|error| {
        Error::Config(format!(
            "read oauth client seed {}: {error}",
            path.display()
        ))
    })?;
    let value = serde_json::from_slice(&bytes).map_err(|error| {
        Error::Config(format!(
            "parse oauth client seed {}: {error}",
            path.display()
        ))
    })?;
    parse_client_value(value, &path.display().to_string())
}

fn read_export(path: &Path, id: &str, tenant: &str, realm: &str) -> Result<Value> {
    let bytes = std::fs::read(path).map_err(|error| {
        if error.kind() == ErrorKind::NotFound {
            Error::Config(format!(
                "local oauth client export missing: {}; run `aic oauth pull {} --tenant {} --realm {}` first",
                path.display(),
                shell_quote(id),
                shell_quote(tenant),
                shell_quote(realm)
            ))
        } else {
            Error::Config(format!(
                "read oauth client export {}: {error}",
                path.display()
            ))
        }
    })?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|error| {
        Error::Config(format!(
            "parse oauth client export {}: {error}",
            path.display()
        ))
    })?;
    parse_client_value(value, &path.display().to_string())
}

fn read_snapshot(path: &Path) -> Result<Option<Value>> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(Error::Config(format!(
                "read oauth client snapshot {}: {error}",
                path.display()
            )));
        }
    };
    let value: Value = serde_json::from_slice(&bytes).map_err(|error| {
        Error::Config(format!(
            "parse oauth client snapshot {}: {error}",
            path.display()
        ))
    })?;
    Ok(Some(parse_client_value(
        value,
        &path.display().to_string(),
    )?))
}

fn remove_snapshot_if_present(tenant: &str, realm: &str, id: &str) -> Result<()> {
    let path = snapshot_path(tenant, realm, id)?;
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(Error::Config(format!(
            "remove oauth client snapshot {}: {error}",
            path.display()
        ))),
    }
}

fn api_not_found(error: &Error) -> bool {
    matches!(error, Error::Api { status: 404, .. })
}

fn ensure_create_allowed(exists: bool, force: bool, id: &str) -> Result<()> {
    if exists && !force {
        return Err(Error::Config(format!(
            "oauth client {id:?} already exists; pass --force to replace it"
        )));
    }
    Ok(())
}

fn generate_client_secret() -> String {
    let mut bytes = [0_u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn schema_for_validation(result: Result<Value>) -> Option<Value> {
    match result {
        Ok(schema) => Some(schema),
        Err(error) => {
            eprintln!(
                "warning: could not fetch oauth client schema; deferring enum validation to AIC: {error}"
            );
            None
        }
    }
}

const JWT_BEARER_GRANT: &str = "urn:ietf:params:oauth:grant-type:jwt-bearer";

async fn run_grant_change(args: GrantChangeArgs, operation: spec::GrantOperation) -> Result<()> {
    let tenant = tenant_for(args.tenant)?;
    let realm = realm_arg("oauth", args.realm)?;
    validate_client_id(&args.id)?;

    let (client, schema) = tokio::join!(
        api::read_client(&tenant, &realm, &args.id),
        api::client_schema(&tenant, &realm, args.yes),
    );
    let client = client?;
    let schema = schema_for_validation(schema);
    let adding_jwt_bearer = operation == spec::GrantOperation::Add
        && args.grant.iter().any(|grant| grant == JWT_BEARER_GRANT)
        && !spec::grant_types(&client)
            .map_err(Error::Config)?
            .iter()
            .any(|grant| grant == JWT_BEARER_GRANT);
    let update = spec::update_grants(&client, &args.grant, operation).map_err(Error::Config)?;
    spec::validate_grant_types(&update.body, schema.as_ref()).map_err(Error::Config)?;

    if !update.changed {
        println!("oauth client {} grants already match; no change", args.id);
        return Ok(());
    }

    if adding_jwt_bearer {
        eprintln!(
            "warning: jwt-bearer lets a Trusted JWT Issuer with empty allowedSubjects mint tokens as any user in realm {realm}; AIC has no per-client issuer restriction"
        );
    }

    prod_hint(api::upsert_client(&tenant, &realm, &args.id, update.body, args.yes).await)?;
    let verb = match operation {
        spec::GrantOperation::Add => "added to",
        spec::GrantOperation::Remove => "removed from",
    };
    println!("grant types {verb} oauth client {}", args.id);
    Ok(())
}

pub async fn run(cmd: OauthCommand) -> Result<()> {
    match cmd {
        OauthCommand::List {
            filter,
            no_dynamic,
            realm,
            tenant,
            json,
        } => {
            let tenant = tenant_for(tenant)?;
            let realm = realm_arg("oauth", realm)?;
            let all = api::list_client_rows(&tenant, &realm)
                .await?
                .iter()
                .map(spec::client_row)
                .collect::<Vec<_>>();
            let kept = all
                .iter()
                .filter(|row| filter.as_deref().is_none_or(|n| spec::row_matches(row, n)))
                .filter(|row| !(no_dynamic && row.uuid_id))
                .collect::<Vec<_>>();

            if json {
                // Still ids, as before: `--json` on a list is what a script
                // pipes into the next command, and widening it would break
                // every one of those.
                let ids = kept.iter().map(|row| row.id.clone()).collect::<Vec<_>>();
                print_json(&ids)?;
            } else {
                let rows = kept
                    .iter()
                    .map(|row| {
                        vec![
                            row.id.clone(),
                            absent_cell(&row.name),
                            absent_cell(&row.client_type),
                            absent_cell(&row.status),
                            absent_cell(&row.grants),
                        ]
                    })
                    .collect::<Vec<_>>();
                print_table(&["CLIENT_ID", "NAME", "TYPE", "STATUS", "GRANTS"], &rows);
            }
            eprintln!("{}", list_tally(all.len(), kept.len(), &filter, no_dynamic));
            Ok(())
        }
        OauthCommand::Get {
            id,
            realm,
            tenant,
            json,
        } => {
            let tenant = tenant_for(tenant)?;
            let realm = realm_arg("oauth", realm)?;
            validate_client_id(&id)?;
            let client = api::read_client(&tenant, &realm, &id).await?;
            if json {
                print_json(&client)?;
                return Ok(());
            }
            let mut summary = spec::client_summary(&client);
            spec::resolve_script_ids(&mut summary, &script_names(&tenant, &realm).await);
            let rows = summary
                .into_iter()
                .map(|(field, value)| vec![field, value])
                .collect::<Vec<_>>();
            print_table(&["FIELD", "VALUE"], &rows);
            for fault in spec::override_faults(&client) {
                eprintln!("warning: {fault}");
            }
            Ok(())
        }
        OauthCommand::Create { id, options } => {
            let tenant = tenant_for(options.tenant.clone())?;
            let realm = realm_arg("oauth", options.realm.clone())?;
            validate_client_id(&id)?;

            let exists = match api::read_client(&tenant, &realm, &id).await {
                Ok(_) => true,
                Err(error) if api_not_found(&error) => false,
                Err(error) => return Err(error),
            };
            ensure_create_allowed(exists, options.force, &id)?;

            let seed = options.from.as_deref().map(read_seed).transpose()?;
            let (template, schema) = tokio::join!(
                api::client_template(&tenant, &realm, options.yes),
                api::client_schema(&tenant, &realm, options.yes),
            );
            let template = prod_hint(template)?;
            let schema = schema_for_validation(schema);

            let generated_secret = options.generate_secret.then(generate_client_secret);
            let secret = if options.secret_stdin {
                Some(read_password_line(std::io::stdin().lock())?)
            } else {
                generated_secret.clone()
            };
            let create_spec = spec::CreateClientSpec {
                name: options.name,
                description: options.description,
                client_type: options.client_type,
                secret,
                scopes: options.scope,
                default_scopes: options.default_scope,
                redirect_uris: options.redirect_uri,
                grants: options.grant,
                response_types: options.response_type,
                token_endpoint_auth_method: options.token_endpoint_auth_method,
                subject_type: options.subject_type,
                implied_consent: options.implied_consent.then_some(true),
                access_token_lifetime: options.access_token_lifetime,
                refresh_token_lifetime: options.refresh_token_lifetime,
                authorization_code_lifetime: options.authorization_code_lifetime,
            };
            let body =
                spec::build_create_body(template, seed, &create_spec).map_err(Error::Config)?;
            spec::validate_enumerated_fields(&body, schema.as_ref()).map_err(Error::Config)?;

            prod_hint(api::create_client(&tenant, &realm, &id, body, options.yes).await)?;
            if let Some(secret) = generated_secret {
                println!("client secret: {secret}");
            }
            let verb = if exists { "replaced" } else { "created" };
            println!("{verb} oauth client {id}");
            Ok(())
        }
        OauthCommand::Grant { command } => match command {
            GrantCommand::List { id, realm, tenant } => {
                let tenant = tenant_for(tenant)?;
                let realm = realm_arg("oauth", realm)?;
                validate_client_id(&id)?;
                let client = api::read_client(&tenant, &realm, &id).await?;
                let grants = spec::grant_types(&client).map_err(Error::Config)?;
                let rows = grants
                    .iter()
                    .map(|grant| vec![grant.clone()])
                    .collect::<Vec<_>>();
                print_table(&["GRANT_TYPE"], &rows);
                eprintln!("{} grant types", grants.len());
                Ok(())
            }
            GrantCommand::Add(args) => run_grant_change(args, spec::GrantOperation::Add).await,
            GrantCommand::Remove(args) => {
                run_grant_change(args, spec::GrantOperation::Remove).await
            }
        },
        OauthCommand::Provider { command } => match command {
            ProviderCommand::Get {
                realm,
                tenant,
                json,
            } => {
                let tenant = tenant_for(tenant)?;
                let realm = realm_arg("oauth", realm)?;
                let provider = api::read_provider(&tenant, &realm).await?;
                if json {
                    print_json(&provider)
                } else {
                    let mut summary = spec::provider_summary(&provider);
                    spec::resolve_script_ids(&mut summary, &script_names(&tenant, &realm).await);
                    for (field, value) in summary {
                        println!("{field}: {value}");
                    }
                    Ok(())
                }
            }
        },
        OauthCommand::Pull { id, realm, tenant } => {
            let tenant = tenant_for(tenant)?;
            let realm = realm_arg("oauth", realm)?;
            let path = export_path(&tenant, &realm, &id)?;
            let snapshot = snapshot_path(&tenant, &realm, &id)?;
            let client = api::read_client(&tenant, &realm, &id).await?;
            let bytes = serde_json::to_vec_pretty(&client)?;
            write_bytes(&path, &bytes)?;
            write_bytes(&snapshot, &bytes)?;
            println!("pulled oauth client {id} -> {}", path.display());
            Ok(())
        }
        OauthCommand::Push {
            id,
            force,
            realm,
            tenant,
        } => {
            let tenant = tenant_for(tenant)?;
            let realm = realm_arg("oauth", realm)?;
            let path = export_path(&tenant, &realm, &id)?;
            let snapshot = snapshot_path(&tenant, &realm, &id)?;
            let local = read_export(&path, &id, &tenant, &realm)?;
            let remote = match api::read_client(&tenant, &realm, &id).await {
                Ok(client) => Some(client),
                Err(error) if api_not_found(&error) => None,
                Err(error) => return Err(error),
            };

            let Some(remote) = remote else {
                api::upsert_client(&tenant, &realm, &id, local, false).await?;
                let refreshed = api::read_client(&tenant, &realm, &id).await?;
                write_snapshot(&tenant, &realm, &id, &refreshed)?;
                println!("created oauth client {id}");
                return Ok(());
            };

            let snapshot_value = read_snapshot(&snapshot)?;
            match push_decision(&local, &remote, snapshot_value.as_ref(), force) {
                PushDecision::NothingToDo => {
                    write_snapshot(&tenant, &realm, &id, &remote)?;
                    println!("oauth client {id} already matches remote -> {tenant}/{realm}");
                    Ok(())
                }
                PushDecision::Blocked(reason) => {
                    // Show what the refusal is about before refusing. The
                    // report this closes describes reaching for `--force`
                    // blind and nearly pushing the wrong grant types; the
                    // whole point is that the drift is knowable without it.
                    // Rendered from the values the decision was made on, so
                    // the diff cannot describe a different tenant state than
                    // the refusal does.
                    let sides = diff_sides_from(
                        blocked_push_diff_mode(&reason),
                        Some(&local),
                        snapshot_value.as_ref(),
                        Some(&remote),
                    );
                    // A renderer failure (no `git` on PATH, say) must not
                    // replace the refusal with a story about temp files.
                    let artefacts = LocalArtefacts {
                        // `push` read the local export to get here, and the
                        // snapshot presence is the branch it is standing in.
                        local: true,
                        snapshot: snapshot_value.is_some(),
                    };
                    if let Err(error) = render_diff_sides(&id, &tenant, &realm, &sides, artefacts) {
                        eprintln!("warning: could not render the diff: {error}");
                    }
                    Err(Error::Config(push_block_message(
                        &id, &reason, &tenant, &realm,
                    )))
                }
                PushDecision::Push => {
                    api::upsert_client(&tenant, &realm, &id, local, false).await?;
                    let refreshed = api::read_client(&tenant, &realm, &id).await?;
                    write_snapshot(&tenant, &realm, &id, &refreshed)?;
                    println!("pushed oauth client {id}");
                    Ok(())
                }
            }
        }
        OauthCommand::Diff {
            id,
            local_vs_snapshot,
            snapshot_vs_remote,
            realm,
            tenant,
        } => {
            let tenant = tenant_for(tenant)?;
            let realm = realm_arg("oauth", realm)?;
            let mode = DiffMode::from_flags(local_vs_snapshot, snapshot_vs_remote);
            let local = if mode.uses(Side::Local) {
                read_export_opt(&export_path(&tenant, &realm, &id)?, &id, &tenant, &realm)?
            } else {
                None
            };
            let snapshot = if mode.uses(Side::Snapshot) {
                read_snapshot(&snapshot_path(&tenant, &realm, &id)?)?
            } else {
                None
            };
            let remote = if mode.uses(Side::Remote) {
                match api::read_client(&tenant, &realm, &id).await {
                    Ok(client) => Some(client),
                    Err(error) if api_not_found(&error) => None,
                    Err(error) => return Err(error),
                }
            } else {
                None
            };
            let sides = diff_sides_from(mode, local.as_ref(), snapshot.as_ref(), remote.as_ref());
            // Both files, not just the two this mode compared: `pull` writes
            // both, so a suggestion built from the compared sides alone would
            // offer to overwrite the one the mode never looked at.
            let artefacts = LocalArtefacts::on_disk(&tenant, &realm, &id)?;
            render_diff_sides(&id, &tenant, &realm, &sides, artefacts)
        }
        OauthCommand::Delete {
            id,
            force,
            realm,
            tenant,
        } => {
            let tenant = tenant_for(tenant)?;
            let realm = realm_arg("oauth", realm)?;
            if !force {
                eprintln!(
                    "would delete oauth client {id} from {tenant}/{realm}; pass --force to delete it"
                );
                return Err(Error::Config("oauth client delete requires --force".into()));
            }
            api::delete_client(&tenant, &realm, &id).await?;
            remove_snapshot_if_present(&tenant, &realm, &id)?;
            println!("deleted oauth client {id}");
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use serde_json::json;

    #[test]
    fn oauth_realm_defaults_to_alpha_and_accepts_bravo() {
        assert_eq!(realm_arg("oauth", None).unwrap(), "alpha");
        assert_eq!(realm_arg("oauth", Some("bravo".into())).unwrap(), "bravo");
    }

    #[test]
    fn oauth_realm_rejects_other_realms() {
        let error = realm_arg("oauth", Some("root".into())).unwrap_err();
        assert!(error.to_string().contains("alpha or bravo"));
    }

    #[test]
    fn export_path_rejects_ids_with_path_separators() {
        let error = export_path("sandbox", "alpha", "folder/client").unwrap_err();
        assert!(error.to_string().contains("path separator"));
    }

    #[test]
    fn export_path_uses_the_oauth_workspace_tree() {
        assert_eq!(
            export_path("sandbox", "bravo", "service_C1").unwrap(),
            PathBuf::from("workspace/sandbox/oauth/bravo/service_C1.json")
        );
    }

    #[test]
    fn snapshot_path_rejects_ids_with_path_separators() {
        let error = snapshot_path("sandbox", "alpha", "folder/client").unwrap_err();
        assert!(error.to_string().contains("path separator"));
    }

    #[test]
    fn snapshot_path_uses_snapshots_sibling_directory() {
        assert_eq!(
            snapshot_path("sandbox", "bravo", "service_C1").unwrap(),
            PathBuf::from("workspace/sandbox/oauth/bravo/.snapshots/service_C1.json")
        );
    }

    #[test]
    fn create_refuses_an_existing_client_without_force() {
        let error = ensure_create_allowed(true, false, "existing-client").unwrap_err();
        assert!(error.to_string().contains("already exists"));
        assert!(error.to_string().contains("--force"));
        assert!(ensure_create_allowed(true, true, "existing-client").is_ok());
        assert!(ensure_create_allowed(false, false, "new-client").is_ok());
    }

    #[test]
    fn create_flags_parse_without_an_argv_secret_value() {
        let cli = crate::cli::Cli::try_parse_from([
            "aic",
            "oauth",
            "create",
            "test-client",
            "--from",
            "source.json",
            "--name",
            "Test client",
            "--scope",
            "openid",
            "--scope",
            "profile",
            "--secret-stdin",
            "--token-endpoint-auth-method",
            "client_secret_post",
            "--access-token-lifetime",
            "0",
            "--force",
            "--yes",
        ])
        .unwrap();

        let Some(crate::cli::Command::Oauth {
            command: OauthCommand::Create { id, options },
        }) = cli.command
        else {
            panic!("expected oauth create");
        };
        assert_eq!(id, "test-client");
        assert_eq!(options.from, Some(PathBuf::from("source.json")));
        assert_eq!(options.name.as_deref(), Some("Test client"));
        assert_eq!(options.scope, ["openid", "profile"]);
        assert!(options.secret_stdin);
        assert_eq!(
            options.token_endpoint_auth_method.as_deref(),
            Some("client_secret_post")
        );
        assert_eq!(options.access_token_lifetime, Some(0));
        assert!(options.force);
        assert!(options.yes);

        assert!(
            crate::cli::Cli::try_parse_from([
                "aic",
                "oauth",
                "create",
                "test-client",
                "--secret",
                "visible-in-argv"
            ])
            .is_err()
        );
        assert!(
            crate::cli::Cli::try_parse_from([
                "aic",
                "oauth",
                "create",
                "test-client",
                "--secret-stdin",
                "--generate-secret"
            ])
            .is_err()
        );
    }

    #[test]
    fn grant_commands_parse_repeatable_grants_and_production_confirmation() {
        let cli = crate::cli::Cli::try_parse_from([
            "aic",
            "oauth",
            "grant",
            "add",
            "existing-client",
            "client_credentials",
            "urn:ietf:params:oauth:grant-type:jwt-bearer",
            "--realm",
            "bravo",
            "--tenant",
            "sandbox",
            "--yes",
        ])
        .unwrap();

        let Some(crate::cli::Command::Oauth {
            command:
                OauthCommand::Grant {
                    command: GrantCommand::Add(args),
                },
        }) = cli.command
        else {
            panic!("expected oauth grant add");
        };
        assert_eq!(args.id, "existing-client");
        assert_eq!(
            args.grant,
            [
                "client_credentials",
                "urn:ietf:params:oauth:grant-type:jwt-bearer"
            ]
        );
        assert_eq!(args.realm.as_deref(), Some("bravo"));
        assert_eq!(args.tenant.as_deref(), Some("sandbox"));
        assert!(args.yes);

        assert!(
            crate::cli::Cli::try_parse_from(["aic", "oauth", "grant", "remove", "existing-client"])
                .is_err()
        );
    }

    #[test]
    fn provider_get_parses_json_and_realm_selection() {
        let cli = crate::cli::Cli::try_parse_from([
            "aic", "oauth", "provider", "get", "--realm", "bravo", "--tenant", "sandbox", "--json",
        ])
        .unwrap();

        let Some(crate::cli::Command::Oauth {
            command:
                OauthCommand::Provider {
                    command:
                        ProviderCommand::Get {
                            realm,
                            tenant,
                            json,
                        },
                },
        }) = cli.command
        else {
            panic!("expected oauth provider get");
        };
        assert_eq!(realm.as_deref(), Some("bravo"));
        assert_eq!(tenant.as_deref(), Some("sandbox"));
        assert!(json);
    }

    #[test]
    fn generated_secret_is_256_bits_of_url_safe_random_data() {
        let secret = generate_client_secret();
        let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(secret)
            .unwrap();
        assert_eq!(decoded.len(), 32);
    }

    #[test]
    fn failed_schema_fetch_becomes_validation_fallback() {
        let schema = schema_for_validation(Err(Error::Config("schema unavailable".into())));
        let body = json!({
            "advancedOAuth2ClientConfig": {
                "grantTypes": ["tenant-future-grant"]
            }
        });

        assert!(schema.is_none());
        assert!(spec::validate_enumerated_fields(&body, schema.as_ref()).is_ok());
    }

    fn client_with(name: &str, secret: &str, rev: &str) -> Value {
        json!({
            "_id": "client-a",
            "_rev": rev,
            "coreOAuth2ClientConfig": {
                "clientName": {"inherited": false, "value": [name]},
                "userpassword": secret
            }
        })
    }

    #[test]
    fn push_decision_returns_nothing_to_do_when_local_matches_remote() {
        let local = client_with("same", "secret", "local-rev");
        let remote = client_with("same", "secret", "remote-rev");

        assert_eq!(
            push_decision(&local, &remote, None, false),
            PushDecision::NothingToDo
        );
    }

    #[test]
    fn push_decision_pushes_when_remote_matches_snapshot_and_local_differs() {
        let snapshot = client_with("old", "secret", "snapshot-rev");
        let remote = client_with("old", "secret", "remote-rev");
        let local = client_with("new", "secret", "local-rev");

        assert_eq!(
            push_decision(&local, &remote, Some(&snapshot), false),
            PushDecision::Push
        );
    }

    #[test]
    fn push_decision_blocks_when_remote_drifted_without_force() {
        let snapshot = client_with("old", "secret", "snapshot-rev");
        let remote = client_with("remote-change", "secret", "remote-rev");
        let local = client_with("local-change", "secret", "local-rev");

        assert_eq!(
            push_decision(&local, &remote, Some(&snapshot), false),
            PushDecision::Blocked(PushBlockReason::RemoteDrift)
        );
    }

    #[test]
    fn push_decision_allows_remote_drift_with_force() {
        let snapshot = client_with("old", "secret", "snapshot-rev");
        let remote = client_with("remote-change", "secret", "remote-rev");
        let local = client_with("local-change", "secret", "local-rev");

        assert_eq!(
            push_decision(&local, &remote, Some(&snapshot), true),
            PushDecision::Push
        );
    }

    #[test]
    fn push_decision_blocks_missing_snapshot_without_force() {
        let remote = client_with("old", "secret", "remote-rev");
        let local = client_with("new", "secret", "local-rev");

        assert_eq!(
            push_decision(&local, &remote, None, false),
            PushDecision::Blocked(PushBlockReason::MissingSnapshot)
        );
    }

    #[test]
    fn push_decision_allows_missing_snapshot_with_force() {
        let remote = client_with("old", "secret", "remote-rev");
        let local = client_with("new", "secret", "local-rev");

        assert_eq!(
            push_decision(&local, &remote, None, true),
            PushDecision::Push
        );
    }

    /// `-` is the older side and `+` the newer one in all three modes, so a
    /// diff always reads "this is what changed to get here". Getting a pair
    /// backwards renders a correct diff describing the change in reverse,
    /// which no compiler and no green suite would catch.
    #[test]
    fn every_mode_puts_the_older_side_on_the_left() {
        assert_eq!(
            DiffMode::LocalVsSnapshot.sides(),
            (Side::Snapshot, Side::Local)
        );
        assert_eq!(
            DiffMode::SnapshotVsRemote.sides(),
            (Side::Snapshot, Side::Remote)
        );
        assert_eq!(DiffMode::RemoteVsLocal.sides(), (Side::Remote, Side::Local));
    }

    /// The default must be the mode that needs no flags to answer the everyday
    /// question, and each flag must select its own mode.
    #[test]
    fn diff_flags_select_the_mode_they_name() {
        assert_eq!(
            DiffMode::from_flags(false, false),
            DiffMode::RemoteVsLocal,
            "default"
        );
        assert_eq!(DiffMode::from_flags(true, false), DiffMode::LocalVsSnapshot);
        assert_eq!(
            DiffMode::from_flags(false, true),
            DiffMode::SnapshotVsRemote
        );
    }

    /// The discriminating case is `--local-vs-snapshot`: it must not read the
    /// tenant, or the one mode advertised as offline silently is not.
    #[test]
    fn only_the_modes_naming_the_tenant_read_it() {
        assert!(!DiffMode::LocalVsSnapshot.uses(Side::Remote));
        assert!(DiffMode::SnapshotVsRemote.uses(Side::Remote));
        assert!(DiffMode::RemoteVsLocal.uses(Side::Remote));

        assert!(!DiffMode::SnapshotVsRemote.uses(Side::Local));
        assert!(!DiffMode::RemoteVsLocal.uses(Side::Snapshot));
    }

    #[test]
    fn diff_modes_are_mutually_exclusive_at_the_parser() {
        assert!(
            crate::cli::Cli::try_parse_from([
                "aic",
                "oauth",
                "diff",
                "a-client",
                "--local-vs-snapshot",
                "--snapshot-vs-remote",
            ])
            .is_err()
        );
    }

    #[test]
    fn diff_takes_the_text_of_the_two_sides_its_mode_names() {
        let local = json!({"_rev": "l", "v": "local"});
        let snapshot = json!({"_rev": "s", "v": "snapshot"});
        let remote = json!({"_rev": "r", "v": "remote"});

        let sides = diff_sides_from(
            DiffMode::SnapshotVsRemote,
            Some(&local),
            Some(&snapshot),
            Some(&remote),
        );

        assert_eq!(sides.left, Side::Snapshot);
        assert_eq!(sides.right, Side::Remote);
        assert!(sides.left_text.as_ref().unwrap().contains("snapshot"));
        assert!(sides.right_text.as_ref().unwrap().contains("remote"));
        // Normalised the same way the drift check normalises, so a `_rev`
        // that always differs never renders as drift.
        assert!(!sides.left_text.as_ref().unwrap().contains("_rev"));
    }

    /// A side that does not exist stays `None` rather than becoming an empty
    /// document, because the renderer says which of the two it was — and an
    /// empty side and a missing side read identically in a diff.
    #[test]
    fn a_missing_side_is_absent_not_empty() {
        let remote = json!({"v": "remote"});
        let sides = diff_sides_from(DiffMode::RemoteVsLocal, None, None, Some(&remote));

        assert_eq!(sides.right, Side::Local);
        assert!(sides.right_text.is_none());
        assert!(sides.left_text.is_some());
    }

    /// A mistyped client id has no tenant document and no local file, and two
    /// empty sides compare equal — so without this the answer is "identical"
    /// at exit 0, which is the most confident wrong answer the command can
    /// give. The discriminating case is the second: one real side must still
    /// render a diff rather than being swept up by the same guard.
    #[test]
    fn comparing_two_things_that_do_not_exist_is_refused_not_called_identical() {
        let both_absent = diff_sides_from(DiffMode::RemoteVsLocal, None, None, None);
        let message = nothing_to_compare("typo-client", &both_absent, None, LocalArtefacts::none())
            .expect("refusal");
        assert!(message.contains("nothing to compare"), "{message}");
        assert!(message.contains("tenant"), "{message}");
        assert!(message.contains("local"), "{message}");

        let remote = json!({"v": 1});
        let one_side = diff_sides_from(DiffMode::RemoteVsLocal, None, None, Some(&remote));
        assert!(nothing_to_compare("a-client", &one_side, None, LocalArtefacts::none()).is_none());
    }

    /// An unfiltered listing says the plain count; a filtered one has to say
    /// what it hid and which flag hid it. The discriminating case is the
    /// third: `--no-dynamic` can hide a real client whose id happens to be a
    /// UUID, and a tally that only reported the rows printed would give the
    /// operator no way to notice.
    #[test]
    fn a_filtered_listing_says_what_it_hid_and_why() {
        assert_eq!(list_tally(46, 46, &None, false), "46 oauth clients");
        assert_eq!(
            list_tally(46, 5, &Some("test".into()), false),
            "5 of 46 oauth clients (41 hidden by --filter \"test\")"
        );
        assert_eq!(
            list_tally(1002, 12, &None, true),
            "12 of 1002 oauth clients (990 hidden by --no-dynamic)"
        );
        assert_eq!(
            list_tally(1002, 3, &Some("participant".into()), true),
            "3 of 1002 oauth clients (999 hidden by --filter \"participant\" and --no-dynamic)"
        );
    }

    /// Presentation only. A `ClientRow` keeps the empty string so a filter
    /// cannot match the placeholder — `--filter -` must not return every
    /// unnamed client.
    #[test]
    fn an_unset_field_is_a_dash_in_the_table_and_empty_in_the_row() {
        assert_eq!(absent_cell(""), "-");
        assert_eq!(absent_cell("Active"), "Active");

        let row = spec::client_row(&json!({"_id": "AicEdit"}));
        assert_eq!(row.status, "");
        assert!(!spec::row_matches(&row, "-"));
    }

    /// `get` exists because reading a client used to mean `pull`, which
    /// writes a workspace file and overwrites the snapshot. There is no flag
    /// that makes it write, and no `--force`: the parse is the guarantee.
    #[test]
    fn get_reads_and_takes_no_write_flag() {
        let cli =
            crate::cli::Cli::try_parse_from(["aic", "oauth", "get", "a-client", "--json"]).unwrap();
        let Some(crate::cli::Command::Oauth {
            command: OauthCommand::Get { id, json, .. },
        }) = cli.command
        else {
            panic!("expected oauth get");
        };
        assert_eq!(id, "a-client");
        assert!(json);

        assert!(
            crate::cli::Cli::try_parse_from(["aic", "oauth", "get", "a-client", "--force"])
                .is_err()
        );
    }

    /// The mode a refusal explains itself with. Swapping the two renders a
    /// correct diff that answers the wrong question — on drift it would show
    /// your own edits, which are not what changed.
    #[test]
    fn a_refusal_shows_the_comparison_that_caused_it() {
        assert_eq!(
            blocked_push_diff_mode(&PushBlockReason::RemoteDrift),
            DiffMode::SnapshotVsRemote
        );
        assert_eq!(
            blocked_push_diff_mode(&PushBlockReason::MissingSnapshot),
            DiffMode::RemoteVsLocal
        );
    }

    /// Through the renderer rather than the guard alone: a `render_diff_sides`
    /// that forgot to consult `nothing_to_compare` would pass the guard's own
    /// test and still shell out to `git` on two empty files, printing
    /// "identical" at exit 0.
    #[test]
    fn the_renderer_refuses_before_diffing_two_absent_sides() {
        let sides = diff_sides_from(DiffMode::RemoteVsLocal, None, None, None);
        let error = render_diff_sides(
            "typo-client",
            "sandbox",
            "alpha",
            &sides,
            LocalArtefacts::none(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("nothing to compare"), "{error}");
    }

    /// Each side's note has to name that side's own cause: pointing at
    /// `oauth pull` when the client simply is not on the tenant sends someone
    /// to run a command that cannot succeed.
    #[test]
    fn each_absent_side_names_its_own_cause() {
        let pull = Some("aic oauth pull a-client --tenant sandbox --realm alpha");
        let local = absent_side_note("a-client", Side::Local, "sandbox", "alpha", pull, "nothing");
        let snapshot = absent_side_note(
            "a-client",
            Side::Snapshot,
            "sandbox",
            "alpha",
            pull,
            "nothing",
        );
        let remote = absent_side_note(
            "a-client",
            Side::Remote,
            "sandbox",
            "alpha",
            pull,
            "nothing",
        );

        assert!(local.contains("no local export"), "{local}");
        assert!(local.contains("aic oauth pull a-client"), "{local}");
        assert!(snapshot.contains("no snapshot"), "{snapshot}");
        assert!(remote.contains("sandbox/alpha"), "{remote}");
        assert!(
            !remote.contains("oauth pull"),
            "a client that is not there cannot be pulled: {remote}"
        );
    }

    /// `pull` writes **both** the local export and the snapshot, so it is only
    /// safe advice when neither survives. The three discriminating cases are
    /// the ones where something does: suggesting it then destroys the side the
    /// operator still has — their local edits, or the snapshot that is the
    /// only record of what the tenant looked like when they pulled.
    /// `pull` writes **both** the local export and the snapshot, so it is safe
    /// advice exactly when neither file exists.
    ///
    /// The rule reads the files, not the compared sides, and that is the whole
    /// point. Two earlier versions read the sides and were wrong: the second
    /// could not see a file its mode had not loaded, so the default
    /// `remote-vs-local` (which never loads the snapshot) and
    /// `--snapshot-vs-remote` (which never loads the local file) each offered
    /// a `pull` that would overwrite the other. Those are the middle two cases
    /// here, and both are data loss.
    #[test]
    fn pull_is_suggested_only_when_neither_file_it_writes_exists() {
        let cases = [
            (false, false, true),
            (true, false, false),
            (false, true, false),
            (true, true, false),
        ];
        for (local, snapshot, expect_suggestion) in cases {
            let artefacts = LocalArtefacts { local, snapshot };
            assert_eq!(
                safe_pull_suggestion(artefacts, "a-client", "sandbox", "alpha").is_some(),
                expect_suggestion,
                "local={local} snapshot={snapshot}"
            );
        }

        // …and the note then says what pull would cost instead of naming it.
        let survivor = LocalArtefacts {
            local: false,
            snapshot: true,
        };
        let pull = safe_pull_suggestion(survivor, "a-client", "sandbox", "alpha");
        let note = absent_side_note(
            "a-client",
            Side::Local,
            "sandbox",
            "alpha",
            pull.as_deref(),
            &pull_cost(survivor),
        );
        // Names the file at risk, which here is the snapshot — not the local
        // side this comparison is missing.
        assert!(note.contains("would overwrite the snapshot"), "{note}");
    }

    /// The suggestion has to reach a user. It used to be produced only for the
    /// both-absent case, which the renderer refuses *before* printing any
    /// note — so the coordinate-bearing command existed and was unreachable.
    /// This drives the renderer for the case that now carries it, and pins
    /// that the refusal carries it too.
    #[test]
    fn the_coordinate_bearing_pull_actually_reaches_the_output() {
        let pull =
            safe_pull_suggestion(LocalArtefacts::none(), "a-client", "staging", "bravo").unwrap();
        let note = absent_side_note(
            "a-client",
            Side::Local,
            "staging",
            "bravo",
            Some(&pull),
            "nothing",
        );
        assert!(note.contains("--tenant staging"), "{note}");
        assert!(note.contains("--realm bravo"), "{note}");

        let neither = diff_sides_from(DiffMode::RemoteVsLocal, None, None, None);
        let pull = safe_pull_suggestion(LocalArtefacts::none(), "a-client", "staging", "bravo");
        let refusal = nothing_to_compare(
            "a-client",
            &neither,
            pull.as_deref(),
            LocalArtefacts::none(),
        )
        .unwrap();
        assert!(refusal.contains("--tenant staging"), "{refusal}");

        // Both compared sides gone but a third file still on disk: the
        // refusal has to name it, or "neither side has it" reads as nothing
        // left at all — right before someone reaches for `pull`.
        let survivor = LocalArtefacts {
            local: true,
            snapshot: false,
        };
        let pull = safe_pull_suggestion(survivor, "a-client", "staging", "bravo");
        let refusal = nothing_to_compare("a-client", &neither, pull.as_deref(), survivor).unwrap();
        assert!(refusal.contains("local file is still there"), "{refusal}");
        assert!(refusal.contains("would overwrite it"), "{refusal}");
    }

    /// A tenant name is not restricted to a safe character set, so a
    /// suggestion has to survive being pasted. The discriminating case is the
    /// space: unquoted, it runs as two arguments.
    #[test]
    fn a_suggested_command_survives_being_pasted() {
        assert_eq!(shell_quote("sandbox"), "sandbox");
        assert_eq!(shell_quote("client-a.v2_x"), "client-a.v2_x");
        assert_eq!(shell_quote("two words"), "'two words'");
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
        assert_eq!(shell_quote(""), "''");
    }

    /// The refusals are the messages someone actually copies out of a failed
    /// push, so they carry coordinates too — and say what re-pulling costs,
    /// because it overwrites the local file the refusal just protected.
    #[test]
    fn a_refusal_names_the_tenant_and_realm_it_refused_for() {
        for reason in [
            PushBlockReason::RemoteDrift,
            PushBlockReason::MissingSnapshot,
        ] {
            let message = push_block_message("a-client", &reason, "staging", "bravo");
            assert!(message.contains("--tenant staging"), "{message}");
            assert!(message.contains("--realm bravo"), "{message}");
            assert!(message.contains("overwrites your local file"), "{message}");
        }
    }
}
