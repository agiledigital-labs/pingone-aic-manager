//! Metadata documents for the cert tests, written the way AM's export writes
//! them, so the splicer is tested against the layout it will meet.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;

use crate::saml::cert::spec::{CertPlan, Planned, Target};
use crate::saml::metadata;
use crate::saml::pem;
use crate::saml::spec::{Role, SIGNING_USE};

pub const RSA_DER: &[u8] = include_bytes!("../fixtures/cert-rsa.der");
pub const RSA_SHA: &str = "353f92d3903677e8efd96420323c35de7f7961c088e9b6f2c1828fc3c3ba8447";
pub const PREFIX_PEM: &[u8] = include_bytes!("../fixtures/cert-prefix.crt");
pub const PREFIX_SHA: &str = "35897af3b7fba63b3d5fab862b9447522979469562133aced02038fc4d2447c6";
pub const EC_PEM: &[u8] = include_bytes!("../fixtures/cert-ec.crt");
pub const EC_SHA: &str = "7f3ce2e437673b639c20bc54bc9a152a248b36280a3a32f29a02a6d2066600d9";
pub const ENTITY: &str = "https://sts.windows.net/00000000-0000-0000-0000-000000000000/";

pub fn der(pem_bytes: &[u8]) -> Vec<u8> {
    pem::read_certificate(pem_bytes).unwrap().der
}

/// A key descriptor the way AM's export writes one: unprefixed, `ds:`
/// declared on the root, base64 wrapped at 76 columns, 4-space indent.
pub fn am_key(key_use: Option<&str>, der: &[u8]) -> String {
    let body = STANDARD.encode(der);
    let wrapped = body
        .as_bytes()
        .chunks(76)
        .map(|chunk| std::str::from_utf8(chunk).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    let use_attr = key_use.map_or(String::new(), |key_use| format!(" use=\"{key_use}\""));
    format!(
        "        <KeyDescriptor{use_attr}>\n            <ds:KeyInfo>\n                \
         <ds:X509Data>\n                    <ds:X509Certificate>\n{wrapped}\n\
         </ds:X509Certificate>\n                </ds:X509Data>\n            </ds:KeyInfo>\n\
         \x20       </KeyDescriptor>\n"
    )
}

pub fn idp(keys: &[(Option<&str>, &[u8])]) -> String {
    format!(
        "    <IDPSSODescriptor protocolSupportEnumeration=\"urn:oasis:names:tc:SAML:2.0:protocol\">\n\
         {}        <SingleSignOnService Binding=\"urn:oasis:names:tc:SAML:2.0:bindings:HTTP-Redirect\" \
         Location=\"https://login.example.com/saml2\"/>\n    </IDPSSODescriptor>\n",
        keys.iter()
            .map(|(key_use, der)| am_key(*key_use, der))
            .collect::<String>()
    )
}

pub fn sp(keys: &[(Option<&str>, &[u8])]) -> String {
    format!(
        "    <SPSSODescriptor protocolSupportEnumeration=\"urn:oasis:names:tc:SAML:2.0:protocol\">\n\
         {}        <AssertionConsumerService index=\"0\" Binding=\"urn:oasis:names:tc:SAML:2.0:bindings:HTTP-POST\" \
         Location=\"https://login.example.com/acs\"/>\n    </SPSSODescriptor>\n",
        keys.iter()
            .map(|(key_use, der)| am_key(*key_use, der))
            .collect::<String>()
    )
}

pub fn document(roles: &str) -> Vec<u8> {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<EntityDescriptor \
         xmlns=\"urn:oasis:names:tc:SAML:2.0:metadata\" \
         xmlns:ds=\"http://www.w3.org/2000/09/xmldsig#\"\n  \
         entityID=\"{ENTITY}\">\n{roles}\
         </EntityDescriptor>\n"
    )
    .into_bytes()
}

pub fn target(roles: &[Role]) -> Target {
    Target {
        entity_id: ENTITY.into(),
        realm: "alpha".into(),
        roles: roles.to_vec(),
    }
}

pub fn change(planned: Planned) -> CertPlan {
    match planned {
        Planned::Change(plan) => plan,
        Planned::NoChange(sentence) => panic!("expected a change, got: {sentence}"),
    }
}

pub fn signing_of(xml: &[u8], descriptor: &str) -> Vec<String> {
    metadata::cert_refs(xml)
        .unwrap()
        .into_iter()
        .filter(|cert| {
            cert.descriptor == descriptor && cert.key_use.as_deref() == Some(SIGNING_USE)
        })
        .map(|cert| cert.sha256)
        .collect()
}
