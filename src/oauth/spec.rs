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
fn overrides_enabled(doc: &Value) -> Option<bool> {
    doc.get(CLIENT_OVERRIDES)?
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

/// With the block switched off, the only entries worth showing are the ones
/// someone deliberately set and which are therefore doing nothing — a script
/// id, or a plugin type flipped to something other than `PROVIDER`. Booleans
/// are not: an ignored `false` is not a setting, it is the absence of one, and
/// listing fifteen of them buries the two rows that matter.
fn override_entry_is_a_dormant_setting(field: &str, value: &Value) -> bool {
    if override_entry_inherits(field, value) {
        return false;
    }
    field.ends_with("Script") || field.ends_with("PluginType") || field.ends_with("Class")
}

fn push_override_rows(rows: &mut Vec<(String, String)>, doc: &Value) {
    let Some(Value::Object(config)) = doc.get(CLIENT_OVERRIDES).map(inherited_value) else {
        rows.push((CLIENT_OVERRIDES.to_string(), "<absent>".to_string()));
        return;
    };

    let enabled = overrides_enabled(doc);
    rows.push((
        CLIENT_OVERRIDES.to_string(),
        match enabled {
            Some(true) => "<in effect: every field below applies, set or defaulted>".to_string(),
            Some(false) => {
                "<ignored: providerOverridesEnabled is false, the realm applies>".to_string()
            }
            None => "<providerOverridesEnabled absent>".to_string(),
        },
    ));

    let show: fn(&str, &Value) -> bool = if enabled == Some(false) {
        override_entry_is_a_dormant_setting
    } else {
        |field, value| !override_entry_inherits(field, value)
    };

    let mut hidden = 0_usize;
    for (field, value) in config {
        // The switch itself is the section header above; counting it as
        // "not shown" would be wrong in both directions — it is shown, and
        // it is not an inherited setting.
        if field == "providerOverridesEnabled" {
            continue;
        }
        if show(field, value) {
            push_provider_value_rows(rows, &format!("  {field}"), value, provider_value_cell);
        } else {
            hidden += 1;
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

        let off = summary_row(&client_summary(&off), "overrideOAuth2ClientConfig");
        let on = summary_row(&client_summary(&on), "overrideOAuth2ClientConfig");

        assert!(off[0].contains("ignored"), "{off:?}");
        assert!(off[0].contains("realm applies"), "{off:?}");
        assert!(on[0].contains("in effect"), "{on:?}");
    }

    /// With the block switched off the only interesting entries are the ones
    /// someone set that are therefore doing nothing. The discriminating case
    /// is `issueRefreshToken: true`: a rule that showed every non-default
    /// value would list it, and an ignored `true` is not a setting.
    #[test]
    fn a_disabled_override_block_shows_only_the_dormant_settings() {
        let mut client = a_client();
        client["overrideOAuth2ClientConfig"]["providerOverridesEnabled"] = json!(false);
        client["overrideOAuth2ClientConfig"]["validateScopeScript"] =
            json!("3482b626-5446-4734-a8a9-9a47e653de33");

        let rows = client_summary(&client);

        assert_eq!(
            summary_row(&rows, "  validateScopeScript"),
            ["3482b626-5446-4734-a8a9-9a47e653de33"]
        );
        assert!(summary_row(&rows, "  issueRefreshToken").is_empty());
        assert!(summary_row(&rows, "  statelessTokensEnabled").is_empty());
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
