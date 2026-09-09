//! Plain input specifications and request transforms for OAuth2 clients.
//!
//! These types contain no TUI state. The CLI create command and a future tab
//! create flow can therefore build identical bodies from the tenant template.

use serde_json::{Map, Value, json};

/// Common OAuth2 client settings exposed by create surfaces.
///
/// `secret` is intentionally excluded from `Debug`: client secrets must never
/// reach logs or diagnostics.
#[derive(Default)]
pub struct CreateClientSpec {
    pub name: Option<String>,
    pub description: Option<String>,
    pub client_type: Option<String>,
    pub secret: Option<String>,
    pub scopes: Vec<String>,
    pub default_scopes: Vec<String>,
    pub redirect_uris: Vec<String>,
    pub grants: Vec<String>,
    pub response_types: Vec<String>,
    pub token_endpoint_auth_method: Option<String>,
    pub subject_type: Option<String>,
    pub implied_consent: Option<bool>,
    pub access_token_lifetime: Option<u64>,
    pub refresh_token_lifetime: Option<u64>,
    pub authorization_code_lifetime: Option<u64>,
}

/// The grant-list change requested by `aic oauth grant add` or `remove`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrantOperation {
    Add,
    Remove,
}

/// Result of applying a grant-list change to a complete client object.
#[derive(Debug, Clone, PartialEq)]
pub struct GrantUpdate {
    pub body: Value,
    pub grants: Vec<String>,
    pub changed: bool,
}

const PROVIDER_GROUPS: &[&str] = &[
    "advancedOIDCConfig",
    "coreOIDCConfig",
    "advancedOAuth2Config",
    "coreOAuth2Config",
    "clientDynamicRegistrationConfig",
    "consent",
    "cibaConfig",
    "deviceCodeConfig",
    "pluginsConfig",
    "aiAgentsConfig",
];

const TOKEN_EXCHANGE_GRANT: &str = "urn:ietf:params:oauth:grant-type:token-exchange";
const TOKEN_TYPE_PREFIX: &str = "urn:ietf:params:oauth:token-type:";

/// Project a provider document into compact, human-readable CLI rows.
///
/// This intentionally accepts the untyped service document: its documented
/// shape is a skeleton and AM may add groups. Inherited field wrappers are
/// unwrapped so equivalent effective values render identically.
pub fn provider_summary(doc: &Value) -> Vec<(String, String)> {
    // Same reason `client_summary` does it: `pluginsConfig` and any group this
    // command does not know are printed as whole JSON cells, so a
    // `*-encrypted` value inside one would reach the terminal verbatim.
    let doc = &mask_secret_values(doc);
    let mut rows = Vec::new();

    for group in PROVIDER_GROUPS {
        rows.push((group.to_string(), provider_group_state(doc, group)));
        match *group {
            "advancedOAuth2Config" => {
                rows.push((
                    "  token-exchange granted".to_string(),
                    token_exchange_granted(doc).to_string(),
                ));
                push_provider_field_rows(&mut rows, doc, group, "grantTypes");
                push_provider_field_rows(&mut rows, doc, group, "tokenExchangeClasses");
                push_provider_field_rows(
                    &mut rows,
                    doc,
                    group,
                    "acceptAudienceParametersInTokenExchangeRequests",
                );
            }
            "coreOAuth2Config" => {
                push_provider_field_rows(&mut rows, doc, group, "accessTokenMayActScript");
            }
            "pluginsConfig" => push_provider_group_fields(&mut rows, doc, group),
            _ => {}
        }
    }

    if let Some(groups) = doc.as_object() {
        for (name, value) in groups {
            if !name.starts_with('_') && !PROVIDER_GROUPS.contains(&name.as_str()) {
                rows.push((format!("unknown.{name}"), provider_value_cell(value)));
            }
        }
    } else {
        rows.push(("document".to_string(), provider_value_cell(doc)));
    }

    rows
}

fn provider_group<'a>(doc: &'a Value, group: &str) -> Option<&'a Value> {
    doc.as_object()?.get(group).map(inherited_value)
}

fn provider_group_state(doc: &Value, group: &str) -> String {
    match provider_group(doc, group) {
        None => "<absent>".to_string(),
        Some(Value::Object(config)) if config.is_empty() => "<empty>".to_string(),
        Some(Value::Object(_)) => "<configured>".to_string(),
        Some(value) => format!("<not an object: {}>", provider_value_cell(value)),
    }
}

fn push_provider_field_rows(
    rows: &mut Vec<(String, String)>,
    doc: &Value,
    group: &str,
    field: &str,
) {
    let label = match field {
        "acceptAudienceParametersInTokenExchangeRequests" => {
            "  accept audience parameters".to_string()
        }
        field => format!("  {field}"),
    };
    let render = if field == "tokenExchangeClasses" {
        token_exchange_class_cell
    } else {
        provider_value_cell
    };
    match provider_group(doc, group) {
        None => rows.push((label, "<group absent>".to_string())),
        Some(Value::Object(config)) => match config.get(field) {
            Some(value) => push_provider_value_rows(rows, &label, value, render),
            None => rows.push((label, "<absent>".to_string())),
        },
        Some(_) => rows.push((label, "<group is not an object>".to_string())),
    }
}

fn push_provider_group_fields(rows: &mut Vec<(String, String)>, doc: &Value, group: &str) {
    if let Some(Value::Object(config)) = provider_group(doc, group) {
        for (field, value) in config {
            push_provider_value_rows(rows, &format!("  {field}"), value, provider_value_cell);
        }
    }
}

fn push_provider_value_rows(
    rows: &mut Vec<(String, String)>,
    label: &str,
    value: &Value,
    render: fn(&Value) -> String,
) {
    match inherited_value(value) {
        Value::Array(values) if values.is_empty() => {
            rows.push((label.to_string(), "<empty>".to_string()));
        }
        Value::Array(values) => {
            for value in values {
                rows.push((label.to_string(), render(value)));
            }
        }
        value => rows.push((label.to_string(), render(value))),
    }
}

fn token_exchange_granted(doc: &Value) -> &'static str {
    let Some(Value::Object(config)) = provider_group(doc, "advancedOAuth2Config") else {
        return "no";
    };
    let Some(grants) = config.get("grantTypes") else {
        return "no";
    };
    let Some(grants) = inherited_value(grants).as_array() else {
        return "<unknown>";
    };
    if grants
        .iter()
        .any(|grant| grant.as_str() == Some(TOKEN_EXCHANGE_GRANT))
    {
        "yes"
    } else {
        "no"
    }
}

fn provider_value_cell(value: &Value) -> String {
    match normalized_inherited_value(value) {
        Value::String(value) if value == "[Empty]" => "<not set>".to_string(),
        Value::String(value) => value,
        value => serde_json::to_string(&value).unwrap_or_else(|_| "<unprintable>".to_string()),
    }
}

fn token_exchange_class_cell(value: &Value) -> String {
    let fallback = provider_value_cell(value);
    inherited_value(value)
        .as_str()
        .and_then(summarize_token_exchange_class)
        .unwrap_or(fallback)
}

fn summarize_token_exchange_class(value: &str) -> Option<String> {
    let (mapping, class) = value.split_once('|')?;
    if class.contains('|') {
        return None;
    }
    let (from, to) = mapping.split_once("=>")?;
    if to.contains("=>") {
        return None;
    }
    let from = from.strip_prefix(TOKEN_TYPE_PREFIX)?;
    let to = to.strip_prefix(TOKEN_TYPE_PREFIX)?;
    let (_, class) = class.rsplit_once('.')?;
    if from.is_empty() || to.is_empty() || class.is_empty() {
        return None;
    }

    Some(format!("{from} => {to} ({class})"))
}

fn normalized_inherited_value(value: &Value) -> Value {
    let value = inherited_value(value);
    match value {
        Value::Array(values) => {
            Value::Array(values.iter().map(normalized_inherited_value).collect())
        }
        Value::Object(values) => Value::Object(
            values
                .iter()
                .map(|(key, value)| (key.clone(), normalized_inherited_value(value)))
                .collect(),
        ),
        value => value.clone(),
    }
}

/// One row of `aic oauth list`.
///
/// Built from the listing projection, whose nested values arrive **unwrapped**
/// (measured 2026-09-09) — so no `inherited_value` here, unlike
/// [`client_summary`], which reads a single-client `GET`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientRow {
    pub id: String,
    pub name: String,
    pub client_type: String,
    pub status: String,
    pub grants: String,
    /// AM mints a UUID `client_id` for a client registered through DCR, so a
    /// UUID-shaped id is how a listing tells the dynamic ones from the ones
    /// someone named. See [`looks_dynamically_registered`] for what that does
    /// and does not prove.
    pub uuid_id: bool,
}

pub fn client_row(value: &Value) -> ClientRow {
    let core = value.get("coreOAuth2ClientConfig");
    let advanced = value.get("advancedOAuth2ClientConfig");
    let id = value
        .get("_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    ClientRow {
        uuid_id: looks_dynamically_registered(&id),
        name: first_string(core, "clientName"),
        client_type: plain_string(core, "clientType"),
        status: plain_string(core, "status"),
        grants: join_strings(advanced, "grantTypes"),
        id,
    }
}

/// Does this id have the shape AM generates for a dynamic registration?
///
/// It tests the **id**, not the provenance: AM records nothing that says a
/// client came from DCR, so this is the observable, not the property. A
/// hand-made client given a UUID id matches, and a dynamic registration that
/// supplied its own `client_id` does not. That is why the CLI counts what the
/// flag hid rather than quietly shortening the list.
///
/// Verified 2026-09-10 against a **real** dynamic registration: a client
/// registered at `POST /am/oauth2{realm}/register` came back with
/// `client_id` `53f39d30-3ed1-4fbf-9baa-b53bd5f8474f`, and `--no-dynamic` hid
/// exactly that one row of 30. So AM does mint a UUID for a registration that
/// supplies no `client_id`, which is the half of this that was inference
/// before.
pub fn looks_dynamically_registered(id: &str) -> bool {
    is_uuid(id)
}

/// The canonical 8-4-4-4-12 hex form, and nothing else.
pub fn is_uuid(value: &str) -> bool {
    let groups: Vec<&str> = value.split('-').collect();
    groups.len() == 5
        && [8, 4, 4, 4, 12] == groups.iter().map(|g| g.len()).collect::<Vec<_>>()[..]
        && groups
            .iter()
            .all(|group| group.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Script ids to script names, for turning a bare UUID in a config document
/// into something a reader recognises.
#[derive(Debug, Default, Clone)]
pub struct ScriptNames(std::collections::BTreeMap<String, String>);

impl ScriptNames {
    pub fn new<I, K, V>(entries: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        Self(
            entries
                .into_iter()
                .map(|(id, name)| (id.into(), name.into()))
                .collect(),
        )
    }

    fn label(&self, id: &str) -> Option<String> {
        self.0.get(id).map(|name| format!("{name} ({id})"))
    }

    /// The script's name alone, or the bare id when nothing resolves it.
    ///
    /// For the width-constrained surfaces. The id is not lost — it is what
    /// `--json` carries and what `oauth get` prints beside the name.
    ///
    /// An **empty** name counts as unresolved: the listing turns a missing or
    /// malformed `name` into `""`, and rendering that produced a cell reading
    /// `access=` — a resolution failure disguised as a resolution, with the
    /// one searchable string thrown away.
    pub fn name_or(&self, id: &str) -> String {
        self.0
            .get(id)
            .filter(|name| !name.is_empty())
            .cloned()
            .unwrap_or_else(|| id.to_string())
    }

    /// The named form, or the bare id when nothing resolves it.
    ///
    /// An unresolved id is a finding, not a formatting problem — printing
    /// `<unknown>` would erase the one string you can go looking with.
    pub fn label_or(&self, id: &str) -> String {
        self.label(id).unwrap_or_else(|| id.to_string())
    }
}

/// Does this row's label name a field that holds a script id?
///
/// Every one of them ends in `Script` — the five plugin pairs and the two
/// may-act fields alike (`docs/api/05-oauth2-oidc.md`). Asking the label is
/// what stops a coincidence from being rewritten: a client `_id`, a scope, a
/// `pluginsConfig` value or an unknown group's contents can all be
/// UUID-shaped, and one that happened to equal a real script id would print
/// as that script's name — including, worst of all, on the `id` row, which
/// would stop showing the client you asked for.
fn labels_a_script_field(label: &str) -> bool {
    label.trim().ends_with("Script")
}

/// Rewrite every script-id cell that is a bare UUID we have a name for.
///
/// A post-pass over finished rows rather than a hook inside each renderer, so
/// `client_summary` and `provider_summary` gain it identically and neither has
/// to grow a lookup parameter it would then have to thread everywhere.
///
/// An id with no match is left exactly as it was: a UUID naming a script this
/// realm does not have is a real finding, and rewriting it to `<unknown>`
/// would erase the id you need in order to go looking.
pub fn resolve_script_ids(rows: &mut [(String, String)], names: &ScriptNames) {
    for (label, value) in rows.iter_mut() {
        if labels_a_script_field(label)
            && is_uuid(value)
            && let Some(named) = names.label(value)
        {
            *value = named;
        }
    }
}

/// The `…Script` / `…PluginType` pairs in an override block, and the two ways
/// they disagree — each of which AM accepts and then quietly does the opposite
/// of what the document appears to say.
///
/// Pairing is derived from the keys present rather than a hardcoded list, so
/// the may-act fields (which have **no** `…PluginType` companion — setting the
/// id is enough) correctly produce no finding, and a pair AM adds later is
/// covered without an edit here.
pub fn override_faults(doc: &Value) -> Vec<String> {
    let Some(Value::Object(config)) = doc.get(CLIENT_OVERRIDES).map(inherited_value) else {
        return Vec::new();
    };

    // A dormant block's faults are latent, not live — enabling the block is
    // what makes them bite. Saying so beats both alternatives: suppressing
    // them hides a problem you are about to create, and reporting them flatly
    // sends someone hunting a runtime effect that is not happening.
    let dormant = overrides_enabled(doc) != Some(true);
    let prefix = if dormant { "(dormant) " } else { "" };

    let mut faults = Vec::new();
    for (field, value) in config {
        let Some(base) = field.strip_suffix("Script") else {
            continue;
        };
        let companion = format!("{base}PluginType");
        let Some(plugin_type) = config.get(&companion) else {
            continue;
        };
        let scripted = inherited_value(plugin_type).as_str() == Some("SCRIPTED");
        let has_script = !override_entry_inherits(field, value);

        if has_script && !scripted {
            let plugin_type = provider_value_cell(plugin_type);
            faults.push(format!(
                "{prefix}{field} is set but {companion} is {plugin_type}, not SCRIPTED — AM ignores the script"
            ));
        } else if scripted && !has_script {
            faults.push(format!(
                "{prefix}{companion} is SCRIPTED but {field} is not set — nothing runs"
            ));
        }
    }
    faults
}

/// One client's part in the realm's token-exchange configuration.
///
/// Built from the exchange projection, whose nested values arrive **unwrapped**
/// exactly as [`ClientRow`]'s do, and whose absent fields are absent rather
/// than null — so every reader here treats missing as "not set".
///
/// The two fields that drive a categorical answer are `Option<bool>` rather
/// than `bool`, and the third state is **unknown**: the projection carried a
/// value of a type this reader cannot use. Substituting `false` there let the
/// command state "no client holds the grant" about a client that may well
/// hold it, with a warning appended underneath — and a warning does not make
/// the preceding sentence true.
///
/// Everything here is what the **client** carries. Whether a client is a
/// working subject also depends on the realm's `accessTokenMayActScript`,
/// which this type deliberately does not know about — see [`may_act_source`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExchangeRow {
    pub id: String,
    /// Holds `urn:ietf:params:oauth:grant-type:token-exchange`, so it can
    /// present someone else's token and ask to act. This is the only thing in
    /// AM that says which clients may act at all. `None` when `grantTypes`
    /// could not be read.
    pub actor: Option<bool>,
    /// The may-act script fields this client sets, as `(field, script id)`.
    ///
    /// A client whose block is **live** and carries one of these stamps
    /// `may_act` on the tokens it issues — the exchange is refused unless the
    /// subject token carries the claim. Under a dormant block these are
    /// configuration that does not run; [`Self::live_may_act`] is the one that
    /// applies. There is no field naming the actor: the script decides, so the
    /// relationship is only fully readable by reading the script.
    pub may_act: Vec<(String, String)>,
    /// `providerOverridesEnabled` — the master switch over the whole override
    /// block. Not `true`, and every field in that block is ignored. `None`
    /// when the value could not be read, which makes the may-act source
    /// unknowable rather than inherited.
    pub overrides_live: Option<bool>,
    /// `tokenExchangeAuthLevel`, verbatim. `0` is AM's default; what a
    /// non-zero value does is **not verified** — see open question 4 in
    /// `docs/api/22-token-exchange.md`. Reported, not interpreted.
    pub auth_level: Option<i64>,
    /// `advancedOAuth2ClientConfig.allowedResourceServerAudienceValues`; empty
    /// means the client cannot be asked for an `audience` at all. Not part of
    /// the override block, so [`Self::overrides_live`] does not gate it —
    /// confirmed against the live client schema 2026-09-10.
    pub audience_values: Vec<String>,
    /// `overrideOAuth2ClientConfig.acceptAudienceParametersInTokenExchangeRequests`,
    /// as configured. `None` when the client does not carry the key.
    ///
    /// It lives in the **override** block, so it is only in effect when
    /// [`Self::overrides_live`] — an earlier version read it from
    /// `advancedOAuth2ClientConfig`, where it does not exist, and therefore
    /// always answered `None`.
    pub accept_audience: Option<bool>,
    /// False when the field was present and unreadable, so `accept_audience:
    /// None` under a live block means "could not tell" rather than "absent,
    /// so the block's default applies".
    pub accept_audience_readable: bool,
    /// Fields that were **present** in the projection with a type this reader
    /// cannot use.
    ///
    /// Absent stays quiet, because absent really does mean not set;
    /// present-and-wrong says so, and where the field drives an answer that
    /// answer becomes unknown rather than a default.
    pub shape_faults: Vec<String>,
}

impl ExchangeRow {
    /// The client's may-act entries are configured but the master switch is
    /// not `true`, so none of them runs.
    pub fn may_act_dormant(&self) -> bool {
        !self.may_act.is_empty() && self.overrides_live == Some(false)
    }

    /// The may-act entries that actually run — none, when the block is
    /// dormant or its state is unknown.
    pub fn live_may_act(&self) -> &[(String, String)] {
        if self.overrides_live == Some(true) {
            &self.may_act
        } else {
            &[]
        }
    }

    /// `acceptAudienceParametersInTokenExchangeRequests` as the client's
    /// override sets it, or `None` when the override does not decide it.
    ///
    /// Deliberately **not** called effective. The runtime value cannot be
    /// computed from this row: with the block dormant the realm's
    /// `advancedOAuth2Config` copy governs, and with the block live but the
    /// field absent the block's *own* default governs — a value this
    /// projection never sees.
    pub fn accept_audience_override(&self) -> Option<bool> {
        if self.overrides_live == Some(true) {
            self.accept_audience
        } else {
            None
        }
    }

    /// Does the client set an `audience` allow-list or a non-default auth
    /// level of its own?
    pub fn has_own_exchange_settings(&self) -> bool {
        !self.audience_values.is_empty()
            || self.accept_audience.is_some()
            || self.auth_level.is_some_and(|level| level != 0)
    }

    /// Does this client appear in the default view?
    ///
    /// A realm-wide may-act script makes **every** client a subject, which is
    /// a fact about the realm and is reported in the header — repeating it on
    /// all 1000 rows would be the noise, not the signal. So the default view
    /// keeps the clients that say something the realm does not:
    ///
    /// * it holds the grant, or that could not be determined;
    /// * it configures a may-act script of its own, live or dormant;
    /// * it has a **live** override block while the realm stamps `may_act` —
    ///   an explicit exception to the realm-wide script, which stops
    ///   inheritance for the whole block and so stops the claim being stamped
    ///   at all. Hiding pure inheritance is defensible; hiding an exception to
    ///   it is not, and a filter that only looked at script entries did
    ///   exactly that;
    /// * it sets its own audience or auth-level values;
    /// * something in its projection could not be read.
    pub fn participates(&self, realm_stamps: Option<bool>) -> bool {
        // A live block is an exception to *any* script the realm might have,
        // so an unknown realm keeps it too — `None` here means "there may be
        // one", not "there is none".
        let realm_may_stamp = realm_stamps != Some(false);
        self.actor != Some(false)
            || !self.may_act.is_empty()
            || self.has_own_exchange_settings()
            || !self.shape_faults.is_empty()
            || (realm_may_stamp && self.overrides_live != Some(false))
    }
}

/// What actually stamps `may_act` on this client's tokens.
///
/// The client's override wins when the block is live; otherwise the realm's
/// `accessTokenMayActScript` applies; otherwise nothing does, and every
/// exchange of this client's tokens is refused. Reading only the client — as
/// the first version of this did — mislabels all three cases: a dormant
/// override reads as a subject, a client inheriting a live realm script reads
/// as a bystander, and a dormant override under a live realm script reads as a
/// subject for the wrong reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MayActSource {
    /// The client's own override block, and it is in effect.
    Client,
    /// Inherited from the realm's `accessTokenMayActScript`.
    Realm,
    /// Nothing stamps it, so this client's tokens cannot be exchanged.
    /// Either no source is configured at all, or a **live** override block
    /// explicitly sets no script — which also stops the realm's applying.
    None,
    /// The master switch could not be read, so which of the three applies is
    /// not knowable from this document.
    Unknown,
}

/// `realm_stamps` is `None` when the realm's own may-act script could not be
/// read. That has to travel: collapsing it to `false` at the call site
/// published `"none"` for a client whose answer was simply not known, which is
/// the failure this enum exists to prevent — one level up from where it was
/// last fixed.
pub fn may_act_source(row: &ExchangeRow, realm_stamps: Option<bool>) -> MayActSource {
    match (row.overrides_live, realm_stamps) {
        // A live block answers the question either way, whatever the realm
        // holds. `[Empty]` under it is an explicit "no script", not a request
        // to inherit.
        (Some(true), _) if row.may_act.is_empty() => MayActSource::None,
        (Some(true), _) => MayActSource::Client,
        // Inheritance applies, so the realm's answer is the answer.
        (Some(false), Some(true)) => MayActSource::Realm,
        (Some(false), Some(false)) => MayActSource::None,
        (Some(false), None) | (None, _) => MayActSource::Unknown,
    }
}

/// The may-act fields, in the order they are shown.
///
/// Two, because AM stamps the claim separately for the two token types and a
/// client can set one without the other; a deployment that exchanges access
/// tokens and forgets `oidcMayActScript` still works, so this is a listing
/// rather than a fault.
const MAY_ACT_FIELDS: &[&str] = &["accessTokenMayActScript", "oidcMayActScript"];

/// The group the non-override exchange fields live in.
const CLIENT_ADVANCED: &str = "advancedOAuth2ClientConfig";

/// What a value *is*, for a message about what it should have been.
fn json_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

/// A config group, or `None` when it is absent **or** present and not an
/// object — the second case noted as a fault, because every field under it is
/// then unreadable rather than unset.
fn readable_group<'a>(
    group: Option<&'a Value>,
    name: &str,
    faults: &mut ShapeFaults,
) -> Option<&'a Value> {
    let group = group?;
    match inherited_value(group) {
        Value::Object(_) => Some(group),
        other => {
            faults.note("client", name, other, "an object");
            None
        }
    }
}

/// Collects "this field was present and unusable" notes for one document.
#[derive(Default)]
struct ShapeFaults(Vec<String>);

impl ShapeFaults {
    fn note(&mut self, group: &str, field: &str, value: &Value, expected: &str) {
        self.0.push(format!(
            "{group}.{field} is {}, expected {expected}",
            json_type_name(value)
        ));
    }

    fn mentions(&self, needle: &str) -> bool {
        self.0.iter().any(|fault| fault.contains(needle))
    }

    fn bool_at(&mut self, group: &str, field: &str, value: Option<&Value>) -> Option<bool> {
        match value {
            Some(Value::Bool(value)) => Some(*value),
            Some(other) => {
                self.note(group, field, other, "a boolean");
                None
            }
            None => None,
        }
    }
}

pub fn exchange_row(value: &Value) -> ExchangeRow {
    let advanced = value.get(CLIENT_ADVANCED);
    let overrides = value.get(CLIENT_OVERRIDES);
    let id = value
        .get("_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    let mut faults = ShapeFaults::default();
    // A group that is present but not an object makes every field in it
    // unreadable, and `projected` cannot tell that from the group being
    // absent — which produced a confident `Some(false)` with no fault at all.
    let advanced = readable_group(advanced, CLIENT_ADVANCED, &mut faults);
    let overrides = readable_group(overrides, CLIENT_OVERRIDES, &mut faults);
    let advanced_readable = advanced.is_some() || !faults.mentions(CLIENT_ADVANCED);
    let overrides_readable = overrides.is_some() || !faults.mentions(CLIENT_OVERRIDES);

    let actor = match projected(advanced, "grantTypes") {
        Some(Value::Array(grants)) => {
            let unreadable = grants.iter().filter(|grant| !grant.is_string());
            let mut any_unreadable = false;
            for grant in unreadable {
                faults.note(CLIENT_ADVANCED, "grantTypes[]", grant, "a string");
                any_unreadable = true;
            }
            if grants
                .iter()
                .any(|grant| grant.as_str() == Some(TOKEN_EXCHANGE_GRANT))
            {
                // Present is present, whatever else the list holds.
                Some(true)
            } else if any_unreadable {
                // Absent from the elements that could be read is not absent:
                // the unreadable one may have been the grant.
                None
            } else {
                Some(false)
            }
        }
        Some(other) => {
            faults.note(CLIENT_ADVANCED, "grantTypes", other, "an array");
            None
        }
        None if advanced_readable => Some(false),
        None => None,
    };

    let may_act = MAY_ACT_FIELDS
        .iter()
        .filter_map(|field| {
            let value = projected(overrides, field)?;
            // The same "inherit" vocabulary the summary uses: `[Empty]`, null
            // and an empty value all mean the field is not set, and AM writes
            // `[Empty]` far more often than it omits the key.
            if override_entry_inherits(field, value) {
                return None;
            }
            // A script id is a string. Anything else is not a configured
            // script, and counting it as one selected `MayActSource::Client`
            // and suppressed the deny-by-default warning.
            match value {
                Value::String(script) => Some(((*field).to_string(), script.clone())),
                other => {
                    faults.note(CLIENT_OVERRIDES, field, other, "a string");
                    None
                }
            }
        })
        .collect::<Vec<_>>();

    let overrides_live = match projected(overrides, "providerOverridesEnabled") {
        Some(Value::Bool(live)) => Some(*live),
        Some(other) => {
            faults.note(
                CLIENT_OVERRIDES,
                "providerOverridesEnabled",
                other,
                "a boolean",
            );
            None
        }
        // Absent is not enabled — the doc's wording is that `true` is
        // required — and that is a reading, not a failure to read. Unless the
        // whole group was unreadable, in which case nothing was read.
        None if overrides_readable => Some(false),
        None => None,
    };

    let auth_level = match projected(advanced, "tokenExchangeAuthLevel") {
        Some(Value::Number(number)) => {
            let level = number.as_i64();
            if level.is_none() {
                faults.note(
                    CLIENT_ADVANCED,
                    "tokenExchangeAuthLevel",
                    &Value::Number(number.clone()),
                    "a whole number",
                );
            }
            level
        }
        Some(other) => {
            faults.note(CLIENT_ADVANCED, "tokenExchangeAuthLevel", other, "a number");
            None
        }
        None => None,
    };

    let audience_values = match projected(advanced, "allowedResourceServerAudienceValues") {
        // Render every element, not only the strings: silently dropping a
        // non-string would shorten the list a reader is using to decide
        // whether an `audience` can be asked for at all. A non-string is also
        // reported, because `7` and `"7"` render alike and are not alike.
        Some(Value::Array(values)) => {
            for value in values.iter().filter(|value| !value.is_string()) {
                faults.note(
                    CLIENT_ADVANCED,
                    "allowedResourceServerAudienceValues[]",
                    value,
                    "a string",
                );
            }
            values.iter().map(provider_value_cell).collect()
        }
        Some(other) => {
            faults.note(
                CLIENT_ADVANCED,
                "allowedResourceServerAudienceValues",
                other,
                "an array",
            );
            Vec::new()
        }
        None => Vec::new(),
    };

    let accept_audience = faults.bool_at(
        CLIENT_OVERRIDES,
        "acceptAudienceParametersInTokenExchangeRequests",
        projected(overrides, "acceptAudienceParametersInTokenExchangeRequests"),
    );
    let accept_audience_readable =
        overrides_readable && !faults.mentions("acceptAudienceParametersInTokenExchangeRequests");

    ExchangeRow {
        actor,
        may_act,
        overrides_live,
        auth_level,
        audience_values,
        accept_audience,
        accept_audience_readable,
        shape_faults: faults.0,
        id,
    }
}

/// The realm half of the answer, and what could not be read of it.
///
/// Each field that drives a categorical finding carries its own readability,
/// because "the realm configures no exchangers" and "the exchanger list could
/// not be read" are different sentences and only one of them is ever true.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RealmExchange {
    /// Does `advancedOAuth2Config.grantTypes` include the exchange grant?
    /// `None` when the list — or the group holding it — could not be read.
    /// Tenant-wide, so more worth withholding than a single client's, not
    /// less: a substituted `false` reports every actor in the realm as broken.
    pub granted: Option<bool>,
    /// `coreOAuth2Config.accessTokenMayActScript`, the fallback for every
    /// client whose override block is not live.
    pub may_act_script: Option<String>,
    /// False when the may-act field or its group was unreadable, so
    /// `may_act_script: None` means "could not tell" rather than "not set".
    pub may_act_readable: bool,
    /// The `from => to` exchanges the realm has an exchanger class for.
    pub exchangers: Vec<String>,
    /// False when the class list was unreadable, so an empty `exchangers`
    /// means "could not tell" rather than "none configured".
    pub exchangers_readable: bool,
    /// False when the realm's audience switch or its group was unreadable, so
    /// `accept_audience: None` means "could not tell" rather than "not set" —
    /// and a client cell reading `realm` then resolves to nothing knowable.
    pub accept_audience_readable: bool,
    /// The realm's `acceptAudienceParametersInTokenExchangeRequests`, so a
    /// reader can resolve what a `null` client-side override means.
    pub accept_audience: Option<bool>,
    pub shape_faults: Vec<String>,
}

impl Default for RealmExchange {
    fn default() -> Self {
        Self {
            granted: None,
            may_act_script: None,
            may_act_readable: true,
            exchangers: Vec::new(),
            exchangers_readable: true,
            accept_audience: None,
            accept_audience_readable: true,
            shape_faults: Vec::new(),
        }
    }
}

impl RealmExchange {
    /// Does the realm stamp `may_act` for the clients that inherit? `None`
    /// when the answer could not be read.
    pub fn stamps(&self) -> Option<bool> {
        self.may_act_readable.then(|| self.may_act_script.is_some())
    }
}

const PROVIDER_ADVANCED: &str = "advancedOAuth2Config";
const PROVIDER_CORE: &str = "coreOAuth2Config";

pub fn realm_exchange(doc: &Value) -> RealmExchange {
    let mut faults = ShapeFaults::default();
    let advanced = match provider_group(doc, PROVIDER_ADVANCED) {
        Some(Value::Object(config)) => Some(config),
        Some(other) => {
            faults.note("provider", PROVIDER_ADVANCED, other, "an object");
            None
        }
        None => None,
    };
    // Absent and unreadable are different: an absent group leaves the realm on
    // AM's defaults, an unreadable one leaves us unable to say what it holds.
    let advanced_readable = !faults.mentions(PROVIDER_ADVANCED);

    let granted = match advanced
        .and_then(|config| config.get("grantTypes"))
        .map(inherited_value)
    {
        // Same rule as a client's grant list: a match is a match whatever else
        // the array holds, but an unreadable element may have *been* the
        // grant, so absence among the readable ones cannot answer "no".
        Some(Value::Array(grants)) => {
            let mut any_unreadable = false;
            for grant in grants.iter().filter(|grant| !grant.is_string()) {
                faults.note(PROVIDER_ADVANCED, "grantTypes[]", grant, "a string");
                any_unreadable = true;
            }
            if grants
                .iter()
                .any(|grant| grant.as_str() == Some(TOKEN_EXCHANGE_GRANT))
            {
                Some(true)
            } else if any_unreadable {
                None
            } else {
                Some(false)
            }
        }
        Some(other) => {
            faults.note(PROVIDER_ADVANCED, "grantTypes", other, "an array");
            None
        }
        None if advanced_readable => Some(false),
        None => None,
    };

    let (exchangers, exchangers_readable) = match advanced
        .and_then(|config| config.get("tokenExchangeClasses"))
        .map(inherited_value)
    {
        Some(Value::Array(values)) => {
            // An element must at least be a string. A non-string rendered as
            // an exchanger, which both invented a configured class and
            // suppressed the "no exchangers" finding. The string's *content*
            // stays unvalidated on purpose — custom classes are supported and
            // `token_exchange_class_cell` already falls back to printing one
            // it cannot parse.
            let mut readable = true;
            for value in values.iter().filter(|value| !value.is_string()) {
                faults.note(
                    PROVIDER_ADVANCED,
                    "tokenExchangeClasses[]",
                    value,
                    "a string",
                );
                readable = false;
            }
            (
                values
                    .iter()
                    .filter(|value| value.is_string())
                    .map(token_exchange_class_cell)
                    .collect(),
                readable,
            )
        }
        Some(other) => {
            faults.note(PROVIDER_ADVANCED, "tokenExchangeClasses", other, "an array");
            (Vec::new(), false)
        }
        None => (Vec::new(), advanced_readable),
    };

    let accept_audience = faults.bool_at(
        PROVIDER_ADVANCED,
        "acceptAudienceParametersInTokenExchangeRequests",
        advanced
            .and_then(|config| config.get("acceptAudienceParametersInTokenExchangeRequests"))
            .map(inherited_value),
    );
    let accept_audience_readable =
        advanced_readable && !faults.mentions("acceptAudienceParametersInTokenExchangeRequests");

    let core = match provider_group(doc, PROVIDER_CORE) {
        Some(Value::Object(config)) => Some(config),
        Some(other) => {
            faults.note("provider", PROVIDER_CORE, other, "an object");
            None
        }
        None => None,
    };
    let mut may_act_readable = !faults.mentions(PROVIDER_CORE);
    let may_act_script = match core.and_then(|config| config.get("accessTokenMayActScript")) {
        Some(value) if override_entry_inherits("accessTokenMayActScript", value) => None,
        Some(value) => match inherited_value(value) {
            Value::String(script) => Some(script.clone()),
            other => {
                faults.note(PROVIDER_CORE, "accessTokenMayActScript", other, "a string");
                may_act_readable = false;
                None
            }
        },
        None => None,
    };

    RealmExchange {
        granted,
        may_act_script,
        may_act_readable,
        exchangers,
        exchangers_readable,
        accept_audience,
        accept_audience_readable,
        shape_faults: faults.0,
    }
}

/// The statically visible prerequisites that are not met.
///
/// Deliberately **not** "everything that makes an exchange fail". Several
/// causes are invisible from configuration: a may-act script that names the
/// wrong actor or throws, a `tokenExchangeAuthLevel` the subject token does
/// not reach, an `audience` the acting client does not allow, a forged subject
/// token. Each produces the same `400 invalid_request: Invalid token
/// exchange.`, so ruling these out narrows the search rather than ending it —
/// and the may-act script still has to be read.
///
/// A conclusion is stated only where its inputs were readable. An unknown
/// actor or an unknown master switch withholds the conclusion it feeds and
/// reports the shape fault instead: a warning underneath a wrong sentence does
/// not make the sentence right.
pub fn exchange_findings(realm: &RealmExchange, rows: &[ExchangeRow]) -> Vec<String> {
    let mut findings = Vec::new();
    let realm_stamps = realm.stamps();
    let actors = rows.iter().filter(|row| row.actor == Some(true)).count();
    let unknown_actors = rows.iter().filter(|row| row.actor.is_none()).count();
    // **Every** client, not only the actors. Acting and stamping are
    // independent roles — one client can hold both, and the sandbox has one
    // that does — so a client's actor status says nothing about whether it is
    // a subject, and filtering on it removed exactly the rows that could
    // disprove this.
    let none_stamped = realm_stamps.is_some()
        && !rows.is_empty()
        && rows
            .iter()
            .all(|row| may_act_source(row, realm_stamps) == MayActSource::None);

    if realm.granted == Some(false) && actors > 0 {
        findings.push(format!(
            "the realm does not grant {TOKEN_EXCHANGE_GRANT}, so all {actors} client(s) holding it fail with unsupported_grant_type"
        ));
    }
    // `unknown_actors` withholds this one: a client whose grant list could not
    // be read might be the actor whose absence is being reported.
    if realm.granted == Some(true) && actors == 0 && unknown_actors == 0 {
        findings.push(
            "the realm grants token-exchange but no client holds the grant, so nothing can act"
                .to_string(),
        );
    }
    if realm.granted == Some(true) && realm.exchangers.is_empty() && realm.exchangers_readable {
        findings.push(
            "the realm grants token-exchange but configures no tokenExchangeClasses, so no token type can be exchanged"
                .to_string(),
        );
    }
    if actors > 0 && none_stamped {
        findings.push(
            "nothing stamps may_act — no live client override and no realm accessTokenMayActScript — so every exchange is refused as invalid_request"
                .to_string(),
        );
    }
    for row in rows.iter().filter(|row| row.may_act_dormant()) {
        // What happens instead is the operative half, and it differs: with no
        // realm script this client's tokens cannot be exchanged at all, which
        // the earlier wording claimed the opposite of.
        let instead = match (realm.may_act_readable, realm.may_act_script.as_deref()) {
            (true, Some(script)) => format!("the realm's {script} runs instead"),
            (true, None) => {
                "the realm sets none either, so nothing stamps may_act for it".to_string()
            }
            (false, _) => "and the realm's own script could not be read".to_string(),
        };
        findings.push(format!(
            "{}: may-act script set but providerOverridesEnabled is not true, so it does not run — {instead}",
            row.id
        ));
    }
    // Last, because a shape fault means one of the answers above was withheld
    // rather than computed — so it qualifies everything else rather than
    // standing beside it.
    for fault in &realm.shape_faults {
        findings.push(format!("realm: {fault}, so what it governs is unknown"));
    }
    for row in rows {
        for fault in &row.shape_faults {
            findings.push(format!("{}: {fault}, so it was not read", row.id));
        }
    }
    findings
}

/// Case-insensitive substring over the id and the client name.
///
/// Both, because the two disagree in practice and either can be the one you
/// remember: the field report's ticket said `ParticipantMobileApplicationV2`
/// and the tenant had `ParticipantMobileAppV2`.
pub fn row_matches(row: &ClientRow, needle: &str) -> bool {
    // `to_lowercase`, not `to_ascii_lowercase`: the docs promise
    // case-insensitive without qualification, and a client named `ÄBC` should
    // answer `--filter äbc`.
    let needle = needle.to_lowercase();
    row.id.to_lowercase().contains(&needle) || row.name.to_lowercase().contains(&needle)
}

/// The projection's value for `field`, unwrapped if it ever arrives wrapped.
///
/// The measured response is raw, and `inherited_value` is a no-op on a raw
/// value — so paying for it costs nothing and removes a silent failure mode:
/// were the collection endpoint to start returning `{inherited, value}` like
/// the single-client `GET`, a raw-only reader would not error, it would print
/// an empty column.
fn projected<'a>(group: Option<&'a Value>, field: &str) -> Option<&'a Value> {
    group
        .map(inherited_value)
        .and_then(|group| group.get(field))
        .map(inherited_value)
}

fn plain_string(group: Option<&Value>, field: &str) -> String {
    projected(group, field)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// `clientName` is an array with at most one useful value — but a bare string
/// reads as itself rather than as nothing, so a shape change costs a wrong
/// arity rather than a blank column.
fn first_string(group: Option<&Value>, field: &str) -> String {
    let Some(value) = projected(group, field) else {
        return String::new();
    };
    match value {
        Value::Array(values) => values
            .first()
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        Value::String(text) => text.clone(),
        _ => String::new(),
    }
}

fn join_strings(group: Option<&Value>, field: &str) -> String {
    let Some(value) = projected(group, field) else {
        return String::new();
    };
    match value {
        Value::Array(values) => values
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(", "),
        Value::String(text) => text.clone(),
        _ => String::new(),
    }
}

/// The suffix AM puts on a cluster-local AES-wrapped value.
const ENCRYPTED_SUFFIX: &str = "-encrypted";

/// The write-only client secret. AM reads it back as `null`, so it reaches a
/// rendering only from a **local** file someone authored — which is the one
/// case where the value is plaintext rather than AES-wrapped.
const SECRET_FIELD: &str = "userpassword";

/// Does this key hold secret material that must not be rendered?
///
/// Lives here rather than beside any one surface because all three need the
/// same answer and only differ in what they substitute: the CLI diff uses a
/// digest so a rotation still shows as a change, while the summary table and
/// the TUI detail pane use a constant, having nothing to compare against.
pub fn is_secret_key(key: &str) -> bool {
    key.ends_with(ENCRYPTED_SUFFIX) || key == SECRET_FIELD
}

const CLIENT_OVERRIDES: &str = "overrideOAuth2ClientConfig";
const DEFAULT_PLUGIN_CLASS_PREFIX: &str = "org.forgerock.oauth2.core.plugins.registry.Default";
const NOT_SET: &str = "[Empty]";

/// The client fields worth a row, in the order someone reads them: what the
/// client is, how it authenticates, what it may ask for.
///
/// Deliberately not every field — the raw document is ~360 lines and `--json`
/// is right there. Everything omitted is counted, never silently dropped.
const CLIENT_CORE_FIELDS: &[(&str, &str)] = &[
    ("coreOAuth2ClientConfig", "status"),
    ("coreOAuth2ClientConfig", "clientName"),
    ("coreOAuth2ClientConfig", "clientType"),
    ("advancedOAuth2ClientConfig", "tokenEndpointAuthMethod"),
    ("advancedOAuth2ClientConfig", "subjectType"),
    ("advancedOAuth2ClientConfig", "grantTypes"),
    ("advancedOAuth2ClientConfig", "responseTypes"),
    ("coreOAuth2ClientConfig", "scopes"),
    ("coreOAuth2ClientConfig", "defaultScopes"),
    ("coreOAuth2ClientConfig", "redirectionUris"),
    ("advancedOAuth2ClientConfig", "isConsentImplied"),
    ("coreOAuth2ClientConfig", "accessTokenLifetime"),
    ("coreOAuth2ClientConfig", "refreshTokenLifetime"),
    ("coreOAuth2ClientConfig", "authorizationCodeLifetime"),
];

/// Project one OAuth2 client into compact CLI rows.
///
/// Reuses the provider summary's cell rendering, so `[Empty]` reads as
/// `<not set>` and an `{inherited, value}` wrapper renders as its effective
/// value on both surfaces.
pub fn client_summary(doc: &Value) -> Vec<(String, String)> {
    // Before anything is projected, not per-field. The override block renders
    // through the generic value path rather than the core allowlist, so a
    // `*-encrypted` key someone adds there — or one AM adds — reached a row
    // as its raw value until this walked the whole document.
    let doc = &mask_secret_values(doc);
    let mut rows = Vec::new();

    rows.push((
        "id".to_string(),
        doc.get("_id")
            .and_then(Value::as_str)
            .unwrap_or("<absent>")
            .to_string(),
    ));

    for (group, field) in CLIENT_CORE_FIELDS {
        push_client_field_rows(&mut rows, doc, group, field);
    }

    push_override_rows(&mut rows, doc);
    rows
}

/// Replace every secret value with a constant, at any depth.
///
/// A constant rather than the digest `aic oauth diff` uses: a table row has
/// nothing to compare itself against, so the digest would be noise.
fn mask_secret_values(value: &Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.iter().map(mask_secret_values).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| {
                    if is_secret_key(key) && !value.is_null() {
                        (key.clone(), Value::String("<secret>".into()))
                    } else {
                        (key.clone(), mask_secret_values(value))
                    }
                })
                .collect(),
        ),
        value => value.clone(),
    }
}

fn push_client_field_rows(rows: &mut Vec<(String, String)>, doc: &Value, group: &str, field: &str) {
    let label = field.to_string();
    match doc.get(group).map(inherited_value) {
        Some(Value::Object(config)) => match config.get(field) {
            Some(value) => push_provider_value_rows(rows, &label, value, provider_value_cell),
            None => rows.push((label, "<absent>".to_string())),
        },
        Some(_) => rows.push((label, "<group is not an object>".to_string())),
        None => rows.push((label, "<group absent>".to_string())),
    }
}

/// Is the client's whole override block in effect?
///
/// `providerOverridesEnabled` is a master switch, verified 2026-08-25: with it
/// `false` every field in the block is inherited from the realm no matter what
/// it holds, and with it `true` the *whole* block applies at once, its own
/// defaults included. Same JSON, two meanings — a summary that showed the
/// entries without this row would be describing configuration that may not run.
///
/// An **absent** switch is not an enabled one. The doc's wording is that
/// `true` is *required*, so anything else — including a document that never
/// carries the key — leaves the block ignored. Reading `None` as "probably on"
/// is the direction that misleads.
fn overrides_enabled(doc: &Value) -> Option<bool> {
    let block = inherited_value(doc.get(CLIENT_OVERRIDES)?);
    block
        .get("providerOverridesEnabled")
        .map(|value| inherited_value(value) == &Value::Bool(true))
}

/// A `…PluginType` still on `PROVIDER`, a `…Class` still on AM's `Default…`
/// implementation, the `[Empty]` sentinel, `null`, and an empty array all say
/// the same thing: inherit. The `providerOverridesEnabled` row already says it
/// once, so repeating it up to 29 times is the noise, not the signal.
fn override_entry_inherits(field: &str, value: &Value) -> bool {
    let value = inherited_value(value);
    if field.ends_with("PluginType") {
        return value.as_str() == Some("PROVIDER");
    }
    if field.ends_with("Class") {
        return value
            .as_str()
            .is_some_and(|class| class.starts_with(DEFAULT_PLUGIN_CLASS_PREFIX));
    }
    match value {
        Value::Null => true,
        Value::String(text) => text == NOT_SET,
        Value::Array(values) => values.is_empty(),
        _ => false,
    }
}

fn push_override_rows(rows: &mut Vec<(String, String)>, doc: &Value) {
    let Some(Value::Object(config)) = doc.get(CLIENT_OVERRIDES).map(inherited_value) else {
        rows.push((CLIENT_OVERRIDES.to_string(), "<absent>".to_string()));
        return;
    };

    rows.push((
        CLIENT_OVERRIDES.to_string(),
        match overrides_enabled(doc) {
            Some(true) => "<in effect: every field below applies, set or defaulted>".to_string(),
            Some(false) => {
                "<dormant: providerOverridesEnabled is false, the realm applies>".to_string()
            }
            None => "<dormant: providerOverridesEnabled absent, so the realm applies>".to_string(),
        },
    ));

    // One filter in both states, deliberately. An earlier version hid every
    // boolean while the block was dormant, on the theory that an ignored value
    // is not a setting. That is wrong for the question this command gets
    // asked: "is it safe to enable this block?" A dormant
    // `statelessTokensEnabled: true` is precisely what an operator attaching
    // one script needs to see before flipping the switch turns it live — and
    // the old version did worse than hide it, it counted it as "inherited or
    // unset", which it is not. The header says whether the block runs; the
    // rows say what is in it.
    let mut hidden = 0_usize;
    for (field, value) in config {
        // The switch itself is the section header above; counting it as
        // "not shown" would be wrong in both directions — it is shown, and
        // it is not an inherited setting.
        if field == "providerOverridesEnabled" {
            continue;
        }
        if override_entry_inherits(field, value) {
            hidden += 1;
        } else {
            push_provider_value_rows(rows, &format!("  {field}"), value, provider_value_cell);
        }
    }
    if hidden > 0 {
        // Never drop rows silently: say how many and how to see them.
        rows.push((
            "  (not shown)".to_string(),
            format!("{hidden} inherited or unset — `--json` for the whole document"),
        ));
    }
}

/// Build a create body from live defaults, an optional JSON seed, and common
/// inputs. Object seeds merge recursively so a partial seed retains tenant
/// defaults; arrays and scalar values replace their template counterparts.
pub fn build_create_body(
    mut template: Value,
    seed: Option<Value>,
    spec: &CreateClientSpec,
) -> Result<Value, String> {
    if !template.is_object() {
        return Err("oauth client template is not a JSON object".into());
    }

    let has_seed = seed.is_some();
    let seed_sets_token_endpoint_auth_method = seed.as_ref().is_some_and(|seed| {
        seed.pointer("/advancedOAuth2ClientConfig/tokenEndpointAuthMethod")
            .is_some()
    });
    if let Some(seed) = seed {
        if !seed.is_object() {
            return Err("oauth client --from seed is not a JSON object".into());
        }
        merge_value(&mut template, seed);
    }

    if let Some(name) = &spec.name {
        set_field(
            &mut template,
            "coreOAuth2ClientConfig",
            "clientName",
            json!([name]),
        )?;
    }
    if let Some(description) = &spec.description {
        set_field(
            &mut template,
            "advancedOAuth2ClientConfig",
            "descriptions",
            json!([description]),
        )?;
    }
    if let Some(client_type) = &spec.client_type {
        set_field(
            &mut template,
            "coreOAuth2ClientConfig",
            "clientType",
            json!(client_type),
        )?;
    } else if !has_seed {
        set_field(
            &mut template,
            "coreOAuth2ClientConfig",
            "clientType",
            json!("Confidential"),
        )?;
    }
    if let Some(secret) = &spec.secret {
        set_field(
            &mut template,
            "coreOAuth2ClientConfig",
            "userpassword",
            json!(secret),
        )?;
    }
    set_nonempty_array(
        &mut template,
        "coreOAuth2ClientConfig",
        "scopes",
        &spec.scopes,
    )?;
    set_nonempty_array(
        &mut template,
        "coreOAuth2ClientConfig",
        "defaultScopes",
        &spec.default_scopes,
    )?;
    set_nonempty_array(
        &mut template,
        "coreOAuth2ClientConfig",
        "redirectionUris",
        &spec.redirect_uris,
    )?;
    set_nonempty_array(
        &mut template,
        "advancedOAuth2ClientConfig",
        "grantTypes",
        &spec.grants,
    )?;
    set_nonempty_array(
        &mut template,
        "advancedOAuth2ClientConfig",
        "responseTypes",
        &spec.response_types,
    )?;
    if let Some(method) = &spec.token_endpoint_auth_method {
        set_field(
            &mut template,
            "advancedOAuth2ClientConfig",
            "tokenEndpointAuthMethod",
            json!(method),
        )?;
    } else if !seed_sets_token_endpoint_auth_method {
        set_field(
            &mut template,
            "advancedOAuth2ClientConfig",
            "tokenEndpointAuthMethod",
            // Deliberately not AM's template default (`client_secret_basic`).
            // `aic auth` speaks `client_secret_post` unless told otherwise, and
            // a client created here that AM would refuse to authenticate that
            // way is a client this tool cannot use. Override with
            // `--token-endpoint-auth-method client_secret_basic`.
            json!("client_secret_post"),
        )?;
    }
    if let Some(subject_type) = &spec.subject_type {
        set_field(
            &mut template,
            "advancedOAuth2ClientConfig",
            "subjectType",
            json!(subject_type),
        )?;
    }
    if let Some(implied) = spec.implied_consent {
        set_field(
            &mut template,
            "advancedOAuth2ClientConfig",
            "isConsentImplied",
            json!(implied),
        )?;
    }
    set_optional_number(
        &mut template,
        "accessTokenLifetime",
        spec.access_token_lifetime,
    )?;
    set_optional_number(
        &mut template,
        "refreshTokenLifetime",
        spec.refresh_token_lifetime,
    )?;
    set_optional_number(
        &mut template,
        "authorizationCodeLifetime",
        spec.authorization_code_lifetime,
    )?;

    Ok(sanitize_for_write(&template))
}

/// Read the effective grant list without changing the surrounding client shape.
pub fn grant_types(client: &Value) -> Result<Vec<String>, String> {
    let Some(group) = client
        .as_object()
        .and_then(|client| client.get("advancedOAuth2ClientConfig"))
    else {
        return Ok(Vec::new());
    };
    let Some(field) = group.as_object().and_then(|group| group.get("grantTypes")) else {
        return Ok(Vec::new());
    };
    let field = inherited_value(field);
    let Some(grants) = field.as_array() else {
        return Err("advancedOAuth2ClientConfig.grantTypes is not an array".into());
    };
    grants
        .iter()
        .map(|grant| {
            grant.as_str().map(str::to_owned).ok_or_else(|| {
                "advancedOAuth2ClientConfig.grantTypes contains a non-string value".into()
            })
        })
        .collect()
}

/// Apply an idempotent grant-list change and prepare the full replacement body.
///
/// An inherited grant field becomes a local override only when its contents
/// change. Other inherited wrappers are deliberately left untouched so this
/// follows the same round-trip shape as `aic oauth push`.
pub fn update_grants(
    client: &Value,
    requested: &[String],
    operation: GrantOperation,
) -> Result<GrantUpdate, String> {
    let current = grant_types(client)?;
    let mut grants = current.clone();
    match operation {
        GrantOperation::Add => {
            for grant in requested {
                if !grants.contains(grant) {
                    grants.push(grant.clone());
                }
            }
        }
        GrantOperation::Remove => grants.retain(|grant| !requested.contains(grant)),
    }

    let changed = grants != current;
    let mut body = client.clone();
    if changed {
        set_grant_types(&mut body, grants.clone())?;
    }
    Ok(GrantUpdate {
        body: sanitize_for_write(&body),
        grants,
        changed,
    })
}

/// Validate enum-backed fields against the live tenant schema.
///
/// `None`, missing enum metadata, and malformed enum metadata all skip local
/// validation. A server rejection is safer than rejecting a value using an
/// incomplete or version-incompatible schema.
pub fn validate_enumerated_fields(body: &Value, schema: Option<&Value>) -> Result<(), String> {
    let Some(schema) = schema else {
        return Ok(());
    };

    for target in ENUM_TARGETS {
        validate_enum_target(body, schema, target)?;
    }
    Ok(())
}

/// Validate only the grant list against the live tenant schema.
pub fn validate_grant_types(body: &Value, schema: Option<&Value>) -> Result<(), String> {
    let Some(schema) = schema else {
        return Ok(());
    };

    validate_enum_target(body, schema, &GRANT_TYPES_TARGET)
}

/// Remove fields that the OAuth2 client PUT endpoint rejects or must never
/// receive. Encryption wrappers are cluster-local, so the suffix rule applies
/// recursively rather than only to today's known secret fields.
pub(crate) fn sanitize_for_write(client: &Value) -> Value {
    let Value::Object(map) = client else {
        return strip_encrypted_fields(client);
    };

    Value::Object(
        map.iter()
            .filter(|(key, _)| !matches!(key.as_str(), "_id" | "_rev" | "_type" | "_provider"))
            .filter(|(key, _)| !key.ends_with("-encrypted"))
            .map(|(key, value)| (key.clone(), strip_encrypted_fields(value)))
            .collect(),
    )
}

fn merge_value(base: &mut Value, overlay: Value) {
    match (base, overlay) {
        (Value::Object(base), Value::Object(overlay)) => {
            for (key, value) in overlay {
                match base.get_mut(&key) {
                    Some(base_value) => merge_value(base_value, value),
                    None => {
                        base.insert(key, value);
                    }
                }
            }
        }
        (base, overlay) => *base = overlay,
    }
}

fn set_field(body: &mut Value, group: &str, field: &str, value: Value) -> Result<(), String> {
    let groups = body
        .as_object_mut()
        .ok_or_else(|| "oauth client body is not a JSON object".to_string())?;
    let group_value = groups
        .entry(group.to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    let group_body = group_value
        .as_object_mut()
        .ok_or_else(|| format!("oauth client group {group} is not a JSON object"))?;
    group_body.insert(field.to_string(), value);
    Ok(())
}

fn set_nonempty_array(
    body: &mut Value,
    group: &str,
    field: &str,
    values: &[String],
) -> Result<(), String> {
    if !values.is_empty() {
        set_field(body, group, field, json!(values))?;
    }
    Ok(())
}

fn set_optional_number(body: &mut Value, field: &str, value: Option<u64>) -> Result<(), String> {
    if let Some(value) = value {
        set_field(body, "coreOAuth2ClientConfig", field, json!(value))?;
    }
    Ok(())
}

fn strip_encrypted_fields(value: &Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.iter().map(strip_encrypted_fields).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(key, _)| !key.ends_with("-encrypted"))
                .map(|(key, value)| (key.clone(), strip_encrypted_fields(value)))
                .collect(),
        ),
        value => value.clone(),
    }
}

fn inherited_value(value: &Value) -> &Value {
    let Some(object) = value.as_object() else {
        return value;
    };
    if object.get("inherited").and_then(Value::as_bool).is_some() {
        object.get("value").unwrap_or(value)
    } else {
        value
    }
}

fn set_grant_types(client: &mut Value, grants: Vec<String>) -> Result<(), String> {
    let object = client
        .as_object_mut()
        .ok_or_else(|| "oauth client body is not a JSON object".to_string())?;
    let group = object
        .entry("advancedOAuth2ClientConfig")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| {
            "oauth client group advancedOAuth2ClientConfig is not a JSON object".to_string()
        })?;
    let field = group
        .entry("grantTypes")
        .or_insert_with(|| Value::Array(Vec::new()));
    if let Some(field_object) = field.as_object_mut()
        && field_object
            .get("inherited")
            .and_then(Value::as_bool)
            .is_some()
    {
        field_object.insert("inherited".into(), Value::Bool(false));
        field_object.insert("value".into(), json!(grants));
    } else {
        *field = json!(grants);
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct EnumTarget {
    group: &'static str,
    field: &'static str,
    array: bool,
}

const ENUM_TARGETS: &[EnumTarget] = &[
    EnumTarget {
        group: "coreOAuth2ClientConfig",
        field: "clientType",
        array: false,
    },
    EnumTarget {
        group: "advancedOAuth2ClientConfig",
        field: "grantTypes",
        array: true,
    },
    // Inert today: the sandbox schema carries neither `enum` nor `items.enum`
    // for responseTypes (checked 2026-08-06), so this target always skips and
    // the server does the rejecting. Kept rather than deleted so validation
    // starts working for free if a later AM version publishes the choices.
    EnumTarget {
        group: "advancedOAuth2ClientConfig",
        field: "responseTypes",
        array: true,
    },
    EnumTarget {
        group: "advancedOAuth2ClientConfig",
        field: "tokenEndpointAuthMethod",
        array: false,
    },
    EnumTarget {
        group: "advancedOAuth2ClientConfig",
        field: "subjectType",
        array: false,
    },
];

const GRANT_TYPES_TARGET: EnumTarget = EnumTarget {
    group: "advancedOAuth2ClientConfig",
    field: "grantTypes",
    array: true,
};

fn validate_enum_target(body: &Value, schema: &Value, target: &EnumTarget) -> Result<(), String> {
    let schema_path = format!("/properties/{}/properties/{}", target.group, target.field);
    let Some(field_schema) = schema.pointer(&schema_path) else {
        return Ok(());
    };
    let enum_value = if target.array {
        field_schema.pointer("/items/enum")
    } else {
        field_schema.get("enum")
    };
    let Some(allowed) = enum_value.and_then(Value::as_array) else {
        return Ok(());
    };
    let Some(allowed) = allowed
        .iter()
        .map(Value::as_str)
        .collect::<Option<Vec<_>>>()
    else {
        return Ok(());
    };

    let body_path = format!("/{}/{}", target.group, target.field);
    let Some(value) = body.pointer(&body_path).map(inherited_value) else {
        return Ok(());
    };
    let values = if target.array {
        let Some(values) = value.as_array() else {
            return Ok(());
        };
        let Some(values) = values.iter().map(Value::as_str).collect::<Option<Vec<_>>>() else {
            return Ok(());
        };
        values
    } else {
        let Some(value) = value.as_str() else {
            return Ok(());
        };
        vec![value]
    };

    if let Some(invalid) = values.into_iter().find(|value| !allowed.contains(value)) {
        return Err(format!(
            "invalid value {invalid:?} for {}.{}; allowed by the tenant schema: {}",
            target.group,
            target.field,
            allowed.join(", ")
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary_value<'a>(rows: &'a [(String, String)], field: &str) -> &'a str {
        rows.iter()
            .find(|(name, _)| name == field)
            .map(|(_, value)| value.as_str())
            .unwrap_or_else(|| panic!("missing summary field {field}"))
    }

    fn summary_values<'a>(rows: &'a [(String, String)], field: &str) -> Vec<&'a str> {
        rows.iter()
            .filter(|(name, _)| name == field)
            .map(|(_, value)| value.as_str())
            .collect()
    }

    /// The listing projection returns nested values **unwrapped** — measured
    /// 2026-09-09 — unlike the single-client `GET`. The discriminating case is
    /// the second row: feeding a listing row through `inherited_value` would
    /// work on this shape and then silently print `{"inherited":...}` if the
    /// projection ever changed, so the test pins the shape we measured.
    #[test]
    fn a_listing_row_reads_the_unwrapped_projection() {
        let row = client_row(&json!({
            "_id": "service_C1",
            "_rev": "594475566",
            "coreOAuth2ClientConfig": {
                "clientName": ["service_C1"],
                "clientType": "Confidential",
                "status": "Active"
            },
            "advancedOAuth2ClientConfig": {
                "grantTypes": ["client_credentials", "password"]
            }
        }));

        assert_eq!(row.id, "service_C1");
        assert_eq!(row.name, "service_C1");
        assert_eq!(row.client_type, "Confidential");
        assert_eq!(row.status, "Active");
        assert_eq!(row.grants, "client_credentials, password");
        assert!(!row.uuid_id);
    }

    /// `AicEdit` on the sandbox really has `status: null` and no client name,
    /// so a row has to survive both without inventing a value. The empty
    /// string is deliberate: the `-` is added at the table, so a filter
    /// cannot match on it.
    #[test]
    fn a_row_with_nothing_set_reads_as_empty_not_as_a_placeholder() {
        let row = client_row(&json!({
            "_id": "AicEdit",
            "coreOAuth2ClientConfig": {"clientName": null, "status": null}
        }));

        assert_eq!(row.name, "");
        assert_eq!(row.status, "");
        assert_eq!(row.client_type, "");
        assert_eq!(row.grants, "");
    }

    /// A bare UUID becomes `name (uuid)`; anything else is left alone.
    ///
    /// The discriminating case is the last: an id with no match keeps its
    /// UUID. A script id naming something this realm does not have is a real
    /// finding, and rewriting it to a placeholder would erase the one value
    /// you need to go looking with.
    #[test]
    fn a_known_script_id_gains_its_name_and_an_unknown_one_keeps_its_id() {
        let names = ScriptNames::new([(
            "f65303d2-f1ff-4beb-8787-57f9a432c5ce",
            "TestAccessTokenModification",
        )]);
        let mut rows = vec![
            (
                "  accessTokenModificationScript".to_string(),
                "f65303d2-f1ff-4beb-8787-57f9a432c5ce".to_string(),
            ),
            ("  clientType".to_string(), "Confidential".to_string()),
            (
                "  oidcClaimsScript".to_string(),
                "00000000-1111-2222-3333-444444444444".to_string(),
            ),
        ];

        resolve_script_ids(&mut rows, &names);

        assert_eq!(
            rows[0].1,
            "TestAccessTokenModification (f65303d2-f1ff-4beb-8787-57f9a432c5ce)"
        );
        assert_eq!(rows[1].1, "Confidential");
        assert_eq!(rows[2].1, "00000000-1111-2222-3333-444444444444");
    }

    /// Only a row whose label names a script field. The `id` row is the case
    /// that matters most: a client whose id happened to equal a script's would
    /// have stopped showing the client you asked for. A scope, a client name
    /// and an unknown provider group can all be UUID-shaped too.
    #[test]
    fn a_uuid_somewhere_that_is_not_a_script_field_is_left_alone() {
        let uuid = "f65303d2-f1ff-4beb-8787-57f9a432c5ce";
        let names = ScriptNames::new([(uuid, "SomeScript")]);
        let mut rows = vec![
            ("id".to_string(), uuid.to_string()),
            ("clientName".to_string(), uuid.to_string()),
            ("scopes".to_string(), uuid.to_string()),
            ("unknown.someGroup".to_string(), uuid.to_string()),
            ("  validateScopeScript".to_string(), uuid.to_string()),
            ("  accessTokenMayActScript".to_string(), uuid.to_string()),
        ];

        resolve_script_ids(&mut rows, &names);

        for (label, value) in &rows[..4] {
            assert_eq!(value, uuid, "{label} was rewritten");
        }
        // Both script fields, including the may-act one that has no
        // `…PluginType` companion.
        assert_eq!(rows[4].1, format!("SomeScript ({uuid})"));
        assert_eq!(rows[5].1, format!("SomeScript ({uuid})"));
    }

    /// The provider summary prints `pluginsConfig` and any group it does not
    /// know as whole JSON cells, so a secret nested in one reached the
    /// terminal verbatim until this masked the document first.
    #[test]
    fn a_secret_in_an_unknown_provider_group_never_reaches_a_row() {
        let rows = provider_summary(&json!({
            "pluginsConfig": {"something-encrypted": "AQICWrappedBytes"},
            "someGroupAmAddedLater": {"userpassword": "plaintext"}
        }));

        for (label, value) in &rows {
            assert!(!value.contains("AQICWrappedBytes"), "{label} = {value}");
            assert!(!value.contains("plaintext"), "{label} = {value}");
        }
    }

    /// The two ways a `…Script` and its `…PluginType` disagree, both of which
    /// AM accepts and then quietly does the opposite of what the document
    /// says. `JAVA` is in here because the sandbox really has clients set that
    /// way — the rule is "not SCRIPTED", not "is PROVIDER".
    #[test]
    fn a_script_that_cannot_run_is_reported_with_its_reason() {
        let doc = json!({
            "overrideOAuth2ClientConfig": {
                "providerOverridesEnabled": true,
                "oidcClaimsScript": "3cde9333-4516-4561-a734-3429c83ec570",
                "oidcClaimsPluginType": "PROVIDER",
                "validateScopeScript": "9c98f803-f352-480a-99d6-e57428f075a5",
                "validateScopePluginType": "JAVA",
                "evaluateScopeScript": "[Empty]",
                "evaluateScopePluginType": "SCRIPTED"
            }
        });

        let faults = override_faults(&doc);

        assert_eq!(faults.len(), 3, "{faults:#?}");
        assert!(
            faults.iter().any(|f| f.contains("oidcClaimsScript is set")
                && f.contains("is PROVIDER, not SCRIPTED")),
            "{faults:#?}"
        );
        assert!(
            faults.iter().any(|f| f.contains("is JAVA, not SCRIPTED")),
            "{faults:#?}"
        );
        assert!(
            faults
                .iter()
                .any(|f| f.contains("evaluateScopePluginType is SCRIPTED")
                    && f.contains("nothing runs")),
            "{faults:#?}"
        );
    }

    /// A configuration that agrees with itself produces nothing — and neither
    /// do the may-act fields, which have **no** `…PluginType` companion at
    /// all. Pairing off the keys present is what gets that right; a hardcoded
    /// list of five pairs would have needed the exception written down.
    #[test]
    fn a_consistent_block_and_the_companionless_fields_report_nothing() {
        let doc = json!({
            "overrideOAuth2ClientConfig": {
                "providerOverridesEnabled": true,
                "accessTokenModificationScript": "f65303d2-f1ff-4beb-8787-57f9a432c5ce",
                "accessTokenModificationPluginType": "SCRIPTED",
                "validateScopeScript": "[Empty]",
                "validateScopePluginType": "PROVIDER",
                "accessTokenMayActScript": "9c98f803-f352-480a-99d6-e57428f075a5",
                "oidcMayActScript": "[Empty]"
            }
        });

        assert!(
            override_faults(&doc).is_empty(),
            "{:#?}",
            override_faults(&doc)
        );
    }

    /// A dormant block's faults are latent, and saying which is the point:
    /// suppressing them hides what enabling the block would create, and
    /// reporting them flatly sends someone hunting a runtime effect that is
    /// not happening.
    #[test]
    fn a_dormant_blocks_faults_are_marked_as_not_yet_biting() {
        let mut doc = json!({
            "overrideOAuth2ClientConfig": {
                "providerOverridesEnabled": false,
                "oidcClaimsScript": "3cde9333-4516-4561-a734-3429c83ec570",
                "oidcClaimsPluginType": "PROVIDER"
            }
        });

        let dormant = override_faults(&doc);
        assert_eq!(dormant.len(), 1);
        assert!(dormant[0].starts_with("(dormant) "), "{dormant:?}");

        doc["overrideOAuth2ClientConfig"]["providerOverridesEnabled"] = json!(true);
        let live = override_faults(&doc);
        assert!(!live[0].starts_with("(dormant)"), "{live:?}");
    }

    /// The projection was measured raw, and the reader is deliberately built
    /// to survive it not staying that way. A wrapped value or a scalar where
    /// an array was measured would otherwise print a blank column — a silent
    /// wrong answer rather than an error.
    #[test]
    fn a_row_survives_a_projection_that_changes_shape() {
        let wrapped = client_row(&json!({
            "_id": "wrapped",
            "coreOAuth2ClientConfig": {
                "inherited": false,
                "value": {
                    "clientName": {"inherited": false, "value": ["Wrapped"]},
                    "clientType": {"inherited": false, "value": "Confidential"}
                }
            },
            "advancedOAuth2ClientConfig": {
                "grantTypes": {"inherited": false, "value": ["client_credentials"]}
            }
        }));
        assert_eq!(wrapped.name, "Wrapped");
        assert_eq!(wrapped.client_type, "Confidential");
        assert_eq!(wrapped.grants, "client_credentials");

        let scalar = client_row(&json!({
            "_id": "scalar",
            "coreOAuth2ClientConfig": {"clientName": "Just A String"},
            "advancedOAuth2ClientConfig": {"grantTypes": "client_credentials"}
        }));
        assert_eq!(scalar.name, "Just A String");
        assert_eq!(scalar.grants, "client_credentials");
    }

    /// The docs promise case-insensitive without qualification, so ASCII-only
    /// folding would make them wrong rather than merely limited.
    #[test]
    fn the_filter_folds_case_beyond_ascii() {
        let row = client_row(&json!({
            "_id": "client",
            "coreOAuth2ClientConfig": {"clientName": ["ÄBC Pty Ltd"]}
        }));
        assert!(row_matches(&row, "äbc"));
    }

    /// The filter searches both id and name because they disagree in
    /// practice, and either is the one you remember. The discriminating case
    /// is the third: the ticket said `ParticipantMobileApplicationV2` and the
    /// tenant had `ParticipantMobileAppV2`, so an id-only filter finds it from
    /// a name and a name-only filter finds it from an id.
    #[test]
    fn the_filter_searches_the_id_and_the_name() {
        let row = client_row(&json!({
            "_id": "ParticipantMobileAppV2",
            "coreOAuth2ClientConfig": {"clientName": ["Participant Mobile"]}
        }));

        assert!(row_matches(&row, "mobileapp"), "id, case-insensitively");
        assert!(row_matches(&row, "Participant Mobile"), "name");
        assert!(!row_matches(&row, "digital"));
    }

    /// What `--no-dynamic` tests is the id's shape. The discriminating cases
    /// are the near-misses: a name that merely contains a UUID, and a
    /// hex-length-correct id with a non-hex character.
    #[test]
    fn only_a_bare_uuid_id_reads_as_dynamically_registered() {
        assert!(looks_dynamically_registered(
            "004e86a1-1b7a-4cf7-9e2d-48e74a1bc1c0"
        ));
        assert!(!looks_dynamically_registered(
            "client-004e86a1-1b7a-4cf7-9e2d-48e74a1bc1c0"
        ));
        assert!(!looks_dynamically_registered(
            "004e86a1-1b7a-4cf7-9e2d-48e74a1bc1cg"
        ));
        assert!(!looks_dynamically_registered("ParticipantMobileAppV2"));
        assert!(!looks_dynamically_registered(""));
    }

    fn summary_row(rows: &[(String, String)], label: &str) -> Vec<String> {
        rows.iter()
            .filter(|(name, _)| name == label)
            .map(|(_, value)| value.clone())
            .collect()
    }

    fn a_client() -> Value {
        json!({
            "_id": "vKTest",
            "_rev": "283006219",
            "coreOAuth2ClientConfig": {
                "clientName": {"inherited": false, "value": ["vKTest"]},
                "clientType": {"inherited": false, "value": "Confidential"},
                "status": {"inherited": false, "value": "Active"},
                "scopes": {"inherited": false, "value": ["openid", "profile"]},
                "defaultScopes": {"inherited": false, "value": ["openid"]},
                "redirectionUris": {"inherited": false, "value": []},
                "accessTokenLifetime": {"inherited": false, "value": 0},
                "refreshTokenLifetime": {"inherited": false, "value": 0},
                "authorizationCodeLifetime": {"inherited": false, "value": 0},
                "userpassword": null
            },
            "advancedOAuth2ClientConfig": {
                "grantTypes": {"inherited": false, "value": ["client_credentials"]},
                "responseTypes": {"inherited": false, "value": ["token"]},
                "subjectType": {"inherited": false, "value": "public"},
                "tokenEndpointAuthMethod": {"inherited": false, "value": "client_secret_basic"},
                "isConsentImplied": {"inherited": false, "value": false}
            },
            "overrideOAuth2ClientConfig": {
                "providerOverridesEnabled": false,
                "validateScopeScript": "[Empty]",
                "validateScopePluginType": "PROVIDER",
                "validateScopeClass":
                    "org.forgerock.oauth2.core.plugins.registry.DefaultScopeValidator",
                "issueRefreshToken": true,
                "statelessTokensEnabled": false,
                "overrideableOIDCClaims": [],
                "remoteConsentServiceId": null
            }
        })
    }

    /// The fields the field report asked for, each unwrapped from its
    /// `{inherited, value}` envelope, and a repeated array field one row per
    /// value rather than a JSON blob.
    #[test]
    fn client_summary_answers_the_reconnaissance_questions() {
        let rows = client_summary(&a_client());

        assert_eq!(summary_row(&rows, "id"), ["vKTest"]);
        assert_eq!(summary_row(&rows, "clientType"), ["Confidential"]);
        assert_eq!(
            summary_row(&rows, "tokenEndpointAuthMethod"),
            ["client_secret_basic"]
        );
        assert_eq!(summary_row(&rows, "grantTypes"), ["client_credentials"]);
        assert_eq!(summary_row(&rows, "scopes"), ["openid", "profile"]);
        assert_eq!(summary_row(&rows, "defaultScopes"), ["openid"]);
        assert_eq!(summary_row(&rows, "redirectionUris"), ["<empty>"]);
    }

    /// A secret must not reach a summary even when the tenant sends one. The
    /// GET body returns `userpassword: null` today, but `-encrypted` siblings
    /// are documented as reachable, and a summary is the surface most likely
    /// to be pasted into a ticket.
    #[test]
    fn no_summary_row_can_carry_secret_material() {
        let mut client = a_client();
        client["coreOAuth2ClientConfig"]["userpassword"] = json!("plaintext-secret");
        client["coreOAuth2ClientConfig"]["userpassword-encrypted"] = json!("AQICwrapped");
        // The override block renders through the generic value path, not the
        // core allowlist, so a test that only injects into `core…` proves the
        // narrower claim that those fields are not listed — not that no secret
        // can reach a row.
        client["overrideOAuth2ClientConfig"]["something-encrypted"] = json!("AQICwrapped");

        let rows = client_summary(&client);

        for (label, value) in &rows {
            assert!(!value.contains("plaintext-secret"), "{label} = {value}");
            assert!(!value.contains("AQICwrapped"), "{label} = {value}");
            assert!(!label.contains("userpassword"), "{label}");
        }
    }

    /// The switch changes what the block *means*, so the row has to state the
    /// consequence rather than echo a boolean. Verified 2026-08-25 and
    /// recorded in `docs/api/05-oauth2-oidc.md`: with it false every field is
    /// inherited whatever it holds; with it true the whole block applies at
    /// once, its own defaults included.
    #[test]
    fn the_override_row_says_whether_the_block_runs_at_all() {
        let mut off = a_client();
        off["overrideOAuth2ClientConfig"]["providerOverridesEnabled"] = json!(false);
        let mut on = a_client();
        on["overrideOAuth2ClientConfig"]["providerOverridesEnabled"] = json!(true);

        let mut absent = a_client();
        absent["overrideOAuth2ClientConfig"]
            .as_object_mut()
            .unwrap()
            .remove("providerOverridesEnabled");

        let off = summary_row(&client_summary(&off), "overrideOAuth2ClientConfig");
        let on = summary_row(&client_summary(&on), "overrideOAuth2ClientConfig");
        let absent = summary_row(&client_summary(&absent), "overrideOAuth2ClientConfig");

        assert!(off[0].contains("dormant"), "{off:?}");
        assert!(off[0].contains("realm applies"), "{off:?}");
        assert!(on[0].contains("in effect"), "{on:?}");
        // An absent switch is not an enabled one: the doc's wording is that
        // `true` is *required*. Reading a missing key as "probably on" would
        // describe a client's live behaviour as the opposite of what it is.
        assert!(absent[0].contains("dormant"), "{absent:?}");
    }

    /// The switch can arrive inside an `{inherited, value}` envelope like any
    /// other field, and the block itself can too. Reading either raw reports
    /// the switch as absent — which, before the fix above, meant reporting a
    /// disabled block as enabled.
    #[test]
    fn the_switch_is_found_through_an_inherited_wrapper() {
        let wrapped = json!({
            "overrideOAuth2ClientConfig": {
                "inherited": false,
                "value": {
                    "providerOverridesEnabled": {"inherited": false, "value": true},
                    "validateScopeScript": "3482b626-5446-4734-a8a9-9a47e653de33"
                }
            }
        });

        let rows = client_summary(&wrapped);
        let header = summary_row(&rows, "overrideOAuth2ClientConfig");

        assert!(header[0].contains("in effect"), "{header:?}");
        assert_eq!(
            summary_row(&rows, "  validateScopeScript"),
            ["3482b626-5446-4734-a8a9-9a47e653de33"]
        );
    }

    /// A dormant block still shows what is in it.
    ///
    /// An earlier version hid every boolean while the switch was false, on the
    /// theory that an ignored value is not a setting. That is wrong for the
    /// question this command gets asked — "is it safe to enable this block?" —
    /// and it did worse than hide them: it counted them as "inherited or
    /// unset", which they are not. `statelessTokensEnabled` is the
    /// discriminating case, because flipping the switch is what turns it live,
    /// and the doc records it silently changing stateless JWTs into opaque
    /// tokens.
    #[test]
    fn a_dormant_block_still_shows_the_values_that_would_become_live() {
        let mut client = a_client();
        client["overrideOAuth2ClientConfig"]["providerOverridesEnabled"] = json!(false);
        client["overrideOAuth2ClientConfig"]["validateScopeScript"] =
            json!("3482b626-5446-4734-a8a9-9a47e653de33");

        let rows = client_summary(&client);

        assert_eq!(
            summary_row(&rows, "  validateScopeScript"),
            ["3482b626-5446-4734-a8a9-9a47e653de33"]
        );
        assert_eq!(summary_row(&rows, "  issueRefreshToken"), ["true"]);
        assert_eq!(summary_row(&rows, "  statelessTokensEnabled"), ["false"]);
        // The count is only ever the inherit-shaped entries now, in either
        // state — `[Empty]`, `PROVIDER`, the `Default…` class, `null`, `[]`.
        assert_eq!(
            summary_row(&rows, "  (not shown)"),
            ["4 inherited or unset — `--json` for the whole document"]
        );
    }

    /// With the block enabled, `statelessTokensEnabled: false` is a live
    /// setting — the doc records it silently turning stateless JWTs into
    /// opaque tokens — so a `false` must not be filtered out as "unset".
    #[test]
    fn an_enabled_override_block_shows_a_false_that_is_now_in_effect() {
        let mut client = a_client();
        client["overrideOAuth2ClientConfig"]["providerOverridesEnabled"] = json!(true);

        let rows = client_summary(&client);

        assert_eq!(summary_row(&rows, "  statelessTokensEnabled"), ["false"]);
        assert_eq!(summary_row(&rows, "  issueRefreshToken"), ["true"]);
    }

    /// The five ways an override entry says "inherit". Each is suppressed and
    /// each is counted; the count is what keeps the suppression honest.
    #[test]
    fn every_inherit_shaped_override_entry_is_suppressed_and_counted() {
        let mut client = a_client();
        client["overrideOAuth2ClientConfig"] = json!({
            "providerOverridesEnabled": true,
            "validateScopeScript": "[Empty]",
            "validateScopePluginType": "PROVIDER",
            "validateScopeClass":
                "org.forgerock.oauth2.core.plugins.registry.DefaultScopeValidator",
            "overrideableOIDCClaims": [],
            "remoteConsentServiceId": null
        });

        let rows = client_summary(&client);

        assert!(
            summary_row(&rows, "  validateScopeScript").is_empty(),
            "{rows:?}"
        );
        assert_eq!(
            summary_row(&rows, "  (not shown)"),
            ["5 inherited or unset — `--json` for the whole document"]
        );
    }

    /// A non-default plugin class is a real override and must survive the
    /// `Default…` suppression, which would otherwise hide the one field that
    /// says the client is not using AM's implementation.
    #[test]
    fn a_custom_plugin_class_is_not_mistaken_for_the_default_one() {
        let mut client = a_client();
        client["overrideOAuth2ClientConfig"]["providerOverridesEnabled"] = json!(true);
        client["overrideOAuth2ClientConfig"]["validateScopeClass"] = json!("com.example.MyScopes");
        client["overrideOAuth2ClientConfig"]["validateScopePluginType"] = json!("SCRIPTED");

        let rows = client_summary(&client);

        assert_eq!(
            summary_row(&rows, "  validateScopeClass"),
            ["com.example.MyScopes"]
        );
        assert_eq!(
            summary_row(&rows, "  validateScopePluginType"),
            ["SCRIPTED"]
        );
    }

    /// A group the tenant does not send must read as absent rather than
    /// vanish: a summary silently missing `grantTypes` is the one that gets
    /// misread as "this client has no grants".
    #[test]
    fn a_missing_group_is_named_not_dropped() {
        let rows = client_summary(&json!({"_id": "bare"}));

        assert_eq!(summary_row(&rows, "grantTypes"), ["<group absent>"]);
        assert_eq!(
            summary_row(&rows, "overrideOAuth2ClientConfig"),
            ["<absent>"]
        );
    }

    /// The exchange projection, as the collection endpoint returns it:
    /// nested values unwrapped, absent fields simply absent.
    fn exchange_projection(id: &str, grants: &[&str], overrides: Value) -> Value {
        json!({
            "_id": id,
            "advancedOAuth2ClientConfig": {
                "grantTypes": grants,
                "tokenExchangeAuthLevel": 0,
            },
            "overrideOAuth2ClientConfig": overrides,
        })
    }

    const EXCHANGE: &str = "urn:ietf:params:oauth:grant-type:token-exchange";

    /// A realm that grants the type and configures one exchanger, so a test
    /// varies only the thing it is about.
    fn realm(may_act: Option<&str>) -> RealmExchange {
        RealmExchange {
            granted: Some(true),
            may_act_script: may_act.map(ToOwned::to_owned),
            exchangers: vec!["a => b".to_string()],
            ..RealmExchange::default()
        }
    }

    #[test]
    fn an_actor_is_a_client_that_holds_the_grant() {
        let actor = exchange_row(&exchange_projection(
            "caller",
            &[EXCHANGE],
            json!({ "providerOverridesEnabled": true }),
        ));
        let bystander = exchange_row(&exchange_projection(
            "web",
            &["authorization_code"],
            json!({ "providerOverridesEnabled": true }),
        ));

        assert_eq!(actor.actor, Some(true), "the grant holder is the actor");
        assert!(actor.participates(Some(false)));
        assert_eq!(bystander.actor, Some(false));
        assert!(
            !bystander.participates(Some(false)),
            "a client with neither the grant nor a may-act script is not in the exchange"
        );
    }

    /// The discriminating case for the whole subject column. AM writes
    /// `[Empty]` into an unset script field far more often than it omits the
    /// key, so a reader that only tested for the key's *presence* would call
    /// every client in the realm a subject.
    #[test]
    fn the_empty_sentinel_is_not_a_may_act_script() {
        let row = exchange_row(&exchange_projection(
            "web",
            &["authorization_code"],
            json!({
                "providerOverridesEnabled": true,
                "accessTokenMayActScript": "[Empty]",
                "oidcMayActScript": "[Empty]",
            }),
        ));

        assert!(row.may_act.is_empty());
        assert!(!row.participates(Some(false)));
        assert!(
            !row.may_act_dormant(),
            "nothing is configured, so nothing is dormant"
        );
    }

    #[test]
    fn a_may_act_script_under_a_switch_that_is_not_true_is_dormant() {
        let live = exchange_row(&exchange_projection(
            "live",
            &[],
            json!({
                "providerOverridesEnabled": true,
                "accessTokenMayActScript": "script-1",
            }),
        ));
        let off = exchange_row(&exchange_projection(
            "off",
            &[],
            json!({
                "providerOverridesEnabled": false,
                "accessTokenMayActScript": "script-1",
            }),
        ));
        // The discriminating case: an *absent* switch is not an enabled one,
        // and reading `None` as "probably on" is the direction that misleads.
        let absent = exchange_row(&exchange_projection(
            "absent",
            &[],
            json!({ "accessTokenMayActScript": "script-1" }),
        ));

        assert!(!live.may_act_dormant());
        assert!(off.may_act_dormant());
        assert!(absent.may_act_dormant());
        for row in [&live, &off, &absent] {
            assert_eq!(
                row.may_act,
                vec![(
                    "accessTokenMayActScript".to_string(),
                    "script-1".to_string()
                )],
                "dormant changes whether it runs, never whether it is configured"
            );
        }
    }

    /// The projection is measured raw, but a wrapped value must not silently
    /// become an empty column — the same guard `client_row` carries.
    ///
    /// It asserts **every** field the row reads, not the convenient ones. The
    /// earlier version skipped both audience readers, and so passed while
    /// `acceptAudienceParametersInTokenExchangeRequests` was being projected
    /// from `advancedOAuth2ClientConfig`, where the live client schema says it
    /// does not exist — a column that could never be anything but absent.
    #[test]
    fn the_exchange_projection_survives_an_inherited_wrapper() {
        let row = exchange_row(&json!({
            "_id": "wrapped",
            "advancedOAuth2ClientConfig": {
                "inherited": false,
                "value": {
                    "grantTypes": { "inherited": false, "value": [EXCHANGE] },
                    "tokenExchangeAuthLevel": { "inherited": false, "value": 7 },
                    "allowedResourceServerAudienceValues": {
                        "inherited": false,
                        "value": ["https://sp-a.example.com"],
                    },
                },
            },
            "overrideOAuth2ClientConfig": {
                "inherited": false,
                "value": {
                    "providerOverridesEnabled": { "inherited": false, "value": true },
                    "accessTokenMayActScript": { "inherited": false, "value": "script-1" },
                    "acceptAudienceParametersInTokenExchangeRequests": {
                        "inherited": false,
                        "value": true,
                    },
                },
            },
        }));

        assert_eq!(row.actor, Some(true));
        assert_eq!(row.auth_level, Some(7));
        assert_eq!(row.may_act.len(), 1);
        assert!(!row.may_act_dormant());
        assert_eq!(row.audience_values, ["https://sp-a.example.com"]);
        assert_eq!(row.accept_audience, Some(true));
        assert_eq!(row.accept_audience_override(), Some(true));
    }

    /// `acceptAudienceParametersInTokenExchangeRequests` lives in the override
    /// block, so the master switch decides whether it applies. The
    /// discriminating pair: the same configured `true` is effective under a
    /// live switch and inert under a dormant one.
    #[test]
    fn the_client_audience_switch_is_governed_by_the_master_switch() {
        let configured = json!({
            "acceptAudienceParametersInTokenExchangeRequests": true,
            "accessTokenMayActScript": "script-1",
        });
        let mut live = configured.clone();
        live["providerOverridesEnabled"] = json!(true);

        let live = exchange_row(&exchange_projection("live", &[], live));
        let dormant = exchange_row(&exchange_projection("dormant", &[], configured));

        assert_eq!(live.accept_audience, Some(true));
        assert_eq!(live.accept_audience_override(), Some(true));
        assert_eq!(
            dormant.accept_audience,
            Some(true),
            "still configured, and still worth showing"
        );
        assert_eq!(
            dormant.accept_audience_override(),
            None,
            "but the realm's value is what applies"
        );
    }

    /// A non-string in the audience list must stay visible. Filtering to
    /// strings shortened the list a reader uses to decide whether an
    /// `audience` can be asked for at all.
    #[test]
    fn a_non_string_audience_value_is_shown_rather_than_dropped() {
        let row = exchange_row(&json!({
            "_id": "mixed",
            "advancedOAuth2ClientConfig": {
                "allowedResourceServerAudienceValues": ["https://sp-a.example.com", 7],
            },
        }));

        assert_eq!(row.audience_values, ["https://sp-a.example.com", "7"]);
    }

    /// The dormant finding has to say what happens *instead*, and that differs:
    /// with no realm script the client's tokens cannot be exchanged at all.
    /// The first version asserted the realm's script runs, unconditionally.
    #[test]
    fn a_dormant_override_reports_the_realm_fallback_only_when_there_is_one() {
        let rows = vec![exchange_row(&exchange_projection(
            "web",
            &[],
            json!({
                "providerOverridesEnabled": false,
                "accessTokenMayActScript": "script-1",
            }),
        ))];

        let without = exchange_findings(&realm(None), &rows);
        let with = exchange_findings(&realm(Some("realm-script")), &rows);

        assert!(
            without.iter().any(|f| f.contains("the realm sets none")),
            "{without:?}"
        );
        assert!(
            with.iter()
                .any(|f| f.contains("the realm's realm-script runs instead")),
            "{with:?}"
        );
    }

    /// The case the first two attempts both got wrong, in opposite ways. A
    /// **live** override block that sets no script is an explicit "no
    /// script": `providerOverridesEnabled: true` stops inheritance for the
    /// whole block, so the realm's script does not apply either. Falling
    /// through to the realm here labelled a client that stamps nothing as a
    /// subject, and suppressed the warning that says so.
    #[test]
    fn a_live_override_that_sets_no_script_does_not_fall_back_to_the_realm() {
        let live_but_empty = exchange_row(&exchange_projection(
            "empty",
            &[EXCHANGE],
            json!({
                "providerOverridesEnabled": true,
                "accessTokenMayActScript": "[Empty]",
                "oidcMayActScript": "[Empty]",
            }),
        ));
        // The neighbour that separates "live and empty" from "not live":
        // only the second inherits.
        let dormant = exchange_row(&exchange_projection(
            "dormant",
            &[EXCHANGE],
            json!({ "providerOverridesEnabled": false }),
        ));

        assert_eq!(
            may_act_source(&live_but_empty, Some(true)),
            MayActSource::None
        );
        assert_eq!(may_act_source(&dormant, Some(true)), MayActSource::Realm);

        let findings = exchange_findings(&realm(Some("realm-script")), &[live_but_empty]);
        assert!(
            findings
                .iter()
                .any(|f| f.contains("nothing stamps may_act")),
            "{findings:?}"
        );
    }

    /// A field that is *present* with the wrong type is not the same as an
    /// absent one, and this view's answers are the reason: a `grantTypes` that
    /// stopped being an array turns an actor into a bystander. Absent must
    /// stay quiet, or every ordinary client reports faults.
    #[test]
    fn a_malformed_field_is_reported_while_an_absent_one_is_not() {
        let malformed = exchange_row(&json!({
            "_id": "broken",
            "advancedOAuth2ClientConfig": {
                "grantTypes": "urn:ietf:params:oauth:grant-type:token-exchange",
                "tokenExchangeAuthLevel": "high",
                "allowedResourceServerAudienceValues": ["https://sp-a.example.com", 7],
            },
            "overrideOAuth2ClientConfig": { "providerOverridesEnabled": "true" },
        }));
        let absent = exchange_row(&json!({ "_id": "bare" }));

        assert_eq!(malformed.actor, None, "a string grant list cannot be read");
        assert_eq!(malformed.auth_level, None);
        assert_eq!(malformed.overrides_live, None);
        assert_eq!(
            malformed.shape_faults,
            [
                "advancedOAuth2ClientConfig.grantTypes is a string, expected an array",
                "overrideOAuth2ClientConfig.providerOverridesEnabled is a string, expected a boolean",
                "advancedOAuth2ClientConfig.tokenExchangeAuthLevel is a string, expected a number",
                "advancedOAuth2ClientConfig.allowedResourceServerAudienceValues[] is a number, expected a string",
            ]
        );
        assert!(
            absent.shape_faults.is_empty(),
            "an absent field is not a malformed one: {:?}",
            absent.shape_faults
        );

        let findings = exchange_findings(&realm(None), &[malformed]);
        assert!(
            findings
                .iter()
                .any(|f| f.starts_with("broken: advancedOAuth2ClientConfig.grantTypes is a string")),
            "{findings:?}"
        );
    }

    /// A warning underneath a wrong sentence does not make the sentence
    /// right. The discriminating assertion is the **absence** of the
    /// categorical finding: with the only client's grant list unreadable, the
    /// command must not say "no client holds the grant", because that client
    /// may well hold it.
    #[test]
    fn a_malformed_grant_list_withholds_the_no_actor_conclusion() {
        let unreadable = exchange_row(&json!({
            "_id": "broken",
            "advancedOAuth2ClientConfig": { "grantTypes": "not-an-array" },
        }));
        let readable = exchange_row(&exchange_projection("plain", &["password"], json!({})));

        assert_eq!(unreadable.actor, None);
        let withheld = exchange_findings(&realm(Some("realm-script")), &[unreadable]);
        assert!(
            !withheld
                .iter()
                .any(|f| f.contains("no client holds the grant")),
            "{withheld:?}"
        );
        // ...and it is still stated when every grant list *was* readable.
        let stated = exchange_findings(&realm(Some("realm-script")), &[readable]);
        assert!(
            stated
                .iter()
                .any(|f| f.contains("no client holds the grant")),
            "{stated:?}"
        );
    }

    /// The same rule one level down: an unreadable master switch makes the
    /// may-act source unknown, not inherited. Publishing `realm` there is a
    /// confident answer derived from a substituted `false`.
    #[test]
    fn an_unreadable_master_switch_makes_the_source_unknown() {
        let row = exchange_row(&exchange_projection(
            "broken",
            &[EXCHANGE],
            json!({ "providerOverridesEnabled": "true" }),
        ));

        assert_eq!(row.overrides_live, None);
        assert_eq!(may_act_source(&row, Some(true)), MayActSource::Unknown);
        assert_eq!(may_act_source(&row, Some(false)), MayActSource::Unknown);
        // An unknown subject is not a "nothing stamps may_act" realm either.
        let findings = exchange_findings(&realm(None), &[row]);
        assert!(
            !findings
                .iter()
                .any(|f| f.contains("nothing stamps may_act")),
            "{findings:?}"
        );
    }

    /// A may-act field holding something that is not a script id is not a
    /// configured script. Stringifying it selected `MayActSource::Client` and
    /// suppressed the deny-by-default warning for a client that stamps
    /// nothing.
    #[test]
    fn a_may_act_value_that_is_not_a_string_is_a_fault_not_a_script() {
        let row = exchange_row(&exchange_projection(
            "odd",
            &[EXCHANGE],
            json!({
                "providerOverridesEnabled": true,
                "accessTokenMayActScript": true,
            }),
        ));

        assert!(row.may_act.is_empty());
        assert_eq!(may_act_source(&row, Some(true)), MayActSource::None);
        assert_eq!(
            row.shape_faults,
            ["overrideOAuth2ClientConfig.accessTokenMayActScript is a boolean, expected a string"]
        );
    }

    /// The realm document gets the same treatment, and being tenant-wide it
    /// matters more, not less: a `false` substituted for an unreadable grant
    /// list would report every actor in the realm as broken.
    #[test]
    fn an_unreadable_realm_grant_list_is_unknown_rather_than_ungranted() {
        let realm = realm_exchange(&json!({
            "advancedOAuth2Config": { "grantTypes": "not-an-array" },
        }));
        let actors = vec![exchange_row(&exchange_projection(
            "caller",
            &[EXCHANGE],
            json!({}),
        ))];

        assert_eq!(realm.granted, None);
        assert_eq!(
            realm.shape_faults,
            ["advancedOAuth2Config.grantTypes is a string, expected an array"]
        );
        let findings = exchange_findings(&realm, &actors);
        assert!(
            !findings.iter().any(|f| f.contains("does not grant")),
            "{findings:?}"
        );
        assert!(
            findings.iter().any(|f| f.starts_with("realm: ")),
            "{findings:?}"
        );
    }

    /// Acting and stamping are separate roles held by **different** clients:
    /// the subject whose tokens an actor exchanges is, by construction, not
    /// the actor. Restricting the population to actors removed exactly the
    /// clients that could disprove the claim.
    #[test]
    fn a_non_actor_subject_counts_against_the_deny_by_default_finding() {
        let rows = vec![
            exchange_row(&exchange_projection("caller", &[EXCHANGE], json!({}))),
            // Not an actor, and the only thing stamping may_act.
            exchange_row(&exchange_projection(
                "web",
                &["password"],
                json!({
                    "providerOverridesEnabled": true,
                    "accessTokenMayActScript": "script-1",
                }),
            )),
        ];

        let findings = exchange_findings(&realm(None), &rows);

        assert!(
            !findings
                .iter()
                .any(|f| f.contains("nothing stamps may_act")),
            "the subject is a different client from the actor: {findings:?}"
        );
        // ...and with that client gone, the realm genuinely cannot exchange.
        let alone = exchange_findings(&realm(None), &rows[..1]);
        assert!(
            alone.iter().any(|f| f.contains("nothing stamps may_act")),
            "{alone:?}"
        );
    }

    /// The realm's own readability, field by field. An empty exchanger list
    /// that could not be read is not "none configured", and a may-act script
    /// that could not be read is not "not set".
    #[test]
    fn an_unreadable_realm_field_withholds_the_finding_it_feeds() {
        let unreadable = realm_exchange(&json!({
            "advancedOAuth2Config": {
                "grantTypes": ["urn:ietf:params:oauth:grant-type:token-exchange"],
                "tokenExchangeClasses": "not-an-array",
            },
            "coreOAuth2Config": { "accessTokenMayActScript": 7 },
        }));

        assert!(!unreadable.exchangers_readable);
        assert!(!unreadable.may_act_readable);
        assert_eq!(unreadable.stamps(), None);

        let findings = exchange_findings(&unreadable, &[]);
        assert!(
            !findings
                .iter()
                .any(|f| f.contains("no tokenExchangeClasses")),
            "{findings:?}"
        );
        assert!(
            !findings
                .iter()
                .any(|f| f.contains("nothing stamps may_act")),
            "{findings:?}"
        );

        // The discriminating neighbour: genuinely empty, and it is stated.
        let empty = realm_exchange(&json!({
            "advancedOAuth2Config": {
                "grantTypes": ["urn:ietf:params:oauth:grant-type:token-exchange"],
            },
        }));
        assert!(empty.exchangers_readable);
        assert!(
            exchange_findings(&empty, &[])
                .iter()
                .any(|f| f.contains("no tokenExchangeClasses")),
        );
    }

    /// A group that is *present and not an object* makes every field under it
    /// unreadable. `projected` cannot tell that from the group being absent,
    /// which produced a confident `Some(false)` with no fault at all.
    #[test]
    fn a_config_group_that_is_not_an_object_makes_its_fields_unknown() {
        let row = exchange_row(&json!({
            "_id": "broken",
            "advancedOAuth2ClientConfig": "not-an-object",
            "overrideOAuth2ClientConfig": 7,
        }));

        assert_eq!(row.actor, None);
        assert_eq!(row.overrides_live, None);
        assert_eq!(may_act_source(&row, Some(true)), MayActSource::Unknown);
        assert_eq!(row.shape_faults.len(), 2, "{:?}", row.shape_faults);
    }

    /// A grant list whose unreadable element might have *been* the grant
    /// cannot answer "no".
    #[test]
    fn an_unreadable_element_stops_a_grant_list_saying_no() {
        let maybe = exchange_row(&json!({
            "_id": "maybe",
            "advancedOAuth2ClientConfig": { "grantTypes": ["password", 7] },
        }));
        let definitely = exchange_row(&json!({
            "_id": "definitely",
            "advancedOAuth2ClientConfig": { "grantTypes": [EXCHANGE, 7] },
        }));

        assert_eq!(maybe.actor, None, "the unreadable element may be the grant");
        assert_eq!(
            definitely.actor,
            Some(true),
            "present is present, whatever else the list holds"
        );
    }

    /// An unreadable realm may-act script has to reach the row, not stop at
    /// the call site. Collapsing it to "does not stamp" published `none` for a
    /// client whose answer was simply not known — the same failure the enum
    /// exists to prevent, one level further out.
    #[test]
    fn an_unreadable_realm_script_makes_an_inheriting_client_unknown() {
        let inheriting = exchange_row(&exchange_projection(
            "web",
            &["password"],
            json!({ "providerOverridesEnabled": false }),
        ));
        let overriding = exchange_row(&exchange_projection(
            "own",
            &["password"],
            json!({
                "providerOverridesEnabled": true,
                "accessTokenMayActScript": "script-1",
            }),
        ));

        assert_eq!(may_act_source(&inheriting, None), MayActSource::Unknown);
        // A live block answers for itself whatever the realm holds, so it is
        // the case that must NOT become unknown.
        assert_eq!(may_act_source(&overriding, None), MayActSource::Client);
        // ...and a client that inherits is only shown as a subject when the
        // realm is known to stamp.
        assert_eq!(may_act_source(&inheriting, Some(true)), MayActSource::Realm);
        assert_eq!(may_act_source(&inheriting, Some(false)), MayActSource::None);

        // A live block is an exception to any script the realm *might* have,
        // so an unknown realm keeps it visible.
        let live_empty = exchange_row(&exchange_projection(
            "empty",
            &["password"],
            json!({ "providerOverridesEnabled": true }),
        ));
        assert!(live_empty.participates(None));
        assert!(!live_empty.participates(Some(false)));
    }

    /// The realm's own arrays get the client rule: a non-string element makes
    /// a negative answer unknown, and an exchanger that is not a string is not
    /// an exchanger.
    #[test]
    fn a_non_string_element_in_a_realm_array_is_not_configuration() {
        let realm = realm_exchange(&json!({
            "advancedOAuth2Config": {
                "grantTypes": ["authorization_code", 7],
                "tokenExchangeClasses": [7],
            },
        }));

        assert_eq!(
            realm.granted, None,
            "the unreadable element may be the grant"
        );
        assert!(realm.exchangers.is_empty(), "{:?}", realm.exchangers);
        assert!(!realm.exchangers_readable);
        // Neither the "does not grant" nor the "no exchangers" finding may be
        // stated from an array nobody could read.
        let actors = vec![exchange_row(&exchange_projection(
            "caller",
            &[EXCHANGE],
            json!({}),
        ))];
        let findings = exchange_findings(&realm, &actors);
        assert!(
            !findings.iter().any(|f| f.contains("does not grant")),
            "{findings:?}"
        );
        assert!(
            !findings
                .iter()
                .any(|f| f.contains("no tokenExchangeClasses")),
            "{findings:?}"
        );
    }

    /// A present-but-unreadable client audience switch is not an absent one,
    /// so it must not read as the block's default.
    #[test]
    fn an_unreadable_client_audience_switch_is_not_an_absent_one() {
        let unreadable = exchange_row(&exchange_projection(
            "broken",
            &["password"],
            json!({
                "providerOverridesEnabled": true,
                "acceptAudienceParametersInTokenExchangeRequests": "true",
            }),
        ));
        let absent = exchange_row(&exchange_projection(
            "plain",
            &["password"],
            json!({ "providerOverridesEnabled": true }),
        ));

        assert!(!unreadable.accept_audience_readable);
        assert!(absent.accept_audience_readable);
        assert_eq!(unreadable.accept_audience, None);
        assert_eq!(absent.accept_audience, None);
    }

    /// A live override block is an **exception** to a realm-wide script, so
    /// hiding it hides the one client the realm header does not describe. The
    /// discriminating pair is the realm: the same client is worth listing when
    /// the realm stamps and not when it does not.
    #[test]
    fn a_live_block_is_kept_when_it_overrides_a_realm_wide_script() {
        let exception = exchange_row(&exchange_projection(
            "exception",
            &["password"],
            json!({
                "providerOverridesEnabled": true,
                "accessTokenMayActScript": "[Empty]",
            }),
        ));

        assert!(
            exception.participates(Some(true)),
            "it stops the realm's script stamping for this client"
        );
        assert!(
            !exception.participates(Some(false)),
            "with no realm script it is an exception to nothing"
        );
    }

    /// A client inheriting a live realm script IS a subject, so the
    /// deny-by-default finding must not fire. The earlier version counted only
    /// client overrides and reported a working realm as broken.
    #[test]
    fn a_realm_script_satisfies_may_act_for_a_client_with_no_override() {
        let rows = vec![exchange_row(&exchange_projection(
            "caller",
            &[EXCHANGE],
            json!({}),
        ))];

        let findings = exchange_findings(&realm(Some("realm-script")), &rows);

        assert!(
            !findings
                .iter()
                .any(|f| f.contains("nothing stamps may_act")),
            "{findings:?}"
        );
        assert_eq!(may_act_source(&rows[0], Some(true)), MayActSource::Realm);
        assert_eq!(may_act_source(&rows[0], Some(false)), MayActSource::None);
    }

    #[test]
    fn a_realm_that_does_not_grant_the_type_is_reported_against_its_actors() {
        let actors = vec![exchange_row(&exchange_projection(
            "caller",
            &[EXCHANGE],
            json!({}),
        ))];

        let findings = exchange_findings(
            &RealmExchange {
                granted: Some(false),
                ..realm(None)
            },
            &actors,
        );

        assert!(
            findings
                .iter()
                .any(|finding| finding.contains("unsupported_grant_type")),
            "{findings:?}"
        );
    }

    /// D6: an exchange is refused unless something stamps `may_act`, and the
    /// error names none of it. The discriminating case is the realm-level
    /// script — a reader that only looked at clients would report a broken
    /// realm that works.
    #[test]
    fn nothing_stamping_may_act_is_reported_unless_the_realm_stamps_it() {
        let actors = vec![exchange_row(&exchange_projection(
            "caller",
            &[EXCHANGE],
            json!({}),
        ))];
        let refused = |script| {
            exchange_findings(
                &RealmExchange {
                    exchangers: Vec::new(),
                    ..realm(script)
                },
                &actors,
            )
        };

        assert!(
            refused(None)
                .iter()
                .any(|finding| finding.contains("nothing stamps may_act")),
            "no subject anywhere"
        );
        assert!(
            !refused(Some("script-1"))
                .iter()
                .any(|finding| finding.contains("nothing stamps may_act")),
            "the realm script is what stamps it when no client overrides"
        );
    }

    /// A dormant subject is not a live one: it must still leave the
    /// deny-by-default finding standing, or a realm that cannot exchange
    /// anything reads as configured.
    #[test]
    fn a_dormant_subject_does_not_satisfy_the_may_act_requirement() {
        let rows = vec![
            exchange_row(&exchange_projection("caller", &[EXCHANGE], json!({}))),
            exchange_row(&exchange_projection(
                "web",
                &[],
                json!({
                    "providerOverridesEnabled": false,
                    "accessTokenMayActScript": "script-1",
                }),
            )),
        ];

        let findings = exchange_findings(&realm(None), &rows);

        assert!(
            findings
                .iter()
                .any(|finding| finding.contains("nothing stamps may_act")),
            "{findings:?}"
        );
        assert!(
            findings
                .iter()
                .any(|finding| finding.starts_with("web: may-act script set")),
            "{findings:?}"
        );
    }

    #[test]
    fn a_granting_realm_with_no_actor_and_no_exchanger_says_both() {
        let findings = exchange_findings(
            &RealmExchange {
                exchangers: Vec::new(),
                ..realm(Some("script-1"))
            },
            &[],
        );

        assert!(
            findings
                .iter()
                .any(|f| f.contains("no client holds the grant")),
            "{findings:?}"
        );
        assert!(
            findings
                .iter()
                .any(|f| f.contains("no tokenExchangeClasses")),
            "{findings:?}"
        );
    }

    #[test]
    fn the_realm_may_act_script_reads_through_the_empty_sentinel() {
        let unset = json!({ "coreOAuth2Config": { "accessTokenMayActScript": "[Empty]" } });
        let set = json!({ "coreOAuth2Config": { "accessTokenMayActScript": "script-1" } });

        assert_eq!(realm_exchange(&unset).may_act_script, None);
        assert_eq!(
            realm_exchange(&set).may_act_script,
            Some("script-1".to_string())
        );
        assert_eq!(realm_exchange(&json!({})).may_act_script, None);
    }

    #[test]
    fn provider_summary_shows_token_exchange_grant_and_exchanger_together() {
        let provider = json!({
            "advancedOAuth2Config": {
                "grantTypes": ["authorization_code", "refresh_token"],
                "tokenExchangeClasses": [
                    "org.example.AccessTokenToAccessToken",
                    "org.example.IdTokenToIdToken"
                ]
            }
        });

        let rows = provider_summary(&provider);

        assert_eq!(
            summary_values(&rows, "  grantTypes"),
            ["authorization_code", "refresh_token"]
        );
        assert_eq!(
            summary_values(&rows, "  tokenExchangeClasses"),
            [
                "org.example.AccessTokenToAccessToken",
                "org.example.IdTokenToIdToken"
            ]
        );
    }

    #[test]
    fn provider_summary_shortens_structured_exchangers_and_preserves_malformed_values() {
        let structured = concat!(
            "urn:ietf:params:oauth:token-type:access_token=>",
            "urn:ietf:params:oauth:token-type:id_token|",
            "org.forgerock.oauth2.core.tokenexchange.accesstoken.",
            "AccessTokenToIdTokenExchanger"
        );
        let malformed = "custom-exchanger-without-delimiters";
        let rows = provider_summary(&json!({
            "advancedOAuth2Config": {
                "tokenExchangeClasses": [structured, malformed]
            }
        }));

        assert_eq!(
            summary_values(&rows, "  tokenExchangeClasses"),
            [
                "access_token => id_token (AccessTokenToIdTokenExchanger)",
                malformed
            ]
        );
    }

    /// The rewrite is structural, not semantic. A mapping naming a token type
    /// or an exchanger class nobody here has seen still shortens, and that is
    /// deliberate — pinned so introducing an allow-list is a decision someone
    /// makes rather than a behaviour that drifts in.
    #[test]
    fn provider_summary_shortens_unfamiliar_token_types_and_classes() {
        let unfamiliar = concat!(
            "urn:ietf:params:oauth:token-type:bogus=>",
            "urn:ietf:params:oauth:token-type:other|",
            "com.example.MysteryExchanger"
        );
        let no_package = concat!(
            "urn:ietf:params:oauth:token-type:access_token=>",
            "urn:ietf:params:oauth:token-type:id_token|",
            ".Foo"
        );
        let rows = provider_summary(&json!({
            "advancedOAuth2Config": {
                "tokenExchangeClasses": [unfamiliar, no_package]
            }
        }));

        assert_eq!(
            summary_values(&rows, "  tokenExchangeClasses"),
            [
                "bogus => other (MysteryExchanger)",
                "access_token => id_token (Foo)"
            ]
        );
    }

    /// Anything that does not parse must reach the reader whole. A partially
    /// rewritten exchanger is worse than a long one: it reads as a configured
    /// mapping while naming something the tenant does not have.
    #[test]
    fn provider_summary_prints_unparseable_exchangers_in_full() {
        const P: &str = "urn:ietf:params:oauth:token-type:";
        let cases = [
            // no package, so there is no final dot to split the class on
            format!("{P}access_token=>{P}id_token|ExchangerWithoutPackage"),
            // trailing dot leaves the class name empty
            format!("{P}access_token=>{P}id_token|org.example."),
            // a second arrow: which half is the target token type?
            format!("{P}access_token=>{P}id_token=>{P}refresh_token|org.example.C"),
            // a second pipe: which half is the class?
            format!("{P}access_token=>{P}id_token|org.example.C|extra"),
            // target token type is not URN-prefixed, so stripping would lie
            format!("{P}access_token=>id_token|org.example.C"),
            // source is the bare prefix and therefore names nothing
            format!("{P}=>{P}id_token|org.example.C"),
        ];

        for case in cases {
            let rows = provider_summary(&json!({
                "advancedOAuth2Config": {
                    "tokenExchangeClasses": [case.clone()]
                }
            }));

            assert_eq!(
                summary_values(&rows, "  tokenExchangeClasses"),
                [case.as_str()],
                "should have been printed in full: {case}"
            );
        }
    }

    #[test]
    fn provider_summary_reads_grants_from_advanced_group_not_populated_core_group() {
        let token_exchange = "urn:ietf:params:oauth:grant-type:token-exchange";
        let provider = json!({
            "coreOAuth2Config": {
                "accessTokenLifetime": 900,
                "accessTokenMayActScript": "may-act-script-id"
            },
            "advancedOAuth2Config": {
                "grantTypes": [token_exchange],
                "tokenExchangeClasses": ["org.example.AccessTokenToAccessToken"]
            }
        });

        let rows = provider_summary(&provider);

        assert_eq!(summary_value(&rows, "  grantTypes"), token_exchange);
    }

    #[test]
    fn provider_summary_derives_token_exchange_granted_from_the_grant_list() {
        let granting = provider_summary(&json!({
            "advancedOAuth2Config": {
                "grantTypes": [TOKEN_EXCHANGE_GRANT]
            }
        }));
        let not_granting = provider_summary(&json!({
            "advancedOAuth2Config": {
                "grantTypes": ["authorization_code"]
            }
        }));

        assert_eq!(summary_value(&granting, "  token-exchange granted"), "yes");
        assert_eq!(
            summary_value(&not_granting, "  token-exchange granted"),
            "no"
        );
    }

    #[test]
    fn provider_summary_unwraps_inherited_fields_like_bare_fields() {
        let wrapped = json!({
            "advancedOAuth2Config": {
                "grantTypes": {"inherited": false, "value": ["client_credentials"]},
                "tokenExchangeClasses": {"inherited": true, "value": ["exchanger"]}
            }
        });
        let bare = json!({
            "advancedOAuth2Config": {
                "grantTypes": ["client_credentials"],
                "tokenExchangeClasses": ["exchanger"]
            }
        });

        assert_eq!(provider_summary(&wrapped), provider_summary(&bare));
        assert!(!summary_value(&provider_summary(&wrapped), "  grantTypes").contains("inherited"));
    }

    #[test]
    fn provider_summary_distinguishes_absent_empty_and_configured_groups() {
        let absent = provider_summary(&json!({}));
        let empty = provider_summary(&json!({"pluginsConfig": {}}));
        let configured = provider_summary(&json!({
            "pluginsConfig": {
                "scope": "scripted",
                "scripts": ["script-one", "script-two"]
            }
        }));

        assert_eq!(summary_value(&absent, "pluginsConfig"), "<absent>");
        assert_eq!(summary_value(&empty, "pluginsConfig"), "<empty>");
        assert_eq!(summary_value(&configured, "pluginsConfig"), "<configured>");
        assert_eq!(summary_value(&configured, "  scope"), "scripted");
        assert_eq!(
            summary_values(&configured, "  scripts"),
            ["script-one", "script-two"]
        );

        let dcr = provider_summary(&json!({
            "clientDynamicRegistrationConfig": {"enabled": true},
            "aiAgentsConfig": {"aiAgentsEnabled": false}
        }));
        for group in PROVIDER_GROUPS {
            assert!(
                dcr.iter().any(|(field, _)| field == group),
                "missing state row for {group}"
            );
        }
        assert_eq!(
            summary_value(&dcr, "clientDynamicRegistrationConfig"),
            "<configured>"
        );
        assert_eq!(summary_value(&dcr, "aiAgentsConfig"), "<configured>");
        assert!(
            !dcr.iter()
                .any(|(field, _)| field == "unknown.aiAgentsConfig")
        );
    }

    #[test]
    fn provider_summary_distinguishes_absent_unset_sentinel_and_real_script() {
        let absent = provider_summary(&json!({"coreOAuth2Config": {}}));
        let unset = provider_summary(&json!({
            "coreOAuth2Config": {"accessTokenMayActScript": "[Empty]"}
        }));
        let configured = provider_summary(&json!({
            "coreOAuth2Config": {"accessTokenMayActScript": "script-id"}
        }));

        assert_eq!(
            summary_value(&absent, "  accessTokenMayActScript"),
            "<absent>"
        );
        assert_eq!(
            summary_value(&unset, "  accessTokenMayActScript"),
            "<not set>"
        );
        assert_eq!(
            summary_value(&configured, "  accessTokenMayActScript"),
            "script-id"
        );
    }

    #[test]
    fn provider_summary_includes_unknown_top_level_groups() {
        let rows = provider_summary(&json!({
            "advancedOAuth2Config": {},
            "futureProviderConfig": {"enabled": true}
        }));

        assert_eq!(
            summary_value(&rows, "unknown.futureProviderConfig"),
            "{\"enabled\":true}"
        );
    }

    fn template() -> Value {
        json!({
            "overrideOAuth2ClientConfig": {"providerOverridesEnabled": false},
            "advancedOAuth2ClientConfig": {
                "descriptions": [],
                "grantTypes": [],
                "responseTypes": [],
                "tokenEndpointAuthMethod": "client_secret_post",
                "subjectType": "public",
                "isConsentImplied": false,
                "esoteric": "tenant-default"
            },
            "signEncOAuth2ClientConfig": {},
            "coreOAuth2ClientConfig": {
                "clientName": [],
                "clientType": "Confidential",
                "userpassword": null,
                "scopes": [],
                "defaultScopes": [],
                "redirectionUris": [],
                "accessTokenLifetime": 0,
                "refreshTokenLifetime": 0,
                "authorizationCodeLifetime": 0
            },
            "coreOpenIDClientConfig": {},
            "coreUmaClientConfig": {}
        })
    }

    #[test]
    fn every_common_flag_lands_in_its_template_field() {
        let spec = CreateClientSpec {
            name: Some("Example client".into()),
            description: Some("Purpose-built test client".into()),
            client_type: Some("Public".into()),
            secret: Some("not-a-real-secret".into()),
            scopes: vec!["openid".into(), "profile".into()],
            default_scopes: vec!["openid".into()],
            redirect_uris: vec!["https://example.test/callback".into()],
            grants: vec!["authorization_code".into()],
            response_types: vec!["code".into()],
            token_endpoint_auth_method: Some("none".into()),
            subject_type: Some("pairwise".into()),
            implied_consent: Some(true),
            access_token_lifetime: Some(3600),
            refresh_token_lifetime: Some(7200),
            authorization_code_lifetime: Some(120),
        };

        let body = build_create_body(template(), None, &spec).unwrap();

        assert_eq!(
            body["coreOAuth2ClientConfig"]["clientName"],
            json!(["Example client"])
        );
        assert_eq!(
            body["advancedOAuth2ClientConfig"]["descriptions"],
            json!(["Purpose-built test client"])
        );
        assert_eq!(body["coreOAuth2ClientConfig"]["clientType"], "Public");
        assert_eq!(
            body["coreOAuth2ClientConfig"]["userpassword"],
            "not-a-real-secret"
        );
        assert_eq!(
            body["coreOAuth2ClientConfig"]["scopes"],
            json!(["openid", "profile"])
        );
        assert_eq!(
            body["coreOAuth2ClientConfig"]["defaultScopes"],
            json!(["openid"])
        );
        assert_eq!(
            body["coreOAuth2ClientConfig"]["redirectionUris"],
            json!(["https://example.test/callback"])
        );
        assert_eq!(
            body["advancedOAuth2ClientConfig"]["grantTypes"],
            json!(["authorization_code"])
        );
        assert_eq!(
            body["advancedOAuth2ClientConfig"]["responseTypes"],
            json!(["code"])
        );
        assert_eq!(
            body["advancedOAuth2ClientConfig"]["tokenEndpointAuthMethod"],
            "none"
        );
        assert_eq!(
            body["advancedOAuth2ClientConfig"]["subjectType"],
            "pairwise"
        );
        assert_eq!(body["advancedOAuth2ClientConfig"]["isConsentImplied"], true);
        assert_eq!(body["coreOAuth2ClientConfig"]["accessTokenLifetime"], 3600);
        assert_eq!(body["coreOAuth2ClientConfig"]["refreshTokenLifetime"], 7200);
        assert_eq!(
            body["coreOAuth2ClientConfig"]["authorizationCodeLifetime"],
            120
        );
    }

    #[test]
    fn create_states_client_secret_post_instead_of_inheriting_the_template() {
        let body = build_create_body(template(), None, &CreateClientSpec::default()).unwrap();

        assert_eq!(
            body["advancedOAuth2ClientConfig"]["tokenEndpointAuthMethod"],
            "client_secret_post"
        );
    }

    #[test]
    fn seeded_token_endpoint_auth_method_wins_over_the_create_default() {
        // Must seed a value that is NOT the create default, or the assertion
        // passes without the seed being consulted at all.
        let seed = json!({
            "advancedOAuth2ClientConfig": {
                "tokenEndpointAuthMethod": "client_secret_basic"
            }
        });
        let body = build_create_body(template(), Some(seed), &CreateClientSpec::default()).unwrap();

        assert_eq!(
            body["advancedOAuth2ClientConfig"]["tokenEndpointAuthMethod"],
            "client_secret_basic"
        );
    }

    #[test]
    fn from_seed_overlays_template_and_explicit_flags_overlay_seed() {
        let seed = json!({
            "coreOAuth2ClientConfig": {
                "clientName": ["Seed name"],
                "clientType": "Public"
            },
            "advancedOAuth2ClientConfig": {
                "esoteric": "seeded",
                "grantTypes": ["client_credentials"]
            }
        });
        let spec = CreateClientSpec {
            name: Some("Flag name".into()),
            grants: vec!["authorization_code".into()],
            ..CreateClientSpec::default()
        };

        let body = build_create_body(template(), Some(seed), &spec).unwrap();

        assert_eq!(
            body["coreOAuth2ClientConfig"]["clientName"],
            json!(["Flag name"])
        );
        assert_eq!(body["coreOAuth2ClientConfig"]["clientType"], "Public");
        assert_eq!(body["coreOAuth2ClientConfig"]["accessTokenLifetime"], 0);
        assert_eq!(body["advancedOAuth2ClientConfig"]["esoteric"], "seeded");
        assert_eq!(
            body["advancedOAuth2ClientConfig"]["grantTypes"],
            json!(["authorization_code"])
        );
    }

    fn client_with_grants(grant_types: Value) -> Value {
        json!({
            "_id": "client-a",
            "_rev": "123",
            "_type": {"_id": "OAuth2Client"},
            "advancedOAuth2ClientConfig": {"grantTypes": grant_types},
            "coreOAuth2ClientConfig": {
                "userpassword-encrypted": "ciphertext"
            }
        })
    }

    #[test]
    fn grant_add_to_empty_client_is_a_change() {
        let update = update_grants(
            &client_with_grants(json!([])),
            &["client_credentials".into()],
            GrantOperation::Add,
        )
        .unwrap();

        assert!(update.changed);
        assert_eq!(update.grants, ["client_credentials"]);
        assert_eq!(
            update.body["advancedOAuth2ClientConfig"]["grantTypes"],
            json!(["client_credentials"])
        );
    }

    #[test]
    fn grant_add_duplicate_is_idempotent() {
        let update = update_grants(
            &client_with_grants(json!(["client_credentials"])),
            &["client_credentials".into()],
            GrantOperation::Add,
        )
        .unwrap();

        assert!(!update.changed);
        assert_eq!(update.grants, ["client_credentials"]);
    }

    #[test]
    fn grant_remove_only_grant_leaves_an_empty_list() {
        let update = update_grants(
            &client_with_grants(json!(["client_credentials"])),
            &["client_credentials".into()],
            GrantOperation::Remove,
        )
        .unwrap();

        assert!(update.changed);
        assert!(update.grants.is_empty());
        assert_eq!(
            update.body["advancedOAuth2ClientConfig"]["grantTypes"],
            json!([])
        );
    }

    #[test]
    fn grant_remove_absent_is_idempotent() {
        let update = update_grants(
            &client_with_grants(json!(["client_credentials"])),
            &["authorization_code".into()],
            GrantOperation::Remove,
        )
        .unwrap();

        assert!(!update.changed);
        assert_eq!(update.grants, ["client_credentials"]);
    }

    #[test]
    fn grant_update_preserves_wrappers_and_makes_changed_inherited_values_local() {
        let update = update_grants(
            &client_with_grants(json!({
                "inherited": true,
                "value": ["client_credentials"],
                "metadata": "preserved"
            })),
            &["authorization_code".into()],
            GrantOperation::Add,
        )
        .unwrap();

        assert_eq!(
            update.body["advancedOAuth2ClientConfig"]["grantTypes"],
            json!({
                "inherited": false,
                "value": ["client_credentials", "authorization_code"],
                "metadata": "preserved"
            })
        );
    }

    #[test]
    fn grant_update_sanitizes_server_and_encrypted_fields() {
        let update = update_grants(
            &client_with_grants(json!([])),
            &["authorization_code".into()],
            GrantOperation::Add,
        )
        .unwrap();

        assert!(update.body.get("_id").is_none());
        assert!(update.body.get("_rev").is_none());
        assert!(update.body.get("_type").is_none());
        assert!(
            update.body["coreOAuth2ClientConfig"]
                .get("userpassword-encrypted")
                .is_none()
        );
    }

    #[test]
    fn create_body_strips_server_and_encrypted_fields() {
        let seed = json!({
            "_id": "source-client",
            "_rev": "123",
            "_type": {"_id": "OAuth2Client"},
            "coreOAuth2ClientConfig": {
                "userpassword-encrypted": "AQIC...",
                "nested": {"other-encrypted": "ciphertext", "kept": true}
            }
        });

        let body = build_create_body(template(), Some(seed), &CreateClientSpec::default()).unwrap();

        assert!(body.get("_id").is_none());
        assert!(body.get("_rev").is_none());
        assert!(body.get("_type").is_none());
        assert!(
            body["coreOAuth2ClientConfig"]
                .get("userpassword-encrypted")
                .is_none()
        );
        assert!(
            body["coreOAuth2ClientConfig"]["nested"]
                .get("other-encrypted")
                .is_none()
        );
        assert_eq!(body["coreOAuth2ClientConfig"]["nested"]["kept"], true);
    }

    /// Seeding with the template itself must change nothing. The recursive
    /// merge is easy to get subtly wrong — an overlay that replaced whole
    /// groups instead of descending into them would still pass the
    /// example-based tests above, because those only assert the keys they set.
    #[test]
    fn seeding_with_the_template_itself_is_the_identity() {
        let body =
            build_create_body(template(), Some(template()), &CreateClientSpec::default()).unwrap();

        assert_eq!(body, sanitize_for_write(&template()));
    }

    fn schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "coreOAuth2ClientConfig": {
                    "properties": {
                        "clientType": {"enum": ["Confidential", "Public"]}
                    }
                },
                "advancedOAuth2ClientConfig": {
                    "properties": {
                        "grantTypes": {"items": {"enum": ["authorization_code", "client_credentials"]}},
                        "responseTypes": {"items": {"type": "string"}},
                        "tokenEndpointAuthMethod": {"enum": ["client_secret_basic", "none"]},
                        "subjectType": {"enum": ["pairwise", "public"]}
                    }
                }
            }
        })
    }

    #[test]
    fn enum_validation_uses_scalar_and_array_values_from_schema() {
        let valid = json!({
            "coreOAuth2ClientConfig": {"clientType": "Public"},
            "advancedOAuth2ClientConfig": {
                "grantTypes": ["client_credentials"],
                "responseTypes": ["tenant-specific-response"],
                "tokenEndpointAuthMethod": "none",
                "subjectType": "public"
            }
        });
        assert!(validate_enumerated_fields(&valid, Some(&schema())).is_ok());

        let invalid_grant = json!({
            "advancedOAuth2ClientConfig": {"grantTypes": ["stale-hardcoded-value"]}
        });
        let error = validate_enumerated_fields(&invalid_grant, Some(&schema())).unwrap_err();
        assert!(error.contains("grantTypes"));
        assert!(error.contains("authorization_code"));

        let invalid_method = json!({
            "advancedOAuth2ClientConfig": {"tokenEndpointAuthMethod": "made_up"}
        });
        assert!(
            validate_enumerated_fields(&invalid_method, Some(&schema()))
                .unwrap_err()
                .contains("tokenEndpointAuthMethod")
        );
    }

    #[test]
    fn enum_validation_reads_grants_from_an_inherited_wrapper() {
        let body = json!({
            "advancedOAuth2ClientConfig": {
                "grantTypes": {
                    "inherited": false,
                    "value": ["client_credentials"]
                }
            }
        });

        assert!(validate_enumerated_fields(&body, Some(&schema())).is_ok());
    }

    #[test]
    fn grant_validation_ignores_unrelated_stale_fields() {
        let body = json!({
            "advancedOAuth2ClientConfig": {
                "grantTypes": ["client_credentials"],
                "subjectType": "stale-subject-type"
            }
        });

        assert!(validate_grant_types(&body, Some(&schema())).is_ok());
    }

    #[test]
    fn enum_validation_sends_values_through_when_schema_fetch_failed() {
        let body = json!({
            "coreOAuth2ClientConfig": {"clientType": "future-type"},
            "advancedOAuth2ClientConfig": {
                "grantTypes": ["future-grant"],
                "tokenEndpointAuthMethod": "future-method"
            }
        });

        assert!(validate_enumerated_fields(&body, None).is_ok());
    }
}
