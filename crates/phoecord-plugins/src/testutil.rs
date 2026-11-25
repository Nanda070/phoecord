//! Test helpers shared by the plugins' unit tests.

use phoecord_core::{Host, Val};
use std::collections::HashMap;

/// A [`Host`] with settings, a log and a plugin on/off table, nothing else.
#[derive(Default)]
pub struct Cfg {
    pub settings: HashMap<String, Val>,
    pub logs: Vec<String>,
    pub plugins: HashMap<String, bool>,
}

impl Cfg {
    pub fn with(mut self, key: &str, val: Val) -> Cfg {
        self.settings.insert(key.into(), val);
        self
    }
}

impl Host for Cfg {
    fn setting(&self, key: &str) -> Option<Val> {
        self.settings.get(key).cloned()
    }
    fn kv_get(&self, _: &str) -> Option<Val> {
        None
    }
    fn kv_set(&mut self, _: &str, _: Val) {}
    fn self_id(&self) -> Option<u64> {
        None
    }
    fn notify(&mut self, _: &str, _: &str) {}
    fn log(&mut self, message: &str) {
        self.logs.push(message.into());
    }
    fn t(&self, key: &str, args: &[(&str, &str)]) -> String {
        tr("", key, args)
    }
    fn emit_send(&mut self, _: i64, _: Val) {}
    fn guild_emojis(&self, _: u64) -> Vec<u64> {
        Vec::new()
    }
    fn channel_guild(&self, _: u64) -> Option<u64> {
        None
    }
    fn plugin_enabled(&self, id: &str) -> Option<bool> {
        self.plugins.get(id).copied()
    }
}

/// What `Host::t` answers for `key` in `lang` (`""` = English): the first built-in plugin bundle
/// that has the key, else the key. Keys are unique across the bundles that tests meet.
pub fn tr(lang: &str, key: &str, args: &[(&str, &str)]) -> String {
    use std::sync::OnceLock;
    static STRINGS: OnceLock<phoecord_i18n::PluginStrings> = OnceLock::new();
    let strings = STRINGS.get_or_init(|| {
        let s = phoecord_i18n::PluginStrings::new();
        crate::register_strings(&s);
        s
    });
    let lang = if lang.is_empty() { "en" } else { lang };
    for (id, _, _) in crate::I18N {
        let text = strings.t(id, lang, key, args);
        if text != key {
            return text;
        }
    }
    key.to_string()
}

/// A value from JSON text.
pub fn j(s: &str) -> Val {
    Val::from_json(serde_json::from_str(s).unwrap())
}

/// A [`Host`] that records everything: settings, a working KV store, notifications, logs,
/// queued sends and dispatches, and `rest` calls answered from a script. For the data parts of
/// UI plugins (stages 1.2.16 and 1.2.18), which talk to the user and to the API.
#[derive(Default)]
pub struct Rec {
    pub settings: HashMap<String, Val>,
    pub kv: HashMap<String, Val>,
    pub notes: Vec<(String, String)>,
    pub logs: Vec<String>,
    pub sent: Vec<(i64, Val)>,
    pub dispatched: Vec<Val>,
    pub self_id: Option<u64>,
    pub plugins: HashMap<String, bool>,
    /// `"METHOD route"` -> answer; a call with no entry gets status 404.
    pub rest_answers: HashMap<String, phoecord_core::plugin::HostResponse>,
    /// Every `rest` call as `("METHOD route", body)`.
    pub rest_calls: Vec<(String, Option<Val>)>,
    /// Language `Host::t` answers in (`""` = English).
    pub lang: &'static str,
}

impl Rec {
    pub fn with(mut self, key: &str, val: Val) -> Rec {
        self.settings.insert(key.into(), val);
        self
    }
    /// Script a `rest` answer with a JSON body.
    pub fn answer(mut self, method: &str, route: &str, status: u16, body: &str) -> Rec {
        self.rest_answers.insert(
            format!("{method} {route}"),
            phoecord_core::plugin::HostResponse {
                status,
                body: Some(j(body)),
                text: None,
                headers: Vec::new(),
                bytes: None,
            },
        );
        self
    }
}

impl Host for Rec {
    fn setting(&self, key: &str) -> Option<Val> {
        self.settings.get(key).cloned()
    }
    fn kv_get(&self, key: &str) -> Option<Val> {
        self.kv.get(key).cloned()
    }
    fn kv_set(&mut self, key: &str, value: Val) {
        self.kv.insert(key.into(), value);
    }
    fn self_id(&self) -> Option<u64> {
        self.self_id
    }
    fn notify(&mut self, title: &str, body: &str) {
        self.notes.push((title.into(), body.into()));
    }
    fn log(&mut self, message: &str) {
        self.logs.push(message.into());
    }
    fn t(&self, key: &str, args: &[(&str, &str)]) -> String {
        tr(self.lang, key, args)
    }
    fn emit_send(&mut self, op: i64, payload: Val) {
        self.sent.push((op, payload));
    }
    fn guild_emojis(&self, _: u64) -> Vec<u64> {
        Vec::new()
    }
    fn channel_guild(&self, _: u64) -> Option<u64> {
        None
    }
    fn rest(
        &mut self,
        method: &str,
        route: &str,
        body: Option<&Val>,
    ) -> Result<phoecord_core::plugin::HostResponse, String> {
        let key = format!("{method} {route}");
        self.rest_calls.push((key.clone(), body.cloned()));
        Ok(self
            .rest_answers
            .get(&key)
            .cloned()
            .unwrap_or(phoecord_core::plugin::HostResponse {
                status: 404,
                body: None,
                text: None,
                headers: Vec::new(),
                bytes: None,
            }))
    }
    fn emit_dispatch(&mut self, payload: Val) {
        self.dispatched.push(payload);
    }
    fn plugin_enabled(&self, id: &str) -> Option<bool> {
        self.plugins.get(id).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rec_answers_scripted_rest_and_records_calls() {
        let mut h = Rec::default().with("x", Val::Bool(true)).answer(
            "GET",
            "/users/1",
            200,
            r#"{"id":"1"}"#,
        );
        assert_eq!(h.setting("x"), Some(Val::Bool(true)));
        let ok = h.rest("GET", "/users/1", None).unwrap();
        assert_eq!(ok.status, 200);
        assert_eq!(ok.body, Some(j(r#"{"id":"1"}"#)));
        let missing = h.rest("POST", "/users/2", Some(&j("{}"))).unwrap();
        assert_eq!(missing.status, 404);
        assert_eq!(h.rest_calls.len(), 2);
        assert_eq!(h.rest_calls[1].0, "POST /users/2");
    }
}
