use codex_quota_ball::{
    config::ConfigStore, fonts::load_cjk_font, morph::COMPACT_SIZE, ui::FloatingApp,
    worker::spawn_worker,
};
use eframe::{egui, NativeOptions};

fn main() -> eframe::Result {
    if std::env::var("XDG_SESSION_TYPE").as_deref() != Ok("x11") {
        eprintln!("Codex Quota Ball 0.1 supports Ubuntu GNOME X11 only.");
        std::process::exit(2);
    }
    let config_path = ConfigStore::default_path().expect("Linux config directory is unavailable");
    let options = NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(COMPACT_SIZE)
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(false)
            .with_always_on_top(),
        ..Default::default()
    };
    eframe::run_native(
        "Codex Quota Ball",
        options,
        Box::new(move |creation| {
            load_cjk_font(&creation.egui_ctx);
            Ok(Box::new(FloatingApp::new(
                spawn_worker(),
                ConfigStore::new(config_path),
            )))
        }),
    )
}
