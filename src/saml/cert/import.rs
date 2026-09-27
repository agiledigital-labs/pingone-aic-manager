//! `aic saml import` over one existing remote entity, read against its
//! export: what the file would change, what an update will not apply, and
//! the plan for `--certs add|replace`.
//!
//! None of the file's bytes are sent. The plan is the entity's own export
//! with the file's signing certificates spliced in or out through
//! [`super::spec`]'s planner, so everything else in the file — endpoints,
//! encryption keys, extensions — is only ever *reported*, as not applied.

use std::collections::BTreeSet;

use crate::saml::cert::spec::{
    CertInfo, Planned, RoleChange, SigningCerts, Target, editable, finish, insert, is_signing,
    role_keys, role_of, signing_certs, use_label,
};
use crate::saml::metadata::{self, Fact, KeyLayout};
use crate::saml::spec::Role;
use crate::{Error, Result};

/// How an import over an existing remote entity treats its certificates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
#[clap(rename_all = "lowercase")]
pub enum ImportMode {
    /// Merge the file's signing certificates in, deduplicated by fingerprint.
    Add,
    /// Make each role's signing certificates exactly the file's.
    Replace,
}

impl ImportMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Add => "add",
            Self::Replace => "replace",
        }
    }
}

/// What a file says that an update will not apply.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Differences {
    /// Non-certificate content in the file and not on the tenant.
    pub only_in_file: Vec<Fact>,
    /// Non-certificate content on the tenant and not in the file.
    pub only_on_tenant: Vec<Fact>,
    /// Encryption or `use`-less certificates the file has and the tenant's
    /// role does not — out of this verb's scope, so ignored.
    pub ignored_keys: Vec<String>,
}

/// The sentence every summary with a non-certificate difference carries.
/// Pinned by a test: it is the reason an operator does not expect an
/// endpoint change to have happened.
pub const NOT_APPLIED: &str = "not applied — AIC ignores non-certificate changes on an update; \
     changing them means delete and re-import, which drops the entity's circle-of-trust \
     membership";

/// The one line for a file whose only differences are certificates.
pub const ONLY_CERTS_DIFFER: &str =
    "the file and the tenant differ only in certificates; nothing else would be left behind";

impl Differences {
    pub fn lines(&self) -> Vec<String> {
        if self.only_in_file.is_empty()
            && self.only_on_tenant.is_empty()
            && self.ignored_keys.is_empty()
        {
            return vec![ONLY_CERTS_DIFFER.to_string()];
        }
        let mut lines = Vec::new();
        if !self.only_in_file.is_empty() || !self.only_on_tenant.is_empty() {
            lines.push(format!("other differences: {NOT_APPLIED}"));
            for fact in &self.only_in_file {
                lines.push(format!("  file only    {}: {}", fact.owner, fact.text));
            }
            for fact in &self.only_on_tenant {
                lines.push(format!("  tenant only  {}: {}", fact.owner, fact.text));
            }
        }
        if !self.ignored_keys.is_empty() {
            lines.push(
                "non-signing certificates in the file: not applied — `aic saml cert` changes \
                 signing certificates only, and the tenant's encryption keys are kept as they are"
                    .to_string(),
            );
            for key in &self.ignored_keys {
                lines.push(format!("  {key}"));
            }
        }
        lines
    }
}

/// Multiset difference, `a` minus `b`.
fn minus(a: &[Fact], b: &[Fact]) -> Vec<Fact> {
    let mut remaining = b.to_vec();
    let mut out = Vec::new();
    for fact in a {
        if let Some(index) = remaining.iter().position(|held| held == fact) {
            remaining.swap_remove(index);
        } else {
            out.push(fact.clone());
        }
    }
    out.sort();
    out
}

/// An import over an existing remote entity, read against its export.
#[derive(Debug, Clone)]
pub struct ImportComparison {
    pub differences: Differences,
    target: Target,
    export: Vec<u8>,
    export_layout: KeyLayout,
    /// Per file role: its signing certificates, deduplicated, in order.
    file_signing: Vec<(Role, SigningCerts)>,
}

/// Compare an import file with the entity's export. `file` is the document
/// the import would otherwise have sent (sanitised, unless the operator
/// said not to), and **none of its bytes are sent**: the plan is built from
/// the export, and a certificate from the file reaches the document as a key
/// descriptor written fresh from its DER.
pub fn compare_import(target: &Target, export: &[u8], file: &[u8]) -> Result<ImportComparison> {
    let export_layout = editable(export, &target.entity_id)?;
    let file_layout = metadata::key_layout(file)?;
    if file_layout.entity_id != target.entity_id {
        return Err(Error::Config(format!(
            "the file names {:?}, not {:?}; refusing",
            file_layout.entity_id, target.entity_id
        )));
    }
    let held = export_layout
        .roles
        .iter()
        .map(|role| role_of(role.role))
        .collect::<Vec<_>>();
    let mut file_signing = Vec::new();
    let mut ignored_keys = Vec::new();
    for file_role in &file_layout.roles {
        let role = role_of(file_role.role);
        if !held.contains(&role) {
            return Err(Error::Config(format!(
                "the file declares a {} role that {} does not hold. A certificate update would \
                 add that whole role, endpoints and all — AM does not limit it to certificates \
                 for a role the entity lacks — so this is refused; adding a role means delete \
                 and re-import, which drops the entity's circle-of-trust membership",
                role.wire(),
                target.entity_id
            )));
        }
        if file_signing.iter().any(|(seen, _)| *seen == role) {
            return Err(Error::Config(format!(
                "the file declares {} twice; refusing",
                file_role.descriptor
            )));
        }
        let tenant_role = role_keys(&export_layout, role);
        let tenant_all = tenant_role
            .keys
            .iter()
            .flat_map(|key| key.certs.iter().map(|(cert, _)| cert.sha256.clone()))
            .collect::<BTreeSet<_>>();
        let mut signing: SigningCerts = Vec::new();
        for key in &file_role.keys {
            for (cert, der) in &key.certs {
                if is_signing(key) {
                    if !signing.iter().any(|(sha, _)| *sha == cert.sha256) {
                        signing.push((cert.sha256.clone(), der.clone()));
                    }
                } else if !tenant_all.contains(&cert.sha256) {
                    ignored_keys.push(format!(
                        "{} {} {}",
                        role.cli_word(),
                        use_label(key.key_use.as_deref()),
                        cert.sha256
                    ));
                }
            }
        }
        file_signing.push((role, signing));
    }
    let differences = Differences {
        only_in_file: minus(&file_layout.facts, &export_layout.facts),
        only_on_tenant: minus(&export_layout.facts, &file_layout.facts),
        ignored_keys,
    };
    Ok(ImportComparison {
        differences,
        target: target.clone(),
        export: export.to_vec(),
        export_layout,
        file_signing,
    })
}

/// Plan an import in `mode`. Roles the file does not declare are left as
/// they are, which is also what AM does with a role a document omits.
pub fn plan_import(comparison: &ImportComparison, mode: ImportMode) -> Result<Planned> {
    let entity_id = &comparison.target.entity_id;
    let mut edits = Vec::new();
    let mut changes = Vec::new();
    for (role, wanted) in &comparison.file_signing {
        let keys = role_keys(&comparison.export_layout, *role);
        let current = signing_certs(keys);
        let mut added = Vec::new();
        for (sha, der) in wanted {
            if !current.iter().any(|(held, _)| held == sha) {
                edits.push(insert(keys, der, entity_id)?);
                added.push(CertInfo::from_der(sha, der));
            }
        }
        let mut removed = Vec::new();
        let mut kept = Vec::new();
        for (sha, der) in &current {
            let goes = mode == ImportMode::Replace && !wanted.iter().any(|(want, _)| want == sha);
            if goes {
                removed.push(CertInfo::from_der(sha, der));
            } else {
                kept.push(CertInfo::from_der(sha, der));
            }
        }
        if mode == ImportMode::Replace {
            for key in keys.keys.iter().filter(|key| is_signing(key)) {
                let dropping = key
                    .certs
                    .iter()
                    .filter(|(cert, _)| !wanted.iter().any(|(want, _)| *want == cert.sha256))
                    .count();
                if dropping == 0 {
                    continue;
                }
                if dropping < key.certs.len() {
                    return Err(Error::Config(format!(
                        "a <KeyDescriptor> in {entity_id}'s {} holds certificates the file keeps \
                         and certificates it drops, and replacing only some of them would mean \
                         rewriting that key descriptor; refusing",
                        keys.descriptor
                    )));
                }
                edits.push(key.remove());
            }
        }
        changes.push(RoleChange {
            role: *role,
            added,
            removed,
            kept,
        });
    }
    finish(&comparison.target, &comparison.export, &edits, changes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::saml::cert::testdoc::*;

    fn file(roles: &str) -> Vec<u8> {
        document(roles)
    }

    #[test]
    fn replace_keeps_encryption_keys_byte_for_byte_and_drops_other_signing_ones() {
        let export = document(&idp(&[
            (Some("signing"), RSA_DER),
            (Some("encryption"), &der(EC_PEM)),
        ]));
        let upload = file(&idp(&[(Some("signing"), &der(PREFIX_PEM))]));
        let comparison = compare_import(&target(&[Role::Idp]), &export, &upload).unwrap();
        let plan = change(plan_import(&comparison, ImportMode::Replace).unwrap());
        assert_eq!(signing_of(&plan.document, "IDPSSODescriptor"), [PREFIX_SHA]);
        let encryption = am_key(Some("encryption"), &der(EC_PEM));
        assert!(
            String::from_utf8(plan.document.clone())
                .unwrap()
                .contains(&encryption),
            "the encryption key descriptor must survive byte for byte"
        );
        assert_eq!(plan.changes[0].removed[0].sha256, RSA_SHA);
        assert_eq!(plan.changes[0].added[0].sha256, PREFIX_SHA);
        assert!(plan.changes[0].kept.is_empty());

        // Add, over the same file, keeps the RSA certificate as well.
        let add = change(plan_import(&comparison, ImportMode::Add).unwrap());
        assert_eq!(
            signing_of(&add.document, "IDPSSODescriptor"),
            [RSA_SHA, PREFIX_SHA]
        );
        assert!(add.changes[0].removed.is_empty());
    }

    #[test]
    fn add_deduplicates_by_fingerprint_and_a_file_already_in_place_is_no_change() {
        let export = document(&idp(&[(Some("signing"), RSA_DER)]));
        let upload = file(&idp(&[
            (Some("signing"), RSA_DER),
            (Some("signing"), RSA_DER),
            (Some("signing"), &der(EC_PEM)),
        ]));
        let comparison = compare_import(&target(&[Role::Idp]), &export, &upload).unwrap();
        let plan = change(plan_import(&comparison, ImportMode::Add).unwrap());
        assert_eq!(
            signing_of(&plan.document, "IDPSSODescriptor"),
            [RSA_SHA, EC_SHA]
        );

        let same = file(&idp(&[(Some("signing"), RSA_DER)]));
        let comparison = compare_import(&target(&[Role::Idp]), &export, &same).unwrap();
        for mode in [ImportMode::Add, ImportMode::Replace] {
            assert!(matches!(
                plan_import(&comparison, mode).unwrap(),
                Planned::NoChange(_)
            ));
        }
        assert_eq!(comparison.differences.lines(), [ONLY_CERTS_DIFFER]);
    }

    #[test]
    fn replace_with_a_file_that_has_no_signing_certificate_refuses() {
        let export = document(&idp(&[(Some("signing"), RSA_DER)]));
        let upload = file(&idp(&[(Some("encryption"), &der(EC_PEM))]));
        let comparison = compare_import(&target(&[Role::Idp]), &export, &upload).unwrap();
        assert!(plan_import(&comparison, ImportMode::Replace).is_err());
        // The encryption certificate is reported as not applied.
        assert_eq!(
            comparison.differences.ignored_keys,
            [format!("idp encryption {EC_SHA}")]
        );
    }

    #[test]
    fn a_file_role_the_entity_lacks_is_refused() {
        let export = document(&idp(&[(Some("signing"), RSA_DER)]));
        let upload = file(&format!(
            "{}{}",
            idp(&[(Some("signing"), RSA_DER)]),
            sp(&[(Some("signing"), RSA_DER)])
        ));
        let error = compare_import(&target(&[Role::Idp]), &export, &upload)
            .unwrap_err()
            .to_string();
        assert!(error.contains("serviceProvider role"), "{error}");
    }

    /// The wording is pinned: it is what stops an operator believing an
    /// endpoint change in the file was applied.
    #[test]
    fn non_certificate_differences_are_reported_as_not_applied() {
        assert_eq!(
            NOT_APPLIED,
            "not applied — AIC ignores non-certificate changes on an update; changing them \
             means delete and re-import, which drops the entity's circle-of-trust membership"
        );
        let export = document(&idp(&[(Some("signing"), RSA_DER)]));
        let upload = String::from_utf8(file(&idp(&[(Some("signing"), &der(EC_PEM))])))
            .unwrap()
            .replace(
                "https://login.example.com/saml2",
                "https://login.example.com/moved",
            );
        let comparison = compare_import(&target(&[Role::Idp]), &export, upload.as_bytes()).unwrap();
        let lines = comparison.differences.lines();
        assert_eq!(lines[0], format!("other differences: {NOT_APPLIED}"));
        assert!(lines[1].starts_with("  file only    IDPSSODescriptor: SingleSignOnService"));
        assert!(lines[1].contains("/moved"));
        assert!(lines[2].starts_with("  tenant only  IDPSSODescriptor: SingleSignOnService"));
        // And the plan still contains only the certificate change.
        let plan = change(plan_import(&comparison, ImportMode::Replace).unwrap());
        assert!(!String::from_utf8(plan.document).unwrap().contains("/moved"));
    }

    #[test]
    fn the_file_must_name_the_entity() {
        let export = document(&idp(&[(Some("signing"), RSA_DER)]));
        let upload = String::from_utf8(file(&idp(&[(Some("signing"), RSA_DER)])))
            .unwrap()
            .replace(ENTITY, "https://sp-b.example.com");
        assert!(compare_import(&target(&[Role::Idp]), &export, upload.as_bytes()).is_err());
    }
}
