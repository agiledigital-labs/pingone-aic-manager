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
use crate::saml::metadata::{self, Fact, KeyLayout, RoleKeys};
use crate::saml::pem;
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
    /// Non-signing key descriptors — encryption or `use`-less — as
    /// `role use sha256`, plus `KeyName "…"` when the descriptor has one,
    /// compared by all of those: in the file and not on the tenant (ignored),
    /// and on the tenant and not in the file (kept). A certificate the tenant
    /// signs with and the file lists for encryption is a difference here, not
    /// a match. Every role either side declares is visited, so a role the
    /// file omits contributes its keys as tenant only. Nothing else inside a
    /// key descriptor is compared — `EncryptionMethod`, say — and never its
    /// raw bytes, because AM re-indents the whitespace inside `<KeyInfo>`.
    pub file_only_keys: Vec<String>,
    pub tenant_only_keys: Vec<String>,
}

/// The sentence every summary with a non-certificate difference carries.
/// Pinned by a test: it is the reason an operator does not expect an
/// endpoint change to have happened.
pub const NOT_APPLIED: &str = "not applied — AIC ignores non-certificate changes on an update; \
     changing them means delete and re-import, which drops the entity's circle-of-trust \
     membership";

/// The one line for a file in which nothing this summary compares differs
/// except signing certificates. It says what it compared, because the
/// comparison is shallow: see [`Differences`] and [`metadata::Fact`].
pub const ONLY_CERTS_DIFFER: &str = "no differences found besides signing certificates, \
     comparing the entity's and each role's attributes and every element nested in them \
     (names, attributes and text, whitespace aside), and each non-signing key descriptor's \
     role, use, KeyName and certificate SHA-256";

impl Differences {
    pub fn lines(&self) -> Vec<String> {
        if self.only_in_file.is_empty()
            && self.only_on_tenant.is_empty()
            && self.file_only_keys.is_empty()
            && self.tenant_only_keys.is_empty()
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
        if !self.file_only_keys.is_empty() || !self.tenant_only_keys.is_empty() {
            lines.push(
                "non-signing key descriptors, compared by role, use, KeyName and certificate \
                 SHA-256: not applied — `aic saml cert` changes signing certificates only, and \
                 the tenant's encryption and use-less key descriptors are sent back as they are"
                    .to_string(),
            );
            for key in &self.file_only_keys {
                lines.push(format!("  file only    {key}"));
            }
            for key in &self.tenant_only_keys {
                lines.push(format!("  tenant only  {key}"));
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

/// The 1-based line of the first non-whitespace byte at or after `offset` —
/// a [`metadata::KeyEntry`]'s range starts at the newline before its element.
fn line_of(xml: &[u8], offset: usize) -> usize {
    let at = xml[offset..]
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .map_or(xml.len(), |skip| offset + skip);
    1 + xml[..at].iter().filter(|byte| **byte == b'\n').count()
}

/// A role's non-signing key descriptors as `role use sha256`, with
/// ` KeyName "…"` appended when there is one — the identity the summary
/// compares them by. `None` (a role one side does not declare) is empty.
fn non_signing(keys: Option<&RoleKeys>, role: Role) -> BTreeSet<String> {
    keys.into_iter()
        .flat_map(|keys| keys.keys.iter())
        .filter(|key| !is_signing(key))
        .flat_map(|key| {
            key.certs.iter().map(|(cert, _)| {
                let name = cert
                    .key_name
                    .as_deref()
                    .map_or(String::new(), |name| format!(" KeyName {name:?}"));
                format!(
                    "{} {} {}{name}",
                    role.cli_word(),
                    use_label(key.key_use.as_deref()),
                    cert.sha256
                )
            })
        })
        .collect()
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
    let mut file_only_keys = Vec::new();
    let mut tenant_only_keys = Vec::new();
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
        // Every signing certificate the file brings is checked the way
        // `cert add` checks its `--cert-file`, before any plan exists: a body
        // that decodes to bytes but not to a certificate would otherwise be
        // published, and count towards the last-signing-certificate guard.
        for key in file_role.keys.iter().filter(|key| is_signing(key)) {
            for (cert, der) in &key.certs {
                if let Err(error) = pem::certificate_from_der(der.clone()) {
                    return Err(Error::Config(format!(
                        "the file's {} signing <KeyDescriptor> at line {} holds a certificate \
                         (sha256 {}) that `aic saml cert add` would refuse: {error}. Refusing \
                         the import; nothing was sent",
                        file_role.descriptor,
                        line_of(file, key.range.start),
                        cert.sha256
                    )));
                }
            }
        }
        let mut signing: SigningCerts = Vec::new();
        for (sha, der) in signing_certs(file_role) {
            if !signing.iter().any(|(held, _)| *held == sha) {
                signing.push((sha, der));
            }
        }
        file_signing.push((role, signing));
    }
    // The union of both sides' roles, not the file's: a role the file omits
    // is left as it is, and its non-signing keys are still named as kept.
    // Every file role is one the entity holds (refused above otherwise), so
    // the entity's roles are that union.
    for role in held.iter().copied().collect::<BTreeSet<_>>() {
        fn find(layout: &KeyLayout, role: Role) -> Option<&RoleKeys> {
            layout.roles.iter().find(|keys| role_of(keys.role) == role)
        }
        let file_keys = non_signing(find(&file_layout, role), role);
        let tenant_keys = non_signing(find(&export_layout, role), role);
        file_only_keys.extend(file_keys.difference(&tenant_keys).cloned());
        tenant_only_keys.extend(tenant_keys.difference(&file_keys).cloned());
    }
    let differences = Differences {
        only_in_file: minus(&file_layout.facts, &export_layout.facts),
        only_on_tenant: minus(&export_layout.facts, &file_layout.facts),
        file_only_keys,
        tenant_only_keys,
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
            comparison.differences.file_only_keys,
            [format!("idp encryption {EC_SHA}")]
        );
    }

    /// Discriminating: the same DER, moved from signing on the tenant to
    /// encryption in the file. A comparison by fingerprint alone finds it on
    /// the tenant and reports nothing; by `(role, use, sha256)` it is a
    /// difference the update will not apply.
    #[test]
    fn a_certificate_moved_from_signing_to_encryption_is_reported_as_not_applied() {
        let export = document(&idp(&[
            (Some("signing"), RSA_DER),
            (Some("signing"), &der(EC_PEM)),
        ]));
        let upload = file(&idp(&[
            (Some("signing"), RSA_DER),
            (Some("encryption"), &der(EC_PEM)),
        ]));
        let comparison = compare_import(&target(&[Role::Idp]), &export, &upload).unwrap();
        assert_eq!(
            comparison.differences.file_only_keys,
            [format!("idp encryption {EC_SHA}")]
        );
        let lines = comparison.differences.lines();
        assert!(
            !lines.contains(&ONLY_CERTS_DIFFER.to_string()),
            "{lines:#?}"
        );
        assert!(
            lines.contains(&format!("  file only    idp encryption {EC_SHA}")),
            "{lines:#?}"
        );
    }

    /// And the other direction: an encryption key the tenant has and the
    /// file lacks is kept, so the file does not "differ only in signing
    /// certificates".
    #[test]
    fn a_tenant_encryption_key_the_file_lacks_is_a_difference_too() {
        let export = document(&idp(&[
            (Some("signing"), RSA_DER),
            (Some("encryption"), &der(EC_PEM)),
        ]));
        let upload = file(&idp(&[(Some("signing"), RSA_DER)]));
        let comparison = compare_import(&target(&[Role::Idp]), &export, &upload).unwrap();
        assert_eq!(
            comparison.differences.tenant_only_keys,
            [format!("idp encryption {EC_SHA}")]
        );
        assert!(comparison.differences.file_only_keys.is_empty());
        assert_ne!(comparison.differences.lines(), [ONLY_CERTS_DIFFER]);
    }

    /// Discriminating: the same encryption certificate, only its `KeyName`
    /// changed. A comparison by `(role, use, sha256)` finds nothing and says
    /// only signing certificates differ; the name is part of the identity.
    /// And the control: the same name under different whitespace — which AM's
    /// re-indenting produces — is no difference at all.
    #[test]
    fn a_key_name_change_is_a_difference_and_its_whitespace_is_not() {
        let slot = "        <SingleSignOnService";
        // The IdP role with a signing key, then an encryption key named
        // `name`, `pad` being the whitespace before the name.
        let with = |name: &str, pad: &str| {
            let key = am_key(Some("encryption"), &der(EC_PEM)).replace(
                "<ds:KeyInfo>",
                &format!("<ds:KeyInfo>{pad}<ds:KeyName>{name}</ds:KeyName>"),
            );
            document(&idp(&[(Some("signing"), RSA_DER)]).replacen(slot, &format!("{key}{slot}"), 1))
        };
        let export = with("enc-1", "\n                ");
        let renamed = with("enc-2", "\n                ");
        let comparison = compare_import(&target(&[Role::Idp]), &export, &renamed).unwrap();
        assert_eq!(
            comparison.differences.file_only_keys,
            [format!("idp encryption {EC_SHA} KeyName \"enc-2\"")]
        );
        assert_eq!(
            comparison.differences.tenant_only_keys,
            [format!("idp encryption {EC_SHA} KeyName \"enc-1\"")]
        );
        assert_ne!(comparison.differences.lines(), [ONLY_CERTS_DIFFER]);

        let reindented = with("enc-1", "\n\t  ");
        assert_ne!(export, reindented, "the control must differ in bytes");
        let comparison = compare_import(&target(&[Role::Idp]), &export, &reindented).unwrap();
        assert_eq!(comparison.differences.lines(), [ONLY_CERTS_DIFFER]);
    }

    /// A role the file does not declare is left alone by AM and by the plan,
    /// but its non-signing keys are still named: without visiting the union of
    /// roles the summary would say only signing certificates differ.
    #[test]
    fn a_role_the_file_omits_still_has_its_encryption_keys_named() {
        let export = document(&format!(
            "{}{}",
            idp(&[(Some("signing"), RSA_DER)]),
            sp(&[
                (Some("signing"), RSA_DER),
                (Some("encryption"), &der(EC_PEM))
            ])
        ));
        let upload = file(&idp(&[(Some("signing"), RSA_DER)]));
        let comparison = compare_import(&target(&[Role::Idp, Role::Sp]), &export, &upload).unwrap();
        assert!(
            comparison
                .differences
                .tenant_only_keys
                .contains(&format!("sp encryption {EC_SHA}")),
            "{:#?}",
            comparison.differences
        );
        assert_ne!(comparison.differences.lines(), [ONLY_CERTS_DIFFER]);
    }

    /// Discriminating: an attribute changed on an element nested inside an
    /// unchanged direct child. A comparison of the child's own attributes and
    /// its text alone finds nothing and says only signing certificates
    /// differ. And the control: the same structure re-indented is no
    /// difference at all.
    #[test]
    fn a_nested_attribute_change_is_reported_and_reindentation_is_not() {
        let acs = |name: &str, gap: &str| {
            let service = format!(
                "        <AttributeConsumingService index=\"0\">{gap}<ServiceName \
                 xml:lang=\"en\">Portal</ServiceName>{gap}<RequestedAttribute \
                 Name=\"{name}\" isRequired=\"true\"/>{gap}</AttributeConsumingService>\n"
            );
            document(&sp(&[(Some("signing"), RSA_DER)]).replace(
                "    </SPSSODescriptor>",
                &format!("{service}    </SPSSODescriptor>"),
            ))
        };
        let export = acs("mail", "\n            ");
        let renamed = acs("email", "\n            ");
        let comparison = compare_import(&target(&[Role::Sp]), &export, &renamed).unwrap();
        let lines = comparison.differences.lines();
        assert!(
            !lines.contains(&ONLY_CERTS_DIFFER.to_string()),
            "{lines:#?}"
        );
        assert!(
            comparison.differences.only_in_file.iter().any(|fact| {
                fact.text.contains("AttributeConsumingService") && fact.text.contains("\"email\"")
            }),
            "{:#?}",
            comparison.differences
        );
        assert!(
            lines.iter().any(|line| line.contains(NOT_APPLIED)),
            "{lines:#?}"
        );

        let reindented = acs("mail", "\n\t ");
        assert_ne!(export, reindented, "the control must differ in bytes");
        let comparison = compare_import(&target(&[Role::Sp]), &export, &reindented).unwrap();
        assert_eq!(comparison.differences.lines(), [ONLY_CERTS_DIFFER]);
    }

    /// `<ds:X509Certificate>AQID</ds:X509Certificate>` is valid base64 for
    /// three bytes and no certificate. Unchecked, `--certs replace` would
    /// swap it in for the real one: it counts as a signing certificate for
    /// the last-one guard and its details just come out blank. The import
    /// applies `cert add`'s check and names where it is.
    #[test]
    fn a_file_certificate_that_is_not_x509_is_refused_before_any_plan() {
        let export = document(&idp(&[(Some("signing"), RSA_DER)]));
        let upload = file(&idp(&[(Some("signing"), &[1, 2, 3])]));
        assert!(
            String::from_utf8(upload.clone())
                .unwrap()
                .contains(">\nAQID\n<"),
            "the fixture must carry the three-byte body"
        );
        let error = compare_import(&target(&[Role::Idp]), &export, &upload)
            .unwrap_err()
            .to_string();
        let line = String::from_utf8(upload.clone())
            .unwrap()
            .lines()
            .position(|line| line.contains("<KeyDescriptor"))
            .unwrap()
            + 1;
        assert!(
            error.contains(&format!("at line {line}"))
                && error.contains("would refuse")
                && error.contains("nothing was sent"),
            "{error}"
        );
        // A real certificate beside it does not rescue it.
        let mixed = file(&idp(&[
            (Some("signing"), RSA_DER),
            (Some("signing"), &[1, 2, 3]),
        ]));
        assert!(compare_import(&target(&[Role::Idp]), &export, &mixed).is_err());
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
