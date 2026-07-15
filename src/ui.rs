use crate::{
    config::{ConfigStore, Position},
    morph::{
        morph_placement, origin_for_size, reflow_expanded_drag, Growth, MorphAnimation, MorphPhase,
        MorphPlacement, COMPACT_SIZE,
    },
    quota::{format_reset_time, ring_tone, weekly_window, QuotaWindow, RingTone},
    usage::{format_tokens, heatmap_cells, month_labels, HeatCell},
    worker::{DashboardViewState, SectionState, WorkerHandle},
    x11::{
        clamp_to_known_bounds, query_monitor_bounds, rounded_input_rectangles, select_bounds,
        Bounds, InputShaper, InputShaperInitError, INPUT_SHAPE_RETRY_MS,
    },
};
use chrono::Local;
use eframe::egui;
use std::time::Instant;

pub const HEAT_CELL: f32 = 7.0;
pub const HEAT_GAP: f32 = 2.0;
pub const POSITION_SETTLE_MS: u64 = 500;

pub fn heat_cell_rect(origin: egui::Pos2, index: usize) -> egui::Rect {
    let week = index / 7;
    let day = index % 7;
    egui::Rect::from_min_size(
        origin
            + egui::vec2(
                week as f32 * (HEAT_CELL + HEAT_GAP),
                day as f32 * (HEAT_CELL + HEAT_GAP),
            ),
        egui::vec2(HEAT_CELL, HEAT_CELL),
    )
}

pub fn should_collapse(target_expanded: bool, escape: bool, focus_lost: bool) -> bool {
    target_expanded && (escape || focus_lost)
}

pub fn should_drive_viewport_position(phase: MorphPhase, drag_active: bool) -> bool {
    phase != MorphPhase::Expanded || !drag_active
}

pub fn point_in_rounded_rect(rect: egui::Rect, radius: f32, point: egui::Pos2) -> bool {
    if !rect.contains(point) {
        return false;
    }
    let radius = radius.clamp(0.0, rect.width().min(rect.height()) / 2.0);
    if radius == 0.0 {
        return true;
    }
    let center = egui::pos2(
        point.x.clamp(rect.left() + radius, rect.right() - radius),
        point.y.clamp(rect.top() + radius, rect.bottom() - radius),
    );
    center.distance_sq(point) <= radius * radius
}

pub fn background_drag_allowed(
    surface: egui::Rect,
    radius: f32,
    press_origin: Option<egui::Pos2>,
    current: Option<egui::Pos2>,
    blockers: &[egui::Rect],
) -> bool {
    press_origin.or(current).is_some_and(|point| {
        point_in_rounded_rect(surface, radius, point)
            && !blockers.iter().any(|rect| rect.contains(point))
    })
}

pub fn input_shape_pixels_per_point(ctx: &egui::Context) -> f32 {
    let pixels_per_point = ctx.pixels_per_point();
    if pixels_per_point.is_finite() && pixels_per_point > 0.0 {
        pixels_per_point
    } else {
        1.0
    }
}

pub fn rounded_surface_rects(surface: egui::Rect, radius: f32) -> Vec<egui::Rect> {
    rounded_input_rectangles(surface.width(), surface.height(), radius, 1.0)
        .into_iter()
        .map(|rect| {
            egui::Rect::from_min_size(
                surface.min + egui::vec2(f32::from(rect.x), f32::from(rect.y)),
                egui::vec2(f32::from(rect.width), f32::from(rect.height)),
            )
        })
        .collect()
}

pub fn compact_face_geometry(
    surface: egui::Rect,
    growth: Growth,
    compact_alpha: f32,
) -> (egui::Pos2, f32) {
    let left = matches!(growth, Growth::LeftDown | Growth::LeftUp);
    let up = matches!(growth, Growth::RightUp | Growth::LeftUp);
    let center = egui::pos2(
        if left {
            surface.right() - COMPACT_SIZE.x / 2.0
        } else {
            surface.left() + COMPACT_SIZE.x / 2.0
        },
        if up {
            surface.bottom() - COMPACT_SIZE.y / 2.0
        } else {
            surface.top() + COMPACT_SIZE.y / 2.0
        },
    );
    (center, 38.0 * compact_alpha.clamp(0.0, 1.0))
}

pub fn heat_tooltip(cell: &HeatCell) -> String {
    format!(
        "{}\n使用 {} tokens",
        cell.date.format("%Y-%m-%d"),
        format_tokens(cell.tokens)
    )
}

#[derive(Default)]
pub struct PositionSettleTracker {
    active: bool,
    initial: Option<Position>,
    candidate: Option<Position>,
    changed_at_ms: u64,
    moved: bool,
}

impl PositionSettleTracker {
    pub fn start(&mut self, position: Option<Position>, now_ms: u64) {
        self.active = true;
        self.initial = position;
        self.candidate = position;
        self.changed_at_ms = now_ms;
        self.moved = false;
    }

    pub fn observe(&mut self, position: Position, now_ms: u64) -> Option<Position> {
        if !self.active {
            return None;
        }
        if self.candidate != Some(position) {
            self.candidate = Some(position);
            self.changed_at_ms = now_ms;
            self.moved |= self.initial != Some(position);
            return None;
        }
        if self.moved && now_ms.saturating_sub(self.changed_at_ms) >= POSITION_SETTLE_MS {
            self.active = false;
            return Some(position);
        }
        None
    }

    fn is_active(&self) -> bool {
        self.active
    }

    fn stop(&mut self) {
        self.active = false;
    }
}

pub fn concise_error(error: &str, max_chars: usize) -> String {
    let count = error.chars().count();
    if count <= max_chars {
        return error.to_owned();
    }
    if max_chars == 0 {
        return String::new();
    }
    error
        .chars()
        .take(max_chars - 1)
        .chain(std::iter::once('…'))
        .collect()
}

pub fn ring_points(center: egui::Pos2, radius: f32, remaining: u8) -> Vec<egui::Pos2> {
    if remaining == 0 {
        return Vec::new();
    }
    let segments = ((remaining as usize * 64) / 100).max(2);
    (0..=segments)
        .map(|step| {
            let angle = -std::f32::consts::FRAC_PI_2
                + std::f32::consts::TAU * remaining as f32 / 100.0 * step as f32 / segments as f32;
            center + egui::vec2(angle.cos(), angle.sin()) * radius
        })
        .collect()
}

fn faded(color: egui::Color32, alpha: f32) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(
        color.r(),
        color.g(),
        color.b(),
        (color.a() as f32 * alpha.clamp(0.0, 1.0)).round() as u8,
    )
}

fn section_status<T>(section: &SectionState<T>, refreshing: bool) -> String {
    if section.stale {
        "数据可能已过期".to_owned()
    } else if refreshing {
        "正在更新…".to_owned()
    } else {
        section
            .updated_at
            .and_then(|time| time.elapsed().ok())
            .map(|elapsed| format!("{} 分钟前更新", elapsed.as_secs() / 60))
            .unwrap_or_else(|| "等待首次更新".to_owned())
    }
}

pub struct FloatingApp {
    worker: WorkerHandle,
    state: DashboardViewState,
    config: ConfigStore,
    compact_anchor: Option<Position>,
    morph: MorphAnimation,
    placement: Option<MorphPlacement>,
    positioned: bool,
    monitor_bounds: Vec<Bounds>,
    primary_monitor: usize,
    position_tracker: PositionSettleTracker,
    started_at: Instant,
    input_shaper: Option<InputShaper>,
    input_shape_disabled: bool,
    input_shape_retry_at_ms: u64,
}

impl FloatingApp {
    pub fn new(worker: WorkerHandle, config: ConfigStore) -> Self {
        let compact_anchor = config.load();
        let (monitor_bounds, primary_monitor) = query_monitor_bounds().unwrap_or_default();
        Self {
            worker,
            state: DashboardViewState::default(),
            config,
            compact_anchor,
            morph: MorphAnimation::default(),
            placement: None,
            positioned: false,
            monitor_bounds,
            primary_monitor,
            position_tracker: PositionSettleTracker::default(),
            started_at: Instant::now(),
            input_shaper: None,
            input_shape_disabled: false,
            input_shape_retry_at_ms: 0,
        }
    }

    fn now_ms(&self) -> u64 {
        self.started_at.elapsed().as_millis() as u64
    }

    fn fallback_bounds(ctx: &egui::Context) -> Bounds {
        let size = ctx
            .input(|input| input.viewport().monitor_size)
            .unwrap_or(egui::vec2(1920.0, 1080.0));
        Bounds {
            x: 0,
            y: 0,
            width: size.x.round() as i32,
            height: size.y.round() as i32,
        }
    }

    fn logical_monitor_bounds(&self, ctx: &egui::Context) -> Vec<Bounds> {
        let pixels_per_point = ctx.input(|input| {
            input
                .viewport()
                .native_pixels_per_point
                .filter(|scale| scale.is_finite() && *scale > 0.0)
                .unwrap_or(1.0)
        });
        self.monitor_bounds
            .iter()
            .map(|bounds| bounds.to_logical(pixels_per_point))
            .collect()
    }

    fn clamped_compact_position(&self, ctx: &egui::Context, position: Position) -> Position {
        let bounds = self.logical_monitor_bounds(ctx);
        clamp_to_known_bounds(
            position,
            select_bounds(&bounds, self.primary_monitor, position),
            COMPACT_SIZE.x.round() as i32,
            COMPACT_SIZE.y.round() as i32,
        )
    }

    fn outer_position(ctx: &egui::Context) -> Option<Position> {
        ctx.input(|input| input.viewport().outer_rect)
            .map(|rect| Position {
                x: rect.min.x.round() as i32,
                y: rect.min.y.round() as i32,
            })
    }

    fn begin_transition(&mut self, ctx: &egui::Context, expanded: bool) {
        if self.morph.target_expanded() == expanded {
            return;
        }
        if !expanded && self.position_tracker.is_active() {
            self.commit_expanded_position(ctx, false);
            self.position_tracker.stop();
        }
        if expanded {
            let anchor = Self::outer_position(ctx)
                .or(self.compact_anchor)
                .unwrap_or(Position { x: 0, y: 0 });
            let anchor = self.clamped_compact_position(ctx, anchor);
            let bounds = self.logical_monitor_bounds(ctx);
            let workarea = select_bounds(&bounds, self.primary_monitor, anchor)
                .unwrap_or_else(|| Self::fallback_bounds(ctx));
            let placement = morph_placement(anchor, workarea);
            self.compact_anchor = Some(placement.compact_anchor);
            self.placement = Some(placement);
            self.worker.request_refresh();
        }
        self.morph.set_expanded(expanded, self.now_ms());
        ctx.request_repaint();
    }

    fn commit_expanded_position(&mut self, ctx: &egui::Context, reposition: bool) {
        let (Some(current), Some(origin)) = (self.placement, Self::outer_position(ctx)) else {
            return;
        };
        let workareas = self.logical_monitor_bounds(ctx);
        let Some(placement) =
            reflow_expanded_drag(origin, current.growth, &workareas, self.primary_monitor)
        else {
            return;
        };
        self.compact_anchor = Some(placement.compact_anchor);
        self.placement = Some(placement);
        if reposition && placement.expanded_origin != origin {
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
                placement.expanded_origin.x as f32,
                placement.expanded_origin.y as f32,
            )));
        }
        let _ = self.config.save(placement.compact_anchor);
    }

    fn place_once(&mut self, ctx: &egui::Context) {
        if self.positioned {
            return;
        }
        let monitor_bounds = self.logical_monitor_bounds(ctx);
        let bounds = self
            .compact_anchor
            .and_then(|position| select_bounds(&monitor_bounds, self.primary_monitor, position))
            .or_else(|| monitor_bounds.get(self.primary_monitor).copied())
            .or_else(|| monitor_bounds.first().copied());
        let (initial, bounds) = match self.compact_anchor {
            Some(position) => (position, bounds),
            None => {
                let bounds = bounds.unwrap_or_else(|| Self::fallback_bounds(ctx));
                (
                    Position {
                        x: bounds
                            .x
                            .saturating_add((bounds.width - COMPACT_SIZE.x as i32 - 24).max(0)),
                        y: bounds.y.saturating_add(24),
                    },
                    Some(bounds),
                )
            }
        };
        let clamped = clamp_to_known_bounds(
            initial,
            bounds,
            COMPACT_SIZE.x as i32,
            COMPACT_SIZE.y as i32,
        );
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
            clamped.x as f32,
            clamped.y as f32,
        )));
        self.compact_anchor = Some(clamped);
        self.positioned = true;
    }

    fn track_current_position(&mut self, ctx: &egui::Context) {
        let Some(position) = Self::outer_position(ctx) else {
            return;
        };
        let Some(settled) = self.position_tracker.observe(position, self.now_ms()) else {
            return;
        };
        match self.morph.phase() {
            MorphPhase::Expanded => self.commit_expanded_position(ctx, true),
            MorphPhase::Collapsed => {
                let position = self.clamped_compact_position(ctx, settled);
                if position != settled {
                    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
                        position.x as f32,
                        position.y as f32,
                    )));
                }
                if self.config.save(position).is_ok() {
                    self.compact_anchor = Some(position);
                }
            }
            MorphPhase::Expanding | MorphPhase::Collapsing => {}
        }
    }

    fn paint_compact(&self, ui: &egui::Ui, rect: egui::Rect, growth: Growth, alpha: f32) {
        let remaining = self
            .state
            .quota
            .value
            .as_ref()
            .and_then(weekly_window)
            .map(|window| window.remaining_percent);
        let color = match ring_tone(remaining) {
            RingTone::Green => egui::Color32::from_rgb(34, 197, 94),
            RingTone::Yellow => egui::Color32::from_rgb(234, 179, 8),
            RingTone::Red => egui::Color32::from_rgb(239, 68, 68),
            RingTone::Gray => egui::Color32::from_rgb(100, 116, 139),
        };
        let (center, radius) = compact_face_geometry(rect, growth, alpha);
        if radius <= 0.0 {
            return;
        }
        let scale = radius / 38.0;
        ui.painter().circle_stroke(
            center,
            radius,
            egui::Stroke::new(
                7.0 * scale,
                faded(egui::Color32::from_rgb(51, 65, 85), alpha),
            ),
        );
        let points = ring_points(center, radius, remaining.unwrap_or(0));
        if points.len() > 1 {
            ui.painter().add(egui::Shape::line(
                points,
                egui::Stroke::new(7.0 * scale, faded(color, alpha)),
            ));
        }
        let label = remaining
            .map(|value| format!("{value}%"))
            .unwrap_or_else(|| "!".into());
        ui.painter().text(
            center,
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(19.0 * scale),
            faded(egui::Color32::WHITE, alpha),
        );
    }

    fn paint_text(
        ui: &egui::Ui,
        pos: egui::Pos2,
        align: egui::Align2,
        text: impl ToString,
        size: f32,
        color: egui::Color32,
        alpha: f32,
    ) {
        ui.painter().text(
            pos,
            align,
            text.to_string(),
            egui::FontId::proportional(size),
            faded(color, alpha),
        );
    }

    fn paint_weekly(
        &self,
        ui: &egui::Ui,
        origin: egui::Pos2,
        weekly: Option<&QuotaWindow>,
        alpha: f32,
    ) {
        let white = egui::Color32::from_rgb(226, 232, 240);
        let muted = egui::Color32::from_rgb(148, 163, 184);
        Self::paint_text(
            ui,
            origin,
            egui::Align2::LEFT_TOP,
            "Weekly limits",
            13.0,
            white,
            alpha,
        );
        if let Some(window) = weekly {
            Self::paint_text(
                ui,
                origin + egui::vec2(254.0, 0.0),
                egui::Align2::RIGHT_TOP,
                format!("{}%", window.remaining_percent),
                13.0,
                egui::Color32::WHITE,
                alpha,
            );
            let bar =
                egui::Rect::from_min_size(origin + egui::vec2(0.0, 20.0), egui::vec2(254.0, 6.0));
            ui.painter()
                .rect_filled(bar, 3.0, faded(egui::Color32::from_rgb(51, 65, 85), alpha));
            let width = bar.width() * window.remaining_percent as f32 / 100.0;
            if width > 0.0 {
                ui.painter().rect_filled(
                    egui::Rect::from_min_size(bar.min, egui::vec2(width, bar.height())),
                    3.0,
                    faded(egui::Color32::from_rgb(34, 197, 94), alpha),
                );
            }
            Self::paint_text(
                ui,
                origin + egui::vec2(0.0, 31.0),
                egui::Align2::LEFT_TOP,
                format!("重置时间 {}", format_reset_time(window.resets_at)),
                10.0,
                muted,
                alpha,
            );
        } else {
            Self::paint_text(
                ui,
                origin + egui::vec2(0.0, 22.0),
                egui::Align2::LEFT_TOP,
                "Weekly limits：不可用",
                11.0,
                muted,
                alpha,
            );
        }
    }

    fn paint_heatmap(&self, ui: &mut egui::Ui, origin: egui::Pos2, alpha: f32, interactive: bool) {
        let white = egui::Color32::from_rgb(226, 232, 240);
        let muted = egui::Color32::from_rgb(148, 163, 184);
        Self::paint_text(
            ui,
            origin,
            egui::Align2::LEFT_TOP,
            "每日使用强度",
            13.0,
            white,
            alpha,
        );
        Self::paint_text(
            ui,
            origin + egui::vec2(254.0, 1.0),
            egui::Align2::RIGHT_TOP,
            "近 26 周",
            10.0,
            muted,
            alpha,
        );

        let Some(daily) = self
            .state
            .usage
            .value
            .as_ref()
            .and_then(|snapshot| snapshot.daily.as_ref())
        else {
            Self::paint_text(
                ui,
                origin + egui::vec2(0.0, 35.0),
                egui::Align2::LEFT_TOP,
                "Token 历史不可用",
                11.0,
                muted,
                alpha,
            );
            return;
        };

        let cells = heatmap_cells(Local::now().date_naive(), daily);
        let grid = origin + egui::vec2(22.0, 37.0);
        for (week, label) in month_labels(&cells) {
            Self::paint_text(
                ui,
                grid + egui::vec2(week as f32 * (HEAT_CELL + HEAT_GAP), -15.0),
                egui::Align2::LEFT_TOP,
                label,
                9.0,
                muted,
                alpha,
            );
        }
        for (row, label) in [(1, "一"), (3, "三"), (5, "五")] {
            Self::paint_text(
                ui,
                grid + egui::vec2(-7.0, row as f32 * (HEAT_CELL + HEAT_GAP) + 3.5),
                egui::Align2::RIGHT_CENTER,
                label,
                8.0,
                muted,
                alpha,
            );
        }
        let today = Local::now().date_naive();
        for (index, cell) in cells.iter().enumerate() {
            if cell.future {
                continue;
            }
            let rect = heat_cell_rect(grid, index);
            let color = match cell.level {
                0 => egui::Color32::from_rgb(51, 65, 85),
                1 => egui::Color32::from_rgb(20, 83, 45),
                2 => egui::Color32::from_rgb(21, 128, 61),
                3 => egui::Color32::from_rgb(34, 197, 94),
                _ => egui::Color32::from_rgb(134, 239, 172),
            };
            ui.painter().rect_filled(rect, 1.0, faded(color, alpha));
            if cell.date == today {
                ui.painter().rect_stroke(
                    rect,
                    1.0,
                    egui::Stroke::new(1.0, faded(egui::Color32::from_rgb(226, 232, 240), alpha)),
                );
            }
            if interactive {
                ui.interact(
                    rect,
                    ui.id().with(("heat-cell", index)),
                    egui::Sense::hover(),
                )
                .on_hover_text(heat_tooltip(cell));
            }
        }
        let legend_y = grid.y + 72.0;
        Self::paint_text(
            ui,
            egui::pos2(grid.x + 147.0, legend_y),
            egui::Align2::LEFT_CENTER,
            "少",
            9.0,
            muted,
            alpha,
        );
        for level in 0..=4 {
            let color = match level {
                0 => egui::Color32::from_rgb(51, 65, 85),
                1 => egui::Color32::from_rgb(20, 83, 45),
                2 => egui::Color32::from_rgb(21, 128, 61),
                3 => egui::Color32::from_rgb(34, 197, 94),
                _ => egui::Color32::from_rgb(134, 239, 172),
            };
            ui.painter().rect_filled(
                egui::Rect::from_min_size(
                    egui::pos2(grid.x + 165.0 + level as f32 * 11.0, legend_y - 3.5),
                    egui::vec2(7.0, 7.0),
                ),
                1.0,
                faded(color, alpha),
            );
        }
        Self::paint_text(
            ui,
            egui::pos2(grid.x + 224.0, legend_y),
            egui::Align2::LEFT_CENTER,
            "多",
            9.0,
            muted,
            alpha,
        );
    }

    fn paint_button(
        ui: &mut egui::Ui,
        rect: egui::Rect,
        id: impl std::hash::Hash,
        text: &str,
        enabled: bool,
        alpha: f32,
    ) -> egui::Response {
        let response = ui.interact(
            rect,
            ui.id().with(id),
            if enabled {
                egui::Sense::click()
            } else {
                egui::Sense::hover()
            },
        );
        let fill = if response.hovered() && enabled {
            egui::Color32::from_rgb(71, 85, 105)
        } else {
            egui::Color32::from_rgb(51, 65, 85)
        };
        ui.painter().rect_filled(rect, 5.0, faded(fill, alpha));
        Self::paint_text(
            ui,
            rect.center(),
            egui::Align2::CENTER_CENTER,
            text,
            10.0,
            egui::Color32::from_rgb(226, 232, 240),
            alpha,
        );
        response
    }

    fn paint_card_content(&mut self, ui: &mut egui::Ui, rect: egui::Rect, alpha: f32) {
        let interactive = self.morph.phase() == MorphPhase::Expanded;
        let origin = rect.min + egui::vec2(18.0, 13.0);
        Self::paint_text(
            ui,
            origin,
            egui::Align2::LEFT_TOP,
            "Codex 额度",
            17.0,
            egui::Color32::WHITE,
            alpha,
        );
        let refresh_rect = Self::refresh_rect(rect);
        if Self::paint_button(
            ui,
            refresh_rect,
            "refresh",
            "↻ 刷新",
            interactive && !self.state.refreshing,
            alpha,
        )
        .clicked()
        {
            self.worker.request_refresh();
        }
        Self::paint_text(
            ui,
            origin + egui::vec2(0.0, 24.0),
            egui::Align2::LEFT_TOP,
            section_status(&self.state.quota, self.state.refreshing),
            10.0,
            egui::Color32::from_rgb(148, 163, 184),
            alpha,
        );
        let weekly = self.state.quota.value.as_ref().and_then(weekly_window);
        self.paint_weekly(ui, origin + egui::vec2(0.0, 45.0), weekly, alpha);
        self.paint_heatmap(ui, origin + egui::vec2(0.0, 102.0), alpha, interactive);

        let mut error_y = origin.y + 232.0;
        if self.state.usage.stale {
            Self::paint_text(
                ui,
                egui::pos2(origin.x, error_y),
                egui::Align2::LEFT_TOP,
                "Token 数据可能已过期",
                9.0,
                egui::Color32::from_rgb(251, 191, 36),
                alpha,
            );
            error_y += 13.0;
        }
        for error in [
            self.state.quota.error.as_deref(),
            self.state.usage.error.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            Self::paint_text(
                ui,
                egui::pos2(origin.x, error_y),
                egui::Align2::LEFT_TOP,
                concise_error(error, 42),
                9.0,
                egui::Color32::from_rgb(248, 113, 113),
                alpha,
            );
            error_y += 13.0;
        }
        if self.state.quota.error.is_some() || self.state.usage.error.is_some() {
            let retry = Self::retry_rect(rect);
            if Self::paint_button(ui, retry, "retry", "重试", interactive, alpha).clicked() {
                self.worker.request_refresh();
            }
        }
    }

    fn start_drag(&mut self, ctx: &egui::Context) {
        let current = Self::outer_position(ctx).or(self.compact_anchor);
        self.position_tracker.start(current, self.now_ms());
        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }

    fn refresh_rect(surface: egui::Rect) -> egui::Rect {
        egui::Rect::from_min_size(
            surface.min + egui::vec2(220.0, 12.0),
            egui::vec2(52.0, 22.0),
        )
    }

    fn retry_rect(surface: egui::Rect) -> egui::Rect {
        egui::Rect::from_min_size(
            surface.min + egui::vec2(236.0, 242.0),
            egui::vec2(36.0, 20.0),
        )
    }

    fn interaction_blockers(&self, surface: egui::Rect) -> Vec<egui::Rect> {
        let mut blockers = vec![Self::refresh_rect(surface)];
        if self.state.quota.error.is_some() || self.state.usage.error.is_some() {
            blockers.push(Self::retry_rect(surface));
        }
        if let Some(daily) = self
            .state
            .usage
            .value
            .as_ref()
            .and_then(|snapshot| snapshot.daily.as_ref())
        {
            let grid = surface.min + egui::vec2(40.0, 152.0);
            blockers.extend(
                heatmap_cells(Local::now().date_naive(), daily)
                    .iter()
                    .enumerate()
                    .filter(|(_, cell)| !cell.future)
                    .map(|(index, _)| heat_cell_rect(grid, index)),
            );
        }
        blockers
    }
}

impl eframe::App for FloatingApp {
    fn clear_color(&self, _: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, native_frame: &mut eframe::Frame) {
        ctx.set_visuals(egui::Visuals::dark());
        self.place_once(ctx);
        while let Ok(event) = self.worker.events.try_recv() {
            self.state.apply(event);
        }

        let escape = ctx.input(|input| input.key_pressed(egui::Key::Escape));
        let focus_lost = ctx.input(|input| input.viewport().focused == Some(false));
        if should_collapse(self.morph.target_expanded(), escape, focus_lost) {
            self.begin_transition(ctx, false);
        }

        let frame = self.morph.frame(self.now_ms());
        let now_ms = self.now_ms();
        if self.input_shaper.is_none()
            && !self.input_shape_disabled
            && now_ms >= self.input_shape_retry_at_ms
        {
            match InputShaper::from_frame(native_frame) {
                Ok(shaper) => self.input_shaper = Some(shaper),
                Err(InputShaperInitError::Retry) => {
                    self.input_shape_retry_at_ms = now_ms.saturating_add(INPUT_SHAPE_RETRY_MS);
                }
                Err(InputShaperInitError::Unsupported) => self.input_shape_disabled = true,
            }
        }
        if let Some(shaper) = self.input_shaper.as_mut() {
            shaper.update(
                frame.size.x,
                frame.size.y,
                frame.corner_radius,
                input_shape_pixels_per_point(ctx),
                now_ms,
            );
        }
        if let Some(placement) = self.placement {
            let origin = origin_for_size(&placement, frame.size);
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(frame.size));
            if should_drive_viewport_position(self.morph.phase(), self.position_tracker.is_active())
            {
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
                    origin.x as f32,
                    origin.y as f32,
                )));
            }
        }
        self.track_current_position(ctx);

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(egui::Color32::TRANSPARENT))
            .show(ctx, |ui| {
                let rect = egui::Rect::from_min_size(ui.min_rect().min, frame.size);
                ui.painter().rect_filled(
                    rect,
                    frame.corner_radius,
                    egui::Color32::from_rgb(30, 41, 59),
                );
                let blockers = if self.morph.phase() == MorphPhase::Expanded {
                    self.interaction_blockers(rect)
                } else {
                    Vec::new()
                };
                let sense = if frame.animating {
                    egui::Sense::hover()
                } else {
                    egui::Sense::click_and_drag()
                };
                let responses: Vec<_> = rounded_surface_rects(rect, frame.corner_radius)
                    .into_iter()
                    .enumerate()
                    .map(|(index, strip)| {
                        ui.interact(strip, ui.id().with(("surface-interaction", index)), sense)
                    })
                    .collect();
                let growth = self
                    .placement
                    .map(|placement| placement.growth)
                    .unwrap_or(Growth::RightDown);
                self.paint_compact(ui, rect, growth, frame.compact_alpha);
                if frame.content_alpha > 0.0 {
                    self.paint_card_content(ui, rect, frame.content_alpha);
                }
                let (press_origin, pointer) =
                    ctx.input(|input| (input.pointer.press_origin(), input.pointer.interact_pos()));
                let background_allowed = !frame.animating
                    && background_drag_allowed(
                        rect,
                        frame.corner_radius,
                        press_origin,
                        pointer,
                        &blockers,
                    );
                if background_allowed && responses.iter().any(egui::Response::drag_started) {
                    self.start_drag(ctx);
                }
                if background_allowed
                    && self.morph.phase() == MorphPhase::Collapsed
                    && responses.iter().any(egui::Response::clicked)
                {
                    self.begin_transition(ctx, true);
                }
                if background_allowed {
                    for response in responses {
                        response.context_menu(|ui| {
                            if ui.button("刷新").clicked() {
                                self.worker.request_refresh();
                                ui.close_menu();
                            }
                            if ui.button("退出").clicked() {
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                        });
                    }
                }
            });

        if self.morph.phase() == MorphPhase::Collapsed && self.placement.is_some() {
            self.placement = None;
        }
        if frame.animating {
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
    }
}
