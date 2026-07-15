use codex_quota_ball::codex::{ClientError, CodexClient, CommandSpec};
use std::{path::PathBuf, sync::Mutex, time::Duration};

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn fake(scenario: &str) -> (std::sync::MutexGuard<'static, ()>, CommandSpec) {
    let guard = ENV_LOCK.lock().unwrap();
    std::env::set_var("FAKE_SCENARIO", scenario);
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake_codex.sh");
    (
        guard,
        CommandSpec::new("bash").arg(script.to_string_lossy()),
    )
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
fn times_out_and_reports_child_exit() {
    let (_guard, command) = fake("timeout");
    let mut client = CodexClient::connect(command, Duration::from_millis(50)).unwrap();
    assert!(matches!(client.read_quota(), Err(ClientError::Timeout)));
    drop(client);
    std::env::set_var("FAKE_SCENARIO", "exit");
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake_codex.sh");
    let mut exited = CodexClient::connect(
        CommandSpec::new("bash").arg(script.to_string_lossy()),
        Duration::from_secs(1),
    )
    .unwrap();
    assert!(matches!(exited.read_quota(), Err(ClientError::Process(_))));
}
