//! Every user-facing string of the CLI, in one place so stage 1.7 can move them to `.ftl` files.

pub const ABOUT: &str = "Vencord-style plugins for Discord, through a local proxy";

pub const CMD_RUN: &str =
    "Run the proxy (the default). Point Discord at it with `--proxy-server=http://127.0.0.1:18463`";
pub const CMD_LAUNCH: &str =
    "Run the proxy and start Discord through it (quit Discord first). Optional: the client command";
pub const CMD_SETUP: &str = "First-time setup: write the config, make the local CA and trust it";
pub const CMD_TRUST: &str = "Trust the local CA in the certificate stores Discord uses";
pub const CMD_UNTRUST: &str = "Remove the local CA from those stores";
pub const CMD_PLUGINS: &str = "List plugins and whether they are on";
pub const CMD_ENABLE: &str = "Switch a plugin on";
pub const CMD_DISABLE: &str = "Switch a plugin off";
pub const CMD_TOGGLE: &str = "Flip a plugin (e.g. `phoecord toggle game_activity_toggle`)";
pub const CMD_SELFTEST: &str = "Check everything works against the real Discord (no login needed)";
pub const CMD_PATHS: &str = "Print where config, CA and plugin data live";

pub fn listening(listen: &str, plugins: &str) -> String {
    format!("PhoeCord listening on {listen} (plugins: {plugins})")
}
pub fn starting(program: &str) -> String {
    format!("starting {program}")
}
pub fn launcher_exited(status: &str) -> String {
    format!("launcher exited ({status}); still serving, Ctrl+C to stop")
}
pub fn client_exited(status: &str) -> String {
    format!("Discord exited ({status})")
}
pub fn no_such_plugin(name: &str) -> String {
    format!("no plugin named {name} (see `phoecord plugins`)")
}
pub fn plugin_state(name: &str, on: bool) -> String {
    format!("{name}: {}", if on { "on" } else { "off" })
}
pub const GATEWAY_NOTE: &str =
    "(gateway plugin: takes effect when Discord reconnects; Ctrl+R in Discord does that)";
pub fn config_line(path: &str) -> String {
    format!("config: {path}")
}
pub fn ca_line(path: &str, hosts: &str) -> String {
    format!("local CA: {path} (only valid for {hosts})")
}
pub const SETUP_DONE: &str = "\nDone. Quit Discord fully, then start it with `phoecord launch`.";
pub fn paths_line(label: &str, path: &str) -> String {
    format!("{label:<8}{path}")
}
