# Codex Quota Ball Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a user-installable Ubuntu GNOME X11 floating ball that reads and displays the signed-in user's remaining Codex quota.

**Architecture:** A single Rust/eframe process renders the transparent always-on-top UI and owns one background worker. The worker manages a `codex app-server --listen stdio://` child over newline-delimited JSON-RPC, converts `usedPercent` into remaining quota, and sends immutable state snapshots to the UI. User-scoped desktop files provide application-menu and login-start integration.

**Tech Stack:** Rust 2021, eframe/egui 0.29, serde/serde_json, chrono, standard-library threads/channels/process I/O, Bash installer tests.

## Global Constraints

- Target Ubuntu GNOME on X11 only.
- Build one application executable; do not create a daemon or `.deb` package.
- Read quota only through `codex app-server`; never open, copy, persist, or log Codex credentials or raw account responses.
- Call only the read-only `account/rateLimits/read` method after the app-server initialization handshake.
- Interpret quota-window remaining percentage as `clamp(100 - usedPercent, 0, 100)`.
- Refresh at startup, every 60 seconds, when the card expands, and on Refresh or Retry; time out a request after 10 seconds.
- Keep the last successful data on refresh failure and visibly mark it stale.
- Persist only window position in `~/.config/codex-quota-ball/config.json` using temporary-file-plus-rename.
- Use green for remaining quota at least 50 percent, yellow for 20–49 percent, red below 20 percent, and gray when unavailable.
- Install only under the current user's home directory and require no `sudo`.
- Follow TDD: add one focused failing test, observe the expected failure, add the minimum implementation, then rerun the focused and full suites.

## File Structure

- `Cargo.toml` — package metadata and the five runtime dependencies.
- `src/lib.rs` — public module boundary used by the binary and integration tests.
- `src/quota.rs` — protocol-response decoding, domain snapshots, percentage conversion, colors, and reset-time formatting.
- `src/config.rs` — position model, monitor clamping, config path, atomic load/save.
- `src/codex.rs` — child-process lifecycle and newline-delimited JSON-RPC client.
- `src/worker.rs` — refresh scheduling, request coalescing, and stale-state reducer.
- `src/ui.rs` — eframe app, floating-ball painting, expanded card, dragging, menus, and viewport commands.
- `src/fonts.rs` — Linux CJK font discovery and egui font registration.
- `src/main.rs` — native window options, worker startup, and `eframe::run_native` wiring.
- `tests/quota_tests.rs` — quota parsing, percentage, color, and time cases.
- `tests/config_tests.rs` — persistence, malformed-config fallback, and clamping cases.
- `tests/codex_client_tests.rs` — handshake, notifications, server errors, malformed output, timeout, and exit cases.
- `tests/worker_tests.rs` — initial refresh and stale-data state transitions.
- `tests/fixtures/fake_codex.sh` — deterministic test-only app-server process.
- `tests/install_scripts.sh` — user-scoped install/uninstall verification under a temporary `HOME`.
- `assets/codex-quota-ball.svg` — application icon.
- `packaging/codex-quota-ball.desktop.in` — application-menu template.
- `packaging/codex-quota-ball-autostart.desktop.in` — login-start template.
- `scripts/install.sh` — build/copy executable and install desktop integration.
- `scripts/uninstall.sh` — remove only installed artifacts while retaining position config.
- `README.md` — requirements, installation, operation, troubleshooting, and protocol-compatibility warning.

---

### Task 1: Bootstrap the crate and quota domain

**Files:**
- Create: `Cargo.toml`
- Create: `src/lib.rs`
- Create: `src/quota.rs`
- Test: `tests/quota_tests.rs`

**Interfaces:**
- Consumes: JSON values shaped like the result of `account/rateLimits/read`.
- Produces: `QuotaSnapshot`, `QuotaWindow`, `QuotaParseError`, `parse_quota_response(&Value)`, `remaining_percent(i64)`, `ring_tone(Option<u8>)`, and `format_reset_time(Option<i64>)`.

- [ ] **Step 1: Write the failing quota-domain tests**

Create `Cargo.toml`, `src/lib.rs`, `src/quota.rs`, and `tests/quota_tests.rs`. The quota module is an API-only RED skeleton so the tests compile and fail at runtime for the missing behavior:

```toml
[package]
name = "codex-quota-ball"
version = "0.1.0"
edition = "2021"
description = "Ubuntu X11 floating indicator for remaining Codex quota"
license = "MIT"

[dependencies]
chrono = { version = "0.4", default-features = false, features = ["clock"] }
dirs = "5"
eframe = { version = "0.29", features = ["x11"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

```rust
// src/lib.rs
pub mod quota;
```

```rust
// src/quota.rs — RED skeleton, replaced in Step 3
use serde_json::Value;
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuotaWindow {
    pub remaining_percent: u8,
    pub resets_at: Option<i64>,
    pub window_duration_mins: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuotaSnapshot {
    pub primary: Option<QuotaWindow>,
    pub secondary: Option<QuotaWindow>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RingTone { Green, Yellow, Red, Gray }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuotaParseError(&'static str);

impl fmt::Display for QuotaParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(self.0) }
}

impl std::error::Error for QuotaParseError {}

pub fn remaining_percent(_: i64) -> u8 { unimplemented!("quota conversion") }
pub fn ring_tone(_: Option<u8>) -> RingTone { unimplemented!("ring tone") }
pub fn format_reset_time(_: Option<i64>) -> String { unimplemented!("reset formatting") }
pub fn parse_quota_response(_: &Value) -> Result<QuotaSnapshot, QuotaParseError> { unimplemented!("quota parsing") }
```

```rust
// tests/quota_tests.rs
use codex_quota_ball::quota::{
    format_reset_time, parse_quota_response, remaining_percent, ring_tone, RingTone,
};
use serde_json::json;

#[test]
fn converts_used_to_remaining_and_clamps_bad_values() {
    assert_eq!(remaining_percent(-4), 100);
    assert_eq!(remaining_percent(0), 100);
    assert_eq!(remaining_percent(50), 50);
    assert_eq!(remaining_percent(80), 20);
    assert_eq!(remaining_percent(100), 0);
    assert_eq!(remaining_percent(140), 0);
}

#[test]
fn selects_exact_color_boundaries() {
    assert_eq!(ring_tone(Some(50)), RingTone::Green);
    assert_eq!(ring_tone(Some(49)), RingTone::Yellow);
    assert_eq!(ring_tone(Some(20)), RingTone::Yellow);
    assert_eq!(ring_tone(Some(19)), RingTone::Red);
    assert_eq!(ring_tone(None), RingTone::Gray);
}

#[test]
fn parses_primary_and_secondary_windows() {
    let response = json!({
        "id": 2,
        "result": {"rateLimits": {
            "primary": {"usedPercent": 28, "resetsAt": 1784109000, "windowDurationMins": 300},
            "secondary": {"usedPercent": 59, "resetsAt": 1784682000, "windowDurationMins": 10080},
            "unknownFutureField": true
        }}
    });
    let parsed = parse_quota_response(&response).unwrap();
    assert_eq!(parsed.primary.unwrap().remaining_percent, 72);
    assert_eq!(parsed.secondary.unwrap().remaining_percent, 41);
}

#[test]
fn prefers_the_codex_multi_bucket_when_present() {
    let response = json!({
        "result": {
            "rateLimits": {"primary": {"usedPercent": 99}},
            "rateLimitsByLimitId": {"codex": {"primary": {"usedPercent": 25}}}
        }
    });
    assert_eq!(parse_quota_response(&response).unwrap().primary.unwrap().remaining_percent, 75);
}

#[test]
fn permits_a_missing_secondary_but_rejects_missing_primary() {
    let primary_only = json!({"result":{"rateLimits":{"primary":{"usedPercent":10}}}});
    assert!(parse_quota_response(&primary_only).unwrap().secondary.is_none());
    let missing = json!({"result":{"rateLimits":{"secondary":{"usedPercent":10}}}});
    assert_eq!(parse_quota_response(&missing).unwrap_err().to_string(), "primary quota is unavailable");
}

#[test]
fn invalid_timestamp_is_displayed_as_unavailable() {
    assert_eq!(format_reset_time(Some(i64::MAX)), "不可用");
    assert_eq!(format_reset_time(None), "不可用");
}
```

- [ ] **Step 2: Run the focused tests and observe the missing-module failure**

Run: `cargo test --test quota_tests -v`

Expected: tests compile, then FAIL with `not implemented: quota conversion`.

- [ ] **Step 3: Implement the quota domain and tolerant response decoder**

Create `src/quota.rs`:

```rust
use chrono::{DateTime, Local, Utc};
use serde::Deserialize;
use serde_json::Value;
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuotaWindow {
    pub remaining_percent: u8,
    pub resets_at: Option<i64>,
    pub window_duration_mins: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuotaSnapshot {
    pub primary: Option<QuotaWindow>,
    pub secondary: Option<QuotaWindow>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RingTone { Green, Yellow, Red, Gray }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuotaParseError(&'static str);

impl fmt::Display for QuotaParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(self.0) }
}

impl std::error::Error for QuotaParseError {}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WindowWire {
    used_percent: i64,
    resets_at: Option<i64>,
    window_duration_mins: Option<i64>,
}

#[derive(Deserialize)]
struct SnapshotWire {
    primary: Option<WindowWire>,
    secondary: Option<WindowWire>,
}

pub fn remaining_percent(used_percent: i64) -> u8 {
    (100 - used_percent).clamp(0, 100) as u8
}

pub fn ring_tone(remaining: Option<u8>) -> RingTone {
    match remaining {
        Some(50..=100) => RingTone::Green,
        Some(20..=49) => RingTone::Yellow,
        Some(_) => RingTone::Red,
        None => RingTone::Gray,
    }
}

pub fn format_reset_time(timestamp: Option<i64>) -> String {
    timestamp
        .and_then(DateTime::<Utc>::from_timestamp)
        .map(|time| time.with_timezone(&Local).format("%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "不可用".to_owned())
}

pub fn parse_quota_response(value: &Value) -> Result<QuotaSnapshot, QuotaParseError> {
    let result = value.get("result").ok_or(QuotaParseError("response has no result"))?;
    let bucket = result
        .pointer("/rateLimitsByLimitId/codex")
        .filter(|value| !value.is_null())
        .or_else(|| result.get("rateLimits"))
        .ok_or(QuotaParseError("response has no rate limits"))?;
    let wire: SnapshotWire = serde_json::from_value(bucket.clone())
        .map_err(|_| QuotaParseError("rate-limit response is incompatible"))?;
    if wire.primary.is_none() {
        return Err(QuotaParseError("primary quota is unavailable"));
    }
    let convert = |window: WindowWire| QuotaWindow {
        remaining_percent: remaining_percent(window.used_percent),
        resets_at: window.resets_at,
        window_duration_mins: window.window_duration_mins,
    };
    Ok(QuotaSnapshot {
        primary: wire.primary.map(convert),
        secondary: wire.secondary.map(convert),
    })
}
```

- [ ] **Step 4: Run focused and full tests**

Run: `cargo test --test quota_tests -v && cargo test -v`

Expected: all six quota tests PASS; the full suite PASS.

- [ ] **Step 5: Commit the quota domain**

```bash
git add Cargo.toml src/lib.rs src/quota.rs tests/quota_tests.rs
git commit -m "feat: add quota response domain"
```

---

### Task 2: Persist and clamp floating-window position

**Files:**
- Create: `src/config.rs`
- Modify: `src/lib.rs`
- Test: `tests/config_tests.rs`

**Interfaces:**
- Consumes: `Position { x: i32, y: i32 }`, monitor size, and window size.
- Produces: `ConfigStore::default_path()`, `ConfigStore::load()`, `ConfigStore::save(Position)`, `clamp_position(...)`, and `default_position(...)`.

- [ ] **Step 1: Write failing position and persistence tests**

Append `pub mod config;` to `src/lib.rs`, create the following API-only RED skeleton as `src/config.rs`, and create `tests/config_tests.rs`:

```rust
// src/config.rs — RED skeleton, replaced in Step 3
use serde::{Deserialize, Serialize};
use std::{io, path::PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position { pub x: i32, pub y: i32 }

#[derive(Clone, Debug)]
pub struct ConfigStore { path: PathBuf }

impl ConfigStore {
    pub fn new(path: PathBuf) -> Self { Self { path } }
    pub fn default_path() -> Option<PathBuf> { unimplemented!("config path") }
    pub fn load(&self) -> Option<Position> { unimplemented!("config load") }
    pub fn save(&self, _: Position) -> io::Result<()> { unimplemented!("config save") }
}

pub fn default_position(_: i32, _: i32) -> Position { unimplemented!("default position") }
pub fn clamp_position(_: Position, _: i32, _: i32, _: i32, _: i32) -> Position { unimplemented!("position clamp") }
```

```rust
use codex_quota_ball::config::{clamp_position, default_position, ConfigStore, Position};
use std::{fs, time::{SystemTime, UNIX_EPOCH}};

fn temp_path(name: &str) -> std::path::PathBuf {
    let id = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    std::env::temp_dir().join(format!("codex-quota-ball-{name}-{id}.json"))
}

#[test]
fn saves_and_loads_position_atomically() {
    let path = temp_path("roundtrip");
    let store = ConfigStore::new(path.clone());
    store.save(Position { x: 123, y: 456 }).unwrap();
    assert_eq!(store.load(), Some(Position { x: 123, y: 456 }));
    assert!(!path.with_extension("json.tmp").exists());
    let _ = fs::remove_file(path);
}

#[test]
fn malformed_config_falls_back_without_panicking() {
    let path = temp_path("malformed");
    fs::write(&path, "not-json").unwrap();
    assert_eq!(ConfigStore::new(path.clone()).load(), None);
    let _ = fs::remove_file(path);
}

#[test]
fn default_is_near_upper_right_and_clamp_keeps_window_visible() {
    assert_eq!(default_position(1920, 88), Position { x: 1808, y: 24 });
    assert_eq!(clamp_position(Position { x: 2000, y: -50 }, 1920, 1080, 360, 260), Position { x: 1560, y: 0 });
}
```

- [ ] **Step 2: Run the tests and observe unresolved config symbols**

Run: `cargo test --test config_tests -v`

Expected: tests compile, then FAIL with `not implemented: config save`.

- [ ] **Step 3: Implement atomic position persistence and geometry helpers**

Create `src/config.rs`:

```rust
use serde::{Deserialize, Serialize};
use std::{fs, io, path::PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position { pub x: i32, pub y: i32 }

#[derive(Clone, Debug)]
pub struct ConfigStore { path: PathBuf }

impl ConfigStore {
    pub fn new(path: PathBuf) -> Self { Self { path } }

    pub fn default_path() -> Option<PathBuf> {
        dirs::config_dir().map(|dir| dir.join("codex-quota-ball/config.json"))
    }

    pub fn load(&self) -> Option<Position> {
        serde_json::from_slice(&fs::read(&self.path).ok()?).ok()
    }

    pub fn save(&self, position: Position) -> io::Result<()> {
        if let Some(parent) = self.path.parent() { fs::create_dir_all(parent)?; }
        let temporary = self.path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(&position).map_err(std::io::Error::other)?;
        fs::write(&temporary, bytes)?;
        fs::rename(temporary, &self.path)
    }
}

pub fn default_position(monitor_width: i32, ball_width: i32) -> Position {
    Position { x: (monitor_width - ball_width - 24).max(0), y: 24 }
}

pub fn clamp_position(
    position: Position,
    monitor_width: i32,
    monitor_height: i32,
    window_width: i32,
    window_height: i32,
) -> Position {
    Position {
        x: position.x.clamp(0, (monitor_width - window_width).max(0)),
        y: position.y.clamp(0, (monitor_height - window_height).max(0)),
    }
}
```

- [ ] **Step 4: Run focused and full tests**

Run: `cargo test --test config_tests -v && cargo test -v`

Expected: all three config tests PASS; the full suite PASS.

- [ ] **Step 5: Commit position persistence**

```bash
git add src/lib.rs src/config.rs tests/config_tests.rs
git commit -m "feat: persist floating window position"
```

---

### Task 3: Implement the Codex app-server JSON-RPC client

**Files:**
- Create: `src/codex.rs`
- Modify: `src/lib.rs`
- Create: `tests/fixtures/fake_codex.sh`
- Test: `tests/codex_client_tests.rs`

**Interfaces:**
- Consumes: `CommandSpec { program: PathBuf, args: Vec<String> }` and a per-request timeout.
- Produces: `CodexClient::connect(CommandSpec, Duration)`, `CodexClient::read_quota() -> Result<QuotaSnapshot, ClientError>`, `ClientError`, and `CommandSpec::codex()`.

- [ ] **Step 1: Add a deterministic fake app-server and failing protocol tests**

Append `pub mod codex;` to `src/lib.rs`. Create this API-only RED skeleton as `src/codex.rs`:

```rust
// src/codex.rs — RED skeleton, replaced in Step 3
use crate::quota::QuotaSnapshot;
use std::{fmt, path::PathBuf, time::Duration};

#[derive(Clone, Debug)]
pub struct CommandSpec { pub program: PathBuf, pub args: Vec<String> }

impl CommandSpec {
    pub fn new(_: impl Into<PathBuf>) -> Self { unimplemented!("command construction") }
    pub fn arg(self, _: impl Into<String>) -> Self { unimplemented!("command argument") }
    pub fn codex() -> Self { unimplemented!("codex command") }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ClientError { MissingCodex, NotLoggedIn, Timeout, Process(String), Protocol(String), Server(String) }

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "{self:?}") }
}

impl std::error::Error for ClientError {}

pub struct CodexClient;

impl CodexClient {
    pub fn connect(_: CommandSpec, _: Duration) -> Result<Self, ClientError> { unimplemented!("client connect") }
    pub fn read_quota(&mut self) -> Result<QuotaSnapshot, ClientError> { unimplemented!("quota request") }
}
```

Create `tests/fixtures/fake_codex.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail
scenario="${FAKE_SCENARIO:-success}"
while IFS= read -r line; do
  if [[ "$line" == *'"method":"initialize"'* ]]; then
    printf '%s\n' '{"id":1,"result":{"userAgent":"fake/0.1","codexHome":"/tmp/fake","platformFamily":"unix","platformOs":"linux"}}'
  elif [[ "$line" == *'"method":"account/rateLimits/read"'* ]]; then
    case "$scenario" in
      success)
        printf '%s\n' '{"method":"account/rateLimits/updated","params":{"rateLimits":{}}}'
        printf '%s\n' '{"id":2,"result":{"rateLimits":{"primary":{"usedPercent":28,"resetsAt":1784109000,"windowDurationMins":300},"secondary":{"usedPercent":59,"resetsAt":1784682000,"windowDurationMins":10080}}}}'
        ;;
      signed-out) printf '%s\n' '{"id":2,"error":{"code":-32603,"message":"not logged in"}}' ;;
      malformed) printf '%s\n' '{broken-json' ;;
      timeout) sleep 2 ;;
      exit) exit 7 ;;
    esac
  fi
done
```

Create `tests/codex_client_tests.rs`:

```rust
use codex_quota_ball::codex::{ClientError, CodexClient, CommandSpec};
use std::{path::PathBuf, sync::Mutex, time::Duration};

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn fake(scenario: &str) -> (std::sync::MutexGuard<'static, ()>, CommandSpec) {
    let guard = ENV_LOCK.lock().unwrap();
    std::env::set_var("FAKE_SCENARIO", scenario);
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake_codex.sh");
    (guard, CommandSpec::new("bash").arg(script.to_string_lossy()))
}

#[test]
fn initializes_ignores_notifications_and_reads_quota() {
    let (_guard, command) = fake("success");
    let mut client = CodexClient::connect(command, Duration::from_secs(1)).unwrap();
    assert_eq!(client.read_quota().unwrap().primary.unwrap().remaining_percent, 72);
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
    let mut exited = CodexClient::connect(CommandSpec::new("bash").arg(script.to_string_lossy()), Duration::from_secs(1)).unwrap();
    assert!(matches!(exited.read_quota(), Err(ClientError::Process(_))));
}
```

- [ ] **Step 2: Run protocol tests and observe missing client types**

Run: `cargo test --test codex_client_tests -- --test-threads=1`

Expected: tests compile, then FAIL with `not implemented: command construction`.

- [ ] **Step 3: Implement child lifecycle, handshake, response matching, and error mapping**

Create `src/codex.rs`:

```rust
use crate::quota::{parse_quota_response, QuotaSnapshot};
use serde_json::{json, Value};
use std::{
    fmt,
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Debug)]
pub struct CommandSpec { pub program: PathBuf, pub args: Vec<String> }

impl CommandSpec {
    pub fn new(program: impl Into<PathBuf>) -> Self { Self { program: program.into(), args: Vec::new() } }
    pub fn arg(mut self, arg: impl Into<String>) -> Self { self.args.push(arg.into()); self }
    pub fn codex() -> Self { Self::new("codex").arg("app-server").arg("--listen").arg("stdio://") }
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
        let version = Command::new(&spec.program)
            .arg("--version")
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
            .filter(|version| !version.is_empty())
            .unwrap_or_else(|| "unknown version".to_owned());
        let mut child = Command::new(&spec.program)
            .args(&spec.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| if error.kind() == std::io::ErrorKind::NotFound { ClientError::MissingCodex } else { ClientError::Process(error.to_string()) })?;
        let stdin = child.stdin.take().ok_or_else(|| ClientError::Process("stdin unavailable".into()))?;
        let stdout = child.stdout.take().ok_or_else(|| ClientError::Process("stdout unavailable".into()))?;
        let (sender, messages) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let parsed = line.map_err(|e| e.to_string()).and_then(|line| serde_json::from_str(&line).map_err(|e| e.to_string()));
                if sender.send(parsed).is_err() { break; }
            }
        });
        let mut client = Self { child, stdin, messages, timeout, next_id: 1, version };
        let initialize = json!({
            "id": 1,
            "method": "initialize",
            "params": {"clientInfo": {"name": "codex-quota-ball", "title": "Codex Quota Ball", "version": env!("CARGO_PKG_VERSION")}, "capabilities": {"experimentalApi": true}}
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
        serde_json::to_writer(&mut self.stdin, message).map_err(|e| ClientError::Protocol(e.to_string()))?;
        self.stdin.write_all(b"\n").and_then(|_| self.stdin.flush()).map_err(|e| ClientError::Process(e.to_string()))
    }

    fn recv_for_id(&mut self, id: u64) -> Result<Value, ClientError> {
        let deadline = Instant::now() + self.timeout;
        loop {
            let wait = deadline.saturating_duration_since(Instant::now());
            if wait.is_zero() { return Err(ClientError::Timeout); }
            let value = self.messages.recv_timeout(wait).map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => ClientError::Timeout,
                mpsc::RecvTimeoutError::Disconnected => ClientError::Process("stdout closed".into()),
            })?.map_err(ClientError::Protocol)?;
            if value.get("id").and_then(Value::as_u64) != Some(id) { continue; }
            if let Some(message) = value.pointer("/error/message").and_then(Value::as_str) {
                let lower = message.to_ascii_lowercase();
                return Err(if lower.contains("login") || lower.contains("auth") || lower.contains("401") { ClientError::NotLoggedIn } else { ClientError::Server(message.to_owned()) });
            }
            return Ok(value);
        }
    }
}

impl Drop for CodexClient {
    fn drop(&mut self) { let _ = self.child.kill(); let _ = self.child.wait(); }
}
```

- [ ] **Step 4: Run focused and full tests**

Run: `cargo test --test codex_client_tests -- --test-threads=1 && cargo test -- --test-threads=1`

Expected: all four protocol tests PASS; the full suite PASS.

- [ ] **Step 5: Commit the protocol client**

```bash
git add src/lib.rs src/codex.rs tests/fixtures/fake_codex.sh tests/codex_client_tests.rs
git commit -m "feat: read quota from codex app server"
```

---
### Task 4: Add refresh scheduling and stale-state reduction

**Files:**
- Create: `src/worker.rs`
- Modify: `src/lib.rs`
- Test: `tests/worker_tests.rs`

**Interfaces:**
- Consumes: `CommandSpec`, request timeout, refresh interval, and `WorkerCommand::Refresh`.
- Produces: `spawn_worker()`, `spawn_worker_with(...)`, `WorkerHandle::request_refresh()`, `WorkerEvent`, and `QuotaViewState::apply(WorkerEvent)`.

- [ ] **Step 1: Write failing worker and state-reducer tests**

Append `pub mod worker;` to `src/lib.rs`, create this API-only RED skeleton as `src/worker.rs`, and create `tests/worker_tests.rs`:

```rust
// src/worker.rs — RED skeleton, replaced in Step 3
use crate::{codex::CommandSpec, quota::QuotaSnapshot};
use std::{sync::mpsc::Receiver, time::{Duration, SystemTime}};

#[derive(Debug)]
pub enum WorkerEvent { Started, Finished(Result<QuotaSnapshot, String>) }

pub struct WorkerHandle { pub events: Receiver<WorkerEvent> }

impl WorkerHandle { pub fn request_refresh(&self) { unimplemented!("refresh request") } }

#[derive(Debug, Default)]
pub struct QuotaViewState {
    pub snapshot: Option<QuotaSnapshot>,
    pub refreshing: bool,
    pub stale: bool,
    pub error: Option<String>,
    pub updated_at: Option<SystemTime>,
}

impl QuotaViewState { pub fn apply(&mut self, _: WorkerEvent) { unimplemented!("state reduction") } }

pub fn spawn_worker() -> WorkerHandle { unimplemented!("worker spawn") }
pub fn spawn_worker_with(_: CommandSpec, _: Duration, _: Duration) -> WorkerHandle { unimplemented!("worker spawn") }
```

```rust
use codex_quota_ball::{
    codex::CommandSpec,
    quota::{QuotaSnapshot, QuotaWindow},
    worker::{spawn_worker_with, QuotaViewState, WorkerEvent},
};
use std::{path::PathBuf, sync::Mutex, time::Duration};

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
    assert!(matches!(handle.events.recv_timeout(Duration::from_secs(1)).unwrap(), WorkerEvent::Started));
    match handle.events.recv_timeout(Duration::from_secs(1)).unwrap() {
        WorkerEvent::Finished(Ok(snapshot)) => assert_eq!(snapshot.primary.unwrap().remaining_percent, 72),
        event => panic!("unexpected event: {event:?}"),
    }
}

#[test]
fn a_failure_keeps_previous_data_and_marks_it_stale() {
    let mut state = QuotaViewState::default();
    let snapshot = QuotaSnapshot {
        primary: Some(QuotaWindow { remaining_percent: 72, resets_at: None, window_duration_mins: Some(300) }),
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
```

- [ ] **Step 2: Run worker tests and observe missing worker symbols**

Run: `cargo test --test worker_tests -- --test-threads=1`

Expected: tests compile, then FAIL with `not implemented: command construction`.

- [ ] **Step 3: Implement one background thread with a capacity-one refresh queue**

Create `src/worker.rs`:

```rust
use crate::{codex::{CodexClient, CommandSpec}, quota::QuotaSnapshot};
use std::{
    sync::mpsc::{self, Receiver, SyncSender, TrySendError},
    thread,
    time::{Duration, SystemTime},
};

#[derive(Clone, Copy, Debug)]
enum WorkerCommand { Refresh }

#[derive(Debug)]
pub enum WorkerEvent { Started, Finished(Result<QuotaSnapshot, String>) }

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
    spawn_worker_with(CommandSpec::codex(), Duration::from_secs(10), Duration::from_secs(60))
}

pub fn spawn_worker_with(spec: CommandSpec, timeout: Duration, interval: Duration) -> WorkerHandle {
    let (command_tx, command_rx) = mpsc::sync_channel(1);
    let (event_tx, event_rx) = mpsc::channel();
    thread::spawn(move || {
        let mut client: Option<CodexClient> = None;
        let mut refresh = true;
        loop {
            if refresh {
                if event_tx.send(WorkerEvent::Started).is_err() { break; }
                let result = (|| {
                    if client.is_none() { client = Some(CodexClient::connect(spec.clone(), timeout)?); }
                    client.as_mut().unwrap().read_quota()
                })();
                if result.is_err() { client = None; }
                if event_tx.send(WorkerEvent::Finished(result.map_err(|error| error.to_string()))).is_err() { break; }
                refresh = false;
            }
            match command_rx.recv_timeout(interval) {
                Ok(WorkerCommand::Refresh) | Err(mpsc::RecvTimeoutError::Timeout) => refresh = true,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    });
    WorkerHandle { commands: command_tx, events: event_rx }
}
```

- [ ] **Step 4: Run focused and full tests**

Run: `cargo test --test worker_tests -- --test-threads=1 && cargo test -- --test-threads=1`

Expected: both worker tests PASS; the full suite PASS.

- [ ] **Step 5: Commit background refresh behavior**

```bash
git add src/lib.rs src/worker.rs tests/worker_tests.rs
git commit -m "feat: schedule quota refreshes"
```

---

### Task 5: Build the transparent floating-ball UI

**Files:**
- Create: `src/fonts.rs`
- Create: `src/ui.rs`
- Create: `src/main.rs`
- Modify: `src/lib.rs`
- Test: `tests/ui_tests.rs`

**Interfaces:**
- Consumes: `WorkerHandle`, `QuotaViewState`, `ConfigStore`, `QuotaSnapshot`, and eframe viewport information.
- Produces: `FloatingApp::new(...)`, `load_cjk_font(&Context)`, `ring_points(center, radius, remaining)`, and `window_size(expanded)`.

- [ ] **Step 1: Write failing tests for ring geometry and viewport sizes**

Append `pub mod fonts;` and `pub mod ui;` to `src/lib.rs`, create an empty `src/fonts.rs`, create this API-only RED skeleton as `src/ui.rs`, and create `tests/ui_tests.rs`:

```rust
// src/ui.rs — RED skeleton, replaced in Step 4
use eframe::egui;

pub const BALL_SIZE: egui::Vec2 = egui::vec2(88.0, 88.0);
pub const EXPANDED_SIZE: egui::Vec2 = egui::vec2(360.0, 260.0);

pub fn window_size(_: bool) -> egui::Vec2 { unimplemented!("window size") }
pub fn ring_points(_: egui::Pos2, _: f32, _: u8) -> Vec<egui::Pos2> { unimplemented!("ring geometry") }
```

```rust
use codex_quota_ball::ui::{ring_points, window_size, BALL_SIZE, EXPANDED_SIZE};
use eframe::egui;

#[test]
fn ring_geometry_handles_empty_half_and_full_values() {
    assert!(ring_points(egui::pos2(44.0, 44.0), 36.0, 0).is_empty());
    assert!(ring_points(egui::pos2(44.0, 44.0), 36.0, 50).len() >= 24);
    let full = ring_points(egui::pos2(44.0, 44.0), 36.0, 100);
    assert!(full.len() >= 48);
    assert!((full.first().unwrap().x - full.last().unwrap().x).abs() < 0.01);
}

#[test]
fn compact_and_expanded_sizes_are_fixed() {
    assert_eq!(window_size(false), BALL_SIZE);
    assert_eq!(window_size(true), EXPANDED_SIZE);
}
```

- [ ] **Step 2: Run UI tests and observe missing UI symbols**

Run: `cargo test --test ui_tests -v`

Expected: tests compile, then FAIL with `not implemented: ring geometry`.

- [ ] **Step 3: Implement CJK font loading**

Create `src/fonts.rs`:

```rust
use eframe::egui;

pub fn load_cjk_font(ctx: &egui::Context) {
    let candidates = [
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
    ];
    let Some(bytes) = candidates.iter().find_map(|path| std::fs::read(path).ok()) else { return; };
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert("cjk".into(), egui::FontData::from_owned(bytes));
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts.families.entry(family).or_default().push("cjk".into());
    }
    ctx.set_fonts(fonts);
}
```

- [ ] **Step 4: Implement the floating ball, card, drag persistence, and context menu**

Create `src/ui.rs`:

```rust
use crate::{
    config::{clamp_position, default_position, ConfigStore, Position},
    quota::{format_reset_time, ring_tone, QuotaWindow, RingTone},
    worker::{QuotaViewState, WorkerHandle},
};
use eframe::egui;

pub const BALL_SIZE: egui::Vec2 = egui::vec2(88.0, 88.0);
pub const EXPANDED_SIZE: egui::Vec2 = egui::vec2(360.0, 260.0);

pub fn window_size(expanded: bool) -> egui::Vec2 { if expanded { EXPANDED_SIZE } else { BALL_SIZE } }

pub fn ring_points(center: egui::Pos2, radius: f32, remaining: u8) -> Vec<egui::Pos2> {
    if remaining == 0 { return Vec::new(); }
    let segments = ((remaining as usize * 64) / 100).max(2);
    (0..=segments).map(|step| {
        let angle = -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * remaining as f32 / 100.0 * step as f32 / segments as f32;
        center + egui::vec2(angle.cos(), angle.sin()) * radius
    }).collect()
}

pub struct FloatingApp {
    worker: WorkerHandle,
    state: QuotaViewState,
    config: ConfigStore,
    saved_position: Option<Position>,
    expanded: bool,
    positioned: bool,
}

impl FloatingApp {
    pub fn new(worker: WorkerHandle, config: ConfigStore) -> Self {
        let saved_position = config.load();
        Self { worker, state: QuotaViewState::default(), config, saved_position, expanded: false, positioned: false }
    }

    fn set_expanded(&mut self, ctx: &egui::Context, expanded: bool) {
        if self.expanded == expanded { return; }
        self.expanded = expanded;
        let size = window_size(expanded);
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
        let (monitor, outer) = ctx.input(|input| (input.viewport().monitor_size, input.viewport().outer_rect));
        if let (Some(monitor), Some(outer)) = (monitor, outer) {
            let position = Position { x: outer.min.x.round() as i32, y: outer.min.y.round() as i32 };
            let clamped = clamp_position(position, monitor.x as i32, monitor.y as i32, size.x as i32, size.y as i32);
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(clamped.x as f32, clamped.y as f32)));
        }
        if expanded { self.worker.request_refresh(); }
    }

    fn place_once(&mut self, ctx: &egui::Context) {
        if self.positioned { return; }
        let monitor = ctx.input(|input| input.viewport().monitor_size).unwrap_or(egui::vec2(1920.0, 1080.0));
        let initial = self.saved_position.unwrap_or_else(|| default_position(monitor.x as i32, BALL_SIZE.x as i32));
        let clamped = clamp_position(initial, monitor.x as i32, monitor.y as i32, BALL_SIZE.x as i32, BALL_SIZE.y as i32);
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(clamped.x as f32, clamped.y as f32)));
        self.positioned = true;
    }

    fn save_current_position(&mut self, ctx: &egui::Context) {
        if let Some(rect) = ctx.input(|input| input.viewport().outer_rect) {
            let position = Position { x: rect.min.x.round() as i32, y: rect.min.y.round() as i32 };
            if self.config.save(position).is_ok() { self.saved_position = Some(position); }
        }
    }

    fn paint_ball(&self, ui: &mut egui::Ui, rect: egui::Rect) {
        let remaining = self.state.snapshot.as_ref().and_then(|snapshot| snapshot.primary.as_ref()).map(|window| window.remaining_percent);
        let color = match ring_tone(remaining) {
            RingTone::Green => egui::Color32::from_rgb(34, 197, 94),
            RingTone::Yellow => egui::Color32::from_rgb(234, 179, 8),
            RingTone::Red => egui::Color32::from_rgb(239, 68, 68),
            RingTone::Gray => egui::Color32::from_rgb(100, 116, 139),
        };
        let center = rect.center();
        ui.painter().circle_filled(center, 35.0, egui::Color32::from_rgb(23, 32, 51));
        ui.painter().circle_stroke(center, 38.0, egui::Stroke::new(7.0, egui::Color32::from_rgb(51, 65, 85)));
        let points = ring_points(center, 38.0, remaining.unwrap_or(0));
        if points.len() > 1 { ui.painter().add(egui::Shape::line(points, egui::Stroke::new(7.0, color))); }
        let label = remaining.map(|value| format!("{value}%")).unwrap_or_else(|| "!".into());
        ui.painter().text(center, egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(19.0), egui::Color32::WHITE);
    }

    fn quota_row(ui: &mut egui::Ui, title: &str, window: Option<&QuotaWindow>) {
        match window {
            Some(window) => {
                ui.horizontal(|ui| { ui.label(title); ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| { ui.strong(format!("{}%", window.remaining_percent)); }); });
                ui.add(egui::ProgressBar::new(window.remaining_percent as f32 / 100.0).show_percentage());
                ui.small(format!("重置时间 {}", format_reset_time(window.resets_at)));
            }
            None => { ui.label(format!("{title}：不可用")); }
        }
    }

    fn expanded_card(&mut self, ctx: &egui::Context) {
        egui::Area::new(egui::Id::new("quota-card")).fixed_pos(egui::pos2(48.0, 12.0)).show(ctx, |ui| {
            egui::Frame::none().fill(egui::Color32::from_rgb(30, 41, 59)).rounding(16.0).inner_margin(16.0).show(ui, |ui| {
                ui.set_width(280.0);
                ui.horizontal(|ui| {
                    ui.heading("Codex 额度");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add_enabled(!self.state.refreshing, egui::Button::new("↻ 刷新")).clicked() { self.worker.request_refresh(); }
                    });
                });
                let status = if self.state.stale {
                    "数据可能已过期".to_owned()
                } else if self.state.refreshing {
                    "正在更新…".to_owned()
                } else {
                    self.state.updated_at
                        .and_then(|time| time.elapsed().ok())
                        .map(|elapsed| format!("{} 分钟前更新", elapsed.as_secs() / 60))
                        .unwrap_or_else(|| "等待首次更新".to_owned())
                };
                ui.small(status);
                ui.add_space(10.0);
                let primary = self.state.snapshot.as_ref().and_then(|snapshot| snapshot.primary.as_ref());
                let secondary = self.state.snapshot.as_ref().and_then(|snapshot| snapshot.secondary.as_ref());
                Self::quota_row(ui, "短周期窗口", primary);
                ui.add_space(10.0);
                Self::quota_row(ui, "周周期窗口", secondary);
                if let Some(error) = &self.state.error {
                    ui.colored_label(egui::Color32::from_rgb(248, 113, 113), error);
                    if ui.button("重试").clicked() { self.worker.request_refresh(); }
                }
            });
        });
    }
}

impl eframe::App for FloatingApp {
    fn clear_color(&self, _: &egui::Visuals) -> [f32; 4] { [0.0, 0.0, 0.0, 0.0] }

    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.place_once(ctx);
        while let Ok(event) = self.worker.events.try_recv() { self.state.apply(event); }
        if self.expanded && ctx.input(|input| input.viewport().focused == Some(false)) { self.set_expanded(ctx, false); }

        egui::CentralPanel::default().frame(egui::Frame::none().fill(egui::Color32::TRANSPARENT)).show(ctx, |ui| {
            let ball = egui::Rect::from_min_size(ui.min_rect().min, BALL_SIZE);
            let response = ui.interact(ball, ui.id().with("ball"), egui::Sense::click_and_drag());
            self.paint_ball(ui, ball);
            if response.drag_started() { ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag); }
            if response.drag_stopped() { self.save_current_position(ctx); }
            if response.clicked() { self.set_expanded(ctx, !self.expanded); }
            response.context_menu(|ui| {
                if ui.button("刷新").clicked() { self.worker.request_refresh(); ui.close_menu(); }
                if ui.button("退出").clicked() { ctx.send_viewport_cmd(egui::ViewportCommand::Close); }
            });
        });
        if self.expanded { self.expanded_card(ctx); }
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
    }
}
```

- [ ] **Step 5: Wire native X11 window options and application startup**

Create `src/main.rs`:

```rust
use codex_quota_ball::{config::ConfigStore, fonts::load_cjk_font, ui::{FloatingApp, BALL_SIZE}, worker::spawn_worker};
use eframe::{egui, NativeOptions};

fn main() -> eframe::Result {
    if std::env::var("XDG_SESSION_TYPE").as_deref() != Ok("x11") {
        eprintln!("Codex Quota Ball 0.1 supports Ubuntu GNOME X11 only.");
        std::process::exit(2);
    }
    let config_path = ConfigStore::default_path().expect("Linux config directory is unavailable");
    let options = NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(BALL_SIZE)
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(false)
            .with_always_on_top(),
        ..Default::default()
    };
    eframe::run_native(
        "Codex Quota Ball",
        options,
        Box::new(move |creation| {
            load_cjk_font(&creation.egui_ctx);
            Ok(Box::new(FloatingApp::new(spawn_worker(), ConfigStore::new(config_path))))
        }),
    )
}
```

- [ ] **Step 6: Run tests, compiler checks, and the local app**

Run: `cargo fmt --check && cargo test -- --test-threads=1 && cargo clippy --all-targets -- -D warnings`

Expected: formatting check PASS, all tests PASS, and clippy exits 0 with no warnings.

Run: `cargo run`

Expected on Ubuntu GNOME X11: a transparent 88-pixel always-on-top ball appears; click expands the card; drag moves it; right-click shows Refresh and Quit. Close it through Quit after manual inspection.

- [ ] **Step 7: Commit the UI application**

```bash
git add src/lib.rs src/fonts.rs src/ui.rs src/main.rs tests/ui_tests.rs
git commit -m "feat: add floating quota interface"
```

---

### Task 6: Add user-scoped installation and acceptance documentation

**Files:**
- Create: `assets/codex-quota-ball.svg`
- Create: `packaging/codex-quota-ball.desktop.in`
- Create: `packaging/codex-quota-ball-autostart.desktop.in`
- Create: `scripts/install.sh`
- Create: `scripts/uninstall.sh`
- Create: `tests/install_scripts.sh`
- Create: `README.md`

**Interfaces:**
- Consumes: optional executable path as `scripts/install.sh [BINARY]`.
- Produces: `~/.local/bin/codex-quota-ball`, application/autostart desktop files with an absolute `Exec` path, installed SVG icon, and a non-destructive uninstaller.

- [ ] **Step 1: Write the failing isolated installer test**

Create `tests/install_scripts.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
test_home="$(mktemp -d)"
trap 'rm -rf "$test_home"' EXIT
fake_binary="$test_home/fake-codex-quota-ball"
printf '%s\n' '#!/usr/bin/env bash' 'exit 0' > "$fake_binary"
chmod +x "$fake_binary"
HOME="$test_home" "$repo/scripts/install.sh" "$fake_binary"
test -x "$test_home/.local/bin/codex-quota-ball"
test -f "$test_home/.local/share/applications/codex-quota-ball.desktop"
test -f "$test_home/.local/share/icons/hicolor/scalable/apps/codex-quota-ball.svg"
test -f "$test_home/.config/autostart/codex-quota-ball.desktop"
grep -F "Exec=$test_home/.local/bin/codex-quota-ball" "$test_home/.local/share/applications/codex-quota-ball.desktop"
mkdir -p "$test_home/.config/codex-quota-ball"
printf '%s\n' '{"x":1,"y":2}' > "$test_home/.config/codex-quota-ball/config.json"
HOME="$test_home" "$repo/scripts/uninstall.sh"
test ! -e "$test_home/.local/bin/codex-quota-ball"
test ! -e "$test_home/.local/share/applications/codex-quota-ball.desktop"
test ! -e "$test_home/.local/share/icons/hicolor/scalable/apps/codex-quota-ball.svg"
test ! -e "$test_home/.config/autostart/codex-quota-ball.desktop"
test -f "$test_home/.config/codex-quota-ball/config.json"
```

- [ ] **Step 2: Run the installer test and observe missing scripts**

Run: `bash tests/install_scripts.sh`

Expected: FAIL with `scripts/install.sh: No such file or directory`.

- [ ] **Step 3: Add desktop templates and the SVG icon**

Create `packaging/codex-quota-ball.desktop.in`:

```ini
[Desktop Entry]
Type=Application
Name=Codex Quota Ball
Comment=Show remaining Codex quota
Exec=@EXEC@
Icon=codex-quota-ball
Terminal=false
Categories=Utility;Development;
StartupNotify=false
```

Create `packaging/codex-quota-ball-autostart.desktop.in`:

```ini
[Desktop Entry]
Type=Application
Name=Codex Quota Ball
Exec=@EXEC@
Icon=codex-quota-ball
Terminal=false
X-GNOME-Autostart-enabled=true
```

Create `assets/codex-quota-ball.svg`:

```xml
<svg xmlns="http://www.w3.org/2000/svg" width="128" height="128" viewBox="0 0 128 128">
  <circle cx="64" cy="64" r="56" fill="#172033" stroke="#334155" stroke-width="12"/>
  <path d="M64 8a56 56 0 1 1-53.3 73.3" fill="none" stroke="#22c55e" stroke-width="12" stroke-linecap="round"/>
  <text x="64" y="72" text-anchor="middle" font-family="sans-serif" font-size="28" font-weight="700" fill="#ffffff">72</text>
</svg>
```

- [ ] **Step 4: Implement user-scoped install and uninstall scripts**

Create `scripts/install.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
binary="${1:-$root/target/release/codex-quota-ball}"
if [[ ! -x "$binary" ]]; then
  cargo build --release --manifest-path "$root/Cargo.toml"
fi
destination="$HOME/.local/bin/codex-quota-ball"
install -Dm755 "$binary" "$destination"
install -Dm644 "$root/assets/codex-quota-ball.svg" "$HOME/.local/share/icons/hicolor/scalable/apps/codex-quota-ball.svg"
install -d "$HOME/.local/share/applications" "$HOME/.config/autostart"
sed "s|@EXEC@|$destination|g" "$root/packaging/codex-quota-ball.desktop.in" > "$HOME/.local/share/applications/codex-quota-ball.desktop"
sed "s|@EXEC@|$destination|g" "$root/packaging/codex-quota-ball-autostart.desktop.in" > "$HOME/.config/autostart/codex-quota-ball.desktop"
printf 'Installed Codex Quota Ball for %s\n' "$USER"
```

Create `scripts/uninstall.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail
rm -f \
  "$HOME/.local/bin/codex-quota-ball" \
  "$HOME/.local/share/applications/codex-quota-ball.desktop" \
  "$HOME/.local/share/icons/hicolor/scalable/apps/codex-quota-ball.svg" \
  "$HOME/.config/autostart/codex-quota-ball.desktop"
printf 'Uninstalled Codex Quota Ball; saved position was retained.\n'
```

Run: `chmod +x scripts/install.sh scripts/uninstall.sh tests/install_scripts.sh`

- [ ] **Step 5: Write the operator-facing README**

Create `README.md`:

````markdown
# Codex Quota Ball

A small always-on-top quota indicator for Codex on Ubuntu GNOME X11.

## Requirements

- Ubuntu GNOME with `XDG_SESSION_TYPE=x11`
- A working `codex` command logged in through ChatGPT
- Rust and Cargo when installing from source

## Install

```bash
./scripts/install.sh
```

The script builds a release binary when needed, installs it under `~/.local`, adds an application-menu entry, and enables login startup. It does not use `sudo`.

## Use

- Left click: expand or collapse quota details.
- Drag: move and remember the ball position.
- Right click: refresh or quit.
- Green means at least 50% remains, yellow means 20–49%, red means below 20%, and gray means quota is unavailable.

## Troubleshooting

- “找不到 Codex CLI”: ensure `codex` is on the desktop session's `PATH`.
- “Codex 尚未登录”: run `codex login`, then choose Retry.
- Protocol incompatibility: run `codex --version` and update Codex Quota Ball or Codex CLI to a compatible version.

Codex app-server is an experimental local protocol. This application reads only `account/rateLimits/read`, stores no credentials, and may require compatibility updates when Codex changes.

## Uninstall

```bash
./scripts/uninstall.sh
```

Uninstalling keeps `~/.config/codex-quota-ball/config.json` so the saved position survives reinstalling.
````

- [ ] **Step 6: Run installer, test, and release verification**

Run: `bash tests/install_scripts.sh`

Expected: exit 0 with no output after the installer/uninstaller status messages.

Run: `cargo fmt --check && cargo test -- --test-threads=1 && cargo clippy --all-targets -- -D warnings && cargo build --release`

Expected: all checks PASS and `target/release/codex-quota-ball` exists as an executable file.

Run: `file target/release/codex-quota-ball`

Expected: reports an x86-64 ELF executable.

- [ ] **Step 7: Perform the Ubuntu GNOME X11 acceptance pass**

Run: `./scripts/install.sh && ~/.local/bin/codex-quota-ball`

Expected: the compact transparent ball starts above normal windows, displays real quota after refresh, expands and collapses, remains on-screen at monitor edges, saves its position after dragging, and offers Refresh/Quit on right click.

Then log out and back in once.

Expected: exactly one Codex Quota Ball starts automatically at the saved position.

Run: `./scripts/uninstall.sh`

Expected: application, icon, menu entry, and autostart entry are removed; `~/.config/codex-quota-ball/config.json` remains.

- [ ] **Step 8: Commit installation and documentation**

```bash
git add assets packaging scripts tests/install_scripts.sh README.md
git commit -m "feat: add user scoped installation"
```

---

## Final Verification

- [ ] Run `cargo fmt --check` and expect exit 0.
- [ ] Run `cargo test -- --test-threads=1` and expect every unit and integration test to pass.
- [ ] Run `cargo clippy --all-targets -- -D warnings` and expect no warnings.
- [ ] Run `cargo build --release` and confirm `target/release/codex-quota-ball` exists.
- [ ] Run `bash tests/install_scripts.sh` and expect exit 0.
- [ ] Repeat the Task 6 X11 acceptance pass and record the installed Codex CLI version used for the successful quota read.
