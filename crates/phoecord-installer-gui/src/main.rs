//! PhoeCord installer window (stage 1.4.3): terms of use, install / repair / uninstall per
//! Discord client, and plugin packages, without a terminal. The work itself is done by
//! `phoecord-installer`, the same code `phoecord setup` and `phoecord uninstall` run.
//!
//! `--dry-run` keeps every change inside a sandbox folder (`PHOECORD_GUI_SANDBOX`, default
//! `<temp>/phoecord-gui-dry-run`) with a simulated trust store; `--demo` adds two made-up Discord
//! installs to it (for screenshots), `--demo-empty` none.

#![cfg_attr(windows, windows_subsystem = "windows")]

mod app;
mod backend;
mod motion;
mod onboarding;
mod packages;
mod splash;
mod tabs;
mod terms;
mod text;
mod theme;
mod update;
mod widgets;

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let mode = backend::Mode::from_args(&args);
    // A downloaded PhoeCord update is installed before the window opens (exits when it takes over).
    let notes = update::at_start(&mode);
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("PhoeCord")
            .with_app_id("dev.phoecord.installer")
            .with_inner_size([960.0, 640.0])
            .with_min_inner_size([520.0, 520.0])
            .with_icon(theme::icon_data()),
        ..Default::default()
    };
    eframe::run_native(
        "PhoeCord",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, mode, notes)))),
    )
}
