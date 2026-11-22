//! The CLI must enforce its requested token lifetime on replies from an
//! already-running agent, including one built before that check existed.

use std::io::{ErrorKind, Read, Write};
use std::ops::Deref;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_aic"))
}

fn reply(
    stream: &mut UnixStream,
    value: serde_json::Value,
    shutdown: &Receiver<()>,
    deadline: Instant,
) -> Result<(), MockAgentEnd> {
    match shutdown.try_recv() {
        Ok(()) | Err(mpsc::TryRecvError::Disconnected) => return Err(MockAgentEnd::Shutdown),
        Err(mpsc::TryRecvError::Empty) => {}
    }
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(MockAgentEnd::TimedOut);
    }
    stream
        .set_write_timeout(Some(remaining))
        .expect("set bounded agent write timeout");
    match writeln!(stream, "{value}") {
        Ok(()) => Ok(()),
        Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
            Err(MockAgentEnd::TimedOut)
        }
        Err(error) => panic!("write mock agent response: {error}"),
    }
}

fn assert_reported_remainder_is_below_floor(stderr: &str, floor: i64) {
    assert!(
        stderr.contains(&format!("requested {floor}s floor")),
        "{stderr}"
    );
    let remainder_text = stderr
        .split_once("agent returned a token with ")
        .expect("TTL refusal diagnostic")
        .1
        .split_once("s remaining")
        .expect("reported remaining lifetime")
        .0;
    let remaining: i64 = remainder_text.parse().expect("integer remaining lifetime");
    assert!(
        (0..floor).contains(&remaining),
        "{remaining}s is not below {floor}s"
    );
}

#[derive(Debug, PartialEq, Eq)]
enum MockAgentEnd {
    Shutdown,
    TokenRequested,
    TimedOut,
}

type MockAgent = (Sender<()>, Receiver<()>, thread::JoinHandle<MockAgentEnd>);
type TimedMockAgent = (MockAgent, Receiver<()>);

struct TempRoot(PathBuf);

impl Deref for TempRoot {
    type Target = std::path::Path;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn temp_root(prefix: &str) -> TempRoot {
    let root = PathBuf::from("/tmp").join(format!("{prefix}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("create project root");
    TempRoot(root)
}

fn check_mock_deadline(shutdown: &Receiver<()>, deadline: Instant) -> Result<(), MockAgentEnd> {
    match shutdown.try_recv() {
        Ok(()) | Err(mpsc::TryRecvError::Disconnected) => Err(MockAgentEnd::Shutdown),
        Err(mpsc::TryRecvError::Empty) if Instant::now() >= deadline => Err(MockAgentEnd::TimedOut),
        Err(mpsc::TryRecvError::Empty) => Ok(()),
    }
}

fn read_agent_request(
    stream: &mut UnixStream,
    shutdown: &Receiver<()>,
    deadline: Instant,
) -> Result<serde_json::Value, MockAgentEnd> {
    stream
        .set_read_timeout(Some(Duration::from_millis(25)))
        .expect("set bounded agent read timeout");
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 1024];
    loop {
        match shutdown.try_recv() {
            Ok(()) | Err(mpsc::TryRecvError::Disconnected) => return Err(MockAgentEnd::Shutdown),
            Err(mpsc::TryRecvError::Empty) => {}
        }
        if Instant::now() >= deadline {
            return Err(MockAgentEnd::TimedOut);
        }
        match stream.read(&mut chunk) {
            Ok(0) => return Err(MockAgentEnd::Shutdown),
            Ok(size) => {
                let received = &chunk[..size];
                if let Some(newline) = received.iter().position(|byte| *byte == b'\n') {
                    bytes.extend_from_slice(&received[..newline]);
                    return Ok(serde_json::from_slice(&bytes).expect("wire JSON"));
                }
                bytes.extend_from_slice(received);
            }
            Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(error) => panic!("read agent request: {error}"),
        }
    }
}

fn mock_agent(listener: UnixListener) -> MockAgent {
    mock_agent_with_timing(
        listener,
        Duration::from_secs(5),
        Duration::from_millis(1100),
    )
    .0
}

fn mock_agent_with_timing(
    listener: UnixListener,
    timeout: Duration,
    reply_delay: Duration,
) -> TimedMockAgent {
    listener
        .set_nonblocking(true)
        .expect("make mock listener nonblocking");
    let (shutdown, shutdown_rx) = mpsc::channel();
    let (accepted_tx, accepted_rx) = mpsc::channel();
    let (started_tx, started_rx) = mpsc::channel();
    let agent = thread::spawn(move || {
        let deadline = Instant::now() + timeout;
        let _ = started_tx.send(());
        loop {
            if let Err(end) = check_mock_deadline(&shutdown_rx, deadline) {
                return end;
            }
            let (mut stream, _) = match listener.accept() {
                Ok(connection) => connection,
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                    continue;
                }
                Err(error) => panic!("accept agent request: {error}"),
            };
            if let Err(end) = check_mock_deadline(&shutdown_rx, deadline) {
                return end;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return MockAgentEnd::TimedOut;
            }
            stream
                .set_write_timeout(Some(remaining))
                .expect("set bounded agent write timeout");
            let _ = accepted_tx.send(());
            let request = match read_agent_request(&mut stream, &shutdown_rx, deadline) {
                Ok(request) => request,
                Err(end) => return end,
            };
            match request["op"].as_str().expect("operation") {
                "ping" => {
                    if let Err(end) = reply(
                        &mut stream,
                        serde_json::json!({ "type": "pong", "version": "test", "pid": 1 }),
                        &shutdown_rx,
                        deadline,
                    ) {
                        return end;
                    }
                }
                "status" => {
                    if let Err(end) = reply(
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
                        &shutdown_rx,
                        deadline,
                    ) {
                        return end;
                    }
                }
                "get_token" => {
                    let now = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .expect("system clock")
                        .as_secs() as i64;
                    let delay_end = Instant::now() + reply_delay;
                    while Instant::now() < delay_end {
                        if let Err(end) = check_mock_deadline(&shutdown_rx, deadline) {
                            return end;
                        }
                        thread::sleep(Duration::from_millis(10));
                    }
                    if let Err(end) = reply(
                        &mut stream,
                        serde_json::json!({
                            "type": "token",
                            "access_token": "synthetic-short-token",
                            "expires_at": now + 899,
                            "scopes": null
                        }),
                        &shutdown_rx,
                        deadline,
                    ) {
                        return end;
                    }
                    return MockAgentEnd::TokenRequested;
                }
                other => panic!("unexpected agent operation: {other}"),
            }
        }
    });
    ((shutdown, accepted_rx, agent), started_rx)
}

struct ChildReader {
    receiver: Receiver<Vec<u8>>,
    thread: thread::JoinHandle<()>,
}

fn start_reader<R: Read + Send + 'static>(mut pipe: R) -> ChildReader {
    let (sender, receiver) = mpsc::channel();
    let thread = thread::spawn(move || {
        let mut output = Vec::new();
        let _ = pipe.read_to_end(&mut output);
        let _ = sender.send(output);
    });
    ChildReader { receiver, thread }
}

fn collect_reader(reader: ChildReader, deadline: Instant) -> Vec<u8> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    match reader.receiver.recv_timeout(remaining) {
        Ok(output) => {
            let _ = reader.thread.join();
            output
        }
        Err(_) => Vec::new(),
    }
}

fn run_cli(root: &std::path::Path, args: &[&str]) -> (Output, bool) {
    let mut child = Command::new(bin())
        .current_dir(root)
        .env_remove("AIC_PROJECT")
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_STATE_HOME", root.join("state"))
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn aic");
    let stdout = start_reader(child.stdout.take().expect("capture child stdout"));
    let stderr = start_reader(child.stderr.take().expect("capture child stderr"));
    wait_for_cli(child, stdout, stderr)
}

fn wait_for_cli(mut child: Child, stdout: ChildReader, stderr: ChildReader) -> (Output, bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    let timed_out = loop {
        if child.try_wait().expect("poll aic").is_some() {
            break false;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            break true;
        }
        thread::sleep(Duration::from_millis(10));
    };
    let status = child.wait().expect("wait for aic");
    let collect_deadline = Instant::now() + Duration::from_millis(250);
    let output = Output {
        status,
        stdout: collect_reader(stdout, collect_deadline),
        stderr: collect_reader(stderr, collect_deadline),
    };
    (output, timed_out)
}

#[test]
fn child_output_larger_than_pipe_capacity_is_collected_without_blocking() {
    let mut child = Command::new("sh")
        .args(["-c", "head -c 262144 /dev/zero"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn output fixture");
    let stdout = start_reader(child.stdout.take().expect("capture fixture stdout"));
    let stderr = start_reader(child.stderr.take().expect("capture fixture stderr"));
    let (output, timed_out) = wait_for_cli(child, stdout, stderr);
    assert!(
        !timed_out,
        "output fixture was killed by the child deadline"
    );
    assert!(output.status.success());
    assert_eq!(output.stdout.len(), 256 * 1024);
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
    let (shutdown, _accepted, agent) = mock_agent(listener);

    let (output, timed_out) = run_cli(
        &root,
        &[
            "--no-prompt",
            "whoami",
            "--token",
            "--json",
            "--min-ttl",
            "900",
        ],
    );
    let _ = shutdown.send(());
    assert_eq!(
        agent.join().expect("mock agent thread"),
        MockAgentEnd::TokenRequested
    );

    assert!(!timed_out, "aic did not exit before its deadline");
    assert!(!output.status.success());
    assert!(
        output.stdout.is_empty(),
        "stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_reported_remainder_is_below_floor(&stderr, 900);
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
    let (shutdown, _accepted, agent) = mock_agent(listener);
    let (output, timed_out) = run_cli(
        &root,
        &["--no-prompt", "whoami", "--json", "--min-ttl", "900"],
    );
    let _ = shutdown.send(());
    assert_eq!(
        agent.join().expect("mock agent thread"),
        MockAgentEnd::Shutdown
    );

    assert!(!timed_out, "aic did not exit before its deadline");
    assert!(!output.status.success());
    assert!(
        !output.stderr.is_empty(),
        "preflight stderr should explain the exit"
    );
}

#[test]
fn mock_agent_exits_when_shutdown_arrives_during_a_partial_request() {
    let root = temp_root("aic-held-open");
    std::fs::create_dir_all(root.join(".aic")).expect("create project");
    let listener = UnixListener::bind(root.join(".aic/agent.sock")).expect("bind mock agent");
    let (shutdown, accepted, agent) = mock_agent(listener);
    let mut peer = UnixStream::connect(root.join(".aic/agent.sock")).expect("connect peer");
    peer.write_all(b"{\"op\":\"ping\"")
        .expect("write partial request");
    accepted
        .recv_timeout(Duration::from_secs(1))
        .expect("mock accepted partial request");
    let _ = shutdown.send(());
    assert_eq!(
        agent.join().expect("mock agent thread"),
        MockAgentEnd::Shutdown
    );
}

#[test]
fn delayed_get_token_reply_stops_at_the_agent_deadline() {
    let root = temp_root("aic-late-token");
    std::fs::create_dir_all(root.join(".aic")).expect("create project");
    let listener = UnixListener::bind(root.join(".aic/agent.sock")).expect("bind mock agent");
    let ((_shutdown, accepted, agent), started) =
        mock_agent_with_timing(listener, Duration::from_secs(2), Duration::from_secs(3));
    started
        .recv_timeout(Duration::from_secs(1))
        .expect("mock agent started its deadline");
    let mut peer = UnixStream::connect(root.join(".aic/agent.sock")).expect("connect peer");
    peer.write_all(b"{\"op\":\"get_token\"}\n")
        .expect("send late request");
    let accepted_request = accepted.recv_timeout(Duration::from_secs(1));
    let result = agent.join().expect("mock agent thread");
    assert!(accepted_request.is_ok(), "late request was not accepted");
    assert_eq!(result, MockAgentEnd::TimedOut);
}
