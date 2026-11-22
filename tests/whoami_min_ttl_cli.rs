//! The CLI must enforce its requested token lifetime on replies from an
//! already-running agent, including one built before that check existed.

use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_aic"))
}

fn reply(stream: &mut UnixStream, value: serde_json::Value) {
    writeln!(stream, "{value}").expect("write mock agent response");
}

#[derive(Debug, PartialEq, Eq)]
enum MockAgentEnd {
    Shutdown,
    TokenRequested,
    TimedOut,
}

fn temp_root(prefix: &str) -> PathBuf {
    let root = PathBuf::from("/tmp").join(format!("{prefix}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("create project root");
    root
}

fn mock_agent(listener: UnixListener) -> (Sender<()>, thread::JoinHandle<MockAgentEnd>) {
    listener
        .set_nonblocking(true)
        .expect("make mock listener nonblocking");
    let (shutdown, shutdown_rx) = mpsc::channel();
    let agent = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if shutdown_rx.try_recv().is_ok() {
                return MockAgentEnd::Shutdown;
            }
            let (mut stream, _) = match listener.accept() {
                Ok(connection) => connection,
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return MockAgentEnd::TimedOut;
                    }
                    thread::sleep(Duration::from_millis(10));
                    continue;
                }
                Err(error) => panic!("accept agent request: {error}"),
            };
            let mut line = String::new();
            BufReader::new(stream.try_clone().expect("clone stream"))
                .read_line(&mut line)
                .expect("read request");
            let request: serde_json::Value = serde_json::from_str(&line).expect("wire JSON");
            match request["op"].as_str().expect("operation") {
                "ping" => reply(
                    &mut stream,
                    serde_json::json!({ "type": "pong", "version": "test", "pid": 1 }),
                ),
                "status" => reply(
                    &mut stream,
                    serde_json::json!({
                        "type": "status",
                        "unlocked": true,
                        "project_dir": "/tmp/aic-cli-test",
                        "tenants": ["stub"],
                        "cached_tokens": [],
                        "idle_remaining_secs": 100,
                        "idle_timeout_secs": 3600
                    }),
                ),
                "get_token" => {
                    let now = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .expect("system clock")
                        .as_secs() as i64;
                    reply(
                        &mut stream,
                        serde_json::json!({
                            "type": "token",
                            "access_token": "synthetic-short-token",
                            "expires_at": now + 899,
                            "scopes": null
                        }),
                    );
                    return MockAgentEnd::TokenRequested;
                }
                other => panic!("unexpected agent operation: {other}"),
            }
        }
    });
    (shutdown, agent)
}

#[test]
fn refuses_a_short_token_from_a_mixed_version_agent_without_printing_it() {
    let root = temp_root("aic-min-ttl");
    std::fs::create_dir_all(root.join(".aic")).expect("create project");
    std::fs::create_dir_all(root.join("data")).expect("create data directory");
    std::fs::create_dir_all(root.join("state")).expect("create state directory");
    std::fs::write(
        root.join(".aic/config.toml"),
        "project = \"test\"\ndefault_tenant = \"stub\"\n\n[[tenant]]\nname = \"stub\"\nbase_url = \"https://tenant.example.com\"\ntheme = \"sandbox\"\nscopes = []\n",
    )
    .expect("write project config");

    let listener = UnixListener::bind(root.join(".aic/agent.sock")).expect("bind mock agent");
    let (shutdown, agent) = mock_agent(listener);

    let output = Command::new(bin())
        .current_dir(&root)
        .env_remove("AIC_PROJECT")
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_STATE_HOME", root.join("state"))
        .args([
            "--no-prompt",
            "whoami",
            "--token",
            "--json",
            "--min-ttl",
            "900",
        ])
        .output()
        .expect("run aic");
    let _ = shutdown.send(());
    assert_eq!(
        agent.join().expect("mock agent thread"),
        MockAgentEnd::TokenRequested
    );
    std::fs::remove_dir_all(&root).expect("cleanup project");

    assert!(!output.status.success());
    assert!(
        output.stdout.is_empty(),
        "stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("899s remaining"), "{stderr}");
    assert!(stderr.contains("900s floor"), "{stderr}");
    assert!(stderr.contains("full issued lifetime"), "{stderr}");
    assert!(stderr.contains("slightly lower floor"), "{stderr}");
    assert!(stderr.contains("aic session stop"), "{stderr}");
    assert!(!stderr.contains("synthetic-short-token"), "{stderr}");
}

#[test]
fn reports_a_cli_preflight_exit_instead_of_waiting_for_the_mock_agent() {
    let root = temp_root("aic-preflight");
    std::fs::create_dir_all(root.join(".aic")).expect("create project");
    std::fs::create_dir_all(root.join("data")).expect("create data directory");
    std::fs::create_dir_all(root.join("state")).expect("create state directory");
    std::fs::write(root.join(".aic/config.toml"), "invalid = [\n").expect("write bad config");

    let listener = UnixListener::bind(root.join(".aic/agent.sock")).expect("bind mock agent");
    let (shutdown, agent) = mock_agent(listener);
    let output = Command::new(bin())
        .current_dir(&root)
        .env_remove("AIC_PROJECT")
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_STATE_HOME", root.join("state"))
        .args(["--no-prompt", "whoami", "--json", "--min-ttl", "900"])
        .output()
        .expect("run aic");
    let _ = shutdown.send(());
    assert_eq!(
        agent.join().expect("mock agent thread"),
        MockAgentEnd::Shutdown
    );
    std::fs::remove_dir_all(&root).expect("cleanup project");

    assert!(!output.status.success());
    assert!(
        !output.stderr.is_empty(),
        "preflight stderr should explain the exit"
    );
}
