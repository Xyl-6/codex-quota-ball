use crate::config::Position;
use x11rb::connection::Connection;
use x11rb::protocol::randr::ConnectionExt as _;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bounds {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

pub fn select_bounds(bounds: &[Bounds], primary: usize, point: Position) -> Option<Bounds> {
    bounds
        .iter()
        .copied()
        .find(|bounds| bounds.contains(point))
        .or_else(|| bounds.get(primary).copied())
        .or_else(|| bounds.first().copied())
}

pub fn clamp_to_bounds(
    position: Position,
    bounds: Bounds,
    window_width: i32,
    window_height: i32,
) -> Position {
    let max_x = bounds
        .x
        .saturating_add(bounds.width.saturating_sub(window_width).max(0));
    let max_y = bounds
        .y
        .saturating_add(bounds.height.saturating_sub(window_height).max(0));
    Position {
        x: position.x.clamp(bounds.x, max_x),
        y: position.y.clamp(bounds.y, max_y),
    }
}

pub fn clamp_to_known_bounds(
    position: Position,
    bounds: Option<Bounds>,
    window_width: i32,
    window_height: i32,
) -> Position {
    bounds.map_or(position, |bounds| {
        clamp_to_bounds(position, bounds, window_width, window_height)
    })
}

impl Bounds {
    pub fn to_logical(self, pixels_per_point: f32) -> Self {
        let scale = if pixels_per_point.is_finite() && pixels_per_point > 0.0 {
            pixels_per_point
        } else {
            1.0
        };
        let logical = |value: i32| (value as f64 / scale as f64).round() as i32;
        Self {
            x: logical(self.x),
            y: logical(self.y),
            width: logical(self.width).max(1),
            height: logical(self.height).max(1),
        }
    }

    fn contains(self, point: Position) -> bool {
        point.x >= self.x
            && point.y >= self.y
            && point.x < self.x.saturating_add(self.width)
            && point.y < self.y.saturating_add(self.height)
    }
}

pub fn query_monitor_bounds() -> Option<(Vec<Bounds>, usize)> {
    let (connection, screen_number) = x11rb::connect(None).ok()?;
    let root = connection.setup().roots.get(screen_number)?.root;
    let reply = connection
        .randr_get_monitors(root, true)
        .ok()?
        .reply()
        .ok()?;
    let mut primary = 0;
    let mut bounds = Vec::new();
    for monitor in reply.monitors {
        if monitor.width == 0 || monitor.height == 0 {
            continue;
        }
        if monitor.primary {
            primary = bounds.len();
        }
        bounds.push(Bounds {
            x: i32::from(monitor.x),
            y: i32::from(monitor.y),
            width: i32::from(monitor.width),
            height: i32::from(monitor.height),
        });
    }
    (!bounds.is_empty()).then_some((bounds, primary))
}
