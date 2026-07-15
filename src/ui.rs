use crate::{
    config::{ConfigStore, Position},
    quota::{format_reset_time, ring_tone, QuotaWindow, RingTone},
    worker::{QuotaViewState, WorkerHandle},
    x11::{clamp_to_bounds, query_monitor_bounds, select_bounds, Bounds},
};
use eframe::egui;
use std::time::Instant;

pub const BALL_SIZE: egui::Vec2 = egui::vec2(88.0, 88.0);
pub const EXPANDED_SIZE: egui::Vec2 = egui::vec2(360.0, 260.0);
pub const POSITION_SETTLE_MS: u64 = 500;

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
    saved_position: Option<Position>,
    expanded: bool,
    positioned: bool,
    monitor_bounds: Vec<Bounds>,
    primary_monitor: usize,
    position_tracker: PositionSettleTracker,
    started_at: Instant,
}

impl FloatingApp {
    pub fn new(worker: WorkerHandle, config: ConfigStore) -> Self {
        let saved_position = config.load();
        let (monitor_bounds, primary_monitor) = query_monitor_bounds().unwrap_or_default();
        Self {
            worker,
            state: QuotaViewState::default(),
            config,
            saved_position,
            expanded: false,
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

    fn bounds_for(&self, ctx: &egui::Context, position: Position) -> Bounds {
        select_bounds(&self.monitor_bounds, self.primary_monitor, position)
            .unwrap_or_else(|| Self::fallback_bounds(ctx))
    }

    fn clamped_position(
        &self,
        ctx: &egui::Context,
        position: Position,
        size: egui::Vec2,
    ) -> Position {
        clamp_to_bounds(
            position,
            self.bounds_for(ctx, position),
            size.x.round() as i32,
            size.y.round() as i32,
        )
    }

    fn set_expanded(&mut self, ctx: &egui::Context, expanded: bool) {
        if self.expanded == expanded {
            return;
        }
        self.expanded = expanded;
        let size = window_size(expanded);
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
        if let Some(outer) = ctx.input(|input| input.viewport().outer_rect) {
            let position = self.clamped_position(
                ctx,
                Position {
                    x: outer.min.x.round() as i32,
                    y: outer.min.y.round() as i32,
                },
                size,
            );
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
                position.x as f32,
                position.y as f32,
            )));
        }
        if expanded {
            self.worker.request_refresh();
        }
    }

    fn place_once(&mut self, ctx: &egui::Context) {
        if self.positioned {
            return;
        }
        let bounds = self
            .saved_position
            .and_then(|position| {
                select_bounds(&self.monitor_bounds, self.primary_monitor, position)
            })
            .or_else(|| self.monitor_bounds.get(self.primary_monitor).copied())
            .or_else(|| self.monitor_bounds.first().copied())
            .unwrap_or_else(|| Self::fallback_bounds(ctx));
        let initial = self.saved_position.unwrap_or(Position {
            x: bounds
                .x
                .saturating_add((bounds.width - BALL_SIZE.x as i32 - 24).max(0)),
            y: bounds.y.saturating_add(24),
        });
        let clamped = clamp_to_bounds(initial, bounds, BALL_SIZE.x as i32, BALL_SIZE.y as i32);
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
            clamped.x as f32,
            clamped.y as f32,
        )));
        self.positioned = true;
    }

    fn track_current_position(&mut self, ctx: &egui::Context) {
        let Some(rect) = ctx.input(|input| input.viewport().outer_rect) else {
            return;
        };
        let observed = Position {
            x: rect.min.x.round() as i32,
            y: rect.min.y.round() as i32,
        };
        let now_ms = self.started_at.elapsed().as_millis() as u64;
        let Some(settled) = self.position_tracker.observe(observed, now_ms) else {
            return;
        };
        let size = window_size(self.expanded);
        let position = self.clamped_position(ctx, settled, size);
        if position != settled {
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
                position.x as f32,
                position.y as f32,
            )));
        }
        if self.config.save(position).is_ok() {
            self.saved_position = Some(position);
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
        egui::Area::new(egui::Id::new("quota-card"))
            .fixed_pos(egui::pos2(48.0, 12.0))
            .show(ctx, |ui| {
                egui::Frame::none()
                    .fill(egui::Color32::from_rgb(30, 41, 59))
                    .rounding(16.0)
                    .inner_margin(16.0)
                    .show(ui, |ui| {
                        ui.set_width(280.0);
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
                let ball = egui::Rect::from_min_size(ui.min_rect().min, BALL_SIZE);
                let response =
                    ui.interact(ball, ui.id().with("ball"), egui::Sense::click_and_drag());
                self.paint_ball(ui, ball);
                if response.drag_started() {
                    let current =
                        ctx.input(|input| input.viewport().outer_rect)
                            .map(|rect| Position {
                                x: rect.min.x.round() as i32,
                                y: rect.min.y.round() as i32,
                            });
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
