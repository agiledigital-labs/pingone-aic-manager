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
//! promise anything about. Tokenising is not validating, so the scanner adds
//! what `quick_xml` does not check: exactly one root element, which events are
//! legal in each of XML's three document sections, and an
//! expanded name for every element *and* every attribute — an undeclared
//! prefix anywhere on an element is a refusal, because we cannot know what
//! name it was meant to be.

use std::fmt;
use std::ops::Range;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use quick_xml::NsReader;
use quick_xml::XmlVersion;
use quick_xml::escape::unescape;
use quick_xml::events::{BytesStart, Event};
use quick_xml::name::{Namespace, NamespaceResolver, QName, ResolveResult};
use serde::Serialize;
use sha2::{Digest, Sha256};

type Result<T> = std::result::Result<T, MetadataError>;

/// SAML 2.0 metadata. Root, roles, and key descriptors live here.
const SAML_NS: &[u8] = b"urn:oasis:names:tc:SAML:2.0:metadata";
/// XML Signature. A `Signature` in any other namespace is not an enveloped
/// signature over this document.
const DSIG_NS: &[u8] = b"http://www.w3.org/2000/09/xmldsig#";
/// WS-Federation. Entra authors extra roles as SAML's abstract
/// `{SAML}RoleDescriptor` with `xsi:type` a QName in this namespace
/// (`SecurityTokenServiceType`, `ApplicationServiceType`). Matching that
/// expanded type — not the element's namespace, not the prefix — is what
/// hits real Entra metadata and leaves a SAML `RoleDescriptor` without
/// that type alone.
///
/// **Not verified against AM's importer.** A live import is what can promote
/// this to a claim in `docs/api/06-saml.md`.
const WSFED_NS: &[u8] = b"http://docs.oasis-open.org/wsfed/federation/200706";
/// `xsi:type` lives here. A bare `type` attribute is a different name.
const XSI_NS: &[u8] = b"http://www.w3.org/2001/XMLSchema-instance";

/// The XML-Signature element. Only a *direct child* of `EntityDescriptor`
/// signs the whole document; one further down signs the role it sits in.
/// Conditional on `--keep-signature`, and only removed when some *other* cut
/// would change the bytes it covers.
const SIGNATURE_LOCAL_NAME: &str = "Signature";

/// The five entities XML predefines. Any other name needs a DTD declaration
/// to have a replacement text at all, and we do not read DTDs.
const PREDEFINED_ENTITIES: [&str; 5] = ["amp", "lt", "gt", "quot", "apos"];

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
    /// A WS-Federation role: SAML's abstract `RoleDescriptor` given a
    /// WS-Federation `xsi:type` — `wsfed_role_type` holds the full selector.
    UnsupportedRole,
    /// An enveloped signature over the whole document, when this rewrite
    /// changes bytes it covers.
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
    /// Whether the document carries an enveloped signature over *itself* — a
    /// `Signature` that is a direct child of `EntityDescriptor`. A signature
    /// further down covers one role; removing a *sibling* role leaves it
    /// verifying, so it is neither counted here nor removed. A fact about the
    /// document, separate from whether we would remove it.
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

/// Default sanitise drops the document's enveloped signature only when some
/// other cut would change the bytes it covers. `--keep-signature` drops
/// signature cuts always, and sets [`Sanitised::stale_signature`] when content
/// still changes.
///
/// Scope matters and the scanner has already applied it: the only
/// [`RemovalReason::EnvelopedSignature`] cut that reaches here is a direct
/// child of `EntityDescriptor`, so "any other cut" really does mean "inside
/// the bytes this signature signs".
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

/// Which of XML's three document sections an event arrived in.
///
/// The grammar is `prolog element Misc*`, and it is the *only* thing that
/// makes a second XML declaration, a `DOCTYPE` after the root, or a stray
/// CDATA section ill-formed — each of them tokenises perfectly. `quick_xml`
/// is a tokeniser, so this is ours to enforce, and the scanner matches every
/// event kind against it rather than ignoring the kinds it does not read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    /// Everything before the root element's start tag.
    Prolog,
    /// Inside the root element.
    Root,
    /// Everything after the root element's end tag.
    Epilog,
}

impl Section {
    /// How to name this section in a refusal, as the tail of a sentence.
    fn placement(self) -> &'static str {
        match self {
            Self::Prolog => "before the root element",
            Self::Root => "inside the root element",
            Self::Epilog => "after the root element",
        }
    }
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
    let mut any_event_seen = false;

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
            ResolveResult::Unknown(prefix) => return Err(unknown_prefix(start, &prefix)),
            ResolveResult::Unbound => None,
        };
        let end = position(&reader);
        let ns = ns.as_deref();
        let first_event = !any_event_seen;
        any_event_seen = true;
        let section = if !root_seen {
            Section::Prolog
        } else if path.is_empty() {
            Section::Epilog
        } else {
            Section::Root
        };

        // Every event kind is matched, and every arm decides against
        // `section`. A wildcard arm here is how a second XML declaration, a
        // misplaced `DOCTYPE` and a trailing CDATA section all used to pass.
        match event {
            Event::Decl(decl) => {
                // A declaration is legal exactly once, as the first thing in
                // the document. Anywhere else it is character data pretending
                // to be a prolog.
                if !first_event {
                    return Err(misplaced(start, "an XML declaration", section));
                }
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
            Event::DocType(_) => {
                // Legal in the prolog only. We read no DTD, so any entity it
                // declares is refused where it is *used* — see
                // `Event::GeneralRef` below.
                if section != Section::Prolog {
                    return Err(misplaced(start, "a DOCTYPE declaration", section));
                }
            }
            // Legal in all three sections, and neither is content we read.
            // Splicing carries their bytes through untouched.
            Event::Comment(_) | Event::PI(_) => {}
            Event::Start(element) | Event::Empty(element) => {
                let empty = end > start && xml[start..end].ends_with(b"/>");
                let name = local_name(&element);
                // Read every attribute now, resolved, so an undeclared prefix
                // on one we never look at is still a refusal.
                let attrs = Attrs::read(&element, reader.resolver(), start)?;

                match section {
                    Section::Epilog => {
                        return Err(misplaced(start, &format!("<{name}>"), section));
                    }
                    Section::Prolog => {
                        root_seen = true;
                        if name != ROOT_LOCAL_NAME || ns != Some(SAML_NS) {
                            return Err(MetadataError::NotEntityMetadata { root: name });
                        }
                        entity_id = attrs.unqualified("entityID").map(str::to_owned);
                    }
                    Section::Root => {}
                }

                if let Some(role) = Role::from_expanded_name(&name, ns) {
                    if !scan.roles.contains(&role) {
                        scan.roles.push(role);
                    }
                }
                // Only a direct child of the root signs the whole document;
                // `path` holds just the root at that point.
                let enveloped_signature =
                    name == SIGNATURE_LOCAL_NAME && ns == Some(DSIG_NS) && path.len() == 1;
                if enveloped_signature {
                    scan.signed = true;
                }
                if let (Some(binding), Some(location)) =
                    (attrs.unqualified("Binding"), attrs.unqualified("Location"))
                {
                    scan.endpoints.push(Endpoint {
                        descriptor: enclosing_descriptor(&path),
                        kind: name.clone(),
                        binding: binding.to_owned(),
                        location: location.to_owned(),
                    });
                }
                if name == "KeyDescriptor" && ns == Some(SAML_NS) {
                    pending_key = Some(PendingKey {
                        key_use: attrs.unqualified("use").map(str::to_owned),
                        key_name: None,
                        depth: path.len(),
                        certs: Vec::new(),
                    });
                }

                // Only the outermost match is recorded: a strip-list element
                // nested inside one already being removed goes with it, and
                // reporting it separately would tell an operator about bytes
                // that were never theirs to keep.
                let strip = if enveloped_signature {
                    Some((RemovalReason::EnvelopedSignature, None))
                } else {
                    wsfed_role_type(&name, ns, &attrs, reader.resolver(), start)?
                        .map(|xsi_type| (RemovalReason::UnsupportedRole, Some(xsi_type)))
                };
                if open_cut.is_none()
                    && let Some((reason, xsi_type)) = strip
                {
                    let removal = Removal {
                        element: qualified_name(&element),
                        local_name: name.clone(),
                        xsi_type,
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
                let raw = text.decode().map_err(|error| MetadataError::Malformed {
                    offset: start,
                    detail: error.to_string(),
                })?;
                match section {
                    Section::Root if pending_key.is_some() => {
                        let body = unescape(&raw)
                            .map_err(|error| MetadataError::Malformed {
                                offset: start,
                                detail: error.to_string(),
                            })?
                            .into_owned();
                        take_key_text(pending_key.as_mut(), &path, line_at(xml, start), body);
                    }
                    // Character data we do not read. Splicing preserves it.
                    Section::Root => {}
                    // Whitespace around the root is the only character data
                    // the prolog and the epilog may hold.
                    Section::Prolog | Section::Epilog => {
                        if !raw.chars().all(char::is_whitespace) {
                            return Err(misplaced(start, "text", section));
                        }
                    }
                }
            }
            Event::CData(cdata) => {
                if section != Section::Root {
                    return Err(misplaced(start, "a CDATA section", section));
                }
                // Character data with no escaping. A certificate written this
                // way is still a certificate, and skipping the event would
                // report a `<KeyDescriptor>` as carrying none.
                let body = cdata.decode().map_err(|error| MetadataError::Malformed {
                    offset: start,
                    detail: error.to_string(),
                })?;
                take_key_text(
                    pending_key.as_mut(),
                    &path,
                    line_at(xml, start),
                    body.into_owned(),
                );
            }
            Event::GeneralRef(reference) => {
                if section != Section::Root {
                    return Err(misplaced(start, "an entity reference", section));
                }
                let name = reference
                    .decode()
                    .map_err(|error| MetadataError::Malformed {
                        offset: start,
                        detail: error.to_string(),
                    })?
                    .into_owned();
                // A reference splits the character data around it, so a value
                // we actually read would arrive as fragments. Refusing beats
                // fingerprinting half a certificate.
                if let Some(holder) = reading_key_text(pending_key.as_ref(), &path) {
                    return Err(MetadataError::Malformed {
                        offset: start,
                        detail: format!("an entity reference inside <{holder}>"),
                    });
                }
                // Elsewhere it is text we do not read and splicing preserves
                // it verbatim — but only if we can be sure it *has* a
                // replacement text. A DTD-declared entity does not qualify:
                // we never read the DTD.
                if !reference.is_char_ref() && !PREDEFINED_ENTITIES.contains(&name.as_str()) {
                    return Err(MetadataError::Malformed {
                        offset: start,
                        detail: format!("undeclared entity reference '&{name};'"),
                    });
                }
            }
            Event::Eof => break,
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

/// An event that tokenised cleanly but is not legal where it appeared.
fn misplaced(offset: usize, what: &str, section: Section) -> MetadataError {
    MetadataError::Malformed {
        offset,
        detail: format!("{what} {}", section.placement()),
    }
}

/// The `<KeyDescriptor>` child whose character data we are reading, if any.
/// Both are leaf elements holding exactly one value.
fn reading_key_text<'a>(pending_key: Option<&PendingKey>, path: &'a [String]) -> Option<&'a str> {
    pending_key?;
    match path.last().map(String::as_str) {
        Some(holder @ ("X509Certificate" | "KeyName")) => Some(holder),
        _ => None,
    }
}

/// File one run of character data against the `<KeyDescriptor>` being read.
/// Called for `Text` and for `CData`, which differ only in escaping.
fn take_key_text(pending_key: Option<&mut PendingKey>, path: &[String], line: usize, body: String) {
    let Some(key) = pending_key else { return };
    match path.last().map(String::as_str) {
        Some("X509Certificate") => key.certs.push((line, body)),
        Some("KeyName") => key.key_name = Some(body.trim().to_string()),
        _ => {}
    }
}

/// The WS-Federation `xsi:type` of a role Entra adds and AM's importer
/// rejects, or `None` when this element is not one.
///
/// The whole selector, in expanded names:
///
/// - the element is `{SAML}RoleDescriptor` — SAML's *abstract* role, which is
///   what real Entra metadata and the WS-Federation specification both emit.
///   The element is **not** in the WS-Federation namespace; only its type is;
/// - it carries an `{XSI}type` attribute — a bare `type` is a different name;
/// - that attribute's value, read as a QName in this element's own namespace
///   scope, is in [`WSFED_NS`].
///
/// Every part is an expanded name, so the `fed:` or `wsfed:` prefix a
/// particular document happens to author never enters into it. Any WS-Fed
/// type counts: Entra's `SecurityTokenServiceType` and
/// `ApplicationServiceType` are the two seen in the wild, not a closed list.
fn wsfed_role_type(
    local_name: &str,
    ns: Option<&[u8]>,
    attrs: &Attrs,
    resolver: &NamespaceResolver,
    offset: usize,
) -> Result<Option<String>> {
    if local_name != "RoleDescriptor" || ns != Some(SAML_NS) {
        return Ok(None);
    }
    let Some(raw) = attrs.get(Some(XSI_NS), "type") else {
        return Ok(None);
    };
    // An unprefixed QName *value* resolves against the default namespace,
    // unlike an attribute *name* — hence `true`. Schema collapses the
    // surrounding whitespace a QName value may carry; the report still names
    // the value as the file spells it.
    let (resolved, _) = resolver.resolve(QName(raw.trim().as_bytes()), true);
    match resolved {
        ResolveResult::Bound(Namespace(uri)) if uri == WSFED_NS => Ok(Some(raw.to_owned())),
        ResolveResult::Unknown(prefix) => Err(unknown_prefix(offset, &prefix)),
        ResolveResult::Bound(_) | ResolveResult::Unbound => Ok(None),
    }
}

/// Every attribute of one element, each name resolved to its expanded form.
///
/// Read once per element rather than once per lookup, because the check that
/// matters most is the one on the attributes we *don't* read: an undeclared
/// prefix anywhere on the element means we cannot say what the element
/// carries, and this module refuses what it cannot read.
struct Attrs {
    /// `(namespace, local name, normalised value)`, in document order. An
    /// unprefixed attribute name has no namespace — the default namespace
    /// applies to element names only.
    items: Vec<(Option<Vec<u8>>, String, String)>,
}

impl Attrs {
    fn read(element: &BytesStart<'_>, resolver: &NamespaceResolver, offset: usize) -> Result<Self> {
        let mut items = Vec::new();
        for attribute in element.attributes() {
            let attribute = attribute.map_err(|error| MetadataError::Malformed {
                offset,
                detail: error.to_string(),
            })?;
            let (resolved, local) = resolver.resolve_attribute(attribute.key);
            let ns = match resolved {
                ResolveResult::Bound(Namespace(uri)) => Some(uri.to_vec()),
                ResolveResult::Unknown(prefix) => return Err(unknown_prefix(offset, &prefix)),
                ResolveResult::Unbound => None,
            };
            let value = attribute
                .normalized_value(XmlVersion::default())
                .map_err(|error| MetadataError::Malformed {
                    offset,
                    detail: error.to_string(),
                })?;
            items.push((
                ns,
                String::from_utf8_lossy(local.as_ref()).into_owned(),
                value.into_owned(),
            ));
        }
        Ok(Self { items })
    }

    fn get(&self, ns: Option<&[u8]>, local: &str) -> Option<&str> {
        self.items
            .iter()
            .find(|(item_ns, item_local, _)| item_ns.as_deref() == ns && item_local == local)
            .map(|(_, _, value)| value.as_str())
    }

    /// An attribute the SAML schema declares unqualified — `entityID`,
    /// `Binding`, `Location`, `use`. A prefixed `ext:entityID` is a
    /// *different* name and must not answer for the one the schema requires.
    fn unqualified(&self, local: &str) -> Option<&str> {
        self.get(None, local)
    }
}

fn unknown_prefix(offset: usize, prefix: &[u8]) -> MetadataError {
    MetadataError::Malformed {
        offset,
        detail: format!(
            "unknown namespace prefix '{}'",
            String::from_utf8_lossy(prefix)
        ),
    }
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
                "line 36: removed <RoleDescriptor xsi:type=\"fed:SecurityTokenServiceType\"> — not a SAML 2.0 role; on the strip list",
                "line 60: removed <RoleDescriptor xsi:type=\"fed:ApplicationServiceType\"> — not a SAML 2.0 role; on the strip list",
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

    // ── WS-Fed RoleDescriptor matching ──────────────────────────────────────

    const HEAD: &str = "<?xml version=\"1.0\"?>\n\
        <EntityDescriptor xmlns=\"urn:oasis:names:tc:SAML:2.0:metadata\"\n\
        \x20                 entityID=\"https://sp-a.example.com\">";
    const TAIL: &str = "\n  <SPSSODescriptor \
        protocolSupportEnumeration=\"urn:oasis:names:tc:SAML:2.0:protocol\" />\n\
        </EntityDescriptor>\n";
    const WSFED_URI: &str = "http://docs.oasis-open.org/wsfed/federation/200706";
    const XSI_URI: &str = "http://www.w3.org/2001/XMLSchema-instance";
    const SAML_URI: &str = "urn:oasis:names:tc:SAML:2.0:metadata";
    const DSIG_URI: &str = "http://www.w3.org/2000/09/xmldsig#";

    /// Real Entra shape: `{SAML}RoleDescriptor` with `xsi:type` a QName in
    /// WS-Fed, whatever prefix the document bound that URI to.
    fn entra_shaped(prefix: &str, type_local: &str) -> String {
        format!(
            "{HEAD}\n  <RoleDescriptor xmlns:{prefix}=\"{WSFED_URI}\" xmlns:xsi=\"{XSI_URI}\" \
             xsi:type=\"{prefix}:{type_local}\" protocolSupportEnumeration=\"urn:x\">\
             \n    <KeyDescriptor use=\"signing\" />\
             \n  </RoleDescriptor>{TAIL}"
        )
    }

    #[test]
    fn wsfed_roles_match_saml_roledescriptor_plus_xsi_type() {
        let stripped = format!("{HEAD}{TAIL}");
        // Each case: what goes in, what must come out, how many removals.
        // The first three are the real Entra form with different prefixes and
        // type local names; the rest are the matchers a plausible-but-wrong
        // implementation agrees with the fixture on.
        let cases: &[(&str, String, String, usize)] = &[
            (
                "entra's fed: type prefix",
                entra_shaped("fed", "SecurityTokenServiceType"),
                stripped.clone(),
                1,
            ),
            (
                "another tenant's type prefix",
                entra_shaped("wsfed", "ApplicationServiceType"),
                stripped.clone(),
                1,
            ),
            (
                "a WS-Fed type that is not one of Entra's two names",
                entra_shaped("fed", "SomeFutureType"),
                stripped.clone(),
                1,
            ),
            (
                // A QName *value* with no prefix resolves against the default
                // namespace, unlike an attribute *name*. Here the element is
                // SAML through a prefix and the type is WS-Fed through the
                // default declaration — the mirror image of the usual shape.
                "an xsi:type with no prefix at all",
                format!(
                    "{HEAD}\n  <md:RoleDescriptor xmlns:md=\"{SAML_URI}\" xmlns=\"{WSFED_URI}\" \
                     xmlns:xsi=\"{XSI_URI}\" xsi:type=\"SecurityTokenServiceType\" \
                     protocolSupportEnumeration=\"urn:x\"/>{TAIL}"
                ),
                stripped.clone(),
                1,
            ),
            (
                // Schema collapses the whitespace around a QName value.
                "an xsi:type padded with whitespace",
                format!(
                    "{HEAD}\n  <RoleDescriptor xmlns:fed=\"{WSFED_URI}\" xmlns:xsi=\"{XSI_URI}\" \
                     xsi:type=\" fed:SecurityTokenServiceType \" \
                     protocolSupportEnumeration=\"urn:x\"/>{TAIL}"
                ),
                stripped.clone(),
                1,
            ),
            (
                "a SAML RoleDescriptor with no xsi:type",
                format!(
                    "{HEAD}\n  <RoleDescriptor protocolSupportEnumeration=\"urn:x\">\
                     \n    <KeyDescriptor use=\"signing\" />\
                     \n  </RoleDescriptor>{TAIL}"
                ),
                format!(
                    "{HEAD}\n  <RoleDescriptor protocolSupportEnumeration=\"urn:x\">\
                     \n    <KeyDescriptor use=\"signing\" />\
                     \n  </RoleDescriptor>{TAIL}"
                ),
                0,
            ),
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

    /// One `<ds:Signature>`, small enough to read inside a fixture literal.
    fn dsig() -> String {
        format!("<ds:Signature xmlns:ds=\"{DSIG_URI}\"><ds:SignedInfo/></ds:Signature>")
    }

    /// The WS-Federation role every case below has cut out from under it.
    fn wsfed_role() -> String {
        format!(
            "<RoleDescriptor xmlns:xsi=\"{XSI_URI}\" xmlns:fed=\"{WSFED_URI}\" \
             xsi:type=\"fed:SecurityTokenServiceType\""
        )
    }

    #[test]
    fn only_a_signature_over_the_whole_document_is_invalidated() {
        // A signature signs the subtree it hangs off. Removing a WS-Fed role
        // changes the document, so a signature on `EntityDescriptor` can no
        // longer verify — but one on a *retained sibling* role covers bytes
        // this rewrite never touches, and deleting it destroys a valid
        // signature to no purpose.
        let sig = dsig();
        let role = wsfed_role();
        let idp = "<IDPSSODescriptor protocolSupportEnumeration=\"urn:oasis:names:tc:SAML:2.0:protocol\">";

        // case, document, expected reasons, does a signature survive, `signed`
        let cases: &[(&str, String, Vec<RemovalReason>, bool, bool)] = &[
            (
                "a signature on a retained role survives a sibling being cut",
                format!("{HEAD}\n  {role} />\n  {idp}{sig}</IDPSSODescriptor>{TAIL}"),
                vec![RemovalReason::UnsupportedRole],
                true,
                false,
            ),
            (
                "the document's own signature goes when a role is cut",
                format!("{HEAD}\n  {sig}\n  {role} />{TAIL}"),
                vec![
                    RemovalReason::EnvelopedSignature,
                    RemovalReason::UnsupportedRole,
                ],
                false,
                true,
            ),
            (
                "a signature inside the cut role goes with it, unreported",
                format!("{HEAD}\n  {role}>{sig}</RoleDescriptor>{TAIL}"),
                vec![RemovalReason::UnsupportedRole],
                false,
                false,
            ),
            (
                "a role signature with nothing else to strip is left alone",
                format!("{HEAD}\n  {idp}{sig}</IDPSSODescriptor>{TAIL}"),
                vec![],
                true,
                false,
            ),
        ];

        for (case, input, reasons, signature_survives, signed) in cases {
            let result = clean(input);
            assert_eq!(
                result
                    .removed
                    .iter()
                    .map(|removal| removal.reason)
                    .collect::<Vec<_>>(),
                *reasons,
                "{case}"
            );
            assert_eq!(
                String::from_utf8(result.bytes)
                    .expect("utf-8")
                    .contains("<ds:Signature"),
                *signature_survives,
                "{case}"
            );
            assert_eq!(
                inspect(input.as_bytes()).expect("parses").signed,
                *signed,
                "{case}: `signed` reports the document's own signature"
            );
        }
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

    #[test]
    fn a_wsfed_element_named_roledescriptor_is_not_the_entra_form() {
        // The previous matcher keyed on `{WSFED}RoleDescriptor`. Real Entra
        // (and the WS-Federation spec) put the element in the SAML namespace.
        let input = format!(
            "{HEAD}\n  <fed:RoleDescriptor xmlns:fed=\"{WSFED_URI}\" \
             protocolSupportEnumeration=\"urn:x\"/>{TAIL}"
        );
        let result = clean(&input);
        assert_eq!(
            String::from_utf8(result.bytes).expect("utf-8"),
            input,
            "stripped {{WS-Fed}}RoleDescriptor; Entra's form is {{SAML}}RoleDescriptor + xsi:type"
        );
        assert!(result.removed.is_empty(), "{:?}", result.removed);
    }

    #[test]
    fn a_bare_type_attribute_is_not_xsi_type() {
        let input = format!(
            "{HEAD}\n  <RoleDescriptor xmlns:fed=\"{WSFED_URI}\" \
             type=\"fed:SecurityTokenServiceType\" protocolSupportEnumeration=\"urn:x\"/>{TAIL}"
        );
        let result = clean(&input);
        assert_eq!(
            String::from_utf8(result.bytes).expect("utf-8"),
            input,
            "matched a bare type attribute as xsi:type"
        );
        assert!(result.removed.is_empty(), "{:?}", result.removed);
    }

    #[test]
    fn an_xsi_type_in_some_other_namespace_is_not_stripped() {
        let input = format!(
            "{HEAD}\n  <RoleDescriptor xmlns:xsi=\"{XSI_URI}\" xmlns:other=\"urn:not-wsfed\" \
             xsi:type=\"other:SecurityTokenServiceType\" protocolSupportEnumeration=\"urn:x\"/>{TAIL}"
        );
        let result = clean(&input);
        assert_eq!(
            String::from_utf8(result.bytes).expect("utf-8"),
            input,
            "stripped a RoleDescriptor whose xsi:type was not WS-Federation"
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
        // Each case must fail for the reason its name gives, so every one that
        // gets as far as the root element declares the SAML namespace: without
        // it the root check fires first and the case proves nothing.
        let root = format!("<EntityDescriptor xmlns=\"{SAML_URI}\"");
        let entity = format!("{root} entityID=\"https://sp-a.example.com\"");
        let cases: &[(&str, String)] = &[
            ("unclosed element", format!("{entity}>")),
            ("mismatched end tag", format!("{entity}></Other>")),
            (
                "not metadata at all",
                "<html><body>hello</body></html>".to_string(),
            ),
            (
                "an aggregate of entities",
                format!("<EntitiesDescriptor>{entity} /></EntitiesDescriptor>"),
            ),
            ("no entityID", format!("{root} />")),
            ("empty input", String::new()),
            (
                "two top-level EntityDescriptors",
                format!("{entity}/><EntityDescriptor xmlns=\"{SAML_URI}\" entityID=\"https://sp-b.example.com\"/>"),
            ),
            (
                "an XML-invalid comment",
                format!("{entity}><!-- bad -- comment --></EntityDescriptor>"),
            ),
            (
                "a root in some other namespace",
                "<html:EntityDescriptor xmlns:html=\"urn:not-saml\" entityID=\"https://sp-a.example.com\"/>".to_string(),
            ),
            (
                "a certificate that is not base64",
                format!(
                    "{entity}><KeyDescriptor use=\"signing\">\
                     <X509Certificate>not base64 !!</X509Certificate></KeyDescriptor>\
                     </EntityDescriptor>"
                ),
            ),
            (
                "an undeclared prefix on xsi:type's QName",
                format!(
                    "{entity}><RoleDescriptor xmlns:xsi=\"{XSI_URI}\" \
                     xsi:type=\"nope:SecurityTokenServiceType\"/></EntityDescriptor>"
                ),
            ),
            (
                "an undeclared xsi prefix on RoleDescriptor",
                format!(
                    "{entity}><RoleDescriptor xsi:type=\"fed:SecurityTokenServiceType\" \
                     xmlns:fed=\"{WSFED_URI}\"/></EntityDescriptor>"
                ),
            ),
            // ── namespace-resolved attribute names ──────────────────────────
            (
                // Nothing reads `bad:attr`, which is the point: an element
                // carrying a name we cannot expand is an element we cannot
                // describe.
                "an undeclared prefix on an attribute nothing reads",
                format!("{entity} bad:attr=\"1\"/>"),
            ),
            (
                // `entityID` is unqualified in the schema. Matching on the
                // local name alone let a foreign vocabulary's attribute
                // satisfy the one AM compares assertions against.
                "a prefixed entityID standing in for the required one",
                format!("{root} xmlns:ext=\"urn:x\" ext:entityID=\"https://sp-a.example.com\"/>"),
            ),
            // ── document sections ───────────────────────────────────────────
            ("text before the root element", format!("hello{entity}/>")),
            (
                "an XML declaration that is not the first thing",
                format!("<!-- c -->\n<?xml version=\"1.0\"?>{entity}/>"),
            ),
            (
                "a second XML declaration after the root",
                format!("<?xml version=\"1.0\"?>{entity}/><?xml version=\"1.0\"?>"),
            ),
            (
                "a DOCTYPE after the root",
                format!("{entity}/><!DOCTYPE EntityDescriptor>"),
            ),
            (
                "a CDATA section after the root",
                format!("{entity}/><![CDATA[trailing]]>"),
            ),
            (
                "an entity reference after the root",
                format!("{entity}/>&amp;"),
            ),
            // ── references we cannot resolve ────────────────────────────────
            (
                "an entity reference with no declaration to define it",
                format!("{entity}><Organization>&oops;</Organization></EntityDescriptor>"),
            ),
            (
                // Predefined, so its replacement text is known — but it splits
                // the character data, and half a certificate must not be
                // fingerprinted as if it were whole.
                "an entity reference splitting a certificate body",
                format!(
                    "{entity}><KeyDescriptor use=\"signing\">\
                     <X509Certificate>AAEC&amp;AwQ=</X509Certificate></KeyDescriptor>\
                     </EntityDescriptor>"
                ),
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
    fn legal_furniture_around_and_inside_the_root_is_accepted() {
        // The control for the table above, which a scanner that refused every
        // document would satisfy on its own. Each of these is well-formed XML
        // that a real export can contain, and must survive byte for byte.
        let body = format!(
            "<EntityDescriptor xmlns=\"{SAML_URI}\" entityID=\"https://sp-a.example.com\">\
             <SPSSODescriptor protocolSupportEnumeration=\"urn:oasis:names:tc:SAML:2.0:protocol\" />\
             </EntityDescriptor>"
        );
        let cases: &[(&str, String)] = &[
            (
                "a declaration, a DOCTYPE, a comment and a PI in the prolog",
                format!(
                    "<?xml version=\"1.0\"?>\n<!DOCTYPE EntityDescriptor>\n\
                     <!-- hand-authored -->\n<?target data?>\n{body}"
                ),
            ),
            (
                "a comment, a PI and whitespace in the epilog",
                format!("{body}\n<!-- trailing note -->\n<?target data?>\n"),
            ),
            (
                "a predefined entity in text we never read",
                format!(
                    "<EntityDescriptor xmlns=\"{SAML_URI}\" entityID=\"https://sp-a.example.com\">\
                     <Organization><OrganizationName>Cats &amp; Dogs</OrganizationName>\
                     </Organization></EntityDescriptor>"
                ),
            ),
            (
                "a character reference in text we never read",
                format!(
                    "<EntityDescriptor xmlns=\"{SAML_URI}\" entityID=\"https://sp-a.example.com\">\
                     <Organization><OrganizationName>&#65;cme</OrganizationName>\
                     </Organization></EntityDescriptor>"
                ),
            ),
        ];
        for (case, xml) in cases {
            let result = sanitise(xml.as_bytes(), SanitiseOpts::default())
                .unwrap_or_else(|error| panic!("{case}: {error}"));
            assert_eq!(
                String::from_utf8(result.bytes).expect("utf-8"),
                *xml,
                "{case}"
            );
            assert!(result.removed.is_empty(), "{case}: {:?}", result.removed);
        }
    }

    #[test]
    fn a_certificate_written_as_cdata_reads_the_same_as_one_in_plain_text() {
        // CDATA arrives as its own event. Ignoring that event would report a
        // KeyDescriptor as carrying no certificate at all — a silent nothing,
        // not an error.
        let document = |body: &str| {
            format!(
                "<EntityDescriptor xmlns=\"{SAML_URI}\" entityID=\"https://sp-a.example.com\">\
                 <KeyDescriptor use=\"signing\"><ds:KeyInfo xmlns:ds=\"{DSIG_URI}\">\
                 <ds:X509Data><X509Certificate>{body}</X509Certificate></ds:X509Data>\
                 </ds:KeyInfo></KeyDescriptor></EntityDescriptor>"
            )
        };
        let plain = cert_refs(document("AAECAwQ=").as_bytes()).expect("plain text parses");
        let cdata = cert_refs(document("<![CDATA[AAECAwQ=]]>").as_bytes()).expect("CDATA parses");
        assert_eq!(plain.len(), 1, "{plain:?}");
        assert_eq!(cdata, plain);
    }

    #[test]
    fn a_self_closing_strip_list_element_is_removed_whole() {
        // The empty-element form takes a different branch from the
        // start/end pair, and an unhandled `<RoleDescriptor />` would leave
        // the cut open and swallow the rest of the document.
        let input = format!(
            "{HEAD}\n  <RoleDescriptor xmlns:xsi=\"{XSI_URI}\" xmlns:fed=\"{WSFED_URI}\" \
             xsi:type=\"fed:SecurityTokenServiceType\" />{TAIL}"
        );
        let result = clean(&input);
        assert_eq!(
            String::from_utf8(result.bytes).expect("output is utf-8"),
            format!("{HEAD}{TAIL}")
        );
        assert_eq!(result.removed.len(), 1);
    }
}
