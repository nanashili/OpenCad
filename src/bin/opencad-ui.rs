#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[cfg(target_os = "macos")]
#[path = "support/macos_quit.rs"]
mod macos_quit;

#[cfg(target_os = "macos")]
#[path = "support/macos_menu.rs"]
mod macos_menu;

fn main() -> eframe::Result {
    let path = std::env::args_os().nth(1).map(std::path::PathBuf::from);
    eframe::run_native(
        "OpenCad",
        eframe::NativeOptions {
            viewport: eframe::egui::ViewportBuilder::default()
                .with_inner_size([1440.0, 920.0])
                .with_min_inner_size([980.0, 640.0])
                .with_app_id("org.opendesk.opencad"),
            ..Default::default()
        },
        Box::new(move |cc| {
            #[cfg(target_os = "macos")]
            macos_quit::install(cc)?;
            let app = opencad::desktop::OpenCadApp::new(cc, path);
            #[cfg(target_os = "macos")]
            macos_menu::install(&cc.egui_ctx)?;
            Ok(Box::new(app))
        }),
    )
}
