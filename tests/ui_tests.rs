use codex_quota_ball::{
    config::Position,
    ui::{concise_error, ring_points, PositionSettleTracker},
    x11::{
        clamp_to_bounds, clamp_to_known_bounds, parse_workarea_rects, resolve_workareas,
        select_bounds, Bounds,
    },
};
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
fn monitor_bounds_support_negative_and_positive_origins() {
    let monitors = [
        Bounds {
            x: -1920,
            y: 0,
            width: 1920,
            height: 1080,
        },
        Bounds {
            x: 0,
            y: 0,
            width: 2560,
            height: 1440,
        },
        Bounds {
            x: 2560,
            y: 200,
            width: 1600,
            height: 900,
        },
    ];

    assert_eq!(
        select_bounds(&monitors, 1, Position { x: -100, y: 500 }),
        Some(monitors[0])
    );
    assert_eq!(
        clamp_to_bounds(Position { x: -2000, y: -20 }, monitors[0], 88, 88),
        Position { x: -1920, y: 0 }
    );
    assert_eq!(
        clamp_to_bounds(Position { x: 5000, y: 1200 }, monitors[2], 360, 260),
        Position { x: 3800, y: 840 }
    );
}

#[test]
fn monitor_selection_falls_back_to_primary() {
    let monitors = [
        Bounds {
            x: 100,
            y: 100,
            width: 800,
            height: 600,
        },
        Bounds {
            x: 900,
            y: 100,
            width: 1200,
            height: 900,
        },
    ];

    assert_eq!(
        select_bounds(&monitors, 1, Position { x: -500, y: -500 }),
        Some(monitors[1])
    );
    assert_eq!(
        select_bounds(&monitors, 99, Position { x: -500, y: -500 }),
        Some(monitors[0])
    );
    assert_eq!(select_bounds(&[], 0, Position { x: 0, y: 0 }), None);
}

#[test]
fn physical_monitor_bounds_convert_to_logical_points_before_clamping() {
    let primary = Bounds {
        x: 0,
        y: 0,
        width: 3840,
        height: 2160,
    }
    .to_logical(2.0);
    let left = Bounds {
        x: -3840,
        y: -200,
        width: 3840,
        height: 2160,
    }
    .to_logical(2.0);

    assert_eq!(
        primary,
        Bounds {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        }
    );
    assert_eq!(left.x, -1920);
    assert_eq!(left.y, -100);
    assert_eq!(
        clamp_to_bounds(Position { x: 1800, y: 1000 }, primary, 360, 260),
        Position { x: 1560, y: 820 }
    );
}

#[test]
fn monitor_scale_defaults_to_one_and_unknown_bounds_preserve_position() {
    let physical = Bounds {
        x: -1920,
        y: 0,
        width: 1920,
        height: 1080,
    };
    assert_eq!(physical.to_logical(0.0), physical);
    assert_eq!(physical.to_logical(f32::NAN), physical);

    let current = Position { x: -1600, y: 80 };
    assert_eq!(clamp_to_known_bounds(current, None, 360, 260), current);
}

#[test]
fn gtk_workareas_parse_signed_multi_monitor_rectangles() {
    let values = [(-1920_i32) as u32, 24, 1920, 1056, 0, 24, 2560, 1416];
    assert_eq!(
        parse_workarea_rects(&values),
        Some(vec![
            Bounds {
                x: -1920,
                y: 24,
                width: 1920,
                height: 1056,
            },
            Bounds {
                x: 0,
                y: 24,
                width: 2560,
                height: 1416,
            },
        ])
    );
    assert_eq!(parse_workarea_rects(&[0, 0, 100]), None);
    assert_eq!(parse_workarea_rects(&[0, 0, 0, 100]), None);
}

#[test]
fn workarea_resolution_prefers_gtk_then_intersects_ewmh_with_monitors() {
    let monitors = [
        Bounds {
            x: -1920,
            y: 0,
            width: 1920,
            height: 1080,
        },
        Bounds {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        },
    ];
    let gtk = [(-1920_i32) as u32, 30, 1920, 1050, 0, 30, 1920, 1050];
    let net = [(-1920_i32) as u32, 40, 3840, 1040];
    assert_eq!(
        resolve_workareas(Some(&gtk), Some(&net), 0, &monitors),
        parse_workarea_rects(&gtk).unwrap()
    );

    assert_eq!(
        resolve_workareas(None, Some(&net), 0, &monitors),
        vec![
            Bounds {
                x: -1920,
                y: 40,
                width: 1920,
                height: 1040,
            },
            Bounds {
                x: 0,
                y: 40,
                width: 1920,
                height: 1040,
            },
        ]
    );
}

#[test]
fn malformed_or_missing_workareas_fall_back_to_randr_monitors() {
    let monitors = [Bounds {
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
    }];
    assert_eq!(resolve_workareas(None, None, 0, &monitors), monitors);
    assert_eq!(
        resolve_workareas(Some(&[1, 2, 3]), Some(&[0, 0, 20]), 0, &monitors),
        monitors
    );
    let two_desktops = [0, 24, 1920, 1056, 0, 48, 1920, 1032];
    assert_eq!(
        resolve_workareas(None, Some(&two_desktops), 1, &monitors),
        vec![Bounds {
            x: 0,
            y: 48,
            width: 1920,
            height: 1032,
        }]
    );
}

#[test]
fn position_is_saved_once_only_after_movement_settles() {
    let mut tracker = PositionSettleTracker::default();
    tracker.start(Some(Position { x: 10, y: 10 }), 0);

    assert_eq!(tracker.observe(Position { x: 20, y: 10 }, 100), None);
    assert_eq!(tracker.observe(Position { x: 30, y: 10 }, 450), None);
    assert_eq!(tracker.observe(Position { x: 30, y: 10 }, 949), None);
    assert_eq!(
        tracker.observe(Position { x: 30, y: 10 }, 950),
        Some(Position { x: 30, y: 10 })
    );
    assert_eq!(tracker.observe(Position { x: 30, y: 10 }, 1_500), None);
}

#[test]
fn concise_error_is_unicode_safe_and_bounded() {
    assert_eq!(concise_error("网络错误，请稍后重试", 6), "网络错误，…");
    assert_eq!(concise_error("short", 10), "short");
    assert_eq!(concise_error("anything", 0), "");
    assert!(concise_error("éééééé", 4).chars().count() <= 4);
}
