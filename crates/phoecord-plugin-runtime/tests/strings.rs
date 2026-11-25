//! `Host::t` for WASM packages: the `i18n/<lang>.ftl` files of a package, loaded with it and on
//! every hot reload, answer the guest's `t` import in the user's language.

mod common;

use common::{TempDir, write_package};
use phoecord_core::{Host, Plugin, Val};
use phoecord_i18n::PluginStrings;
use phoecord_plugin_runtime::package::{PackagePlugin, PackageSet};
use phoecord_plugin_runtime::{Chain, NullHost, Outcome};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

/// A host that answers `t` from a [`PluginStrings`], scoped by `begin_plugin` like the proxy's.
struct StringsHost {
    inner: NullHost,
    strings: Arc<PluginStrings>,
    lang: &'static str,
    plugin: String,
}

impl StringsHost {
    fn new(strings: &Arc<PluginStrings>, lang: &'static str) -> StringsHost {
        StringsHost {
            inner: NullHost::default(),
            strings: strings.clone(),
            lang,
            plugin: String::new(),
        }
    }
}

impl Host for StringsHost {
    fn setting(&self, _: &str) -> Option<Val> {
        None
    }
    fn kv_get(&self, _: &str) -> Option<Val> {
        None
    }
    fn kv_set(&mut self, _: &str, _: Val) {}
    fn self_id(&self) -> Option<u64> {
        None
    }
    fn notify(&mut self, title: &str, body: &str) {
        self.inner.notify(title, body)
    }
    fn log(&mut self, message: &str) {
        self.inner.log(message)
    }
    fn emit_send(&mut self, _: i64, _: Val) {}
    fn guild_emojis(&self, _: u64) -> Vec<u64> {
        Vec::new()
    }
    fn channel_guild(&self, _: u64) -> Option<u64> {
        None
    }
    fn begin_plugin(&mut self, id: &str) {
        self.plugin = id.to_string();
    }
    fn t(&self, key: &str, args: &[(&str, &str)]) -> String {
        self.strings.t(&self.plugin, self.lang, key, args)
    }
}

fn write_ftl(dir: &Path, lang: &str, text: &str) {
    std::fs::create_dir_all(dir.join("i18n")).unwrap();
    std::fs::write(dir.join("i18n").join(format!("{lang}.ftl")), text).unwrap();
}

/// What the guest's `t` calls came back as: `greeting|only-en`.
fn asked(chain: &Chain, host: &mut StringsHost) -> String {
    match chain.dispatch(host, "STRINGS", &mut Val::map(), &mut vec![]) {
        Outcome::Replied {
            status: 200,
            body: Some(Val::Str(s)),
            ..
        } => s,
        other => panic!("unexpected {other:?}"),
    }
}

const EN: &str = "greeting = Hello, { $who }!\nonly-en = English only\n";
const RU: &str = "greeting = Привет, { $who }!\n";

#[test]
fn a_package_string_resolves_from_its_own_ftl_in_the_users_language() {
    let root = TempDir::new();
    let dir = write_package(&root.0, "greeter", 1, "events = [\"dispatch\"]");
    write_ftl(&dir, "en", EN);
    write_ftl(&dir, "ru", RU);
    let strings = Arc::new(PluginStrings::new());
    let chain = Chain::default();
    let mut set = PackageSet::new(common::runtime(), root.0.clone()).with_strings(strings.clone());
    set.sync(&chain);
    assert_eq!(chain.ids(), vec!["greeter"]);

    assert_eq!(
        asked(&chain, &mut StringsHost::new(&strings, "ru")),
        "Привет, Мир!|English only",
        "Russian, with the English fallback for a key `ru` lacks"
    );
    assert_eq!(
        asked(&chain, &mut StringsHost::new(&strings, "en")),
        "Hello, Мир!|English only"
    );

    // Unloading the package takes its strings with it.
    std::fs::remove_dir_all(&dir).unwrap();
    set.sync(&chain);
    assert_eq!(strings.t("greeter", "ru", "greeting", &[]), "greeting");
}

#[test]
fn a_package_without_ftl_files_answers_with_the_key() {
    let root = TempDir::new();
    write_package(&root.0, "plain", 1, "events = [\"dispatch\"]");
    let strings = Arc::new(PluginStrings::new());
    let chain = Chain::default();
    PackageSet::new(common::runtime(), root.0.clone())
        .with_strings(strings.clone())
        .sync(&chain);
    assert_eq!(
        asked(&chain, &mut StringsHost::new(&strings, "ru")),
        "greeting|only-en"
    );
}

#[test]
fn editing_an_ftl_file_is_picked_up_by_the_hot_reload() {
    let root = TempDir::new();
    let dir = write_package(&root.0, "greeter", 1, "events = [\"dispatch\"]");
    write_ftl(&dir, "en", EN);
    let strings = Arc::new(PluginStrings::new());
    let plugin = PackagePlugin::load_with(&common::runtime(), &dir, Some(strings.clone())).unwrap();
    plugin.set_check_interval(Duration::ZERO);
    let chain = Chain::new(vec![Box::new(plugin) as Box<dyn Plugin>]);
    assert_eq!(
        asked(&chain, &mut StringsHost::new(&strings, "ru")),
        "Hello, Мир!|English only"
    );

    write_ftl(&dir, "ru", RU);
    write_ftl(
        &dir,
        "en",
        "greeting = Hi, { $who }.\nonly-en = English only!\n",
    );
    assert_eq!(
        asked(&chain, &mut StringsHost::new(&strings, "ru")),
        "Привет, Мир!|English only!"
    );
}

#[test]
fn an_oversized_ftl_is_skipped_and_reported() {
    use phoecord_plugin_runtime::package::MAX_FTL_BYTES;
    let root = TempDir::new();
    let dir = write_package(&root.0, "greeter", 1, "events = [\"dispatch\"]");
    write_ftl(&dir, "en", EN);
    let big = format!(
        "greeting = {}
",
        "x".repeat(MAX_FTL_BYTES as usize)
    );
    write_ftl(&dir, "ru", &big);
    let strings = Arc::new(PluginStrings::new());
    let plugin = PackagePlugin::load_with(&common::runtime(), &dir, Some(strings.clone())).unwrap();
    let chain = Chain::new(vec![Box::new(plugin) as Box<dyn Plugin>]);
    let mut host = StringsHost::new(&strings, "ru");
    // Russian was skipped, so English answers.
    assert_eq!(asked(&chain, &mut host), "Hello, Мир!|English only");
    assert!(
        host.inner
            .logs
            .iter()
            .any(|l| l.contains("ru.ftl") && l.contains("skipped")),
        "{:?}",
        host.inner.logs
    );
}
