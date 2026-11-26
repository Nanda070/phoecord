//! Translate: translate messages with Google Translate (Vencord: Translate), data part.
//!
//! Ported from Vencord `src/plugins/translate/` (GPL-3.0-or-later, Copyright (c) 2023 Vendicated
//! and contributors; authors Ven, AshtonMemer, koish1). Vencord translates a message you pick
//! (context menu, hover button; the result shows under the message as "Translated from …") and,
//! with `autoTranslate`, every message you send. The language pairs are `receivedInput` /
//! `receivedOutput` (default `auto` → `en`) and `sentInput` / `sentOutput` (`auto` → `en`).
//!
//! Here, through `Host::fetch_with` to Google's endpoint that Vencord uses
//! (`translate-pa.googleapis.com`, with Vencord's public key):
//!
//! - `autoTranslate` (or `/phoecord autotranslate`, which flips it and keeps the choice in the
//!   plugin's kv store) rewrites the `content` of `POST /channels/{id}/messages` with the sent
//!   pair; when translating fails the message goes out as written and a notification says why;
//! - `/phoecord translate text:<text>` or `message:<id or link>` shows the translation (received
//!   pair, or `to` / `from`) with "Translated from …", as a message only you see.
//!
//! Not ported: DeepL and Kagi (`service`). DeepL needs an `Authorization` header and Kagi a
//! `Cookie`, and `Host::fetch_with` refuses both; with those services set the plugin uses Google,
//! as Vencord does when the DeepL key is missing. The buttons and the accessory under the message
//! wait for Phase 2.

use crate::commands::{
    Invocation, Kept, Subcommand, invocation, is_interaction, kept_answer, message_ref,
    snowflake_arg,
};
use crate::music_rich_presence::encode_component;
use phoecord_core::plugin::HostResponse;
use phoecord_core::{Event, EventKind, Host, Manifest, Plugin, Val, Verdict};

const PLUGIN_ID: &str = "translate";
const HOST: &str = "translate-pa.googleapis.com";
/// Vencord's "some google API key".
const GOOGLE_KEY: &str = "AIzaSyDLEeFI5OtFBwYBIoK_jj5m32rZK5CkCXA";
const KV_AUTO: &str = "autoTranslate";

pub struct Translate {
    manifest: Manifest,
    kept: Kept,
}

impl Translate {
    pub fn new() -> Translate {
        Translate {
            manifest: {
                let mut m = Manifest::new(
                    PLUGIN_ID,
                    "Translate",
                    env!("CARGO_PKG_VERSION"),
                    "Translate messages, and optionally everything you send",
                );
                m.authors = vec!["Cheterin Group".into()];
                m.vencord = Some("Translate".into());
                m.fetch = vec![HOST.into()];
                m
            },
            kept: Kept::default(),
        }
    }

    /// `/phoecord translate` and `/phoecord autotranslate`.
    pub fn commands(&self) -> Vec<Box<dyn Subcommand>> {
        vec![
            kept_answer(
                PLUGIN_ID,
                "translate",
                "Translate a text or a message",
                &self.kept,
            )
            .option("text", "Text to translate", false)
            .option("message", "Message id or link to translate", false)
            .option(
                "to",
                "Language to translate to (default: receivedOutput)",
                false,
            )
            .option(
                "from",
                "Language to translate from (default: receivedInput)",
                false,
            )
            .boxed(),
            kept_answer(
                PLUGIN_ID,
                "autotranslate",
                "Translate your messages before sending: on or off",
                &self.kept,
            )
            .boxed(),
        ]
    }
}

impl Default for Translate {
    fn default() -> Self {
        Self::new()
    }
}

/// A translation: Vencord's `TranslationValue`.
#[derive(Debug, PartialEq)]
pub struct Translation {
    pub source_language: String,
    pub text: String,
}

fn setting(host: &dyn Host, key: &str, default: &str) -> String {
    host.setting(key)
        .and_then(|v| v.as_str().map(str::to_string))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| default.to_string())
}

/// Vencord's `googleTranslate`.
pub fn google(
    host: &mut dyn Host,
    text: &str,
    from: &str,
    to: &str,
) -> Result<Translation, String> {
    let url = format!(
        "https://{HOST}/v1/translate?params.client=gtx&dataTypes=TRANSLATION&key={GOOGLE_KEY}\
         &query.sourceLanguage={}&query.targetLanguage={}&query.text={}",
        encode_component(from),
        encode_component(to),
        encode_component(text)
    );
    match host.fetch_with("GET", &url, &[], None)? {
        HostResponse {
            status: 200..=299,
            body: Some(b),
            ..
        } => {
            let code = b.str_at(&["sourceLanguage"]).unwrap_or("").to_string();
            let text = b
                .str_at(&["translation"])
                .ok_or("no translation in the answer")?
                .to_string();
            let name = language_name(&code).map_or(code, str::to_string);
            Ok(Translation {
                source_language: name,
                text,
            })
        }
        HostResponse { status, .. } => Err(format!("HTTP {status} ({from} -> {to})")),
    }
}

fn language_name(code: &str) -> Option<&'static str> {
    GOOGLE_LANGUAGES
        .iter()
        .find(|(c, _)| *c == code)
        .map(|(_, n)| *n)
}

/// A language as the user typed it: a code or a name ("German"), as Google's code.
fn language_code(typed: &str) -> Option<&'static str> {
    let t = typed.trim();
    GOOGLE_LANGUAGES
        .iter()
        .find(|(c, n)| c.eq_ignore_ascii_case(t) || n.eq_ignore_ascii_case(t))
        .map(|(c, _)| *c)
}

/// Whether sent messages are translated: the command's choice, else the setting.
fn auto_on(host: &dyn Host) -> bool {
    host.kv_get(KV_AUTO)
        .and_then(|v| v.as_bool())
        .or_else(|| host.setting(KV_AUTO).and_then(|v| v.as_bool()))
        .unwrap_or(false)
}

/// Vencord's `getMessageContent`: the content, else a forwarded message's, else an AutoMod
/// alert's text.
fn message_content(m: &Val) -> Option<String> {
    let non_empty = |s: Option<&str>| s.filter(|s| !s.is_empty()).map(str::to_string);
    non_empty(m.str_at(&["content"]))
        .or_else(|| {
            m.get("message_snapshots")
                .and_then(|s| s.as_list().first())
                .and_then(|s| non_empty(s.str_at(&["message", "content"])))
        })
        .or_else(|| {
            m.get("embeds")
                .map(Val::as_list)
                .unwrap_or_default()
                .iter()
                .find(|e| e.str_at(&["type"]) == Some("auto_moderation_message"))
                .and_then(|e| non_empty(e.str_at(&["description"])))
        })
}

fn is_send(method: &str, route: &str) -> bool {
    let seg: Vec<&str> = route.trim_matches('/').split('/').collect();
    method == "POST" && matches!(seg.as_slice(), ["channels", _, "messages"])
}

impl Translate {
    fn run(&self, host: &mut dyn Host, inv: &Invocation) -> String {
        if inv.sub == "autotranslate" {
            let on = !auto_on(host);
            host.kv_set(KV_AUTO, Val::Bool(on));
            return host.t(
                if on {
                    "translate-auto-on"
                } else {
                    "translate-auto-off"
                },
                &[],
            );
        }
        let pair = |key: &str, default: &str, typed: Option<&str>| match typed {
            Some(t) => language_code(t)
                .map(str::to_string)
                .ok_or_else(|| t.to_string()),
            None => Ok(setting(host, key, default)),
        };
        let (from, to) = match (
            pair("receivedInput", "auto", inv.get("from")),
            pair("receivedOutput", "en", inv.get("to")),
        ) {
            (Ok(f), Ok(t)) => (f, t),
            (Err(bad), _) | (_, Err(bad)) => {
                return host.t("translate-bad-language", &[("language", &bad)]);
            }
        };
        let text = match (inv.get("text"), inv.get("message")) {
            (Some(t), _) => t.to_string(),
            (None, Some(m)) => {
                let Some((linked, id)) = message_ref(m) else {
                    return host.t("translate-bad-message", &[]);
                };
                let channel = inv
                    .get("channel")
                    .and_then(snowflake_arg)
                    .or(linked)
                    .or(inv.channel_id);
                let Some(channel) = channel else {
                    return host.t("translate-bad-message", &[]);
                };
                let page = host.rest(
                    "GET",
                    &format!("/channels/{channel}/messages?limit=1&around={id}"),
                    None,
                );
                let found = match page {
                    Ok(HostResponse {
                        status: 200..=299,
                        body: Some(b),
                        ..
                    }) => b
                        .as_list()
                        .iter()
                        .find(|m| m.id_at(&["id"]) == Some(id))
                        .and_then(message_content),
                    _ => None,
                };
                match found {
                    Some(t) => t,
                    None => return host.t("translate-no-text", &[]),
                }
            }
            (None, None) => return host.t("translate-need-text", &[]),
        };
        match google(host, &text, &from, &to) {
            Ok(tr) => host.t(
                "translate-result",
                &[("text", &tr.text), ("language", &tr.source_language)],
            ),
            Err(e) => host.t("translate-failed", &[("error", &e)]),
        }
    }

    /// `autoTranslate` on a message you send; whether it changed.
    fn outgoing(&self, host: &mut dyn Host, body: &mut Val) -> bool {
        if !auto_on(host) {
            return false;
        }
        let Some(text) = body
            .str_at(&["content"])
            .filter(|t| !t.trim().is_empty())
            .map(str::to_string)
        else {
            return false;
        };
        let (from, to) = (
            setting(host, "sentInput", "auto"),
            setting(host, "sentOutput", "en"),
        );
        match google(host, &text, &from, &to) {
            Ok(tr) => {
                let new = body.get("content").expect("checked").str_like(&tr.text);
                body.set("content", new);
                true
            }
            Err(e) => {
                let title = host.t("translate-auto-failed-title", &[]);
                host.notify(&title, &e);
                false
            }
        }
    }
}

impl Plugin for Translate {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        matches!(event, EventKind::RestRequest { method, route }
            if is_interaction(method, route) || is_send(method, route))
    }

    fn on_event(&self, host: &mut dyn Host, event: Event<'_>) -> Verdict {
        let Event::RestRequest {
            method,
            route,
            body: Some(body),
        } = event
        else {
            return Verdict::Pass;
        };
        if is_send(method, route) {
            return if self.outgoing(host, body) {
                Verdict::Changed
            } else {
                Verdict::Pass
            };
        }
        if is_interaction(method, route)
            && let Some(inv) = invocation(body)
            && (inv.sub == "translate" || inv.sub == "autotranslate")
        {
            let answer = self.run(host, &inv);
            *self.kept.lock() = Some(answer);
        }
        Verdict::Pass
    }
}

/// Google Translate's language codes and names (Vencord's `GoogleLanguages`, from
/// `src/plugins/translate/languages.ts`).
pub const GOOGLE_LANGUAGES: &[(&str, &str)] = &[
    ("auto", "Detect language"),
    ("af", "Afrikaans"),
    ("sq", "Albanian"),
    ("am", "Amharic"),
    ("ar", "Arabic"),
    ("hy", "Armenian"),
    ("as", "Assamese"),
    ("ay", "Aymara"),
    ("az", "Azerbaijani"),
    ("bm", "Bambara"),
    ("eu", "Basque"),
    ("be", "Belarusian"),
    ("bn", "Bengali"),
    ("bho", "Bhojpuri"),
    ("bs", "Bosnian"),
    ("bg", "Bulgarian"),
    ("ca", "Catalan"),
    ("ceb", "Cebuano"),
    ("ny", "Chichewa"),
    ("zh-CN", "Chinese (Simplified)"),
    ("zh-TW", "Chinese (Traditional)"),
    ("co", "Corsican"),
    ("hr", "Croatian"),
    ("cs", "Czech"),
    ("da", "Danish"),
    ("dv", "Dhivehi"),
    ("doi", "Dogri"),
    ("nl", "Dutch"),
    ("en", "English"),
    ("eo", "Esperanto"),
    ("et", "Estonian"),
    ("ee", "Ewe"),
    ("tl", "Filipino"),
    ("fi", "Finnish"),
    ("fr", "French"),
    ("fy", "Frisian"),
    ("gl", "Galician"),
    ("ka", "Georgian"),
    ("de", "German"),
    ("el", "Greek"),
    ("gn", "Guarani"),
    ("gu", "Gujarati"),
    ("ht", "Haitian Creole"),
    ("ha", "Hausa"),
    ("haw", "Hawaiian"),
    ("iw", "Hebrew"),
    ("hi", "Hindi"),
    ("hmn", "Hmong"),
    ("hu", "Hungarian"),
    ("is", "Icelandic"),
    ("ig", "Igbo"),
    ("ilo", "Ilocano"),
    ("id", "Indonesian"),
    ("ga", "Irish"),
    ("it", "Italian"),
    ("ja", "Japanese"),
    ("jw", "Javanese"),
    ("kn", "Kannada"),
    ("kk", "Kazakh"),
    ("km", "Khmer"),
    ("rw", "Kinyarwanda"),
    ("gom", "Konkani"),
    ("ko", "Korean"),
    ("kri", "Krio"),
    ("ku", "Kurdish (Kurmanji)"),
    ("ckb", "Kurdish (Sorani)"),
    ("ky", "Kyrgyz"),
    ("lo", "Lao"),
    ("la", "Latin"),
    ("lv", "Latvian"),
    ("ln", "Lingala"),
    ("lt", "Lithuanian"),
    ("lg", "Luganda"),
    ("lb", "Luxembourgish"),
    ("mk", "Macedonian"),
    ("mai", "Maithili"),
    ("mg", "Malagasy"),
    ("ms", "Malay"),
    ("ml", "Malayalam"),
    ("mt", "Maltese"),
    ("mi", "Maori"),
    ("mr", "Marathi"),
    ("mni-Mtei", "Meiteilon (Manipuri)"),
    ("lus", "Mizo"),
    ("mn", "Mongolian"),
    ("my", "Myanmar (Burmese)"),
    ("ne", "Nepali"),
    ("no", "Norwegian"),
    ("or", "Odia (Oriya)"),
    ("om", "Oromo"),
    ("ps", "Pashto"),
    ("fa", "Persian"),
    ("pl", "Polish"),
    ("pt", "Portuguese"),
    ("pa", "Punjabi"),
    ("qu", "Quechua"),
    ("ro", "Romanian"),
    ("ru", "Russian"),
    ("sm", "Samoan"),
    ("sa", "Sanskrit"),
    ("gd", "Scots Gaelic"),
    ("nso", "Sepedi"),
    ("sr", "Serbian"),
    ("st", "Sesotho"),
    ("sn", "Shona"),
    ("sd", "Sindhi"),
    ("si", "Sinhala"),
    ("sk", "Slovak"),
    ("sl", "Slovenian"),
    ("so", "Somali"),
    ("es", "Spanish"),
    ("su", "Sundanese"),
    ("sw", "Swahili"),
    ("sv", "Swedish"),
    ("tg", "Tajik"),
    ("ta", "Tamil"),
    ("tt", "Tatar"),
    ("te", "Telugu"),
    ("th", "Thai"),
    ("ti", "Tigrinya"),
    ("ts", "Tsonga"),
    ("tr", "Turkish"),
    ("tk", "Turkmen"),
    ("ak", "Twi"),
    ("uk", "Ukrainian"),
    ("ur", "Urdu"),
    ("ug", "Uyghur"),
    ("uz", "Uzbek"),
    ("vi", "Vietnamese"),
    ("cy", "Welsh"),
    ("xh", "Xhosa"),
    ("yi", "Yiddish"),
    ("yo", "Yoruba"),
    ("zu", "Zulu"),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::Args;
    use crate::testutil::{Rec, j};

    fn url(from: &str, to: &str, text: &str) -> String {
        format!(
            "https://{HOST}/v1/translate?params.client=gtx&dataTypes=TRANSLATION&key={GOOGLE_KEY}\
             &query.sourceLanguage={from}&query.targetLanguage={to}&query.text={text}"
        )
    }

    fn run(p: &Translate, h: &mut Rec, sub: &str, opts: &[(&str, &str)]) -> String {
        let options: Vec<String> = opts
            .iter()
            .map(|(k, v)| {
                format!(
                    r#"{{"type":3,"name":"{k}","value":{}}}"#,
                    serde_json::to_string(v).unwrap()
                )
            })
            .collect();
        let mut body = j(&format!(
            r#"{{"type":2,"channel_id":"9","data":{{"name":"phoecord","options":[{{"type":1,"name":"{sub}","options":[{}]}}]}}}}"#,
            options.join(",")
        ));
        p.on_event(
            h,
            Event::RestRequest {
                method: "POST",
                route: "/interactions",
                body: Some(&mut body),
            },
        );
        let cmds = p.commands();
        cmds.iter()
            .find(|c| c.name() == sub)
            .unwrap()
            .run(h, &Args { options: vec![] })
    }

    #[test]
    fn translates_a_text_and_names_the_source_language() {
        let p = Translate::new();
        let mut h = Rec::default().answer_fetch(
            "GET",
            &url("auto", "en", "Hallo%20Welt"),
            200,
            r#"{"translation":"Hello world","sourceLanguage":"de"}"#,
        );
        assert_eq!(
            run(&p, &mut h, "translate", &[("text", "Hallo Welt")]),
            "Hello world\n-# Translated from German"
        );
        assert!(h.fetch_calls[0].1.is_empty(), "no extra headers");
    }

    #[test]
    fn translates_a_message_by_link_with_typed_languages() {
        let p = Translate::new();
        let mut h = Rec::default()
            .answer(
                "GET",
                "/channels/8/messages?limit=1&around=5",
                200,
                r#"[{"id":"5","content":"","message_snapshots":[{"message":{"content":"Bonjour"}}]}]"#,
            )
            .answer_fetch(
                "GET",
                &url("fr", "ru", "Bonjour"),
                200,
                r#"{"translation":"Привет","sourceLanguage":"fr"}"#,
            );
        let out = run(
            &p,
            &mut h,
            "translate",
            &[
                ("message", "https://discord.com/channels/1/8/5"),
                ("to", "Russian"),
                ("from", "fr"),
            ],
        );
        assert_eq!(out, "Привет\n-# Translated from French");
        assert!(
            run(&p, &mut h, "translate", &[("text", "x"), ("to", "Elvish")]).contains("Elvish")
        );
        assert!(run(&p, &mut h, "translate", &[]).contains("Give"));
    }

    #[test]
    fn auto_translate_rewrites_sent_messages_and_the_command_flips_it() {
        let p = Translate::new();
        let mut h = Rec::default()
            .with("sentOutput", Val::Str("es".into()))
            .answer_fetch(
                "GET",
                &url("auto", "es", "hi"),
                200,
                r#"{"translation":"hola","sourceLanguage":"en"}"#,
            );
        let send = |p: &Translate, h: &mut Rec, text: &str| {
            let mut body = j(&format!(r#"{{"content":"{text}","nonce":"1"}}"#));
            let v = p.on_event(
                h,
                Event::RestRequest {
                    method: "POST",
                    route: "/channels/9/messages",
                    body: Some(&mut body),
                },
            );
            (v, body.str_at(&["content"]).unwrap().to_string())
        };
        assert_eq!(
            send(&p, &mut h, "hi"),
            (Verdict::Pass, "hi".into()),
            "off by default"
        );
        assert_eq!(
            run(&p, &mut h, "autotranslate", &[]),
            "Auto-translate is **on**: your messages are translated before they are sent."
        );
        assert_eq!(send(&p, &mut h, "hi"), (Verdict::Changed, "hola".into()));
        // A failure sends the message as written and says why.
        assert_eq!(send(&p, &mut h, "other"), (Verdict::Pass, "other".into()));
        assert_eq!(h.notes.len(), 1);
        assert!(run(&p, &mut h, "autotranslate", &[]).contains("off"));
    }

    #[test]
    fn the_language_table_is_vencords() {
        assert_eq!(GOOGLE_LANGUAGES.len(), 134);
        assert_eq!(language_name("auto"), Some("Detect language"));
        assert_eq!(language_code("german"), Some("de"));
        assert_eq!(language_code("zh-CN"), Some("zh-CN"));
    }
}
