use codex_quota_ball::{
    codex::CommandSpec,
    quota::{QuotaSnapshot, QuotaWindow},
    worker::{spawn_worker_with, QuotaViewState, WorkerEvent},
};
use std::{
    path::PathBuf,
    sync::Mutex,
    time::{Duration, Instant},
};

static ENV_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn worker_refreshes_immediately() {
    let _guard = ENV_LOCK.lock().unwrap();
    std::env::set_var("FAKE_SCENARIO", "success");
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake_codex.sh");
    let handle = spawn_worker_with(
        CommandSpec::new("bash").arg(script.to_string_lossy()),
        Duration::from_secs(1),
        Duration::from_secs(60),
    );
    assert!(matches!(
        handle.events.recv_timeout(Duration::from_secs(1)).unwrap(),
        WorkerEvent::Started
    ));
    match handle.events.recv_timeout(Duration::from_secs(1)).unwrap() {
        WorkerEvent::Finished(Ok(snapshot)) => {
            assert_eq!(snapshot.primary.unwrap().remaining_percent, 72)
        }
        event => panic!("unexpected event: {event:?}"),
    }
}

#[test]
fn a_failure_keeps_previous_data_and_marks_it_stale() {
    let mut state = QuotaViewState::default();
    let snapshot = QuotaSnapshot {
        primary: Some(QuotaWindow {
            remaining_percent: 72,
            resets_at: None,
            window_duration_mins: Some(300),
        }),
        secondary: None,
    };
    state.apply(WorkerEvent::Started);
    state.apply(WorkerEvent::Finished(Ok(snapshot.clone())));
    state.apply(WorkerEvent::Started);
    state.apply(WorkerEvent::Finished(Err("timeout".into())));
    assert_eq!(state.snapshot, Some(snapshot));
    assert!(state.stale);
    assert_eq!(state.error.as_deref(), Some("timeout"));
    assert!(!state.refreshing);
}

#[test]
fn worker_refreshes_on_the_interval() {
    let _guard = ENV_LOCK.lock().unwrap();
    std::env::set_var("FAKE_SCENARIO", "success");
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake_codex.sh");
    let handle = spawn_worker_with(
        CommandSpec::new(script),
        Duration::from_secs(1),
        Duration::from_millis(20),
    );

    assert!(matches!(
        handle.events.recv_timeout(Duration::from_secs(1)).unwrap(),
        WorkerEvent::Started
    ));
    assert!(matches!(
        handle.events.recv_timeout(Duration::from_secs(1)).unwrap(),
        WorkerEvent::Finished(Ok(_))
    ));
    assert!(matches!(
        handle.events.recv_timeout(Duration::from_secs(1)).unwrap(),
        WorkerEvent::Started
    ));
}

#[test]
fn manual_refresh_requests_are_nonblocking_and_coalesced() {
    let _guard = ENV_LOCK.lock().unwrap();
    std::env::set_var("FAKE_SCENARIO", "timeout");
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake_codex.sh");
    let handle = spawn_worker_with(
        CommandSpec::new(script),
        Duration::from_millis(50),
        Duration::from_secs(60),
    );
    assert!(matches!(
        handle.events.recv_timeout(Duration::from_secs(1)).unwrap(),
        WorkerEvent::Started
    ));

    let started = Instant::now();
    for _ in 0..100 {
        handle.request_refresh();
    }
    assert!(started.elapsed() < Duration::from_secs(1));

    assert!(matches!(
        handle.events.recv_timeout(Duration::from_secs(1)).unwrap(),
        WorkerEvent::Finished(Err(_))
    ));
    assert!(matches!(
        handle.events.recv_timeout(Duration::from_secs(1)).unwrap(),
        WorkerEvent::Started
    ));
    assert!(matches!(
        handle.events.recv_timeout(Duration::from_secs(1)).unwrap(),
        WorkerEvent::Finished(Err(_))
    ));
    assert!(handle
        .events
        .recv_timeout(Duration::from_millis(150))
        .is_err());
}

#[test]
fn worker_recreates_the_client_after_an_error() {
    let _guard = ENV_LOCK.lock().unwrap();
    std::env::set_var("FAKE_SCENARIO", "signed-out");
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake_codex.sh");
    let handle = spawn_worker_with(
        CommandSpec::new(script),
        Duration::from_secs(1),
        Duration::from_secs(60),
    );
    assert!(matches!(
        handle.events.recv_timeout(Duration::from_secs(1)).unwrap(),
        WorkerEvent::Started
    ));
    assert!(matches!(
        handle.events.recv_timeout(Duration::from_secs(1)).unwrap(),
        WorkerEvent::Finished(Err(_))
    ));

    std::env::set_var("FAKE_SCENARIO", "success");
    handle.request_refresh();
    assert!(matches!(
        handle.events.recv_timeout(Duration::from_secs(1)).unwrap(),
        WorkerEvent::Started
    ));
    assert!(matches!(
        handle.events.recv_timeout(Duration::from_secs(1)).unwrap(),
        WorkerEvent::Finished(Ok(_))
    ));
}
