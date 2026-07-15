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
