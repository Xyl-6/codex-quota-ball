# Daily Token Heatmap and Morphing Card Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the split ball/card interface with a smooth 88×88 circle → 290×292 rounded-card morph that shows only Weekly limits and a 26-week calendar of exact daily token usage.

**Architecture:** Extend the existing single Codex app-server client with an independent `account/usage/read` request, keep Weekly and usage outcomes separate in the worker state, and derive all calendar cells from the server-provided daily buckets. Put pure date/color logic in `usage.rs` and pure animation/anchor geometry in `morph.rs`; keep `ui.rs` responsible for egui input and drawing only.

**Tech Stack:** Rust 2021, `serde_json`, `chrono`, `eframe`/`egui`, `x11rb`, Bash integration fixtures.

## Global Constraints

- Target Ubuntu GNOME X11; do not add Wayland behavior.
- Compact size is exactly 88×88 logical pixels; expanded size is exactly 290×292 logical pixels.
- Expansion is 220 ms ease-out; collapse is 180 ms ease-in; expanded corner radius is 18 pixels.
- Show only the longest available rate-limit window as `Weekly limits`.
- Use exact `account/usage/read` daily buckets; do not estimate missing usage and do not add a database.
- Render 26 week columns, Sunday–Saturday rows, month labels above, and Monday/Wednesday/Friday labels at left.
- Use four logarithmic green levels; zero-use past dates are gray, future dates are transparent, and today has a light outline.
- Weekly and usage refresh results must fail and become stale independently.
- Preserve the existing position JSON format, user-scoped installer, always-on-top behavior, timeout bounds, and no raw response/credential logging.
- Add no production dependency; reuse `chrono`, `serde_json`, `egui`, and `x11rb`.
- Follow red-green-refactor TDD and commit after every task.

---

## File Structure

- Create `src/usage.rs`: parse account daily-token responses, normalize duplicate/invalid buckets, construct 182 calendar positions, format tokens, and assign color levels.
- Create `src/morph.rs`: animation state, easing, interpolated size/radius/alpha, work-area growth direction, anchor restoration, and expanded-drag reflow.
- Modify `src/quota.rs`: select the longest available rate-limit window for Weekly display.
- Modify `src/codex.rs`: issue `account/usage/read` and expose whether a connection failure made the client terminal.
- Modify `src/worker.rs`: carry independent quota and usage results and stale states.
- Modify `src/ui.rs`: draw one animated surface, Weekly-only details, calendar grid, tooltips, and expanded drag behavior.
- Modify `src/lib.rs` and `src/main.rs`: export the new modules and use the compact size from `morph`.
- Modify `tests/fixtures/fake_codex.sh`: echo dynamic request IDs and model independent quota/usage failures.
- Create `tests/usage_tests.rs` and `tests/morph_tests.rs`; modify client, worker, quota, and UI tests.
- Modify `README.md`: document Weekly-only display, daily token data, hover behavior, animation, and protocol methods.

---

### Task 1: Daily Usage Domain and Weekly Window Selection

**Files:**
- Create: `src/usage.rs`
- Modify: `src/lib.rs:1-7`
- Modify: `src/quota.rs:52-95`
- Create: `tests/usage_tests.rs`
- Modify: `tests/quota_tests.rs`

**Interfaces:**
- Produces: `DailyUsage { date: NaiveDate, tokens: u64 }`
- Produces: `UsageSnapshot { daily: Option<Vec<DailyUsage>> }`; `None` means unavailable, `Some(vec![])` means valid empty history.
- Produces: `HeatCell { date: NaiveDate, tokens: u64, level: u8, future: bool }`
- Produces: `parse_usage_response(&Value) -> Result<UsageSnapshot, UsageParseError>`
- Produces: `heatmap_cells(NaiveDate, &[DailyUsage]) -> Vec<HeatCell>` returning exactly 182 positions.
- Produces: `token_level(u64, u64) -> u8`, `format_tokens(u64) -> String`, and `month_labels(&[HeatCell]) -> Vec<(usize, String)>`.
- Produces: `weekly_window(&QuotaSnapshot) -> Option<&QuotaWindow>`.

- [ ] **Step 1: Write failing usage and Weekly-selection tests**

Create `tests/usage_tests.rs` with these concrete cases:

```rust
use chrono::{Datelike, NaiveDate};
use codex_quota_ball::usage::{
    format_tokens, heatmap_cells, month_labels, parse_usage_response, token_level,
};
use serde_json::json;

#[test]
fn parses_daily_buckets_ignores_invalid_values_and_keeps_duplicate_maximum() {
    let parsed = parse_usage_response(&json!({
        "result": {
            "dailyUsageBuckets": [
                {"startDate":"2026-07-13","tokens":1200},
                {"startDate":"2026-07-13","tokens":900},
                {"startDate":"bad-date","tokens":500},
                {"startDate":"2026-07-14","tokens":-1}
            ],
            "summary": {}
        }
    }))
    .unwrap();
    let daily = parsed.daily.unwrap();
    assert_eq!(daily.len(), 1);
    assert_eq!(daily[0].date, NaiveDate::from_ymd_opt(2026, 7, 13).unwrap());
    assert_eq!(daily[0].tokens, 1200);
}

#[test]
fn distinguishes_unavailable_history_from_valid_empty_history() {
    let unavailable = parse_usage_response(&json!({
        "result":{"dailyUsageBuckets":null,"summary":{}}
    }))
    .unwrap();
    let empty = parse_usage_response(&json!({
        "result":{"dailyUsageBuckets":[],"summary":{}}
    }))
    .unwrap();
    assert_eq!(unavailable.daily, None);
    assert_eq!(empty.daily, Some(vec![]));
}

#[test]
fn builds_26_sunday_first_weeks_and_marks_future_positions() {
    let today = NaiveDate::from_ymd_opt(2026, 7, 15).unwrap();
    let cells = heatmap_cells(today, &[]);
    assert_eq!(cells.len(), 182);
    assert_eq!(cells[0].date, NaiveDate::from_ymd_opt(2026, 1, 18).unwrap());
    assert_eq!(cells[178].date, today);
    assert!(!cells[178].future);
    assert!(cells[179].future);
    assert_eq!(cells[181].date, NaiveDate::from_ymd_opt(2026, 7, 18).unwrap());
    let labels = month_labels(&cells);
    assert_eq!(labels[0], (0, "1月".to_owned()));
    assert_eq!(labels[1], (2, "2月".to_owned()));
}

#[test]
fn calendar_crosses_years_and_contains_leap_day() {
    let today = NaiveDate::from_ymd_opt(2024, 3, 1).unwrap();
    let cells = heatmap_cells(today, &[]);
    assert!(cells.iter().any(|cell| {
        cell.date == NaiveDate::from_ymd_opt(2024, 2, 29).unwrap()
    }));
    assert_eq!(cells[0].date.weekday(), chrono::Weekday::Sun);
}

#[test]
fn logarithmic_levels_are_monotonic_and_peak_at_four() {
    let levels = [0, 1, 10, 100, 1000]
        .map(|tokens| token_level(tokens, 1000));
    assert_eq!(levels[0], 0);
    assert!(levels.windows(2).all(|pair| pair[0] <= pair[1]));
    assert_eq!(levels[4], 4);
    assert_eq!(token_level(u64::MAX, u64::MAX), 4);
}

#[test]
fn formats_exact_token_counts_with_grouping() {
    assert_eq!(format_tokens(0), "0");
    assert_eq!(format_tokens(2_386_420), "2,386,420");
}
```

Add Weekly selection imports and tests to `tests/quota_tests.rs`, and replace the existing `permits_a_missing_secondary_but_rejects_missing_primary` test because secondary-only responses are now valid:

```rust
use codex_quota_ball::quota::{weekly_window, QuotaSnapshot, QuotaWindow};

#[test]
fn weekly_selection_uses_longest_window_and_prefers_secondary_on_ties() {
    let primary = QuotaWindow {
        remaining_percent: 80,
        resets_at: None,
        window_duration_mins: Some(300),
    };
    let secondary = QuotaWindow {
        remaining_percent: 55,
        resets_at: None,
        window_duration_mins: Some(10080),
    };
    let snapshot = QuotaSnapshot {
        primary: Some(primary),
        secondary: Some(secondary.clone()),
    };
    assert_eq!(weekly_window(&snapshot), Some(&secondary));

    let missing_durations = QuotaSnapshot {
        primary: Some(QuotaWindow {
            remaining_percent: 70,
            resets_at: None,
            window_duration_mins: None,
        }),
        secondary: Some(QuotaWindow {
            remaining_percent: 60,
            resets_at: None,
            window_duration_mins: None,
        }),
    };
    assert_eq!(weekly_window(&missing_durations).unwrap().remaining_percent, 60);
}

#[test]
fn quota_parser_accepts_either_single_window_but_rejects_no_windows() {
    let secondary_only = parse_quota_response(&json!({
        "result": {
            "rateLimits": {
                "primary": null,
                "secondary": {"usedPercent": 40, "windowDurationMins": 10080}
            }
        }
    }))
    .unwrap();
    assert_eq!(weekly_window(&secondary_only).unwrap().remaining_percent, 60);

    assert!(parse_quota_response(&json!({
        "result": {"rateLimits": {"primary": null, "secondary": null}}
    }))
    .is_err());
}
```

- [ ] **Step 2: Run the focused tests and confirm RED**

Run:

```bash
cargo test --locked --test usage_tests --test quota_tests -- --test-threads=1
```

Expected: compilation fails because `usage` and `weekly_window` do not exist.

- [ ] **Step 3: Implement the usage domain**

Add `pub mod usage;` to `src/lib.rs`, then create `src/usage.rs` with these types and functions:

```rust
use chrono::{Datelike, Duration, NaiveDate};
use serde_json::Value;
use std::{collections::BTreeMap, fmt};

pub const HEATMAP_WEEKS: usize = 26;
pub const HEATMAP_DAYS: usize = HEATMAP_WEEKS * 7;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DailyUsage {
    pub date: NaiveDate,
    pub tokens: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsageSnapshot {
    pub daily: Option<Vec<DailyUsage>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeatCell {
    pub date: NaiveDate,
    pub tokens: u64,
    pub level: u8,
    pub future: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsageParseError(&'static str);

impl fmt::Display for UsageParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for UsageParseError {}

pub fn parse_usage_response(value: &Value) -> Result<UsageSnapshot, UsageParseError> {
    let result = value
        .get("result")
        .ok_or(UsageParseError("response has no result"))?;
    let daily = match result.get("dailyUsageBuckets") {
        None | Some(Value::Null) => None,
        Some(Value::Array(items)) => {
            let mut dates = BTreeMap::<NaiveDate, u64>::new();
            for item in items {
                let Some(date) = item
                    .get("startDate")
                    .and_then(Value::as_str)
                    .and_then(|raw| NaiveDate::parse_from_str(raw, "%Y-%m-%d").ok())
                else {
                    continue;
                };
                let Some(tokens) = item.get("tokens").and_then(Value::as_i64) else {
                    continue;
                };
                let Ok(tokens) = u64::try_from(tokens) else {
                    continue;
                };
                dates
                    .entry(date)
                    .and_modify(|current| *current = (*current).max(tokens))
                    .or_insert(tokens);
            }
            Some(
                dates
                    .into_iter()
                    .map(|(date, tokens)| DailyUsage { date, tokens })
                    .collect(),
            )
        }
        Some(_) => return Err(UsageParseError("daily usage response is incompatible")),
    };
    Ok(UsageSnapshot { daily })
}

pub fn token_level(tokens: u64, peak: u64) -> u8 {
    if tokens == 0 || peak == 0 {
        return 0;
    }
    let score = (tokens as f64).ln_1p() / (peak as f64).ln_1p();
    (score * 4.0).ceil().clamp(1.0, 4.0) as u8
}

pub fn heatmap_cells(today: NaiveDate, daily: &[DailyUsage]) -> Vec<HeatCell> {
    let current_sunday = today - Duration::days(today.weekday().num_days_from_sunday() as i64);
    let start = current_sunday - Duration::weeks((HEATMAP_WEEKS - 1) as i64);
    let values: BTreeMap<_, _> = daily
        .iter()
        .filter(|item| item.date >= start && item.date <= today)
        .map(|item| (item.date, item.tokens))
        .collect();
    let peak = values.values().copied().max().unwrap_or(0);

    (0..HEATMAP_DAYS)
        .map(|offset| {
            let date = start + Duration::days(offset as i64);
            let future = date > today;
            let tokens = if future {
                0
            } else {
                values.get(&date).copied().unwrap_or(0)
            };
            HeatCell {
                date,
                tokens,
                level: token_level(tokens, peak),
                future,
            }
        })
        .collect()
}

pub fn format_tokens(tokens: u64) -> String {
    let digits = tokens.to_string();
    let mut output = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            output.push(',');
        }
        output.push(digit);
    }
    output
}

pub fn month_labels(cells: &[HeatCell]) -> Vec<(usize, String)> {
    let mut labels = Vec::new();
    let mut previous_month = None;
    for (week, cell) in cells.iter().step_by(7).enumerate() {
        let month = cell.date.month();
        if previous_month != Some(month) {
            labels.push((week, format!("{month}月")));
            previous_month = Some(month);
        }
    }
    labels
}
```

- [ ] **Step 4: Implement deterministic Weekly selection**

Add this function to `src/quota.rs`:

```rust
pub fn weekly_window(snapshot: &QuotaSnapshot) -> Option<&QuotaWindow> {
    match (&snapshot.primary, &snapshot.secondary) {
        (None, None) => None,
        (Some(primary), None) => Some(primary),
        (None, Some(secondary)) => Some(secondary),
        (Some(primary), Some(secondary)) => {
            let primary_duration = primary.window_duration_mins.unwrap_or(i64::MIN);
            let secondary_duration = secondary.window_duration_mins.unwrap_or(i64::MIN);
            if secondary_duration >= primary_duration {
                Some(secondary)
            } else {
                Some(primary)
            }
        }
    }
}
```

In `parse_quota_response`, replace the primary-only requirement with the exact no-window guard:

```rust
if wire.primary.is_none() && wire.secondary.is_none() {
    return Err(QuotaParseError("quota is unavailable"));
}
```

This makes a secondary-only Weekly response usable without accepting a response that has no quota window at all.

- [ ] **Step 5: Run focused and full tests**

Run:

```bash
cargo fmt --check
cargo test --locked --test usage_tests --test quota_tests -- --test-threads=1
cargo test --locked -- --test-threads=1
```

Expected: all usage, quota, and existing tests pass.

- [ ] **Step 6: Commit Task 1**

```bash
git add src/lib.rs src/usage.rs src/quota.rs tests/usage_tests.rs tests/quota_tests.rs
git commit -m "feat: model daily token usage"
```

---

### Task 2: App-server Daily Usage Request

**Files:**
- Modify: `src/codex.rs:1-6,167-186`
- Modify: `tests/fixtures/fake_codex.sh`
- Modify: `tests/codex_client_tests.rs`

**Interfaces:**
- Consumes: `parse_usage_response(&Value) -> Result<UsageSnapshot, UsageParseError>` from Task 1.
- Produces: `CodexClient::read_usage(&mut self) -> Result<UsageSnapshot, ClientError>`.
- Produces: `CodexClient::is_terminal(&self) -> bool` for worker reconnect decisions.
- Preserves: monotonically increasing request IDs and all current deadline behavior.

- [ ] **Step 1: Extend the fake server and write failing client tests**

Change the fixture so every response uses the request ID extracted from the incoming line instead of hard-coding `2`. Add an `account/usage/read` branch whose success response is:

```bash
id="$(sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p' <<<"$line")"
printf '%s\n' "{\"id\":$id,\"result\":{\"summary\":{},\"dailyUsageBuckets\":[{\"startDate\":\"2026-07-13\",\"tokens\":1200}]}}"
```

For `FAKE_SCENARIO=usage-error`, return the normal quota response and this usage error:

```bash
printf '%s\n' "{\"id\":$id,\"error\":{\"code\":-32601,\"message\":\"account/usage/read unavailable\"}}"
```

Append these tests to `tests/codex_client_tests.rs`:

```rust
#[test]
fn reads_daily_usage_after_quota_with_incrementing_ids() {
    let (_guard, command) = fake("success");
    let mut client = CodexClient::connect(command, Duration::from_secs(1)).unwrap();
    assert_eq!(client.read_quota().unwrap().primary.unwrap().remaining_percent, 72);
    let usage = client.read_usage().unwrap().daily.unwrap();
    assert_eq!(usage[0].tokens, 1200);
    assert!(!client.is_terminal());
}

#[test]
fn usage_method_error_does_not_poison_a_working_quota_connection() {
    let (_guard, command) = fake("usage-error");
    let mut client = CodexClient::connect(command, Duration::from_secs(1)).unwrap();
    assert!(client.read_quota().is_ok());
    assert!(matches!(client.read_usage(), Err(ClientError::Server(_))));
    assert!(!client.is_terminal());
    assert!(client.read_quota().is_ok());
}
```

- [ ] **Step 2: Run the client tests and confirm RED**

Run:

```bash
cargo test --locked --test codex_client_tests -- --test-threads=1
```

Expected: compilation fails because `read_usage` and `is_terminal` do not exist.

- [ ] **Step 3: Add a shared request helper and the usage method**

Import `parse_usage_response` and `UsageSnapshot`, then replace the duplicated quota request body with:

```rust
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
    parse_quota_response(&response).map_err(|error| {
        ClientError::Protocol(format!("{} ({})", error, self.version))
    })
}

pub fn read_usage(&mut self) -> Result<UsageSnapshot, ClientError> {
    let response = self.request("account/usage/read")?;
    parse_usage_response(&response).map_err(|error| {
        ClientError::Protocol(format!("{} ({})", error, self.version))
    })
}

pub fn is_terminal(&self) -> bool {
    self.terminal.is_some()
}
```

Response-shape incompatibility remains section-local so the other method can still succeed. Malformed JSON, write failures, timeouts, and closed pipes still call `fail` in the existing transport path and therefore remain terminal.

- [ ] **Step 4: Run client and full tests**

```bash
bash -n tests/fixtures/fake_codex.sh
cargo test --locked --test codex_client_tests -- --test-threads=1
cargo test --locked -- --test-threads=1
```

Expected: dynamic IDs work across repeated reads, usage errors remain non-terminal, and all tests pass.

- [ ] **Step 5: Commit Task 2**

```bash
git add src/codex.rs tests/codex_client_tests.rs tests/fixtures/fake_codex.sh
git commit -m "feat: read daily token usage"
```

---

### Task 3: Independent Worker Section State

**Files:**
- Modify: `src/worker.rs:1-108`
- Modify: `tests/worker_tests.rs`

**Interfaces:**
- Consumes: `CodexClient::read_quota`, `CodexClient::read_usage`, and `CodexClient::is_terminal`.
- Produces: `DashboardRead { quota: Result<QuotaSnapshot, String>, usage: Result<UsageSnapshot, String> }`.
- Produces: `SectionState<T> { value, stale, error, updated_at }`.
- Produces: `DashboardViewState { quota, usage, refreshing }` and `DashboardViewState::apply(WorkerEvent)`.

- [ ] **Step 1: Replace single-result worker tests with failing independent-result tests**

Add these state tests to `tests/worker_tests.rs`:

```rust
use codex_quota_ball::usage::UsageSnapshot;
use codex_quota_ball::worker::{DashboardRead, DashboardViewState};

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
fn usage_failure_keeps_usage_stale_but_updates_quota() {
    let mut state = DashboardViewState::default();
    state.apply(WorkerEvent::Started);
    state.apply(WorkerEvent::Finished(DashboardRead {
        quota: Ok(sample_quota()),
        usage: Ok(UsageSnapshot { daily: Some(vec![]) }),
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
        usage: Ok(UsageSnapshot { daily: Some(vec![]) }),
    }));
    assert!(state.quota.value.is_none());
    assert!(state.quota.error.is_some());
    assert!(state.usage.value.is_some());
    assert!(!state.usage.stale);
    assert!(!state.refreshing);
}
```

Refactor existing immediate, interval, coalescing, and reconnect assertions to match `WorkerEvent::Finished(DashboardRead { .. })`. Add a fixture-backed `usage-error` test that expects `quota.is_ok()` and `usage.is_err()` in the same event.

- [ ] **Step 2: Run worker tests and confirm RED**

```bash
cargo test --locked --test worker_tests -- --test-threads=1
```

Expected: compilation fails because the dashboard result and section state types do not exist.

- [ ] **Step 3: Implement the independent state model**

Replace the single snapshot state with:

```rust
#[derive(Debug)]
pub struct DashboardRead {
    pub quota: Result<QuotaSnapshot, String>,
    pub usage: Result<UsageSnapshot, String>,
}

#[derive(Debug)]
pub enum WorkerEvent {
    Started,
    Finished(DashboardRead),
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
```

- [ ] **Step 4: Read both methods per refresh and reconnect only terminal clients**

Replace the worker loop's single `result` closure with this borrow-safe explicit flow:

```rust
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
        let active = client.as_mut().expect("client exists after successful connect");
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
```

Keep the existing capacity-one command channel and `recv_timeout(interval)` scheduling unchanged.

- [ ] **Step 5: Run worker and full tests**

```bash
cargo test --locked --test worker_tests -- --test-threads=1
cargo test --locked -- --test-threads=1
cargo clippy --locked --all-targets -- -D warnings
```

Expected: partial results, stale retention, immediate/periodic/manual refresh, reconnect behavior, and all existing tests pass.

- [ ] **Step 6: Commit Task 3**

```bash
git add src/worker.rs tests/worker_tests.rs
git commit -m "feat: refresh dashboard sections independently"
```

---

### Task 4: Pure Morph Animation and Anchor Geometry

**Files:**
- Create: `src/morph.rs`
- Modify: `src/lib.rs`
- Create: `tests/morph_tests.rs`
- Modify: `tests/ui_tests.rs` to remove tests for the superseded side-by-side `ExpandedLayout` API.

**Interfaces:**
- Produces constants `COMPACT_SIZE`, `EXPANDED_SIZE`, `EXPAND_MS`, `COLLAPSE_MS`.
- Produces `MorphAnimation::set_expanded(bool, u64)` and `MorphAnimation::frame(u64) -> MorphFrame`.
- Produces `MorphPlacement`, `morph_placement`, `origin_for_size`, `compact_anchor_from_expanded`, and `reflow_expanded_drag`.
- Consumes existing `Position`, `Bounds`, `select_bounds`, and `clamp_to_known_bounds`.

- [ ] **Step 1: Write failing animation and geometry tests**

Create `tests/morph_tests.rs` with these required behaviors:

```rust
use codex_quota_ball::{
    config::Position,
    morph::{
        compact_anchor_from_expanded, morph_placement, origin_for_size,
        reflow_expanded_drag, Growth, MorphAnimation, COLLAPSE_MS, COMPACT_SIZE,
        EXPANDED_SIZE, EXPAND_MS,
    },
    x11::Bounds,
};

fn workarea() -> Bounds {
    Bounds { x: -1920, y: 24, width: 1920, height: 1056 }
}

#[test]
fn animation_has_exact_endpoints_duration_radius_and_alpha() {
    let mut animation = MorphAnimation::default();
    animation.set_expanded(true, 1000);
    let start = animation.frame(1000);
    assert_eq!(start.size, COMPACT_SIZE);
    assert_eq!(start.corner_radius, 44.0);
    assert!(start.compact_alpha > start.content_alpha);

    let end = animation.frame(1000 + EXPAND_MS);
    assert_eq!(end.size, EXPANDED_SIZE);
    assert_eq!(end.corner_radius, 18.0);
    assert!(!end.animating);
    assert!(end.content_alpha > end.compact_alpha);

    animation.set_expanded(false, 2000);
    assert_eq!(animation.frame(2000 + COLLAPSE_MS).size, COMPACT_SIZE);
}

#[test]
fn morph_grows_inward_at_all_workarea_corners_and_restores_anchor() {
    let bounds = workarea();
    for anchor in [
        Position { x: -1920, y: 24 },
        Position { x: -88, y: 24 },
        Position { x: -1920, y: 992 },
        Position { x: -88, y: 992 },
    ] {
        let placement = morph_placement(anchor, bounds);
        let compact_origin = origin_for_size(&placement, COMPACT_SIZE);
        let expanded_origin = origin_for_size(&placement, EXPANDED_SIZE);
        assert_eq!(compact_origin, anchor);
        assert!(expanded_origin.x >= bounds.x);
        assert!(expanded_origin.y >= bounds.y);
        assert!(expanded_origin.x + EXPANDED_SIZE.x as i32 <= bounds.x + bounds.width);
        assert!(expanded_origin.y + EXPANDED_SIZE.y as i32 <= bounds.y + bounds.height);
        assert_eq!(
            compact_anchor_from_expanded(expanded_origin, placement.growth, bounds),
            anchor
        );
    }
}

#[test]
fn expanded_drag_crosses_monitors_and_preserves_growth_direction() {
    let areas = [
        Bounds { x: -1920, y: 24, width: 1920, height: 1056 },
        Bounds { x: 0, y: 24, width: 1920, height: 1056 },
    ];
    let moved = reflow_expanded_drag(
        Position { x: 1630, y: 700 },
        Growth::LeftUp,
        &areas,
        1,
    )
    .unwrap();
    assert_eq!(moved.compact_anchor, Position { x: 1832, y: 904 });
    assert!(moved.expanded_origin.x + EXPANDED_SIZE.x as i32 <= 1920);
    assert!(moved.expanded_origin.y + EXPANDED_SIZE.y as i32 <= 1080);
}
```

Also test a reversal halfway through expansion: call `set_expanded(false, midpoint)` and assert the next frame starts at the same size without a jump, then reaches compact size in no more than `COLLAPSE_MS`.

- [ ] **Step 2: Run morph tests and confirm RED**

```bash
cargo test --locked --test morph_tests -- --test-threads=1
```

Expected: compilation fails because `morph` does not exist.

- [ ] **Step 3: Implement the animation state**

Add `pub mod morph;` to `src/lib.rs`. In `src/morph.rs`, define:

```rust
use crate::{
    config::Position,
    x11::{clamp_to_known_bounds, select_bounds, Bounds},
};
use eframe::egui;

pub const COMPACT_SIZE: egui::Vec2 = egui::vec2(88.0, 88.0);
pub const EXPANDED_SIZE: egui::Vec2 = egui::vec2(290.0, 292.0);
pub const EXPAND_MS: u64 = 220;
pub const COLLAPSE_MS: u64 = 180;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Growth {
    RightDown,
    RightUp,
    LeftDown,
    LeftUp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MorphPhase {
    Collapsed,
    Expanding,
    Expanded,
    Collapsing,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MorphFrame {
    pub progress: f32,
    pub size: egui::Vec2,
    pub corner_radius: f32,
    pub compact_alpha: f32,
    pub content_alpha: f32,
    pub animating: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct MorphAnimation {
    progress: f32,
    start_progress: f32,
    target: f32,
    started_ms: u64,
}

impl Default for MorphAnimation {
    fn default() -> Self {
        Self { progress: 0.0, start_progress: 0.0, target: 0.0, started_ms: 0 }
    }
}

impl MorphAnimation {
    fn sampled_progress(&self, now_ms: u64) -> f32 {
        if self.progress == self.target {
            return self.target;
        }
        let full_duration = if self.target > self.start_progress {
            EXPAND_MS
        } else {
            COLLAPSE_MS
        };
        let distance = (self.target - self.start_progress).abs();
        let duration = (full_duration as f32 * distance).max(1.0);
        let t = (now_ms.saturating_sub(self.started_ms) as f32 / duration).clamp(0.0, 1.0);
        let eased = if self.target > self.start_progress {
            1.0 - (1.0 - t).powi(3)
        } else {
            t.powi(3)
        };
        self.start_progress + (self.target - self.start_progress) * eased
    }

    pub fn set_expanded(&mut self, expanded: bool, now_ms: u64) {
        self.progress = self.sampled_progress(now_ms);
        self.start_progress = self.progress;
        self.target = if expanded { 1.0 } else { 0.0 };
        self.started_ms = now_ms;
    }

    pub fn frame(&mut self, now_ms: u64) -> MorphFrame {
        self.progress = self.sampled_progress(now_ms);
        if (self.progress - self.target).abs() < f32::EPSILON {
            self.progress = self.target;
            self.start_progress = self.target;
        }
        let progress = self.progress;
        MorphFrame {
            progress,
            size: egui::vec2(
                egui::lerp(COMPACT_SIZE.x..=EXPANDED_SIZE.x, progress),
                egui::lerp(COMPACT_SIZE.y..=EXPANDED_SIZE.y, progress),
            ),
            corner_radius: egui::lerp(44.0..=18.0, progress),
            compact_alpha: (1.0 - progress / 0.45).clamp(0.0, 1.0),
            content_alpha: ((progress - 0.35) / 0.65).clamp(0.0, 1.0),
            animating: progress != self.target,
        }
    }

    pub fn target_expanded(&self) -> bool {
        self.target == 1.0
    }

    pub fn phase(&self) -> MorphPhase {
        match (self.progress, self.target) {
            (0.0, 0.0) => MorphPhase::Collapsed,
            (1.0, 1.0) => MorphPhase::Expanded,
            (_, 1.0) => MorphPhase::Expanding,
            _ => MorphPhase::Collapsing,
        }
    }
}
```

The implementation may extract a private `lerp` helper if the installed egui version does not accept inclusive ranges; keep the public interface unchanged.

- [ ] **Step 4: Implement placement, inverse anchoring, and drag reflow**

Define `MorphPlacement` with `compact_anchor`, `expanded_origin`, and `growth`. Select horizontal and vertical growth independently by checking whether the exact expanded dimension fits from the compact anchor; when both directions fit, prefer right and down. Use the following complete geometry implementation:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MorphPlacement {
    pub compact_anchor: Position,
    pub expanded_origin: Position,
    pub growth: Growth,
}

fn grows_left(growth: Growth) -> bool {
    matches!(growth, Growth::LeftDown | Growth::LeftUp)
}

fn grows_up(growth: Growth) -> bool {
    matches!(growth, Growth::RightUp | Growth::LeftUp)
}

fn raw_anchor_from_expanded(origin: Position, growth: Growth) -> Position {
    Position {
        x: if grows_left(growth) {
            origin.x + EXPANDED_SIZE.x as i32 - COMPACT_SIZE.x as i32
        } else {
            origin.x
        },
        y: if grows_up(growth) {
            origin.y + EXPANDED_SIZE.y as i32 - COMPACT_SIZE.y as i32
        } else {
            origin.y
        },
    }
}

fn desired_expanded_origin(anchor: Position, growth: Growth) -> Position {
    Position {
        x: if grows_left(growth) {
            anchor.x + COMPACT_SIZE.x as i32 - EXPANDED_SIZE.x as i32
        } else {
            anchor.x
        },
        y: if grows_up(growth) {
            anchor.y + COMPACT_SIZE.y as i32 - EXPANDED_SIZE.y as i32
        } else {
            anchor.y
        },
    }
}

pub fn morph_placement(anchor: Position, workarea: Bounds) -> MorphPlacement {
    let compact_anchor = clamp_to_known_bounds(
        anchor,
        Some(workarea),
        COMPACT_SIZE.x as i32,
        COMPACT_SIZE.y as i32,
    );
    let right = workarea.x.saturating_add(workarea.width);
    let bottom = workarea.y.saturating_add(workarea.height);
    let fits_right = compact_anchor.x + EXPANDED_SIZE.x as i32 <= right;
    let fits_left = compact_anchor.x + COMPACT_SIZE.x as i32
        - EXPANDED_SIZE.x as i32
        >= workarea.x;
    let fits_down = compact_anchor.y + EXPANDED_SIZE.y as i32 <= bottom;
    let fits_up = compact_anchor.y + COMPACT_SIZE.y as i32
        - EXPANDED_SIZE.y as i32
        >= workarea.y;

    let left = if fits_right {
        false
    } else if fits_left {
        true
    } else {
        compact_anchor.x + COMPACT_SIZE.x as i32 - workarea.x
            > right - compact_anchor.x
    };
    let up = if fits_down {
        false
    } else if fits_up {
        true
    } else {
        compact_anchor.y + COMPACT_SIZE.y as i32 - workarea.y
            > bottom - compact_anchor.y
    };
    let growth = match (left, up) {
        (false, false) => Growth::RightDown,
        (false, true) => Growth::RightUp,
        (true, false) => Growth::LeftDown,
        (true, true) => Growth::LeftUp,
    };
    let desired = desired_expanded_origin(compact_anchor, growth);
    let expanded_origin = clamp_to_known_bounds(
        desired,
        Some(workarea),
        EXPANDED_SIZE.x as i32,
        EXPANDED_SIZE.y as i32,
    );
    MorphPlacement { compact_anchor, expanded_origin, growth }
}

pub fn origin_for_size(placement: &MorphPlacement, size: egui::Vec2) -> Position {
    let progress = ((size.x - COMPACT_SIZE.x) / (EXPANDED_SIZE.x - COMPACT_SIZE.x))
        .clamp(0.0, 1.0);
    Position {
        x: (placement.compact_anchor.x as f32
            + (placement.expanded_origin.x - placement.compact_anchor.x) as f32 * progress)
            .round() as i32,
        y: (placement.compact_anchor.y as f32
            + (placement.expanded_origin.y - placement.compact_anchor.y) as f32 * progress)
            .round() as i32,
    }
}

pub fn compact_anchor_from_expanded(
    expanded_origin: Position,
    growth: Growth,
    workarea: Bounds,
) -> Position {
    clamp_to_known_bounds(
        raw_anchor_from_expanded(expanded_origin, growth),
        Some(workarea),
        COMPACT_SIZE.x as i32,
        COMPACT_SIZE.y as i32,
    )
}

pub fn reflow_expanded_drag(
    expanded_origin: Position,
    growth: Growth,
    workareas: &[Bounds],
    primary_monitor: usize,
) -> Option<MorphPlacement> {
    let raw_anchor = raw_anchor_from_expanded(expanded_origin, growth);
    let workarea = select_bounds(workareas, primary_monitor, raw_anchor)
        .or_else(|| select_bounds(workareas, primary_monitor, expanded_origin))?;
    let expanded_origin = clamp_to_known_bounds(
        expanded_origin,
        Some(workarea),
        EXPANDED_SIZE.x as i32,
        EXPANDED_SIZE.y as i32,
    );
    let compact_anchor = compact_anchor_from_expanded(expanded_origin, growth, workarea);
    Some(MorphPlacement { compact_anchor, expanded_origin, growth })
}
```

Do not retain `ExpandedLayout`, `ball_offset`, `card_origin`, or `CARD_WIDTH`; the final surface is unified.

- [ ] **Step 5: Run focused and full geometry tests**

```bash
cargo fmt --check
cargo test --locked --test morph_tests --test ui_tests -- --test-threads=1
cargo test --locked -- --test-threads=1
```

Expected: exact animation endpoints, reversals, negative-origin corners, cross-monitor drag, and all retained UI/X11 geometry tests pass.

- [ ] **Step 6: Commit Task 4**

```bash
git add src/lib.rs src/morph.rs tests/morph_tests.rs tests/ui_tests.rs
git commit -m "feat: model morphing window geometry"
```

---

### Task 5: Unified Weekly and Token Heatmap UI

**Files:**
- Modify: `src/ui.rs:1-594`
- Modify: `src/main.rs:1-20`
- Modify: `tests/ui_tests.rs`

**Interfaces:**
- Consumes: `DashboardViewState`, `weekly_window`, `heatmap_cells`, `format_tokens`, `MorphAnimation`, and `MorphPlacement`.
- Produces: `heat_cell_rect(origin: Pos2, index: usize) -> Rect` for deterministic week-column/day-row layout.
- Produces: `heat_tooltip(&HeatCell) -> String` so exact hover copy is unit tested.
- Preserves: position settle tracking, X11 work-area selection, right-click Refresh/Quit, manual refresh, CJK fonts, and transparent always-on-top viewport.

- [ ] **Step 1: Write failing UI layout and interaction-state tests**

Replace obsolete side-by-side-card assertions in `tests/ui_tests.rs` and add:

```rust
use chrono::NaiveDate;
use codex_quota_ball::{
    ui::{heat_cell_rect, heat_tooltip, should_collapse},
    usage::HeatCell,
};

#[test]
fn heat_cells_advance_days_vertically_and_weeks_horizontally() {
    let origin = egui::pos2(20.0, 40.0);
    assert_eq!(heat_cell_rect(origin, 0).min, egui::pos2(20.0, 40.0));
    assert_eq!(heat_cell_rect(origin, 1).min, egui::pos2(20.0, 49.0));
    assert_eq!(heat_cell_rect(origin, 7).min, egui::pos2(29.0, 40.0));
    assert_eq!(heat_cell_rect(origin, 181).size(), egui::vec2(7.0, 7.0));
}

#[test]
fn escape_or_focus_loss_collapses_only_an_open_target() {
    assert!(should_collapse(true, true, false));
    assert!(should_collapse(true, false, true));
    assert!(!should_collapse(false, true, true));
    assert!(!should_collapse(true, false, false));
}

#[test]
fn token_tooltip_contains_exact_date_and_grouped_count() {
    let cell = HeatCell {
        date: NaiveDate::from_ymd_opt(2026, 7, 15).unwrap(),
        tokens: 2_386_420,
        level: 3,
        future: false,
    };
    assert_eq!(heat_tooltip(&cell), "2026-07-15\n使用 2,386,420 tokens");
}
```

Keep tests for ring geometry, Unicode error truncation, position settling, monitor scaling, work-area parsing, and bounds selection.

- [ ] **Step 2: Run UI tests and confirm RED**

```bash
cargo test --locked --test ui_tests -- --test-threads=1
```

Expected: compilation fails because the new helpers do not exist and old expanded-layout APIs have been removed.

- [ ] **Step 3: Replace the boolean expanded state with morph state**

Update `FloatingApp` to hold:

```rust
state: DashboardViewState,
compact_anchor: Option<Position>,
morph: MorphAnimation,
placement: Option<MorphPlacement>,
positioned: bool,
monitor_bounds: Vec<Bounds>,
primary_monitor: usize,
position_tracker: PositionSettleTracker,
started_at: Instant,
```

Add a `begin_transition(&mut self, ctx: &egui::Context, expanded: bool)` method. On expansion, clamp the observed compact anchor, select a work area, store `morph_placement`, start the animation, and request a refresh. On collapse, commit any active expanded drag before setting the compact target. Ignore duplicate target requests.

On every update frame:

```rust
let now_ms = self.started_at.elapsed().as_millis() as u64;
let frame = self.morph.frame(now_ms);
if let Some(placement) = self.placement {
    let origin = origin_for_size(&placement, frame.size);
    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(frame.size));
    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
        origin.x as f32,
        origin.y as f32,
    )));
}
if frame.animating {
    ctx.request_repaint();
} else {
    ctx.request_repaint_after(std::time::Duration::from_millis(250));
}
```

When collapse reaches progress `0.0`, clear `placement` after the viewport has returned to the saved compact anchor. Update `src/main.rs` to import `morph::COMPACT_SIZE` and use it as the initial viewport size.

- [ ] **Step 4: Draw one background and Weekly-only content**

Replace `paint_ball` plus `expanded_card` with one central surface:

- Paint a rounded rectangle covering the current viewport using RGB `(30, 41, 59)` and `frame.corner_radius`; do not create a separately styled title frame.
- Multiply ball/ring colors and percentage text by `frame.compact_alpha`.
- Put all card text and controls in one transparent child UI and multiply its visuals by `frame.content_alpha`.
- Read the compact percentage from `state.quota.value.as_ref().and_then(weekly_window)`.
- Render one row labeled `Weekly limits`, its progress bar, and reset time. Remove short-cycle and primary/secondary UI labels.
- Show quota and usage errors independently and keep the existing concise Retry action.

The title and body must share the same fill. No `CentralPanel` or nested `Frame` may introduce a light default background; explicitly use transparent frames inside the single painted surface.

- [ ] **Step 5: Draw the calendar, month labels, weekday labels, and tooltips**

Add these public constants/helpers near the top of `src/ui.rs`:

```rust
pub const HEAT_CELL: f32 = 7.0;
pub const HEAT_GAP: f32 = 2.0;

pub fn heat_cell_rect(origin: egui::Pos2, index: usize) -> egui::Rect {
    let week = index / 7;
    let day = index % 7;
    egui::Rect::from_min_size(
        origin + egui::vec2(
            week as f32 * (HEAT_CELL + HEAT_GAP),
            day as f32 * (HEAT_CELL + HEAT_GAP),
        ),
        egui::vec2(HEAT_CELL, HEAT_CELL),
    )
}

pub fn should_collapse(target_expanded: bool, escape: bool, focus_lost: bool) -> bool {
    target_expanded && (escape || focus_lost)
}

pub fn heat_tooltip(cell: &HeatCell) -> String {
    format!(
        "{}\n使用 {} tokens",
        cell.date.format("%Y-%m-%d"),
        format_tokens(cell.tokens)
    )
}
```

Use `Local::now().date_naive()` and `heatmap_cells` only when `state.usage.value.as_ref().and_then(|snapshot| snapshot.daily.as_ref())` is `Some`. Use `month_labels(&cells)` for the top labels. For each non-future cell, paint:

```rust
let color = match cell.level {
    0 => egui::Color32::from_rgb(51, 65, 85),
    1 => egui::Color32::from_rgb(20, 83, 45),
    2 => egui::Color32::from_rgb(21, 128, 61),
    3 => egui::Color32::from_rgb(34, 197, 94),
    _ => egui::Color32::from_rgb(134, 239, 172),
};
```

Allocate a hover interaction for each painted rect and call `on_hover_text(heat_tooltip(cell))`.

Paint a one-pixel light outline around today's cell. Leave future cells transparent and non-interactive. Draw month text when a visible column's first date has a different month from the preceding column. Draw `一`, `三`, and `五` beside row indices 1, 3, and 5. When history is `None`, show `Token 历史不可用`; when it is `Some(vec![])`, call `heatmap_cells` and render a valid gray calendar.

- [ ] **Step 6: Wire interaction and expanded dragging**

- Compact, settled state: clicking toggles expansion; dragging sends `ViewportCommand::StartDrag`; right click shows Refresh/Quit.
- Animating state: use `Sense::hover()` for the surface so no drag or second transition starts.
- Expanded, settled state: interactive buttons and heat cells consume their own responses. A drag response on non-interactive title/background sends `StartDrag` and starts the existing settle tracker.
- On expanded settle or pre-collapse commit, call `reflow_expanded_drag`, update `placement`, save the returned compact anchor, and correct the expanded viewport position when needed.
- Detect Escape with `input.key_pressed(egui::Key::Escape)` and focus loss with `input.viewport().focused == Some(false)`; call `begin_transition(ctx, false)` when `should_collapse` returns true.

- [ ] **Step 7: Run UI, integration, lint, and build checks**

```bash
cargo fmt --check
cargo test --locked --test ui_tests --test morph_tests --test worker_tests -- --test-threads=1
cargo test --locked -- --test-threads=1
cargo clippy --locked --all-targets -- -D warnings
cargo build --locked --release
git diff --check
```

Expected: all tests pass, Clippy emits no warnings, and the optimized X11 binary builds.

- [ ] **Step 8: Commit Task 5**

```bash
git add src/ui.rs src/main.rs tests/ui_tests.rs
git commit -m "feat: add morphing token heatmap interface"
```

---

### Task 6: Documentation and End-to-End Acceptance

**Files:**
- Modify: `README.md:27-40`
- Verify: `scripts/install.sh`
- Verify: `tests/install_scripts.sh`

**Interfaces:**
- Documents the exact user-visible behavior and the two experimental read-only app-server methods.
- Does not change installation artifacts or configuration paths.

- [ ] **Step 1: Update README behavior and privacy text**

Replace the Use bullets and protocol paragraph with text covering:

```markdown
- Left click: smoothly expand the quota circle into the Weekly and token-history card.
- Click another window or press Escape: collapse back to the saved circle position.
- Drag the circle, or drag non-interactive card background: move and remember the position.
- Hover a heatmap cell: show its date and exact daily token count.
- Right click the circle: refresh or quit.
- Green/yellow/red indicate Weekly remaining quota; the heatmap uses relative logarithmic green levels for the latest 26 weeks.

Codex app-server is an experimental local protocol. This application reads only `account/rateLimits/read` and `account/usage/read`, stores no credentials, prompts, source code, or local token database, and may require compatibility updates when Codex changes.
```

Add troubleshooting text stating that `Token 历史不可用` can mean the installed Codex version does not expose `account/usage/read`; recommend checking `codex --version` and updating Codex.

- [ ] **Step 2: Run the isolated installer and static checks**

```bash
bash tests/install_scripts.sh
bash -n scripts/install.sh scripts/uninstall.sh tests/install_scripts.sh
git diff --check
```

Expected: installation into the temporary HOME, desktop-file validation, `gio launch`, uninstall, configuration retention, shell syntax, and whitespace checks pass.

- [ ] **Step 3: Run the final automated verification from a clean process**

```bash
cargo fmt --check
cargo test --locked -- --test-threads=1
cargo clippy --locked --all-targets -- -D warnings
cargo build --locked --release
test -x target/release/codex-quota-ball
git status --short
```

Expected: all test targets pass with zero failures, Clippy has zero warnings, the release binary is executable, and only the intended README change remains before the task commit.

- [ ] **Step 4: Run real GNOME X11 smoke acceptance**

With user approval to access the display, run:

```bash
timeout 10s target/release/codex-quota-ball
```

Expected automated evidence: the process survives until timeout without an X11 initialization or app-server crash. Manually confirm during the 10 seconds:

- the circle opens into one dark rounded rectangle without a white title strip;
- the transition is smooth in both directions;
- only Weekly limits appears;
- the 26-week grid has separate cells, month labels, weekday labels, and hover token details;
- clicking another window and Escape each collapse to the original anchor;
- an edge-positioned circle expands fully inside the GNOME work area.

Record any visual failure as incomplete work; do not claim it passed based only on process survival.

- [ ] **Step 5: Commit Task 6**

```bash
git add README.md
git commit -m "docs: describe token heatmap usage"
```

- [ ] **Step 6: Request final whole-branch review**

Review the complete range from the plan execution base through `HEAD` against `docs/superpowers/specs/2026-07-15-token-heatmap-morph-design.md`. Fix every Critical and Important finding, rerun the full verification commands, and re-review until approved. Record the three previously deferred non-blocking observations only if still present: version-probe descendant cleanup, stale partial GTK work-area arrays, and experimental protocol compatibility.
