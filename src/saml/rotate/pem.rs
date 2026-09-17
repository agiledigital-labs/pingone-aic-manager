//! Offline validation of the PEM key pair that backs a SAML signing label.
//!
//! An AIC SAML signing certificate is a **version of an ESV secret**, and that
//! secret's value is `cat key.pem cert.pem` — a private key PEM followed by the
//! certificate PEM (`docs/api/03-esvs.md`, `docs/api/06-saml.md`). The tenant
//! will take almost anything: a `pem` secret whose content is not a PEM
//! document at all answers `500 "Failed to create secret"`, and one that *is* a
//! PEM document but carries the wrong key answers **200**. The label then
//! resolves to a key whose certificate the peer holds nothing for, and the
//! failure surfaces as a rejected assertion on the peer, hours later.
//!
//! So the check has to happen here, and it has to be the real check. The value
//! must be **exactly** one private key and one certificate, and the
//! certificate must be the certificate *of that key* — two certificates, a
//! certificate alone, or a key and a stranger's certificate are all refused by
//! name.
//!
//! This deliberately does **not** tighten [`crate::esv::api::encode_secret_value`],
//! whose `pem` arm only looks for `-----BEGIN`. A `pem` ESV secret is not
//! required to be a key pair — a bare certificate is a legal one, and
//! `docs/api/03-esvs.md` created exactly that — so the stronger rule belongs to
//! the caller that actually needs a key pair rather than to every `pem` secret
//! in the tenant.
//!
//! ## Why there is a DER walk in here
//!
//! "The public keys match" is not a property of the armour. It is a property of
//! the modulus and exponent inside both documents, so both have to be parsed:
//! the certificate down to its `subjectPublicKeyInfo`, and the private key down
//! to the `modulus` / `publicExponent` that PKCS#1 and PKCS#8 both carry in
//! clear. The walk is deliberately minimal — it reads structure, never
//! cryptography, and it verifies no signature — which is why it needs no key
//! library and why its fixtures can be hand-built bytes rather than real key
//! material.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use sha2::{Digest, Sha256};

/// `rsaEncryption`, OID 1.2.840.113549.1.1.1, DER content bytes.
///
/// The only algorithm this module can prove a match for. An EC or Ed25519
/// private key does not carry its public key in a place this walk could read
/// without an elliptic-curve implementation, and a check that silently skipped
/// the comparison would be worse than no check: it would report "validated" of
/// a pair it never compared.
const RSA_ENCRYPTION_OID: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01];

const CERTIFICATE_LABEL: &str = "CERTIFICATE";
const PKCS8_LABEL: &str = "PRIVATE KEY";
const PKCS1_LABEL: &str = "RSA PRIVATE KEY";
const ENCRYPTED_LABEL: &str = "ENCRYPTED PRIVATE KEY";

/// Every armour label this module treats as a private key, whether or not it
/// can then read it. An `EC PRIVATE KEY` is recognised so that it is refused
/// as *the wrong algorithm* rather than as *not a private key*.
const PRIVATE_KEY_LABELS: [&str; 4] = [PKCS8_LABEL, PKCS1_LABEL, ENCRYPTED_LABEL, "EC PRIVATE KEY"];

/// Why a proposed ESV secret value is not a usable SAML signing key pair.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeyPairError {
    #[error("the key pair must be text; it is not valid UTF-8 ({0})")]
    NotText(String),
    #[error(
        "the value must be exactly one private key PEM block followed by one certificate PEM \
         block ({0}); build it with `cat key.pem cert.pem`"
    )]
    NotAKeyPair(String),
    #[error(
        "the private key is encrypted (`{ENCRYPTED_LABEL}`); AM has no passphrase to decrypt it. \
         Decrypt it first: `openssl pkcs8 -topk8 -nocrypt -in key.pem -out plain.pem`"
    )]
    EncryptedKey,
    #[error("the `{label}` block on line {line} is not valid base64: {detail}")]
    NotBase64 {
        label: String,
        line: usize,
        detail: String,
    },
    #[error("the certificate is not a readable X.509 document: {0}")]
    BadCertificate(String),
    #[error("the private key is not a readable RSA private key: {0}")]
    BadPrivateKey(String),
    #[error(
        "the certificate's public key algorithm is not RSA, so this command cannot prove the \
         private key and the certificate belong together — and a rotation that published a \
         mismatched pair would only fail on the peer. Map the label by hand if you are sure."
    )]
    UnsupportedAlgorithm,
    #[error(
        "the certificate does not belong to this private key: the certificate's public modulus \
         is not the one the key carries. Check you concatenated the matching `key.pem` and \
         `cert.pem`."
    )]
    Mismatch,
}

impl From<KeyPairError> for crate::Error {
    fn from(error: KeyPairError) -> Self {
        crate::Error::Config(format!("SAML signing key pair: {error}"))
    }
}

/// A validated `cat key.pem cert.pem` value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyPair {
    /// The certificate's DER bytes, as decoded from its armour.
    pub certificate_der: Vec<u8>,
    /// Lowercase hex SHA-256 over those DER bytes.
    ///
    /// The **same identity** [`crate::saml::metadata::CertRef::sha256`] reports
    /// for a certificate in exported metadata, which is what lets a staged
    /// certificate be recognised in the tenant's own export rather than assumed
    /// to be there.
    pub sha256: String,
}

/// Validate a proposed ESV secret value as a SAML signing key pair.
///
/// Refuses anything that is not exactly one private key followed by one
/// certificate, and anything whose two halves do not belong together.
pub fn validate_key_pair(bytes: &[u8]) -> Result<KeyPair, KeyPairError> {
    let text =
        std::str::from_utf8(bytes).map_err(|error| KeyPairError::NotText(error.to_string()))?;
    let blocks = pem_blocks(text);

    if blocks.len() != 2 {
        return Err(KeyPairError::NotAKeyPair(describe(&blocks)));
    }
    let (key_block, cert_block) = (&blocks[0], &blocks[1]);
    if key_block.label == ENCRYPTED_LABEL {
        return Err(KeyPairError::EncryptedKey);
    }
    if !PRIVATE_KEY_LABELS.contains(&key_block.label.as_str())
        || cert_block.label != CERTIFICATE_LABEL
    {
        return Err(KeyPairError::NotAKeyPair(describe(&blocks)));
    }

    let key_der = decode(key_block)?;
    let cert_der = decode(cert_block)?;

    let spki = certificate_spki(&cert_der).map_err(KeyPairError::BadCertificate)?;
    if spki.algorithm_oid != RSA_ENCRYPTION_OID {
        return Err(KeyPairError::UnsupportedAlgorithm);
    }
    let certificate_key = rsa_public_key(&spki.subject_public_key)
        .map_err(|detail| KeyPairError::BadCertificate(format!("subjectPublicKey: {detail}")))?;

    let private_key = match key_block.label.as_str() {
        PKCS1_LABEL => rsa_key_from_pkcs1(&key_der),
        PKCS8_LABEL => rsa_key_from_pkcs8(&key_der),
        // `EC PRIVATE KEY` armour over a certificate whose SPKI says RSA is
        // contradictory, and neither half is trustworthy after that.
        _ => Err("the armour says it is not an RSA key, but the certificate says RSA".to_string()),
    }
    .map_err(KeyPairError::BadPrivateKey)?;

    if private_key != certificate_key {
        return Err(KeyPairError::Mismatch);
    }

    Ok(KeyPair {
        sha256: hex(&Sha256::digest(&cert_der)),
        certificate_der: cert_der,
    })
}

/// The RSA public key both documents carry, as the two integers that define
/// it. Compared as DER content bytes with any leading zero padding removed, so
/// that the same modulus written with and without a leading `00` compares
/// equal — which is the difference between the two encodings this module has
/// to reconcile, not a difference between two keys.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RsaPublicKey {
    modulus: Vec<u8>,
    exponent: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PemBlock {
    label: String,
    body: String,
    /// 1-based line of the `-----BEGIN …-----` marker.
    line: usize,
}

/// Every complete `-----BEGIN X-----` … `-----END X-----` block, in order.
///
/// A block whose end marker names a different label, or that never ends, is
/// not a block: PEM armour that does not close is corruption, and treating a
/// truncated key as a key is how a half-written file becomes a live secret.
fn pem_blocks(text: &str) -> Vec<PemBlock> {
    let mut blocks = Vec::new();
    let mut open: Option<(String, usize, String)> = None;
    for (index, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if let Some(label) = marker(line, "-----BEGIN ") {
            // A second BEGIN inside an unclosed block abandons the first.
            open = Some((label, index + 1, String::new()));
            continue;
        }
        if let Some(label) = marker(line, "-----END ") {
            if let Some((open_label, open_line, body)) = open.take() {
                if open_label == label {
                    blocks.push(PemBlock {
                        label,
                        body,
                        line: open_line,
                    });
                }
            }
            continue;
        }
        if let Some((_, _, body)) = open.as_mut() {
            body.push_str(line);
        }
    }
    blocks
}

fn marker(line: &str, prefix: &str) -> Option<String> {
    Some(
        line.strip_prefix(prefix)?
            .strip_suffix("-----")?
            .trim()
            .to_string(),
    )
}

/// What the value turned out to be, for the refusal message. Operator-facing:
/// "2 CERTIFICATE blocks" is actionable where "invalid key pair" is not.
fn describe(blocks: &[PemBlock]) -> String {
    if blocks.is_empty() {
        return "it contains no complete PEM block".to_string();
    }
    let mut counts: Vec<(String, usize)> = Vec::new();
    for block in blocks {
        match counts.iter_mut().find(|(label, _)| *label == block.label) {
            Some((_, count)) => *count += 1,
            None => counts.push((block.label.clone(), 1)),
        }
    }
    format!(
        "it contains {}",
        counts
            .iter()
            .map(|(label, count)| format!("{count} {label} block(s)"))
            .collect::<Vec<_>>()
            .join(" and ")
    )
}

fn decode(block: &PemBlock) -> Result<Vec<u8>, KeyPairError> {
    B64.decode(&block.body)
        .map_err(|error| KeyPairError::NotBase64 {
            label: block.label.clone(),
            line: block.line,
            detail: error.to_string(),
        })
}

// ---------------------------------------------------------------------------
// The smallest DER reader that answers the question.
// ---------------------------------------------------------------------------

const TAG_INTEGER: u8 = 0x02;
const TAG_BIT_STRING: u8 = 0x03;
const TAG_OCTET_STRING: u8 = 0x04;
const TAG_OID: u8 = 0x06;
const TAG_SEQUENCE: u8 = 0x30;
/// `[0] EXPLICIT`, which is how an X.509 `version` is tagged.
const TAG_CONTEXT_0: u8 = 0xa0;

#[derive(Debug, Clone, Copy)]
struct Tlv<'a> {
    tag: u8,
    value: &'a [u8],
    /// Bytes consumed, tag and length header included.
    consumed: usize,
}

/// Read one tag-length-value triple.
///
/// Definite lengths only: BER's indefinite form (`0x80`) has no place in DER,
/// and accepting it would mean guessing where a value ends.
fn read_tlv(input: &[u8]) -> Result<Tlv<'_>, String> {
    let tag = *input.first().ok_or("truncated: no tag")?;
    let first = *input.get(1).ok_or("truncated: no length")? as usize;
    let (length, header) = if first < 0x80 {
        (first, 2)
    } else if first == 0x80 {
        return Err("indefinite length is not DER".to_string());
    } else {
        let count = first & 0x7f;
        if count > 4 {
            return Err(format!("length of {count} bytes is implausible"));
        }
        let bytes = input.get(2..2 + count).ok_or("truncated: length bytes")?;
        (
            bytes
                .iter()
                .fold(0usize, |acc, byte| (acc << 8) | *byte as usize),
            2 + count,
        )
    };
    let value = input
        .get(header..header + length)
        .ok_or("truncated: value shorter than its length")?;
    Ok(Tlv {
        tag,
        value,
        consumed: header + length,
    })
}

/// Every element of a constructed value, in order.
fn elements(mut input: &[u8]) -> Result<Vec<Tlv<'_>>, String> {
    let mut out = Vec::new();
    while !input.is_empty() {
        let tlv = read_tlv(input)?;
        input = &input[tlv.consumed..];
        out.push(tlv);
    }
    Ok(out)
}

fn expect<'a>(tlv: &Tlv<'a>, tag: u8, what: &str) -> Result<&'a [u8], String> {
    if tlv.tag == tag {
        Ok(tlv.value)
    } else {
        Err(format!(
            "{what}: expected tag 0x{tag:02x}, found 0x{:02x}",
            tlv.tag
        ))
    }
}

struct Spki {
    algorithm_oid: Vec<u8>,
    subject_public_key: Vec<u8>,
}

/// Pull `tbsCertificate.subjectPublicKeyInfo` out of an X.509 certificate.
///
/// ```text
/// Certificate     ::= SEQUENCE { tbsCertificate, signatureAlgorithm, signatureValue }
/// TBSCertificate  ::= SEQUENCE { [0] version DEFAULT v1, serialNumber, signature,
///                                issuer, validity, subject, subjectPublicKeyInfo, … }
/// ```
///
/// `version` is `[0] EXPLICIT` and **optional**, which is the one place a
/// fixed index would be wrong: a v1 certificate omits it and every later field
/// shifts by one.
fn certificate_spki(der: &[u8]) -> Result<Spki, String> {
    let certificate = expect(&read_tlv(der)?, TAG_SEQUENCE, "Certificate")?;
    let tbs_tlv = read_tlv(certificate)?;
    let tbs = expect(&tbs_tlv, TAG_SEQUENCE, "TBSCertificate")?;
    let fields = elements(tbs)?;

    let first_after_version = usize::from(fields.first().map(|tlv| tlv.tag) == Some(TAG_CONTEXT_0));
    // serialNumber, signature, issuer, validity, subject, then the SPKI.
    let spki_index = first_after_version + 5;
    let spki_tlv = fields
        .get(spki_index)
        .ok_or("TBSCertificate ends before subjectPublicKeyInfo")?;
    let spki = expect(spki_tlv, TAG_SEQUENCE, "subjectPublicKeyInfo")?;
    let spki_fields = elements(spki)?;

    let algorithm = spki_fields
        .first()
        .ok_or("subjectPublicKeyInfo has no algorithm")?;
    let algorithm_oid = algorithm_oid(expect(algorithm, TAG_SEQUENCE, "AlgorithmIdentifier")?)?;

    let bits = expect(
        spki_fields
            .get(1)
            .ok_or("subjectPublicKeyInfo has no subjectPublicKey")?,
        TAG_BIT_STRING,
        "subjectPublicKey",
    )?;
    let (unused, key) = bits.split_first().ok_or("subjectPublicKey is empty")?;
    if *unused != 0 {
        return Err(format!("subjectPublicKey has {unused} unused bits"));
    }

    Ok(Spki {
        algorithm_oid,
        subject_public_key: key.to_vec(),
    })
}

fn algorithm_oid(algorithm: &[u8]) -> Result<Vec<u8>, String> {
    let oid = read_tlv(algorithm)?;
    Ok(expect(&oid, TAG_OID, "algorithm OID")?.to_vec())
}

/// `RSAPublicKey ::= SEQUENCE { modulus INTEGER, publicExponent INTEGER }` —
/// the content of an RSA `subjectPublicKey` BIT STRING.
fn rsa_public_key(der: &[u8]) -> Result<RsaPublicKey, String> {
    let sequence = expect(&read_tlv(der)?, TAG_SEQUENCE, "RSAPublicKey")?;
    let fields = elements(sequence)?;
    integers(&fields, 0, "RSAPublicKey")
}

/// `RSAPrivateKey ::= SEQUENCE { version, modulus, publicExponent, … }` —
/// the two public components are in clear, third and second field in.
fn rsa_key_from_pkcs1(der: &[u8]) -> Result<RsaPublicKey, String> {
    let sequence = expect(&read_tlv(der)?, TAG_SEQUENCE, "RSAPrivateKey")?;
    let fields = elements(sequence)?;
    integers(&fields, 1, "RSAPrivateKey")
}

/// `PrivateKeyInfo ::= SEQUENCE { version, privateKeyAlgorithm, privateKey OCTET STRING }`,
/// whose octet string is a PKCS#1 `RSAPrivateKey` when the algorithm is RSA.
fn rsa_key_from_pkcs8(der: &[u8]) -> Result<RsaPublicKey, String> {
    let sequence = expect(&read_tlv(der)?, TAG_SEQUENCE, "PrivateKeyInfo")?;
    let fields = elements(sequence)?;
    let algorithm = fields.get(1).ok_or("PrivateKeyInfo has no algorithm")?;
    let oid = algorithm_oid(expect(algorithm, TAG_SEQUENCE, "privateKeyAlgorithm")?)?;
    if oid != RSA_ENCRYPTION_OID {
        return Err("the private key's algorithm is not rsaEncryption".to_string());
    }
    let inner = expect(
        fields.get(2).ok_or("PrivateKeyInfo has no privateKey")?,
        TAG_OCTET_STRING,
        "privateKey",
    )?;
    rsa_key_from_pkcs1(inner)
}

/// The `modulus` / `publicExponent` pair starting at `offset`.
fn integers(fields: &[Tlv<'_>], offset: usize, what: &str) -> Result<RsaPublicKey, String> {
    let modulus = expect(
        fields.get(offset).ok_or(format!("{what} has no modulus"))?,
        TAG_INTEGER,
        "modulus",
    )?;
    let exponent = expect(
        fields
            .get(offset + 1)
            .ok_or(format!("{what} has no publicExponent"))?,
        TAG_INTEGER,
        "publicExponent",
    )?;
    Ok(RsaPublicKey {
        modulus: unpad(modulus),
        exponent: unpad(exponent),
    })
}

/// Drop a DER INTEGER's sign-padding zero so that two encodings of the same
/// non-negative integer compare equal.
fn unpad(bytes: &[u8]) -> Vec<u8> {
    let start = bytes.iter().take_while(|byte| **byte == 0).count();
    bytes[start..].to_vec()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------
    // A DER writer, so the fixtures are bytes rather than key material.
    //
    // Nothing here signs anything and the walk never verifies a signature,
    // so a certificate for these purposes is exactly its structure — which
    // means the awkward cases (a v1 certificate with no `[0]` version, a
    // modulus written with and without sign padding, an EC algorithm OID)
    // can be built exactly, rather than hoped for from whatever `openssl`
    // happened to emit. It also keeps a private key out of the repository:
    // `RsaPrivateKey::new` is banned under `#[cfg(test)]` (see
    // `lib.rs::repo_hygiene`) and a committed key is banned outright.
    // -----------------------------------------------------------------

    fn tlv(tag: u8, value: &[u8]) -> Vec<u8> {
        let mut out = vec![tag];
        let length = value.len();
        if length < 0x80 {
            out.push(length as u8);
        } else if length < 0x100 {
            out.extend_from_slice(&[0x81, length as u8]);
        } else {
            out.extend_from_slice(&[0x82, (length >> 8) as u8, (length & 0xff) as u8]);
        }
        out.extend_from_slice(value);
        out
    }

    fn seq(parts: &[&[u8]]) -> Vec<u8> {
        tlv(TAG_SEQUENCE, &parts.concat())
    }

    fn int(value: &[u8]) -> Vec<u8> {
        tlv(TAG_INTEGER, value)
    }

    fn oid(content: &[u8]) -> Vec<u8> {
        tlv(TAG_OID, content)
    }

    fn null() -> Vec<u8> {
        vec![0x05, 0x00]
    }

    fn bit_string(content: &[u8]) -> Vec<u8> {
        let mut body = vec![0u8];
        body.extend_from_slice(content);
        tlv(TAG_BIT_STRING, &body)
    }

    /// `id-ecPublicKey`, 1.2.840.10045.2.1 — a real algorithm this module
    /// deliberately cannot check.
    const EC_PUBLIC_KEY_OID: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01];

    fn spki(algorithm: &[u8], modulus: &[u8], exponent: &[u8]) -> Vec<u8> {
        seq(&[
            &seq(&[&oid(algorithm), &null()]),
            &bit_string(&seq(&[&int(modulus), &int(exponent)])),
        ])
    }

    /// A structurally complete certificate whose only load-bearing part is
    /// its `subjectPublicKeyInfo`. `versioned` chooses between a v3
    /// certificate (with the optional `[0] EXPLICIT version`) and a v1 one
    /// (without), which is the case a fixed field index gets wrong.
    fn certificate_with(
        algorithm: &[u8],
        modulus: &[u8],
        exponent: &[u8],
        versioned: bool,
    ) -> Vec<u8> {
        let version = tlv(TAG_CONTEXT_0, &int(&[0x02]));
        let mut tbs_parts: Vec<Vec<u8>> = Vec::new();
        if versioned {
            tbs_parts.push(version);
        }
        tbs_parts.push(int(&[0x01])); // serialNumber
        tbs_parts.push(seq(&[&oid(&[0x2a]), &null()])); // signature
        tbs_parts.push(seq(&[])); // issuer
        tbs_parts.push(seq(&[])); // validity
        tbs_parts.push(seq(&[])); // subject
        tbs_parts.push(spki(algorithm, modulus, exponent));
        let tbs = seq(&tbs_parts.iter().map(Vec::as_slice).collect::<Vec<_>>());
        seq(&[
            &tbs,
            &seq(&[&oid(&[0x2a]), &null()]),
            &bit_string(&[0xde, 0xad]),
        ])
    }

    fn certificate(modulus: &[u8], exponent: &[u8]) -> Vec<u8> {
        certificate_with(RSA_ENCRYPTION_OID, modulus, exponent, true)
    }

    fn pkcs1(modulus: &[u8], exponent: &[u8]) -> Vec<u8> {
        seq(&[
            &int(&[0x00]),
            &int(modulus),
            &int(exponent),
            &int(&[0x07, 0x07]), // privateExponent; never read
        ])
    }

    fn pkcs8(modulus: &[u8], exponent: &[u8]) -> Vec<u8> {
        seq(&[
            &int(&[0x00]),
            &seq(&[&oid(RSA_ENCRYPTION_OID), &null()]),
            &tlv(TAG_OCTET_STRING, &pkcs1(modulus, exponent)),
        ])
    }

    fn armour(label: &str, der: &[u8]) -> String {
        let encoded = B64.encode(der);
        let mut out = format!("-----BEGIN {label}-----\n");
        for chunk in encoded.as_bytes().chunks(64) {
            out.push_str(std::str::from_utf8(chunk).expect("base64 is ascii"));
            out.push('\n');
        }
        out.push_str(&format!("-----END {label}-----\n"));
        out
    }

    /// A modulus wide enough that the sign-padding case is not degenerate:
    /// its top byte has the high bit set, so a real encoder writes it with a
    /// leading zero.
    const MODULUS: &[u8] = &[0x00, 0xc1, 0x5a, 0x77, 0x3f, 0x01];
    const OTHER_MODULUS: &[u8] = &[0x00, 0xc1, 0x5a, 0x77, 0x3f, 0x02];
    const EXPONENT: &[u8] = &[0x01, 0x00, 0x01];

    fn matching_pair() -> String {
        format!(
            "{}{}",
            armour(PKCS8_LABEL, &pkcs8(MODULUS, EXPONENT)),
            armour(CERTIFICATE_LABEL, &certificate(MODULUS, EXPONENT))
        )
    }

    #[test]
    fn a_matching_pkcs8_pair_validates_and_fingerprints_the_certificate() {
        let pair = validate_key_pair(matching_pair().as_bytes()).expect("a matching pair");
        let der = certificate(MODULUS, EXPONENT);
        assert_eq!(pair.certificate_der, der);
        assert_eq!(pair.sha256, hex(&Sha256::digest(&der)));
        // The fingerprint is over the DER, not over the armour: a digest of
        // the base64 text would be the same length and just as hex-looking.
        assert_ne!(
            pair.sha256,
            hex(&Sha256::digest(B64.encode(&der).as_bytes()))
        );
    }

    #[test]
    fn a_matching_pkcs1_pair_validates() {
        let value = format!(
            "{}{}",
            armour(PKCS1_LABEL, &pkcs1(MODULUS, EXPONENT)),
            armour(CERTIFICATE_LABEL, &certificate(MODULUS, EXPONENT))
        );
        assert!(validate_key_pair(value.as_bytes()).is_ok());
    }

    #[test]
    fn sign_padding_is_not_a_different_key() {
        // The certificate writes the modulus with the leading zero DER
        // requires; the private key here writes it without. Same integer.
        let unpadded = &MODULUS[1..];
        let value = format!(
            "{}{}",
            armour(PKCS8_LABEL, &pkcs8(unpadded, EXPONENT)),
            armour(CERTIFICATE_LABEL, &certificate(MODULUS, EXPONENT))
        );
        assert!(validate_key_pair(value.as_bytes()).is_ok());
    }

    #[test]
    fn a_v1_certificate_has_no_version_field_and_still_resolves_its_public_key() {
        // The one case a fixed field index gets wrong: without the optional
        // `[0] EXPLICIT version`, every later field sits one place earlier,
        // so a walk that always skipped index 0 would read `validity` as the
        // subjectPublicKeyInfo.
        let value = format!(
            "{}{}",
            armour(PKCS8_LABEL, &pkcs8(MODULUS, EXPONENT)),
            armour(
                CERTIFICATE_LABEL,
                &certificate_with(RSA_ENCRYPTION_OID, MODULUS, EXPONENT, false)
            )
        );
        assert!(validate_key_pair(value.as_bytes()).is_ok());
    }

    #[test]
    fn a_certificate_for_a_different_key_is_refused() {
        let value = format!(
            "{}{}",
            armour(PKCS8_LABEL, &pkcs8(MODULUS, EXPONENT)),
            armour(CERTIFICATE_LABEL, &certificate(OTHER_MODULUS, EXPONENT))
        );
        assert_eq!(
            validate_key_pair(value.as_bytes()).unwrap_err(),
            KeyPairError::Mismatch
        );
    }

    #[test]
    fn a_matching_modulus_with_a_different_exponent_is_refused() {
        // Discriminating against a comparison that stops at the modulus:
        // the moduli here are identical and only the public exponent moves.
        let value = format!(
            "{}{}",
            armour(PKCS8_LABEL, &pkcs8(MODULUS, EXPONENT)),
            armour(CERTIFICATE_LABEL, &certificate(MODULUS, &[0x03]))
        );
        assert_eq!(
            validate_key_pair(value.as_bytes()).unwrap_err(),
            KeyPairError::Mismatch
        );
    }

    #[test]
    fn shapes_that_are_not_one_key_and_one_certificate_are_refused() {
        let key = armour(PKCS8_LABEL, &pkcs8(MODULUS, EXPONENT));
        let cert = armour(CERTIFICATE_LABEL, &certificate(MODULUS, EXPONENT));
        let cases: Vec<(&str, String)> = vec![
            ("two certificates", format!("{cert}{cert}")),
            ("a certificate alone", cert.clone()),
            ("a key alone", key.clone()),
            ("certificate before key", format!("{cert}{key}")),
            (
                "a key, a certificate and a spare",
                format!("{key}{cert}{cert}"),
            ),
            ("no PEM at all", "just some text\n".to_string()),
            (
                "an unterminated key",
                key.replace(&format!("-----END {PKCS8_LABEL}-----"), ""),
            ),
            (
                "an END naming a different label",
                format!(
                    "{}{cert}",
                    key.replace(
                        &format!("-----END {PKCS8_LABEL}-----"),
                        "-----END CERTIFICATE-----"
                    )
                ),
            ),
        ];
        for (what, value) in cases {
            assert!(
                matches!(
                    validate_key_pair(value.as_bytes()),
                    Err(KeyPairError::NotAKeyPair(_))
                ),
                "{what} should not be a key pair, got {:?}",
                validate_key_pair(value.as_bytes())
            );
        }
    }

    #[test]
    fn an_encrypted_private_key_is_named_rather_than_called_malformed() {
        let value = format!(
            "{}{}",
            armour(ENCRYPTED_LABEL, &[0x30, 0x00]),
            armour(CERTIFICATE_LABEL, &certificate(MODULUS, EXPONENT))
        );
        assert_eq!(
            validate_key_pair(value.as_bytes()).unwrap_err(),
            KeyPairError::EncryptedKey
        );
    }

    #[test]
    fn a_non_rsa_certificate_is_refused_rather_than_passed_unchecked() {
        let value = format!(
            "{}{}",
            armour("EC PRIVATE KEY", &[0x30, 0x00]),
            armour(
                CERTIFICATE_LABEL,
                &certificate_with(EC_PUBLIC_KEY_OID, MODULUS, EXPONENT, true)
            )
        );
        assert_eq!(
            validate_key_pair(value.as_bytes()).unwrap_err(),
            KeyPairError::UnsupportedAlgorithm
        );
    }

    #[test]
    fn unreadable_der_is_refused_by_the_half_it_came_from() {
        let key = armour(PKCS8_LABEL, &pkcs8(MODULUS, EXPONENT));
        let cert = armour(CERTIFICATE_LABEL, &certificate(MODULUS, EXPONENT));

        // A certificate truncated mid-value: the outer length outruns the
        // bytes, which is exactly what a half-written file looks like.
        let mut short = certificate(MODULUS, EXPONENT);
        short.truncate(short.len() - 8);
        assert!(matches!(
            validate_key_pair(format!("{key}{}", armour(CERTIFICATE_LABEL, &short)).as_bytes()),
            Err(KeyPairError::BadCertificate(_))
        ));

        // A private key whose armour claims PKCS#8 over something that is not.
        assert!(matches!(
            validate_key_pair(format!("{}{cert}", armour(PKCS8_LABEL, &int(&[0x01]))).as_bytes()),
            Err(KeyPairError::BadPrivateKey(_))
        ));

        // A PKCS#8 wrapper whose own algorithm is not RSA, under an RSA
        // certificate — contradictory, and the key half is what is wrong.
        let ec_pkcs8 = seq(&[
            &int(&[0x00]),
            &seq(&[&oid(EC_PUBLIC_KEY_OID), &null()]),
            &tlv(TAG_OCTET_STRING, &[0x30, 0x00]),
        ]);
        assert!(matches!(
            validate_key_pair(format!("{}{cert}", armour(PKCS8_LABEL, &ec_pkcs8)).as_bytes()),
            Err(KeyPairError::BadPrivateKey(_))
        ));
    }

    #[test]
    fn a_block_body_that_is_not_base64_names_the_block() {
        let value = format!(
            "-----BEGIN {PKCS8_LABEL}-----\nnot!base64!\n-----END {PKCS8_LABEL}-----\n{}",
            armour(CERTIFICATE_LABEL, &certificate(MODULUS, EXPONENT))
        );
        match validate_key_pair(value.as_bytes()).unwrap_err() {
            KeyPairError::NotBase64 { label, line, .. } => {
                assert_eq!(label, PKCS8_LABEL);
                assert_eq!(line, 1);
            }
            other => panic!("expected a base64 complaint, got {other:?}"),
        }
    }

    #[test]
    fn a_value_that_is_not_text_is_refused_before_anything_else() {
        assert!(matches!(
            validate_key_pair(&[0xff, 0xfe, 0x00]),
            Err(KeyPairError::NotText(_))
        ));
    }

    #[test]
    fn der_lengths_that_are_not_definite_are_refused() {
        // BER's indefinite form would make "where does this value end" a
        // guess, and a guess here reads a neighbouring field as a modulus.
        assert!(read_tlv(&[0x30, 0x80, 0x00, 0x00]).is_err());
        assert!(read_tlv(&[0x30, 0x85, 0x01, 0x02, 0x03, 0x04, 0x05]).is_err());
        // Long form, correctly read.
        let long = tlv(TAG_OCTET_STRING, &[0u8; 200]);
        assert_eq!(read_tlv(&long).unwrap().value.len(), 200);
    }
}
