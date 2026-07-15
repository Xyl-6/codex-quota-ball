use codex_quota_ball::{
    config::Position,
    morph::{
        compact_anchor_from_expanded, morph_placement, origin_for_size, reflow_expanded_drag,
        Growth, MorphAnimation, MorphPhase, COLLAPSE_MS, COMPACT_SIZE, EXPANDED_SIZE, EXPAND_MS,
    },
    x11::Bounds,
};

fn workarea() -> Bounds {
    Bounds {
        x: -1920,
        y: 24,
        width: 1920,
        height: 1056,
    }
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
fn animation_reverses_without_jumping_and_finishes_within_collapse_duration() {
    let mut animation = MorphAnimation::default();
    animation.set_expanded(true, 1000);
    let midpoint = 1000 + EXPAND_MS / 2;
    let before_reversal = animation.frame(midpoint);

    animation.set_expanded(false, midpoint);
    let after_reversal = animation.frame(midpoint);
    assert_eq!(after_reversal.size, before_reversal.size);
    assert_eq!(after_reversal.corner_radius, before_reversal.corner_radius);

    let before_deadline = animation.frame(midpoint + 157);
    assert!(before_deadline.animating);

    let collapsed = animation.frame(midpoint + 158);
    assert_eq!(collapsed.size, COMPACT_SIZE);
    assert!(!collapsed.animating);
}

#[test]
fn animation_uses_exact_easing_curves_and_reports_all_phases() {
    let mut animation = MorphAnimation::default();
    assert_eq!(animation.phase(), MorphPhase::Collapsed);

    animation.set_expanded(true, 1000);
    assert_eq!(animation.phase(), MorphPhase::Expanding);
    let expanding = animation.frame(1000 + EXPAND_MS / 4);
    assert!((expanding.progress - 0.578_125).abs() < 0.000_001);

    animation.frame(1000 + EXPAND_MS);
    assert_eq!(animation.phase(), MorphPhase::Expanded);
    animation.set_expanded(false, 2000);
    assert_eq!(animation.phase(), MorphPhase::Collapsing);
    let collapsing = animation.frame(2000 + COLLAPSE_MS / 4);
    assert!((collapsing.progress - 0.984_375).abs() < 0.000_001);

    animation.frame(2000 + COLLAPSE_MS);
    assert_eq!(animation.phase(), MorphPhase::Collapsed);
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
        Bounds {
            x: -1920,
            y: 24,
            width: 1920,
            height: 1056,
        },
        Bounds {
            x: 0,
            y: 24,
            width: 1920,
            height: 1056,
        },
    ];
    let moved =
        reflow_expanded_drag(Position { x: 1630, y: 700 }, Growth::LeftUp, &areas, 1).unwrap();
    assert_eq!(moved.growth, Growth::LeftUp);
    assert_eq!(moved.compact_anchor, Position { x: 1832, y: 904 });
    assert!(moved.expanded_origin.x + EXPANDED_SIZE.x as i32 <= 1920);
    assert!(moved.expanded_origin.y + EXPANDED_SIZE.y as i32 <= 1080);
}

#[test]
fn expanded_drag_prefers_monitor_containing_origin_before_primary_fallback() {
    let areas = [
        Bounds {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        },
        Bounds {
            x: 3000,
            y: 0,
            width: 1920,
            height: 1080,
        },
    ];

    let moved =
        reflow_expanded_drag(Position { x: 4800, y: 100 }, Growth::LeftDown, &areas, 0).unwrap();

    assert_eq!(moved.expanded_origin, Position { x: 4630, y: 100 });
    assert_eq!(moved.compact_anchor, Position { x: 4832, y: 100 });
    assert_eq!(moved.growth, Growth::LeftDown);
}
