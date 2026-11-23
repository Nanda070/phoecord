//! Reference GPUI app for PhoeCord research (stage 1.6.2): a themed message list and a tabbed
//! settings-like view. It mimics Zed's `ActiveTheme` pattern with a tiny theme global so the
//! binary stays small; see docs/research/gpui.md for the mapping to Zed's real `theme` crate.

use gpui::{
    App, Bounds, Context, Global, Hsla, SharedString, Window, WindowBounds, WindowOptions, div,
    prelude::*, px, rgb, size, uniform_list,
};
use gpui_platform::application;

/// Colour palette, stored as a GPUI global exactly like Zed's `GlobalTheme`.
struct Theme {
    name: &'static str,
    background: Hsla,
    surface: Hsla,
    text: Hsla,
    text_muted: Hsla,
    accent: Hsla,
}

impl Global for Theme {}

impl Theme {
    fn dark() -> Self {
        Self {
            name: "Reference Dark",
            background: rgb(0x1e1f22).into(),
            surface: rgb(0x2b2d31).into(),
            text: rgb(0xf2f3f5).into(),
            text_muted: rgb(0x949ba4).into(),
            accent: rgb(0x5865f2).into(),
        }
    }
    fn light() -> Self {
        Self {
            name: "Reference Light",
            background: rgb(0xffffff).into(),
            surface: rgb(0xf2f3f5).into(),
            text: rgb(0x060607).into(),
            text_muted: rgb(0x5c5e66).into(),
            accent: rgb(0x5865f2).into(),
        }
    }
}

/// Mirrors Zed's `ActiveTheme::theme()` accessor: every colour read goes through here.
trait ActiveTheme {
    fn theme(&self) -> &Theme;
}

impl ActiveTheme for App {
    #[inline(never)]
    fn theme(&self) -> &Theme {
        self.global::<Theme>()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Messages,
    Plugins,
    About,
}

const TABS: [(Tab, &str); 3] = [
    (Tab::Messages, "Messages"),
    (Tab::Plugins, "Plugins"),
    (Tab::About, "About"),
];

struct Message {
    author: SharedString,
    body: SharedString,
}

struct Settings {
    tab: Tab,
    dark: bool,
    messages: Vec<Message>,
}

impl Settings {
    #[inline(never)]
    fn render_tab_list(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (surface, text, muted, accent) = {
            let t = cx.theme();
            (t.surface, t.text, t.text_muted, t.accent)
        };
        let mut list = div()
            .flex()
            .flex_col()
            .gap_1()
            .w(px(180.))
            .p_2()
            .bg(surface);
        for (tab, label) in TABS {
            let active = self.tab == tab;
            list = list.child(
                div()
                    .id(label)
                    .px_3()
                    .py_1()
                    .rounded_md()
                    .cursor_pointer()
                    .text_color(if active { text } else { muted })
                    .when(active, |d| d.bg(accent))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.tab = tab;
                        cx.notify();
                    }))
                    .child(label),
            );
        }
        list
    }

    #[inline(never)]
    fn render_message_row(message: &Message, ix: usize, cx: &App) -> impl IntoElement + use<> {
        let t = cx.theme();
        div()
            .id(ix)
            .flex()
            .flex_col()
            .px_3()
            .py_1()
            .child(div().text_color(t.accent).child(message.author.clone()))
            .child(div().text_color(t.text).child(message.body.clone()))
    }

    #[inline(never)]
    fn render_messages(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (text, accent) = (cx.theme().text, cx.theme().accent);
        let toggle = div()
            .id("toggle-theme")
            .px_3()
            .py_1()
            .m_2()
            .rounded_md()
            .cursor_pointer()
            .bg(accent)
            .text_color(text)
            .on_click(cx.listener(|this, _, _, cx| {
                this.dark = !this.dark;
                cx.set_global(if this.dark {
                    Theme::dark()
                } else {
                    Theme::light()
                });
                cx.notify();
            }))
            .child(format!("Theme: {} (click to switch)", cx.theme().name));
        div().flex().flex_col().size_full().child(toggle).child(
            uniform_list(
                "messages",
                self.messages.len(),
                cx.processor(|this, range: std::ops::Range<usize>, _window, cx| {
                    range
                        .map(|ix| Self::render_message_row(&this.messages[ix], ix, cx))
                        .collect::<Vec<_>>()
                }),
            )
            .flex_1(),
        )
    }
}

impl Render for Settings {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (bg, text) = (cx.theme().background, cx.theme().text);
        let content: gpui::AnyElement = match self.tab {
            Tab::Messages => self.render_messages(cx).into_any_element(),
            Tab::Plugins => div()
                .p_4()
                .child("Plugins: none installed")
                .into_any_element(),
            Tab::About => div()
                .p_4()
                .child("PhoeCord GPUI reference app")
                .into_any_element(),
        };
        div()
            .flex()
            .size_full()
            .bg(bg)
            .text_color(text)
            .child(self.render_tab_list(cx))
            .child(div().flex_1().child(content))
    }
}

fn main() {
    application().run(|cx: &mut App| {
        cx.set_global(Theme::dark());
        let bounds = Bounds::centered(None, size(px(720.), px(480.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| {
                cx.new(|_| Settings {
                    tab: Tab::Messages,
                    dark: true,
                    messages: (0..200)
                        .map(|i| Message {
                            author: format!("user{}", i % 7).into(),
                            body: format!("Reference message number {i}").into(),
                        })
                        .collect(),
                })
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
