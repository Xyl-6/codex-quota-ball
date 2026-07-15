use crate::quota::{parse_quota_response, QuotaSnapshot};
use serde_json::{json, Value};
use std::{
    fmt,
    io::{BufRead, BufReader, Read, Write},
    path::PathBuf,
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Debug)]
pub struct CommandSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
}

impl CommandSpec {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
        }
    }

    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn codex() -> Self {
        Self::new("codex")
            .arg("app-server")
            .arg("--listen")
            .arg("stdio://")
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ClientError {
    MissingCodex,
    NotLoggedIn,
    Timeout,
    Process(String),
    Protocol(String),
    Server(String),
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingCodex => f.write_str("找不到 Codex CLI"),
            Self::NotLoggedIn => f.write_str("Codex 尚未登录，请运行 codex login"),
            Self::Timeout => f.write_str("Codex 在限定时间内没有响应"),
            Self::Process(message) => write!(f, "Codex 服务已停止：{message}"),
            Self::Protocol(message) => write!(f, "Codex 协议不兼容：{message}"),
            Self::Server(message) => write!(f, "Codex 返回错误：{message}"),
        }
    }
}

impl std::error::Error for ClientError {}

pub struct CodexClient {
    child: Child,
    stdin: ChildStdin,
    messages: Receiver<Result<Value, String>>,
    timeout: Duration,
    next_id: u64,
    version: String,
}

impl CodexClient {
    pub fn connect(spec: CommandSpec, timeout: Duration) -> Result<Self, ClientError> {
        let version = probe_version(&spec.program, timeout);
        let mut child = Command::new(&spec.program)
            .args(&spec.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    ClientError::MissingCodex
                } else {
                    ClientError::Process(error.to_string())
                }
            })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| ClientError::Process("stdin unavailable".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ClientError::Process("stdout unavailable".into()))?;
        let (sender, messages) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let parsed = line.map_err(|error| error.to_string()).and_then(|line| {
                    serde_json::from_str(&line).map_err(|error| error.to_string())
                });
                if sender.send(parsed).is_err() {
                    break;
                }
            }
        });
        let mut client = Self {
            child,
            stdin,
            messages,
            timeout,
            next_id: 1,
            version,
        };
        let initialize = json!({
            "id": 1,
            "method": "initialize",
            "params": {
                "clientInfo": {
                    "name": "codex-quota-ball",
                    "title": "Codex Quota Ball",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "capabilities": {"experimentalApi": true}
            }
        });
        client.send(&initialize)?;
        client.recv_for_id(1)?;
        client.send(&json!({"method": "initialized"}))?;
        client.next_id = 2;
        Ok(client)
    }

    pub fn read_quota(&mut self) -> Result<QuotaSnapshot, ClientError> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({"id": id, "method": "account/rateLimits/read"}))?;
        let response = self.recv_for_id(id)?;
        parse_quota_response(&response)
            .map_err(|error| ClientError::Protocol(format!("{} ({})", error, self.version)))
    }

    fn send(&mut self, message: &Value) -> Result<(), ClientError> {
        serde_json::to_writer(&mut self.stdin, message)
            .map_err(|error| ClientError::Protocol(error.to_string()))?;
        self.stdin
            .write_all(b"\n")
            .and_then(|_| self.stdin.flush())
            .map_err(|error| ClientError::Process(error.to_string()))
    }

    fn recv_for_id(&mut self, id: u64) -> Result<Value, ClientError> {
        let deadline = Instant::now()
            .checked_add(self.timeout)
            .ok_or(ClientError::Timeout)?;
        loop {
            let wait = deadline.saturating_duration_since(Instant::now());
            if wait.is_zero() {
                return Err(ClientError::Timeout);
            }
            let value = match self.messages.recv_timeout(wait) {
                Ok(value) => value.map_err(ClientError::Protocol)?,
                Err(mpsc::RecvTimeoutError::Timeout) => return Err(ClientError::Timeout),
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    let message = match self.child.try_wait() {
                        Ok(Some(status)) => format!("stdout closed ({status})"),
                        Ok(None) => "stdout closed".to_owned(),
                        Err(error) => format!("stdout closed ({error})"),
                    };
                    return Err(ClientError::Process(message));
                }
            };
            if value.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(message) = value.pointer("/error/message").and_then(Value::as_str) {
                let lower = message.to_ascii_lowercase();
                return Err(
                    if lower.contains("login")
                        || lower.contains("logged")
                        || lower.contains("auth")
                        || lower.contains("401")
                    {
                        ClientError::NotLoggedIn
                    } else {
                        ClientError::Server(message.to_owned())
                    },
                );
            }
            return Ok(value);
        }
    }
}

fn probe_version(program: &PathBuf, timeout: Duration) -> String {
    let mut child = match Command::new(program)
        .arg("--version")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return "unknown version".to_owned(),
    };
    let deadline = match Instant::now().checked_add(timeout) {
        Some(deadline) => deadline,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return "unknown version".to_owned();
        }
    };
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return "unknown version".to_owned();
                }
                let mut version = String::new();
                if child
                    .stdout
                    .take()
                    .and_then(|mut stdout| stdout.read_to_string(&mut version).ok())
                    .is_some()
                {
                    let version = version.trim();
                    if !version.is_empty() {
                        return version.to_owned();
                    }
                }
                return "unknown version".to_owned();
            }
            Ok(None) if Instant::now() < deadline => {
                thread::sleep(
                    deadline
                        .saturating_duration_since(Instant::now())
                        .min(Duration::from_millis(10)),
                );
            }
            Ok(None) | Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return "unknown version".to_owned();
            }
        }
    }
}

impl Drop for CodexClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
