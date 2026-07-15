use codex_quota_ball::{
    codex::CommandSpec,
    quota::{QuotaSnapshot, QuotaWindow},
    usage::UsageSnapshot,
    worker::{spawn_worker_with, DashboardRead, DashboardViewState, WorkerEvent},
};
use std::{
    path::PathBuf,
    sync::Mutex,
    time::{Duration, Instant},
};

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn sample_quota() -> QuotaSnapshot {
    QuotaSnapshot {
        primary: Some(QuotaWindow {
            remaining_percent: 72,
            resets_at: None,
            window_duration_mins: Some(10080),
        }),
        secondary: None,
    }
}

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
        WorkerEvent::Finished(read) => {
            assert_eq!(read.quota.unwrap().primary.unwrap().remaining_percent, 72);
            assert_eq!(read.usage.unwrap().daily.unwrap()[0].tokens, 1200);
        }
        event => panic!("unexpected event: {event:?}"),
    }
}

#[test]
fn usage_failure_keeps_usage_stale_but_updates_quota() {
    let mut state = DashboardViewState::default();
    state.apply(WorkerEvent::Started);
    state.apply(WorkerEvent::Finished(DashboardRead {
        quota: Ok(sample_quota()),
        usage: Ok(UsageSnapshot {
            daily: Some(vec![]),
        }),
    }));
    state.apply(WorkerEvent::Started);
    state.apply(WorkerEvent::Finished(DashboardRead {
        quota: Ok(sample_quota()),
        usage: Err("usage unavailable".into()),
    }));
    assert!(!state.quota.stale);
    assert!(state.usage.stale);
    assert!(state.usage.value.is_some());
    assert_eq!(state.usage.error.as_deref(), Some("usage unavailable"));
}

#[test]
fn quota_failure_does_not_discard_fresh_usage() {
    let mut state = DashboardViewState::default();
    state.apply(WorkerEvent::Started);
    state.apply(WorkerEvent::Finished(DashboardRead {
        quota: Err("quota unavailable".into()),
        usage: Ok(UsageSnapshot {
            daily: Some(vec![]),
        }),
    }));
    assert!(state.quota.value.is_none());
    assert!(state.quota.error.is_some());
    assert!(state.usage.value.is_some());
    assert!(!state.usage.stale);
    assert!(!state.refreshing);
}

#[test]
fn worker_reports_usage_error_without_discarding_quota() {
    let _guard = ENV_LOCK.lock().unwrap();
    std::env::set_var("FAKE_SCENARIO", "usage-error");
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
    match handle.events.recv_timeout(Duration::from_secs(1)).unwrap() {
        WorkerEvent::Finished(read) => {
            assert!(read.quota.is_ok());
            assert!(read.usage.is_err());
        }
        event => panic!("unexpected event: {event:?}"),
    }
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
        WorkerEvent::Finished(DashboardRead {
            quota: Ok(_),
            usage: Ok(_),
        })
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
        WorkerEvent::Finished(DashboardRead {
            quota: Err(_),
            usage: Err(_),
        })
    ));
    assert!(matches!(
        handle.events.recv_timeout(Duration::from_secs(1)).unwrap(),
        WorkerEvent::Started
    ));
    assert!(matches!(
        handle.events.recv_timeout(Duration::from_secs(1)).unwrap(),
        WorkerEvent::Finished(DashboardRead {
            quota: Err(_),
            usage: Err(_),
        })
    ));
    assert!(handle
        .events
        .recv_timeout(Duration::from_millis(150))
        .is_err());
}

#[test]
fn worker_recreates_the_client_after_a_terminal_error() {
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
    assert!(matches!(
        handle.events.recv_timeout(Duration::from_secs(1)).unwrap(),
        WorkerEvent::Finished(DashboardRead {
            quota: Err(_),
            usage: Err(_),
        })
    ));

    std::env::set_var("FAKE_SCENARIO", "success");
    handle.request_refresh();
    assert!(matches!(
        handle.events.recv_timeout(Duration::from_secs(1)).unwrap(),
        WorkerEvent::Started
    ));
    assert!(matches!(
        handle.events.recv_timeout(Duration::from_secs(1)).unwrap(),
        WorkerEvent::Finished(DashboardRead {
            quota: Ok(_),
            usage: Ok(_),
        })
    ));
}
