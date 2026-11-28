//! The Plugins tab: built-in plugins and installed packages with a switch each, install from a
//! file, a dropped file or an HTTPS link (rights shown first), remove with a confirm.

use crate::app::{App, Dialog, button_row};
use crate::motion::{self, dur};
use crate::packages::{self, Asks};
use crate::theme::{self, color};
use crate::widgets::{self, Button, Kind, Tone};
use eframe::egui::{self, Align2, Id, Rect, Sense, StrokeKind, pos2, vec2};
use egui_phosphor::regular as icon;
use phoecord_catalog::asks as words;

/// A row in one of the two lists.
struct Row {
    id: String,
    name: String,
    desc: String,
    meta: String,
    on: bool,
    package: bool,
    problem: Option<String>,
}

impl App {
    pub(crate) fn plugins_tab(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        widgets::h1(ui, self.tr.t("gui-plugins-title"));
        ui.add_space(theme::S1);
        ui.scope(|ui| {
            ui.set_max_width(560.0f32.min(ui.available_width()));
            widgets::muted(ui, self.tr.t("gui-plugins-sub"));
        });
        ui.add_space(theme::S4);
        let from_file = self.tr.t("gui-plugins-from-file");
        let from_link = self.tr.t("gui-plugins-from-link");
        let browse = self.tr.t("gui-plugins-browse");
        match button_row(
            ui,
            vec![
                Button::new(&from_file, Kind::Primary)
                    .icon(icon::FILE_ZIP)
                    .busy(self.plugin_busy),
                Button::new(&from_link, Kind::Secondary)
                    .icon(icon::LINK)
                    .enabled(!self.plugin_busy),
                Button::new(&browse, Kind::Secondary).icon(icon::STOREFRONT),
            ],
        ) {
            Some(0) => {
                if let Some(path) = rfd::FileDialog::new()
                    .set_title(self.tr.t("gui-plugins-pick-title"))
                    .add_filter(
                        self.tr.t("gui-plugins-pick-filter"),
                        &["zip", "toml", "wasm"],
                    )
                    .pick_file()
                {
                    self.stage_path(path);
                }
            }
            Some(1) => self.open_dialog(Dialog::Link {
                url: String::new(),
                error: None,
                busy: false,
            }),
            Some(2) => self.toggle_catalog(),
            _ => {}
        }
        ui.add_space(theme::S1);
        widgets::caption(ui, self.tr.t("gui-plugins-drop-hint"));
        ui.add_space(theme::S5);
        self.catalog_section(ui);

        // Search.
        let hint = format!(
            "{}  {}",
            icon::MAGNIFYING_GLASS,
            self.tr.t("gui-plugins-search")
        );
        let resp = ui.add(
            egui::TextEdit::singleline(&mut self.search)
                .hint_text(egui::RichText::new(hint).color(color::FAINT))
                .font(theme::regular(theme::T_BODY))
                .margin(egui::Margin::symmetric(12, 9))
                .desired_width(ui.available_width()),
        );
        if resp.has_focus() {
            ui.painter().rect_stroke(
                resp.rect,
                theme::radius(theme::R_CONTROL),
                theme::stroke(1.5, color::ROSE),
                StrokeKind::Inside,
            );
        }
        ui.add_space(theme::S4);

        let q = self.search.trim().to_lowercase();
        let matches = |r: &Row| {
            q.is_empty()
                || r.name.to_lowercase().contains(&q)
                || r.id.contains(&q)
                || r.desc.to_lowercase().contains(&q)
        };
        let packages: Vec<Row> = self
            .packages
            .iter()
            .map(|p| match &p.manifest {
                Some(m) => Row {
                    id: p.id.clone(),
                    name: m.manifest.name.clone(),
                    desc: m.manifest.description.clone(),
                    meta: self.by_line(&m.manifest.version, &m.manifest.authors),
                    on: self.config.is_enabled(&p.id),
                    package: true,
                    problem: None,
                },
                None => Row {
                    id: p.id.clone(),
                    name: p.id.clone(),
                    desc: String::new(),
                    meta: String::new(),
                    on: false,
                    package: true,
                    problem: p.problem.clone(),
                },
            })
            .filter(|r| matches(r))
            .collect();
        let builtin: Vec<Row> = self
            .builtin
            .iter()
            .map(|m| Row {
                id: m.id.clone(),
                name: m.name.clone(),
                desc: self.tr.plugin_desc(&m.id, &m.description),
                // The section title already says "Built in".
                meta: String::new(),
                on: self.config.is_enabled(&m.id),
                package: false,
                problem: None,
            })
            .filter(|r| matches(r))
            .collect();

        self.section_title(ui, &self.tr.t("gui-plugins-yours"), packages.len());
        if packages.is_empty() {
            let msg = if q.is_empty() {
                self.tr.t("gui-plugins-none")
            } else {
                self.tr.t("gui-plugins-no-match")
            };
            egui::Frame::new()
                .stroke(theme::stroke(1.0, color::LINE))
                .corner_radius(theme::radius(theme::R_TILE))
                .inner_margin(egui::Margin::symmetric(16, 18))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(icon::PUZZLE_PIECE)
                                .font(theme::icon(20.0))
                                .color(color::MUTED),
                        );
                        ui.add_space(theme::S1);
                        widgets::muted(ui, msg);
                    });
                });
        }
        for r in &packages {
            self.plugin_row(ui, r);
        }
        // Only `--demo` has built-ins here: the real ones are managed inside Discord.
        if !self.builtin.is_empty() {
            ui.add_space(theme::S5);
            self.section_title(ui, &self.tr.t("gui-plugins-builtin"), builtin.len());
            if builtin.is_empty() {
                widgets::muted(ui, self.tr.t("gui-plugins-no-match"));
            }
            for r in &builtin {
                self.plugin_row(ui, r);
            }
        }
        let _ = ctx;
    }

    pub(crate) fn by_line(&self, version: &str, authors: &[String]) -> String {
        if authors.is_empty() {
            self.tr
                .f("gui-plugins-meta-version", &[("version", version)])
        } else {
            self.tr.f(
                "gui-plugins-meta",
                &[("version", version), ("authors", &authors.join(", "))],
            )
        }
    }

    fn section_title(&self, ui: &mut egui::Ui, title: &str, count: usize) {
        ui.horizontal(|ui| {
            widgets::title(ui, title);
            widgets::text(
                ui,
                count.to_string(),
                theme::regular(theme::T_TITLE),
                color::FAINT,
            );
        });
        ui.add_space(theme::S2);
    }

    /// A plugin row. It grows in when it first appears and collapses before its package is
    /// removed.
    fn plugin_row(&mut self, ui: &mut egui::Ui, r: &Row) {
        let ctx = ui.ctx().clone();
        let id = Id::new(("plugin-row", r.package, &r.id));
        let leaving = self.leaving.contains(&r.id) && r.package;
        let t = motion::presence(&ctx, id.with("in"), !leaving, dur::ROW, dur::EXIT);
        if leaving && t <= 0.001 {
            return;
        }
        let full_h = 64.0;
        let h = (full_h + theme::S2) * t;
        let w = ui.available_width();
        let (slot, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
        let rect = Rect::from_min_size(slot.min + vec2(0.0, 6.0 * (1.0 - t)), vec2(w, full_h));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).id_salt(id));
        child.set_clip_rect(slot.intersect(ui.clip_rect()));
        child.set_opacity(t);
        let ui = &mut child;
        let hover = ui.rect_contains_pointer(rect);
        let hv = motion::tween(
            &ctx,
            id.with("h"),
            hover as u8 as f32,
            dur::HOVER,
            motion::out_cubic,
        );
        ui.painter().rect(
            rect,
            theme::radius(theme::R_TILE),
            theme::lerp_color(color::SURFACE, color::RAISED, hv * 0.55),
            theme::stroke(1.0, theme::lerp_color(color::LINE, color::LINE_STRONG, hv)),
            StrokeKind::Inside,
        );
        let inner = rect.shrink2(vec2(16.0, 12.0));
        // Right side: switch, then (packages) a remove button.
        let sw = Rect::from_min_size(
            pos2(inner.right() - 40.0, inner.center().y - 12.0),
            vec2(40.0, 24.0),
        );
        let mut text_right = sw.left() - theme::S4;
        if r.problem.is_none() {
            let mut c = ui.new_child(egui::UiBuilder::new().max_rect(sw));
            if widgets::switch(&mut c, r.on, &r.name).clicked() {
                self.toggle(&ctx, &r.id, !r.on);
            }
        } else {
            text_right = inner.right();
        }
        if r.package {
            let b = Rect::from_min_size(
                pos2(text_right - 32.0, inner.center().y - 16.0),
                vec2(32.0, 32.0),
            );
            let mut c = ui.new_child(egui::UiBuilder::new().max_rect(b));
            let label = self.tr.f("gui-plugins-remove-label", &[("name", &r.name)]);
            if widgets::icon_button(&mut c, icon::TRASH, &label, true).clicked() {
                self.open_dialog(Dialog::RemovePlugin {
                    id: r.id.clone(),
                    name: r.name.clone(),
                });
            }
            text_right = b.left() - theme::S2;
        }
        // Left side: name and meta on one line, description below (one line, cut with an
        // ellipsis; the full text is in the tooltip).
        let p = ui.painter();
        let name_g = p.layout_no_wrap(r.name.clone(), theme::semibold(theme::T_BODY), color::TEXT);
        let name_w = name_g.size().x;
        p.galley(inner.left_top(), name_g, color::TEXT);
        let mut x = inner.left() + name_w + theme::S2;
        if r.problem.is_some() {
            let mut c = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_size(
                pos2(x, inner.top() - 2.0),
                vec2(200.0, 22.0),
            )));
            widgets::pill(
                &mut c,
                id.with("bad"),
                &self.tr.t("gui-plugins-broken"),
                Tone::Warn,
            );
        } else if !r.meta.is_empty() {
            let meta = elide(
                ui,
                &r.meta,
                theme::regular(theme::T_CAPTION),
                text_right - x,
            );
            ui.painter().text(
                pos2(x, inner.top() + 1.5),
                Align2::LEFT_TOP,
                meta,
                theme::regular(theme::T_CAPTION),
                color::FAINT,
            );
            x = 0.0;
        }
        let _ = x;
        let desc = r.problem.clone().unwrap_or_else(|| r.desc.clone());
        let shown = elide(
            ui,
            &desc,
            theme::regular(theme::T_CAPTION),
            text_right - inner.left(),
        );
        let truncated = shown != desc;
        ui.painter().text(
            pos2(inner.left(), inner.bottom()),
            Align2::LEFT_BOTTOM,
            shown,
            theme::regular(theme::T_CAPTION),
            color::MUTED,
        );
        if truncated {
            let area = Rect::from_x_y_ranges(inner.left()..=text_right, inner.y_range());
            ui.interact(area, id.with("tip"), Sense::hover())
                .on_hover_text(desc);
        }
    }

    fn toggle(&mut self, ctx: &egui::Context, id: &str, on: bool) {
        match self.store.set_enabled(id, on) {
            Ok(()) => {
                self.reload_plugins();
                if on && self.gateway.contains(id) {
                    let t = self.tr.t("gui-toast-gateway");
                    self.toast(ctx, t, Tone::Neutral);
                }
            }
            Err(e) => {
                let t = self.package_error(&e);
                self.toast(ctx, t, Tone::Danger);
            }
        }
    }

    // -----------------------------------------------------------------------------------------
    // Dialogs
    // -----------------------------------------------------------------------------------------

    /// Draw the open dialog; false when it should close.
    pub(crate) fn dialog_body(&mut self, ui: &mut egui::Ui, d: &mut Dialog) -> bool {
        let ctx = ui.ctx().clone();
        match d {
            Dialog::Plugin(c) => {
                let m = &c.manifest.manifest;
                widgets::title(
                    ui,
                    self.tr.f("gui-confirm-plugin-title", &[("name", &m.name)]),
                );
                widgets::caption(ui, self.by_line(&m.version, &m.authors));
                if let Some(old) = &c.replaces {
                    widgets::caption(
                        ui,
                        self.tr
                            .f("gui-confirm-plugin-replaces", &[("version", old)]),
                    );
                }
                if !m.description.is_empty() {
                    ui.add_space(theme::S2);
                    widgets::body(ui, &m.description);
                }
                ui.add_space(theme::S4);
                self.asks_view(ui, &c.asks);
                ui.add_space(theme::S3);
                widgets::caption(ui, self.tr.t("gui-confirm-plugin-trust"));
                ui.add_space(theme::S4);
                let install = self.tr.t("gui-confirm-install");
                let cancel = self.tr.t("gui-cancel");
                match button_row(
                    ui,
                    vec![
                        Button::new(&install, Kind::Primary).icon(icon::DOWNLOAD_SIMPLE),
                        Button::new(&cancel, Kind::Ghost),
                    ],
                ) {
                    Some(0) => {
                        let (store, c) = (self.store.clone(), (**c).clone());
                        self.plugin_busy = true;
                        self.backend.plugin_job("install-plugin", move || {
                            packages::Msg::Installed(store.commit(&c))
                        });
                        false
                    }
                    Some(1) => {
                        self.store.discard(c);
                        false
                    }
                    _ => true,
                }
            }
            Dialog::Link { url, error, busy } => {
                widgets::title(ui, self.tr.t("gui-link-title"));
                widgets::caption(ui, self.tr.t("gui-link-sub"));
                ui.add_space(theme::S4);
                let resp = widgets::field(
                    ui,
                    &self.tr.t("gui-link-label"),
                    url,
                    "https://…/plugin.zip",
                );
                if !*busy && ui.memory(|m| m.focused().is_none()) {
                    resp.request_focus();
                }
                if let Some(e) = error {
                    ui.add_space(theme::S1);
                    widgets::text(
                        ui,
                        e.clone(),
                        theme::regular(theme::T_CAPTION),
                        color::DANGER,
                    );
                }
                ui.add_space(theme::S4);
                let go = self.tr.t("gui-link-download");
                let cancel = self.tr.t("gui-cancel");
                let enter = resp.lost_focus() && ctx.input(|i| i.key_pressed(egui::Key::Enter));
                let hit = button_row(
                    ui,
                    vec![
                        Button::new(&go, Kind::Primary)
                            .icon(icon::DOWNLOAD_SIMPLE)
                            .busy(*busy),
                        Button::new(&cancel, Kind::Ghost),
                    ],
                );
                if (hit == Some(0) || enter) && !*busy {
                    if packages::is_https(url) {
                        *busy = true;
                        *error = None;
                        let u = url.clone();
                        self.stage_url(u);
                    } else {
                        *error = Some(self.tr.t("gui-pkg-err-https"));
                    }
                }
                // A staged package replaces this dialog with the confirm one.
                hit != Some(1)
            }
            Dialog::RemovePlugin { id, name } => {
                widgets::title(ui, self.tr.f("gui-remove-title", &[("name", name)]));
                ui.add_space(theme::S2);
                widgets::muted(ui, self.tr.t("gui-remove-body"));
                ui.add_space(theme::S5);
                let rm = self.tr.t("gui-remove");
                let cancel = self.tr.t("gui-cancel");
                match button_row(
                    ui,
                    vec![
                        Button::new(&rm, Kind::Primary).icon(icon::TRASH),
                        Button::new(&cancel, Kind::Ghost),
                    ],
                ) {
                    Some(0) => {
                        self.leaving.insert(id.clone());
                        let (store, id) = (self.store.clone(), id.clone());
                        self.backend.plugin_job("remove-plugin", move || {
                            // Let the row collapse first.
                            std::thread::sleep(std::time::Duration::from_millis(160));
                            packages::Msg::Removed(store.remove(&id))
                        });
                        false
                    }
                    Some(1) => false,
                    _ => true,
                }
            }
            Dialog::Uninstall => {
                widgets::title(ui, self.tr.t("gui-uninstall-title"));
                ui.add_space(theme::S2);
                widgets::muted(ui, self.tr.t("gui-uninstall-body"));
                if let Some(key) = crate::tabs::install::app_stays_key() {
                    ui.add_space(theme::S2);
                    widgets::muted(ui, self.tr.t(key));
                }
                ui.add_space(theme::S5);
                let un = self.tr.t("gui-uninstall");
                let cancel = self.tr.t("gui-cancel");
                match button_row(
                    ui,
                    vec![
                        Button::new(&un, Kind::Primary).icon(icon::TRASH),
                        Button::new(&cancel, Kind::Ghost),
                    ],
                ) {
                    Some(0) => {
                        self.confirm_uninstall();
                        false
                    }
                    Some(1) => false,
                    _ => true,
                }
            }
            Dialog::StartFailed(f) => {
                widgets::title(ui, self.tr.t("gui-start-failed-title"));
                ui.add_space(theme::S2);
                if f.said.is_empty() {
                    widgets::muted(ui, self.tr.t("gui-start-failed-silent"));
                } else {
                    widgets::muted(ui, self.tr.t("gui-start-failed-body"));
                    ui.add_space(theme::S2);
                    widgets::text(
                        ui,
                        f.said.clone(),
                        egui::FontId::monospace(theme::T_CAPTION),
                        color::DANGER,
                    );
                }
                if let Some(log) = &f.log {
                    ui.add_space(theme::S2);
                    widgets::caption(
                        ui,
                        self.tr.f(
                            "gui-start-failed-log",
                            &[("path", &log.display().to_string())],
                        ),
                    );
                }
                ui.add_space(theme::S5);
                let open = self.tr.t("gui-open-log");
                let close = self.tr.t("gui-close");
                let mut buttons = Vec::new();
                if f.log.is_some() {
                    buttons.push(Button::new(&open, Kind::Ghost).icon(icon::FILE_TEXT));
                }
                buttons.push(Button::new(&close, Kind::Primary));
                let hit = button_row(ui, buttons);
                match (hit, &f.log) {
                    (Some(0), Some(log)) => {
                        if let Err(e) = crate::backend::open_path(log) {
                            let t = self.tr.f("gui-open-failed", &[("why", &format!("{e:#}"))]);
                            self.toast(ui.ctx(), t, Tone::Danger);
                        }
                        true
                    }
                    (Some(_), _) => false,
                    (None, _) => true,
                }
            }
        }
    }

    /// What a plugin will see and may do, in plain words.
    fn asks_view(&self, ui: &mut egui::Ui, asks: &Asks) {
        widgets::text(
            ui,
            self.tr.t("gui-asks-sees"),
            theme::semibold(theme::T_CAPTION + 0.5),
            color::TEXT,
        );
        ui.add_space(theme::S1);
        if asks.events.is_empty() {
            self.ask_line(
                ui,
                icon::INFO,
                &self.tr.t("gui-asks-sees-nothing"),
                color::MUTED,
            );
        }
        for e in &asks.events {
            let text = words::event_text(e, |k| self.tr.t(k));
            self.ask_line(ui, icon::EYE, &text, color::MUTED);
        }
        ui.add_space(theme::S3);
        widgets::text(
            ui,
            self.tr.t("gui-asks-can"),
            theme::semibold(theme::T_CAPTION + 0.5),
            color::TEXT,
        );
        ui.add_space(theme::S1);
        if asks.rights.is_empty() {
            self.ask_line(
                ui,
                icon::SHIELD_CHECK,
                &self.tr.t("gui-asks-can-nothing"),
                color::OK,
            );
        }
        for r in &asks.rights {
            let text = self
                .tr_opt(&words::right_key(r))
                .unwrap_or_else(|| r.clone());
            let strong = words::is_strong(r);
            self.ask_line(
                ui,
                if strong { icon::WARNING } else { icon::CHECK },
                &text,
                if strong { color::WARN } else { color::MUTED },
            );
        }
        if !asks.fetch.is_empty() {
            ui.add_space(theme::S3);
            widgets::text(
                ui,
                self.tr.t("gui-asks-connects"),
                theme::semibold(theme::T_CAPTION + 0.5),
                color::TEXT,
            );
            ui.add_space(theme::S1);
            for h in &asks.fetch {
                self.ask_line(ui, icon::GLOBE, h, color::MUTED);
            }
        }
    }

    fn ask_line(&self, ui: &mut egui::Ui, glyph: &str, text: &str, col: egui::Color32) {
        ui.horizontal_top(|ui| {
            ui.label(
                egui::RichText::new(glyph)
                    .font(theme::icon(15.0))
                    .color(col),
            );
            ui.add_space(2.0);
            widgets::text(
                ui,
                text,
                theme::regular(theme::T_CAPTION + 0.5),
                if col == color::MUTED {
                    color::TEXT
                } else {
                    col
                },
            );
        });
    }

    fn tr_opt(&self, key: &str) -> Option<String> {
        self.tr.try_t(key)
    }
}

/// `s` cut to fit `width`, with an ellipsis.
fn elide(ui: &egui::Ui, s: &str, font: egui::FontId, width: f32) -> String {
    let fits = |t: &str| {
        ui.painter()
            .layout_no_wrap(t.to_string(), font.clone(), color::TEXT)
            .size()
            .x
            <= width
    };
    if fits(s) {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let (mut lo, mut hi) = (0, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let t: String = chars[..mid].iter().collect::<String>() + "…";
        if fits(&t) {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    chars[..lo]
        .iter()
        .collect::<String>()
        .trim_end()
        .to_string()
        + "…"
}
