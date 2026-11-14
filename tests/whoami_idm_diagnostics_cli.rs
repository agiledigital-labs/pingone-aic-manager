//! Exercise IDM diagnostic failures through the built CLI and real streams.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::{Command, Output};
use std::thread;

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_aic"))
}

fn reply(stream: &mut UnixStream, value: serde_json::Value) {
    writeln!(stream, "{value}").expect("write agent response");
}

fn mock_agent(root: &std::path::Path, scenario: &'static str) -> thread::JoinHandle<()> {
    let listener = UnixListener::bind(root.join(".aic/agent.sock")).expect("bind mock agent");
    thread::spawn(move || {
        let mut api_calls = 0;
        for _ in 0..8 {
            let Ok((mut stream, _)) = listener.accept() else {
                break;
            };
            let mut line = String::new();
            BufReader::new(stream.try_clone().expect("clone agent stream"))
                .read_line(&mut line)
                .expect("read agent request");
            let request: serde_json::Value = serde_json::from_str(&line).expect("wire JSON");
            match request["op"].as_str().expect("operation") {
                "ping" => reply(
                    &mut stream,
                    serde_json::json!({
                        "type": "pong", "version": "test", "pid": 1
                    }),
                ),
                "status" => reply(
                    &mut stream,
                    serde_json::json!({
                        "type": "status", "unlocked": true, "project_dir": root_placeholder(),
                        "tenants": ["stub"], "cached_tokens": [],
                        "idle_remaining_secs": 100, "idle_timeout_secs": 3600
                    }),
                ),
                "api_call" => {
                    api_calls += 1;
                    let path = request["path"].as_str().unwrap_or_default();
                    if scenario == "transport" {
                        break;
                    } else if scenario == "tokeninfo" && path.ends_with("/tokeninfo") {
                        reply(
                            &mut stream,
                            serde_json::json!({
                                "type": "api_error", "status": 503,
                                "body": "https://synthetic-host.invalid/path bearer-marker-abc"
                            }),
                        );
                    } else if scenario == "host" {
                        reply(
                            &mut stream,
                            serde_json::json!({
                                "type": "json", "value": {"domains": []}
                            }),
                        );
                    } else {
                        reply(
                            &mut stream,
                            serde_json::json!({
                                "type": "json", "value": {"authorization": {
                                    "component": "managed/svcacct", "roles": []
                                }}
                            }),
                        );
                    }
                }
                other => panic!("unexpected agent operation: {other}"),
            }
            if (scenario == "transport" && api_calls == 1)
                || (scenario == "tokeninfo" && api_calls == 2)
                || (scenario == "host" && api_calls == 2)
            {
                break;
            }
        }
    })
}

// Replaced with the project root at fixture creation; StatusInfo only uses it
// for display and the CLI does not inspect this value.
fn root_placeholder() -> &'static str {
    "/tmp/aic-cli-test"
}

fn run(scenario: &'static str, extra: &[&str]) -> Output {
    let root = std::env::temp_dir().join(format!("aic-whoami-idm-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(root.join(".aic")).expect("create project");
    std::fs::create_dir_all(root.join("data")).expect("create isolated data directory");
    std::fs::write(
        root.join(".aic/config.toml"),
        "project = \"test\"\ndefault_tenant = \"stub\"\n\n[[tenant]]\nname = \"stub\"\nbase_url = \"https://tenant.example.com\"\ntheme = \"sandbox\"\nscopes = []\n",
    )
    .expect("write config");
    let agent = mock_agent(&root, scenario);
    let output = Command::new(bin())
        .current_dir(&root)
        .env_remove("AIC_PROJECT")
        .env("XDG_DATA_HOME", root.join("data"))
        .args(["--no-prompt", "whoami", "--idm"])
        .args(extra)
        .output()
        .expect("run aic");
    agent.join().expect("mock agent thread");
    std::fs::remove_dir_all(root).expect("cleanup project");
    output
}

fn assert_markers_absent(output: &Output) {
    for stream in [&output.stdout, &output.stderr] {
        let stream = String::from_utf8_lossy(stream);
        assert!(!stream.contains("synthetic-host.invalid"), "{stream}");
        assert!(!stream.contains("bearer-marker-abc"), "{stream}");
    }
}

#[test]
fn idm_lookup_failures_and_host_validation_hide_details_on_real_streams() {
    let transport = run("transport", &[]);
    assert!(!transport.status.success());
    assert_markers_absent(&transport);

    let api = run("tokeninfo", &[]);
    assert!(!api.status.success());
    assert_markers_absent(&api);
    let stderr = String::from_utf8_lossy(&api.stderr);
    assert!(stderr.contains("HTTP 503"), "{stderr}");

    let host = run("host", &["--host", "custom.synthetic-host.invalid"]);
    assert!(!host.status.success());
    assert_markers_absent(&host);
}
