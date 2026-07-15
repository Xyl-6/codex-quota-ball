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
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
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
