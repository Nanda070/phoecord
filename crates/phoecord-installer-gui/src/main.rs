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

#[cfg(test)]
mod package_tests {
    /// The `.deb` and the `.rpm` share one menu entry and one icon size (found checking the
    /// 0.6.0 packages in Docker: the `.deb` had a generated entry and the 1024 px icon).
    #[test]
    fn deb_and_rpm_ship_the_same_menu_entry_and_icon() {
        let manifest = include_str!("../Cargo.toml");
        let entry = "assets/linux/phoecord-installer.desktop";
        assert!(
            manifest.contains(&format!("desktop-template = \"{entry}\"")),
            "the .deb must use the shared menu entry"
        );
        assert!(manifest.contains(&format!("source = \"{entry}\"")));
        let icons = manifest
            .lines()
            .find(|l| l.starts_with("icons = "))
            .expect("icons line");
        assert!(icons.contains("icon-256.png") && !icons.contains("icon-1024.png"));
        let desktop = include_str!("../assets/linux/phoecord-installer.desktop");
        for line in desktop.lines() {
            assert_eq!(line, line.trim_end(), "trailing space in {line:?}");
        }
        assert!(desktop.contains("\nExec=phoecord-installer\n"));
    }
}
