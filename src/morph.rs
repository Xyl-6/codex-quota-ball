use crate::{
    config::Position,
    x11::{clamp_to_known_bounds, select_bounds, Bounds},
};
use eframe::egui;

pub const COMPACT_SIZE: egui::Vec2 = egui::vec2(88.0, 88.0);
pub const EXPANDED_SIZE: egui::Vec2 = egui::vec2(290.0, 292.0);
pub const EXPAND_MS: u64 = 220;
pub const COLLAPSE_MS: u64 = 180;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Growth {
    RightDown,
    RightUp,
    LeftDown,
    LeftUp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MorphPhase {
    Collapsed,
    Expanding,
    Expanded,
    Collapsing,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MorphFrame {
    pub progress: f32,
    pub size: egui::Vec2,
    pub corner_radius: f32,
    pub compact_alpha: f32,
    pub content_alpha: f32,
    pub animating: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct MorphAnimation {
    progress: f32,
    start_progress: f32,
    target: f32,
    started_ms: u64,
}

impl Default for MorphAnimation {
    fn default() -> Self {
        Self {
            progress: 0.0,
            start_progress: 0.0,
            target: 0.0,
            started_ms: 0,
        }
    }
}

impl MorphAnimation {
    fn sampled_progress(&self, now_ms: u64) -> f32 {
        if self.progress == self.target {
            return self.target;
        }
        let full_duration = if self.target > self.start_progress {
            EXPAND_MS
        } else {
            COLLAPSE_MS
        };
        let distance = (self.target - self.start_progress).abs();
        let duration = (full_duration as f32 * distance).max(1.0);
        let t = (now_ms.saturating_sub(self.started_ms) as f32 / duration).clamp(0.0, 1.0);
        let eased = if self.target > self.start_progress {
            1.0 - (1.0 - t).powi(3)
        } else {
            t.powi(3)
        };
        self.start_progress + (self.target - self.start_progress) * eased
    }

    pub fn set_expanded(&mut self, expanded: bool, now_ms: u64) {
        self.progress = self.sampled_progress(now_ms);
        self.start_progress = self.progress;
        self.target = if expanded { 1.0 } else { 0.0 };
        self.started_ms = now_ms;
    }

    pub fn frame(&mut self, now_ms: u64) -> MorphFrame {
        self.progress = self.sampled_progress(now_ms);
        if (self.progress - self.target).abs() < f32::EPSILON {
            self.progress = self.target;
            self.start_progress = self.target;
        }
        let progress = self.progress;
        MorphFrame {
            progress,
            size: egui::vec2(
                egui::lerp(COMPACT_SIZE.x..=EXPANDED_SIZE.x, progress),
                egui::lerp(COMPACT_SIZE.y..=EXPANDED_SIZE.y, progress),
            ),
            corner_radius: egui::lerp(44.0..=18.0, progress),
            compact_alpha: (1.0 - progress / 0.45).clamp(0.0, 1.0),
            content_alpha: ((progress - 0.35) / 0.65).clamp(0.0, 1.0),
            animating: progress != self.target,
        }
    }

    pub fn target_expanded(&self) -> bool {
        self.target == 1.0
    }

    pub fn phase(&self) -> MorphPhase {
        match (self.progress, self.target) {
            (0.0, 0.0) => MorphPhase::Collapsed,
            (1.0, 1.0) => MorphPhase::Expanded,
            (_, 1.0) => MorphPhase::Expanding,
            _ => MorphPhase::Collapsing,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MorphPlacement {
    pub compact_anchor: Position,
    pub expanded_origin: Position,
    pub growth: Growth,
}

fn grows_left(growth: Growth) -> bool {
    matches!(growth, Growth::LeftDown | Growth::LeftUp)
}

fn grows_up(growth: Growth) -> bool {
    matches!(growth, Growth::RightUp | Growth::LeftUp)
}

fn raw_anchor_from_expanded(origin: Position, growth: Growth) -> Position {
    Position {
        x: if grows_left(growth) {
            origin.x + EXPANDED_SIZE.x as i32 - COMPACT_SIZE.x as i32
        } else {
            origin.x
        },
        y: if grows_up(growth) {
            origin.y + EXPANDED_SIZE.y as i32 - COMPACT_SIZE.y as i32
        } else {
            origin.y
        },
    }
}

fn desired_expanded_origin(anchor: Position, growth: Growth) -> Position {
    Position {
        x: if grows_left(growth) {
            anchor.x + COMPACT_SIZE.x as i32 - EXPANDED_SIZE.x as i32
        } else {
            anchor.x
        },
        y: if grows_up(growth) {
            anchor.y + COMPACT_SIZE.y as i32 - EXPANDED_SIZE.y as i32
        } else {
            anchor.y
        },
    }
}

pub fn morph_placement(anchor: Position, workarea: Bounds) -> MorphPlacement {
    let compact_anchor = clamp_to_known_bounds(
        anchor,
        Some(workarea),
        COMPACT_SIZE.x as i32,
        COMPACT_SIZE.y as i32,
    );
    let right = workarea.x.saturating_add(workarea.width);
    let bottom = workarea.y.saturating_add(workarea.height);
    let fits_right = compact_anchor.x + EXPANDED_SIZE.x as i32 <= right;
    let fits_left = compact_anchor.x + COMPACT_SIZE.x as i32 - EXPANDED_SIZE.x as i32 >= workarea.x;
    let fits_down = compact_anchor.y + EXPANDED_SIZE.y as i32 <= bottom;
    let fits_up = compact_anchor.y + COMPACT_SIZE.y as i32 - EXPANDED_SIZE.y as i32 >= workarea.y;

    let left = if fits_right {
        false
    } else if fits_left {
        true
    } else {
        compact_anchor.x + COMPACT_SIZE.x as i32 - workarea.x > right - compact_anchor.x
    };
    let up = if fits_down {
        false
    } else if fits_up {
        true
    } else {
        compact_anchor.y + COMPACT_SIZE.y as i32 - workarea.y > bottom - compact_anchor.y
    };
    let growth = match (left, up) {
        (false, false) => Growth::RightDown,
        (false, true) => Growth::RightUp,
        (true, false) => Growth::LeftDown,
        (true, true) => Growth::LeftUp,
    };
    let desired = desired_expanded_origin(compact_anchor, growth);
    let expanded_origin = clamp_to_known_bounds(
        desired,
        Some(workarea),
        EXPANDED_SIZE.x as i32,
        EXPANDED_SIZE.y as i32,
    );
    MorphPlacement {
        compact_anchor,
        expanded_origin,
        growth,
    }
}

pub fn origin_for_size(placement: &MorphPlacement, size: egui::Vec2) -> Position {
    let progress = ((size.x - COMPACT_SIZE.x) / (EXPANDED_SIZE.x - COMPACT_SIZE.x)).clamp(0.0, 1.0);
    Position {
        x: (placement.compact_anchor.x as f32
            + (placement.expanded_origin.x - placement.compact_anchor.x) as f32 * progress)
            .round() as i32,
        y: (placement.compact_anchor.y as f32
            + (placement.expanded_origin.y - placement.compact_anchor.y) as f32 * progress)
            .round() as i32,
    }
}

pub fn compact_anchor_from_expanded(
    expanded_origin: Position,
    growth: Growth,
    workarea: Bounds,
) -> Position {
    clamp_to_known_bounds(
        raw_anchor_from_expanded(expanded_origin, growth),
        Some(workarea),
        COMPACT_SIZE.x as i32,
        COMPACT_SIZE.y as i32,
    )
}

pub fn reflow_expanded_drag(
    expanded_origin: Position,
    growth: Growth,
    workareas: &[Bounds],
    primary_monitor: usize,
) -> Option<MorphPlacement> {
    let raw_anchor = raw_anchor_from_expanded(expanded_origin, growth);
    let workarea = select_bounds(workareas, primary_monitor, raw_anchor)
        .or_else(|| select_bounds(workareas, primary_monitor, expanded_origin))?;
    let expanded_origin = clamp_to_known_bounds(
        expanded_origin,
        Some(workarea),
        EXPANDED_SIZE.x as i32,
        EXPANDED_SIZE.y as i32,
    );
    let compact_anchor = compact_anchor_from_expanded(expanded_origin, growth, workarea);
    Some(MorphPlacement {
        compact_anchor,
        expanded_origin,
        growth,
    })
}
