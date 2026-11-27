//! Run the real `aic journey push` against an in-process agent fixture, for
//! a journey whose page holds child nodes the tree does not list. The socket
//! belongs to a temporary project and never connects to a tenant.

use serde_json::{Map, Value, json};
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

    /// The snapshot's and the export's bytes. Comparing them proves the
    /// contents are unchanged byte for byte — a reformatted or reordered
    /// rewrite fails — but not that the files were never rewritten: writing
    /// back the identical bytes passes.
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
    /// A PUT to this node id (or `Login`, the tree) answers with this agent
    /// reply instead — after applying the write when the flag is set, as a
    /// reply lost after the tenant acted would.
    fail_put: Option<(&'static str, Value, bool)>,
    /// Other journeys' trees, by name, for the realm listing.
    others: Map<String, Value>,
    /// A node id, or a journey name, whose GET answers with this agent reply
    /// instead.
    fail_get: Option<(&'static str, Value)>,
    /// An agent reply that replaces the realm's journey listing.
    listing: Option<Value>,
    /// The caller-chosen transaction ids the requests carried, in order,
    /// each with its REST path below the trees prefix.
    sent_ids: Vec<(String, String)>,
    /// After this many PUTs, every agent ping answers with a protocol
    /// mismatch, so the next request is never dispatched.
    ping_fails_after_puts: Option<usize>,
    calls: Vec<(String, String)>,
    /// Node configs by REST path below the trees prefix, answered ahead of
    /// the by-id state: two node types can share one id.
    by_path: Map<String, Value>,
}

impl Tenant {
    fn new(state: Value) -> Self {
        Self {
            state,
            fail_put: None,
            others: Map::new(),
            fail_get: None,
            listing: None,
            sent_ids: Vec::new(),
            ping_fails_after_puts: None,
            calls: Vec::new(),
            by_path: Map::new(),
        }
    }

    fn api(&mut self, request: &Value) -> Value {
        let method = request["method"].as_str().unwrap();
        let path = request["path"].as_str().unwrap();
        self.calls.push((method.into(), path.into()));
        if let Some(id) = request["transaction_id"].as_str() {
            let rest = path.strip_prefix(TREES).unwrap_or(path);
            self.sent_ids.push((rest.into(), id.into()));
        }
        let rest = path.strip_prefix(TREES).unwrap_or_default();
        let found = |value: Option<&Value>| match value {
            Some(value) => json!({"type": "json", "value": value}),
            None => json!({"type": "api_error", "status": 404, "body": "fixture: no such object"}),
        };
        match (method, rest.split('/').collect::<Vec<_>>().as_slice()) {
            ("GET", ["", "trees?_queryFilter=true&_pageSize=1000"]) if self.listing.is_some() => {
                self.listing.clone().unwrap()
            }
            ("GET", ["", "trees", id] | ["", "nodes", _, id])
                if self
                    .fail_get
                    .as_ref()
                    .is_some_and(|(failing, _)| failing == id) =>
            {
                self.fail_get.clone().unwrap().1
            }
            ("GET", ["", "trees?_queryFilter=true&_pageSize=1000"]) => {
                let names = std::iter::once("Login").chain(self.others.keys().map(String::as_str));
                let result: Vec<Value> = names.map(|name| json!({"_id": name})).collect();
                found(Some(&json!({ "result": result })))
            }
            ("GET", ["", "nodes", ..]) if self.by_path.contains_key(rest) => {
                found(self.by_path.get(rest))
            }
            ("GET", ["", "trees", "Login"]) => found(Some(&self.state["tree"])),
            ("GET", ["", "trees", name]) => found(self.others.get(*name)),
            ("GET", ["", "nodes", _, id]) => found(self.state["nodes"].get(*id)),
            ("PUT", ["", kind @ ("nodes" | "trees"), .., id]) => {
                let failure = self.fail_put.clone().filter(|(failing, ..)| failing == id);
                if let Some((_, reply, false)) = failure {
                    return reply;
                }
                let mut body = request["body"].clone();
                body["_id"] = json!(id);
                if *kind == "trees" {
                    self.state["tree"] = body.clone();
                } else {
                    self.state["nodes"][*id] = body.clone();
                }
                match failure {
                    Some((_, reply, _)) => reply,
                    None => found(Some(&body)),
                }
            }
            _ => panic!("unexpected request {method} {path}"),
        }
    }

    fn ping_down(&self) -> bool {
        let puts = self
            .calls
            .iter()
            .filter(|(method, _)| method == "PUT")
            .count();
        self.ping_fails_after_puts
            .is_some_and(|after| puts >= after)
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
    // A second run in one project rebinds the socket the first left behind.
    let socket = project.0.join(".aic/agent.sock");
    let _ = std::fs::remove_file(&socket);
    let listener = UnixListener::bind(socket).unwrap();
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
            "ping" if tenant.ping_down() => {
                json!({"type": "protocol_mismatch", "expected": 999, "received": 6})
            }
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
    tenant.fail_put = Some((
        "script",
        json!({"type": "api_error", "status": 503, "body": "fixture: unavailable"}),
        false,
    ));

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

/// Regression (review round 3 P2-2): the agent's `Locked` reply comes before
/// it dispatches anything, yet a push stopped by one said the write may have
/// landed. Over the wire, the first child's PUT answers `{"type":"locked"}`;
/// the control is a lost reply (`transport`) at the same write, which is
/// unknown.
#[test]
fn journey_push_stopped_by_a_locked_agent_says_nothing_landed() {
    for (reply, outcome) in [
        (json!({"type": "locked"}), "was refused and not applied"),
        (
            json!({"type": "transport", "message": "fixture: reset"}),
            "may have landed",
        ),
    ] {
        let project = Project::new();
        project.write_snapshot(&journey());
        let mut local = journey();
        local["nodes"]["user"]["validateInput"] = json!(true);
        project.write_export(&local);
        let mut tenant = Tenant::new(journey());
        tenant.fail_put = Some(("user", reply, false));

        let (output, _) = project.push(tenant, &[]);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{stderr}");
        assert!(
            stderr.contains(&format!("UsernameCollectorNode/user {outcome}")),
            "{outcome}: {stderr}"
        );
        assert!(stderr.contains("not attempted (4)"), "{stderr}");
    }
}

/// Regression (review round 3 P1-3, round 4 P2-1): the tree `PUT` is
/// applied and its reply lost. The push attaches a new child `fresh` to the
/// page and changes the tree itself, so the tree landing is observable: the
/// tenant must hold the new tree body, the error must call the tree's
/// outcome unknown rather than failed, and the next `status` must show both
/// `fresh` and the tree as changed on the tenant. With the fixture's
/// apply-before-reply flag off, the tree assertion fails.
#[test]
fn journey_push_with_a_lost_tree_reply_reports_every_node_acknowledged() {
    let project = Project::new();
    project.write_snapshot(&journey());
    let mut local = journey();
    local["tree"]["description"] = json!("v2");
    local["nodes"]["page"]["nodes"]
        .as_array_mut()
        .unwrap()
        .push(json!({"_id": "fresh", "nodeType": "MessageNode"}));
    local["nodes"]["fresh"] = json!({"_id": "fresh", "message": {"en": "hi"}});
    project.write_export(&local);
    let before = project.files();
    let mut tenant = Tenant::new(journey());
    tenant.fail_put = Some((
        "Login",
        json!({"type": "transport", "message": "fixture: reset"}),
        true,
    ));

    let (output, mut tenant) = project.push(tenant, &[]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    for expected in [
        "tree may have landed",
        "acknowledged by the tenant (5)",
        "reads what the tenant's current tree and pages reference; run it",
    ] {
        assert!(stderr.contains(expected), "{expected}: {stderr}");
    }
    for wrong in ["will not show", "does not show"] {
        assert!(!stderr.contains(wrong), "{wrong}: {stderr}");
    }
    assert_eq!(tenant.state["tree"]["description"], "v2", "the tree landed");
    // Review round 8 P2: the lookup names the id the tree PUT carried.
    let (path, tree_id) = tenant.sent_ids.last().unwrap();
    assert!(path.ends_with("/trees/Login"), "{path}");
    assert!(
        stderr.contains(&format!("`aic logs tx {tree_id} --tenant=stub`")),
        "{stderr}"
    );
    assert_eq!(project.files(), before);

    tenant.fail_put = None;
    let (output, _) = project.journey(tenant, &["status", "Login", "--json"]);
    let stdout: Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("{error}: {}", String::from_utf8_lossy(&output.stderr)));
    let remote = &stdout["journeys"][0]["remote_changes"];
    let parts: Vec<&Value> = remote.as_array().unwrap().iter().collect();
    assert!(parts.iter().any(|part| part["id"] == "fresh"), "{remote}");
    assert!(parts.iter().any(|part| part["part"] == "tree"), "{remote}");
}

/// Regression (review round 3 P2-3, round 2 P1-4) at the CLI: Login's
/// snapshot lists `user` under its private page `other`; on the tenant Login
/// now lists it under `page`, which Register also uses, and `user` changed.
/// `status --json` must place the child by the tenant's copy and so mark it
/// shared with Register. The discriminating input is the move: placing it by
/// the snapshot gives `other` and no sharing.
#[test]
fn journey_status_places_a_moved_child_by_the_tenant_and_marks_it_shared() {
    let project = Project::new();
    let mut snapshot = journey();
    let page = snapshot["nodes"]["page"].take();
    snapshot["nodes"].as_object_mut().unwrap().remove("page");
    snapshot["nodes"]["other"] = page;
    snapshot["nodes"]["other"]["_id"] = json!("other");
    let entry = snapshot["tree"]["nodes"]
        .as_object_mut()
        .unwrap()
        .remove("page")
        .unwrap();
    snapshot["tree"]["nodes"]["other"] = entry;
    project.write_snapshot(&snapshot);
    project.write_export(&snapshot);
    let mut remote = journey();
    remote["nodes"]["user"]["validateInput"] = json!(true);
    let mut tenant = Tenant::new(remote);
    tenant.others.insert(
        "Register".into(),
        json!({"_id": "Register", "nodes": {"page": {"nodeType": "PageNode"}}}),
    );

    let (output, _) = project.journey(tenant, &["status", "Login", "--json"]);
    let stdout: Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("{error}: {}", String::from_utf8_lossy(&output.stderr)));
    let user = stdout["journeys"][0]["remote_changes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|part| part["id"] == "user")
        .unwrap_or_else(|| panic!("{stdout}"))
        .clone();
    assert_eq!(user["page"], "page", "{user}");
    assert_eq!(user["shared_with"], json!(["Register"]), "{user}");
}

/// Regression (review round 3 P1-2, P2-3) at the CLI: a page child with no
/// `nodeType` hid its valid siblings, so `diff` named the bad entry and not
/// `user`, whose body the export lacks. Both notes must reach stderr.
#[test]
fn journey_diff_notes_a_malformed_child_beside_a_missing_sibling() {
    let project = Project::new();
    project.write_snapshot(&journey());
    let mut local = journey();
    local["nodes"].as_object_mut().unwrap().remove("user");
    local["nodes"]["page"]["nodes"]
        .as_array_mut()
        .unwrap()
        .push(json!({"_id": "bad"}));
    project.write_export(&local);

    let (output, _) = project.journey(
        Tenant::new(journey()),
        &["diff", "Login", "--local-vs-snapshot"],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("the local lists node(s) it does not carry: user —"),
        "{stderr}"
    );
    assert!(
        stderr.contains("references are not all readable") && stderr.contains(r#""bad""#),
        "{stderr}"
    );
}

/// Regression (review round 4 P1-1): a malformed page child was quoted whole
/// in diagnostics, which reach stderr, status notes and pull errors without
/// `diff`'s secret masking. The discriminating input is a synthetic secret
/// inside a malformed child, locally (diff, status) and on the tenant (pull,
/// status); it must appear on neither stream, while the diagnostic still
/// names the page and the entry's fields.
#[test]
fn journey_diagnostics_never_quote_a_malformed_entry() {
    const SECRET: &str = "SYNTHETIC-SECRET-7f3a";
    let with_bad_child = || {
        let mut copy = journey();
        copy["nodes"]["page"]["nodes"]
            .as_array_mut()
            .unwrap()
            .extend([
                json!({"_id": "bad"}),
                json!({"_id": "bad2", "secretKey": SECRET}),
            ]);
        copy
    };
    let cases: [(&str, &[&str], bool, bool); 4] = [
        (
            "diff",
            &["diff", "Login", "--local-vs-snapshot", "--json"],
            true,
            false,
        ),
        ("status", &["status", "Login", "--json"], true, true),
        ("pull", &["pull", "Login", "--force"], false, true),
        (
            "diff remote",
            &["diff", "Login", "--snapshot-vs-remote"],
            false,
            true,
        ),
    ];
    for (case, args, local_bad, remote_bad) in cases {
        let project = Project::new();
        project.write_snapshot(&journey());
        project.write_export(&if local_bad {
            with_bad_child()
        } else {
            journey()
        });
        let remote = if remote_bad {
            with_bad_child()
        } else {
            journey()
        };
        let (output, _) = project.journey(Tenant::new(remote), args);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!stdout.contains(SECRET), "{case} stdout: {stdout}");
        assert!(!stderr.contains(SECRET), "{case} stderr: {stderr}");
        let both = format!("{stdout}{stderr}");
        assert!(
            both.contains("bad2") && both.contains("`secretKey`"),
            "{case}: the diagnostic still names the entry and its fields: {both}"
        );
    }
}

/// Regression (review round 4 P1-4): status described a local retype by the
/// tenant's old endpoint. Locally the page's `user` is relisted as a
/// `PasswordCollectorNode` and edited; the local change must carry that
/// type, the endpoint a push writes, while the page still comes from the
/// tenant's copy.
#[test]
fn journey_status_names_a_local_retype_by_the_local_type() {
    let project = Project::new();
    project.write_snapshot(&journey());
    let mut local = journey();
    local["nodes"]["page"]["nodes"][0]["nodeType"] = json!("PasswordCollectorNode");
    local["nodes"]["user"] = json!({"_id": "user", "validateInput": true});
    project.write_export(&local);

    let (output, _) = project.journey(Tenant::new(journey()), &["status", "Login", "--json"]);
    let stdout: Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("{error}: {}", String::from_utf8_lossy(&output.stderr)));
    let user = stdout["journeys"][0]["local_changes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|part| part["id"] == "user")
        .unwrap_or_else(|| panic!("{stdout}"))
        .clone();
    assert_eq!(user["node_type"], "PasswordCollectorNode", "{user}");
    assert_eq!(user["page"], "page", "{user}");
}

/// Regression (review round 5 P1-1): an AIC error body can echo the config of
/// the node it concerns, and journey printed it whole — in `status`
/// problems, on `diff` stderr and in the partial-push report — past `diff`'s
/// masking. The discriminating input is a synthetic secret inside the error
/// body of a child's GET (status, diff) and of a child's PUT (push). Each run
/// must still name the status and the endpoint.
#[test]
fn journey_diagnostics_never_print_an_error_body() {
    const SECRET: &str = "SYNTHETIC-ERROR-SECRET-91c2";
    let error = json!({
        "type": "api_error",
        "status": 500,
        "body": format!(r#"{{"detail":{{"config":{{"secretKey":"{SECRET}"}}}}}}"#),
    });
    let cases: [(&str, &[&str], bool); 3] = [
        ("status", &["status", "Login", "--json"], false),
        (
            "diff",
            &["diff", "Login", "--snapshot-vs-remote", "--json"],
            false,
        ),
        ("push", &["push", "Login", "--yes"], true),
    ];
    for (case, args, put) in cases {
        let project = Project::new();
        project.write_snapshot(&journey());
        let mut local = journey();
        local["nodes"]["user"]["validateInput"] = json!(true);
        project.write_export(&local);
        let mut tenant = Tenant::new(journey());
        if put {
            tenant.fail_put = Some(("user", error.clone(), false));
        } else {
            tenant.fail_get = Some(("user", error.clone()));
        }
        let (output, _) = project.journey(tenant, args);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!stdout.contains(SECRET), "{case} stdout: {stdout}");
        assert!(!stderr.contains(SECRET), "{case} stderr: {stderr}");
        let both = format!("{stdout}{stderr}");
        assert!(
            both.contains("UsernameCollectorNode/user") && both.contains("500"),
            "{case}: status and endpoint are still named: {both}"
        );
    }
}

/// Regression (review round 5 P1-3): a retype with a byte-identical config
/// produced no node part, because a part was only made for a config that
/// differs — yet the push writes a different endpoint. Locally the page's
/// `user` is relisted as a `PasswordCollectorNode` with its config
/// untouched; `status` and `diff` must both list `user` under the new type.
#[test]
fn journey_status_and_diff_list_a_type_only_retype() {
    let project = Project::new();
    project.write_snapshot(&journey());
    let mut local = journey();
    local["nodes"]["page"]["nodes"][0]["nodeType"] = json!("PasswordCollectorNode");
    project.write_export(&local);
    let user_part = |parts: &Value| {
        parts
            .as_array()
            .unwrap()
            .iter()
            .find(|part| part["id"] == "user")
            .cloned()
            .unwrap_or_else(|| panic!("no user part: {parts}"))
    };

    let (output, tenant) = project.journey(Tenant::new(journey()), &["status", "Login", "--json"]);
    let stdout: Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("{error}: {}", String::from_utf8_lossy(&output.stderr)));
    let user = user_part(&stdout["journeys"][0]["local_changes"]);
    assert_eq!(user["node_type"], "PasswordCollectorNode", "{user}");

    let (output, _) = project.journey(tenant, &["diff", "Login", "--local-vs-snapshot", "--json"]);
    let stdout: Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("{error}: {}", String::from_utf8_lossy(&output.stderr)));
    let parts = stdout
        .get("parts")
        .or_else(|| stdout.get("changes"))
        .unwrap_or_else(|| panic!("{stdout}"));
    let user = user_part(parts);
    assert_eq!(user["node_type"], "PasswordCollectorNode", "{user}");
}

/// Regression (review round 5 P1-2): a journey listing whose `result` is not
/// an array was reported by quoting the whole body. The discriminating input
/// is a synthetic secret inside an object `result`; the diagnostic must name
/// the listing and the shape, not the value.
#[test]
fn a_malformed_journey_listing_is_described_by_shape() {
    const SECRET: &str = "SYNTHETIC-LISTING-SECRET-4be0";
    let project = Project::new();
    project.write_snapshot(&journey());
    project.write_export(&journey());
    // A tenant change to a page child, so status asks which other journeys
    // share it — the one read that needs the listing.
    let mut remote = journey();
    remote["nodes"]["user"]["validateInput"] = json!(true);
    let mut tenant = Tenant::new(remote);
    tenant.listing = Some(json!({"type": "json", "value": {"result": {"secretKey": SECRET}}}));

    let (output, _) = project.journey(tenant, &["status", "Login", "--json"]);
    let both = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!both.contains(SECRET), "{both}");
    assert!(both.contains("unexpected journey list shape"), "{both}");
    assert!(
        both.contains("secretKey"),
        "the shape names the field: {both}"
    );
}

/// A body ending in the sentence the transport appends to a non-JSON 5xx
/// (`aic::trace_gateway_body`), naming ids `aic` never sent. Real or forged,
/// nothing in it may reach output.
fn gateway_body(marker: &str) -> String {
    format!(
        "<html>{marker}</html>\n\na non-JSON 503 Service Unavailable answer (server: gw), the \
         shape a gateway gives rather than AM or IDM. `aic logs tx tx-first`, `aic logs tx \
         tx-retry` shows whether AM or IDM recorded the request."
    )
}

fn streams(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// Regression (review round 6 P1-1): a change to a page child alone makes
/// `status` read the realm listing to mark sharing, and a failed listing
/// printed its raw body. The discriminating input is a 500 body carrying a
/// marker; the diagnostic must name the status and the listing instead.
#[test]
fn a_failed_listing_read_for_sharing_withholds_its_body() {
    const MARKER: &str = "SYNTHETIC-LISTING-BODY-7d1e";
    let project = Project::new();
    project.write_snapshot(&journey());
    project.write_export(&journey());
    let mut remote = journey();
    remote["nodes"]["user"]["validateInput"] = json!(true);
    let mut tenant = Tenant::new(remote);
    tenant.listing = Some(json!({
        "type": "api_error",
        "status": 500,
        "body": format!(r#"{{"detail":{{"tree":{{"description":"{MARKER}"}}}}}}"#),
    }));

    let (output, _) = project.journey(tenant, &["status", "Login", "--json"]);
    let both = streams(&output);
    assert!(!both.contains(MARKER), "{both}");
    assert!(
        both.contains("500") && both.contains("journey list"),
        "{both}"
    );
}

/// Regression (review round 6 P2-1, round 7 P1-1): a failed write's
/// `aic logs tx` lookup is how the operator learns whether a write that "may
/// have landed" did — but it was read back out of the error body, where a
/// server's text can forge it. The discriminating inputs are a decorated
/// gateway 503 and a plain 400 ending in the same sentence, each on a
/// child's PUT (push) and a child's GET (status). No id from the body may
/// appear; a push must name the id the agent was actually sent.
#[test]
fn a_write_names_the_id_it_sent_and_no_lookup_comes_from_a_body() {
    const MARKER: &str = "SYNTHETIC-GATEWAY-BODY-30af";
    for status in [503, 400] {
        let error = json!({"type": "api_error", "status": status, "body": gateway_body(MARKER)});
        for (case, args, put) in [
            ("push", &["push", "Login", "--yes"][..], true),
            ("status", &["status", "Login", "--json"][..], false),
        ] {
            let project = Project::new();
            project.write_snapshot(&journey());
            let mut local = journey();
            local["nodes"]["user"]["validateInput"] = json!(true);
            project.write_export(&local);
            let mut tenant = Tenant::new(journey());
            if put {
                tenant.fail_put = Some(("user", error.clone(), false));
            } else {
                tenant.fail_get = Some(("user", error.clone()));
            }
            let (output, tenant) = project.journey(tenant, args);
            let both = streams(&output);
            let case = format!("{status} {case}");
            assert!(!both.contains(MARKER), "{case}: {both}");
            assert!(
                !both.contains("tx-first") && !both.contains("tx-retry"),
                "{case}: {both}"
            );
            assert!(
                both.contains("UsernameCollectorNode/user"),
                "{case}: {both}"
            );
            if put {
                let (_, sent) = tenant
                    .sent_ids
                    .last()
                    .unwrap_or_else(|| panic!("{case}: the PUT carried no id"));
                assert!(
                    both.contains(&format!("`aic logs tx {sent} --tenant=stub`")),
                    "{case}: {both}"
                );
            }
        }
    }
}

/// Regression (review round 6 P2-2): a failed tree read in pull, push's
/// preflight and a remote diff reached the operator before anything named
/// the journey, so the diagnostic said "500, body withheld" and nothing
/// else. Each must name the journey and its tree endpoint.
#[test]
fn a_failed_tree_read_names_the_journey_and_its_endpoint() {
    const MARKER: &str = "SYNTHETIC-TREE-BODY-5c29";
    for args in [
        &["pull", "Login", "--force"][..],
        &["push", "Login", "--yes"][..],
        &["diff", "Login", "--snapshot-vs-remote"][..],
    ] {
        let project = Project::new();
        project.write_snapshot(&journey());
        let mut local = journey();
        local["nodes"]["user"]["validateInput"] = json!(true);
        project.write_export(&local);
        let mut tenant = Tenant::new(journey());
        tenant.fail_get = Some((
            "Login",
            json!({"type": "api_error", "status": 500, "body": MARKER}),
        ));
        let (output, _) = project.journey(tenant, args);
        let both = streams(&output);
        assert!(!output.status.success(), "{args:?}: {both}");
        assert!(!both.contains(MARKER), "{args:?}: {both}");
        assert!(
            both.contains("journey `Login`'s tree (`trees/Login`)"),
            "{args:?}: {both}"
        );
    }
}

/// Regression (review round 6 P2-3): a failed read during `using-script`
/// was a warning, so `--json` printed `[]` and exited 0 — the same answer as
/// a complete search that found nothing. The discriminating input is a 503
/// on the only node using the script (a page child) and, separately, on a
/// journey's tree (the pre-branch case); the control reads everything.
#[test]
fn an_incomplete_script_search_says_so_and_fails() {
    const SCRIPT: &str = "00000000-0000-0000-0000-000000000000";
    let args = ["using-script", SCRIPT, "--json"];
    let project = Project::new();

    let (output, _) = project.journey(Tenant::new(journey()), &args);
    assert!(output.status.success(), "{}", streams(&output));
    let stdout: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(stdout, json!(["Login"]), "the control: a complete search");

    for (failing, unread) in [
        ("script", "Login: ScriptedDecisionNode/script"),
        ("Login", "Login: its tree"),
    ] {
        let mut tenant = Tenant::new(journey());
        tenant.fail_get = Some((
            failing,
            json!({"type": "api_error", "status": 503, "body": "gateway"}),
        ));
        let (output, _) = project.journey(tenant, &args);
        assert!(!output.status.success(), "{failing}: {}", streams(&output));
        let stdout: Value = serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|error| panic!("{error}: {}", streams(&output)));
        assert_eq!(
            stdout,
            json!({"journeys": [], "incomplete": true, "unread": [unread]}),
            "{failing}"
        );
    }

    // A match survives another journey's failed read; the run still fails.
    let mut tenant = Tenant::new(journey());
    tenant
        .others
        .insert("Register".into(), json!({"_id": "Register", "nodes": {}}));
    tenant.fail_get = Some((
        "Register",
        json!({"type": "api_error", "status": 503, "body": "gateway"}),
    ));
    let (output, _) = project.journey(tenant, &args);
    assert!(!output.status.success(), "{}", streams(&output));
    let stdout: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        stdout,
        json!({"journeys": ["Login"], "incomplete": true, "unread": ["Register: its tree"]})
    );
}

/// Regression (review round 7 P1-2, P2-1, P2-2): every way a successful
/// reply can still leave a journey's answer unknown — and the one where a
/// match settles it anyway. Login's only script reference is the page child
/// `script`. Each case reshapes one reply; an unknown answer must exit
/// non-zero with the incomplete object naming what was unread, and a settled
/// one must stay the plain array with exit 0.
#[test]
fn every_unusable_successful_reply_makes_the_script_search_incomplete() {
    const SCRIPT: &str = "00000000-0000-0000-0000-000000000000";
    type Reshape = fn(&mut Tenant);
    let cases: [(&str, Reshape, Value); 7] = [
        (
            "a page with no `nodes`",
            |t| t.state["nodes"]["page"] = json!({"_id": "page"}),
            json!(["Login: the children of PageNode/page"]),
        ),
        (
            "the scripted child with no `nodeType`",
            |t| {
                t.state["nodes"]["page"]["nodes"][1]
                    .as_object_mut()
                    .unwrap()
                    .remove("nodeType");
            },
            json!(["Login: a child of PageNode/page"]),
        ),
        (
            "a candidate config that is JSON null",
            |t| t.state["nodes"]["script"] = Value::Null,
            json!(["Login: ScriptedDecisionNode/script"]),
        ),
        (
            "a candidate config that is not an object",
            |t| t.state["nodes"]["script"] = json!(["not", "a", "config"]),
            json!(["Login: ScriptedDecisionNode/script"]),
        ),
        (
            "a tree whose `nodes` is an array",
            |t| t.state["tree"]["nodes"] = json!([]),
            json!(["Login: its tree (`nodes` is an array of 0)"]),
        ),
        (
            "a tree entry with no `nodeType`",
            |t| {
                t.state["tree"]["nodes"]["page"]
                    .as_object_mut()
                    .unwrap()
                    .remove("nodeType");
            },
            json!(["Login: tree entry page (`nodeType` is absent)"]),
        ),
        (
            "a listing row with no `_id`",
            |t| {
                t.listing = Some(
                    json!({"type": "json", "value": {"result": [{"_id": "Login"}, {"name": "x"}]}}),
                )
            },
            json!(["journey list row 1: `_id` is absent"]),
        ),
    ];
    let project = Project::new();
    for (case, reshape, unread) in cases {
        let mut tenant = Tenant::new(journey());
        reshape(&mut tenant);
        let (output, _) = project.journey(tenant, &["using-script", SCRIPT, "--json"]);
        assert!(!output.status.success(), "{case}: {}", streams(&output));
        let stdout: Value = serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|error| panic!("{case}: {error}: {}", streams(&output)));
        let journeys = if case.contains("listing") {
            json!(["Login"])
        } else {
            json!([])
        };
        assert_eq!(
            stdout,
            json!({"journeys": journeys, "incomplete": true, "unread": unread}),
            "{case}"
        );
    }

    // Settled: an unaddressable entry beside the matching sibling.
    let mut tenant = Tenant::new(journey());
    tenant.state["nodes"]["page"]["nodes"][0]
        .as_object_mut()
        .unwrap()
        .remove("nodeType");
    let (output, _) = project.journey(tenant, &["using-script", SCRIPT, "--json"]);
    assert!(output.status.success(), "{}", streams(&output));
    let stdout: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(stdout, json!(["Login"]));
}

/// Regression (review round 8 P1-1): the stopped write's id was described
/// as "sent" whatever happened, but the push assigns it before the call, and
/// the call can fail before dispatch — here the agent stops answering pings
/// after the first child's PUT, so the second is never sent. The output must
/// call the id assigned, make the lookup conditional, and the id it names
/// must be one no request carried.
#[test]
fn journey_push_never_calls_an_undispatched_write_sent() {
    let project = Project::new();
    project.write_snapshot(&journey());
    let mut local = journey();
    local["nodes"]["user"]["validateInput"] = json!(true);
    local["nodes"]["script"]["script"] = json!("11111111-1111-1111-1111-111111111111");
    project.write_export(&local);
    let mut tenant = Tenant::new(journey());
    tenant.ping_fails_after_puts = Some(1);

    let (output, tenant) = project.push(tenant, &[]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert_eq!(
        tenant.sent_ids.len(),
        1,
        "only the first PUT went out: {stderr}"
    );
    assert!(!stderr.contains("was sent with"), "{stderr}");
    let marker = "assigned transaction id ";
    let at = stderr.find(marker).unwrap_or_else(|| panic!("{stderr}")) + marker.len();
    let assigned: String = stderr[at..].chars().take_while(|c| *c != ';').collect();
    assert_ne!(assigned, tenant.sent_ids[0].1, "{stderr}");
    assert!(
        stderr.contains(&format!(
            "if the request reached the tenant, `aic logs tx {assigned} --tenant=stub`"
        )),
        "{stderr}"
    );
}

/// The printed `aic logs tx` lookup, split into words by a real shell — what
/// the operator gets by pasting it. Comparing words rather than text keeps a
/// correctly quoted path (a `TMPDIR` with a space) from failing the test, and
/// still fails on a quote that is wrong.
fn lookup_words(stderr: &str) -> Vec<String> {
    let start = stderr
        .find("`aic logs tx ")
        .unwrap_or_else(|| panic!("no lookup: {stderr}"));
    let rest = &stderr[start + "`aic ".len()..];
    let command = &rest[..rest.find('`').unwrap()];
    let output = Command::new("sh")
        .arg("-c")
        .arg(format!("printf '%s\\n' {command}"))
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect()
}

/// A failed child PUT, pushed to `alias` while `stub` stays the default.
fn push_failing_to(alias: &str) -> (String, String) {
    let project = Project::new();
    std::fs::write(
        project.0.join(".aic/config.toml"),
        format!(
            "project = \"test\"\ndefault_tenant = \"stub\"\n\n\
             [[tenant]]\nname = \"stub\"\nbase_url = \"https://tenant.example.com\"\ntheme = \"production\"\nscopes = []\n\n\
             [[tenant]]\nname = \"{alias}\"\nbase_url = \"https://tenant-b.example.com\"\ntheme = \"production\"\nscopes = []\n"
        ),
    )
    .unwrap();
    project.write_snapshot(&journey());
    let mut local = journey();
    local["nodes"]["user"]["validateInput"] = json!(true);
    project.write_export(&local);
    // The workspace is per tenant: move the files to the alias's.
    std::fs::rename(
        project.0.join("workspace/stub"),
        project.0.join("workspace").join(alias),
    )
    .unwrap();
    let mut tenant = Tenant::new(journey());
    tenant.fail_put = Some((
        "user",
        json!({"type": "transport", "message": "fixture: reset"}),
        true,
    ));
    let (output, tenant) = project.push(tenant, &[&format!("--tenant={alias}")]);
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    let (_, sent) = tenant
        .sent_ids
        .last()
        .unwrap_or_else(|| panic!("no PUT: {stderr}"));
    (sent.clone(), stderr)
}

/// Regression (review round 8 P1-2): the printed lookup left the tenant to
/// the logs command's own resolution, so with another project default it
/// searched a tenant the push never wrote. The discriminating input is a
/// push with `--tenant other` while `stub` is the project default — the
/// fixture's usual tenant, so a lookup naming the default, or any fixed
/// tenant, is caught.
#[test]
fn journey_push_lookup_names_the_tenant_it_wrote_to() {
    let (sent, stderr) = push_failing_to("other");
    assert_eq!(
        lookup_words(&stderr),
        ["logs", "tx", &sent, "--tenant=other"],
        "{stderr}"
    );
}

/// Regression (review round 10 P1-2): `--tenant -other` is two flags to
/// clap however it is quoted, so the lookup for an alias with a leading
/// hyphen could not be run. The value must be attached to its option.
#[test]
fn journey_push_lookup_attaches_a_hyphen_led_tenant() {
    let (sent, stderr) = push_failing_to("-other");
    assert_eq!(
        lookup_words(&stderr),
        ["logs", "tx", &sent, "--tenant=-other"],
        "{stderr}"
    );
}

/// A failed child PUT pushed from a directory outside the project, which
/// `choose` points the push at (by flag or environment).
fn push_from_elsewhere(choose: impl FnOnce(&Project, &mut Command)) -> (String, String, PathBuf) {
    let project = Project::new();
    project.write_snapshot(&journey());
    let mut local = journey();
    local["nodes"]["user"]["validateInput"] = json!(true);
    project.write_export(&local);
    let elsewhere = std::env::temp_dir().join(format!("aic-elsewhere-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&elsewhere).unwrap();
    let mut tenant = Tenant::new(journey());
    tenant.fail_put = Some((
        "user",
        json!({"type": "transport", "message": "fixture: reset"}),
        true,
    ));
    let mut command = Command::new(env!("CARGO_BIN_EXE_aic"));
    command
        .current_dir(&elsewhere)
        .env_remove("AIC_PROJECT")
        .env("XDG_DATA_HOME", project.0.join("data"))
        .env("XDG_STATE_HOME", project.0.join("state"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    choose(&project, &mut command);
    command.args(["--no-prompt", "journey", "push", "Login", "--yes"]);
    let (output, tenant) = run(&project, command, tenant);
    let _ = std::fs::remove_dir_all(&elsewhere);
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    let (_, sent) = tenant.sent_ids.last().unwrap_or_else(|| panic!("{stderr}"));
    // The CLI canonicalises the root it prints; the fixture's spelling may go
    // through a symlinked TMPDIR, so compare the canonical path.
    (sent.clone(), stderr, project.0.canonicalize().unwrap())
}

/// Regression (review round 9 P1-2): a push run with `--project` from
/// another directory printed a lookup without it, so pasting the lookup
/// where the operator stands read a different project's config and log keys.
/// The discriminating input is that invocation; the lookup must carry the
/// project's absolute root. The same push from inside the project (every
/// other test) prints no `--project`.
#[test]
fn journey_push_lookup_carries_the_project_it_was_given() {
    let (sent, stderr, root) = push_from_elsewhere(|project, command| {
        command.arg("--project").arg(&project.0);
    });
    let project = format!("--project={}", root.display());
    assert_eq!(
        lookup_words(&stderr),
        ["logs", "tx", &sent, "--tenant=stub", &project],
        "{stderr}"
    );
}

/// Regression (review round 10 P1-1): `AIC_PROJECT=<dir> aic journey push`
/// sets the variable for the push alone, so a lookup without `--project`
/// pasted into the same shell reads the project of the operator's directory.
/// An environment-chosen project must be carried exactly as the flag is.
#[test]
fn journey_push_lookup_carries_a_project_chosen_by_the_environment() {
    let (sent, stderr, root) = push_from_elsewhere(|project, command| {
        command.env("AIC_PROJECT", &project.0);
    });
    let project = format!("--project={}", root.display());
    assert_eq!(
        lookup_words(&stderr),
        ["logs", "tx", &sent, "--tenant=stub", &project],
        "{stderr}"
    );
}

/// Regression (Astra pre-release P1-1): a child GET that succeeds with an
/// empty body decodes to `null`, and `pull` wrote `"script": null` into the
/// export and the snapshot and exited 0. A config that is not a JSON object
/// is unreadable: the pull fails and neither file is touched. The control is
/// the same pull with the child readable, which succeeds.
#[test]
fn journey_pull_refuses_a_child_config_that_is_not_an_object() {
    for config in [json!(null), json!("text"), json!([])] {
        let project = Project::new();
        project.write_snapshot(&journey());
        project.write_export(&journey());
        let before = project.files();
        let mut tenant = Tenant::new(journey());
        tenant.fail_get = Some(("script", json!({"type": "json", "value": config})));
        let (output, _) = project.journey(tenant, &["pull", "Login"]);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{config}: {stderr}");
        assert!(
            stderr.contains("ScriptedDecisionNode/script"),
            "{config}: {stderr}"
        );
        assert_eq!(project.files(), before, "{config}");
    }
    let project = Project::new();
    project.write_snapshot(&journey());
    project.write_export(&journey());
    let (output, _) = project.journey(Tenant::new(journey()), &["pull", "Login"]);
    assert!(output.status.success(), "{}", streams(&output));
}

/// Regression (Astra pre-release P1-1, local half): a local export whose
/// node config is not an object was planned and PUT. It must be refused
/// before any write.
#[test]
fn journey_push_refuses_a_local_node_config_that_is_not_an_object() {
    let project = Project::new();
    project.write_snapshot(&journey());
    let mut local = journey();
    local["nodes"]["script"] = json!(null);
    project.write_export(&local);
    let (output, tenant) = project.push(Tenant::new(journey()), &[]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(
        stderr.contains("ScriptedDecisionNode/script (null) is not a JSON object"),
        "{stderr}"
    );
    assert_eq!(tenant.writes(), Vec::<String>::new(), "{stderr}");
}

/// Regression (Astra pre-release P1-2): a page PUT's measured refusal was
/// matched anywhere in the 400 body, so a 400 whose message was something
/// else, with "Node does not exist" in another field, after the write had
/// applied, was reported as refused and printed no lookup. Only the body's
/// `message` field carries the measured diagnostic. The control is the
/// measured refusal itself, which stays refused.
#[test]
fn journey_push_reads_a_page_refusal_only_from_its_message() {
    for (body, refused) in [
        (
            json!({"code": 400, "message": "Invalid upstream response", "detail": "Node does not exist: user"}),
            false,
        ),
        // The whole measured diagnostic, but outside `message`.
        (
            json!({"code": 400, "message": "Invalid upstream response", "detail": "Invalid config: Node does not exist: user"}),
            false,
        ),
        (
            json!({"code": 400, "reason": "Bad Request", "message": "Invalid config: Node does not exist: user"}),
            true,
        ),
    ] {
        let project = Project::new();
        project.write_snapshot(&journey());
        let mut local = journey();
        local["nodes"]["page"]["stage"] = json!("edited");
        project.write_export(&local);
        let mut tenant = Tenant::new(journey());
        tenant.fail_put = Some((
            "page",
            json!({"type": "api_error", "status": 400, "body": body.to_string()}),
            !refused,
        ));
        let (output, _) = project.push(tenant, &[]);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{stderr}");
        assert_eq!(
            stderr.contains("PageNode/page was refused and not applied"),
            refused,
            "{body}: {stderr}"
        );
        assert_eq!(
            stderr.contains("`aic logs tx "),
            !refused,
            "{body}: {stderr}"
        );
    }
}

/// Regression (Astra pre-release P1-3): the script search de-duplicated
/// reads by bare id, so a page child `ScriptedDecisionNode/shared` holding
/// the script was never read once the tree listed a `DeviceMatchNode` at the
/// same id, and `using-script` answered `[]` with exit 0. The two are
/// distinct endpoints and both must be read.
#[test]
fn journey_using_script_reads_two_types_sharing_an_id() {
    const SCRIPT: &str = "00000000-0000-0000-0000-000000000000";
    let mut state = journey();
    state["tree"]["nodes"]["shared"] =
        json!({"nodeType": "DeviceMatchNode", "displayName": "d", "connections": {}});
    state["nodes"]["page"]["nodes"][1]["_id"] = json!("shared");
    let mut tenant = Tenant::new(state);
    // One id, two endpoints: the fixture keys configs by id, so answer the
    // DeviceMatchNode with an empty config and the scripted node with the
    // script by path.
    tenant.by_path.insert(
        "/nodes/DeviceMatchNode/shared".into(),
        json!({"_id": "shared"}),
    );
    tenant.by_path.insert(
        "/nodes/ScriptedDecisionNode/shared".into(),
        json!({"_id": "shared", "script": SCRIPT}),
    );
    let project = Project::new();
    let (output, tenant) = project.journey(tenant, &["using-script", SCRIPT, "--json"]);
    assert!(output.status.success(), "{}", streams(&output));
    let stdout: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(stdout, json!(["Login"]), "{}", streams(&output));
    assert!(
        tenant
            .calls
            .iter()
            .any(|(_, path)| path.ends_with("/ScriptedDecisionNode/shared")),
        "{:?}",
        tenant.calls
    );
}

/// Regression (Astra round 2 P1-1): the preflight read of a node the push
/// newly attaches counted any unequal successful body as existing content,
/// so a `null` (an empty body) read as "reused" and `--force=reuse` then
/// overwrote the node and the tree. A non-object body is unread: the push
/// stops before any write, whatever the override.
#[test]
fn journey_push_stops_when_a_newly_attached_node_reads_as_no_config() {
    for config in [json!(null), json!("text")] {
        let mut export = journey();
        export["nodes"]["page"]["nodes"]
            .as_array_mut()
            .unwrap()
            .push(json!({"_id": "borrowed", "nodeType": "UsernameCollectorNode"}));
        export["nodes"]["borrowed"] = json!({"_id": "borrowed", "validateInput": false});
        let mut state = journey();
        state["nodes"]["borrowed"] = json!({"_id": "borrowed", "validateInput": true});
        let mut tenant = Tenant::new(state);
        tenant.fail_get = Some(("borrowed", json!({"type": "json", "value": config})));
        let project = Project::new();
        project.write_snapshot(&journey());
        project.write_export(&export);
        let (output, tenant) = project.push(tenant, &["--force=reuse"]);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{config}: {stderr}");
        assert!(
            stderr.contains("UsernameCollectorNode/borrowed"),
            "{config}: {stderr}"
        );
        assert!(
            tenant.writes().is_empty(),
            "{config}: {:?} {stderr}",
            tenant.writes()
        );
        assert_eq!(tenant.state["nodes"]["borrowed"]["validateInput"], true);
    }
}
