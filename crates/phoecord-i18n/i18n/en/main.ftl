# PhoeCord shared strings (English, the source language).
# Every message here must exist in ../ru/main.ftl with the same { $variables }.

plugin-enabled = Plugin { $name } enabled.
plugin-disabled = Plugin { $name } disabled.
plugin-unknown = No plugin named { $name }.
plugin-failed = Plugin { $name } failed and was disabled: { $reason }

proxy-listening = Proxy listening on { $addr }.
proxy-stopped = Proxy stopped.
proxy-port-busy = Port { $port } is already in use.

ca-created = Certificate authority created at { $path }.
ca-exists = Certificate authority already exists at { $path }.
ca-trusted = Certificate authority added to the trust store.
ca-trust-failed = Could not add the certificate authority to the trust store: { $reason }

error-generic = Something went wrong: { $reason }
error-config-invalid = The configuration file { $path } is invalid: { $reason }
error-permission-denied = Permission denied. Try running as administrator.

lang-changed = Language set to { $language }.

# CLI: setup, uninstall, paths
cli-intro =
    PhoeCord is a local mod for Discord: it runs a small proxy on this computer and changes what Discord shows.
    Most people want the installer app (PhoeCord.app on a Mac, the PhoeCord setup on Windows), not this program.

    Commands for the command line:
      phoecord setup     set PhoeCord up (certificate, autostart)
      phoecord launch    start Discord through PhoeCord
      phoecord run       run only the proxy, in this window
      phoecord --help    all commands

cli-setup-done = Done. Quit Discord fully, then start it with `phoecord launch`.
cli-uninstall-done = PhoeCord is removed. Restart Discord without `phoecord launch`.
cli-uninstall-incomplete = some steps could not be undone; they stay in install.log, run `phoecord uninstall` again
cli-setup-steps = { $count } new install step(s) recorded in install.log
cli-uninstall-removed = removed  { $what }
cli-uninstall-kept = kept     { $what }
cli-uninstall-failed = failed   { $what }: { $why }
cli-paths-config = config
cli-paths-ca = CA
cli-paths-plugins = plugins
cli-paths-language = language
lang-source-flag = --lang flag
lang-source-config = config.toml
lang-source-os = OS language
lang-source-default = default

# CLI: --help and subcommands
cmd-about = Vencord-style plugins for Discord, through a local proxy
cmd-run = Run the proxy (the default). Point Discord at it with `--proxy-server=http://127.0.0.1:18463`
cmd-launch = Run the proxy and start Discord through it (quit Discord first). Optional: the client command
cmd-setup = First-time setup: write the config, make the local CA, trust it and start the proxy at login
cmd-uninstall = Undo everything `setup` did: autostart, CA trust, the files it created (your config stays)
cmd-trust = Trust the local CA in the certificate stores Discord uses
cmd-untrust = Remove the local CA from those stores
cmd-plugins = List plugins and whether they are on
cmd-enable = Switch a plugin on
cmd-disable = Switch a plugin off
cmd-toggle = Flip a plugin (e.g. `phoecord toggle game_activity_toggle`)
cmd-log = Show the newest deleted and edited messages kept by message_logger
cmd-import-vencord = Switch on the PhoeCord plugins that match your enabled Vencord / Vesktop / Equicord plugins
cmd-selftest = Check everything works against the real Discord (no login needed)
cmd-paths = Print where config, CA and plugin data live
cli-arg-import-path = Path to a Vencord settings.json (found automatically when omitted)
cli-import-none = no Vencord, Vesktop or Equicord settings found; pass one with --path FILE
cli-import-found = Reading { $name } settings: { $path }
cli-import-read = cannot read { $path }
cli-import-summary = Switched on { $enabled }, already on { $already }, no PhoeCord counterpart for { $unmatched } Vencord plugin(s).
cli-help-usage = Usage:
cli-help-commands = Commands
cli-help-options = Options
cli-help-arguments = Arguments
cli-help-help = Print help
cli-help-version = Print version
cli-arg-lang = UI language: en or ru (default: `language` in config.toml, then the OS language)
cli-arg-log-n = How many entries to show (default 20)

# CLI: run, launch
cli-listening = PhoeCord listening on { $listen } (plugins: { $plugins })
cli-starting = starting { $program }
cli-launcher-exited = launcher exited ({ $status }); still serving, Ctrl+C to stop
cli-client-exited = Discord exited ({ $status })
cli-err-listen = listening on { $listen }
cli-err-start-discord = starting Discord
cli-err-no-discord-linux = no Discord found on PATH; pass the command: phoecord launch -- /path/to/discord
cli-err-no-discord-windows = no Discord found in %LOCALAPPDATA%; pass the command: phoecord launch -- C:\path\to\Discord.exe
cli-err-no-discord-macos = no Discord found in /Applications; pass the command: phoecord launch -- /path/to/Discord
cli-error-prefix = error
cli-warning-prefix = warning

# CLI: plugins, enable, disable, toggle
cli-plugin-on = on
cli-plugin-off = off
cli-plugins-vencord = (Vencord: { $name })
cli-no-such-plugin = no plugin named { $name } (see `phoecord plugins`)
cli-plugin-state-on = { $name }: on
cli-plugin-state-off = { $name }: off
cli-gateway-note = (gateway plugin: takes effect when Discord reconnects; Ctrl+R in Discord does that)
cli-config-line = config: { $path }
cli-ca-line = local CA: { $path } (only valid for { $hosts })
plugin-desc-fake_nitro = Send any emoji without Nitro (as linked images)
plugin-desc-game_activity_toggle = While on, your status doesn't show the game you're playing
plugin-desc-message_logger = Keep deleted messages visible and show edit history
plugin-desc-no_track = Block Discord's analytics and crash reports
plugin-desc-relationship_notifier = Notify you when you're unfriended or removed from a server or group
plugin-desc-silent_typing = Your typing indicator is never sent
plugin-desc-clear_urls = Strip tracking parameters from links you send
plugin-desc-unindent = Strip shared indentation from code blocks you send
plugin-desc-friend_invites = Create, list and revoke friend invite links with commands
plugin-desc-petpet = Make a headpat GIF of an avatar or picture with /phoecord petpet
plugin-desc-custom_commands = Your own text snippets with arguments, sent with a command
plugin-desc-spotify_share_commands = Share your current Spotify track, album or artist with a command
plugin-desc-server_info = Show facts about a server: owner, age, boosts, channels, roles
plugin-desc-view_raw = View the raw JSON of a message, channel, server, role or user
plugin-desc-unsuppress_embeds = Show the embeds of a message again, or hide them
plugin-desc-custom_idle = Change or switch off the time before you show as idle
plugin-desc-ignore_activities = Keep chosen games and activities out of your status for others
plugin-desc-auto_dnd_while_playing = Show Do Not Disturb (or another status) while you play a game
plugin-desc-commands = The /phoecord slash command: plugins, toggle
plugin-desc-i_love_spam = Do not hide messages from 'likely spammers'
plugin-desc-no_blocked_messages = Hide all blocked/ignored messages from chat completely
plugin-desc-no_reply_mention = Disables reply pings by default
plugin-desc-text_replace = Replace text in the messages you send
plugin-desc-send_timestamps = Turn `12:34` in your messages into a Discord timestamp
plugin-desc-anonymise_file_names = Anonymise uploaded file names
plugin-desc-silent_message_toggle = Send messages without notifications
plugin-desc-better_sessions = Exact times for your sessions and alerts about new ones
plugin-desc-experiments = Discord experiments and developer features
plugin-desc-implicit_relationships = People you talk to a lot but are not friends with
plugin-desc-member_count = Online, total and voice member counts per server
plugin-desc-message_latency = Messages that took a long time to send
plugin-desc-message_link_embeds = Previews of linked Discord messages
plugin-desc-mutual_group_dms = The group DMs you share with someone
plugin-desc-new_guild_settings = Mute new servers and apply your other settings when you join
plugin-desc-override_forum_defaults = Your own default forum layout and sort order
plugin-desc-pause_invites_forever = Pause invites to a server indefinitely
plugin-desc-platform_indicators = See whether people are on desktop, mobile or web
plugin-desc-read_all_notifications_button = Mark all server notifications as read at once
plugin-desc-show_hidden_channels = List channels you cannot open
plugin-desc-show_hidden_things = Show timeouts, paused invites and other hidden things
plugin-desc-typing_indicator = See who is typing in each channel
plugin-desc-user_voice_show = See who is in voice channels
plugin-desc-valid_reply = Fix “Message could not be loaded” in replies
plugin-desc-valid_user = Fix mentions of unknown users that show as @unknown-user
plugin-desc-vc_narrator = Notify you when people join or leave your voice channel (no speech)
plugin-desc-who_reacted = Who reacted to a message
plugin-desc-fake_deafen = Look deafened to others while still hearing
plugin-desc-fake_deafen_detector = Flag members who speak while shown as muted or deafened
plugin-desc-follow_user = Follow a user from voice channel to voice channel
plugin-desc-anti_rickroll = Find rickroll links in messages
plugin-desc-local_dev_badges = Show staff and developer badges on your own profile, locally only
plugin-desc-voice_chat_utilities = Move, mute, deafen or disconnect everyone in a voice channel
plugin-desc-rmcord = Delete your own messages in bulk
plugin-desc-silent_delete = Replace a message, then delete it, so loggers keep the replacement
plugin-desc-quest_master = Complete Discord Quests automatically

# CLI: log
cli-log-empty = nothing logged yet ({ $path })
cli-log-place-channel = in channel { $channel }
cli-log-place-guild = in guild { $guild }, channel { $channel }
cli-log-edited = { $time }  ✏ { $author } { $place }: { $was } → { $content }{ $tail }
cli-log-deleted = { $time }  🗑 { $author } { $place }: { $content }{ $tail }

# CLI: uninstall report (what was removed)
cli-step-dir = folder { $path }
cli-step-file = file { $path }
cli-step-ca-trusted = CA { $path } from the system trust stores
cli-step-run-key = Run key value { $name }
cli-step-launch-agent = LaunchAgent { $label }
cli-step-systemd-unit = systemd user unit { $unit }
cli-kept-already-gone = already gone
cli-kept-not-empty = not empty
cli-kept-holds-log = holds the log of failed steps
cli-kept-line = kept     { $what } ({ $why })

# CLI: selftest
cli-selftest-rest-ok = ✓ REST: discord.com intercepted, local CA accepted, API reachable
cli-selftest-rest-fail = ✗ REST: { $reason }
cli-selftest-rest-timeout = ✗ REST: timed out
cli-selftest-gateway-ok = ✓ gateway { $combo }: HELLO round-tripped (heartbeat { $ms } ms)
cli-selftest-gateway-fail = ✗ gateway { $combo }: { $reason }
cli-selftest-gateway-timeout = ✗ gateway { $combo }: timed out
cli-selftest-undecodable = ✗ { $count } gateway payload(s) PhoeCord could not decode
cli-selftest-decoded = proxy decoded { $count } payload(s)
cli-selftest-failed = { $count } check(s) failed
cli-selftest-passed = all checks passed
cli-selftest-err-connect-closed = proxy closed the CONNECT
cli-selftest-err-proxy-answered = proxy answered { $line }
cli-selftest-err-http = HTTP { $status }: { $body }
cli-selftest-err-handshake = handshake
cli-selftest-err-closed-before-hello = closed before HELLO
cli-selftest-err-expected-hello = expected HELLO, got op { $op }

# Terms of use (TERMS.md)
terms-prompt = Type `yes` to accept these terms and install; anything else cancels:
terms-declined = Terms not accepted; nothing was installed.
cli-arg-accept-terms = Skip the terms prompt: you have read TERMS.md and accept it

# Bug fixes 1.4.8
cli-uninstall-stopped = stopped  PhoeCord proxy (process { $pid })
cli-uninstall-stop-failed = could not stop a running PhoeCord proxy: { $why }
cli-arg-channel = Which Discord to start: stable, ptb or canary (needed when more than one is installed)
cli-err-channel-not-installed = Discord { $channel } is not installed
cli-err-several-clients = several Discord clients are installed, so none is picked for you. Pass --channel:
    { $list }
cli-proxy-reused = A PhoeCord proxy (version { $version }) already runs on { $listen }; starting Discord through it.
cli-err-port-taken = { $listen } is used by another program, not by PhoeCord. Close it or change `listen` in config.toml.
cli-arg-setup-channel = Add a Start-menu shortcut that starts this Discord through PhoeCord: stable, ptb or canary (default: the only one installed)
cli-shortcut-created = shortcut "{ $name }" added: { $path }
cli-shortcut-unsupported = No shortcut on this system; start Discord with `phoecord launch --channel { $channel }`.
cli-shortcut-skipped = No shortcut added: { $why }
cli-warn-ca-untrusted = Warning: the local CA ({ $path }) is not in your trust store, so Discord will not connect through PhoeCord and may keep restarting. Run `phoecord trust` (or `phoecord setup`) to fix it.

# Installer window (1.4.3)
gui-tab-install = Install
gui-tab-plugins = Plugins
gui-tab-settings = Settings
gui-tab-about = About
gui-dry-run = Dry run
gui-proxy-running = PhoeCord is running
gui-proxy-stopped = PhoeCord is not running
gui-proxy-port-taken = Port in use
gui-proxy-tip = The local proxy on { $listen }. Discord connects through it when started with PhoeCord.
gui-client-stable = Discord
gui-client-ptb = Discord PTB
gui-client-canary = Discord Canary
gui-client-phoenix = Discord Phoenix
gui-version = Version { $version }
gui-version-unknown = Version unknown
gui-state-on = PhoeCord on
gui-state-off = Not installed
gui-state-repair = Needs repair
gui-install-title = Which Discord do you use?
gui-install-title-on = PhoeCord is installed
gui-install-sub = PhoeCord runs next to Discord and starts it through a local proxy. You can undo everything with one button.
gui-install-sub-on = Pick a Discord to open it with PhoeCord, or add PhoeCord to another one.
gui-install-action = Install PhoeCord
gui-open-action = Open { $name }
gui-add-action = Add to { $name }
gui-install-hint = Takes a few seconds. Your system may ask you to confirm the certificate.
gui-open-hint = The menu shortcut "(PhoeCord)" opens it the same way.
gui-repair = Repair
gui-uninstall = Uninstall
gui-repair-banner-title = PhoeCord needs a repair
gui-repair-banner = Your system no longer trusts PhoeCord's certificate, so Discord cannot connect through it. Repair fixes this.
gui-missing-binary-title = A PhoeCord file is missing
gui-missing-binary = The main program was not found at { $path }. Download the full release again and keep all its files in one folder.
gui-no-clients-title = No Discord found
gui-no-clients = If Discord is installed somewhere else, choose its folder. If not, download it, open it once, then check again.
gui-no-clients-looked = Looked in:
gui-choose-folder = Choose folder…
gui-choose-folder-title = Choose the folder Discord is installed in
gui-download-discord = Download Discord
gui-elsewhere = Discord installed somewhere else?
gui-folder-no-discord = No Discord in { $path }. Choose the folder that holds Discord itself.
gui-folder-added = Added { $name }. PhoeCord will look in this folder from now on.
gui-folder-already-listed = { $name } from this folder is already in the list. There is no need to add it again.
gui-folder-save-failed = Could not remember this folder: { $error }. Check that you can write to the settings folder, then try again.
gui-check-again = Check again
gui-install-anyway = Install without Discord
gui-fact-adds-title = What it adds
gui-fact-adds = A certificate that only works for Discord's own sites, a local proxy that starts when you sign in, and a menu shortcut.
gui-fact-never-title = What it never does
gui-fact-never = Send your data anywhere or change Discord's own files.
gui-progress-title = Installing on { $name }
gui-progress-title-plain = Installing PhoeCord
gui-progress-repair-title = Repairing PhoeCord
gui-progress-failed-title = The install stopped
gui-progress-sub = You can keep using your computer. This takes a few seconds.
gui-progress-failed-sub = The steps that finished stay recorded, so Uninstall can still remove them.
gui-phase-cert = Certificate for Discord
gui-phase-cert-sub = Created on this computer and trusted. Your system may ask you to confirm.
gui-phase-autostart = Start when you sign in
gui-phase-autostart-sub = The proxy starts with your computer.
gui-phase-shortcut = Menu shortcut
gui-phase-shortcut-sub = Opens { $name } through PhoeCord.
gui-phase-shortcut-mac = Not needed on macOS: open Discord from this window.
gui-phase-shortcut-none = Skipped: no Discord was picked.
gui-err-cert-title = The certificate was not trusted
gui-err-cert = Your system refused or cancelled the certificate. Try again and confirm the system prompt. If no prompt appears, restart your computer and try once more.
gui-err-autostart-title = Start at sign-in could not be set
gui-err-autostart = PhoeCord still works when you open Discord from this window. Try again; if it keeps failing, copy the details and report it.
gui-err-shortcut-title = The shortcut could not be made
gui-err-shortcut = Everything else is installed. Open Discord from this window, or try again.
gui-err-generic-title = Something went wrong
gui-err-generic = Try again. If it keeps failing, copy the details and report it on GitHub.
gui-try-again = Try again
gui-copy-details = Copy details
gui-back = Back
gui-done = Done
gui-ready-title = PhoeCord is ready
gui-ready-title-repair = Repaired
gui-ready-sub = Open { $name } from here or from its "(PhoeCord)" menu shortcut.
gui-ready-sub-plain = Open Discord from this window once it is installed.
gui-removing-title = Removing PhoeCord
gui-removing-sub = Stopping the proxy and undoing every step it made.
gui-removed-title = PhoeCord is removed
gui-removed-partly-title = PhoeCord is partly removed
gui-removed-sub = Removed { $count } item(s). Your plugins and settings file stay; delete the data folder by hand if you want them gone.
gui-removed-failed-title = Some steps could not be undone
gui-removed-failed = They stay recorded, so Uninstall can retry them. Close Discord and try again.
gui-uninstall-title = Uninstall PhoeCord?
gui-uninstall-body = This removes the certificate, the proxy at sign-in and the shortcuts for every Discord. Discord itself, your plugins and your settings stay.
gui-uninstall-app-windows = This app stays installed. To remove it too, use Settings → Apps.
gui-uninstall-app-macos = This app stays installed. To remove it too, move PhoeCord from Applications to the Trash.
gui-cancel = Cancel
gui-close = Close
gui-save = Save
gui-plugins-title = Plugins
gui-plugins-sub = Switch plugins on and off, or add one from a file or a link. Changes apply without restarting Discord.
gui-plugins-from-file = Install from file
gui-plugins-from-link = From a link
gui-plugins-pick-title = Pick a plugin (.zip or plugin.toml)
gui-plugins-pick-filter = PhoeCord plugin
gui-plugins-drop-hint = Or drop a .zip or a plugin folder on this window.
gui-plugins-search = Search plugins
gui-plugins-yours = Added by you
gui-plugins-builtin = Built in
gui-plugins-none = No plugins added yet. Add a .zip, or a folder with plugin.toml and plugin.wasm.
gui-plugins-no-match = Nothing matches your search.
gui-plugins-meta = { $version } by { $authors }
gui-plugins-meta-version = Version { $version }
gui-plugins-broken = Broken
gui-plugins-remove-label = Remove { $name }
gui-drop-title = Drop to install the plugin
gui-drop-body = A .zip, or a folder with plugin.toml and plugin.wasm.
gui-confirm-plugin-title = Install { $name }?
gui-confirm-plugin-replaces = Replaces the installed version { $version }.
gui-confirm-plugin-trust = Install plugins only from people you trust. PhoeCord limits a plugin to what is listed here.
gui-confirm-install = Install
gui-asks-sees = It will see
gui-asks-sees-nothing = No Discord events
gui-asks-can = It can
gui-asks-can-nothing = Nothing beyond reading the events above
gui-asks-connects = It can connect to
gui-event-rest-request = Requests your Discord sends
gui-event-rest-response = Answers from Discord
gui-event-dispatch = Live events from Discord
gui-event-send = Live events your Discord sends
gui-event-all = all of them
gui-right-kv = Remember its own settings and data
gui-right-notify = Show notifications
gui-right-emit-send = Send live events as you, such as changing your status
gui-right-emit-dispatch = Show made-up events in your Discord, on this computer only
gui-right-guilds = Read server emoji and which server a channel is in
gui-right-fetch = Connect to the sites listed below
gui-right-rest = Use the Discord API as you: send, edit and delete on your behalf
gui-right-plugins = See and switch your other plugins
gui-right-data = Save files in its own folder
gui-link-title = Install from a link
gui-link-sub = Paste an https:// link to a plugin .zip. PhoeCord shows what it asks for before installing.
gui-link-label = Link
gui-link-download = Download
gui-remove-title = Remove { $name }?
gui-remove-body = This deletes the plugin and the data it saved. It cannot be undone.
gui-remove = Remove
gui-pkg-err-not-package = This is not a PhoeCord plugin. Pick a .zip, or a folder with plugin.toml and plugin.wasm.
gui-pkg-err-no-wasm = The package has no plugin.wasm. Ask the author for the full package.
gui-pkg-err-too-big = The plugin is larger than { $mb } MB. Check that you picked the right file.
gui-pkg-err-unsafe = The archive tries to write outside its folder ({ $path }) and was refused. Do not install it.
gui-pkg-err-manifest = The plugin's plugin.toml is wrong: { $why }. Ask the author for a fixed version.
gui-pkg-err-broken = The plugin's code does not load: { $why }. Ask the author for a version that works with this PhoeCord.
gui-pkg-err-https = Only https:// links are allowed.
gui-pkg-err-download = The download failed: { $why }. Check the link and your connection, then try again.
gui-pkg-err-io = Could not write the file: { $why }. Check that the folder is not open elsewhere, then try again.
gui-toast-starting = Starting Discord through PhoeCord
gui-start-failed-title = Discord did not start
gui-start-failed-body = PhoeCord stopped while starting it. What it said:
gui-start-failed-silent = PhoeCord stopped while starting it, without saying why.
gui-start-failed-log = Full output: { $path }
gui-open-log = Open log
gui-open-failed = Could not open it: { $why }
gui-toast-plugin-installed = { $name } is installed and on
gui-toast-plugin-removed = { $name } is removed
gui-toast-gateway = This plugin starts working after Discord reconnects. Restart Discord if it does not.
gui-toast-copied = Details copied
gui-toast-wait = Wait until PhoeCord finishes, then close the window.
gui-settings-title = Settings
gui-settings-lang = Language
gui-settings-lang-hint = For this window and the PhoeCord messages.
gui-settings-lang-system = System
gui-settings-motion = Motion
gui-settings-motion-hint = Turn off animations in this window.
gui-settings-motion-switch = Reduce motion
gui-settings-upstream = Connection through a proxy
gui-settings-upstream-hint = For VPN or zapret users: PhoeCord sends its connections through this proxy. Leave empty to connect directly.
gui-settings-upstream-label = Proxy address
gui-settings-upstream-saved = Saved. New connections use it.
gui-settings-upstream-bad = Use socks5://host:port or http://host:port.
gui-settings-files = Files
gui-settings-files-hint = Where PhoeCord keeps its certificate, plugins and settings.
gui-settings-open-data = Open data folder
gui-settings-open-config = Open settings folder
gui-about-version = Version { $version }
gui-about-body = PhoeCord is an open-source mod loader for Discord, written in Rust. It changes how Discord behaves through a local proxy and plugins, without touching Discord's files.
gui-about-source = Source code
gui-about-report = Report a problem
gui-about-terms = Terms of use
gui-terms-title = Before you install
gui-terms-title-view = Terms of use
gui-terms-sub = Version { $version }. Read to the end, then accept to continue.
gui-terms-accept = I accept
gui-terms-quit = Quit
gui-terms-scroll = Scroll to the end to accept
gui-open-hint-mac = Open it from this window each time: macOS has no PhoeCord shortcut.
gui-ready-sub-mac = Open { $name } from this window whenever you want it with PhoeCord.

## Installer window: first start
gui-onboard-step = Step { $n } of { $total }
gui-onboard-back = Back
gui-onboard-next = Continue
gui-onboard-skip = Skip for now
gui-onboard-later = Not now
gui-onboard-start = Get started
gui-onboard-welcome-title = Welcome to PhoeCord
gui-onboard-welcome-sub = A mod loader for Discord. Three things to know before you start:
gui-onboard-point-proxy-title = It runs next to Discord
gui-onboard-point-proxy = A small local proxy changes what Discord sends and receives. Discord's own files stay untouched.
gui-onboard-point-plugins-title = You pick the plugins
gui-onboard-point-plugins = Add plugins from a file or a link. Before anything installs, you see what it can do.
gui-onboard-point-undo-title = One button undoes it
gui-onboard-point-undo = Uninstall removes the certificate, the proxy and the shortcut, and leaves nothing behind.
gui-onboard-pick-sub = PhoeCord starts this Discord through its proxy. You can add another one later.
gui-onboard-looking = Looking for Discord…
gui-onboard-done-title = You're all set
gui-onboard-done-what = What happens
gui-onboard-done-sub = PhoeCord will be set up for { $name }. It takes a few seconds, and you can undo it with one button.

## Installer window: credits
gui-credits = Credits

# Loader install and reinstall (1.5.6)
cli-step-loader-wanted = loader opt-in for Discord { $channel }
cli-step-loader-copy = loader DLL { $path }
cli-kept-not-ours = changed by another program, left in place
cli-arg-setup-loader = Also install the native loader (version.dll) into the chosen Discord; Windows only. Every `phoecord launch` and autostart puts it back after a Discord update
cli-loader-installed = loader installed for Discord { $channel }: { $path }
cli-loader-up-to-date = loader for Discord { $channel } is already in place: { $path }
cli-loader-foreign = loader not installed for Discord { $channel }: { $path } belongs to another program, so PhoeCord leaves it alone. Remove or rename it yourself if you want the PhoeCord loader there.
cli-loader-source-missing = loader not installed for Discord { $channel }: { $path } is missing (it ships next to the phoecord program)
cli-loader-no-build = loader not installed for Discord { $channel }: no app-x.y.z build in { $path }
cli-loader-failed = loader for Discord { $channel } could not be put in place: { $why }
cli-loader-unsupported = The loader is Windows-only; nothing was installed.
cli-loader-skipped = No loader installed: { $why }
cli-loader-check-failed = could not check the loader: { $why }
