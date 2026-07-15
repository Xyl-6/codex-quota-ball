use codex_quota_ball::codex::{ClientError, CodexClient, CommandSpec};
use std::{path::PathBuf, sync::Mutex, time::Duration, time::Instant};

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn fake(scenario: &str) -> (std::sync::MutexGuard<'static, ()>, CommandSpec) {
    let guard = ENV_LOCK.lock().unwrap();
    std::env::set_var("FAKE_SCENARIO", scenario);
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake_codex.sh");
    (guard, CommandSpec::new(script))
}

#[test]
fn initializes_ignores_notifications_and_reads_quota() {
    let (_guard, command) = fake("success");
    let mut client = CodexClient::connect(command, Duration::from_secs(1)).unwrap();
    assert_eq!(
        client
            .read_quota()
            .unwrap()
            .primary
            .unwrap()
            .remaining_percent,
        72
    );
}

#[test]
fn categorizes_signed_out_response() {
    let (_guard, command) = fake("signed-out");
    let mut client = CodexClient::connect(command, Duration::from_secs(1)).unwrap();
    assert!(matches!(client.read_quota(), Err(ClientError::NotLoggedIn)));
}

#[test]
fn rejects_malformed_output_without_panicking() {
    let (_guard, command) = fake("malformed");
    let mut client = CodexClient::connect(command, Duration::from_secs(1)).unwrap();
    assert!(matches!(client.read_quota(), Err(ClientError::Protocol(_))));
}

#[test]
fn timeout_makes_client_terminal_and_reaps_the_blocking_process() {
    let (_guard, command) = fake("timeout");
    let mut client = CodexClient::connect(command, Duration::from_millis(50)).unwrap();
    assert!(matches!(client.read_quota(), Err(ClientError::Timeout)));

    let reuse_started = Instant::now();
    assert!(matches!(client.read_quota(), Err(ClientError::Timeout)));
    assert!(reuse_started.elapsed() < Duration::from_millis(25));
    drop(client);
}

#[test]
fn reports_child_exit_status_when_stdout_closes() {
    let (_guard, command) = fake("exit");
    let mut client = CodexClient::connect(command, Duration::from_secs(1)).unwrap();
    assert!(matches!(
        client.read_quota(),
        Err(ClientError::Process(message)) if message.contains("exit status: 7")
    ));
}

#[test]
fn bounds_version_probe_and_reports_unknown_version() {
    let (_guard, command) = fake("version-hang");
    let started = Instant::now();
    let mut client = CodexClient::connect(command, Duration::from_millis(50)).unwrap();

    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(matches!(
        client.read_quota(),
        Err(ClientError::Protocol(message)) if message.contains("unknown version")
    ));
}

#[test]
fn bounds_version_probe_when_descendant_holds_stdout() {
    let (_guard, command) = fake("version-descendant");
    let started = Instant::now();
    let mut client = CodexClient::connect(command, Duration::from_millis(50)).unwrap();

    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(matches!(
        client.read_quota(),
        Err(ClientError::Protocol(message)) if message.contains("unknown version")
    ));
}

#[test]
fn duration_max_returns_an_error_without_panicking() {
    let (_guard, command) = fake("success");
    assert!(matches!(
        CodexClient::connect(command, Duration::MAX),
        Err(ClientError::Timeout)
    ));
}
