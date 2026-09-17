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
//! what `quick_xml` does not check: the XML 1.0 well-formedness and namespace
//! productions a tokeniser walks straight past (see the
//! *well-formedness* section below), exactly one root element, which events
//! are legal in each of XML's three document sections, and an expanded name
//! for every element *and* every attribute — an undeclared prefix anywhere on
//! an element is a refusal, because we cannot know what name it was meant to
//! be.
//!
//! **One admission boundary, two depths.** Every entry point here walks the
//! same [`scan`], so a document `sanitise` refuses cannot be waved through by
//! an export classification. [`Depth`] is the only thing that varies:
//! [`validate_export_document`] stops at the document's own grammar, because
//! a *successful* export of a roleless entity carries no `entityID` and a
//! certificate body we dislike is a complaint about content rather than
//! evidence that the export failed.

use std::fmt;
use std::ops::Range;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use quick_xml::NsReader;
use quick_xml::XmlVersion;
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

/// The five entities XML predefines, with the replacement text each stands
/// for. Any other name needs a DTD declaration to have a replacement text at
/// all, and we do not read DTDs.
const PREDEFINED_ENTITIES: [(&str, char); 5] = [
    ("amp", '&'),
    ("lt", '<'),
    ("gt", '>'),
    ("quot", '"'),
    ("apos", '\''),
];

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
    /// The entity role that publishes it — `IDPSSODescriptor`,
    /// `SPSSODescriptor`, or whatever other role descriptor holds the key.
    ///
    /// **This is identity, not decoration.** A dual-role entity can publish
    /// `use="signing"` from both its IdP and its SP role with no `KeyName` on
    /// either, and then `use` plus fingerprint names two certificates
    /// identically. Rotation has to know which one it is replacing, so the
    /// role is recorded at the `<KeyDescriptor>` that owns the key and a key
    /// descriptor that is not a direct child of a direct role is refused
    /// rather than attributed to a role it does not belong to.
    pub descriptor: String,
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
    let scan = scan(xml, Depth::Content)?;
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
    let scan = scan(xml, Depth::Content)?;
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
    Ok(scan(xml, Depth::Content)?.certs)
}

/// Whether a body is a SAML 2.0 metadata document at all.
///
/// Deliberately weaker than [`inspect`], and the difference is the point. The
/// metadata-export JSP reports failure with HTTP 200 and a plain-text body
/// (`docs/api/06-saml.md`), so something has to tell a metadata document from
/// a message. It is weaker in exactly two places, both of which describe
/// *content* rather than the document:
///
/// - a *roleless* entity exports a bare `<EntityDescriptor/>` with no
///   `entityID` at all, which is a successful export that [`inspect`] rejects
///   with [`MetadataError::NoEntityId`];
/// - a certificate body this scanner cannot decode is a complaint about a
///   value, not evidence that the export failed, so certificates are not
///   fingerprinted here.
///
/// It is **not** weaker about the document's own grammar, which is what it
/// used to be. Reading only as far as the first start element classified
///
/// ```xml
/// <EntityDescriptor xmlns="urn:oasis:names:tc:SAML:2.0:metadata">
/// ```
///
/// — a start tag and nothing else — as metadata, and the CLI wrote that
/// truncation to `--out`. This drains to EOF and requires one well-formed
/// SAML `EntityDescriptor` root, so a body that survives here is a body the
/// rest of this module will also read.
pub fn validate_export_document(xml: &[u8]) -> Result<()> {
    scan(xml, Depth::Document).map(drop)
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

/// How much of a document one scan has to understand.
///
/// The document's grammar is checked identically either way — the split is
/// only about the *content* this module reports on, so that classifying an
/// export response and sanitising a file cannot disagree about what a
/// well-formed metadata document is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Depth {
    /// Well-formedness, namespaces, and the identity of the root element.
    Document,
    /// All of that, plus the `entityID` and a fingerprint for every
    /// certificate.
    Content,
}

/// One element on the open-element path, by expanded name. Local names alone
/// used to decide which role owned a key and which element was a WS-Fed role;
/// a namespace is what tells `{SAML}RoleDescriptor` from an extension's
/// element of the same name.
struct Open {
    ns: Option<Vec<u8>>,
    local: String,
}

/// A `<KeyDescriptor>` being read. Certificates are flushed at its end tag so
/// a `<KeyName>` that follows the certificate is still picked up.
///
/// A stack, not a single slot: nesting one key descriptor inside another is
/// not legal metadata, but a document that does it anyway must not silently
/// drop the outer descriptor's certificates.
struct PendingKey {
    descriptor: String,
    key_use: Option<String>,
    key_name: Option<String>,
    depth: usize,
    certs: Vec<(usize, String)>,
}

/// The `{DSIG}X509Certificate` or `{DSIG}KeyName` whose character data is
/// being accumulated.
///
/// One logical value arrives as *any number* of text, CDATA and reference
/// events — a comment, a CDATA section or a character reference splits it —
/// so the value is only complete at the element's end tag. Fingerprinting
/// each event separately reported `AAEC<!--x-->AwQ=` as two certificates,
/// neither of which the document contains.
struct PendingLeaf {
    kind: Leaf,
    depth: usize,
    line: usize,
    body: String,
}

/// The two `<KeyDescriptor>` leaves we read, by expanded name. Matching on
/// the local name alone read an `X509Certificate` in the SAML namespace — or
/// any other — as if it were XMLDSig's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Leaf {
    Certificate,
    KeyName,
}

impl Leaf {
    fn from_expanded_name(local: &str, ns: Option<&[u8]>) -> Option<Self> {
        if ns != Some(DSIG_NS) {
            return None;
        }
        match local {
            "X509Certificate" => Some(Self::Certificate),
            "KeyName" => Some(Self::KeyName),
            _ => None,
        }
    }

    fn element(self) -> &'static str {
        match self {
            Self::Certificate => "X509Certificate",
            Self::KeyName => "KeyName",
        }
    }
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

fn scan(xml: &[u8], depth: Depth) -> Result<Scan> {
    std::str::from_utf8(xml).map_err(|error| MetadataError::Malformed {
        offset: error.valid_up_to(),
        detail: "input is not valid UTF-8".into(),
    })?;

    let mut reader = NsReader::from_reader(xml);
    reader.config_mut().check_end_names = true;
    reader.config_mut().check_comments = true;
    reader.config_mut().expand_empty_elements = false;

    let mut path: Vec<Open> = Vec::new();
    let mut open_cut: Option<(usize, usize, Removal)> = None;
    let mut keys: Vec<PendingKey> = Vec::new();
    let mut leaf: Option<PendingLeaf> = None;
    let mut entity_id: Option<String> = None;
    let mut root_seen = false;
    let mut any_event_seen = false;
    let mut doctype_seen = false;

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
                // `version` is required by the grammar, and an absent one is
                // how `<?xml encoding="UTF-8"?>` used to be read as a
                // declaration at all. 1.1 is refused rather than mishandled:
                // its attribute-value normalisation and its legal-character
                // set both differ from the ones checked here.
                let version = decl.version().map_err(|error| MetadataError::Malformed {
                    offset: start,
                    detail: error.to_string(),
                })?;
                if version.as_ref() != b"1.0" {
                    return Err(MetadataError::Malformed {
                        offset: start,
                        detail: format!(
                            "unsupported XML version '{}'; only 1.0 is accepted",
                            String::from_utf8_lossy(&version)
                        ),
                    });
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
                if let Some(standalone) = decl.standalone() {
                    let standalone = standalone.map_err(|error| MetadataError::Malformed {
                        offset: start,
                        detail: error.to_string(),
                    })?;
                    if standalone.as_ref() != b"yes" && standalone.as_ref() != b"no" {
                        return Err(MetadataError::Malformed {
                            offset: start,
                            detail: format!(
                                "standalone must be 'yes' or 'no', not '{}'",
                                String::from_utf8_lossy(&standalone)
                            ),
                        });
                    }
                }
            }
            Event::DocType(doctype) => {
                // Legal in the prolog only, and at most once. We read no DTD,
                // so any entity it declares is refused where it is *used* —
                // see `Event::GeneralRef` below.
                if section != Section::Prolog {
                    return Err(misplaced(start, "a DOCTYPE declaration", section));
                }
                if doctype_seen {
                    return Err(MetadataError::Malformed {
                        offset: start,
                        detail: "a second DOCTYPE declaration".into(),
                    });
                }
                doctype_seen = true;
                let text = decoded(doctype.decode(), start)?;
                check_chars(&text, start, "the DOCTYPE declaration")?;
            }
            // Legal in all three sections, and neither is content we read.
            // Splicing carries their bytes through untouched — which is
            // exactly why their own characters still have to be legal ones.
            Event::Comment(comment) => {
                let text = decoded(comment.decode(), start)?;
                check_chars(&text, start, "a comment")?;
            }
            Event::PI(pi) => {
                check_name(pi.target(), start, "processing-instruction target")?;
                // `[xX][mM][lL]` is reserved, so `<?XML …?>` is not a legal
                // processing instruction however much it looks like one.
                if pi.target().eq_ignore_ascii_case(b"xml") {
                    return Err(MetadataError::Malformed {
                        offset: start,
                        detail: format!(
                            "'{}' is a reserved processing-instruction target",
                            String::from_utf8_lossy(pi.target())
                        ),
                    });
                }
                let text = String::from_utf8_lossy(&pi).into_owned();
                check_chars(&text, start, "a processing instruction")?;
            }
            Event::Start(element) | Event::Empty(element) => {
                let empty = end > start && xml[start..end].ends_with(b"/>");
                check_qname(element.name().as_ref(), start)?;
                let name = local_name(&element);
                // Read every attribute now, resolved, so an undeclared prefix
                // on one we never look at is still a refusal.
                let attrs = Attrs::read(&element, reader.resolver(), start)?;

                // `<ds:X509Certificate>` holds one base64 value. An element
                // inside it means the content is not that value, and
                // concatenating the text either side of the child is how a
                // fingerprint gets invented for bytes nobody wrote.
                if let Some(open_leaf) = &leaf {
                    return Err(MetadataError::Malformed {
                        offset: start,
                        detail: format!("an element inside <{}>", open_leaf.kind.element()),
                    });
                }

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

                // A role is a *direct child* of the root. An `<Extensions>`
                // container may hold anything at all, including an element
                // shaped exactly like a role, and none of it is a role this
                // entity publishes.
                let entity_role = path.len() == 1;
                if entity_role
                    && let Some(role) = Role::from_expanded_name(&name, ns)
                    && !scan.roles.contains(&role)
                {
                    scan.roles.push(role);
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
                        descriptor: enclosing_role(&path).map(str::to_owned),
                        kind: name.clone(),
                        binding: binding.to_owned(),
                        location: location.to_owned(),
                    });
                }
                if name == "KeyDescriptor" && ns == Some(SAML_NS) {
                    // The schema puts a key descriptor under a role and
                    // nowhere else, and the role is the identity a rotation
                    // needs. One somewhere else is refused rather than
                    // attributed to a role it does not belong to, or reported
                    // with no role at all.
                    let Some(descriptor) = enclosing_role(&path).filter(|_| path.len() == 2) else {
                        return Err(MetadataError::Malformed {
                            offset: start,
                            detail: "<KeyDescriptor> outside a role descriptor".into(),
                        });
                    };
                    keys.push(PendingKey {
                        descriptor: descriptor.to_owned(),
                        key_use: attrs.unqualified("use").map(str::to_owned),
                        key_name: None,
                        depth: path.len(),
                        certs: Vec::new(),
                    });
                }
                if !empty
                    && !keys.is_empty()
                    && let Some(kind) = Leaf::from_expanded_name(&name, ns)
                {
                    leaf = Some(PendingLeaf {
                        kind,
                        depth: path.len(),
                        line: line_at(xml, start),
                        body: String::new(),
                    });
                }

                // Only the outermost match is recorded: a strip-list element
                // nested inside one already being removed goes with it, and
                // reporting it separately would tell an operator about bytes
                // that were never theirs to keep.
                let strip = if enveloped_signature {
                    Some((RemovalReason::EnvelopedSignature, None))
                } else if entity_role {
                    wsfed_role_type(&name, ns, &attrs, reader.resolver(), start)?
                        .map(|xsi_type| (RemovalReason::UnsupportedRole, Some(xsi_type)))
                } else {
                    None
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
                    path.push(Open {
                        ns: ns.map(<[u8]>::to_vec),
                        local: name,
                    });
                }
            }
            Event::End(_) => {
                let closed = path.pop();
                // The leaf's value is whole only now, however many events it
                // arrived in.
                if let Some(done) = leaf.take_if(|open| open.depth == path.len())
                    && let Some(key) = keys.last_mut()
                {
                    match done.kind {
                        Leaf::Certificate => key.certs.push((done.line, done.body)),
                        Leaf::KeyName => key.key_name = Some(done.body.trim().to_owned()),
                    }
                }
                if closed.as_ref().is_some_and(|open| {
                    open.local == "KeyDescriptor" && open.ns.as_deref() == Some(SAML_NS)
                }) && keys.last().is_some_and(|key| key.depth == path.len())
                {
                    let key = keys.pop().expect("just checked there is one");
                    if depth == Depth::Content {
                        for (line, body) in &key.certs {
                            scan.certs.push(CertRef {
                                descriptor: key.descriptor.clone(),
                                key_use: key.key_use.clone(),
                                key_name: key.key_name.clone(),
                                sha256: fingerprint(body, *line)?,
                            });
                        }
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
                let raw = decoded(text.decode(), start)?;
                check_chars(&raw, start, "text")?;
                // `]]>` is forbidden in character data outright: it is how a
                // CDATA section ends, so a document carrying one literally
                // means something different once anything re-serialises it.
                if let Some(index) = raw.find("]]>") {
                    return Err(MetadataError::Malformed {
                        offset: start + index,
                        detail: "']]>' in character data".into(),
                    });
                }
                match section {
                    // Character data we do not read is preserved by splicing;
                    // character data inside a key leaf is accumulated.
                    Section::Root => {
                        if let Some(open_leaf) = leaf.as_mut() {
                            open_leaf.body.push_str(&raw);
                        }
                    }
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
                let body = decoded(cdata.decode(), start)?;
                check_chars(&body, start, "a CDATA section")?;
                if let Some(open_leaf) = leaf.as_mut() {
                    open_leaf.body.push_str(&body);
                }
            }
            Event::GeneralRef(reference) => {
                if section != Section::Root {
                    return Err(misplaced(start, "an entity reference", section));
                }
                let name = decoded(reference.decode(), start)?.into_owned();
                // A reference is part of the character data around it, so a
                // value we read has to take its replacement text — but only
                // when we can be sure it *has* one. A DTD-declared entity
                // does not qualify: we never read the DTD. A character
                // reference has to name a character XML actually permits;
                // `&#x110000;` names nothing at all.
                let replacement = if reference.is_char_ref() {
                    char_reference(&name).ok_or_else(|| MetadataError::Malformed {
                        offset: start,
                        detail: format!("'&{name};' is not a legal XML character"),
                    })?
                } else {
                    PREDEFINED_ENTITIES
                        .iter()
                        .find(|(entity, _)| *entity == name)
                        .map(|(_, replacement)| *replacement)
                        .ok_or_else(|| MetadataError::Malformed {
                            offset: start,
                            detail: format!("undeclared entity reference '&{name};'"),
                        })?
                };
                if let Some(open_leaf) = leaf.as_mut() {
                    open_leaf.body.push(replacement);
                }
            }
            Event::Eof => break,
        }
    }

    if !root_seen {
        return Err(MetadataError::Empty);
    }
    if let Some(open) = path.last() {
        return Err(MetadataError::Malformed {
            offset: xml.len(),
            detail: format!("unclosed <{}>", open.local),
        });
    }
    if depth == Depth::Content {
        scan.entity_id = entity_id.ok_or(MetadataError::NoEntityId)?;
    }
    Ok(scan)
}

/// An event that tokenised cleanly but is not legal where it appeared.
fn misplaced(offset: usize, what: &str, section: Section) -> MetadataError {
    MetadataError::Malformed {
        offset,
        detail: format!("{what} {}", section.placement()),
    }
}

/// One place to turn `quick_xml`'s decode failure into this module's refusal.
/// Each event type has its own inherent `decode`, so this takes the result
/// rather than the event.
fn decoded<'a, E: fmt::Display>(
    text: std::result::Result<std::borrow::Cow<'a, str>, E>,
    offset: usize,
) -> Result<std::borrow::Cow<'a, str>> {
    text.map_err(|error| MetadataError::Malformed {
        offset,
        detail: error.to_string(),
    })
}

// ── XML 1.0 well-formedness ─────────────────────────────────────────────────
//
// `quick_xml` is a tokeniser: it says where the angle brackets are, not
// whether what sits between them is XML. Everything below is a production the
// specification requires and the tokeniser does not check, and it runs inside
// the one `scan` every entry point shares — so a document `sanitise` refuses
// cannot be admitted by an export classification, which is how the two used to
// disagree.
//
// This is not a validator: no DTD or schema is read, and nothing here is about
// what SAML says a document should contain. It is the boundary of what this
// module is willing to claim it understood.

/// XML 1.0 `Char` — the characters a document may contain at all. Rust's
/// `char` has already excluded the surrogates, so what is left is the C0
/// controls other than tab, newline and carriage return, plus the two
/// non-characters at the end of the BMP.
fn is_xml_char(character: char) -> bool {
    matches!(character,
        '\t' | '\n' | '\r'
        | ' '..='\u{d7ff}'
        | '\u{e000}'..='\u{fffd}'
        | '\u{10000}'..='\u{10ffff}')
}

fn check_chars(text: &str, offset: usize, what: &str) -> Result<()> {
    match text
        .char_indices()
        .find(|(_, character)| !is_xml_char(*character))
    {
        Some((index, character)) => Err(MetadataError::Malformed {
            offset: offset + index,
            detail: format!(
                "U+{:04X} is not a legal XML character, in {what}",
                character as u32
            ),
        }),
        None => Ok(()),
    }
}

/// XML 1.0 `NameStartChar`.
fn is_name_start(character: char) -> bool {
    matches!(character,
        'A'..='Z' | 'a'..='z' | '_'
        | '\u{c0}'..='\u{d6}' | '\u{d8}'..='\u{f6}' | '\u{f8}'..='\u{2ff}'
        | '\u{370}'..='\u{37d}' | '\u{37f}'..='\u{1fff}'
        | '\u{200c}'..='\u{200d}' | '\u{2070}'..='\u{218f}'
        | '\u{2c00}'..='\u{2fef}' | '\u{3001}'..='\u{d7ff}'
        | '\u{f900}'..='\u{fdcf}' | '\u{fdf0}'..='\u{fffd}'
        | '\u{10000}'..='\u{effff}')
}

/// XML 1.0 `NameChar`, minus the colon — which is `NCName`, the half of a
/// qualified name Namespaces in XML allows.
fn is_name_char(character: char) -> bool {
    is_name_start(character)
        || matches!(character,
            '-' | '.' | '0'..='9' | '\u{b7}'
            | '\u{300}'..='\u{36f}' | '\u{203f}'..='\u{2040}')
}

fn is_ncname(text: &str) -> bool {
    let mut characters = text.chars();
    characters.next().is_some_and(is_name_start) && characters.all(is_name_char)
}

/// An XML `Name`, where a colon is still a legal character: a
/// processing-instruction target.
fn check_name(name: &[u8], offset: usize, what: &str) -> Result<()> {
    let text = std::str::from_utf8(name).map_err(|error| MetadataError::Malformed {
        offset,
        detail: error.to_string(),
    })?;
    let legal = text
        .split(':')
        .collect::<Vec<_>>()
        .split_first()
        .is_some_and(|(first, rest)| {
            is_ncname(first) && rest.iter().all(|part| part.chars().all(is_name_char))
        });
    if legal {
        Ok(())
    } else {
        Err(MetadataError::Malformed {
            offset,
            detail: format!("'{text}' is not a legal {what}"),
        })
    }
}

/// An element or attribute name, as Namespaces in XML requires it: an
/// `NCName`, or `prefix:local` with both halves `NCName`s. `<1bad/>` and
/// `<a:b:c/>` are both refused — the tokeniser hands back either as a name.
fn check_qname(name: &[u8], offset: usize) -> Result<()> {
    let text = std::str::from_utf8(name).map_err(|error| MetadataError::Malformed {
        offset,
        detail: error.to_string(),
    })?;
    let legal = match text.split_once(':') {
        Some((prefix, local)) => is_ncname(prefix) && is_ncname(local),
        None => is_ncname(text),
    };
    if legal {
        Ok(())
    } else {
        Err(MetadataError::Malformed {
            offset,
            detail: format!("'{text}' is not a legal element or attribute name"),
        })
    }
}

/// The character a `&#…;` reference stands for, or `None` when it names no
/// character XML permits. Only a lowercase `x` introduces hexadecimal, so
/// `&#X41;` is not a character reference at all.
fn char_reference(spelling: &str) -> Option<char> {
    let digits = spelling.strip_prefix('#')?;
    let code = match digits.strip_prefix('x') {
        Some(hexadecimal) => u32::from_str_radix(hexadecimal, 16).ok()?,
        None => digits.parse::<u32>().ok()?,
    };
    let character = char::from_u32(code)?;
    is_xml_char(character).then_some(character)
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
            check_qname(attribute.key.as_ref(), offset)?;
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
            let local = String::from_utf8_lossy(local.as_ref()).into_owned();
            let value = value.into_owned();
            check_chars(&value, offset, &format!("the value of '{local}'"))?;
            // `quick_xml` catches two attributes spelled the same. Namespaces
            // in XML forbids two that *resolve* the same, which two prefixes
            // bound to one URI do while spelling differently.
            if items.iter().any(
                |(item_ns, item_local, _): &(Option<Vec<u8>>, String, String)| {
                    *item_ns == ns && *item_local == local
                },
            ) {
                return Err(MetadataError::Malformed {
                    offset,
                    detail: format!(
                        "two attributes share the expanded name '{local}'{}",
                        match &ns {
                            Some(uri) => format!(" in {}", String::from_utf8_lossy(uri)),
                            None => String::new(),
                        }
                    ),
                });
            }
            items.push((ns, local, value));
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

/// The entity role everything under it belongs to: the *direct child* of
/// `EntityDescriptor` the current element sits in, when that child is a role
/// descriptor in the SAML namespace.
///
/// Depth is the whole point. Searching up the path for the nearest name
/// ending in `Descriptor` attributed an `<Extensions>` container's own
/// contents — vendor elements in a vendor namespace — to a role that never
/// declared them. `EntityDescriptor` is the document itself and
/// `KeyDescriptor` is a key, so neither is a role.
fn enclosing_role(path: &[Open]) -> Option<&str> {
    let role = path.get(1)?;
    (role.ns.as_deref() == Some(SAML_NS)
        && role.local.ends_with("Descriptor")
        && role.local != "KeyDescriptor")
        .then_some(role.local.as_str())
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
    // Base64 decodes an empty body to an empty DER quite happily, and the
    // SHA-256 of nothing looks exactly like a fingerprint.
    if packed.is_empty() {
        return Err(MetadataError::BadCertificate {
            line,
            detail: "the element is empty".into(),
        });
    }
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
    const PROTOCOL_URI: &str = "urn:oasis:names:tc:SAML:2.0:protocol";

    /// `inner` inside a `<KeyDescriptor>` in the one place the schema puts
    /// one — under a role descriptor. Placement is checked, so a case about
    /// what a key descriptor *contains* has to put it somewhere legal or it
    /// proves nothing about its contents.
    fn keyed(inner: &str) -> String {
        format!(
            "<EntityDescriptor xmlns=\"{SAML_URI}\" xmlns:ds=\"{DSIG_URI}\" \
             entityID=\"https://sp-a.example.com\">\
             <IDPSSODescriptor protocolSupportEnumeration=\"{PROTOCOL_URI}\">\
             <KeyDescriptor use=\"signing\"><ds:KeyInfo><ds:X509Data>{inner}\
             </ds:X509Data></ds:KeyInfo></KeyDescriptor>\
             </IDPSSODescriptor></EntityDescriptor>"
        )
    }

    /// One certificate body, in the element XMLDSig defines for it.
    fn certified(body: &str) -> String {
        keyed(&format!("<ds:X509Certificate>{body}</ds:X509Certificate>"))
    }

    /// A dual-role entity with a **different** certificate at every location:
    /// the enveloped signature, the IdP's signing key, and the SP's signing
    /// and encryption keys. Distinct bytes are what make an assertion about
    /// certificate identity discriminating — the Entra fixture publishes one
    /// certificate three times, so a scanner that counted the signature's key
    /// and dropped a role's would report the same values there.
    ///
    /// Each body is base64 for its own ASCII string, so every pinned hash can
    /// be reproduced by hand: `printf 'IDP-SIGN' | sha256sum`.
    fn dual_role() -> String {
        format!(
            "<EntityDescriptor xmlns=\"{SAML_URI}\" xmlns:ds=\"{DSIG_URI}\" \
             entityID=\"https://sp-a.example.com\">\
             <ds:Signature><ds:KeyInfo><ds:X509Data>\
             <ds:X509Certificate>U0lHTkFUVVJF</ds:X509Certificate>\
             </ds:X509Data></ds:KeyInfo></ds:Signature>\
             <IDPSSODescriptor protocolSupportEnumeration=\"{PROTOCOL_URI}\">\
             <KeyDescriptor use=\"signing\"><ds:KeyInfo><ds:X509Data>\
             <ds:X509Certificate>SURQLVNJR04=</ds:X509Certificate>\
             </ds:X509Data></ds:KeyInfo></KeyDescriptor>\
             </IDPSSODescriptor>\
             <SPSSODescriptor protocolSupportEnumeration=\"{PROTOCOL_URI}\">\
             <KeyDescriptor use=\"signing\"><ds:KeyInfo><ds:X509Data>\
             <ds:X509Certificate>U1AtU0lHTg==</ds:X509Certificate>\
             </ds:X509Data></ds:KeyInfo></KeyDescriptor>\
             <KeyDescriptor use=\"encryption\"><ds:KeyInfo>\
             <ds:KeyName>sp-a-encryption</ds:KeyName><ds:X509Data>\
             <ds:X509Certificate>U1AtRU5DUllQVA==</ds:X509Certificate>\
             </ds:X509Data></ds:KeyInfo></KeyDescriptor>\
             </SPSSODescriptor></EntityDescriptor>"
        )
    }

    /// The SHA-256 of `SIGNATURE`, the one certificate in [`dual_role`] that
    /// no `<KeyDescriptor>` publishes.
    const SIGNATURE_CERT: &str = "45a7c54c84f0cbde87f6c2f6d221058ff112a419c588f4facbd0b7b5da153e8e";
    const IDP_SIGNING_CERT: &str =
        "e3a2e9142485d2a82cf0bb7f2c241fcb277bfe68adf462a19689408a88cc9686";
    const SP_SIGNING_CERT: &str =
        "2ee7db797991bb74864b2ffa80725d7f25de05d292a082c6fa238cfc08c7d9fb";
    const SP_ENCRYPTION_CERT: &str =
        "71bc4684b28c5172dd874fd6881c32bac667ffea749c4b1444ef0cf404007ffa";

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
        // two of them inside the RoleDescriptors, and the enveloped signature
        // carries a fourth copy. The signature's copy identifies the signer,
        // not a key the entity uses, and must not be counted.
        //
        // Because every copy is the same certificate, this fixture can only
        // show that by *placement* — a fourth entry, published by no role.
        // `a_certificate_is_identified_by_the_role_that_publishes_it` is where
        // distinct bytes make the same point by value.
        const ENTRA_CERT: &str = "5f50984266ddca8c23154e9c6549ddf6c38f5b3b6f32d9315e8108000cebf33c";
        let raw = cert_refs(ENTRA.as_bytes()).expect("fixture parses");
        assert_eq!(
            raw.iter().map(identity).collect::<Vec<_>>(),
            vec![
                ("RoleDescriptor", Some("signing"), None, ENTRA_CERT),
                ("RoleDescriptor", Some("signing"), None, ENTRA_CERT),
                ("IDPSSODescriptor", Some("signing"), None, ENTRA_CERT),
            ]
        );

        // Stripping the WS-Federation roles takes two of the three with it.
        let cleaned = cert_refs(&clean(ENTRA).bytes).expect("output parses");
        assert_eq!(
            cleaned.iter().map(identity).collect::<Vec<_>>(),
            vec![("IDPSSODescriptor", Some("signing"), None, ENTRA_CERT)]
        );
    }

    /// `(role, use, key name, fingerprint)` — the identity a rotation has to
    /// address a certificate by.
    fn identity(cert: &CertRef) -> (&str, Option<&str>, Option<&str>, &str) {
        (
            cert.descriptor.as_str(),
            cert.key_use.as_deref(),
            cert.key_name.as_deref(),
            cert.sha256.as_str(),
        )
    }

    #[test]
    fn a_certificate_is_identified_by_the_role_that_publishes_it() {
        // Both roles publish `use="signing"` with no `KeyName`, so `use` plus
        // fingerprint names two *different* keys identically. The role is the
        // missing half of the identity, and a rotation that replaces the wrong
        // one breaks federation with a peer it never meant to touch.
        let certs = cert_refs(dual_role().as_bytes()).expect("fixture parses");
        assert_eq!(
            certs.iter().map(identity).collect::<Vec<_>>(),
            vec![
                ("IDPSSODescriptor", Some("signing"), None, IDP_SIGNING_CERT),
                ("SPSSODescriptor", Some("signing"), None, SP_SIGNING_CERT),
                (
                    "SPSSODescriptor",
                    Some("encryption"),
                    Some("sp-a-encryption"),
                    SP_ENCRYPTION_CERT
                ),
            ]
        );
        // The discriminating case for "no signature key": its certificate is
        // distinct here, so counting it would change a value and not just a
        // count.
        assert!(
            !certs.iter().any(|cert| cert.sha256 == SIGNATURE_CERT),
            "the signature's own certificate was reported as a key: {certs:?}"
        );
    }

    #[test]
    fn a_certificate_split_across_events_is_one_certificate() {
        // `AAECAwQ=` is the DER 00 01 02 03 04 however the file breaks it up:
        // a comment, a CDATA section, a character reference and a line wrap
        // all split the character data into several events. Fingerprinting
        // each event on its own reported two certificates — neither of them a
        // value the document contains.
        const WHOLE: &str = "08bb5e5d6eaac1049ede0893d30ed022b1a4d9b5b48db414871f51c9cb35283d";
        let cases = [
            ("one run of text", "AAECAwQ="),
            ("split by a comment", "AAEC<!--split-->AwQ="),
            ("split by a CDATA section", "AAEC<![CDATA[AwQ=]]>"),
            ("split by a character reference", "AAEC&#65;wQ="),
            (
                "wrapped across lines, as AM writes it",
                "AAEC\n          AwQ=",
            ),
        ];
        for (what, body) in cases {
            let certs = cert_refs(certified(body).as_bytes())
                .unwrap_or_else(|error| panic!("{what}: {error}"));
            assert_eq!(certs.len(), 1, "{what}: {certs:?}");
            assert_eq!(certs[0].sha256, WHOLE, "{what}");
        }
    }

    #[test]
    fn an_x509_certificate_outside_xmldsig_is_not_one() {
        // The leaf matcher used to check the local name only. The SAML
        // namespace is the *default* one in every metadata document, so an
        // unprefixed `<X509Certificate>` — which is not XMLDSig's element at
        // all — was read as if it were.
        let cases = [
            (
                "the SAML default namespace",
                "<X509Certificate>AAECAwQ=</X509Certificate>",
            ),
            (
                "a foreign namespace",
                "<x:X509Certificate xmlns:x=\"urn:x\">AAECAwQ=</x:X509Certificate>",
            ),
        ];
        for (what, leaf) in cases {
            assert!(
                cert_refs(keyed(leaf).as_bytes())
                    .expect("document parses")
                    .is_empty(),
                "{what}: an element of that name was fingerprinted"
            );
        }
    }

    #[test]
    fn cert_refs_reads_a_wrapped_body_and_the_key_name_beside_it() {
        // AM wraps the base64 at 64 columns and names its keys; Entra does
        // neither. The fingerprints are pinned, because hashing the base64
        // *text* rather than the DER it decodes to would also produce two
        // distinct 64-character strings.
        let certs = cert_refs(AM_SP.as_bytes()).expect("fixture parses");
        assert_eq!(
            certs.iter().map(identity).collect::<Vec<_>>(),
            vec![
                (
                    "SPSSODescriptor",
                    Some("signing"),
                    Some("sp-a-signing"),
                    "b1367ec4f9f90099e9351f917720c544bef4fba40b3edb8d3ad9bbe933851834"
                ),
                (
                    "SPSSODescriptor",
                    Some("encryption"),
                    Some("sp-a-encryption"),
                    "e6b72773774ea667c0620ecbcf91372b1ab8281468588cf47edee63ba09a2e4f"
                ),
            ]
        );
    }

    // ── failing closed ──────────────────────────────────────────────────────

    /// Whether a refusal is about the document's own grammar — in which case
    /// classifying an export response has to refuse it too — or about
    /// content, which a *successful* export is allowed to carry and
    /// [`validate_export_document`] must therefore still accept.
    ///
    /// The column is not bookkeeping: it is where the two depths are asserted
    /// to differ exactly as intended, rather than by accident.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Reach {
        Document,
        ContentOnly,
    }

    #[test]
    fn unreadable_or_unexpected_input_is_refused_rather_than_passed_through() {
        use Reach::{ContentOnly, Document};

        // Each case must fail for the reason its name gives, so every one that
        // gets as far as the root element declares the SAML namespace: without
        // it the root check fires first and the case proves nothing.
        let root = format!("<EntityDescriptor xmlns=\"{SAML_URI}\"");
        let entity = format!("{root} entityID=\"https://sp-a.example.com\"");
        let text =
            |body: &str| format!("{entity}><Organization>{body}</Organization></EntityDescriptor>");
        let cases: &[(&str, String, Reach)] = &[
            ("unclosed element", format!("{entity}>"), Document),
            ("mismatched end tag", format!("{entity}></Other>"), Document),
            (
                "not metadata at all",
                "<html><body>hello</body></html>".to_string(),
                Document,
            ),
            (
                "an aggregate of entities",
                format!("<EntitiesDescriptor>{entity} /></EntitiesDescriptor>"),
                Document,
            ),
            // The one refusal an export is allowed to produce: a roleless
            // entity really does export a bare descriptor with no entityID.
            ("no entityID", format!("{root} />"), ContentOnly),
            ("empty input", String::new(), Document),
            (
                "two top-level EntityDescriptors",
                format!("{entity}/><EntityDescriptor xmlns=\"{SAML_URI}\" entityID=\"https://sp-b.example.com\"/>"),
                Document,
            ),
            (
                "an XML-invalid comment",
                format!("{entity}><!-- bad -- comment --></EntityDescriptor>"),
                Document,
            ),
            (
                "a root in some other namespace",
                "<html:EntityDescriptor xmlns:html=\"urn:not-saml\" entityID=\"https://sp-a.example.com\"/>".to_string(),
                Document,
            ),
            (
                "a certificate that is not base64",
                certified("not base64 !!"),
                ContentOnly,
            ),
            (
                // Base64 decodes nothing to nothing quite happily, and the
                // SHA-256 of an empty DER looks exactly like a fingerprint.
                "an empty certificate element",
                certified(""),
                ContentOnly,
            ),
            (
                // One value, or text either side of a child element? There is
                // no answer, so there is no certificate.
                "an element inside a certificate body",
                keyed("<ds:X509Certificate>AA<ds:Other/>EC</ds:X509Certificate>"),
                Document,
            ),
            (
                // The schema puts a key descriptor under a role, and the role
                // is the identity a rotation addresses the key by. Reporting
                // one from anywhere else would have to invent that role or
                // omit it.
                "a KeyDescriptor outside a role descriptor",
                format!(
                    "{entity}><KeyDescriptor use=\"signing\"/></EntityDescriptor>"
                ),
                Document,
            ),
            (
                "an undeclared prefix on xsi:type's QName",
                format!(
                    "{entity}><RoleDescriptor xmlns:xsi=\"{XSI_URI}\" \
                     xsi:type=\"nope:SecurityTokenServiceType\"/></EntityDescriptor>"
                ),
                Document,
            ),
            (
                "an undeclared xsi prefix on RoleDescriptor",
                format!(
                    "{entity}><RoleDescriptor xsi:type=\"fed:SecurityTokenServiceType\" \
                     xmlns:fed=\"{WSFED_URI}\"/></EntityDescriptor>"
                ),
                Document,
            ),
            // ── namespace-resolved attribute names ──────────────────────────
            (
                // Nothing reads `bad:attr`, which is the point: an element
                // carrying a name we cannot expand is an element we cannot
                // describe.
                "an undeclared prefix on an attribute nothing reads",
                format!("{entity} bad:attr=\"1\"/>"),
                Document,
            ),
            (
                // `entityID` is unqualified in the schema. Matching on the
                // local name alone let a foreign vocabulary's attribute
                // satisfy the one AM compares assertions against.
                "a prefixed entityID standing in for the required one",
                format!("{root} xmlns:ext=\"urn:x\" ext:entityID=\"https://sp-a.example.com\"/>"),
                // The *document* is well-formed; what is missing is the
                // entityID, which a roleless export legitimately omits.
                ContentOnly,
            ),
            // ── document sections ───────────────────────────────────────────
            ("text before the root element", format!("hello{entity}/>"), Document),
            (
                "an XML declaration that is not the first thing",
                format!("<!-- c -->\n<?xml version=\"1.0\"?>{entity}/>"),
                Document,
            ),
            (
                "a second XML declaration after the root",
                format!("<?xml version=\"1.0\"?>{entity}/><?xml version=\"1.0\"?>"),
                Document,
            ),
            (
                "a DOCTYPE after the root",
                format!("{entity}/><!DOCTYPE EntityDescriptor>"),
                Document,
            ),
            (
                "a CDATA section after the root",
                format!("{entity}/><![CDATA[trailing]]>"),
                Document,
            ),
            (
                "an entity reference after the root",
                format!("{entity}/>&amp;"),
                Document,
            ),
            // ── document grammar a tokeniser walks straight past ────────────
            // Every one of these was accepted and passed through unchanged:
            // with no selected removals, `sanitise` emitted the malformed
            // bytes verbatim, which is the principal way this module could
            // produce output that is not well-formed XML.
            (
                "a second DOCTYPE declaration",
                format!("<!DOCTYPE EntityDescriptor><!DOCTYPE EntityDescriptor>{entity}/>"),
                Document,
            ),
            (
                "an XML declaration with no version",
                format!("<?xml encoding=\"UTF-8\"?>{entity}/>"),
                Document,
            ),
            (
                "an XML version we do not implement",
                format!("<?xml version=\"1.1\"?>{entity}/>"),
                Document,
            ),
            (
                "the reserved <?XML ...?> processing-instruction target",
                format!("<?XML data?>{entity}/>"),
                Document,
            ),
            (
                "an illegal element name",
                format!("{entity}><1bad/></EntityDescriptor>"),
                Document,
            ),
            (
                "an illegal attribute name",
                format!("{entity}><Organization 1bad=\"x\"/></EntityDescriptor>"),
                Document,
            ),
            (
                "a control character in text",
                text("a\u{8}b"),
                Document,
            ),
            (
                // `]]>` is how a CDATA section ends. Character data may not
                // spell it, however the bytes arrived.
                "a literal ]]> in text",
                text("a]]>b"),
                Document,
            ),
            (
                "a character reference outside Unicode",
                text("&#x110000;"),
                Document,
            ),
            (
                "a character reference to a forbidden control character",
                text("&#8;"),
                Document,
            ),
            (
                // Two spellings, one expanded name. Namespaces in XML forbids
                // it, and nothing about the document says which value wins.
                "two attributes sharing an expanded name",
                format!(
                    "{entity} xmlns:p=\"urn:x\" xmlns:q=\"urn:x\" p:a=\"1\" q:a=\"2\"/>"
                ),
                Document,
            ),
            // ── references we cannot resolve ────────────────────────────────
            (
                "an entity reference with no declaration to define it",
                text("&oops;"),
                Document,
            ),
            (
                // Predefined, so its replacement text is known and goes into
                // the certificate body like any other character data — which
                // is what makes the body `AAEC&AwQ=`, and not base64.
                "a predefined entity inside a certificate body",
                certified("AAEC&amp;AwQ="),
                ContentOnly,
            ),
        ];
        for (case, xml, reach) in cases {
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
            match reach {
                Reach::Document => assert!(
                    validate_export_document(xml.as_bytes()).is_err(),
                    "{case}: export classification accepted it, and the CLI \
                     writes a classified body to --out"
                ),
                Reach::ContentOnly => assert!(
                    validate_export_document(xml.as_bytes()).is_ok(),
                    "{case}: export classification refused a body a successful \
                     export can legitimately carry"
                ),
            }
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
            assert!(
                validate_export_document(xml.as_bytes()).is_ok(),
                "{case}: export classification refused a well-formed document"
            );
        }
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

    /// `validate_export_document` is what stands between a 200 that failed and
    /// a file called `entity.xml`, so it is tested on the boundary cases
    /// `inspect` deliberately treats differently — and on the malformed ones
    /// it used to wave through, because it stopped reading at the first start
    /// element.
    #[test]
    fn validate_export_document_accepts_one_whole_entity_descriptor() {
        let cases: [(&str, String, bool); 12] = [
            ("a full document", format!("{HEAD}{TAIL}"), true),
            (
                // inspect() rejects this with NoEntityId; the export that
                // produced it still succeeded.
                "a roleless entity's bare self-closing descriptor",
                format!("<EntityDescriptor xmlns=\"{SAML_URI}\"/>"),
                true,
            ),
            (
                "a prolog in front of it",
                format!(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!-- note -->\n\
                     <EntityDescriptor xmlns=\"{SAML_URI}\"/>"
                ),
                true,
            ),
            (
                "a prefixed but correctly bound root",
                format!("<md:EntityDescriptor xmlns:md=\"{SAML_URI}\"/>"),
                true,
            ),
            (
                // The discriminating case for the namespace check: same local
                // name, wrong namespace.
                "the right local name in the wrong namespace",
                "<EntityDescriptor xmlns=\"urn:not-saml\"/>".to_string(),
                false,
            ),
            (
                "an EntitiesDescriptor aggregate",
                format!("<EntitiesDescriptor xmlns=\"{SAML_URI}\"/>"),
                false,
            ),
            (
                "the JSP's plain-text failure body",
                "ERROR : No metadata for entity found.".to_string(),
                false,
            ),
            ("nothing at all", String::new(), false),
            (
                // The bug this function was rewritten for: a correctly
                // namespaced start tag and then nothing. Reading only as far
                // as the first start element called it metadata, and the CLI
                // wrote the truncation to --out.
                "a start tag with the document truncated after it",
                format!("<EntityDescriptor xmlns=\"{SAML_URI}\">"),
                false,
            ),
            (
                "a mismatched end tag",
                format!("<EntityDescriptor xmlns=\"{SAML_URI}\"></Other>"),
                false,
            ),
            (
                "an unclosed child element",
                format!("{HEAD}\n  <SPSSODescriptor>\n</EntityDescriptor>\n"),
                false,
            ),
            (
                "a second document appended to the first",
                format!("{HEAD}{TAIL}<EntityDescriptor xmlns=\"{SAML_URI}\"/>"),
                false,
            ),
        ];

        for (what, xml, expected) in cases {
            assert_eq!(
                validate_export_document(xml.as_bytes()).is_ok(),
                expected,
                "{what}"
            );
        }
    }

    #[test]
    fn validate_export_document_rejects_bytes_that_are_not_utf8() {
        assert!(
            validate_export_document(
                b"<EntityDescriptor xmlns=\"urn:oasis:names:tc:SAML:2.0:metadata\">\xff\xfe"
            )
            .is_err()
        );
    }

    // ── extension content is not ours ───────────────────────────────────────

    #[test]
    fn a_ws_fed_shaped_element_inside_an_extension_is_left_alone() {
        // `<Extensions>` holds whatever a vendor puts there, including an
        // element shaped exactly like a role. The selector used to apply at
        // every depth, so sanitising deleted an extension's own content —
        // bytes this tool never had any claim on — and `inspect` reported the
        // entity as publishing a role it does not.
        let role = format!(
            "<RoleDescriptor xmlns:xsi=\"{XSI_URI}\" xmlns:fed=\"{WSFED_URI}\" \
             xsi:type=\"fed:SecurityTokenServiceType\"><fed:Note>keep me</fed:Note>\
             </RoleDescriptor>"
        );
        let nested = format!(
            "{HEAD}\n  <Extensions>\n    <ext:Container xmlns:ext=\"urn:x\">\n      \
             {role}\n      <IDPSSODescriptor protocolSupportEnumeration=\"{PROTOCOL_URI}\">\n        \
             <SingleSignOnService Binding=\"urn:x\" Location=\"https://sp-b.example.com/sso\" />\n      \
             </IDPSSODescriptor>\n    </ext:Container>\n  </Extensions>\n  \
             <SPSSODescriptor protocolSupportEnumeration=\"{PROTOCOL_URI}\">\n    \
             <AssertionConsumerService Binding=\"urn:x\" Location=\"https://sp-a.example.com/acs\" />\n  \
             </SPSSODescriptor>\n</EntityDescriptor>\n"
        );

        let result = clean(&nested);
        assert_eq!(
            String::from_utf8(result.bytes).expect("output is utf-8"),
            nested,
            "extension content was rewritten"
        );
        assert!(result.removed.is_empty(), "{:?}", result.removed);
        // Role reporting has the same depth, for the same reason.
        assert_eq!(
            inspect(nested.as_bytes()).expect("parses").roles,
            vec![Role::ServiceProvider]
        );

        // Endpoints are attributed the same way, and an extension's nested
        // role is not a role — so the endpoint inside it belongs to none,
        // while the one under the real SPSSODescriptor still belongs to that.
        assert_eq!(
            inspect(nested.as_bytes())
                .expect("parses")
                .endpoints
                .iter()
                .map(|endpoint| (endpoint.descriptor.as_deref(), endpoint.kind.as_str()))
                .collect::<Vec<_>>(),
            vec![
                (None, "SingleSignOnService"),
                (Some("SPSSODescriptor"), "AssertionConsumerService"),
            ]
        );

        // The control: the same element as a direct child of the root *is* a
        // role this entity publishes, and is stripped. Without it a selector
        // that had simply stopped matching would pass the test above.
        let direct = format!("{HEAD}\n  {role}{TAIL}");
        let stripped = clean(&direct);
        assert_eq!(
            String::from_utf8(stripped.bytes.clone()).expect("output is utf-8"),
            format!("{HEAD}{TAIL}")
        );
        assert_eq!(
            report(&stripped),
            vec![
                "line 4: removed <RoleDescriptor xsi:type=\"fed:SecurityTokenServiceType\"> \
                 — not a SAML 2.0 role; on the strip list"
            ]
        );
    }
}
