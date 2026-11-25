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

    /// The snapshot's and the export's bytes, so an assertion that they are
    /// untouched cannot be satisfied by a rewrite of the same JSON.
    fn files(&self) -> (Vec<u8>, Vec<u8>) {
        let read = |path: PathBuf| std::fs::read(path).unwrap();
        (
            read(self.journeys().join(".snapshots/Login.json")),
            read(self.journeys().join("Login.json")),
        )
    }

    fn push(&self, tenant: Tenant, force: &[&str]) -> (Output, Tenant) {
        self.journey(tenant, &[&["push", "Login", "--yes"], force].concat())
    }

    fn journey(&self, tenant: Tenant, args: &[&str]) -> (Output, Tenant) {
        let mut command = Command::new(env!("CARGO_BIN_EXE_aic"));
        command
            .current_dir(&self.0)
            .env_remove("AIC_PROJECT")
            .env("XDG_DATA_HOME", self.0.join("data"))
            .env("XDG_STATE_HOME", self.0.join("state"))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .args(["--no-prompt", "journey"])
            .args(args);
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
/// carried the child was in neither the snapshot nor the remote read, so the
/// drift went unnoticed (the old writer did not write `user` either). What
/// this pins is that the preflight reads every child the push writes: a
/// version writing children without comparing them would overwrite `user`.
/// The control is the same push with the tenant unchanged, which writes.
#[test]
fn journey_push_is_blocked_by_a_tenant_change_to_a_page_child_alone() {
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

        let (output, tenant) = project.push(Tenant::new(remote), &[]);
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

/// Regression (review round 1 P1-1, round 2 P1-2): a push that failed on its
/// second child returned the bare 503, though the first child was already
/// rewritten; then it counted the 503'd write as not landed, which a 503
/// does not establish. The CLI must name what was acknowledged, say the
/// failing write may have landed, write nothing after it, and leave the
/// snapshot's bytes alone.
#[test]
fn journey_push_failing_on_a_second_child_reports_three_groups_and_keeps_the_snapshot() {
    let project = Project::new();
    project.write_snapshot(&journey());
    let mut local = journey();
    local["nodes"]["user"]["validateInput"] = json!(true);
    project.write_export(&local);
    let before = project.files();
    let mut tenant = Tenant::new(journey());
    tenant.fail_put = Some("script");

    let (output, tenant) = project.push(tenant, &[]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert_eq!(
        tenant.writes(),
        [
            "PUT /nodes/UsernameCollectorNode/user",
            "PUT /nodes/ScriptedDecisionNode/script"
        ]
    );
    for expected in [
        "ScriptedDecisionNode/script may have landed",
        "acknowledged by the tenant (1): UsernameCollectorNode/user",
        "not attempted (3): PageNode/page, SetSuccessUrlNode/zafter, tree",
    ] {
        assert!(stderr.contains(expected), "{expected}: {stderr}");
    }
    assert_eq!(tenant.state["nodes"]["user"]["validateInput"], true);
    assert_eq!(project.files(), before);
}

/// Regression (review round 2 P1-1): a push that newly attached an existing
/// realm node overwrote it without reading it, since the node was in neither
/// the snapshot nor the tenant's copy of this journey. The discriminating
/// input is `borrowed`, added to Login's page locally with an older body.
/// Bare `--force` must not override this; `--force=reuse` does; equal content
/// is the control that needs no flag.
#[test]
fn journey_push_refuses_to_overwrite_an_existing_node_it_newly_attaches() {
    let with_borrowed = |valid: bool| {
        let mut export = journey();
        export["nodes"]["page"]["nodes"]
            .as_array_mut()
            .unwrap()
            .push(json!({"_id": "borrowed", "nodeType": "UsernameCollectorNode"}));
        export["nodes"]["borrowed"] = json!({"_id": "borrowed", "validateInput": valid});
        export
    };
    let tenant = |valid: bool| {
        let mut state = journey();
        state["nodes"]["borrowed"] = json!({"_id": "borrowed", "validateInput": valid});
        Tenant::new(state)
    };
    for (force, tenant_value, refused) in [
        (&[][..], true, true),
        (&["--force"][..], true, true),
        (&["--force=reuse"][..], true, false),
        (&[][..], false, false),
    ] {
        let project = Project::new();
        project.write_snapshot(&journey());
        project.write_export(&with_borrowed(false));
        let (output, tenant) = project.push(tenant(tenant_value), force);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let case = format!("{force:?} tenant={tenant_value}: {stderr}");
        assert!(
            tenant
                .calls
                .iter()
                .any(|(method, path)| method == "GET" && path.ends_with("/borrowed")),
            "{case}"
        );
        if refused {
            assert_eq!(output.status.code(), Some(1), "{case}");
            assert!(stderr.contains("UsernameCollectorNode/borrowed"), "{case}");
            assert!(stderr.contains("--force=reuse"), "{case}");
            assert!(tenant.writes().is_empty(), "{case} {:?}", tenant.writes());
            assert_eq!(tenant.state["nodes"]["borrowed"]["validateInput"], true);
        } else {
            assert!(output.status.success(), "{case}");
            assert_eq!(tenant.state["nodes"]["borrowed"]["validateInput"], false);
        }
    }
}

/// A local edit to a page's child alone is protected on pull like any other:
/// with the tenant's copy of that child also different, a pull with no
/// terminal and no `--force` refuses and leaves both files' bytes as they
/// were. The discriminating input is the edit to `user`, which only a pull
/// that compares children can see; the control is the same pull with the
/// local export unedited, which installs the tenant's copy.
#[test]
fn journey_pull_refuses_to_overwrite_a_local_edit_to_a_page_child_alone() {
    for edited in [true, false] {
        let project = Project::new();
        project.write_snapshot(&journey());
        let mut local = journey();
        if edited {
            local["nodes"]["user"]["validateInput"] = json!(true);
        }
        project.write_export(&local);
        let before = project.files();
        let mut remote = journey();
        remote["nodes"]["user"]["validateInput"] = json!("remote");

        let (output, tenant) = project.journey(Tenant::new(remote), &["pull", "Login"]);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            tenant
                .calls
                .iter()
                .any(|(_, path)| path.ends_with("/UsernameCollectorNode/user")),
            "the child was read"
        );
        if edited {
            assert_eq!(output.status.code(), Some(1), "{stderr}");
            assert!(stderr.contains("requires confirmation"), "{stderr}");
            assert_eq!(project.files(), before);
        } else {
            assert!(output.status.success(), "{stderr}");
            let export: Value = serde_json::from_slice(&project.files().1).unwrap();
            assert_eq!(export["nodes"]["user"]["validateInput"], "remote");
        }
    }
}
