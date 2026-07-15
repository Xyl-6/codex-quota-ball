use codex_quota_ball::quota::{
    format_reset_time, parse_quota_response, remaining_percent, ring_tone, weekly_window,
    QuotaSnapshot, QuotaWindow, RingTone,
};
use serde_json::json;

#[test]
fn converts_used_to_remaining_and_clamps_bad_values() {
    assert_eq!(remaining_percent(i64::MIN), 100);
    assert_eq!(remaining_percent(-4), 100);
    assert_eq!(remaining_percent(0), 100);
    assert_eq!(remaining_percent(50), 50);
    assert_eq!(remaining_percent(80), 20);
    assert_eq!(remaining_percent(100), 0);
    assert_eq!(remaining_percent(140), 0);
}

#[test]
fn selects_exact_color_boundaries() {
    assert_eq!(ring_tone(Some(255)), RingTone::Green);
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
    assert_eq!(
        parse_quota_response(&response)
            .unwrap()
            .primary
            .unwrap()
            .remaining_percent,
        75
    );
}

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
    assert_eq!(
        weekly_window(&missing_durations).unwrap().remaining_percent,
        60
    );
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
    assert_eq!(
        weekly_window(&secondary_only).unwrap().remaining_percent,
        60
    );

    assert!(parse_quota_response(&json!({
        "result": {"rateLimits": {"primary": null, "secondary": null}}
    }))
    .is_err());
}

#[test]
fn invalid_timestamp_is_displayed_as_unavailable() {
    assert_eq!(format_reset_time(Some(i64::MAX)), "不可用");
    assert_eq!(format_reset_time(None), "不可用");
}
