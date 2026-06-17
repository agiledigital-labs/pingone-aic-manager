//! Pure data and derived state for ESV variables.
//!
//! This module owns the tenant caches, edit form state, write plans, refresh
//! payloads, and render-oriented summaries. It deliberately contains no
//! background tasks or event sends; those live in `ops`.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use crate::app::App;
use crate::esv::api::StartupStatus;
use crate::tui::list_state::TenantListState;
use crate::tui::widgets::TextField;

/// Per-tenant ESV load state. `app.esv.list.data` maps tenant name → this.
#[derive(Debug, Clone)]
pub enum LoadState {
    Loading,
    Loaded(Vec<serde_json::Value>),
    Failed(String),
}

/// Which sub-view of the ESVs tab is showing. Secrets and variables share the
/// tab's apply/restart banner and a single background poll; mappings are static
/// content and are only surfaced on lower-environment tenants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EsvView {
    Variables,
    Secrets,
    Mappings,
}

impl EsvView {
    pub fn next(self, mappings_allowed: bool) -> Self {
        match self.clamp(mappings_allowed) {
            EsvView::Variables => EsvView::Secrets,
            EsvView::Secrets if mappings_allowed => EsvView::Mappings,
            EsvView::Secrets => EsvView::Variables,
            EsvView::Mappings => EsvView::Variables,
        }
    }

    pub fn prev(self, mappings_allowed: bool) -> Self {
        match self.clamp(mappings_allowed) {
            EsvView::Variables if mappings_allowed => EsvView::Mappings,
            EsvView::Variables => EsvView::Secrets,
            EsvView::Secrets => EsvView::Variables,
            EsvView::Mappings => EsvView::Secrets,
        }
    }

    pub fn clamp(self, mappings_allowed: bool) -> Self {
        if mappings_allowed {
            self
        } else {
            match self {
                EsvView::Mappings => EsvView::Variables,
                EsvView::Variables | EsvView::Secrets => self,
            }
        }
    }
}

/// Per-tenant apply state shown in the ESV banner. This is sticky: refreshes
/// keep showing the previous value until a tenant response lets us replace it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyState {
    NoChanges,
    Unapplied(usize),
    Restarting(usize),
}

impl ApplyState {
    pub fn from_authoritative(startup: StartupStatus, pending: usize) -> Self {
        match startup {
            StartupStatus::Restarting => Self::Restarting(pending),
            StartupStatus::Ready if pending > 0 => Self::Unapplied(pending),
            StartupStatus::Ready => Self::NoChanges,
        }
    }
}

/// Result of a background ESV refresh. Variables and startup status are kept
/// separate so we can update whichever half succeeded without clobbering the
/// cached state owned by the other half.
#[derive(Debug)]
pub struct RefreshOutcome {
    pub variables: std::result::Result<Vec<serde_json::Value>, String>,
    pub pending_variables: std::result::Result<Vec<serde_json::Value>, String>,
    pub secrets: std::result::Result<Vec<serde_json::Value>, String>,
    pub pending_secrets: std::result::Result<Vec<serde_json::Value>, String>,
    pub startup: std::result::Result<StartupStatus, String>,
}

/// One row in the rendered ESV list. The UI consumes `Vec<Match>` from
/// [`State::matches`]; sorted-by-score with match positions in `_id` for
/// per-char highlight.
#[derive(Debug, Clone)]
pub struct Match {
    pub idx: Option<usize>,
    pub id: String,
    pub score: u32,
    pub positions: Vec<u32>,
    pub deleted: bool,
}

/// Extract the `_id` field of an ESV variable; falls back to `"?"` when
/// the API returned something unexpected.
pub fn id_of(v: &serde_json::Value) -> &str {
    v.get("_id").and_then(|x| x.as_str()).unwrap_or("?")
}

/// True iff a variable's `loaded` flag is `false` — i.e. the runtime
/// hasn't picked it up yet and a tenant restart is needed. New variables
/// land with `loaded=false`, as do recently-saved edits.
pub fn is_pending(v: &serde_json::Value) -> bool {
    !v.get("loaded").and_then(|x| x.as_bool()).unwrap_or(true)
}

/// How many variables in this tenant's cached list still need a restart
/// to take effect. Drives the banner above the preview pane and gates
/// the `^S` apply keybind.
/// The single source of truth for the ESV tab's restart gating. Folds both
/// halves of the tab together: variable + secret changes that need a restart
/// (`pending`), and variable + secret writes still in flight (`in_flight`).
/// All banner / `^S` / apply-state decisions go through here so the two halves
/// can never disagree.
#[derive(Debug, Clone, Copy, Default)]
pub struct EsvPending {
    pub pending: usize,
    pub in_flight: usize,
}

pub fn pending_summary(app: &App, tenant: &str) -> EsvPending {
    let mut ids = app
        .esv
        .list
        .pending_ids
        .get(tenant)
        .cloned()
        .unwrap_or_default();
    if let Some(LoadState::Loaded(items)) = app.esv.list.data.get(tenant) {
        ids.extend(
            items
                .iter()
                .filter(|v| is_pending(v))
                .map(|v| id_of(v).to_string()),
        );
    }
    // Secrets share the tab's restart gate: a `useInPlaceholders:true` secret
    // whose active version hasn't loaded also needs the tenant restart.
    let pending = ids.len() + crate::secrets::state::pending_count(app, tenant);
    let var_in_flight = app
        .esv
        .in_flight_writes
        .iter()
        .filter(|(t, _)| t == tenant)
        .count();
    let secret_in_flight = app
        .secret
        .in_flight
        .iter()
        .filter(|(t, _)| t == tenant)
        .count();
    EsvPending {
        pending,
        in_flight: var_in_flight + secret_in_flight,
    }
}

pub fn pending_count(app: &App, tenant: &str) -> usize {
    pending_summary(app, tenant).pending
}

/// AIC's documented set of expression types for variables. Acts as a chip
/// cycle on the edit form — `←/→` step through `ORDER`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpressionType {
    String,
    Array,
    Bool,
    Int,
    Number,
    Object,
    List,
    KeyValueList,
    Base64EncodedInlined,
}

impl ExpressionType {
    pub const ORDER: &'static [ExpressionType] = &[
        ExpressionType::String,
        ExpressionType::Array,
        ExpressionType::Bool,
        ExpressionType::Int,
        ExpressionType::Number,
        ExpressionType::Object,
        ExpressionType::List,
        ExpressionType::KeyValueList,
        ExpressionType::Base64EncodedInlined,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            ExpressionType::String => "string",
            ExpressionType::Array => "array",
            ExpressionType::Bool => "bool",
            ExpressionType::Int => "int",
            ExpressionType::Number => "number",
            ExpressionType::Object => "object",
            ExpressionType::List => "list",
            ExpressionType::KeyValueList => "keyvaluelist",
            ExpressionType::Base64EncodedInlined => "base64encodedinlined",
        }
    }

    /// Parse an AIC-emitted expressionType string. Falls back to `String`
    /// for unknown values rather than refusing to load — better to let the
    /// user re-pick than to block editing.
    pub fn parse(s: &str) -> Self {
        Self::ORDER
            .iter()
            .copied()
            .find(|e| e.as_str() == s)
            .unwrap_or(ExpressionType::String)
    }

    pub fn cycle(self, delta: i32) -> Self {
        let i = Self::ORDER.iter().position(|e| *e == self).unwrap_or(0) as i32;
        let n = Self::ORDER.len() as i32;
        let next = (i + delta).rem_euclid(n) as usize;
        Self::ORDER[next]
    }

    /// Local pre-flight validation of the on-screen value against the
    /// selected type. Catches type / value mismatches before we ship a
    /// PUT that would either be rejected by AIC or — worse — accepted
    /// and confuse the runtime. Returns `Ok(())` for types we don't have
    /// a strong contract for (string, list, keyvaluelist,
    /// base64encodedinlined).
    pub fn validate(self, value: &str) -> Result<(), String> {
        match self {
            ExpressionType::String
            | ExpressionType::List
            | ExpressionType::KeyValueList
            | ExpressionType::Base64EncodedInlined => Ok(()),
            ExpressionType::Bool => match value.trim() {
                "true" | "false" => Ok(()),
                _ => Err("Value must be 'true' or 'false'".into()),
            },
            ExpressionType::Int => value
                .trim()
                .parse::<i64>()
                .map(|_| ())
                .map_err(|_| "Value must be an integer".into()),
            ExpressionType::Number => value
                .trim()
                .parse::<f64>()
                .map(|_| ())
                .map_err(|_| "Value must be a number".into()),
            ExpressionType::Object => match serde_json::from_str::<serde_json::Value>(value.trim())
            {
                Ok(serde_json::Value::Object(_)) => Ok(()),
                Ok(_) => Err("Value must be a JSON object (e.g. {\"k\":\"v\"})".into()),
                Err(e) => Err(format!("Value must be valid JSON: {e}")),
            },
            ExpressionType::Array => {
                match serde_json::from_str::<serde_json::Value>(value.trim()) {
                    Ok(serde_json::Value::Array(_)) => Ok(()),
                    Ok(_) => Err("Value must be a JSON array (e.g. [1,2,3])".into()),
                    Err(e) => Err(format!("Value must be valid JSON: {e}")),
                }
            }
        }
    }
}

/// Focusable fields on the edit form. Tab/Shift-Tab walks through these
/// in order, skipping read-only rows. `Id` is only focusable when
/// creating a new variable; the edit form has it pinned as read-only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditField {
    Id,
    Description,
    Type,
    Value,
    Save,
}

impl EditField {
    const EDIT_ORDER: &'static [EditField] = &[
        EditField::Description,
        EditField::Type,
        EditField::Value,
        EditField::Save,
    ];
    const CREATE_ORDER: &'static [EditField] = &[
        EditField::Id,
        EditField::Description,
        EditField::Type,
        EditField::Value,
        EditField::Save,
    ];

    fn order(creating: bool) -> &'static [EditField] {
        if creating {
            Self::CREATE_ORDER
        } else {
            Self::EDIT_ORDER
        }
    }

    pub fn next(self, creating: bool) -> Self {
        let order = Self::order(creating);
        let i = order.iter().position(|f| *f == self).unwrap_or(0);
        order[(i + 1) % order.len()]
    }

    pub fn prev(self, creating: bool) -> Self {
        let order = Self::order(creating);
        let i = order.iter().position(|f| *f == self).unwrap_or(0);
        order[(i + order.len() - 1) % order.len()]
    }
}

/// Live state for the edit form. `original` is the snapshot we were
/// editing from — we compare against a fresh refetch on Save for
/// content-based conflict detection (variables have no `_rev`). For a
/// create flow, `creating` is true, `original` is `null`, and `id_input`
/// is the user-typed identifier.
#[derive(Debug)]
pub struct EditState {
    pub id: String,
    pub original: serde_json::Value,
    pub creating: bool,
    /// Editable identifier when `creating`; ignored otherwise.
    pub id_input: TextField,
    pub description: TextField,
    pub expr_type: ExpressionType,
    pub value: TextField,
    pub focused: EditField,
    /// Pre-flight validation error to surface in the form. Set on bad
    /// id / type-value mismatch; cleared on any user input.
    pub error: Option<String>,
}

/// Fully-validated save payload, captured before any production confirmation
/// modal takes focus. Executing the plan performs the optimistic local update
/// and spawns the tenant write.
#[derive(Debug)]
pub struct SavePlan {
    pub(crate) tenant_name: String,
    pub(crate) id: String,
    pub(crate) description: String,
    pub(crate) expr_type: String,
    pub(crate) value_b64: String,
    pub(crate) original: Option<serde_json::Value>,
    pub(crate) optimistic: serde_json::Value,
    pub(crate) was_creating: bool,
}

/// Fully-captured delete payload. The original body is required both for
/// conflict detection and for undo.
#[derive(Debug, Clone)]
pub struct DeletePlan {
    pub(crate) tenant_name: String,
    pub(crate) id: String,
    pub(crate) original: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct DeleteTombstone {
    pub deleted_at: Instant,
    pub body: serde_json::Value,
}

#[derive(Debug)]
pub struct State {
    /// Shared per-tenant list mechanics: cached data, pending ids, the search
    /// query, and cursor/scroll. See [`TenantListState`].
    pub list: TenantListState,
    /// Tenants whose ESV refetch is currently in flight — guards against
    /// duplicate spawns when the user re-enters the tab or the poll fires.
    pub refreshing: HashSet<String>,
    /// When the last poll-refresh ran. Drives the 30s cadence in `tick`.
    pub last_poll: Instant,

    /// In-flight edit. `None` = preview pane (read-only display). `Some`
    /// = the right pane is the editable form. See `start_edit` / `cancel_edit`.
    pub editing: Option<EditState>,

    /// Variables we just saved that may not yet appear in a polled list
    /// (AIC's `/environment/variables` is eventually consistent — a brand-
    /// new variable can take a few seconds to show up). Keyed by
    /// `(tenant, id)`; each value is `(saved_at, body)`. On every
    /// `apply_refresh` we re-merge any entries still within
    /// `RECENT_WRITE_TTL` so the local view never loses them.
    pub recent_writes: HashMap<(String, String), (Instant, serde_json::Value)>,

    /// Variables deleted locally/through AIC that we keep visible as local
    /// undo tombstones. Ping's pending endpoints do not report deletes.
    pub recent_deletes: HashMap<(String, String), DeleteTombstone>,

    /// (tenant, id) for any optimistic save whose background request
    /// failed. The local cache still holds the user's attempted body
    /// (via `recent_writes`); the list renders these rows in red so the
    /// user can re-open and retry. Cleared on a subsequent successful
    /// save for the same id.
    pub failed_writes: HashSet<(String, String)>,

    /// Last known apply state per tenant. Refreshes update this only after
    /// authoritative tenant responses arrive; while a refresh is in flight
    /// the previous value keeps rendering.
    pub apply_states: HashMap<String, ApplyState>,

    /// When a restart first moved into `ApplyState::Restarting`. This is
    /// display metadata only; it never times the state out.
    pub restart_started_at: HashMap<String, Instant>,

    /// (tenant, id) for every optimistic save whose background PUT
    /// hasn't returned yet. Drives the "queued" banner colour and gates
    /// the `^S` apply keybind — we don't want to restart while a write
    /// is in flight. Cleared by `apply_save_result` regardless of
    /// success / failure.
    pub in_flight_writes: HashSet<(String, String)>,

    /// Original values for optimistic deletes that are still in flight.
    /// Used to restore the local cache if the background DELETE fails.
    pub in_flight_deletes: HashMap<(String, String), serde_json::Value>,

    /// Delete plan waiting on the local y/n confirmation popover.
    pub pending_delete: Option<DeletePlan>,

    /// Which half of the tab (variables / secrets) is showing.
    pub view: EsvView,
}

/// How long we keep a freshly-saved variable pinned to the local cache
/// after a save. Long enough to cover AIC's indexing lag on creates.
pub const RECENT_WRITE_TTL: std::time::Duration = std::time::Duration::from_secs(120);

/// How long we keep a delete tombstone: both the red `!` ghost row in the
/// list and the negative pin that suppresses the deleted id if AIC's
/// eventually-consistent list endpoint still returns it. After this the
/// ghost drops off; the undo entry itself outlives it (recover via `^Y`).
pub const DELETE_TOMBSTONE_TTL: std::time::Duration = std::time::Duration::from_secs(300);

impl Default for State {
    fn default() -> Self {
        Self {
            list: TenantListState::new(),
            refreshing: HashSet::new(),
            last_poll: Instant::now(),
            editing: None,
            recent_writes: HashMap::new(),
            recent_deletes: HashMap::new(),
            failed_writes: HashSet::new(),
            apply_states: HashMap::new(),
            restart_started_at: HashMap::new(),
            in_flight_writes: HashSet::new(),
            in_flight_deletes: HashMap::new(),
            pending_delete: None,
            view: EsvView::Variables,
        }
    }
}

/// Banner display mode — picks the colour and the wording. Computed
/// fresh on every draw from the cached apply state plus in-flight saves.
#[derive(Debug, Clone, Copy)]
pub enum BannerState {
    None,
    /// `n` variables have been saved and need a tenant restart to take
    /// effect. The default state — pastel blue.
    ToApply(usize),
    /// `n` background saves haven't returned yet. Pastel purple. `^S`
    /// is disabled until queued drops to zero.
    Queued(usize),
    /// `n` variables are mid-restart. Pastel yellow. `^S` is a no-op.
    Applying(usize),
}

/// Number of background ESV saves currently in flight for `tenant`.
pub fn queued_count(app: &App, tenant: &str) -> usize {
    // In-flight writes for *both* halves of the tab — a secret mutation in
    // flight must gate `^S` just like a variable one.
    pending_summary(app, tenant).in_flight
}

/// Pick which banner (if any) to display. Precedence is applying, then
/// queued, then to-apply. The applying state wins even when queued > 0
/// because a restart in progress is the more important signal.
pub fn banner_state(app: &App, tenant: &str) -> BannerState {
    if let Some(ApplyState::Restarting(n)) = app.esv.apply_states.get(tenant).copied() {
        return BannerState::Applying(n);
    }
    let q = queued_count(app, tenant);
    if q > 0 {
        return BannerState::Queued(q);
    }
    if let Some(ApplyState::Unapplied(n)) = app.esv.apply_states.get(tenant).copied() {
        return BannerState::ToApply(n);
    }
    BannerState::None
}

/// True when the "applying" banner state should be shown. This is driven by
/// cached tenant state, not elapsed local time.
pub fn is_applying(app: &App, tenant: &str) -> bool {
    matches!(
        app.esv.apply_states.get(tenant),
        Some(ApplyState::Restarting(_))
    )
}

/// True when `^S` can open the restart confirmation for this tenant.
pub fn can_request_restart(app: &App, tenant: &str) -> bool {
    matches!(
        app.esv.apply_states.get(tenant),
        Some(ApplyState::Unapplied(n)) if *n > 0
    ) && queued_count(app, tenant) == 0
}

impl State {
    pub fn new() -> Self {
        Self::default()
    }

    /// Drop view state (filter + selection). Called on tenant switch.
    pub fn reset_view(&mut self) {
        self.list.query.clear();
        self.list.selected = 0;
        self.list.scroll = 0;
    }

    /// Apply the fuzzy filter to a tenant's loaded ESV list. Returns
    /// matches sorted by score (descending), with match positions for
    /// per-char highlighting. Empty when the tenant isn't `Loaded`.
    pub fn matches(&self, tenant: Option<&str>) -> Vec<Match> {
        let Some(name) = tenant else {
            return Vec::new();
        };
        let Some(LoadState::Loaded(items)) = self.list.data.get(name) else {
            return Vec::new();
        };
        if self.list.query.is_empty() {
            let mut indexed: Vec<Match> = items
                .iter()
                .enumerate()
                .map(|(i, v)| Match {
                    idx: Some(i),
                    id: id_of(v).to_string(),
                    score: 0,
                    positions: Vec::new(),
                    deleted: false,
                })
                .collect();
            let live_ids: HashSet<String> = items.iter().map(|v| id_of(v).to_string()).collect();
            indexed.extend(
                self.recent_deletes
                    .iter()
                    .filter(|((t, id), _)| t == name && !live_ids.contains(id))
                    .map(|((_, id), _)| Match {
                        idx: None,
                        id: id.clone(),
                        score: 0,
                        positions: Vec::new(),
                        deleted: true,
                    }),
            );
            indexed.sort_by(|a, b| a.id.cmp(&b.id));
            return indexed;
        }
        use nucleo_matcher::{
            Config, Matcher, Utf32Str,
            pattern::{AtomKind, CaseMatching, Normalization, Pattern},
        };
        let mut matcher = Matcher::new(Config::DEFAULT);
        let pattern = Pattern::new(
            self.list.query.value(),
            CaseMatching::Ignore,
            Normalization::Smart,
            AtomKind::Fuzzy,
        );
        let mut out: Vec<Match> = Vec::new();
        let mut buf = Vec::new();
        let mut positions: Vec<u32> = Vec::new();
        let live_ids: HashSet<String> = items.iter().map(|v| id_of(v).to_string()).collect();
        let mut rows: Vec<(Option<usize>, String, bool, bool)> = items
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let id = id_of(v).to_string();
                let pending = is_pending(v)
                    || self
                        .list
                        .pending_ids
                        .get(name)
                        .is_some_and(|ids| ids.contains(&id));
                (Some(i), id, false, pending)
            })
            .collect();
        rows.extend(
            self.recent_deletes
                .iter()
                .filter(|((t, id), _)| t == name && !live_ids.contains(id))
                .map(|((_, id), _)| (None, id.clone(), true, false)),
        );
        for (idx, id, deleted, pending) in rows {
            // Build the fuzzy haystack from `id` plus synthetic tags so
            // `/!`, `/!pending`, `/!failed` filter to those rows. The
            // tags are stripped from the highlight positions so the id
            // renders without spurious tag chars.
            let failed = self.failed_writes.contains(&(name.to_string(), id.clone()));
            let mut haystack_text = id.clone();
            if pending {
                haystack_text.push_str(" !pending");
            }
            if failed {
                haystack_text.push_str(" !failed");
            }
            if deleted {
                haystack_text.push_str(" !deleted");
            }
            let id_chars = id.chars().count();
            let haystack = Utf32Str::new(&haystack_text, &mut buf);
            positions.clear();
            if let Some(score) = pattern.indices(haystack, &mut matcher, &mut positions) {
                let display_positions: Vec<u32> = positions
                    .iter()
                    .copied()
                    .filter(|p| (*p as usize) < id_chars)
                    .collect();
                out.push(Match {
                    idx,
                    id: id.clone(),
                    score,
                    positions: display_positions,
                    deleted,
                });
            }
        }
        out.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.id.cmp(&b.id)));
        out
    }

    /// Re-clamp the selection in case the matched list shrunk (e.g. after
    /// a refetch returns a shorter list, or after the filter changed).
    pub fn clamp_selection(&mut self, n: usize) {
        if self.list.selected >= n {
            self.list.selected = n.saturating_sub(1);
        }
    }
}

/// Outcome of a background save attempt. Carries the server's echo on
/// success so the local cache can absorb server-managed fields
/// (`lastChangeDate`, `lastChangedBy`).
#[derive(Debug)]
pub struct SaveOutcome {
    pub body: serde_json::Value,
    /// True when we opened the form as an "edit" but the variable was
    /// absent on AIC (a prior create failed), so the save became a create.
    pub created: bool,
}

#[derive(Debug)]
pub struct DeleteOutcome;

#[derive(Debug)]
pub struct UndoOutcome {
    pub(crate) description: String,
    pub(crate) applied: UndoApplied,
}

#[derive(Debug)]
pub(crate) enum UndoApplied {
    Upsert {
        id: String,
        body: serde_json::Value,
    },
    Delete {
        id: String,
        body: Option<serde_json::Value>,
    },
    /// A secret was deleted as the undo of its create. The secrets list owns
    /// the local-state update (variables and secrets are cached separately).
    SecretRemoved {
        id: String,
    },
    /// A secret's description was reverted. Just re-poll the secrets list.
    SecretDescriptionSet,
}

#[derive(Debug)]
pub enum UndoFailure {
    Conflict(String),
    Failed(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn esv_view_cycles_through_mappings_when_allowed() {
        assert_eq!(EsvView::Variables.next(true), EsvView::Secrets);
        assert_eq!(EsvView::Secrets.next(true), EsvView::Mappings);
        assert_eq!(EsvView::Mappings.next(true), EsvView::Variables);

        assert_eq!(EsvView::Variables.prev(true), EsvView::Mappings);
        assert_eq!(EsvView::Mappings.prev(true), EsvView::Secrets);
        assert_eq!(EsvView::Secrets.prev(true), EsvView::Variables);
    }

    #[test]
    fn esv_view_cycles_between_variables_and_secrets_when_mappings_are_blocked() {
        assert_eq!(EsvView::Variables.next(false), EsvView::Secrets);
        assert_eq!(EsvView::Secrets.next(false), EsvView::Variables);
        assert_eq!(EsvView::Mappings.next(false), EsvView::Secrets);

        assert_eq!(EsvView::Variables.prev(false), EsvView::Secrets);
        assert_eq!(EsvView::Secrets.prev(false), EsvView::Variables);
        assert_eq!(EsvView::Mappings.prev(false), EsvView::Secrets);
    }

    #[test]
    fn authoritative_apply_state_prefers_startup_restarting() {
        assert_eq!(
            ApplyState::from_authoritative(StartupStatus::Restarting, 0),
            ApplyState::Restarting(0)
        );
        assert_eq!(
            ApplyState::from_authoritative(StartupStatus::Restarting, 3),
            ApplyState::Restarting(3)
        );
    }

    #[test]
    fn authoritative_apply_state_uses_pending_when_ready() {
        assert_eq!(
            ApplyState::from_authoritative(StartupStatus::Ready, 0),
            ApplyState::NoChanges
        );
        assert_eq!(
            ApplyState::from_authoritative(StartupStatus::Ready, 2),
            ApplyState::Unapplied(2)
        );
    }
}
