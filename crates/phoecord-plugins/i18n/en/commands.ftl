# The /phoecord command. Every `/phoecord <name>` answer is rendered while this plugin runs, so
# the answers of other plugins' subcommands live here too, with the plugin id as key prefix.
plugins-header = PhoeCord plugins
plugin-off = The `{ $id }` plugin is off. Switch it on with `/phoecord toggle { $id }`.
unknown-command = Unknown command `{ $name }`.
toggle-unknown = No plugin named `{ $id }`. Try `/phoecord plugins`.
toggle-self = `{ $id }` cannot switch itself off here; use `phoecord disable { $id }`.
toggle-on = `{ $id }` is now on.
toggle-off = `{ $id }` is now off.
toggle-reconnect = Takes effect when Discord reconnects (Ctrl+R).
toggle-failed = Could not switch `{ $id }`: { $error }
