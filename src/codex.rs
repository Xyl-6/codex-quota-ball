use crate::quota::{parse_quota_response, QuotaSnapshot};
use crate::usage::{parse_usage_response, UsageSnapshot};
use serde_json::{json, Value};
use std::{
    fmt,
    io::{BufRead, BufReader, Read, Write},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver, Sender},
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

#[derive(Clone, Debug, PartialEq, Eq)]
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
    writer: Sender<WriteRequest>,
    messages: Receiver<Result<Value, String>>,
    timeout: Duration,
    next_id: u64,
    version: String,
    terminal: Option<ClientError>,
}

struct WriteRequest {
    bytes: Vec<u8>,
    ack: Sender<Result<(), String>>,
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
        let mut stdin = match child.stdin.take() {
            Some(stdin) => stdin,
            None => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(ClientError::Process("stdin unavailable".into()));
            }
        };
        let stdout = match child.stdout.take() {
            Some(stdout) => stdout,
            None => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(ClientError::Process("stdout unavailable".into()));
            }
        };
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
        let (writer, writes) = mpsc::channel::<WriteRequest>();
        thread::spawn(move || {
            for request in writes {
                let result = stdin
                    .write_all(&request.bytes)
                    .and_then(|_| stdin.flush())
                    .map_err(|error| error.to_string());
                let failed = result.is_err();
                let _ = request.ack.send(result);
                if failed {
                    break;
                }
            }
        });
        let mut client = Self {
            child,
            writer,
            messages,
            timeout,
            next_id: 1,
            version,
            terminal: None,
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
        let deadline = client.deadline()?;
        client.send(&initialize, deadline)?;
        client.recv_for_id(1, deadline)?;
        let deadline = client.deadline()?;
        client.send(&json!({"method": "initialized"}), deadline)?;
        client.next_id = 2;
        Ok(client)
    }

    fn request(&mut self, method: &str) -> Result<Value, ClientError> {
        if let Some(error) = &self.terminal {
            return Err(error.clone());
        }
        let id = self.next_id;
        self.next_id += 1;
        let deadline = self.deadline()?;
        self.send(&json!({"id": id, "method": method}), deadline)?;
        self.recv_for_id(id, deadline)
    }

    pub fn read_quota(&mut self) -> Result<QuotaSnapshot, ClientError> {
        let response = self.request("account/rateLimits/read")?;
        parse_quota_response(&response)
            .map_err(|error| ClientError::Protocol(format!("{} ({})", error, self.version)))
    }

    pub fn read_usage(&mut self) -> Result<UsageSnapshot, ClientError> {
        let response = self.request("account/usage/read")?;
        parse_usage_response(&response)
            .map_err(|error| ClientError::Protocol(format!("{} ({})", error, self.version)))
    }

    pub fn is_terminal(&self) -> bool {
        self.terminal.is_some()
    }

    fn deadline(&mut self) -> Result<Instant, ClientError> {
        match Instant::now().checked_add(self.timeout) {
            Some(deadline) => Ok(deadline),
            None => self.fail(ClientError::Timeout),
        }
    }

    fn send(&mut self, message: &Value, deadline: Instant) -> Result<(), ClientError> {
        let mut bytes = match serde_json::to_vec(message) {
            Ok(bytes) => bytes,
            Err(error) => return self.fail(ClientError::Protocol(error.to_string())),
        };
        bytes.push(b'\n');
        let (ack, written) = mpsc::channel();
        if self.writer.send(WriteRequest { bytes, ack }).is_err() {
            return self.fail(ClientError::Process("stdin writer closed".into()));
        }
        let wait = deadline.saturating_duration_since(Instant::now());
        if wait.is_zero() {
            return self.fail(ClientError::Timeout);
        }
        match written.recv_timeout(wait) {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => self.fail(ClientError::Process(error)),
            Err(mpsc::RecvTimeoutError::Timeout) => self.fail(ClientError::Timeout),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                self.fail(ClientError::Process("stdin writer closed".into()))
            }
        }
    }

    fn recv_for_id(&mut self, id: u64, deadline: Instant) -> Result<Value, ClientError> {
        loop {
            let wait = deadline.saturating_duration_since(Instant::now());
            if wait.is_zero() {
                return self.fail(ClientError::Timeout);
            }
            let value = match self.messages.recv_timeout(wait) {
                Ok(Ok(value)) => value,
                Ok(Err(error)) => return self.fail(ClientError::Protocol(error)),
                Err(mpsc::RecvTimeoutError::Timeout) => return self.fail(ClientError::Timeout),
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    let message = match self.child.try_wait() {
                        Ok(Some(status)) => format!("stdout closed ({status})"),
                        Ok(None) => "stdout closed".to_owned(),
                        Err(error) => format!("stdout closed ({error})"),
                    };
                    return self.fail(ClientError::Process(message));
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

    fn fail<T>(&mut self, error: ClientError) -> Result<T, ClientError> {
        self.terminal.get_or_insert_with(|| error.clone());
        Err(error)
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
    let mut stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return "unknown version".to_owned();
        }
    };
    let (output_sender, output) = mpsc::channel();
    thread::spawn(move || {
        let mut version = String::new();
        let result = stdout.read_to_string(&mut version).map(|_| version);
        let _ = output_sender.send(result);
    });
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
                let wait = deadline.saturating_duration_since(Instant::now());
                let version = match output.recv_timeout(wait) {
                    Ok(Ok(version)) => version,
                    Ok(Err(_)) | Err(_) => return "unknown version".to_owned(),
                };
                let version = version.trim();
                if !version.is_empty() {
                    return version.to_owned();
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
