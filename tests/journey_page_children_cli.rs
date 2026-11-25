//! Run the real `aic journey push` against an in-process agent fixture, for
//! a journey whose page holds child nodes the tree does not list. The socket
//! belongs to a temporary project and never connects to a tenant.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::time::Duration;

const TREES: &str =
    "/am/json/realms/root/realms/alpha/realm-config/authentication/authenticationtrees";

struct Project(PathBuf);

impl Project {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("aic-journey-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join(".aic")).unwrap();
        std::fs::write(
            root.join(".aic/config.toml"),
            "project = \"test\"\ndefault_tenant = \"stub\"\n\n[[tenant]]\nname = \"stub\"\nbase_url = \"https://tenant.example.com\"\ntheme = \"production\"\nscopes = []\n",
        )
        .unwrap();
        std::fs::write(
            root.join(".aic/settings.toml"),
            "[operator]\nname = \"tester\"\n",
        )
        .unwrap();
        Self(root)
    }

    fn journeys(&self) -> PathBuf {
        self.0.join("workspace/stub/journeys/alpha")
    }

    fn write_export(&self, export: &Value) {
        std::fs::create_dir_all(self.journeys()).unwrap();
        std::fs::write(
            self.journeys().join("Login.json"),
            serde_json::to_vec_pretty(export).unwrap(),
        )
        .unwrap();
    }

    fn write_snapshot(&self, export: &Value) {
        let dir = self.journeys().join(".snapshots");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("Login.json"),
            serde_json::to_vec_pretty(export).unwrap(),
        )
        .unwrap();
    }

    fn snapshot(&self) -> Value {
        let bytes = std::fs::read(self.journeys().join(".snapshots/Login.json")).unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    fn push(&self, tenant: Tenant) -> (Output, Tenant) {
        let mut command = Command::new(env!("CARGO_BIN_EXE_aic"));
        command
            .current_dir(&self.0)
            .env_remove("AIC_PROJECT")
            .env("XDG_DATA_HOME", self.0.join("data"))
            .env("XDG_STATE_HOME", self.0.join("state"))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .args(["--no-prompt", "journey", "push", "Login", "--yes"]);
        run(self, command, tenant)
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The journey as an export: a page holding a username collector and a
/// scripted decision, and a success-URL node after it. Only the page and the
/// success-URL node are in the tree.
fn journey() -> Value {
    json!({
        "tree": {"_id": "Login", "entryNodeId": "page", "nodes": {
            "page": {"nodeType": "PageNode", "displayName": "Page", "connections": {}},
            "zafter": {"nodeType": "SetSuccessUrlNode", "displayName": "Done", "connections": {}}
        }},
        "nodes": {
            "page": {"_id": "page", "nodes": [
                {"_id": "user", "nodeType": "UsernameCollectorNode", "displayName": "u"},
                {"_id": "script", "nodeType": "ScriptedDecisionNode", "displayName": "s"}
            ]},
            "zafter": {"_id": "zafter", "successUrl": "https://sp-a.example.com/"},
            "user": {"_id": "user", "validateInput": false},
            "script": {"_id": "script", "script": "00000000-0000-0000-0000-000000000000"}
        }
    })
}

/// The fake tenant: the journey's tree and node configs, by REST path.
struct Tenant {
    state: Value,
    /// A node id whose PUT answers 503.
    fail_put: Option<&'static str>,
    calls: Vec<(String, String)>,
}

impl Tenant {
    fn new(state: Value) -> Self {
        Self {
            state,
            fail_put: None,
            calls: Vec::new(),
        }
    }

    fn api(&mut self, request: &Value) -> Value {
        let method = request["method"].as_str().unwrap();
        let path = request["path"].as_str().unwrap();
        self.calls.push((method.into(), path.into()));
        let rest = path.strip_prefix(TREES).unwrap_or_default();
        let found = |value: Option<&Value>| match value {
            Some(value) => json!({"type": "json", "value": value}),
            None => json!({"type": "api_error", "status": 404, "body": "fixture: no such object"}),
        };
        match (method, rest.split('/').collect::<Vec<_>>().as_slice()) {
            ("GET", ["", "trees", "Login"]) => found(Some(&self.state["tree"])),
            ("GET", ["", "nodes", _, id]) => found(self.state["nodes"].get(*id)),
            ("PUT", ["", "nodes", _, id]) => {
                if self.fail_put == Some(*id) {
                    return json!({"type": "api_error", "status": 503, "body": "fixture: unavailable"});
                }
                let mut body = request["body"].clone();
                body["_id"] = json!(id);
                self.state["nodes"][*id] = body.clone();
                found(Some(&body))
            }
            ("PUT", ["", "trees", "Login"]) => {
                let mut body = request["body"].clone();
                body["_id"] = json!("Login");
                self.state["tree"] = body.clone();
                found(Some(&body))
            }
            _ => panic!("unexpected request {method} {path}"),
        }
    }

    fn writes(&self) -> Vec<String> {
        self.calls
            .iter()
            .filter(|(method, _)| method != "GET")
            .map(|(method, path)| format!("{method} {}", path.strip_prefix(TREES).unwrap()))
            .collect()
    }
}

fn run(project: &Project, mut command: Command, mut tenant: Tenant) -> (Output, Tenant) {
    let listener = UnixListener::bind(project.0.join(".aic/agent.sock")).unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut child = command.spawn().unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if std::time::Instant::now() >= deadline {
            child.kill().unwrap();
            panic!(
                "CLI exceeded test deadline: {:?}",
                child.wait_with_output().unwrap()
            );
        }
        let (mut stream, _) = match listener.accept() {
            Ok(connection) => connection,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(1));
                continue;
            }
            Err(error) => panic!("{error}"),
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let mut line = String::new();
        BufReader::new(&stream).read_line(&mut line).unwrap();
        let request: Value = serde_json::from_str(&line).unwrap();
        let response = match request["op"].as_str().unwrap() {
            "ping" => json!({"type":"pong","version":"test","pid":1}),
            "status" => json!({"type":"status","unlocked":true,"project_dir":"/tmp/aic-test",
                "tenants":["stub"],"cached_tokens":[],"idle_remaining_secs":100,"idle_timeout_secs":3600}),
            "api_call" => tenant.api(&request),
            other => panic!("unexpected agent operation: {other}"),
        };
        let _ = writeln!(stream, "{response}");
    }
    (child.wait_with_output().unwrap(), tenant)
}

/// A change made on the tenant to a page's child alone must block an unforced
/// push, exactly as a change to a tree-listed node does. Before children were
/// carried, the child was in neither the snapshot nor the remote read, so
/// this push went through and overwrote it. The control is the same push
/// with the tenant unchanged, which writes.
#[test]
fn a_tenant_change_to_a_page_child_alone_blocks_an_unforced_push() {
    for drifted in [true, false] {
        let project = Project::new();
        project.write_snapshot(&journey());
        let mut local = journey();
        local["nodes"]["zafter"]["successUrl"] = json!("https://sp-b.example.com/");
        project.write_export(&local);
        let mut remote = journey();
        if drifted {
            remote["nodes"]["user"]["validateInput"] = json!(true);
        }

        let (output, tenant) = project.push(Tenant::new(remote));
        let stderr = String::from_utf8_lossy(&output.stderr);
        if drifted {
            assert_eq!(output.status.code(), Some(1), "{stderr}");
            assert!(
                stderr.contains("changed since you last pulled (1 nodes)"),
                "{stderr}"
            );
            assert!(tenant.writes().is_empty(), "{:?}", tenant.writes());
            assert_eq!(tenant.state["nodes"]["user"]["validateInput"], true);
        } else {
            assert!(output.status.success(), "{stderr}");
            assert_eq!(
                tenant.state["nodes"]["zafter"]["successUrl"],
                "https://sp-b.example.com/"
            );
        }
    }
}

/// Regression (review P1-1): a push that failed on its second child returned
/// the bare 503, though the first child was already rewritten on the tenant.
/// The CLI must name what landed, write nothing after the failure, and leave
/// the snapshot alone.
#[test]
fn a_push_failing_on_a_second_child_names_the_first_and_keeps_the_snapshot() {
    let project = Project::new();
    project.write_snapshot(&journey());
    let mut local = journey();
    local["nodes"]["user"]["validateInput"] = json!(true);
    project.write_export(&local);
    let mut tenant = Tenant::new(journey());
    tenant.fail_put = Some("script");

    let (output, tenant) = project.push(tenant);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert_eq!(
        tenant.writes(),
        [
            "PUT /nodes/UsernameCollectorNode/user",
            "PUT /nodes/ScriptedDecisionNode/script"
        ]
    );
    assert!(
        stderr.contains("1 of 5 writes landed, then ScriptedDecisionNode/script failed"),
        "{stderr}"
    );
    assert!(
        stderr.contains("already written to the tenant: UsernameCollectorNode/user"),
        "{stderr}"
    );
    assert_eq!(tenant.state["nodes"]["user"]["validateInput"], true);
    assert_eq!(project.snapshot(), journey());
}
