//! Human-readable helper text for AM secret labels.

const OAUTH_CLIENT_PREFIX: &str = "am.applications.oauth2.client.";
const OAUTH_CLIENT_SUFFIXES: [(&str, &str); 4] = [
    (".id.token.enc.public.key", "id.token.enc.public.key"),
    (".mtls.trusted.cert", "mtls.trusted.cert"),
    (".jwt.public.key", "jwt.public.key"),
    (".secret", "secret"),
];

pub fn describe(secret_id: &str) -> String {
    if let Some(description) = oauth2_client_description(secret_id) {
        return description;
    }
    if let Some(description) = exact_description(secret_id) {
        return description.to_string();
    }
    if let Some(description) = remote_consent_description(secret_id) {
        return description;
    }
    if let Some(description) = oidc_provider_description(secret_id) {
        return description;
    }
    if let Some(description) = default_saml_description(secret_id) {
        return description;
    }
    if let Some(description) = mfa_device_description(secret_id) {
        return description;
    }
    if let Some(description) = persistent_cookie_description(secret_id) {
        return description;
    }

    humanize(secret_id)
}

pub fn category(secret_id: &str) -> &'static str {
    if oauth2_client_parts(secret_id).is_some() {
        "OAuth2 client"
    } else if secret_id.starts_with("am.services.oauth2.oidc.") {
        "OIDC provider"
    } else if is_saml(secret_id) {
        "SAML2"
    } else if is_agent(secret_id) {
        "Agents"
    } else if is_mfa_device(secret_id) {
        "MFA / devices"
    } else if is_authentication(secret_id) {
        "Authentication"
    } else {
        "Other"
    }
}

fn oauth2_client_description(secret_id: &str) -> Option<String> {
    let (client, kind) = oauth2_client_parts(secret_id)?;
    Some(match kind {
        "secret" => format!("Client secret for OAuth2/OIDC client '{client}'."),
        "jwt.public.key" => format!(
            "Public key verifying signed JWTs from OAuth2 client '{client}' (private_key_jwt auth / signed request objects)."
        ),
        "id.token.enc.public.key" => {
            format!("Public key used to encrypt ID tokens issued to OAuth2 client '{client}'.")
        }
        "mtls.trusted.cert" => format!(
            "Trusted client certificate for mTLS authentication by OAuth2 client '{client}'."
        ),
        _ => return None,
    })
}

fn oauth2_client_parts(secret_id: &str) -> Option<(&str, &'static str)> {
    let rest = secret_id.strip_prefix(OAUTH_CLIENT_PREFIX)?;
    for (suffix, kind) in OAUTH_CLIENT_SUFFIXES {
        let Some(client) = rest.strip_suffix(suffix) else {
            continue;
        };
        if !client.is_empty() {
            return Some((client, kind));
        }
    }
    None
}

fn exact_description(secret_id: &str) -> Option<&'static str> {
    match secret_id {
        "am.services.saml2.metadata.signing.RSA" => Some("SAML2 metadata signing key."),
        "am.authn.authid.signing.HMAC" => {
            Some("HMAC key used to sign authentication-tree authId JWTs.")
        }
        "am.authn.trees.transientstate.encryption" => {
            Some("Encryption key for transient authentication-tree state.")
        }
        "am.applications.agents.ig.secret" => {
            Some("Shared secret used by Identity Gateway agents.")
        }
        "am.services.selfservice.token.signing" => {
            Some("Signing key for self-service recovery tokens.")
        }
        "am.services.selfservice.token.encryption" => {
            Some("Encryption key for self-service recovery tokens.")
        }
        "am.services.pushnotification.sns.accesskey.secret" => {
            Some("AWS SNS access-key secret for push notifications.")
        }
        "am.services.uma.pct.encryption" => Some("Encryption key for UMA persisted claims tokens."),
        "am.policy.configuration.service.mtls.cert" => {
            Some("mTLS certificate used by the policy configuration service.")
        }
        "am.services.attestation.google.public.key" => {
            Some("Google public key used for device attestation validation.")
        }
        "am.authentication.nodes.webauthn.fidometadataservice.rootcertificate" => {
            Some("Root certificate for the WebAuthn FIDO metadata service.")
        }
        _ => None,
    }
}

fn remote_consent_description(secret_id: &str) -> Option<String> {
    let alg = secret_id.strip_prefix("am.applications.agents.remote.consent.request.signing.")?;
    match alg {
        "ES256" | "ES384" | "ES512" | "RSA" => Some(format!(
            "Remote-consent request signing key for agents ({alg})."
        )),
        _ => None,
    }
}

fn oidc_provider_description(secret_id: &str) -> Option<String> {
    let rest = secret_id.strip_prefix("am.services.oauth2.oidc.")?;
    let detail = humanize(rest);

    if rest == "mtls.client.authentication" {
        Some("Trust material for OIDC provider mTLS client authentication.".to_string())
    } else if rest.contains("signing") || rest.contains("jwt") {
        Some(format!("OIDC provider JWT signing key ({detail})."))
    } else if rest.contains("encryption") && rest.contains("decryption") {
        Some(format!(
            "OIDC provider ID-token encryption/decryption key ({detail})."
        ))
    } else if rest.contains("encryption") || rest.contains("enc") {
        Some(format!("OIDC provider ID-token encryption key ({detail})."))
    } else if rest.contains("decryption") || rest.contains("dec") {
        Some(format!("OIDC provider ID-token decryption key ({detail})."))
    } else {
        Some(format!("OIDC provider secret material ({detail})."))
    }
}

fn default_saml_description(secret_id: &str) -> Option<String> {
    let rest =
        secret_id.strip_prefix("am.default.applications.federation.entity.providers.saml2.")?;
    let mut parts = rest.split('.');
    let role = parts.next()?;
    let purpose = parts.next()?;
    if parts.next().is_some() {
        return None;
    }

    let role = match role {
        "idp" => "IdP",
        "sp" => "SP",
        _ => return None,
    };
    let purpose = match purpose {
        "signing" => "signing key",
        "encryption" => "encryption key",
        "mtls" => "mTLS certificate",
        _ => return None,
    };

    Some(format!("Default SAML2 {role} {purpose}."))
}

fn mfa_device_description(secret_id: &str) -> Option<String> {
    if !is_mfa_device(secret_id) {
        return None;
    }

    Some("Encryption key for MFA authenticator and device data.".to_string())
}

fn persistent_cookie_description(secret_id: &str) -> Option<String> {
    if !is_persistent_cookie(secret_id) {
        return None;
    }

    if secret_id.ends_with(".signing") {
        Some("Persistent-cookie signing key for remember-me authentication.".to_string())
    } else if secret_id.ends_with(".encryption") {
        Some("Persistent-cookie encryption key for remember-me authentication.".to_string())
    } else {
        None
    }
}

fn is_saml(secret_id: &str) -> bool {
    secret_id == "am.services.saml2.metadata.signing.RSA"
        || secret_id.starts_with("am.default.applications.federation.entity.providers.saml2.")
}

fn is_agent(secret_id: &str) -> bool {
    secret_id == "am.applications.agents.ig.secret"
        || secret_id.starts_with("am.applications.agents.remote.consent.request.signing.")
}

fn is_mfa_device(secret_id: &str) -> bool {
    ((secret_id.starts_with("am.services.authenticator")
        || secret_id.starts_with("am.services.device"))
        && secret_id.ends_with(".encryption"))
        || secret_id == "am.services.attestation.google.public.key"
        || secret_id == "am.authentication.nodes.webauthn.fidometadataservice.rootcertificate"
}

fn is_authentication(secret_id: &str) -> bool {
    secret_id == "am.authn.authid.signing.HMAC"
        || secret_id == "am.authn.trees.transientstate.encryption"
        || secret_id.starts_with("am.services.selfservice.")
        || is_persistent_cookie(secret_id)
}

fn is_persistent_cookie(secret_id: &str) -> bool {
    secret_id.contains(".persistentcookie.")
        && (secret_id.ends_with(".signing") || secret_id.ends_with(".encryption"))
}

fn humanize(secret_id: &str) -> String {
    let trimmed = secret_id.strip_prefix("am.").unwrap_or(secret_id);
    let words: String = trimmed
        .chars()
        .map(|ch| {
            if matches!(ch, '.' | '_' | '-') {
                ' '
            } else {
                ch
            }
        })
        .collect();
    let collapsed = words.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        return "Other secret label".to_string();
    }

    let mut chars = collapsed.chars();
    let Some(first) = chars.next() else {
        return "Other secret label".to_string();
    };
    let mut out = String::new();
    out.push(first.to_ascii_uppercase());
    out.push_str(chars.as_str());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_oauth2_client_secret() {
        assert_eq!(
            describe("am.applications.oauth2.client.pega.secret"),
            "Client secret for OAuth2/OIDC client 'pega'."
        );
        assert_eq!(
            category("am.applications.oauth2.client.pega.secret"),
            "OAuth2 client"
        );
    }

    #[test]
    fn describes_oauth2_client_jwt_public_key_with_dotted_client() {
        assert_eq!(
            describe("am.applications.oauth2.client.alpha.vktest.jwt.public.key"),
            "Public key verifying signed JWTs from OAuth2 client 'alpha.vktest' (private_key_jwt auth / signed request objects)."
        );
    }

    #[test]
    fn describes_oauth2_client_id_token_encryption_key() {
        assert_eq!(
            describe("am.applications.oauth2.client.alpha.vktest.id.token.enc.public.key"),
            "Public key used to encrypt ID tokens issued to OAuth2 client 'alpha.vktest'."
        );
    }

    #[test]
    fn describes_oauth2_client_mtls_certificate() {
        assert_eq!(
            describe("am.applications.oauth2.client.pega.mtls.trusted.cert"),
            "Trusted client certificate for mTLS authentication by OAuth2 client 'pega'."
        );
    }

    #[test]
    fn describes_curated_saml_authentication_and_agent_ids() {
        assert_eq!(
            describe("am.services.saml2.metadata.signing.RSA"),
            "SAML2 metadata signing key."
        );
        assert_eq!(category("am.services.saml2.metadata.signing.RSA"), "SAML2");

        assert_eq!(
            describe("am.authn.authid.signing.HMAC"),
            "HMAC key used to sign authentication-tree authId JWTs."
        );
        assert_eq!(category("am.authn.authid.signing.HMAC"), "Authentication");

        assert_eq!(
            describe("am.authn.trees.transientstate.encryption"),
            "Encryption key for transient authentication-tree state."
        );

        assert_eq!(
            describe("am.applications.agents.ig.secret"),
            "Shared secret used by Identity Gateway agents."
        );
        assert_eq!(category("am.applications.agents.ig.secret"), "Agents");

        assert_eq!(
            describe("am.applications.agents.remote.consent.request.signing.ES384"),
            "Remote-consent request signing key for agents (ES384)."
        );
    }

    #[test]
    fn describes_self_service_push_uma_policy_and_attestation_ids() {
        assert_eq!(
            describe("am.services.selfservice.token.signing"),
            "Signing key for self-service recovery tokens."
        );
        assert_eq!(
            describe("am.services.selfservice.token.encryption"),
            "Encryption key for self-service recovery tokens."
        );
        assert_eq!(
            describe("am.services.pushnotification.sns.accesskey.secret"),
            "AWS SNS access-key secret for push notifications."
        );
        assert_eq!(
            describe("am.services.uma.pct.encryption"),
            "Encryption key for UMA persisted claims tokens."
        );
        assert_eq!(
            describe("am.policy.configuration.service.mtls.cert"),
            "mTLS certificate used by the policy configuration service."
        );
        assert_eq!(
            describe("am.services.attestation.google.public.key"),
            "Google public key used for device attestation validation."
        );
        assert_eq!(
            describe("am.authentication.nodes.webauthn.fidometadataservice.rootcertificate"),
            "Root certificate for the WebAuthn FIDO metadata service."
        );
        assert_eq!(
            category("am.services.attestation.google.public.key"),
            "MFA / devices"
        );
    }

    #[test]
    fn describes_oidc_provider_patterns() {
        let signing = describe("am.services.oauth2.oidc.rsa.jwt.signing");
        assert!(signing.contains("OIDC provider JWT signing key"));
        assert_eq!(
            category("am.services.oauth2.oidc.rsa.jwt.signing"),
            "OIDC provider"
        );

        let encryption = describe("am.services.oauth2.oidc.id.token.encryption");
        assert!(encryption.contains("OIDC provider ID-token encryption key"));
    }

    #[test]
    fn describes_mfa_device_patterns() {
        assert_eq!(
            describe("am.services.authenticatorpush.encryption"),
            "Encryption key for MFA authenticator and device data."
        );
        assert_eq!(
            category("am.services.devicebinding.encryption"),
            "MFA / devices"
        );
    }

    #[test]
    fn describes_default_saml_provider_patterns() {
        assert_eq!(
            describe("am.default.applications.federation.entity.providers.saml2.idp.signing"),
            "Default SAML2 IdP signing key."
        );
        assert_eq!(
            describe("am.default.applications.federation.entity.providers.saml2.sp.encryption"),
            "Default SAML2 SP encryption key."
        );
        assert_eq!(
            describe("am.default.applications.federation.entity.providers.saml2.idp.mtls"),
            "Default SAML2 IdP mTLS certificate."
        );
    }

    #[test]
    fn describes_persistent_cookie_patterns() {
        assert_eq!(
            describe("am.authentication.nodes.persistentcookie.signing"),
            "Persistent-cookie signing key for remember-me authentication."
        );
        assert_eq!(
            describe("am.default.authentication.modules.persistentcookie.encryption"),
            "Persistent-cookie encryption key for remember-me authentication."
        );
        assert_eq!(
            category("am.authentication.nodes.persistentcookie.signing"),
            "Authentication"
        );
    }

    #[test]
    fn fallback_humanizes_unknown_ids() {
        let description = describe("am.custom.unknown.secret");

        assert!(!description.is_empty());
        assert!(!description.starts_with("am."));
        assert_eq!(description, "Custom unknown secret");
        assert_eq!(category("am.custom.unknown.secret"), "Other");
    }
}
