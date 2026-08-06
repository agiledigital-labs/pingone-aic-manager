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
    pub token_auth_method: Option<String>,
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
    if let Some(method) = &spec.token_auth_method {
        set_field(
            &mut template,
            "advancedOAuth2ClientConfig",
            "tokenEndpointAuthMethod",
            json!(method),
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
            token_auth_method: Some("none".into()),
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
