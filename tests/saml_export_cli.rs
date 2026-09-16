//! `aic saml metadata export` against a stub AM.
//!
//! These exist because the export endpoint's two hazards are both invisible to
//! a unit test of the classifier:
//!
//! 1. **A failed export is HTTP 200** with a plain-text body, so a `run` arm
//!    that wrote the response straight to `--out` would look perfectly healthy
//!    and save `ERROR : …` under an `.xml` name. Only driving the binary shows
//!    whether the classification is actually wired into the write.
//! 2. **The endpoint takes no authentication**, so the request must carry no
//!    `Authorization` header and must work with no agent running at all. Both
//!    are properties of the request this test can read directly.
//!
//! The stub is a raw `TcpListener` rather than a mock-HTTP dependency: one
//! canned response per connection is all AM's JSP does here.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::mpsc;

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_aic"))
}

/// A one-shot HTTP server returning `body` with a 200, as the JSP does for
/// *both* outcomes. Hands back the request head it received.
struct StubAm {
    base_url: String,
    request: mpsc::Receiver<String>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl StubAm {
    fn serving(body: &'static str, content_type: Option<&'static str>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind a stub AM");
        let port = listener.local_addr().expect("stub port").port();
        let (tx, request) = mpsc::channel();
        let handle = std::thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
            let mut head = String::new();
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
                head.push_str(&line);
            }
            let _ = tx.send(head);
            let mut response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n",
                body.len()
            );
            if let Some(content_type) = content_type {
                response.push_str(&format!("Content-Type: {content_type}\r\n"));
            }
            response.push_str("\r\n");
            response.push_str(body);
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
            let _ = stream.shutdown(Shutdown::Write);
            // Drain anything still in flight so the client sees a clean close.
            let mut sink = Vec::new();
            let _ = reader.read_to_end(&mut sink);
        });
        Self {
            base_url: format!("http://127.0.0.1:{port}"),
            request,
            handle: Some(handle),
        }
    }

    fn request_head(&mut self) -> String {
        let head = self
            .request
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("the CLI made a request");
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        head
    }
}

/// A project root holding nothing but `.aic/config.toml`. No vault, no agent,
/// no key: an unauthenticated verb must need none of them.
fn project_root(base_url: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("aic-saml-export-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(root.join(".aic")).expect("create project root");
    std::fs::write(
        root.join(".aic/config.toml"),
        format!(
            "project = \"test\"\n\
             default_tenant = \"stub\"\n\
             \n\
             [[tenant]]\n\
             name = \"stub\"\n\
             base_url = \"{base_url}\"\n\
             theme = \"sandbox\"\n\
             scopes = []\n"
        ),
    )
    .expect("write config");
    root
}

fn export_in(root: &Path, args: &[&str]) -> Output {
    Command::new(bin())
        .current_dir(root)
        .args(["--no-prompt", "saml", "metadata", "export"])
        .args(args)
        .output()
        .expect("run export")
}

const DESCRIPTOR: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>",
    "<EntityDescriptor xmlns=\"urn:oasis:names:tc:SAML:2.0:metadata\" ",
    "entityID=\"https://sp-a.example.com\"><SPSSODescriptor ",
    "protocolSupportEnumeration=\"urn:oasis:names:tc:SAML:2.0:protocol\"/>",
    "</EntityDescriptor>"
);

/// AM's real failure body: 200, no `Content-Type`, doubly HTML-escaped.
const ERROR_BODY: &str = "ERROR : No metadata for entity \
     &quot;https&amp;#x3a;&amp;#x2f;&amp;#x2f;sp-z.example.com&quot; \
     under realm &quot;&amp;#x2f;bravo&quot; found.";

#[test]
fn a_successful_export_writes_the_xml_and_sends_no_credential() {
    let mut stub = StubAm::serving(DESCRIPTOR, Some("text/xml;charset=utf-8"));
    let root = project_root(&stub.base_url);
    let out = root.join("entity.xml");

    let output = export_in(
        &root,
        &[
            "https://sp-a.example.com",
            "--realm",
            "bravo",
            "--out",
            out.to_str().expect("utf-8 path"),
        ],
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(&out).expect("export written"),
        DESCRIPTOR
    );

    let head = stub.request_head();
    // The endpoint is unauthenticated; handing it the service-account bearer
    // would be gratuitous exposure, and nothing in this process could mint one
    // anyway — there is no vault here.
    assert!(
        !head.to_lowercase().contains("authorization:"),
        "export sent a credential:\n{head}"
    );
    // `realm` is never defaulted server-side: omitting it selects root.
    assert!(
        head.contains("GET /am/saml2/jsp/exportmetadata.jsp?")
            && head.contains("entityid=https%3A%2F%2Fsp-a.example.com")
            && head.contains("realm=%2Fbravo"),
        "unexpected request line:\n{head}"
    );
    std::fs::remove_dir_all(root).expect("cleanup");
}

/// The test the whole verb exists to pass. Delete the `classify_export` arm in
/// `saml::cli::export` — write the body straight through — and this turns red
/// on both assertions: the command exits 0 and `entity.xml` holds `ERROR : …`.
#[test]
fn a_failed_export_arrives_as_200_and_must_not_be_written() {
    let mut stub = StubAm::serving(ERROR_BODY, None);
    let root = project_root(&stub.base_url);
    let out = root.join("entity.xml");

    let output = export_in(
        &root,
        &[
            "https://sp-z.example.com",
            "--realm",
            "bravo",
            "--out",
            out.to_str().expect("utf-8 path"),
        ],
    );
    assert!(
        !output.status.success(),
        "a 200 carrying ERROR must fail; stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        !out.exists(),
        "a failed export left a file behind: {}",
        std::fs::read_to_string(&out).unwrap_or_default()
    );
    // The tenant's own message reaches the operator, unescaped enough to read.
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("No metadata for entity \"https://sp-z.example.com\""),
        "the tenant's message did not survive: {stderr}"
    );
    let _ = stub.request_head();
    std::fs::remove_dir_all(root).expect("cleanup");
}

/// The other half of the same hazard: a body that is neither metadata nor an
/// `ERROR :` message — a login page, a proxy error — still arrives as a 200.
#[test]
fn a_200_that_is_not_metadata_is_refused_and_quoted_back() {
    let mut stub = StubAm::serving(
        "<html><head><title>Sign in</title></head></html>",
        Some("text/html"),
    );
    let root = project_root(&stub.base_url);

    let output = export_in(&root, &["https://sp-a.example.com", "--realm", "bravo"]);
    assert!(!output.status.success());
    assert!(
        output.stdout.is_empty(),
        "an unrecognised body reached stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("not SAML metadata") && stderr.contains("<html>"),
        "unhelpful rejection: {stderr}"
    );
    let _ = stub.request_head();
    std::fs::remove_dir_all(root).expect("cleanup");
}
