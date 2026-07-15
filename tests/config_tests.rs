use codex_quota_ball::config::{clamp_position, default_position, ConfigStore, Position};
use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

fn temp_path(name: &str) -> std::path::PathBuf {
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
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
fn saves_when_legacy_temporary_path_is_occupied() {
    let path = temp_path("temporary-collision");
    let legacy_temporary = path.with_extension("json.tmp");
    fs::create_dir(&legacy_temporary).unwrap();

    let store = ConfigStore::new(path.clone());
    store.save(Position { x: 321, y: 654 }).unwrap();

    assert_eq!(store.load(), Some(Position { x: 321, y: 654 }));
    let _ = fs::remove_file(path);
    let _ = fs::remove_dir(legacy_temporary);
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
    assert_eq!(
        clamp_position(Position { x: 2000, y: -50 }, 1920, 1080, 360, 260),
        Position { x: 1560, y: 0 }
    );
}
