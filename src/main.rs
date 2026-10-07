mod app;
mod calc;
mod components;
mod contrast;
mod ecs;
mod glass;
#[cfg(target_os = "macos")]
mod menu;
mod paint;
mod paint_editor;
mod resources;
mod scrub;
mod systems;
mod widgets;
mod world;

use eframe::egui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Skwiggle")
            .with_inner_size([1280.0, 800.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Skwiggle",
        options,
        Box::new(|cc| {
            glass::init(cc.wgpu_render_state.as_ref());
            Ok(Box::new(app::SkwiggleApp::new(&cc.egui_ctx)))
        }),
    )
}
