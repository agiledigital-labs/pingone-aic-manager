//! Run the real relationship commands against an in-process agent fixture.
//! The socket belongs to a temporary project and never connects to a tenant.

use pingone_aic_manager::undo::{DiskLog, EntryStatus, UndoLog};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;

const MANAGED: &str = "/openidm/config/managed";
const REPO: &str = "/openidm/config/repo.ds";
const SYNC: &str = "/openidm/config/sync";

struct Project(PathBuf);

impl Project {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("aic-rel-{}", uuid::Uuid::new_v4()));
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

    fn command(&self, no_prompt: bool) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_aic"));
        command
            .current_dir(&self.0)
            .env_remove("AIC_PROJECT")
            .env("XDG_DATA_HOME", self.0.join("data"))
            .env("XDG_STATE_HOME", self.0.join("state"))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if no_prompt {
            command.arg("--no-prompt");
        }
        command.args(["managed", "relationship"]);
        command
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn managed(mapped_pair_declared: bool) -> Value {
    let mut doc = json!({"_id":"managed","objects":[
        {"name":"widget","onCreate":{"type":"text/javascript","source":"original"},
         "schema":{"type":"object","properties":{}}},
        {"name":"holder","schema":{"type":"object","properties":{}}}
    ]});
    if mapped_pair_declared {
        doc["objects"][0]["schema"]["properties"]["owner"] = json!({
            "type":"relationship", "resourceCollection":[{"path":"managed/holder"}],
            "reverseRelationship":true, "reversePropertyName":"widgets"
        });
        doc["objects"][1]["schema"]["properties"]["widgets"] = json!({
            "type":"array", "items":{"type":"relationship","resourceCollection":[{"path":"managed/widget"}],
            "reverseRelationship":true, "reversePropertyName":"owner"}
        });
    }
    doc
}

fn json_reply(value: Value) -> Value {
    json!({"type":"json","value":value})
}

struct Tenant {
    managed: Value,
    repo: Value,
    sync: Value,
    calls: Vec<(String, String)>,
}

impl Tenant {
    fn new(pair: bool) -> Self {
        Self {
            managed: managed(pair),
            repo: json!({"resourceMapping":{"genericMapping":{"managed/*":{"properties":{}}}}}),
            sync: json!({"mappings":[]}),
            calls: Vec::new(),
        }
    }

    fn api(&mut self, request: &Value) -> Value {
        let method = request["method"].as_str().unwrap();
        let path = request["path"].as_str().unwrap();
        self.calls.push((method.into(), path.into()));
        match (method, path) {
            ("GET", MANAGED) => json_reply(self.managed.clone()),
            ("GET", REPO) => json_reply(self.repo.clone()),
            ("GET", SYNC) => json_reply(self.sync.clone()),
            ("PUT", MANAGED) => {
                self.managed = request["body"].clone();
                json_reply(self.managed.clone())
            }
            ("PUT", "/openidm/config/repo.ds?waitForCompletion=true") => {
                self.repo = request["body"].clone();
                json_reply(self.repo.clone())
            }
            ("GET", _) if path.contains("?_queryFilter=") => json_reply(json!({"result":[]})),
            _ => json!({"type":"api_error","status":404,"body":"fixture: no record"}),
        }
    }

    fn writes(&self) -> Vec<(&str, &str)> {
        self.calls
            .iter()
            .filter(|(method, _)| method != "GET")
            .map(|(method, path)| (method.as_str(), path.as_str()))
            .collect()
    }
}

fn run(
    project: &Project,
    args: &[&str],
    tenant: Tenant,
    respond: impl FnMut(&mut Tenant, &Value, u32) -> Option<Value>,
) -> (Output, Tenant) {
    let mut command = project.command(true);
    command.args(args);
    run_command(project, command, tenant, respond)
}

fn run_command(
    project: &Project,
    mut command: Command,
    mut tenant: Tenant,
    mut respond: impl FnMut(&mut Tenant, &Value, u32) -> Option<Value>,
) -> (Output, Tenant) {
    let listener = UnixListener::bind(project.0.join(".aic/agent.sock")).unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut child = command.spawn().unwrap();
    drop(command); // Close the parent's copies of any terminal slave handles.
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    let mut unanswered = Vec::new();
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
            "api_call" => {
                respond(&mut tenant, &request, child.id()).unwrap_or_else(|| tenant.api(&request))
            }
            other => panic!("unexpected agent operation: {other}"),
        };
        if response["fixture_hold"] == true {
            unanswered.push(stream);
        } else if let Some(wire) = response["fixture_raw"].as_str() {
            let _ = stream.write_all(wire.as_bytes());
        } else {
            let _ = writeln!(stream, "{response}");
        }
    }
    (child.wait_with_output().unwrap(), tenant)
}

fn set_args() -> [&'static str; 13] {
    [
        "set",
        "widget.owner",
        "--target",
        "holder",
        "--forward",
        "one",
        "--reverse",
        "many",
        "--reverse-key",
        "widgets",
        "--wait-activation=3",
        "--yes",
        "--json",
    ]
}

fn map_args(capture: &Path) -> [&str; 7] {
    [
        "map",
        "widget.owner",
        "--capture",
        capture.to_str().unwrap(),
        "--wait-activation=3",
        "--yes",
        "--json",
    ]
}

#[test]
fn hook_edit_after_consent_inventory_refuses_before_any_write() {
    for set in [true, false] {
        let project = Project::new();
        let capture = project.0.join("capture.json");
        let mut args = if set {
            set_args().to_vec()
        } else {
            map_args(&capture).to_vec()
        };
        args.push("--force");
        let mut changed = false;
        let (output, tenant) = run(&project, &args, Tenant::new(!set), |tenant, request, _| {
            if request["path"] == SYNC && !changed {
                // The planned managed read saw the original hook. The next
                // read after consent must see the operator's edit.
                changed = true;
                tenant.managed["objects"][0]["onCreate"]["source"] = json!("edited");
            }
            None
        });
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(changed, "{stderr}");
        assert_eq!(output.status.code(), Some(1), "{stderr}");
        assert!(stderr.contains("changed since consent"), "{stderr}");
        assert!(tenant.writes().is_empty(), "{:?} {stderr}", tenant.writes());
        assert_eq!(tenant.managed["objects"][0]["onCreate"]["source"], "edited");
        assert!(!project.0.join(".aic/undo.log").exists());
        assert!(!capture.exists());
    }
}

#[test]
fn observed_managed_drift_refuses_without_writing_or_recording_undo() {
    let project = Project::new();
    let mut args = set_args().to_vec();
    args.push("--force");
    let mut surveys = 0;
    let (output, tenant) = run(&project, &args, Tenant::new(false), |tenant, request, _| {
        if request["path"] == SYNC {
            surveys += 1;
            if surveys == 2 {
                tenant.managed["objects"][0]["onCreate"]["source"] = json!("edited");
            }
        }
        None
    });
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(
        stderr.contains("config/managed changed since the relationship was planned"),
        "{stderr}"
    );
    assert!(tenant.writes().is_empty(), "{:?}", tenant.writes());
    assert_eq!(tenant.managed["objects"][0]["onCreate"]["source"], "edited");
    assert!(
        !project.0.join(".aic/undo.log").exists(),
        "the refused write left an undo entry"
    );
}

fn reported_writes(set: bool) -> Value {
    if set {
        json!(["config/managed", "config/repo.ds"])
    } else {
        json!(["config/repo.ds"])
    }
}

fn stopped_report(output: &Output, set: bool, capture: &Path) -> Value {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    let body: Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("missing progress JSON: {error}; {stderr}"));
    assert_eq!(body["confirmed_writes"], reported_writes(set), "{body}");
    assert!(stderr.contains("config/repo.ds"), "{stderr}");
    if set {
        assert!(stderr.contains("config/managed"), "{stderr}");
        assert_eq!(body["changed"].as_array().unwrap().len(), 2, "{body}");
    } else {
        assert_eq!(body["complete"], false, "{body}");
        assert_eq!(body["capture"], capture.to_str().unwrap(), "{body}");
        assert_eq!(body["restored"], json!([]), "{body}");
        let journal: Value = serde_json::from_slice(&std::fs::read(capture).unwrap()).unwrap();
        assert_eq!(journal[0]["state"], "switching", "{journal}");
    }
    body
}

#[test]
fn stale_consent_after_config_writes_reports_and_refuses_before_first_create() {
    for set in [true, false] {
        let project = Project::new();
        let capture = project.0.join("capture.json");
        let mut args = if set {
            set_args().to_vec()
        } else {
            map_args(&capture).to_vec()
        };
        args.push("--force");
        let (output, tenant) = run(&project, &args, Tenant::new(!set), |tenant, request, _| {
            if request["method"] == "PUT" && request["path"].as_str().unwrap().starts_with(REPO) {
                tenant.managed["objects"][0]["onCreate"]["source"] = json!("edited");
            }
            None
        });
        let body = stopped_report(&output, set, &capture);
        let activation = &body["storage_activation"];
        assert_eq!(activation["outcome"], "refused", "{body}");
        assert_eq!(activation["objects"].as_array().unwrap().len(), 2);
        assert!(
            activation["objects"]
                .as_array()
                .unwrap()
                .iter()
                .all(|object| object["outcome"] == "refused")
        );
        assert!(tenant.writes().iter().all(|(method, _)| *method == "PUT"));
    }
}

#[test]
fn later_probe_refuses_drift_on_its_mapping_target_and_preserves_earlier_id() {
    for set in [true, false] {
        let project = Project::new();
        let capture = project.0.join("capture.json");
        let mut args = if set {
            set_args().to_vec()
        } else {
            map_args(&capture).to_vec()
        };
        args.push("--force");
        args.extend(["--probe-prefix", "fixture-"]);
        let mut id = String::new();
        let mut fixture = Tenant::new(!set);
        fixture.managed["objects"]
            .as_array_mut()
            .unwrap()
            .push(json!({
                "name":"observer","schema":{"type":"object","properties":{}},
                "onCreate":{"type":"text/javascript","source":"original"}
            }));
        fixture.sync = json!({"mappings":[
            {"name":"watchWidgets","source":"managed/widget","target":"managed/observer"}
        ]});
        let (output, tenant) = run(&project, &args, fixture, |tenant, request, _| {
            let method = request["method"].as_str().unwrap();
            let path = request["path"].as_str().unwrap();
            if method == "POST" && path == "/openidm/managed/holder?_action=create" {
                id = request["body"]["_id"].as_str().unwrap().to_owned();
                tenant.calls.push((method.into(), path.into()));
                return Some(json_reply(json!({"_id":id})));
            }
            if method == "GET"
                && !id.is_empty()
                && path == format!("/openidm/managed/holder/{id}?_fields=_id")
            {
                return Some(json_reply(json!({"_id":id})));
            }
            if method == "DELETE"
                && !id.is_empty()
                && path == format!("/openidm/managed/holder/{id}")
            {
                tenant.calls.push((method.into(), path.into()));
                tenant.managed["objects"][2]["onCreate"]["source"] = json!("edited");
                return Some(
                    json!({"type":"api_error","status":403,"body":"fixture: keep recovery evidence"}),
                );
            }
            None
        });
        assert!(id.starts_with("fixture-"));
        let body = stopped_report(&output, set, &capture);
        let activation = &body["storage_activation"];
        assert_eq!(activation["outcome"], "refused", "{body}");
        assert_eq!(
            activation["objects"][0]["outcome"], "not_confirmed",
            "{body}"
        );
        assert_eq!(activation["objects"][0]["id"], id, "{body}");
        assert_eq!(activation["objects"][1]["outcome"], "refused", "{body}");
        assert_eq!(
            tenant
                .writes()
                .iter()
                .filter(|(method, _)| *method == "POST")
                .count(),
            1
        );
        assert!(String::from_utf8_lossy(&output.stderr).contains(&id));
    }
}

#[test]
fn later_probe_inventory_timeout_refuses_and_reports_confirmed_writes() {
    for set in [false, true] {
        let project = Project::new();
        let capture = project.0.join("capture.json");
        let mut args = if set {
            set_args().to_vec()
        } else {
            map_args(&capture).to_vec()
        };
        args.push("--force");

        let mut held = false;
        let (output, tenant) = run(&project, &args, Tenant::new(!set), |tenant, request, _| {
            if request["path"] == SYNC
                && tenant
                    .writes()
                    .contains(&("POST", "/openidm/managed/holder?_action=create"))
            {
                assert!(!held);
                held = true;
                // Keep this request open until the child exits. Reaching it
                // proves the second survey started before the probe deadline.
                return Some(json!({"fixture_hold":true}));
            }
            None
        });
        assert!(held, "the later survey never started: {output:?}");
        let body = stopped_report(&output, set, &capture);
        let activation = &body["storage_activation"];
        assert_eq!(activation["outcome"], "refused", "{body}");
        assert_eq!(activation["interrupted"], false, "{body}");
        assert_eq!(
            activation["objects"][0]["outcome"], "not_confirmed",
            "{body}"
        );
        assert_eq!(activation["objects"][1]["outcome"], "refused", "{body}");
        assert!(
            activation["objects"][1]["detail"]
                .as_str()
                .unwrap()
                .contains("inventory could not be re-read"),
            "{body}"
        );
        assert_eq!(
            tenant
                .writes()
                .into_iter()
                .filter(|(method, _)| *method == "POST")
                .collect::<Vec<_>>(),
            [("POST", "/openidm/managed/holder?_action=create")]
        );
    }
}

#[test]
fn sigint_cancels_the_first_postwrite_survey_and_reports_confirmed_writes() {
    for set in [true, false] {
        let project = Project::new();
        let capture = project.0.join("capture.json");
        let mut args = if set {
            set_args().to_vec()
        } else {
            map_args(&capture).to_vec()
        };
        args.push("--force");
        let mut signalled = false;
        let (output, tenant) = run(
            &project,
            &args,
            Tenant::new(!set),
            |tenant, request, pid| {
                if request["path"] == SYNC
                    && tenant
                        .writes()
                        .iter()
                        .any(|(_, path)| path.starts_with(REPO))
                {
                    assert!(!signalled);
                    signalled = true;
                    assert_eq!(unsafe { libc::kill(pid as i32, libc::SIGINT) }, 0);
                    // Do not answer this request. Only cancellation can let the
                    // command report the writes rather than wait on the survey.
                    return Some(json!({"fixture_hold":true}));
                }
                None
            },
        );
        assert!(signalled, "{output:?}");
        let body = stopped_report(&output, set, &capture);
        assert_eq!(body["storage_activation"]["interrupted"], true, "{body}");
        assert!(
            body["storage_activation"]["objects"]
                .as_array()
                .unwrap()
                .iter()
                .all(|object| object.get("id").is_none()),
            "{body}"
        );
        assert!(tenant.writes().iter().all(|(method, _)| *method == "PUT"));
    }
}

#[test]
fn production_set_observes_probe_consent_undo_and_put_order() {
    for force in [false, true] {
        let project = Project::new();
        let undo = project.0.join(".aic/undo.log");
        let mut args = set_args().to_vec();
        if force {
            args.push("--force");
        }
        let mut undo_on_first_survey = None;
        let mut undo_on_managed_put = None;
        let (output, tenant) = run(&project, &args, Tenant::new(false), |_, request, _| {
            if request["path"] == SYNC && undo_on_first_survey.is_none() {
                undo_on_first_survey = Some(undo.exists());
            }
            if request["method"] == "PUT" && request["path"] == MANAGED {
                undo_on_managed_put = Some((undo.exists(), request["confirmed_prod"] == true));
            }
            None
        });
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(undo_on_first_survey, Some(false), "{stderr}");
        if force {
            assert_eq!(output.status.code(), Some(0), "{stderr}");
            assert_eq!(undo_on_managed_put, Some((true, true)), "{stderr}");
            assert!(undo.exists());
        } else {
            assert_eq!(output.status.code(), Some(1), "{stderr}");
            assert!(
                stderr.contains("storage activation probe requires confirmation"),
                "{stderr}"
            );
            assert!(tenant.writes().is_empty(), "{:?}", tenant.writes());
            assert!(!undo.exists());
        }
    }
}

fn assert_first_managed_put_refusal_retires_undo(reply: Value, message: &str) {
    let project = Project::new();
    let mut args = set_args().to_vec();
    args.push("--force");
    let (output, tenant) = run(&project, &args, Tenant::new(false), |tenant, request, _| {
        if request["method"] == "PUT" && request["path"] == MANAGED {
            tenant.calls.push(("PUT".into(), MANAGED.into()));
            return Some(reply.clone());
        }
        None
    });
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains(message), "{stderr}");
    assert_eq!(tenant.managed, managed(false));
    assert_eq!(tenant.writes(), [("PUT", MANAGED)]);
    let history = DiskLog::load(project.0.join(".aic/undo.log"))
        .unwrap()
        .list(10);
    assert_eq!(history.len(), 1, "{history:?}");
    assert_eq!(history[0].status, EntryStatus::Expired, "{history:?}");
}

#[test]
fn first_managed_put_rejection_retires_undo() {
    assert_first_managed_put_refusal_retires_undo(
        json!({"type":"api_error","status":403,"body":"fixture: config write denied"}),
        "403 fixture: config write denied",
    );
}

#[test]
fn locked_agent_refusal_retires_undo() {
    assert_first_managed_put_refusal_retires_undo(json!({"type":"locked"}), "agent is locked");
}

#[test]
fn production_guard_refusal_retires_undo() {
    assert_first_managed_put_refusal_retires_undo(
        json!({"type":"prod_confirm_required"}),
        "Prod write confirmation required",
    );
}

#[test]
fn uncertain_managed_put_outcomes_keep_undo() {
    for scenario in [
        "applied 500",
        "lost response",
        "missing agent reply",
        "malformed agent reply",
        "untyped lock message",
        "request timeout",
        "rejected retry",
    ] {
        let project = Project::new();
        let mut args = set_args().to_vec();
        args.push("--force");
        let mut attempts = 0;
        let (output, tenant) = run(&project, &args, Tenant::new(false), |tenant, request, _| {
            if request["method"] != "PUT" || request["path"] != MANAGED {
                return None;
            }
            attempts += 1;
            tenant.calls.push(("PUT".into(), MANAGED.into()));
            Some(match scenario {
                "applied 500" => {
                    tenant.managed = request["body"].clone();
                    json!({"type":"api_error","status":500,"body":"fixture: write applied but response failed"})
                }
                "lost response" => json!({"type":"transport","message":"fixture: lost response"}),
                "missing agent reply" => {
                    tenant.managed = request["body"].clone();
                    json!({"fixture_raw":""})
                }
                "malformed agent reply" => json!({"fixture_raw":"not JSON\n"}),
                "untyped lock message" => json!({"type":"error","message":"agent is locked"}),
                "request timeout" => {
                    json!({"type":"api_error","status":408,"body":"fixture: request timed out"})
                }
                "rejected retry" if attempts == 1 => {
                    // The PUT was accepted but Q14 reads may still return the old document.
                    json_reply(request["body"].clone())
                }
                "rejected retry" => {
                    json!({"type":"api_error","status":403,"body":"fixture: retry denied"})
                }
                _ => unreachable!(),
            })
        });
        let stderr = String::from_utf8_lossy(&output.stderr);
        if scenario == "applied 500" {
            assert_eq!(output.status.code(), Some(0), "{stderr}");
            assert!(stderr.contains("a re-read shows every change"), "{stderr}");
            assert_ne!(tenant.managed, managed(false));
        } else {
            assert_eq!(output.status.code(), Some(1), "{scenario}: {stderr}");
        }
        assert_eq!(
            attempts,
            if scenario == "rejected retry" { 2 } else { 1 },
            "{scenario}"
        );
        let history = DiskLog::load(project.0.join(".aic/undo.log"))
            .unwrap()
            .list(10);
        assert_eq!(history.len(), 1, "{scenario}: {history:?}");
        assert_eq!(
            history[0].status,
            EntryStatus::Pending,
            "{scenario}: {history:?}"
        );
    }
}

#[cfg(target_os = "linux")]
fn script_prompts(
    command: &mut Command,
    replies: &'static [(&'static str, &'static [u8])],
) -> std::thread::JoinHandle<(usize, String)> {
    use std::io::Read;
    use std::os::fd::FromRawFd;
    use std::os::unix::process::CommandExt;

    let mut master = -1;
    let mut slave = -1;
    assert_eq!(
        unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
            )
        },
        0,
        "openpty: {}",
        std::io::Error::last_os_error(),
    );
    let mut master = unsafe { std::fs::File::from_raw_fd(master) };
    let slave = unsafe { std::fs::File::from_raw_fd(slave) };
    command
        .stdin(slave.try_clone().unwrap())
        .stderr(slave)
        .env("TERM", "xterm");
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 || libc::ioctl(libc::STDIN_FILENO, libc::TIOCSCTTY, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    std::thread::spawn(move || {
        let mut transcript = Vec::new();
        let mut pos = 0;
        let mut sent = 0;
        let mut bytes = [0; 1024];
        loop {
            let count = match master.read(&mut bytes) {
                Ok(0) => break,
                Ok(count) => count,
                Err(error) if error.raw_os_error() == Some(libc::EIO) => break,
                Err(error) => panic!("reading prompts: {error}"),
            };
            transcript.extend_from_slice(&bytes[..count]);
            if let Some((prompt, answer)) = replies.get(sent)
                && let Some(index) = transcript[pos..]
                    .windows(prompt.len())
                    .position(|w| w == prompt.as_bytes())
            {
                pos += index + prompt.len();
                master.write_all(answer).unwrap();
                sent += 1;
            }
        }
        (sent, String::from_utf8_lossy(&transcript).into_owned())
    })
}

#[cfg(target_os = "linux")]
#[test]
fn map_declines_probe_without_persisting_capture_or_switching() {
    let project = Project::new();
    let capture = project.0.join("capture.json");
    let mut command = project.command(false);
    command.args(map_args(&capture));
    let terminal = script_prompts(
        &mut command,
        &[
            ("map 1 pair(s), 0 links", b"y\r"),
            ("create, read, and delete one probe record", b"n\r"),
        ],
    );
    let (output, tenant) = run_command(&project, command, Tenant::new(true), |_, _, _| None);
    let (answered, transcript) = terminal.join().unwrap();
    assert_eq!(answered, 2, "{transcript}");
    assert!(
        !capture.exists(),
        "capture was saved after refusal: {transcript}"
    );
    assert!(tenant.writes().is_empty(), "{:?}", tenant.writes());
    assert_eq!(output.status.code(), Some(1), "{transcript}");
    assert!(transcript.contains("probe declined"), "{transcript}");
}
