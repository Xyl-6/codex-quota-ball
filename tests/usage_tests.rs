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
    assert_eq!(
        cells[181].date,
        NaiveDate::from_ymd_opt(2026, 7, 18).unwrap()
    );
    let labels = month_labels(&cells);
    assert_eq!(labels[0], (0, "1月".to_owned()));
    assert_eq!(labels[1], (2, "2月".to_owned()));
}

#[test]
fn calendar_crosses_years_and_contains_leap_day() {
    let today = NaiveDate::from_ymd_opt(2024, 3, 1).unwrap();
    let cells = heatmap_cells(today, &[]);
    assert!(cells
        .iter()
        .any(|cell| { cell.date == NaiveDate::from_ymd_opt(2024, 2, 29).unwrap() }));
    assert_eq!(cells[0].date.weekday(), chrono::Weekday::Sun);
}

#[test]
fn logarithmic_levels_are_monotonic_and_peak_at_four() {
    let levels = [0, 1, 10, 100, 1000].map(|tokens| token_level(tokens, 1000));
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
