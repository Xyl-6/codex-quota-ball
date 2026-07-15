use crate::config::Position;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use x11rb::connection::Connection;
use x11rb::protocol::randr::ConnectionExt as _;
use x11rb::protocol::shape::{ConnectionExt as _, SK, SO};
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, Rectangle, Window};
use x11rb::rust_connection::RustConnection;

pub fn rounded_input_rectangles(
    logical_width: f32,
    logical_height: f32,
    logical_radius: f32,
    pixels_per_point: f32,
) -> Vec<Rectangle> {
    let scale = if pixels_per_point.is_finite() && pixels_per_point > 0.0 {
        pixels_per_point
    } else {
        1.0
    };
    let physical = |value: f32| {
        (value.max(0.0) * scale)
            .round()
            .clamp(1.0, f32::from(i16::MAX)) as i32
    };
    let width = physical(logical_width);
    let height = physical(logical_height);
    let radius = physical(logical_radius).min(width.min(height) / 2);

    let mut rectangles: Vec<Rectangle> = Vec::new();
    for y in 0..height {
        let edge_distance = if y < radius {
            radius as f32 - (y as f32 + 0.5)
        } else if y >= height - radius {
            y as f32 + 0.5 - (height - radius) as f32
        } else {
            0.0
        };
        let inset = if edge_distance > 0.0 {
            let half_width = ((radius * radius) as f32 - edge_distance * edge_distance)
                .max(0.0)
                .sqrt();
            (radius as f32 - half_width - 0.5).ceil().max(0.0) as i32
        } else {
            0
        };
        let row_width = width.saturating_sub(inset.saturating_mul(2));
        if row_width <= 0 {
            continue;
        }
        if let Some(previous) = rectangles.last_mut() {
            if i32::from(previous.x) == inset
                && i32::from(previous.width) == row_width
                && i32::from(previous.y) + i32::from(previous.height) == y
                && previous.height < u16::MAX
            {
                previous.height += 1;
                continue;
            }
        }
        rectangles.push(Rectangle {
            x: inset as i16,
            y: y as i16,
            width: row_width as u16,
            height: 1,
        });
    }
    rectangles
}

fn same_rectangles(left: &[Rectangle], right: &[Rectangle]) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(left, right)| {
            left.x == right.x
                && left.y == right.y
                && left.width == right.width
                && left.height == right.height
        })
}

pub struct InputShaper {
    connection: RustConnection,
    window: Window,
    last_rectangles: Vec<Rectangle>,
}

#[derive(Debug)]
pub struct InputShapeError;

impl InputShaper {
    pub fn from_frame(frame: &eframe::Frame) -> Option<Self> {
        let raw = frame.window_handle().ok()?.as_raw();
        let window = match raw {
            RawWindowHandle::Xlib(handle) => u32::try_from(handle.window).ok()?,
            RawWindowHandle::Xcb(handle) => handle.window.get(),
            _ => return None,
        };
        let (connection, _) = x11rb::connect(None).ok()?;
        let version = connection.shape_query_version().ok()?.reply().ok()?;
        if version.major_version < 1 || (version.major_version == 1 && version.minor_version < 1) {
            return None;
        }
        Some(Self {
            connection,
            window,
            last_rectangles: Vec::new(),
        })
    }

    pub fn update(
        &mut self,
        logical_width: f32,
        logical_height: f32,
        logical_radius: f32,
        pixels_per_point: f32,
    ) -> Result<(), InputShapeError> {
        let rectangles = rounded_input_rectangles(
            logical_width,
            logical_height,
            logical_radius,
            pixels_per_point,
        );
        if same_rectangles(&self.last_rectangles, &rectangles) {
            return Ok(());
        }
        self.connection
            .shape_rectangles(
                SO::SET,
                SK::INPUT,
                x11rb::protocol::xproto::ClipOrdering::YX_BANDED,
                self.window,
                0,
                0,
                &rectangles,
            )
            .map_err(|_| InputShapeError)?
            .check()
            .map_err(|_| InputShapeError)?;
        self.connection.flush().map_err(|_| InputShapeError)?;
        self.last_rectangles = rectangles;
        Ok(())
    }
}

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

    fn intersection(self, other: Self) -> Option<Self> {
        let left = self.x.max(other.x);
        let top = self.y.max(other.y);
        let right = self
            .x
            .saturating_add(self.width)
            .min(other.x.saturating_add(other.width));
        let bottom = self
            .y
            .saturating_add(self.height)
            .min(other.y.saturating_add(other.height));
        (right > left && bottom > top).then_some(Self {
            x: left,
            y: top,
            width: right - left,
            height: bottom - top,
        })
    }
}

pub fn parse_workarea_rects(values: &[u32]) -> Option<Vec<Bounds>> {
    if values.is_empty() || !values.len().is_multiple_of(4) {
        return None;
    }
    values
        .chunks_exact(4)
        .map(|rect| {
            let width = i32::try_from(rect[2]).ok()?;
            let height = i32::try_from(rect[3]).ok()?;
            (width > 0 && height > 0).then_some(Bounds {
                x: rect[0] as i32,
                y: rect[1] as i32,
                width,
                height,
            })
        })
        .collect()
}

pub fn resolve_workareas(
    gtk_values: Option<&[u32]>,
    net_values: Option<&[u32]>,
    current_desktop: usize,
    monitors: &[Bounds],
) -> Vec<Bounds> {
    if let Some(workareas) = gtk_values.and_then(parse_workarea_rects) {
        return workareas;
    }

    let net_workarea = net_values
        .and_then(parse_workarea_rects)
        .and_then(|workareas| workareas.get(current_desktop).copied());
    if let Some(workarea) = net_workarea {
        let intersections: Vec<_> = monitors
            .iter()
            .filter_map(|monitor| monitor.intersection(workarea))
            .collect();
        if !intersections.is_empty() {
            return intersections;
        }
        return vec![workarea];
    }

    monitors.to_vec()
}

fn cardinal_property<C: Connection>(connection: &C, root: Window, name: &str) -> Option<Vec<u32>> {
    let atom = connection
        .intern_atom(false, name.as_bytes())
        .ok()?
        .reply()
        .ok()?
        .atom;
    let reply = connection
        .get_property(false, root, atom, AtomEnum::CARDINAL, 0, u32::MAX)
        .ok()?
        .reply()
        .ok()?;
    (reply.format == 32 && reply.type_ == u32::from(AtomEnum::CARDINAL))
        .then(|| reply.value32().map(Iterator::collect))
        .flatten()
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
    if bounds.is_empty() {
        return None;
    }

    let current_desktop = cardinal_property(&connection, root, "_NET_CURRENT_DESKTOP")
        .and_then(|values| values.first().copied())
        .and_then(|desktop| usize::try_from(desktop).ok())
        .unwrap_or(0);
    let gtk_name = format!("_GTK_WORKAREAS_D{current_desktop}");
    let gtk = cardinal_property(&connection, root, &gtk_name);
    let net = cardinal_property(&connection, root, "_NET_WORKAREA");
    let workareas = resolve_workareas(gtk.as_deref(), net.as_deref(), current_desktop, &bounds);
    let primary = primary.min(workareas.len().saturating_sub(1));
    Some((workareas, primary))
}
