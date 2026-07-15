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
pub enum RingTone {
    Green,
    Yellow,
    Red,
    Gray,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuotaParseError(&'static str);

impl fmt::Display for QuotaParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
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
        .and_then(DateTime::<Utc>::from_timestamp_secs)
        .map(|time| time.with_timezone(&Local).format("%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "不可用".to_owned())
}

pub fn parse_quota_response(value: &Value) -> Result<QuotaSnapshot, QuotaParseError> {
    let result = value
        .get("result")
        .ok_or(QuotaParseError("response has no result"))?;
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
