//! First start: four short screens before the main window. What PhoeCord does, the terms (the
//! same screen as later updates of the terms), which Discord to use, and a last screen that starts
//! the install. Shown once per machine, on the first start after PhoeCord is installed (a marker
//! file in the data folder), not again after an update (ruling, 2026-11-26); when the terms were
//! accepted before, the terms screen is skipped.

use crate::app::{App, Tab};
use crate::motion::{self, dur};
use crate::theme::{self, color};
use crate::widgets::{self, Button, Kind};
use eframe::egui::{self, Align, Id, Layout, Rect, Sense, pos2, vec2};
use egui_phosphor::regular as icon;
use phoecord_installer::install::Phase;
use phoecord_installer::system::Os;
use std::path::Path;

pub const STEPS: u8 = 4;
pub const WELCOME: u8 = 0;
pub const TERMS: u8 = 1;
pub const PICK: u8 = 2;
pub const DONE: u8 = 3;

/// The file in the data folder that records that the onboarding was finished (it holds the version
/// that finished it, for support; any version counts).
const MARKER: &str = "onboarded";

/// The onboarding is due: no version of PhoeCord has finished it on this machine. An update,
/// automatic or by hand, never shows it again; a marker that cannot be read counts as missing.
pub fn needed(data: &Path) -> bool {
    std::fs::read_to_string(data.join(MARKER)).is_err()
}

/// Record that this version finished the onboarding.
pub fn mark_done(data: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(data)?;
    std::fs::write(data.join(MARKER), env!("CARGO_PKG_VERSION"))
}

/// The progress strip and its gap above the screen.
const STRIP_H: f32 = 44.0;
const BAR_H: f32 = 84.0;

impl App {
    /// Move to `step`, remembering the direction for the slide.
    pub fn onboard_to(&mut self, ctx: &egui::Context, step: u8) {
        if let Some(cur) = self.onboarding {
            self.onboard_back = step < cur;
        }
        self.onboarding = Some(step);
        self.terms_at_end = false;
        motion::forget(ctx, Id::new("onboard-enter"));
    }

    pub fn onboarding_screen(&mut self, ui: &mut egui::Ui, full: Rect, step: u8) {
        let ctx = ui.ctx().clone();
        self.progress_strip(ui, full, step);
        let area = Rect::from_min_max(pos2(full.left(), full.top() + STRIP_H), full.max);
        let t = motion::tween_from(
            &ctx,
            Id::new("onboard-enter"),
            0.0,
            1.0,
            dur::TAB,
            motion::out_quint,
        );
        let dir = if self.onboard_back { -1.0 } else { 1.0 };
        let shifted = area.translate(vec2(dir * 24.0 * (1.0 - t), 0.0));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(shifted));
        child.set_clip_rect(area);
        child.set_opacity(t);
        match step {
            TERMS => self.terms_screen(&mut child, shifted),
            WELCOME => self.welcome(&mut child, shifted),
            PICK => self.pick(&mut child, shifted),
            _ => self.done(&mut child, shifted),
        }
    }

    /// Four segments; the current one fills from the left.
    fn progress_strip(&self, ui: &mut egui::Ui, full: Rect, step: u8) {
        let ctx = ui.ctx().clone();
        let w = 640.0f32.min(full.width() - 48.0);
        let gap = 6.0;
        let seg = (w - gap * (STEPS as f32 - 1.0)) / STEPS as f32;
        let left = full.center().x - w / 2.0;
        let y = full.top() + 22.0;
        let p = ui.painter();
        for i in 0..STEPS {
            let r = Rect::from_min_size(pos2(left + i as f32 * (seg + gap), y), vec2(seg, 3.0));
            p.rect_filled(r, 2.0, color::LINE);
            let target = if i <= step { 1.0 } else { 0.0 };
            let f = motion::tween(
                &ctx,
                Id::new(("onboard-seg", i)),
                target,
                dur::ROW,
                motion::out_cubic,
            );
            if f > 0.0 {
                let fill = Rect::from_min_size(r.min, vec2(seg * f, r.height()));
                p.rect_filled(fill, 2.0, color::ROSE);
            }
        }
        let label = self.tr.f(
            "gui-onboard-step",
            &[
                ("n", &(step + 1).to_string()),
                ("total", &STEPS.to_string()),
            ],
        );
        p.text(
            pos2(left + w, y + 10.0),
            egui::Align2::RIGHT_TOP,
            label,
            theme::regular(theme::T_CAPTION),
            color::FAINT,
        );
    }

    /// The column the text sits in, and the bottom bar under it.
    fn column(full: Rect) -> (Rect, Rect) {
        let w = 640.0f32.min(full.width() - 48.0);
        let left = full.center().x - w / 2.0;
        let body = Rect::from_min_max(
            pos2(left, full.top() + 28.0),
            pos2(left + w, full.bottom() - BAR_H),
        );
        let bar = Rect::from_min_max(
            pos2(left, full.bottom() - BAR_H),
            pos2(left + w, full.bottom()),
        );
        (body, bar)
    }

    /// The text column of a screen: clipped at the bottom bar's line and scrolled when the window
    /// is shorter than the screen, so nothing is drawn under the bar.
    fn body_column(ui: &mut egui::Ui, body: Rect, id: &str, add: impl FnOnce(&mut egui::Ui)) {
        let mut outer = ui.new_child(egui::UiBuilder::new().max_rect(body));
        outer.set_clip_rect(body.intersect(ui.clip_rect()));
        egui::ScrollArea::vertical()
            .id_salt(id)
            .auto_shrink([false, true])
            .max_height(body.height())
            .show(&mut outer, add);
    }
    /// The bottom bar: an optional Back on the left, the main action on the right.
    fn bottom_bar(
        &self,
        ui: &mut egui::Ui,
        bar: Rect,
        back: bool,
        main: Button<'_>,
        extra: Option<Button<'_>>,
    ) -> (bool, bool, bool) {
        ui.painter().line_segment(
            [bar.left_top(), bar.right_top()],
            theme::stroke(1.0, color::LINE),
        );
        let mut b = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(bar.shrink2(vec2(0.0, 18.0)))
                .layout(Layout::left_to_right(Align::Center)),
        );
        let mut hit = (false, false, false);
        let back_label = self.tr.t("gui-onboard-back");
        if back
            && Button::new(&back_label, Kind::Ghost)
                .icon(icon::ARROW_LEFT)
                .show(&mut b)
                .clicked()
        {
            hit.0 = true;
        }
        b.with_layout(Layout::right_to_left(Align::Center), |ui| {
            hit.1 = main.show(ui).clicked();
            if let Some(e) = extra {
                hit.2 = e.show(ui).clicked();
            }
        });
        hit
    }

    fn welcome(&mut self, ui: &mut egui::Ui, full: Rect) {
        let ctx = ui.ctx().clone();
        let (body, bar) = Self::column(full);
        Self::body_column(ui, body, "onboard-welcome-body", |c| {
            let (mark, _) = c.allocate_exact_size(vec2(56.0, 56.0), Sense::hover());
            theme::paint_mark(c.painter(), mark);
            c.add_space(theme::S4);
            widgets::h1(c, self.tr.t("gui-onboard-welcome-title"));
            c.add_space(theme::S1);
            widgets::muted(c, self.tr.t("gui-onboard-welcome-sub"));
            c.add_space(theme::S6);
            let points = [
                (icon::SHIELD_CHECK, "gui-onboard-point-proxy"),
                (icon::PUZZLE_PIECE, "gui-onboard-point-plugins"),
                (icon::ARROW_COUNTER_CLOCKWISE, "gui-onboard-point-undo"),
            ];
            for (i, (glyph, key)) in points.into_iter().enumerate() {
                // The points arrive one after another, 60 ms apart.
                let age = motion::age(&ctx, Id::new("onboard-welcome"));
                let a = motion::out_quint(((age - 0.06 * i as f32) / dur::ROW).clamp(0.0, 1.0));
                if a < 1.0 {
                    ctx.request_repaint();
                }
                c.scope(|ui| {
                    ui.set_opacity(a);
                    ui.horizontal_top(|ui| {
                        let (r, _) = ui.allocate_exact_size(vec2(36.0, 36.0), Sense::hover());
                        ui.painter()
                            .rect_filled(r, theme::radius(theme::R_CONTROL), color::RAISED);
                        ui.painter().text(
                            r.center(),
                            egui::Align2::CENTER_CENTER,
                            glyph,
                            theme::icon(18.0),
                            color::ROSE,
                        );
                        ui.add_space(theme::S3);
                        ui.vertical(|ui| {
                            ui.set_max_width(520.0f32.min(ui.available_width()));
                            widgets::body(ui, self.tr.t(&format!("{key}-title")));
                            widgets::muted(ui, self.tr.t(key));
                        });
                    });
                });
                c.add_space(theme::S4);
            }
        });
        let start = self.tr.t("gui-onboard-start");
        let lang = if self.tr.lang() == "ru" {
            "English"
        } else {
            "Русский"
        };
        let (_, next, swap) = self.bottom_bar(
            ui,
            bar,
            false,
            Button::new(&start, Kind::Primary),
            Some(Button::new(lang, Kind::Ghost).icon(icon::GLOBE)),
        );
        if next {
            self.onboard_to(&ctx, TERMS);
        }
        if swap {
            let to = if self.tr.lang() == "ru" { "en" } else { "ru" };
            self.tr = crate::text::Tr::new(Some(to));
        }
    }

    fn pick(&mut self, ui: &mut egui::Ui, full: Rect) {
        let ctx = ui.ctx().clone();
        let (body, bar) = Self::column(full);
        let mut c = ui.new_child(egui::UiBuilder::new().max_rect(body));
        c.set_clip_rect(body.intersect(ui.clip_rect()));
        widgets::h1(&mut c, self.tr.t("gui-install-title"));
        c.add_space(theme::S1);
        widgets::muted(&mut c, self.tr.t("gui-onboard-pick-sub"));
        c.add_space(theme::S5);
        let status = self.status.clone();
        let mut has_clients = false;
        egui::ScrollArea::vertical()
            .id_salt("onboard-pick")
            .auto_shrink([false, true])
            .show(&mut c, |ui| match &status {
                None => {
                    widgets::muted(ui, self.tr.t("gui-onboard-looking"));
                }
                Some(s) if s.clients.is_empty() => self.no_clients(ui, true),
                Some(s) => {
                    has_clients = true;
                    self.client_tiles(ui, &s.clients, s);
                    ui.add_space(theme::S4);
                    self.elsewhere_row(ui);
                }
            });
        let next = self.tr.t("gui-onboard-next");
        let skip = self.tr.t("gui-onboard-skip");
        let ready = has_clients && self.selected.is_some();
        let (_, go, skipped) = self.bottom_bar(
            ui,
            bar,
            false,
            Button::new(&next, Kind::Primary).enabled(ready),
            Some(Button::new(&skip, Kind::Ghost)),
        );
        if go {
            self.onboard_to(&ctx, DONE);
        }
        if skipped {
            self.finish_onboarding(&ctx, false);
        }
    }

    fn done(&mut self, ui: &mut egui::Ui, full: Rect) {
        let ctx = ui.ctx().clone();
        let (body, bar) = Self::column(full);
        Self::body_column(ui, body, "onboard-done-body", |c| {
            // A check that draws itself once, then the words.
            let (r, _) = c.allocate_exact_size(vec2(56.0, 56.0), Sense::hover());
            let age = motion::age(&ctx, Id::new("onboard-done"));
            let ring = motion::out_quint((age / 0.3).clamp(0.0, 1.0));
            c.painter().circle_filled(
                r.center(),
                28.0 * (0.9 + 0.1 * ring),
                theme::fade(color::OK_BG, ring),
            );
            let check = motion::out_cubic(((age - 0.12) / dur::CHECK).clamp(0.0, 1.0));
            widgets::check_mark(c.painter(), r.shrink(17.0), check, color::OK, 3.0);
            if age < 0.5 {
                ctx.request_repaint();
            }
            c.add_space(theme::S4);
            let name = self
                .selected
                .map(|ch| self.channel_name(ch))
                .unwrap_or_default();
            widgets::h1(c, self.tr.t("gui-onboard-done-title"));
            c.add_space(theme::S1);
            c.scope(|ui| {
                ui.set_max_width(560.0f32.min(ui.available_width()));
                widgets::muted(ui, self.tr.f("gui-onboard-done-sub", &[("name", &name)]));
            });
            c.add_space(theme::S5);
            // What the install will do, in the words the progress screen uses.
            egui::Frame::new()
                .fill(color::SURFACE)
                .stroke(theme::stroke(1.0, color::LINE))
                .corner_radius(theme::radius(theme::R_TILE))
                .inner_margin(egui::Margin::symmetric(20, 16))
                .show(c, |ui| {
                    ui.set_width(ui.available_width());
                    widgets::caption(ui, self.tr.t("gui-onboard-done-what"));
                    ui.add_space(theme::S2);
                    let mac = Os::current() == Os::MacOs;
                    for (ph, glyph) in [
                        (Phase::Certificate, icon::CERTIFICATE),
                        (Phase::Autostart, icon::POWER),
                        (Phase::Shortcut, icon::ROCKET_LAUNCH),
                    ] {
                        let (title, sub) =
                            self.phase_text(ph, mac && ph == Phase::Shortcut, self.selected);
                        ui.horizontal_top(|ui| {
                            ui.label(
                                egui::RichText::new(glyph)
                                    .font(theme::icon(18.0))
                                    .color(color::ROSE),
                            );
                            ui.add_space(theme::S2);
                            ui.vertical(|ui| {
                                widgets::body(ui, title);
                                widgets::caption(ui, sub);
                            });
                        });
                        ui.add_space(theme::S2);
                    }
                });
        });
        let install = self.tr.t("gui-install-action");
        let later = self.tr.t("gui-onboard-later");
        let (back, go, later_hit) = self.bottom_bar(
            ui,
            bar,
            true,
            Button::new(&install, Kind::Primary).icon(icon::DOWNLOAD_SIMPLE),
            Some(Button::new(&later, Kind::Ghost)),
        );
        if back {
            motion::forget(&ctx, Id::new("onboard-done"));
            self.onboard_to(&ctx, PICK);
        }
        if go {
            self.finish_onboarding(&ctx, true);
        }
        if later_hit {
            self.finish_onboarding(&ctx, false);
        }
    }

    /// Leave the onboarding for the Install tab, starting the install when asked to.
    pub fn finish_onboarding(&mut self, ctx: &egui::Context, install: bool) {
        self.onboarding = None;
        // Not fatal: without the marker the onboarding shows again on the next start.
        let _ = mark_done(&self.backend.mode.paths().data);
        self.go(ctx, Tab::Install);
        motion::forget(ctx, Id::new("tab-enter"));
        if install {
            self.start_install(self.selected, false);
        }
    }
}

// The steps are in order and DONE is the last one.
const _: () = assert!(WELCOME < TERMS && TERMS < PICK && PICK < DONE && DONE + 1 == STEPS);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shown_once_per_machine_not_after_updates() {
        let t = tempfile::tempdir().unwrap();
        let data = t.path().join("data/phoecord");
        assert!(needed(&data), "nothing recorded yet");
        mark_done(&data).unwrap();
        assert!(!needed(&data));
        // Finished by an older version, then PhoeCord updated itself: not shown again.
        std::fs::write(data.join(MARKER), "0.5.0\n").unwrap();
        assert!(!needed(&data));
    }
}
