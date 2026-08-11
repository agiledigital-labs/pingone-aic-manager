//! HTTP wrappers for the IDM `config/access` document.
//! See `docs/api/19-config-access.md`.

use serde_json::Value;

use crate::{Error, Result};

const ACCESS_PATH: &str = "/openidm/config/access";
const AUTHENTICATION_PATH: &str = "/openidm/config/authentication";

/// Read the complete access-control document.
pub async fn get_access(tenant: &str) -> Result<Value> {
    crate::aic::api::get(tenant, ACCESS_PATH).await
}

/// Read the authentication config used to resolve synthetic role references.
pub async fn get_authentication(tenant: &str) -> Result<Value> {
    crate::aic::api::get(tenant, AUTHENTICATION_PATH).await
}

/// Collect role references known either as role objects or authentication
/// mappings. Authentication uses arrays here; `config/access` itself does not.
pub async fn role_index(tenant: &str) -> Result<crate::access::spec::RoleIndex> {
    let (roles, authentication) = tokio::try_join!(
        crate::roles::api::list_roles(tenant),
        get_authentication(tenant)
    )?;
    let mut index = crate::access::spec::RoleIndex::from_roles(
        roles
            .iter()
            .filter_map(|role| role.get("_id").and_then(Value::as_str))
            .map(|id| format!("internal/role/{id}")),
    );

    if let Some(mappings) = authentication
        .pointer("/rsFilter/staticUserMapping")
        .and_then(Value::as_array)
    {
        for mapping in mappings {
            extend_role_index(&mut index, mapping.get("roles"));
        }
    }
    extend_role_index(
        &mut index,
        authentication.pointer("/rsFilter/anonymousUserMapping/roles"),
    );
    Ok(index)
}

fn extend_role_index(index: &mut crate::access::spec::RoleIndex, roles: Option<&Value>) {
    let Some(roles) = roles.and_then(Value::as_array) else {
        return;
    };
    index.extend(roles.iter().filter_map(Value::as_str).map(str::to_string));
}

/// Replace the complete access-control document.
pub async fn put_access(tenant: &str, body: Value, confirmed_prod: bool) -> Result<Value> {
    crate::aic::api::put(tenant, ACCESS_PATH, body, confirmed_prod).await
}

/// Why a confirmed write could not be completed.
#[derive(Debug)]
pub enum ConfirmedWriteError {
    /// The PUT itself failed, so the intended write was not accepted.
    NotWritten(Error),
    /// The PUT succeeded, but the one read-back could not confirm its content.
    AcceptedButUnconfirmed(String),
}

/// Replace the document and confirm whole-document equality on one read-back.
///
/// A lost-update failure has been verified for managed config only (Q14 in
/// `docs/api/99-quirks-and-open-questions.md`); it has not been established for
/// `config/access`. This guard therefore spends the one-read-back budget on an
/// immediate whole-document comparison. It does not retry the write.
pub async fn put_access_confirmed(
    tenant: &str,
    body: Value,
    confirmed_prod: bool,
) -> std::result::Result<(), ConfirmedWriteError> {
    put_access(tenant, body.clone(), confirmed_prod)
        .await
        .map_err(ConfirmedWriteError::NotWritten)?;
    let returned = get_access(tenant).await.map_err(|error| {
        ConfirmedWriteError::AcceptedButUnconfirmed(format!(
            "config/access write for tenant {tenant:?} was accepted, but read-back failed: {error}"
        ))
    })?;
    confirm_document(tenant, &body, &returned)
}

fn confirm_document(
    tenant: &str,
    intended: &Value,
    returned: &Value,
) -> std::result::Result<(), ConfirmedWriteError> {
    if returned == intended {
        Ok(())
    } else {
        Err(ConfirmedWriteError::AcceptedButUnconfirmed(format!(
            "config/access write for tenant {tenant:?} was accepted, but read-back did not equal the intended document"
        )))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::access::ops;
    use crate::access::spec::{RuleEdit, RuleSpec};

    use super::*;

    #[test]
    fn confirmation_rejects_a_discarded_write_for_every_verb() {
        let before = crate::access::six_rule_fixture();
        let add = ops::append(
            &before,
            RuleSpec {
                pattern: "endpoint/new/*".into(),
                roles: "*".into(),
                methods: "read".into(),
                actions: None,
                custom_authz: None,
                exclude_patterns: None,
            },
        )
        .unwrap()
        .document;
        let edit = ops::replace_at(
            &before,
            1,
            RuleEdit {
                actions: Some(String::new()),
                ..RuleEdit::default()
            },
        )
        .unwrap()
        .document;
        let remove = ops::remove_at(&before, &[4]).unwrap().document;
        let mut apply = before.clone();
        apply["configs"][2]["methods"] = json!("read,query");

        for (verb, intended) in [
            ("add", add),
            ("edit", edit),
            ("rm", remove),
            ("apply", apply),
        ] {
            let error = confirm_document("sandbox", &intended, &before).unwrap_err();
            assert!(
                matches!(error, ConfirmedWriteError::AcceptedButUnconfirmed(message) if message.contains("did not equal")),
                "{verb} unexpectedly accepted a discarded write"
            );
        }
    }

    #[test]
    fn confirmation_accepts_an_exact_empty_document() {
        let intended = json!({"_id": "access", "configs": []});
        assert!(confirm_document("sandbox", &intended, &intended).is_ok());
    }
}
