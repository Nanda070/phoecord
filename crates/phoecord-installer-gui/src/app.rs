//! The window: header with tabs, the tab content with its transition, dialogs, toasts, the terms
//! gate, and the messages coming back from background jobs.

use crate::backend::{Backend, Msg, Proxy, Status};
use crate::motion::{self, dur};
use crate::packages::{self, Candidate, Installed, Store};
use crate::text::Tr;
use crate::theme::{self, color};
use crate::widgets::{self, Button, Tone};
use eframe::egui::{self, Align2, Id, Rect, Sense, StrokeKind, pos2, vec2};
use egui_phosphor::regular as icon;
use phoecord_core::Manifest;
use phoecord_installer::Channel;
use phoecord_installer::install::{Phase, PhaseState};
use phoecord_installer::uninstall::UninstallReport;
use phoecord_proxy::config::Config;
use std::collections::HashSet;
use std::path::Path;
use std::sync::mpsc::Receiver;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Install,
    Plugins,
    Settings,
    About,
}

impl Tab {
    pub const ALL: [Tab; 4] = [Tab::Install, Tab::Plugins, Tab::Settings, Tab::About];
    fn index(self) -> usize {
        Tab::ALL.iter().position(|t| *t == self).unwrap()
    }
    fn key(self) -> &'static str {
        match self {
            Tab::Install => "gui-tab-install",
            Tab::Plugins => "gui-tab-plugins",
            Tab::Settings => "gui-tab-settings",
            Tab::About => "gui-tab-about",
        }
    }
}

/// One install step as the window shows it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PState {
    Waiting,
    Running,
    Done,
    Skipped,
    Failed,
}

/// Where the Install tab is.
pub enum Flow {
    Idle,
    Installing {
        channel: Option<Channel>,
        repair: bool,
        phases: [PState; 3],
    },
    Ready {
        channel: Option<Channel>,
        repair: bool,
    },
    Failed {
        channel: Option<Channel>,
        phases: [PState; 3],
        error: String,
        failed: Option<Phase>,
    },
    Uninstalling,
    Removed(UninstallReport),
    RemoveFailed(String),
}

pub enum Dialog {
    /// Confirm installing a staged plugin.
    Plugin(Box<Candidate>),
    /// "Install from link".
    Link {
        url: String,
        error: Option<String>,
        busy: bool,
    },
    RemovePlugin {
        id: String,
        name: String,
    },
    Uninstall,
    /// "Open Discord" failed: what `phoecord launch` said.
    StartFailed(Box<crate::backend::LaunchFailure>),
}

pub struct Toast {
    pub id: u64,
    pub text: String,
    pub tone: Tone,
    pub born: f64,
    pub ttl: f64,
}

pub struct App {
    pub backend: Backend,
    rx: Receiver<Msg>,
    pub tr: Tr,
    pub status: Option<Status>,
    pub tab: Tab,
    prev_tab: Tab,
    pub terms_needed: bool,
    /// First start: the onboarding step shown instead of the window (`crate::onboarding`).
    pub onboarding: Option<u8>,
    /// The last onboarding move went back (the screens slide the other way).
    pub onboard_back: bool,
    pub terms_viewing: bool,
    pub terms_at_end: bool,
    pub flow: Flow,
    pub selected: Option<Channel>,
    // Plugins tab.
    pub store: Store,
    pub packages: Vec<Installed>,
    pub builtin: Vec<Manifest>,
    pub gateway: HashSet<String>,
    pub config: Config,
    pub search: String,
    pub plugin_busy: bool,
    /// The community catalog (`tabs/catalog.rs`).
    pub catalog: crate::tabs::catalog::State,
    /// "Open Discord" was clicked and its first seconds are being watched.
    pub launching: bool,
    /// Rows collapsing before their package is removed.
    pub leaving: HashSet<String>,
    // Settings tab.
    pub upstream: String,
    pub upstream_msg: Option<(Tone, String)>,
    pub reduce_motion: bool,
    /// PhoeCord's own updates: the latest note and the last error (`crate::update`).
    pub update_note: Option<crate::update::Note>,
    pub update_error: Option<String>,
    pub dialog: Option<Dialog>,
    closing: Option<Dialog>,
    pub toasts: Vec<Toast>,
    next_toast: u64,
    /// `--open`: staged once the terms are accepted.
    pending_open: Option<std::path::PathBuf>,
}

const REDUCE_KEY: &str = "reduce_motion";

impl App {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        mode: crate::backend::Mode,
        notes: Vec<crate::update::Note>,
    ) -> App {
        theme::apply(&cc.egui_ctx);
        let reduce_motion = cc
            .storage
            .and_then(|s| s.get_string(REDUCE_KEY))
            .is_some_and(|v| v == "true");
        motion::set_reduced(&cc.egui_ctx, reduce_motion);
        let paths = mode.paths();
        let config = Config::load(&paths.config).unwrap_or_default();
        let tr = Tr::new(config.language.as_deref());
        let env = mode.env().ok();
        let terms_needed = env
            .as_ref()
            .is_none_or(phoecord_installer::terms::needs_acceptance);
        // The first start of this version, even when the terms were accepted before (an earlier
        // build, the CLI): the terms screen is skipped then (owner, PR #66 review).
        let onboarding =
            crate::onboarding::needed(&paths.data).then_some(crate::onboarding::WELCOME);
        let store = Store::new(&paths.data, &paths.config);
        // Built-in plugins are managed inside Discord (owner, PR #66 review); the window shows
        // them only in `--demo`, for screenshots, until that in-client tab exists.
        let chain = phoecord_plugin_runtime::Chain::new(if mode.demo {
            phoecord_plugins::builtin()
        } else {
            Vec::new()
        });
        let builtin = chain.manifests();
        let gateway = builtin
            .iter()
            .filter(|m| chain.uses_gateway(&m.id))
            .map(|m| m.id.clone())
            .collect();
        let (backend, rx) = Backend::start(mode, cc.egui_ctx.clone());
        let mut app = App {
            backend,
            rx,
            tr,
            status: None,
            tab: Tab::Install,
            prev_tab: Tab::Install,
            terms_needed,
            onboarding,
            onboard_back: false,
            terms_viewing: false,
            terms_at_end: false,
            flow: Flow::Idle,
            selected: None,
            store,
            packages: Vec::new(),
            builtin,
            gateway,
            upstream: config.upstream.clone().unwrap_or_default(),
            config,
            search: String::new(),
            plugin_busy: false,
            catalog: Default::default(),
            launching: false,
            leaving: HashSet::new(),
            upstream_msg: None,
            reduce_motion,
            update_note: None,
            update_error: None,
            dialog: None,
            closing: None,
            toasts: Vec::new(),
            next_toast: 0,
            pending_open: None,
        };
        app.reload_plugins();
        app.pending_open = app.backend.mode.open.clone();
        app.update_error = crate::update::last_error(&app.backend.mode);
        for n in notes {
            app.update_noted(&cc.egui_ctx, n);
        }
        crate::update::check_in_background(&app.backend);
        app
    }

    /// Show an update note as a toast and keep it for the Settings line.
    fn update_noted(&mut self, ctx: &egui::Context, n: crate::update::Note) {
        use crate::update::Note;
        let (text, tone) = match &n {
            Note::Applied { from, to } => (
                self.tr
                    .f("gui-update-applied", &[("from", from), ("to", to)]),
                Tone::Ok,
            ),
            Note::Failed(why) => (
                self.tr.f("gui-update-failed", &[("why", why)]),
                Tone::Danger,
            ),
            Note::Ready(v) => (
                self.tr.f("gui-update-ready", &[("version", v)]),
                Tone::Neutral,
            ),
            Note::Manual { version, .. } => (
                self.tr.f("gui-update-manual", &[("version", version)]),
                Tone::Neutral,
            ),
        };
        if let Note::Failed(why) = &n {
            self.update_error = Some(why.clone());
        }
        self.toast(ctx, text, tone);
        self.update_note = Some(n);
    }

    pub fn reload_plugins(&mut self) {
        self.packages = self.store.list();
        self.catalog.refresh_marks(&self.packages);
        if let Ok(c) = Config::load(&self.store.config) {
            self.config = c;
        }
    }

    pub fn toast(&mut self, ctx: &egui::Context, text: impl Into<String>, tone: Tone) {
        self.next_toast += 1;
        self.toasts.push(Toast {
            id: self.next_toast,
            text: text.into(),
            tone,
            born: ctx.input(|i| i.time),
            ttl: 4.0,
        });
        if self.toasts.len() > 3 {
            self.toasts.remove(0);
        }
    }

    pub fn open_dialog(&mut self, d: Dialog) {
        self.closing = None;
        self.dialog = Some(d);
    }

    /// Start `c` through PhoeCord, once at a time; the answer comes as [`Msg::Launched`].
    pub fn open_discord(&mut self, c: Channel) {
        if !self.launching {
            self.launching = true;
            self.backend.launch(c);
        }
    }

    pub fn close_dialog(&mut self) {
        self.closing = self.dialog.take();
    }

    pub fn go(&mut self, ctx: &egui::Context, tab: Tab) {
        if tab != self.tab {
            self.prev_tab = self.tab;
            self.tab = tab;
            motion::forget(ctx, Id::new("tab-enter"));
        }
    }

    /// The real check that a staged plugin's code loads, with one WASM runtime for the window.
    pub fn verify(dir: &Path) -> Result<(), String> {
        use phoecord_plugin_runtime::package::PackagePlugin;
        use phoecord_plugin_runtime::wasm::WasmRuntime;
        use std::sync::{Arc, OnceLock};
        static RT: OnceLock<Result<Arc<WasmRuntime>, String>> = OnceLock::new();
        let rt = RT
            .get_or_init(|| WasmRuntime::new().map_err(|e| e.to_string()))
            .clone()?;
        PackagePlugin::load(&rt, dir).map(|_| ())
    }

    /// Stage a package from a dropped or picked path, then show the confirm dialog.
    pub fn stage_path(&mut self, path: std::path::PathBuf) {
        self.plugin_busy = true;
        let store = self.store.clone();
        self.backend.plugin_job("stage", move || {
            packages::Msg::Staged(store.stage_path(&path, &App::verify).map(Box::new))
        });
    }

    pub fn stage_url(&mut self, url: String) {
        self.plugin_busy = true;
        let store = self.store.clone();
        self.backend.plugin_job("download", move || {
            packages::Msg::Staged(
                packages::download(&url)
                    .and_then(|b| store.stage_zip(&b, &App::verify))
                    .map(Box::new),
            )
        });
    }

    /// Plain words for a package error, with what to do next.
    pub fn package_error(&self, e: &packages::Error) -> String {
        use packages::Error as E;
        match e {
            E::NotAPackage => self.tr.t("gui-pkg-err-not-package"),
            E::NoWasm => self.tr.t("gui-pkg-err-no-wasm"),
            E::TooBig => self.tr.f(
                "gui-pkg-err-too-big",
                &[("mb", &(packages::MAX_BYTES / 1024 / 1024).to_string())],
            ),
            E::UnsafePath(p) => self.tr.f("gui-pkg-err-unsafe", &[("path", p)]),
            E::Manifest(why) => self.tr.f("gui-pkg-err-manifest", &[("why", why)]),
            E::Broken(why) => self.tr.f("gui-pkg-err-broken", &[("why", why)]),
            E::NotHttps => self.tr.t("gui-pkg-err-https"),
            E::Download(why) => self.tr.f("gui-pkg-err-download", &[("why", why)]),
            E::Io(why) => self.tr.f("gui-pkg-err-io", &[("why", why)]),
            E::Index(why) => self.tr.f("gui-pkg-err-index", &[("why", why)]),
            E::NotInCatalog(_) => self.tr.t("gui-pkg-err-not-in-catalog"),
            E::HashMismatch => self.tr.t("gui-pkg-err-hash"),
            E::CatalogMismatch(why) => self.tr.f("gui-pkg-err-catalog-mismatch", &[("why", why)]),
        }
    }

    fn handle(&mut self, ctx: &egui::Context, m: Msg) {
        match m {
            Msg::Update(n) => self.update_noted(ctx, n),
            Msg::Status(s) => {
                let s = *s;
                if self.selected.is_none_or(|c| s.client(c).is_none()) {
                    self.selected = s
                        .clients
                        .iter()
                        .find(|c| s.is_on(c.client.channel))
                        .or(s.clients.first())
                        .map(|c| c.client.channel);
                }
                self.status = Some(s);
                self.reload_plugins();
            }
            Msg::Install(p, st) => {
                if let Flow::Installing { phases, .. } = &mut self.flow {
                    let i = Phase::ALL.iter().position(|x| *x == p).unwrap();
                    phases[i] = match st {
                        PhaseState::Started => PState::Running,
                        PhaseState::Done => PState::Done,
                        PhaseState::Skipped => PState::Skipped,
                        PhaseState::Failed(_) => PState::Failed,
                    };
                }
            }
            Msg::InstallDone(r) => {
                let (channel, repair, phases) = match &self.flow {
                    Flow::Installing {
                        channel,
                        repair,
                        phases,
                    } => (*channel, *repair, *phases),
                    _ => (None, false, [PState::Waiting; 3]),
                };
                self.flow = match r {
                    Ok(()) => Flow::Ready { channel, repair },
                    Err(error) => Flow::Failed {
                        channel,
                        failed: phases
                            .iter()
                            .position(|p| *p == PState::Failed || *p == PState::Running)
                            .map(|i| Phase::ALL[i]),
                        phases: phases.map(|p| {
                            if p == PState::Running {
                                PState::Failed
                            } else {
                                p
                            }
                        }),
                        error,
                    },
                };
            }
            Msg::UninstallDone(r) => {
                self.flow = match r {
                    Ok(rep) => Flow::Removed(*rep),
                    Err(e) => Flow::RemoveFailed(e),
                };
            }
            Msg::Launched(r) => {
                self.launching = false;
                match r {
                    Ok(()) => {
                        let t = self.tr.t("gui-toast-starting");
                        self.toast(ctx, t, Tone::Ok)
                    }
                    Err(e) => self.open_dialog(Dialog::StartFailed(e)),
                }
            }
            Msg::Plugin(p) => {
                self.plugin_busy = false;
                match p {
                    packages::Msg::Staged(Ok(c)) => {
                        self.open_dialog(Dialog::Plugin(c));
                    }
                    packages::Msg::Staged(Err(e)) => {
                        let text = self.package_error(&e);
                        if let Some(Dialog::Link { error, busy, .. }) = &mut self.dialog {
                            *error = Some(text);
                            *busy = false;
                        } else {
                            self.toast(ctx, text, Tone::Danger);
                        }
                    }
                    packages::Msg::Installed(Ok(id)) => {
                        self.reload_plugins();
                        let name = self.package_name(&id);
                        let t = self.tr.f("gui-toast-plugin-installed", &[("name", &name)]);
                        self.toast(ctx, t, Tone::Ok);
                    }
                    packages::Msg::Removed(Ok(id)) => {
                        let name = self.package_name(&id);
                        self.reload_plugins();
                        self.leaving.remove(&id);
                        let t = self.tr.f("gui-toast-plugin-removed", &[("name", &name)]);
                        self.toast(ctx, t, Tone::Neutral);
                    }
                    packages::Msg::Index(r) => {
                        // The error's words are shown in the list.
                        let r = r.map(|i| *i).map_err(|e| self.package_error(&e));
                        self.catalog.loaded(r);
                    }
                    packages::Msg::Installed(Err(e)) | packages::Msg::Removed(Err(e)) => {
                        self.leaving.clear();
                        self.reload_plugins();
                        let t = self.package_error(&e);
                        self.toast(ctx, t, Tone::Danger);
                    }
                }
            }
        }
    }

    pub fn package_name(&self, id: &str) -> String {
        self.packages
            .iter()
            .find(|p| p.id == id)
            .and_then(|p| p.manifest.as_ref().map(|m| m.manifest.name.clone()))
            .unwrap_or_else(|| id.to_string())
    }

    pub fn channel_name(&self, c: Channel) -> String {
        self.tr.t(match c {
            Channel::Stable => "gui-client-stable",
            Channel::Ptb => "gui-client-ptb",
            Channel::Canary => "gui-client-canary",
            Channel::Phoenix => "gui-client-phoenix",
        })
    }

    // -----------------------------------------------------------------------------------------
    // Frame
    // -----------------------------------------------------------------------------------------

    fn header(&mut self, ui: &mut egui::Ui) {
        let full = ui.max_rect();
        let rect = Rect::from_min_size(full.min, vec2(full.width(), theme::HEADER_H));
        let p = ui.painter();
        // A soft garnet glow behind the header: the window's one decorative light.
        let glow =
            Rect::from_center_size(pos2(rect.center().x, rect.top() - 30.0), vec2(620.0, 200.0));
        let mut mesh = egui::epaint::Mesh::default();
        let n = 40;
        mesh.colored_vertex(glow.center(), theme::fade(color::ACCENT, 0.32));
        for i in 0..=n {
            let a = i as f32 / n as f32 * std::f32::consts::TAU;
            mesh.colored_vertex(
                glow.center() + vec2(a.cos() * glow.width() / 2.0, a.sin() * glow.height() / 2.0),
                egui::Color32::TRANSPARENT,
            );
        }
        for i in 0..n {
            mesh.add_triangle(0, 1 + i, 2 + i);
        }
        p.add(mesh);
        p.line_segment(
            [rect.left_bottom(), rect.right_bottom()],
            theme::stroke(1.0, color::LINE),
        );
        // Layout: brand left, tabs centred, status right. When the window is narrow the tabs
        // move right of the brand, then the brand keeps only its mark and the tabs get
        // tighter; the status shows only where it fits.
        let labels: Vec<String> = Tab::ALL.iter().map(|t| self.tr.t(t.key())).collect();
        let font = theme::medium(theme::T_BODY);
        let tabs_width = |pad: f32| -> f32 {
            labels
                .iter()
                .map(|l| {
                    p.layout_no_wrap(l.clone(), font.clone(), color::TEXT)
                        .size()
                        .x
                        + pad * 2.0
                })
                .sum::<f32>()
                + 6.0
        };
        let mark = Rect::from_min_size(
            pos2(rect.left() + 24.0, rect.center().y - 12.0),
            vec2(24.0, 24.0),
        );
        let brand = p.layout_no_wrap("PhoeCord".into(), theme::semibold(16.0), color::TEXT);
        let brand_right_full = mark.right() + 10.0 + brand.size().x;
        let edge = rect.right() - 20.0;
        let (mut pad, mut show_brand) = (16.0, true);
        let mut tw = tabs_width(pad);
        let mut left = rect.center().x - tw / 2.0;
        if left < brand_right_full + 20.0 {
            left = brand_right_full + 20.0;
        }
        if left + tw > edge {
            show_brand = false;
            pad = 10.0;
            tw = tabs_width(pad);
            left = (rect.center().x - tw / 2.0).max(mark.right() + 14.0);
            if left + tw > edge {
                left = (edge - tw).max(mark.right() + 8.0);
            }
        }
        theme::paint_mark(p, mark);
        if show_brand {
            p.galley(
                pos2(mark.right() + 10.0, rect.center().y - brand.size().y / 2.0),
                brand,
                color::TEXT,
            );
        }
        // Status on the right, only where it fits.
        let mut right = rect.right() - 24.0;
        let room_from = left + tw + 20.0;
        if self.backend.mode.is_dry() {
            let t = self.tr.t("gui-dry-run");
            let g = p.layout_no_wrap(t, theme::medium(theme::T_CAPTION), color::WARN);
            let r = Rect::from_min_size(
                pos2(right - g.size().x - 18.0, rect.center().y - 11.0),
                vec2(g.size().x + 18.0, 22.0),
            );
            if r.left() > room_from {
                p.rect_filled(r, theme::radius(11), color::WARN_BG);
                p.galley(
                    pos2(r.left() + 9.0, r.center().y - g.size().y / 2.0),
                    g,
                    color::WARN,
                );
                right = r.left() - 12.0;
            }
        }
        if let Some(s) = &self.status {
            let (txt, col) = match &s.proxy {
                Proxy::Running(_) => (self.tr.t("gui-proxy-running"), color::OK),
                Proxy::Stopped => (self.tr.t("gui-proxy-stopped"), color::MUTED),
                Proxy::PortTaken => (self.tr.t("gui-proxy-port-taken"), color::WARN),
            };
            let g = p.layout_no_wrap(txt, theme::regular(theme::T_CAPTION), color::MUTED);
            let x = right - g.size().x;
            if x - 16.0 > room_from {
                p.galley(pos2(x, rect.center().y - g.size().y / 2.0), g, color::MUTED);
                // The dot is the proxy's state (semantic, not decoration).
                p.circle_filled(pos2(x - 10.0, rect.center().y), 3.5, col);
                let hover = ui.interact(
                    Rect::from_min_max(
                        pos2(x - 16.0, rect.top() + 14.0),
                        pos2(right, rect.bottom() - 14.0),
                    ),
                    Id::new("proxy-chip"),
                    Sense::hover(),
                );
                hover.on_hover_text(self.tr.f("gui-proxy-tip", &[("listen", &s.listen)]));
            }
        }
        let tabs_rect = Rect::from_min_size(pos2(left, rect.center().y - 18.0), vec2(tw, 36.0));
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(tabs_rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        if let Some(i) = widgets::tabs(&mut child, Id::new("tabs"), &labels, self.tab.index(), pad)
        {
            let ctx = ui.ctx().clone();
            self.go(&ctx, Tab::ALL[i]);
        }
    }

    fn content(&mut self, ui: &mut egui::Ui, area: Rect) {
        let ctx = ui.ctx().clone();
        let t = motion::tween_from(
            &ctx,
            Id::new("tab-enter"),
            0.0,
            1.0,
            dur::TAB,
            motion::out_quint,
        );
        let dir = if self.tab.index() >= self.prev_tab.index() {
            1.0
        } else {
            -1.0
        };
        let dx = dir * 18.0 * (1.0 - t);
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(area.translate(vec2(dx, 0.0)))
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        child.set_clip_rect(area);
        child.set_opacity(t);
        egui::ScrollArea::vertical()
            .id_salt(("tab-scroll", self.tab.index()))
            .auto_shrink([false, false])
            .show(&mut child, |ui| {
                let w = ui.available_width();
                let col =
                    theme::CONTENT_MAX.min(w - 2.0 * if w < 760.0 { theme::S5 } else { theme::S7 });
                let side = ((w - col) / 2.0).max(0.0);
                ui.add_space(36.0);
                ui.horizontal_top(|ui| {
                    ui.add_space(side);
                    ui.vertical(|ui| {
                        ui.set_width(col);
                        match self.tab {
                            Tab::Install => self.install_tab(ui),
                            Tab::Plugins => self.plugins_tab(ui),
                            Tab::Settings => self.settings_tab(ui),
                            Tab::About => self.about_tab(ui),
                        }
                    });
                });
                ui.add_space(32.0);
            });
    }

    fn files_dropped(&mut self, ctx: &egui::Context) {
        let dropped: Vec<_> = ctx.input(|i| i.raw.dropped_files.clone());
        if let Some(path) = dropped.into_iter().find_map(|f| f.path)
            && !self.terms_needed
            && self.dialog.is_none()
        {
            self.go(ctx, Tab::Plugins);
            self.stage_path(path);
        }
    }

    fn drop_overlay(&self, ctx: &egui::Context) {
        let hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());
        let t = motion::presence(
            ctx,
            Id::new("drop"),
            hovering && !self.terms_needed,
            dur::ENTER,
            dur::EXIT,
        );
        if t <= 0.001 {
            return;
        }
        let screen = ctx.content_rect();
        let p = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            Id::new("drop-layer"),
        ));
        p.rect_filled(screen, 0.0, theme::fade(color::SCRIM, 0.82 * t));
        let card = widgets::scale_rect(screen.shrink(28.0), 0.98 + 0.02 * t);
        p.rect(
            card,
            theme::radius(theme::R_DIALOG),
            theme::fade(color::SELECTED, t),
            theme::stroke(2.0, theme::fade(color::ROSE, t)),
            StrokeKind::Inside,
        );
        p.text(
            card.center() - vec2(0.0, 28.0),
            Align2::CENTER_CENTER,
            icon::FILE_ZIP,
            theme::icon(40.0),
            theme::fade(color::ROSE, t),
        );
        p.text(
            card.center() + vec2(0.0, 14.0),
            Align2::CENTER_CENTER,
            self.tr.t("gui-drop-title"),
            theme::semibold(theme::T_TITLE),
            theme::fade(color::TEXT, t),
        );
        p.text(
            card.center() + vec2(0.0, 40.0),
            Align2::CENTER_CENTER,
            self.tr.t("gui-drop-body"),
            theme::regular(theme::T_BODY),
            theme::fade(color::MUTED, t),
        );
    }

    fn toasts(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        self.toasts.retain(|t| now - t.born < t.ttl + 0.3);
        let screen = ctx.content_rect();
        let mut y = screen.bottom() - 24.0;
        for t in self.toasts.iter().rev() {
            let id = Id::new(("toast", t.id));
            let alive = now - t.born < t.ttl;
            let a = motion::presence(ctx, id, alive, dur::ENTER, dur::EXIT);
            if !alive {
                ctx.request_repaint();
            } else {
                ctx.request_repaint_after_secs((t.ttl - (now - t.born)) as f32);
            }
            let (fg, _) = t.tone.colors();
            let font = theme::medium(theme::T_BODY);
            let g = ctx.fonts_mut(|f| f.layout(t.text.clone(), font, color::TEXT, 460.0));
            let size = vec2(g.size().x + 54.0, g.size().y + 22.0);
            let rect = Rect::from_min_size(
                pos2(
                    screen.center().x - size.x / 2.0,
                    y - size.y + 12.0 * (1.0 - a),
                ),
                size,
            );
            let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, id));
            p.rect(
                rect,
                theme::radius(theme::R_TILE),
                theme::fade(color::RAISED, a),
                theme::stroke(1.0, theme::fade(color::LINE_STRONG, a)),
                StrokeKind::Inside,
            );
            let glyph = match t.tone {
                Tone::Ok => icon::CHECK_CIRCLE,
                Tone::Danger => icon::WARNING_CIRCLE,
                Tone::Warn => icon::WARNING,
                Tone::Neutral => icon::INFO,
            };
            p.text(
                pos2(rect.left() + 16.0, rect.center().y),
                Align2::LEFT_CENTER,
                glyph,
                theme::icon(18.0),
                theme::fade(fg, a),
            );
            p.galley(
                pos2(rect.left() + 42.0, rect.center().y - g.size().y / 2.0),
                g,
                theme::fade(color::TEXT, a),
            );
            y = rect.top() - 8.0;
        }
    }

    /// The dialog layer: a scrim that takes every click, and the card sliding up into place.
    fn dialogs(&mut self, ctx: &egui::Context) {
        let open = self.dialog.is_some();
        let t = motion::presence(ctx, Id::new("dialog"), open, dur::ENTER, dur::EXIT);
        if !open && t <= 0.001 {
            self.closing = None;
            return;
        }
        let screen = ctx.content_rect();
        egui::Area::new(Id::new("scrim"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                let (r, _) = ui.allocate_exact_size(screen.size(), Sense::click());
                ui.painter()
                    .rect_filled(r, 0.0, theme::fade(color::SCRIM, 0.72 * t));
            });
        let width = 460.0f32.min(screen.width() - 48.0);
        egui::Area::new(Id::new("dialog-card"))
            .order(egui::Order::Tooltip)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 14.0 * (1.0 - t)))
            .show(ctx, |ui| {
                ui.set_opacity(t);
                egui::Frame::new()
                    .fill(color::SURFACE)
                    .stroke(theme::stroke(1.0, color::LINE_STRONG))
                    .corner_radius(theme::radius(theme::R_DIALOG))
                    .inner_margin(egui::Margin::same(24))
                    .show(ui, |ui| {
                        ui.set_width(width);
                        egui::ScrollArea::vertical()
                            .max_height(screen.height() - 96.0)
                            .min_scrolled_height(screen.height() - 96.0)
                            .show(ui, |ui| {
                                ui.set_width(width);
                                if self.dialog.is_some() {
                                    let mut d = self.dialog.take();
                                    let keep = self.dialog_body(ui, d.as_mut().unwrap());
                                    if keep && self.dialog.is_none() {
                                        self.dialog = d;
                                    } else if !keep {
                                        self.closing = d;
                                    }
                                } else if let Some(mut d) = self.closing.take() {
                                    // Draw the closing dialog once more while it fades; ignore input.
                                    ui.disable();
                                    let _ = self.dialog_body(ui, &mut d);
                                    self.closing = Some(d);
                                }
                            });
                    });
            });
        if open && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            if let Some(Dialog::Plugin(c)) = &self.dialog {
                self.store.discard(c);
            }
            self.close_dialog();
        }
    }

    /// Ask before closing the window while a job changes the machine.
    fn busy(&self) -> bool {
        matches!(self.flow, Flow::Installing { .. } | Flow::Uninstalling)
    }
}

/// The rect left for tab content under the header.
fn content_rect(full: Rect) -> Rect {
    Rect::from_min_max(pos2(full.left(), full.top() + theme::HEADER_H), full.max)
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(m) = self.rx.try_recv() {
            self.handle(ctx, m);
        }
        self.files_dropped(ctx);
        if !self.terms_needed
            && self.onboarding.is_none()
            && let Some(p) = self.pending_open.take()
        {
            self.go(ctx, Tab::Plugins);
            self.stage_path(p);
        }
        if ctx.input(|i| i.viewport().close_requested()) && self.busy() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            let t = self.tr.t("gui-toast-wait");
            self.toast(ctx, t, Tone::Warn);
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(color::BG))
            .show(ctx, |ui| {
                let full = ui.max_rect();
                // Under the opening animation the window comes in after the mark: the header
                // first, the rest a beat later (`crate::splash`).
                let entering = |ui: &mut egui::Ui, content: bool| {
                    let (alpha, rise) = crate::splash::reveal(ui.ctx(), content);
                    let mut child = ui.new_child(
                        egui::UiBuilder::new().max_rect(full.translate(vec2(0.0, rise))),
                    );
                    child.set_clip_rect(full);
                    child.set_opacity(alpha);
                    child
                };
                let mut whole = entering(ui, true);
                let ui = &mut whole;
                if let Some(step) = self.onboarding {
                    // The terms step moves on by itself once they are accepted.
                    if step == crate::onboarding::TERMS && !self.terms_needed {
                        self.onboard_to(ctx, crate::onboarding::PICK);
                    }
                    let step = self.onboarding.unwrap_or(step);
                    self.onboarding_screen(ui, full, step);
                    return;
                }
                if self.terms_needed || self.terms_viewing {
                    self.terms_screen(ui, full);
                    return;
                }
                let mut head = entering(ui, false);
                self.header(&mut head);
                self.content(ui, content_rect(full));
            });
        self.dialogs(ctx);
        self.drop_overlay(ctx);
        self.toasts(ctx);
        crate::splash::show(ctx);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        storage.set_string(REDUCE_KEY, self.reduce_motion.to_string());
    }

    /// Scroll positions and the like start fresh on every start.
    fn persist_egui_memory(&self) -> bool {
        false
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        color::BG.to_normalized_gamma_f32()
    }
}

/// A row of buttons laid out left to right; returns which one was clicked.
pub fn button_row(ui: &mut egui::Ui, buttons: Vec<Button<'_>>) -> Option<usize> {
    let mut hit = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(theme::S2, theme::S2);
        for (i, b) in buttons.into_iter().enumerate() {
            if b.show(ui).clicked() {
                hit = Some(i);
            }
        }
    });
    hit
}
