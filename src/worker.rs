use crate::{
    codex::{CodexClient, CommandSpec},
    quota::QuotaSnapshot,
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
    Finished(Result<QuotaSnapshot, String>),
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
            WorkerEvent::Finished(Ok(snapshot)) => {
                self.snapshot = Some(snapshot);
                self.refreshing = false;
                self.stale = false;
                self.error = None;
                self.updated_at = Some(SystemTime::now());
            }
            WorkerEvent::Finished(Err(error)) => {
                self.refreshing = false;
                self.stale = self.snapshot.is_some();
                self.error = Some(error);
            }
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
            let result = (|| {
                if client.is_none() {
                    client = Some(CodexClient::connect(spec.clone(), timeout)?);
                }
                client.as_mut().unwrap().read_quota()
            })();
            if result.is_err() {
                client = None;
            }
            if event_tx
                .send(WorkerEvent::Finished(
                    result.map_err(|error| error.to_string()),
                ))
                .is_err()
            {
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
