use crate::{
    codex::{CodexClient, CommandSpec},
    quota::QuotaSnapshot,
    usage::UsageSnapshot,
};
use std::{
    sync::mpsc::{self, Receiver, SyncSender, TrySendError},
    thread,
    time::{Duration, SystemTime},
};

#[derive(Clone, Copy, Debug)]
enum WorkerCommand {
    Refresh,
}

#[derive(Debug)]
pub enum WorkerEvent {
    Started,
    Finished(DashboardRead),
}

#[derive(Debug)]
pub struct DashboardRead {
    pub quota: Result<QuotaSnapshot, String>,
    pub usage: Result<UsageSnapshot, String>,
}

pub struct WorkerHandle {
    commands: SyncSender<WorkerCommand>,
    pub events: Receiver<WorkerEvent>,
}

impl WorkerHandle {
    pub fn request_refresh(&self) {
        match self.commands.try_send(WorkerCommand::Refresh) {
            Ok(()) | Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {}
        }
    }
}

#[derive(Debug)]
pub struct SectionState<T> {
    pub value: Option<T>,
    pub stale: bool,
    pub error: Option<String>,
    pub updated_at: Option<SystemTime>,
}

impl<T> Default for SectionState<T> {
    fn default() -> Self {
        Self {
            value: None,
            stale: false,
            error: None,
            updated_at: None,
        }
    }
}

impl<T> SectionState<T> {
    fn finish(&mut self, result: Result<T, String>) {
        match result {
            Ok(value) => {
                self.value = Some(value);
                self.stale = false;
                self.error = None;
                self.updated_at = Some(SystemTime::now());
            }
            Err(error) => {
                self.stale = self.value.is_some();
                self.error = Some(error);
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct DashboardViewState {
    pub quota: SectionState<QuotaSnapshot>,
    pub usage: SectionState<UsageSnapshot>,
    pub refreshing: bool,
}

impl DashboardViewState {
    pub fn apply(&mut self, event: WorkerEvent) {
        match event {
            WorkerEvent::Started => self.refreshing = true,
            WorkerEvent::Finished(read) => {
                self.quota.finish(read.quota);
                self.usage.finish(read.usage);
                self.refreshing = false;
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct QuotaViewState {
    pub snapshot: Option<QuotaSnapshot>,
    pub refreshing: bool,
    pub stale: bool,
    pub error: Option<String>,
    pub updated_at: Option<SystemTime>,
}

impl QuotaViewState {
    pub fn apply(&mut self, event: WorkerEvent) {
        match event {
            WorkerEvent::Started => self.refreshing = true,
            WorkerEvent::Finished(read) => match read.quota {
                Ok(snapshot) => {
                    self.snapshot = Some(snapshot);
                    self.refreshing = false;
                    self.stale = false;
                    self.error = None;
                    self.updated_at = Some(SystemTime::now());
                }
                Err(error) => {
                    self.refreshing = false;
                    self.stale = self.snapshot.is_some();
                    self.error = Some(error);
                }
            },
        }
    }
}

pub fn spawn_worker() -> WorkerHandle {
    spawn_worker_with(
        CommandSpec::codex(),
        Duration::from_secs(10),
        Duration::from_secs(60),
    )
}

pub fn spawn_worker_with(spec: CommandSpec, timeout: Duration, interval: Duration) -> WorkerHandle {
    let (command_tx, command_rx) = mpsc::sync_channel(1);
    let (event_tx, event_rx) = mpsc::channel();
    thread::spawn(move || {
        let mut client: Option<CodexClient> = None;
        loop {
            if event_tx.send(WorkerEvent::Started).is_err() {
                break;
            }
            let connect_error = if client.is_none() {
                match CodexClient::connect(spec.clone(), timeout) {
                    Ok(connected) => {
                        client = Some(connected);
                        None
                    }
                    Err(error) => Some(error.to_string()),
                }
            } else {
                None
            };

            let read = match connect_error {
                Some(message) => DashboardRead {
                    quota: Err(message.clone()),
                    usage: Err(message),
                },
                None => {
                    let active = client
                        .as_mut()
                        .expect("client exists after successful connect");
                    DashboardRead {
                        quota: active.read_quota().map_err(|error| error.to_string()),
                        usage: active.read_usage().map_err(|error| error.to_string()),
                    }
                }
            };
            if client
                .as_ref()
                .map(CodexClient::is_terminal)
                .unwrap_or(false)
            {
                client = None;
            }
            if event_tx.send(WorkerEvent::Finished(read)).is_err() {
                break;
            }
            match command_rx.recv_timeout(interval) {
                Ok(WorkerCommand::Refresh) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    });
    WorkerHandle {
        commands: command_tx,
        events: event_rx,
    }
}
