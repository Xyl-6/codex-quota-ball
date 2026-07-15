use crate::config::Position;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use x11rb::connection::Connection;
use x11rb::protocol::randr::ConnectionExt as _;
use x11rb::protocol::shape::{ConnectionExt as _, SK, SO};
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, PixmapEnum, Rectangle, Window};
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
    connection: Option<RustConnection>,
    window: Window,
    last_rectangles: Option<Vec<Rectangle>>,
    policy: InputShapePolicy,
}

pub const INPUT_SHAPE_RETRY_MS: u64 = 1_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputShapeAction {
    Connect,
    Update,
    Reset,
    Wait,
    Idle,
    Disabled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InputShapePhase {
    Connect { reset: bool },
    Ready,
    ResetCurrent,
    ResetFresh,
    RetryConnect { reset: bool, at_ms: u64 },
    RetryUpdate { at_ms: u64 },
    Disabled,
}

pub struct InputShapePolicy {
    phase: InputShapePhase,
    cache_valid: bool,
}

impl Default for InputShapePolicy {
    fn default() -> Self {
        Self::new()
    }
}

impl InputShapePolicy {
    pub fn new() -> Self {
        Self {
            phase: InputShapePhase::Connect { reset: false },
            cache_valid: false,
        }
    }

    pub fn action(&mut self, now_ms: u64, cache_matches: bool) -> InputShapeAction {
        match self.phase {
            InputShapePhase::RetryConnect { reset, at_ms } if now_ms >= at_ms => {
                self.phase = InputShapePhase::Connect { reset };
                InputShapeAction::Connect
            }
            InputShapePhase::RetryUpdate { at_ms } if now_ms >= at_ms => {
                self.phase = InputShapePhase::Ready;
                InputShapeAction::Update
            }
            InputShapePhase::Connect { .. } => InputShapeAction::Connect,
            InputShapePhase::Ready if self.cache_valid && cache_matches => InputShapeAction::Idle,
            InputShapePhase::Ready => InputShapeAction::Update,
            InputShapePhase::ResetCurrent | InputShapePhase::ResetFresh => InputShapeAction::Reset,
            InputShapePhase::RetryConnect { .. } | InputShapePhase::RetryUpdate { .. } => {
                InputShapeAction::Wait
            }
            InputShapePhase::Disabled => InputShapeAction::Disabled,
        }
    }

    pub fn connect_succeeded(&mut self) {
        self.phase = match self.phase {
            InputShapePhase::Connect { reset: true } => InputShapePhase::ResetFresh,
            _ => InputShapePhase::Ready,
        };
    }

    pub fn connect_failed(&mut self, now_ms: u64) {
        let reset = matches!(self.phase, InputShapePhase::Connect { reset: true });
        self.cache_valid = false;
        self.phase = InputShapePhase::RetryConnect {
            reset,
            at_ms: now_ms.saturating_add(INPUT_SHAPE_RETRY_MS),
        };
    }

    pub fn unsupported(&mut self) {
        self.cache_valid = false;
        self.phase = InputShapePhase::Disabled;
    }

    pub fn update_succeeded(&mut self) {
        self.cache_valid = true;
        self.phase = InputShapePhase::Ready;
    }

    pub fn update_failed(&mut self) {
        self.cache_valid = false;
        self.phase = InputShapePhase::ResetCurrent;
    }

    pub fn reset_succeeded(&mut self, now_ms: u64) {
        self.cache_valid = false;
        self.phase = InputShapePhase::RetryUpdate {
            at_ms: now_ms.saturating_add(INPUT_SHAPE_RETRY_MS),
        };
    }

    pub fn reset_failed(&mut self, now_ms: u64) {
        self.cache_valid = false;
        self.phase = match self.phase {
            InputShapePhase::ResetCurrent => InputShapePhase::Connect { reset: true },
            _ => InputShapePhase::RetryConnect {
                reset: true,
                at_ms: now_ms.saturating_add(INPUT_SHAPE_RETRY_MS),
            },
        };
    }

    pub fn cache_valid(&self) -> bool {
        self.cache_valid
    }
}

pub enum InputShaperInitError {
    Retry,
    Unsupported,
}

enum ConnectFailure {
    Transient,
    Unsupported,
}

fn reset_input_shape(connection: &RustConnection, window: Window) -> Result<(), ()> {
    connection
        .shape_mask(SO::SET, SK::INPUT, window, 0, 0, PixmapEnum::NONE)
        .map_err(|_| ())?
        .check()
        .map_err(|_| ())?;
    connection.flush().map_err(|_| ())
}

fn update_input_shape(
    connection: &RustConnection,
    window: Window,
    rectangles: &[Rectangle],
) -> Result<(), ()> {
    connection
        .shape_rectangles(
            SO::SET,
            SK::INPUT,
            x11rb::protocol::xproto::ClipOrdering::YX_BANDED,
            window,
            0,
            0,
            rectangles,
        )
        .map_err(|_| ())?
        .check()
        .map_err(|_| ())?;
    connection.flush().map_err(|_| ())
}

impl InputShaper {
    pub fn from_frame(frame: &eframe::Frame) -> Result<Self, InputShaperInitError> {
        let Ok(handle) = frame.window_handle() else {
            return Err(InputShaperInitError::Retry);
        };
        let raw = handle.as_raw();
        let window = match raw {
            RawWindowHandle::Xlib(handle) => {
                let Ok(window) = u32::try_from(handle.window) else {
                    return Err(InputShaperInitError::Unsupported);
                };
                window
            }
            RawWindowHandle::Xcb(handle) => handle.window.get(),
            _ => return Err(InputShaperInitError::Unsupported),
        };
        Ok(Self {
            connection: None,
            window,
            last_rectangles: None,
            policy: InputShapePolicy::new(),
        })
    }

    fn connect() -> Result<RustConnection, ConnectFailure> {
        let (connection, _) = x11rb::connect(None).map_err(|_| ConnectFailure::Transient)?;
        let version = connection
            .shape_query_version()
            .map_err(|_| ConnectFailure::Transient)?
            .reply()
            .map_err(|_| ConnectFailure::Transient)?;
        if version.major_version < 1 || (version.major_version == 1 && version.minor_version < 1) {
            return Err(ConnectFailure::Unsupported);
        }
        Ok(connection)
    }

    pub fn update(
        &mut self,
        logical_width: f32,
        logical_height: f32,
        logical_radius: f32,
        pixels_per_point: f32,
        now_ms: u64,
    ) {
        let rectangles = rounded_input_rectangles(
            logical_width,
            logical_height,
            logical_radius,
            pixels_per_point,
        );
        for _ in 0..5 {
            let cache_matches = self
                .last_rectangles
                .as_deref()
                .is_some_and(|last| same_rectangles(last, &rectangles));
            match self.policy.action(now_ms, cache_matches) {
                InputShapeAction::Connect => match Self::connect() {
                    Ok(connection) => {
                        self.connection = Some(connection);
                        self.policy.connect_succeeded();
                    }
                    Err(ConnectFailure::Transient) => {
                        self.policy.connect_failed(now_ms);
                        break;
                    }
                    Err(ConnectFailure::Unsupported) => {
                        self.policy.unsupported();
                        break;
                    }
                },
                InputShapeAction::Update => {
                    let result = self.connection.as_ref().map_or(Err(()), |connection| {
                        update_input_shape(connection, self.window, &rectangles)
                    });
                    if result.is_ok() {
                        self.last_rectangles = Some(rectangles);
                        self.policy.update_succeeded();
                        break;
                    }
                    self.last_rectangles = None;
                    self.policy.update_failed();
                }
                InputShapeAction::Reset => {
                    let result = self.connection.as_ref().map_or(Err(()), |connection| {
                        reset_input_shape(connection, self.window)
                    });
                    self.last_rectangles = None;
                    if result.is_ok() {
                        self.policy.reset_succeeded(now_ms);
                        break;
                    }
                    self.connection = None;
                    self.policy.reset_failed(now_ms);
                }
                InputShapeAction::Wait | InputShapeAction::Idle | InputShapeAction::Disabled => {
                    break
                }
            }
        }
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
