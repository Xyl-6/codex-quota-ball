use crate::{
    config::{clamp_position, default_position, ConfigStore, Position},
    quota::{format_reset_time, ring_tone, QuotaWindow, RingTone},
    worker::{QuotaViewState, WorkerHandle},
};
use eframe::egui;

pub const BALL_SIZE: egui::Vec2 = egui::vec2(88.0, 88.0);
pub const EXPANDED_SIZE: egui::Vec2 = egui::vec2(360.0, 260.0);

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
}

impl FloatingApp {
    pub fn new(worker: WorkerHandle, config: ConfigStore) -> Self {
        let saved_position = config.load();
        Self {
            worker,
            state: QuotaViewState::default(),
            config,
            saved_position,
            expanded: false,
            positioned: false,
        }
    }

    fn clamp_to_monitor(&self, ctx: &egui::Context, size: egui::Vec2) -> Option<Position> {
        let (monitor, outer) =
            ctx.input(|input| (input.viewport().monitor_size, input.viewport().outer_rect));
        let (Some(monitor), Some(outer)) = (monitor, outer) else {
            return None;
        };
        Some(clamp_position(
            Position {
                x: outer.min.x.round() as i32,
                y: outer.min.y.round() as i32,
            },
            monitor.x as i32,
            monitor.y as i32,
            size.x as i32,
            size.y as i32,
        ))
    }

    fn set_expanded(&mut self, ctx: &egui::Context, expanded: bool) {
        if self.expanded == expanded {
            return;
        }
        self.expanded = expanded;
        let size = window_size(expanded);
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
        if let Some(position) = self.clamp_to_monitor(ctx, size) {
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
        let monitor = ctx
            .input(|input| input.viewport().monitor_size)
            .unwrap_or(egui::vec2(1920.0, 1080.0));
        let initial = self
            .saved_position
            .unwrap_or_else(|| default_position(monitor.x as i32, BALL_SIZE.x as i32));
        let clamped = clamp_position(
            initial,
            monitor.x as i32,
            monitor.y as i32,
            BALL_SIZE.x as i32,
            BALL_SIZE.y as i32,
        );
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
            clamped.x as f32,
            clamped.y as f32,
        )));
        self.positioned = true;
    }

    fn save_current_position(&mut self, ctx: &egui::Context) {
        let size = window_size(self.expanded);
        let Some(position) = self.clamp_to_monitor(ctx, size) else {
            return;
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
            position.x as f32,
            position.y as f32,
        )));
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
                            ui.colored_label(egui::Color32::from_rgb(248, 113, 113), error);
                            if ui.button("重试").clicked() {
                                self.worker.request_refresh();
                            }
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
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                if response.drag_stopped() {
                    self.save_current_position(ctx);
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
