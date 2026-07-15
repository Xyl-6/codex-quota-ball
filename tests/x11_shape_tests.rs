use codex_quota_ball::x11::rounded_input_rectangles;
use x11rb::protocol::xproto::Rectangle;

fn contains(rectangles: &[Rectangle], x: i32, y: i32) -> bool {
    rectangles.iter().any(|rect| {
        x >= i32::from(rect.x)
            && y >= i32::from(rect.y)
            && x < i32::from(rect.x) + i32::from(rect.width)
            && y < i32::from(rect.y) + i32::from(rect.height)
    })
}

fn extents(rectangles: &[Rectangle]) -> (i32, i32) {
    rectangles.iter().fold((0, 0), |(right, bottom), rect| {
        (
            right.max(i32::from(rect.x) + i32::from(rect.width)),
            bottom.max(i32::from(rect.y) + i32::from(rect.height)),
        )
    })
}

#[test]
fn compact_input_region_matches_the_circle_and_coalesces_rows() {
    let rectangles = rounded_input_rectangles(88.0, 88.0, 44.0, 1.0);
    assert!(!rectangles.is_empty());
    assert!(rectangles.len() < 88);
    assert_eq!(extents(&rectangles), (88, 88));
    assert!(!contains(&rectangles, 0, 0));
    assert!(contains(&rectangles, 44, 0));
    assert!(contains(&rectangles, 44, 44));
}

#[test]
fn expanded_input_region_uses_physical_hidpi_coordinates() {
    let rectangles = rounded_input_rectangles(290.0, 292.0, 18.0, 2.0);
    assert_eq!(extents(&rectangles), (580, 584));
    assert!(!contains(&rectangles, 0, 0));
    assert!(contains(&rectangles, 290, 292));
}

#[test]
fn input_region_never_exceeds_x11_rectangle_limits() {
    let rectangles = rounded_input_rectangles(40_000.0, 4.0, 2.0, 1.0);
    assert!(!rectangles.is_empty());
    assert!(rectangles.iter().all(|rect| {
        rect.x >= 0
            && rect.y >= 0
            && i32::from(rect.x) + i32::from(rect.width) <= i32::from(i16::MAX)
            && i32::from(rect.y) + i32::from(rect.height) <= i32::from(i16::MAX)
    }));
}
