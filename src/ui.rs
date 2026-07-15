use crate::{
    config::{ConfigStore, Position},
    quota::{format_reset_time, ring_tone, QuotaWindow, RingTone},
    worker::{QuotaViewState, WorkerHandle},
    x11::{clamp_to_known_bounds, query_monitor_bounds, select_bounds, Bounds},
};
use eframe::egui;
use std::time::Instant;

pub const BALL_SIZE: egui::Vec2 = egui::vec2(88.0, 88.0);
pub const EXPANDED_SIZE: egui::Vec2 = egui::vec2(360.0, 260.0);
pub const CARD_WIDTH: i32 = 272;
pub const POSITION_SETTLE_MS: u64 = 500;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExpandedLayout {
    pub viewport_origin: Position,
    pub ball_offset: Position,
    pub card_origin: Position,
}

pub fn expanded_layout(anchor: Position, workarea: Bounds) -> ExpandedLayout {
    let viewport_width = EXPANDED_SIZE.x as i32;
    let viewport_height = EXPANDED_SIZE.y as i32;
    let ball_width = BALL_SIZE.x as i32;
    let ball_height = BALL_SIZE.y as i32;
    let right = workarea.x.saturating_add(workarea.width);
    let card_on_right = anchor.x.saturating_add(viewport_width) <= right;
    let desired_x = if card_on_right {
        anchor.x
    } else {
        anchor.x.saturating_sub(CARD_WIDTH)
    };
    let viewport_origin = clamp_to_known_bounds(
        Position {
            x: desired_x,
            y: anchor.y,
        },
        Some(workarea),
        viewport_width,
        viewport_height,
    );
    let ball_offset = Position {
        x: if card_on_right { 0 } else { CARD_WIDTH },
        y: (anchor.y - viewport_origin.y).clamp(0, viewport_height - ball_height),
    };
    let card_origin = Position {
        x: if card_on_right { ball_width } else { 0 },
        y: 0,
    };
    ExpandedLayout {
        viewport_origin,
        ball_offset,
        card_origin,
    }
}

pub fn compact_anchor_from_viewport(
    viewport_origin: Position,
    ball_offset: Position,
    workarea: Bounds,
) -> Position {
    clamp_to_known_bounds(
        Position {
            x: viewport_origin.x.saturating_add(ball_offset.x),
            y: viewport_origin.y.saturating_add(ball_offset.y),
        },
        Some(workarea),
        BALL_SIZE.x as i32,
        BALL_SIZE.y as i32,
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

pub fn window_size(expanded: bool) -> egui::Vec2 {
    if expanded {
        EXPANDED_SIZE
    } else {
        BALL_SIZE
    }
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

pub struct FloatingApp {
    worker: WorkerHandle,
    state: QuotaViewState,
    config: ConfigStore,
    compact_anchor: Option<Position>,
    expanded: bool,
    expanded_layout: Option<ExpandedLayout>,
    positioned: bool,
    monitor_bounds: Vec<Bounds>,
    primary_monitor: usize,
    position_tracker: PositionSettleTracker,
    started_at: Instant,
}

impl FloatingApp {
    pub fn new(worker: WorkerHandle, config: ConfigStore) -> Self {
        let compact_anchor = config.load();
        let (monitor_bounds, primary_monitor) = query_monitor_bounds().unwrap_or_default();
        Self {
            worker,
            state: QuotaViewState::default(),
            config,
            compact_anchor,
            expanded: false,
            expanded_layout: None,
            positioned: false,
            monitor_bounds,
            primary_monitor,
            position_tracker: PositionSettleTracker::default(),
            started_at: Instant::now(),
        }
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

    fn clamped_position(
        &self,
        ctx: &egui::Context,
        position: Position,
        size: egui::Vec2,
    ) -> Position {
        let bounds = self.logical_monitor_bounds(ctx);
        clamp_to_known_bounds(
            position,
            select_bounds(&bounds, self.primary_monitor, position),
            size.x.round() as i32,
            size.y.round() as i32,
        )
    }

    fn set_expanded(&mut self, ctx: &egui::Context, expanded: bool) {
        if self.expanded == expanded {
            return;
        }
        let size = window_size(expanded);
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
        let outer_position = ctx
            .input(|input| input.viewport().outer_rect)
            .map(|outer| Position {
                x: outer.min.x.round() as i32,
                y: outer.min.y.round() as i32,
            });
        if expanded {
            let anchor = outer_position.or(self.compact_anchor);
            if let Some(anchor) = anchor {
                let anchor = self.clamped_position(ctx, anchor, BALL_SIZE);
                self.compact_anchor = Some(anchor);
                let bounds = self.logical_monitor_bounds(ctx);
                let workarea = select_bounds(&bounds, self.primary_monitor, anchor)
                    .unwrap_or_else(|| Self::fallback_bounds(ctx));
                let layout = expanded_layout(anchor, workarea);
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
                    layout.viewport_origin.x as f32,
                    layout.viewport_origin.y as f32,
                )));
                self.expanded_layout = Some(layout);
            }
            self.worker.request_refresh();
        } else if let Some(anchor) = self.compact_anchor {
            let position = self.clamped_position(ctx, anchor, BALL_SIZE);
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
                position.x as f32,
                position.y as f32,
            )));
            self.compact_anchor = Some(position);
            self.expanded_layout = None;
        }
        self.expanded = expanded;
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
                            .saturating_add((bounds.width - BALL_SIZE.x as i32 - 24).max(0)),
                        y: bounds.y.saturating_add(24),
                    },
                    Some(bounds),
                )
            }
        };
        let clamped =
            clamp_to_known_bounds(initial, bounds, BALL_SIZE.x as i32, BALL_SIZE.y as i32);
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
            clamped.x as f32,
            clamped.y as f32,
        )));
        self.compact_anchor = Some(clamped);
        self.positioned = true;
    }

    fn track_current_position(&mut self, ctx: &egui::Context) {
        let Some(rect) = ctx.input(|input| input.viewport().outer_rect) else {
            return;
        };
        let viewport_position = Position {
            x: rect.min.x.round() as i32,
            y: rect.min.y.round() as i32,
        };
        let observed = if let Some(layout) = self.expanded_layout {
            Position {
                x: viewport_position.x.saturating_add(layout.ball_offset.x),
                y: viewport_position.y.saturating_add(layout.ball_offset.y),
            }
        } else {
            viewport_position
        };
        let now_ms = self.started_at.elapsed().as_millis() as u64;
        let Some(settled) = self.position_tracker.observe(observed, now_ms) else {
            return;
        };
        let position = self.clamped_position(ctx, settled, BALL_SIZE);
        if !self.expanded && position != settled {
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
                position.x as f32,
                position.y as f32,
            )));
        }
        if self.config.save(position).is_ok() {
            self.compact_anchor = Some(position);
        }
    }

    fn paint_ball(&self, ui: &mut egui::Ui, rect: egui::Rect) {
        let remaining = self
            .state
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.primary.as_ref())
            .map(|window| window.remaining_percent);
        let color = match ring_tone(remaining) {
            RingTone::Green => egui::Color32::from_rgb(34, 197, 94),
            RingTone::Yellow => egui::Color32::from_rgb(234, 179, 8),
            RingTone::Red => egui::Color32::from_rgb(239, 68, 68),
            RingTone::Gray => egui::Color32::from_rgb(100, 116, 139),
        };
        let center = rect.center();
        ui.painter()
            .circle_filled(center, 35.0, egui::Color32::from_rgb(23, 32, 51));
        ui.painter().circle_stroke(
            center,
            38.0,
            egui::Stroke::new(7.0, egui::Color32::from_rgb(51, 65, 85)),
        );
        let points = ring_points(center, 38.0, remaining.unwrap_or(0));
        if points.len() > 1 {
            ui.painter()
                .add(egui::Shape::line(points, egui::Stroke::new(7.0, color)));
        }
        let label = remaining
            .map(|value| format!("{value}%"))
            .unwrap_or_else(|| "!".into());
        ui.painter().text(
            center,
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(19.0),
            egui::Color32::WHITE,
        );
    }

    fn quota_row(ui: &mut egui::Ui, title: &str, window: Option<&QuotaWindow>) {
        match window {
            Some(window) => {
                ui.horizontal(|ui| {
                    ui.label(title);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.strong(format!("{}%", window.remaining_percent));
                    });
                });
                ui.add(
                    egui::ProgressBar::new(window.remaining_percent as f32 / 100.0)
                        .show_percentage(),
                );
                ui.small(format!("重置时间 {}", format_reset_time(window.resets_at)));
            }
            None => {
                ui.label(format!("{title}：不可用"));
            }
        }
    }

    fn expanded_card(&mut self, ctx: &egui::Context) {
        let card_origin = self
            .expanded_layout
            .map(|layout| layout.card_origin)
            .unwrap_or(Position { x: 88, y: 0 });
        egui::Area::new(egui::Id::new("quota-card"))
            .fixed_pos(egui::pos2(card_origin.x as f32, card_origin.y as f32))
            .show(ctx, |ui| {
                egui::Frame::none()
                    .fill(egui::Color32::from_rgb(30, 41, 59))
                    .rounding(16.0)
                    .inner_margin(16.0)
                    .show(ui, |ui| {
                        ui.set_width(240.0);
                        ui.horizontal(|ui| {
                            ui.heading("Codex 额度");
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .add_enabled(
                                            !self.state.refreshing,
                                            egui::Button::new("↻ 刷新"),
                                        )
                                        .clicked()
                                    {
                                        self.worker.request_refresh();
                                    }
                                },
                            );
                        });
                        let status = if self.state.stale {
                            "数据可能已过期".to_owned()
                        } else if self.state.refreshing {
                            "正在更新…".to_owned()
                        } else {
                            self.state
                                .updated_at
                                .and_then(|time| time.elapsed().ok())
                                .map(|elapsed| format!("{} 分钟前更新", elapsed.as_secs() / 60))
                                .unwrap_or_else(|| "等待首次更新".to_owned())
                        };
                        ui.small(status);
                        ui.add_space(10.0);
                        let primary = self
                            .state
                            .snapshot
                            .as_ref()
                            .and_then(|snapshot| snapshot.primary.as_ref());
                        let secondary = self
                            .state
                            .snapshot
                            .as_ref()
                            .and_then(|snapshot| snapshot.secondary.as_ref());
                        Self::quota_row(ui, "短周期窗口", primary);
                        ui.add_space(10.0);
                        Self::quota_row(ui, "周周期窗口", secondary);
                        if let Some(error) = &self.state.error {
                            ui.horizontal(|ui| {
                                if ui.button("重试").clicked() {
                                    self.worker.request_refresh();
                                }
                                ui.add_sized(
                                    [220.0, 20.0],
                                    egui::Label::new(
                                        egui::RichText::new(concise_error(error, 96))
                                            .color(egui::Color32::from_rgb(248, 113, 113)),
                                    )
                                    .truncate(),
                                );
                            });
                        }
                    });
            });
    }
}

impl eframe::App for FloatingApp {
    fn clear_color(&self, _: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.place_once(ctx);
        self.track_current_position(ctx);
        while let Ok(event) = self.worker.events.try_recv() {
            self.state.apply(event);
        }
        if self.expanded && ctx.input(|input| input.viewport().focused == Some(false)) {
            self.set_expanded(ctx, false);
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(egui::Color32::TRANSPARENT))
            .show(ctx, |ui| {
                let ball_offset = self
                    .expanded_layout
                    .map(|layout| layout.ball_offset)
                    .unwrap_or(Position { x: 0, y: 0 });
                let ball = egui::Rect::from_min_size(
                    ui.min_rect().min + egui::vec2(ball_offset.x as f32, ball_offset.y as f32),
                    BALL_SIZE,
                );
                let response =
                    ui.interact(ball, ui.id().with("ball"), egui::Sense::click_and_drag());
                self.paint_ball(ui, ball);
                if response.drag_started() {
                    let current = self.compact_anchor;
                    self.position_tracker
                        .start(current, self.started_at.elapsed().as_millis() as u64);
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                if response.clicked() {
                    self.set_expanded(ctx, !self.expanded);
                }
                response.context_menu(|ui| {
                    if ui.button("刷新").clicked() {
                        self.worker.request_refresh();
                        ui.close_menu();
                    }
                    if ui.button("退出").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
            });
        if self.expanded {
            self.expanded_card(ctx);
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
    }
}
