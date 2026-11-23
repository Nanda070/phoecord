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

/// A value from JSON text.
pub fn j(s: &str) -> Val {
    Val::from_json(serde_json::from_str(s).unwrap())
}
