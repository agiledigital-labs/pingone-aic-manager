//! Offline transforms over SAML 2.0 metadata XML.
//!
//! Three jobs, no network and no tenant: describe a document ([`inspect`]),
//! strip what an AM import cannot take and say exactly what went
//! ([`sanitise`]), and fingerprint the certificates it carries
//! ([`cert_refs`]).
//!
//! **The parser is a scanner, not a serialiser.** It walks events to collect
//! the byte ranges of the subtrees to drop, then splices the *original*
//! buffer. Every untouched byte survives exactly, which is what makes the
//! committed fixture pair reviewable as a diff, and it sidesteps the whole
//! class of attribute-quoting, self-closing-form and entity-escaping damage a
//! parse-and-reserialise round trip would risk on a document someone else
//! signed.
//!
//! **Parse failure is refusal, never pass-through** — the same philosophy as
//! `src/pullguard.rs`. A document we could not read is a document we cannot
//! promise anything about.

use std::fmt;
use std::ops::Range;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use quick_xml::NsReader;
use quick_xml::XmlVersion;
use quick_xml::escape::unescape;
use quick_xml::events::{BytesStart, Event};
use quick_xml::name::{Namespace, ResolveResult};
use serde::Serialize;
use sha2::{Digest, Sha256};

type Result<T> = std::result::Result<T, MetadataError>;

/// SAML 2.0 metadata. Root, roles, and key descriptors live here.
const SAML_NS: &[u8] = b"urn:oasis:names:tc:SAML:2.0:metadata";
/// XML Signature. A `Signature` in any other namespace is not an enveloped
/// signature over this document.
const DSIG_NS: &[u8] = b"http://www.w3.org/2000/09/xmldsig#";
/// WS-Federation. Entra's extra roles are in this namespace, not SAML's.
///
/// **Not verified against AM's importer.** A live import is what can promote
/// this to a claim in `docs/api/06-saml.md`. Matching the namespace (not the
/// prefix, not the local name alone) is what stops `ext:RoleDescriptor` in
/// some other vocabulary from being deleted.
const WSFED_NS: &[u8] = b"http://docs.oasis-open.org/wsfed/federation/200706";

/// The XML-Signature element. Conditional on `--keep-signature`, and only
/// removed when some *other* cut would change the signed bytes.
const SIGNATURE_LOCAL_NAME: &str = "Signature";

/// The one root element this module accepts. `EntitiesDescriptor` — an
/// aggregate of several entities — is legal SAML metadata and is refused
/// rather than half-handled.
const ROOT_LOCAL_NAME: &str = "EntityDescriptor";

#[derive(Debug, thiserror::Error)]
pub enum MetadataError {
    #[error("not well-formed XML at byte {offset}: {detail}")]
    Malformed { offset: usize, detail: String },
    #[error("the document has no elements")]
    Empty,
    #[error("expected a <{ROOT_LOCAL_NAME}> root element, found <{root}>")]
    NotEntityMetadata { root: String },
    #[error("<{ROOT_LOCAL_NAME}> carries no entityID attribute")]
    NoEntityId,
    #[error("<X509Certificate> on line {line} is not valid base64: {detail}")]
    BadCertificate { line: usize, detail: String },
}

impl From<MetadataError> for crate::Error {
    fn from(error: MetadataError) -> Self {
        crate::Error::Config(format!("SAML metadata error: {error}"))
    }
}

/// A SAML 2.0 role this tenant understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Role {
    IdentityProvider,
    ServiceProvider,
}

impl Role {
    fn from_expanded_name(name: &str, ns: Option<&[u8]>) -> Option<Self> {
        if ns != Some(SAML_NS) {
            return None;
        }
        match name {
            "IDPSSODescriptor" => Some(Self::IdentityProvider),
            "SPSSODescriptor" => Some(Self::ServiceProvider),
            _ => None,
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IdentityProvider => f.write_str("identityProvider"),
            Self::ServiceProvider => f.write_str("serviceProvider"),
        }
    }
}

/// One protocol endpoint. Recognised structurally — any element carrying both
/// `Binding` and `Location` — rather than from a list of element names, so a
/// role we have never seen still reports its endpoints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Endpoint {
    /// The role descriptor it sits under, for example `IDPSSODescriptor`.
    pub descriptor: Option<String>,
    /// The endpoint element's local name, for example `SingleSignOnService`.
    pub kind: String,
    pub binding: String,
    pub location: String,
}

/// A certificate the entity publishes.
///
/// Scoped to `<KeyDescriptor>`: a certificate inside an enveloped signature
/// identifies whoever signed the document, not a key the entity uses, and
/// `use` is only defined on a key descriptor in the first place.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CertRef {
    /// The enclosing `<KeyDescriptor use="…">`. Absent means the key serves
    /// both signing and encryption, which the metadata schema allows.
    pub key_use: Option<String>,
    pub key_name: Option<String>,
    /// Lowercase hex SHA-256 over the DER bytes — the digest
    /// `openssl x509 -fingerprint -sha256` prints, minus the colons.
    pub sha256: String,
}

/// Why an element was removed. Two reasons, two remedies, and an operator
/// staring at a failed import needs to tell them apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RemovalReason {
    /// On [`STRIP_LOCAL_NAMES`].
    UnsupportedRole,
    /// An enveloped signature over content this rewrite changes.
    EnvelopedSignature,
}

impl fmt::Display for RemovalReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedRole => f.write_str("not a SAML 2.0 role; on the strip list"),
            Self::EnvelopedSignature => {
                f.write_str("enveloped signature; it would not cover the bytes we emit")
            }
        }
    }
}

/// One entry of the removal report. Operator-facing: it is the only record
/// that a document was changed, so it names the element as written in the
/// file, where it was, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Removal {
    /// The name as it appears in the file, prefix and all — what an operator
    /// greps for.
    pub element: String,
    /// The prefix-stripped name the strip list matched on. Entra authors the
    /// prefix, so `fed:` is not a constant.
    pub local_name: String,
    /// `xsi:type`, the only thing telling Entra's two WS-Federation roles
    /// apart.
    pub xsi_type: Option<String>,
    /// 1-based line of the element's start tag in the input.
    pub line: usize,
    pub reason: RemovalReason,
}

impl fmt::Display for Removal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: removed <{}", self.line, self.element)?;
        if let Some(xsi_type) = &self.xsi_type {
            write!(f, " xsi:type=\"{xsi_type}\"")?;
        }
        write!(f, "> — {}", self.reason)
    }
}

/// What a metadata document says about itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MetadataDoc {
    pub entity_id: String,
    pub roles: Vec<Role>,
    /// Whether the document carries an enveloped signature at all — a fact
    /// about the document, separate from whether we would remove it.
    pub signed: bool,
    pub endpoints: Vec<Endpoint>,
    pub certs: Vec<CertRef>,
    /// What [`sanitise`] would take out at default options, so the damage can
    /// be read before it is done.
    pub would_remove: Vec<Removal>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SanitiseOpts {
    /// Keep an enveloped signature that no longer matches the content. Only
    /// useful for proving what a rejection was actually caused by.
    pub keep_signature: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sanitised {
    pub bytes: Vec<u8>,
    pub removed: Vec<Removal>,
    /// A signature was kept over content that changed, so the document now
    /// carries a signature that cannot verify. Exactly the state the default
    /// exists to avoid.
    pub stale_signature: bool,
}

/// Describe a metadata document.
pub fn inspect(xml: &[u8]) -> Result<MetadataDoc> {
    let scan = scan(xml)?;
    Ok(MetadataDoc {
        entity_id: scan.entity_id,
        roles: scan.roles,
        signed: scan.signed,
        endpoints: scan.endpoints,
        certs: scan.certs,
        would_remove: cuts_for_opts(scan.cuts, SanitiseOpts::default())
            .into_iter()
            .map(|cut| cut.removal)
            .collect::<Vec<_>>(),
    })
}

/// Remove what an AM import cannot take, and report every removal.
///
/// The report is not decoration: it is the difference between "we stripped X
/// and the import still failed" and an unexplained rejection, so it is
/// produced even when nothing is printed.
pub fn sanitise(xml: &[u8], opts: SanitiseOpts) -> Result<Sanitised> {
    let scan = scan(xml)?;
    let kept_signature = opts.keep_signature && scan.signed;
    let cuts = cuts_for_opts(scan.cuts, opts);
    let changed = cuts
        .iter()
        .any(|cut| cut.removal.reason != RemovalReason::EnvelopedSignature);
    let (ranges, removed): (Vec<_>, Vec<_>) =
        cuts.into_iter().map(|cut| (cut.range, cut.removal)).unzip();
    Ok(Sanitised {
        bytes: splice(xml, &ranges),
        stale_signature: kept_signature && changed,
        removed,
    })
}

/// Default sanitise drops an enveloped signature only when some other cut
/// would change the signed bytes. `--keep-signature` drops signature cuts
/// always, and sets [`Sanitised::stale_signature`] when content still changes.
fn cuts_for_opts(cuts: Vec<Cut>, opts: SanitiseOpts) -> Vec<Cut> {
    if opts.keep_signature {
        return cuts
            .into_iter()
            .filter(|cut| cut.removal.reason != RemovalReason::EnvelopedSignature)
            .collect();
    }
    if cuts
        .iter()
        .any(|cut| cut.removal.reason != RemovalReason::EnvelopedSignature)
    {
        cuts
    } else {
        cuts.into_iter()
            .filter(|cut| cut.removal.reason != RemovalReason::EnvelopedSignature)
            .collect()
    }
}

/// Fingerprint the certificates a document publishes.
///
/// Returns a `Result` rather than the bare `Vec` the slice brief named: an
/// unreadable document must not answer "no certificates".
pub fn cert_refs(xml: &[u8]) -> Result<Vec<CertRef>> {
    Ok(scan(xml)?.certs)
}

// ── the scanner ─────────────────────────────────────────────────────────────

struct Cut {
    range: Range<usize>,
    removal: Removal,
}

struct Scan {
    entity_id: String,
    roles: Vec<Role>,
    signed: bool,
    endpoints: Vec<Endpoint>,
    certs: Vec<CertRef>,
    cuts: Vec<Cut>,
}

/// A `<KeyDescriptor>` being read. Certificates are flushed at its end tag so
/// a `<KeyName>` that follows the certificate is still picked up.
struct PendingKey {
    key_use: Option<String>,
    key_name: Option<String>,
    depth: usize,
    certs: Vec<(usize, String)>,
}

fn scan(xml: &[u8]) -> Result<Scan> {
    std::str::from_utf8(xml).map_err(|error| MetadataError::Malformed {
        offset: error.valid_up_to(),
        detail: "input is not valid UTF-8".into(),
    })?;

    let mut reader = NsReader::from_reader(xml);
    reader.config_mut().check_end_names = true;
    reader.config_mut().check_comments = true;
    reader.config_mut().expand_empty_elements = false;

    let mut path: Vec<String> = Vec::new();
    let mut open_cut: Option<(usize, usize, Removal)> = None;
    let mut pending_key: Option<PendingKey> = None;
    let mut entity_id: Option<String> = None;
    let mut root_seen = false;

    let mut scan = Scan {
        entity_id: String::new(),
        roles: Vec::new(),
        signed: false,
        endpoints: Vec::new(),
        certs: Vec::new(),
        cuts: Vec::new(),
    };

    loop {
        let start = position(&reader);
        let (resolved, event) =
            reader
                .read_resolved_event()
                .map_err(|error| MetadataError::Malformed {
                    offset: start,
                    detail: error.to_string(),
                })?;
        let ns = match resolved {
            ResolveResult::Bound(Namespace(uri)) => Some(uri.to_vec()),
            ResolveResult::Unknown(prefix) => {
                return Err(MetadataError::Malformed {
                    offset: start,
                    detail: format!(
                        "unknown namespace prefix '{}'",
                        String::from_utf8_lossy(&prefix)
                    ),
                });
            }
            ResolveResult::Unbound => None,
        };
        let end = position(&reader);
        let ns = ns.as_deref();

        match event {
            Event::Decl(decl) => {
                if let Some(enc) = decl.encoding() {
                    let enc = enc.map_err(|error| MetadataError::Malformed {
                        offset: start,
                        detail: error.to_string(),
                    })?;
                    if !enc.eq_ignore_ascii_case(b"utf-8") && !enc.eq_ignore_ascii_case(b"utf8") {
                        return Err(MetadataError::Malformed {
                            offset: start,
                            detail: format!(
                                "unsupported encoding '{}'; only UTF-8 is accepted",
                                String::from_utf8_lossy(&enc)
                            ),
                        });
                    }
                }
            }
            Event::Start(element) | Event::Empty(element) => {
                let empty = end > start && xml[start..end].ends_with(b"/>");
                let name = local_name(&element);

                if root_seen && path.is_empty() {
                    return Err(trailing(start, &name));
                }

                if !root_seen {
                    root_seen = true;
                    if name != ROOT_LOCAL_NAME || ns != Some(SAML_NS) {
                        return Err(MetadataError::NotEntityMetadata { root: name });
                    }
                    entity_id = attribute(&element, "entityID", start)?;
                }

                if let Some(role) = Role::from_expanded_name(&name, ns) {
                    if !scan.roles.contains(&role) {
                        scan.roles.push(role);
                    }
                }
                if name == SIGNATURE_LOCAL_NAME && ns == Some(DSIG_NS) {
                    scan.signed = true;
                }
                if let (Some(binding), Some(location)) = (
                    attribute(&element, "Binding", start)?,
                    attribute(&element, "Location", start)?,
                ) {
                    scan.endpoints.push(Endpoint {
                        descriptor: enclosing_descriptor(&path),
                        kind: name.clone(),
                        binding,
                        location,
                    });
                }
                if name == "KeyDescriptor" && ns == Some(SAML_NS) {
                    pending_key = Some(PendingKey {
                        key_use: attribute(&element, "use", start)?,
                        key_name: None,
                        depth: path.len(),
                        certs: Vec::new(),
                    });
                }

                // Only the outermost match is recorded: a strip-list element
                // nested inside one already being removed goes with it, and
                // reporting it separately would tell an operator about bytes
                // that were never theirs to keep.
                if open_cut.is_none()
                    && let Some(reason) = strip_reason(&name, ns)
                {
                    let removal = Removal {
                        element: qualified_name(&element),
                        local_name: name.clone(),
                        xsi_type: attribute(&element, "type", start)?,
                        line: line_at(xml, start),
                        reason,
                    };
                    let from = trim_back(xml, start);
                    if empty {
                        scan.cuts.push(Cut {
                            range: from..end,
                            removal,
                        });
                    } else {
                        open_cut = Some((from, path.len(), removal));
                    }
                }

                if !empty {
                    path.push(name);
                }
            }
            Event::End(_) => {
                let closed = path.pop();
                if let Some(key) = pending_key.take_if(|key| {
                    key.depth == path.len() && closed.as_deref() == Some("KeyDescriptor")
                }) {
                    for (line, body) in &key.certs {
                        scan.certs.push(CertRef {
                            key_use: key.key_use.clone(),
                            key_name: key.key_name.clone(),
                            sha256: fingerprint(body, *line)?,
                        });
                    }
                }
                if let Some((from, _, removal)) =
                    open_cut.take_if(|(_, depth, _)| *depth == path.len())
                {
                    scan.cuts.push(Cut {
                        range: from..end,
                        removal,
                    });
                }
            }
            Event::Text(text) => {
                if root_seen && path.is_empty() {
                    let raw = text.decode().map_err(|error| MetadataError::Malformed {
                        offset: start,
                        detail: error.to_string(),
                    })?;
                    if !raw.chars().all(char::is_whitespace) {
                        return Err(trailing(start, "text"));
                    }
                }
                if let Some(key) = &mut pending_key {
                    let raw = text.decode().map_err(|error| MetadataError::Malformed {
                        offset: start,
                        detail: error.to_string(),
                    })?;
                    let body = unescape(&raw)
                        .map_err(|error| MetadataError::Malformed {
                            offset: start,
                            detail: error.to_string(),
                        })?
                        .into_owned();
                    match path.last().map(String::as_str) {
                        Some("X509Certificate") => key.certs.push((line_at(xml, start), body)),
                        Some("KeyName") => key.key_name = Some(body.trim().to_string()),
                        _ => {}
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }

    if !root_seen {
        return Err(MetadataError::Empty);
    }
    if let Some(name) = path.last() {
        return Err(MetadataError::Malformed {
            offset: xml.len(),
            detail: format!("unclosed <{name}>"),
        });
    }
    scan.entity_id = entity_id.ok_or(MetadataError::NoEntityId)?;
    Ok(scan)
}

fn trailing(offset: usize, what: &str) -> MetadataError {
    MetadataError::Malformed {
        offset,
        detail: format!("trailing {what} after the root element"),
    }
}

fn strip_reason(local_name: &str, ns: Option<&[u8]>) -> Option<RemovalReason> {
    if local_name == SIGNATURE_LOCAL_NAME && ns == Some(DSIG_NS) {
        return Some(RemovalReason::EnvelopedSignature);
    }
    if local_name == "RoleDescriptor" && ns == Some(WSFED_NS) {
        return Some(RemovalReason::UnsupportedRole);
    }
    None
}

/// The nearest enclosing role descriptor. `EntityDescriptor` is the document
/// itself and `KeyDescriptor` is a key, so neither owns an endpoint.
fn enclosing_descriptor(path: &[String]) -> Option<String> {
    path.iter()
        .rev()
        .find(|name| {
            name.ends_with("Descriptor") && *name != ROOT_LOCAL_NAME && *name != "KeyDescriptor"
        })
        .cloned()
}

fn position(reader: &NsReader<&[u8]>) -> usize {
    // The input is an in-memory slice, so its length fits a usize by
    // construction.
    reader.buffer_position() as usize
}

/// The element name with any namespace prefix stripped. Entra authors the
/// `fed:` prefix and it varies between tenants, so matching the qualified name
/// would be a bug that only shows up on someone else's document.
fn local_name(element: &BytesStart<'_>) -> String {
    String::from_utf8_lossy(element.local_name().as_ref()).into_owned()
}

fn qualified_name(element: &BytesStart<'_>) -> String {
    String::from_utf8_lossy(element.name().as_ref()).into_owned()
}

/// One attribute, matched on its local name so `xsi:type` and a bare `type`
/// read the same. Malformed attributes are a parse failure, not an absent
/// value.
fn attribute(element: &BytesStart<'_>, local: &str, offset: usize) -> Result<Option<String>> {
    for attribute in element.attributes() {
        let attribute = attribute.map_err(|error| MetadataError::Malformed {
            offset,
            detail: error.to_string(),
        })?;
        if attribute.key.local_name().as_ref() != local.as_bytes() {
            continue;
        }
        let value = attribute
            .normalized_value(XmlVersion::default())
            .map_err(|error| MetadataError::Malformed {
                offset,
                detail: error.to_string(),
            })?;
        return Ok(Some(value.into_owned()));
    }
    Ok(None)
}

/// SHA-256 over the DER the base64 body decodes to. AM wraps its exports at
/// 64 columns and Entra emits one long line, so whitespace is stripped first.
fn fingerprint(body: &str, line: usize) -> Result<String> {
    let packed = body
        .chars()
        .filter(|character| !character.is_ascii_whitespace())
        .collect::<String>();
    let der = STANDARD
        .decode(&packed)
        .map_err(|error| MetadataError::BadCertificate {
            line,
            detail: error.to_string(),
        })?;
    Ok(hex(&Sha256::digest(&der)))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn line_at(xml: &[u8], offset: usize) -> usize {
    1 + xml[..offset].iter().filter(|byte| **byte == b'\n').count()
}

/// Extend a cut backwards over the indentation on its own line, and over the
/// newline that separates it from the line before. Without this a removal
/// leaves a line of trailing spaces where the element used to be, and the
/// second pass of an idempotence check has different bytes to work on.
///
/// Shrinks only when it actually reaches a newline, so an element that shares
/// a line with a sibling keeps that sibling's trailing text.
fn trim_back(xml: &[u8], start: usize) -> usize {
    let mut cursor = start;
    while cursor > 0 && matches!(xml[cursor - 1], b' ' | b'\t') {
        cursor -= 1;
    }
    if cursor > 0 && xml[cursor - 1] == b'\n' {
        cursor -= 1;
        if cursor > 0 && xml[cursor - 1] == b'\r' {
            cursor -= 1;
        }
        return cursor;
    }
    start
}

/// Copy everything outside the cuts. The cuts arrive in document order and
/// never overlap — only the outermost match opens one — and `max` keeps a
/// pathological input from indexing backwards rather than panicking.
fn splice(xml: &[u8], cuts: &[Range<usize>]) -> Vec<u8> {
    let mut out = Vec::with_capacity(xml.len());
    let mut cursor = 0;
    for cut in cuts {
        let from = cut.start.max(cursor);
        out.extend_from_slice(&xml[cursor..from]);
        cursor = cut.end.max(cursor);
    }
    out.extend_from_slice(&xml[cursor..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENTRA: &str = include_str!("fixtures/entra-federationmetadata.xml");
    const ENTRA_SANITISED: &str = include_str!("fixtures/entra-federationmetadata.sanitised.xml");
    const AM_SP: &str = include_str!("fixtures/am-hosted-sp.xml");

    fn clean(xml: &str) -> Sanitised {
        sanitise(xml.as_bytes(), SanitiseOpts::default()).expect("fixture parses")
    }

    fn report(result: &Sanitised) -> Vec<String> {
        result.removed.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn the_entra_fixture_sanitises_to_its_committed_pair_byte_for_byte() {
        let result = clean(ENTRA);
        assert_eq!(
            String::from_utf8(result.bytes).expect("output is utf-8"),
            ENTRA_SANITISED
        );
    }

    #[test]
    fn the_removal_report_names_each_element_where_it_was_and_why() {
        assert_eq!(
            report(&clean(ENTRA)),
            vec![
                "line 16: removed <ds:Signature> — enveloped signature; it would not cover the bytes we emit",
                "line 36: removed <fed:RoleDescriptor xsi:type=\"fed:SecurityTokenServiceType\"> — not a SAML 2.0 role; on the strip list",
                "line 60: removed <fed:RoleDescriptor xsi:type=\"fed:ApplicationServiceType\"> — not a SAML 2.0 role; on the strip list",
            ]
        );
    }

    #[test]
    fn sanitising_twice_changes_nothing_the_first_pass_left() {
        let once = clean(ENTRA);
        let twice = sanitise(&once.bytes, SanitiseOpts::default()).expect("output parses");
        assert_eq!(twice.bytes, once.bytes);
        assert!(twice.removed.is_empty(), "{:?}", twice.removed);
    }

    #[test]
    fn a_clean_am_document_passes_through_untouched() {
        // The control. Without it a sanitiser that deleted everything would
        // still satisfy the Entra pair.
        let result = clean(AM_SP);
        assert_eq!(
            String::from_utf8(result.bytes).expect("output is utf-8"),
            AM_SP
        );
        assert!(result.removed.is_empty(), "{:?}", result.removed);
    }

    // ── prefix handling ─────────────────────────────────────────────────────

    /// A document whose only prefixed element is the one under test, so every
    /// prefix spelling has the *same* expected output.
    fn prefixed(prefix: &str) -> String {
        let (qualified, declaration) = if prefix.is_empty() {
            (String::new(), String::new())
        } else {
            (
                format!("{prefix}:"),
                format!(" xmlns:{prefix}=\"http://docs.oasis-open.org/wsfed/federation/200706\""),
            )
        };
        format!(
            "{HEAD}\
             \n  <{qualified}RoleDescriptor{declaration} protocolSupportEnumeration=\"urn:x\">\
             \n    <KeyDescriptor use=\"signing\" />\
             \n  </{qualified}RoleDescriptor>\
             {TAIL}"
        )
    }

    const HEAD: &str = "<?xml version=\"1.0\"?>\n\
        <EntityDescriptor xmlns=\"urn:oasis:names:tc:SAML:2.0:metadata\"\n\
        \x20                 entityID=\"https://sp-a.example.com\">";
    const TAIL: &str = "\n  <SPSSODescriptor \
        protocolSupportEnumeration=\"urn:oasis:names:tc:SAML:2.0:protocol\" />\n\
        </EntityDescriptor>\n";

    #[test]
    fn the_strip_list_matches_a_whole_local_name_whatever_the_prefix() {
        let stripped = format!("{HEAD}{TAIL}");
        // Each case: what goes in, what must come out, how many removals.
        // The three prefixes must agree; the three negatives are the ones a
        // substring matcher gets wrong while passing every fixture test.
        let cases: &[(&str, String, String, usize)] = &[
            ("entra's fed: prefix", prefixed("fed"), stripped.clone(), 1),
            (
                "another tenant's prefix",
                prefixed("wsfed"),
                stripped.clone(),
                1,
            ),
            // Unprefixed in the SAML default namespace is a SAML RoleDescriptor,
            // not WS-Fed. Matching the local name alone used to delete it.
            ("no prefix at all", prefixed(""), prefixed("").clone(), 0),
            (
                "a longer name that merely starts with it",
                format!("{HEAD}\n  <RoleDescriptorExtension>keep</RoleDescriptorExtension>{TAIL}"),
                format!("{HEAD}\n  <RoleDescriptorExtension>keep</RoleDescriptorExtension>{TAIL}"),
                0,
            ),
            (
                "the name inside an attribute value",
                format!(
                    "{HEAD}\n  <SingleSignOnService Binding=\"urn:x\" \
                     Location=\"https://sp-a.example.com/RoleDescriptor\" />{TAIL}"
                ),
                format!(
                    "{HEAD}\n  <SingleSignOnService Binding=\"urn:x\" \
                     Location=\"https://sp-a.example.com/RoleDescriptor\" />{TAIL}"
                ),
                0,
            ),
            (
                "the name inside a comment",
                format!("{HEAD}\n  <!-- RoleDescriptor blocks go here -->{TAIL}"),
                format!("{HEAD}\n  <!-- RoleDescriptor blocks go here -->{TAIL}"),
                0,
            ),
        ];

        for (case, input, expected, removals) in cases {
            let result = clean(input);
            assert_eq!(
                String::from_utf8(result.bytes).expect("output is utf-8"),
                *expected,
                "{case}"
            );
            assert_eq!(
                result.removed.len(),
                *removals,
                "{case}: {:?}",
                result.removed
            );
        }
    }

    // ── the signature ───────────────────────────────────────────────────────

    #[test]
    fn keeping_the_signature_keeps_it_and_says_it_is_now_stale() {
        let kept = sanitise(
            ENTRA.as_bytes(),
            SanitiseOpts {
                keep_signature: true,
            },
        )
        .expect("fixture parses");
        let text = String::from_utf8(kept.bytes).expect("output is utf-8");
        assert!(text.contains("<ds:Signature>"), "signature was removed");
        assert!(
            kept.stale_signature,
            "a signature kept over stripped content is stale"
        );
        assert_eq!(
            kept.removed.iter().map(|r| r.line).collect::<Vec<_>>(),
            vec![36, 60]
        );

        // The control: by default the signature goes, and nothing is stale.
        let default = clean(ENTRA);
        assert!(!default.stale_signature);
        assert!(
            !String::from_utf8(default.bytes)
                .expect("output is utf-8")
                .contains("<ds:Signature>")
        );
    }

    #[test]
    fn keeping_the_signature_on_a_document_with_nothing_to_strip_is_not_stale() {
        // Discriminating control for `stale_signature`: it must key on
        // "content changed", not merely on "a signature is present".
        let kept = sanitise(
            AM_SP.as_bytes(),
            SanitiseOpts {
                keep_signature: true,
            },
        )
        .expect("fixture parses");
        assert!(!kept.stale_signature);
    }

    const SIGNED_CLEAN: &str = "<?xml version=\"1.0\"?>\n\
        <EntityDescriptor xmlns=\"urn:oasis:names:tc:SAML:2.0:metadata\"\n\
        \x20                 entityID=\"https://idp.example.com\">\n\
        \x20 <ds:Signature xmlns:ds=\"http://www.w3.org/2000/09/xmldsig#\">\
        <ds:SignedInfo/></ds:Signature>\n\
        \x20 <IDPSSODescriptor protocolSupportEnumeration=\"urn:oasis:names:tc:SAML:2.0:protocol\"/>\n\
        </EntityDescriptor>\n";

    #[test]
    fn a_signed_document_with_nothing_else_to_strip_keeps_its_signature() {
        // Discriminating control for the default: Signature is removed because
        // we are about to change signed bytes, not because AM rejects it.
        let result = clean(SIGNED_CLEAN);
        assert!(
            String::from_utf8(result.bytes.clone())
                .expect("utf-8")
                .contains("<ds:Signature"),
            "default sanitise deleted a signature over unchanged content"
        );
        assert!(result.removed.is_empty(), "{:?}", result.removed);
        let doc = inspect(SIGNED_CLEAN.as_bytes()).expect("parses");
        assert!(doc.signed);
        assert!(doc.would_remove.is_empty(), "{:?}", doc.would_remove);
    }

    #[test]
    fn a_role_descriptor_in_some_other_namespace_is_not_stripped() {
        let input = format!(
            "{HEAD}\n  <ext:RoleDescriptor xmlns:ext=\"urn:not-wsfed\" \
             protocolSupportEnumeration=\"urn:x\"/>{TAIL}"
        );
        let result = clean(&input);
        assert_eq!(
            String::from_utf8(result.bytes).expect("utf-8"),
            input,
            "stripped a RoleDescriptor that was not WS-Federation"
        );
        assert!(result.removed.is_empty(), "{:?}", result.removed);
    }

    // ── inspection ──────────────────────────────────────────────────────────

    #[test]
    fn inspect_reads_the_entity_id_exactly_including_the_trailing_slash() {
        // AM compares entity IDs exactly after trim (docs/api/06-saml.md), and
        // Entra issues its issuer WITH the trailing slash. Losing it here
        // would register an entity that never matches an assertion.
        let doc = inspect(ENTRA.as_bytes()).expect("fixture parses");
        assert_eq!(
            doc.entity_id,
            "https://sts.windows.net/00000000-0000-0000-0000-000000000000/"
        );
        assert_eq!(doc.roles, vec![Role::IdentityProvider]);
        assert!(doc.signed);
        assert_eq!(doc.would_remove.len(), 3);
    }

    #[test]
    fn inspect_reports_endpoints_under_the_role_that_declares_them() {
        let doc = inspect(AM_SP.as_bytes()).expect("fixture parses");
        assert_eq!(doc.roles, vec![Role::ServiceProvider]);
        assert!(!doc.signed);
        assert!(doc.would_remove.is_empty());
        assert_eq!(
            doc.endpoints
                .iter()
                .map(|endpoint| (
                    endpoint.descriptor.as_deref().unwrap_or("-"),
                    endpoint.kind.as_str()
                ))
                .collect::<Vec<_>>(),
            vec![
                ("SPSSODescriptor", "SingleLogoutService"),
                ("SPSSODescriptor", "AssertionConsumerService"),
            ]
        );
        assert_eq!(
            doc.endpoints[1].location,
            "https://tenant.example.com/am/AuthConsumer/metaAlias/alpha/sp-a"
        );
    }

    // ── certificates ────────────────────────────────────────────────────────

    #[test]
    fn cert_refs_fingerprints_every_key_descriptor_and_no_signature_key() {
        // Entra repeats one signing certificate across three KeyDescriptors,
        // two of them inside the RoleDescriptors. The enveloped signature
        // carries the same certificate again and must NOT be counted: it
        // identifies the signer, not a key the entity uses.
        let raw = cert_refs(ENTRA.as_bytes()).expect("fixture parses");
        assert_eq!(raw.len(), 3);
        assert!(
            raw.iter()
                .all(|cert| cert.key_use.as_deref() == Some("signing"))
        );
        assert!(raw.iter().all(|cert| cert.sha256 == raw[0].sha256));
        assert!(raw.iter().all(|cert| cert.key_name.is_none()));

        // Stripping the WS-Federation roles takes two of the three with it.
        let cleaned = cert_refs(&clean(ENTRA).bytes).expect("output parses");
        assert_eq!(cleaned.len(), 1);
        assert_eq!(cleaned[0].sha256, raw[0].sha256);
    }

    #[test]
    fn cert_refs_reads_a_wrapped_body_and_the_key_name_beside_it() {
        // AM wraps the base64 at 64 columns and names its keys; Entra does
        // neither. Distinct certificates must fingerprint distinctly.
        let certs = cert_refs(AM_SP.as_bytes()).expect("fixture parses");
        assert_eq!(
            certs
                .iter()
                .map(|cert| (cert.key_use.as_deref(), cert.key_name.as_deref()))
                .collect::<Vec<_>>(),
            vec![
                (Some("signing"), Some("sp-a-signing")),
                (Some("encryption"), Some("sp-a-encryption")),
            ]
        );
        assert_ne!(certs[0].sha256, certs[1].sha256);
        assert!(certs.iter().all(|cert| cert.sha256.len() == 64));
    }

    // ── failing closed ──────────────────────────────────────────────────────

    #[test]
    fn unreadable_or_unexpected_input_is_refused_rather_than_passed_through() {
        let cases: &[(&str, &str)] = &[
            ("unclosed element", "<EntityDescriptor entityID=\"x\">"),
            (
                "mismatched end tag",
                "<EntityDescriptor entityID=\"x\"></Other>",
            ),
            ("not metadata at all", "<html><body>hello</body></html>"),
            (
                "an aggregate of entities",
                "<EntitiesDescriptor><EntityDescriptor entityID=\"x\" /></EntitiesDescriptor>",
            ),
            ("no entityID", "<EntityDescriptor />"),
            ("empty input", ""),
            (
                "two top-level EntityDescriptors",
                "<EntityDescriptor xmlns=\"urn:oasis:names:tc:SAML:2.0:metadata\" entityID=\"https://sp-a.example.com\"/><EntityDescriptor xmlns=\"urn:oasis:names:tc:SAML:2.0:metadata\" entityID=\"https://sp-b.example.com\"/>",
            ),
            (
                "an XML-invalid comment",
                "<EntityDescriptor xmlns=\"urn:oasis:names:tc:SAML:2.0:metadata\" entityID=\"https://sp-a.example.com\"><!-- bad -- comment --></EntityDescriptor>",
            ),
            (
                "a root in some other namespace",
                "<html:EntityDescriptor xmlns:html=\"urn:not-saml\" entityID=\"https://sp-a.example.com\"/>",
            ),
            (
                "a certificate that is not base64",
                "<EntityDescriptor entityID=\"x\"><KeyDescriptor use=\"signing\">\
                 <X509Certificate>not base64 !!</X509Certificate></KeyDescriptor>\
                 </EntityDescriptor>",
            ),
        ];
        for (case, xml) in cases {
            assert!(
                sanitise(xml.as_bytes(), SanitiseOpts::default()).is_err(),
                "{case}: sanitise accepted it"
            );
            assert!(
                inspect(xml.as_bytes()).is_err(),
                "{case}: inspect accepted it"
            );
            assert!(
                cert_refs(xml.as_bytes()).is_err(),
                "{case}: cert_refs accepted it"
            );
        }

        let invalid_utf8 = b"<EntityDescriptor xmlns=\"urn:oasis:names:tc:SAML:2.0:metadata\" entityID=\"x\">\xff\xfe</EntityDescriptor>";
        assert!(inspect(invalid_utf8).is_err(), "invalid UTF-8 was accepted");
    }

    #[test]
    fn a_self_closing_strip_list_element_is_removed_whole() {
        // The empty-element form takes a different branch from the
        // start/end pair, and an unhandled `<RoleDescriptor />` would leave
        // the cut open and swallow the rest of the document.
        let input = format!(
            "{HEAD}\n  <fed:RoleDescriptor xmlns:fed=\"http://docs.oasis-open.org/wsfed/federation/200706\" />{TAIL}"
        );
        let result = clean(&input);
        assert_eq!(
            String::from_utf8(result.bytes).expect("output is utf-8"),
            format!("{HEAD}{TAIL}")
        );
        assert_eq!(result.removed.len(), 1);
    }
}
