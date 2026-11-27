// Wiki copy, English and Russian. Facts come from the repository docs (README, ARCHITECTURE, RUNBOOK,
// SECURITY, GOVERNANCE, docs/plugins/*, docs/phase-2). Update both languages when those change.
// Inline formatting: `code`, **bold**, [text](url).
import type { Lang } from "../i18n";
import type { Block } from "./richtext";

export interface Section { id: string; title: string; blocks: Block[] }
export interface Group { title: string; sections: Section[] }

const REPO = "https://github.com/Nanda070/phoecord";
const DC = "https://discord.gg/cheterin";
const DOCS = REPO + "/blob/main/docs";

const cmdTable = (lang: Lang) => ({
  head: lang === "ru" ? ["Команда", "Что делает"] : ["Command", "What it does"],
  rows:
    lang === "ru"
      ? [
          ["`phoecord setup`", "Первая настройка: пишет конфиг, создаёт локальный CA, доверяет ему и запускает прокси при входе в систему"],
          ["`phoecord launch`", "Запускает прокси и Discord через него (сначала закройте Discord). `--channel stable|ptb|canary|phoenix`, `--transport proxy|in-process`, `--also-inject <DLL>`"],
          ["`phoecord run`", "Запускает только прокси (по умолчанию)"],
          ["`phoecord stop`", "Останавливает работающий прокси этой установки"],
          ["`phoecord uninstall`", "Отменяет всё, что сделал `setup`: автозапуск, доверие к CA, созданные файлы (ваш конфиг остаётся)"],
          ["`phoecord trust` / `untrust`", "Добавляет локальный CA в хранилища сертификатов, которые использует Discord, или убирает его"],
          ["`phoecord plugins`", "Список плагинов и их состояние"],
          ["`phoecord enable|disable|toggle <id>`", "Включает, выключает или переключает плагин"],
          ["`phoecord install <id|url>`", "Ставит плагин из каталога по id или из https-ссылки на .zip"],
          ["`phoecord update [id]`", "Обновляет плагины каталога, у которых есть новая версия"],
          ["`phoecord remove <id>`", "Удаляет установленный пакет плагина"],
          ["`phoecord catalog`", "Каталог плагинов сообщества и что из него установлено"],
          ["`phoecord import-vencord`", "Включает плагины PhoeCord, соответствующие включённым плагинам Vencord / Vesktop / Equicord (`--path FILE` — свой settings.json)"],
          ["`phoecord theme import <css>`", "Превращает тему Vencord (CSS) в `theme.toml`; переносятся только цветовые переменные"],
          ["`phoecord log [-n N]`", "Последние удалённые и изменённые сообщения, сохранённые message_logger"],
          ["`phoecord selftest`", "Проверяет, что всё работает на настоящем Discord (без входа в аккаунт)"],
          ["`phoecord check`", "Проверяет, какие сигнатуры сборки клиента находятся в её exe (только чтение)"],
          ["`phoecord signatures install|list`", "Обновляет файлы сигнатур, которые читает загрузчик (горячее исправление, Windows)"],
          ["`phoecord paths`", "Показывает, где лежат конфиг, CA и данные плагинов"],
          ["`phoecord upgrade`", "Проверяет новый релиз сейчас и скачивает его (ставится при следующем запуске)"],
          ["`phoecord dev console|watch`", "Инструменты для авторов плагинов: живая консоль и горячая пересборка"],
          ["`--lang en|ru`", "Язык интерфейса CLI (по умолчанию `language` из config.toml, затем язык ОС)"],
        ]
      : [
          ["`phoecord setup`", "First-time setup: writes the config, makes the local CA, trusts it and starts the proxy at login"],
          ["`phoecord launch`", "Runs the proxy and starts Discord through it (quit Discord first). `--channel stable|ptb|canary|phoenix`, `--transport proxy|in-process`, `--also-inject <DLL>`"],
          ["`phoecord run`", "Runs the proxy only (the default)"],
          ["`phoecord stop`", "Stops the running proxy of this install"],
          ["`phoecord uninstall`", "Undoes everything `setup` did: autostart, CA trust, the files it created (your config stays)"],
          ["`phoecord trust` / `untrust`", "Adds or removes the local CA in the certificate stores Discord uses"],
          ["`phoecord plugins`", "Lists plugins and whether they are on"],
          ["`phoecord enable|disable|toggle <id>`", "Switches a plugin on, off or flips it"],
          ["`phoecord install <id|url>`", "Installs a plugin from the catalog by id, or from an https link to a .zip"],
          ["`phoecord update [id]`", "Updates catalog plugins that have a newer version"],
          ["`phoecord remove <id>`", "Removes an installed plugin package"],
          ["`phoecord catalog`", "Lists the community plugin catalog and what is installed"],
          ["`phoecord import-vencord`", "Switches on the PhoeCord plugins that match your enabled Vencord / Vesktop / Equicord plugins (`--path FILE` for a specific settings.json)"],
          ["`phoecord theme import <css>`", "Turns a Vencord CSS theme into a `theme.toml`; only its colour variables carry over"],
          ["`phoecord log [-n N]`", "The newest deleted and edited messages kept by message_logger"],
          ["`phoecord selftest`", "Checks everything works against the real Discord (no login needed)"],
          ["`phoecord check`", "Checks which signatures of a client build are found in its exe (read-only)"],
          ["`phoecord signatures install|list`", "Updates the signature files the loader reads (hot-fix, Windows)"],
          ["`phoecord paths`", "Prints where config, CA and plugin data live"],
          ["`phoecord upgrade`", "Checks for a new release now and downloads it (installed at the next start)"],
          ["`phoecord dev console|watch`", "Tools for plugin authors: a live console and hot rebuild"],
          ["`--lang en|ru`", "UI language of the CLI (default: `language` in config.toml, then the OS language)"],
        ],
});

export const WIKI: Record<Lang, Group[]> = {
  en: [
    {
      title: "Start here",
      sections: [
        {
          id: "overview",
          title: "What is PhoeCord",
          blocks: [
            { p: "PhoeCord adds plugins and themes to Discord's native desktop client (Project Phoenix, built with Rust and GPUI) and to the Discord you use today. It is free and open source under GPL-3.0-or-later, has no telemetry and needs no account." },
            { p: "Phoenix is a compiled program, so there is no JavaScript to patch and no stable binary interface between builds. Vencord's approach cannot work there. PhoeCord does the job in two layers, and a Discord update can break at most the smaller one:" },
            {
              table: {
                head: ["Layer", "What it is", "If an update breaks it"],
                rows: [
                  ["**Network**", "A local proxy rewrites Discord's REST and Gateway traffic. Most plugins live here", "Rarely breaks. Fixed in the proxy's codec, not in plugins"],
                  ["**UI**", "A small in-process loader: themes, a settings tab, on-screen plugins, menu entries", "Only the UI turns off, with a notice. New signatures arrive with signed updates"],
                  ["**Plugins**", "One Plugin API; built-in Rust plugins and sandboxed WASM packages from third parties", "Frozen at v1, so old plugins keep loading"],
                ],
              },
            },
            { note: "Status: early. The network layer runs on today's Discord with 60+ plugins, WASM plugins load without a restart, and releases install and update themselves on Windows, macOS and Linux. The UI layer on Phoenix is in progress (Phase 2)." },
          ],
        },
        {
          id: "quick-start",
          title: "Quick start",
          blocks: [
            { ol: [
              "Download the installer for your system from the [Download](/download) page.",
              "Run it. On Windows click **More info**, **Run anyway** if SmartScreen warns; on macOS right-click the app and choose **Open**.",
              "Open PhoeCord, pick the Discord you use and press **Install PhoeCord**. Your system may ask you to confirm the local certificate.",
              "Start Discord through PhoeCord: from its window or the new shortcut (for example \"Discord Phoenix (PhoeCord)\").",
              "Turn plugins on in Settings. Only `no_track` is on by default.",
            ] },
            { p: "In a terminal the same thing is `phoecord setup` followed by `phoecord launch`. Close Discord completely before launching." },
            { note: "Client mods break Discord's Terms of Service. Your account could be limited or banned: use PhoeCord at your own risk." },
          ],
        },
        {
          id: "requirements",
          title: "Requirements",
          blocks: [
            {
              table: {
                head: ["System", "Supported", "Package"],
                rows: [
                  ["Windows", "Windows 10 and 11, x64", "`PhoeCord_<version>_x64-setup.exe`"],
                  ["macOS", "macOS 13 Ventura or later; Apple Silicon and Intel", "`..._aarch64.dmg`, `..._x64.dmg`"],
                  ["Linux", "x86_64", "`..._amd64.deb` (Debian, Ubuntu, Mint), `..._x86_64.rpm` (Fedora, openSUSE), or the CLI archive"],
                ],
              },
            },
            { ul: [
              "Supported Discord clients: Stable, PTB, Canary and Desktop Phoenix.",
              "The UI layer (loader) is Windows-first and aimed at Phoenix; on the Electron client PhoeCord stays network-only.",
              "On Linux, NSS `certutil` must be installed for the certificate to be trusted in Chromium-style stores.",
            ] },
          ],
        },
      ],
    },
    {
      title: "Using PhoeCord",
      sections: [
        {
          id: "install",
          title: "Installing and updating",
          blocks: [
            { p: "**Which file:** Windows `PhoeCord_<version>_x64-setup.exe`; Apple Silicon `PhoeCord_<version>_aarch64.dmg`; Intel Mac `PhoeCord_<version>_x64.dmg`; Debian-family `PhoeCord_<version>_amd64.deb` (`sudo apt install ./<file>`); Fedora-family `PhoeCord_<version>_x86_64.rpm` (`sudo dnf install ./<file>`). The `phoecord-*` archives are the command-line tool only." },
            { p: "**Check before you run.** Releases have no publisher certificate. Compare the file with `SHA256SUMS` from the same release:" },
            { code: "# Windows\nGet-FileHash .\\<file> -Algorithm SHA256\n# macOS\nshasum -a 256 <file>\n# Linux\nsha256sum -c SHA256SUMS --ignore-missing" },
            { p: "`SHA256SUMS` itself is signed with the owner's minisign key: `minisign -Vm SHA256SUMS -p update-key.pub` must print \"Signature and comment signature verified\"." },
            { p: "**Automatic updates.** Once a day an installed PhoeCord checks GitHub for a newer published release (no drafts, no prereleases), downloads the installer, `SHA256SUMS` and `SHA256SUMS.minisig`, verifies them, and installs at the next start of the window or the background proxy, never mid-session. On Linux the window asks for your password." },
            { ul: [
              "Turn it off: Settings in the window, or `[update] auto = false` in `config.toml`.",
              "Check now: `phoecord upgrade`. State is in `updates/state.json` in the data folder.",
              "Update by hand: install the new file over the old one. Settings, the CA and plugins stay in the data folder.",
              "CLI archives and source builds are never updated automatically.",
            ] },
          ],
        },
        {
          id: "launch",
          title: "Starting Discord",
          blocks: [
            { p: "Always start Discord through PhoeCord, so it goes through the proxy or gets the loader." },
            { code: "phoecord launch                      # one client installed\nphoecord launch --channel canary     # stable | ptb | canary | phoenix\nphoecord launch --channel phoenix --transport proxy" },
            { ul: [
              "With several clients installed and no `--channel`, `launch` lists them and stops: it never picks silently.",
              "`phoecord setup --channel canary` adds a Start-menu shortcut \"Discord Canary (PhoeCord)\"; `uninstall` removes it.",
              "If the proxy already runs (autostart), `launch` just starts the client with the proxy flag.",
              "macOS needs 13 (Ventura) or later: the client is started with `open -n -a`.",
            ] },
            { p: "**Phoenix transports.** On Desktop Phoenix `launch` prints one line saying which transport it picked and why: `in-process, no proxy` or `through the PhoeCord proxy`. Force a mode with `--transport proxy` or `--transport in-process` (or `[phoenix] transport` in `config.toml`). A build marked failed stays on the proxy until a signature update; deleting `phoenix-check.json` from the data folder makes the next launch check again." },
            { p: "**Other mods.** On Phoenix another mod's DLL can load next to PhoeCord's loader: `--also-inject <DLL>` (repeatable) or `[phoenix] also_inject` in `config.toml`. A missing file stops the launch." },
          ],
        },
        {
          id: "plugins-use",
          title: "Using plugins",
          blocks: [
            { p: "Plugin names match Vencord's; ids are the `snake_case` form (`MessageLogger` is `message_logger`). Only `no_track` is on by default, as in Vencord." },
            { code: "phoecord plugins                # list with on/off\nphoecord enable message_logger\nphoecord disable fake_nitro\nphoecord toggle game_activity_toggle" },
            { ul: [
              "A **Gateway plugin** takes effect when Discord reconnects: press Ctrl+R in Discord.",
              "Plugin settings live in `config.toml` (section `[plugins.<id>]`) and in the window's Settings; the file is reloaded on change, and a mistake is reported as `config not reloaded` in the log.",
              "Many plugins answer in chat to a `/phoecord ...` command with a local message only you see.",
              "Browse everything, with status and layer, on the [Plugins](/plugins) page.",
            ] },
            {
              table: {
                head: ["Status", "Meaning"],
                rows: [
                  ["Ready", "Works today"],
                  ["Partial", "Works in part; the page says what is missing"],
                  ["Planned", "Not built yet, usually waits for the UI layer on Phoenix"],
                  ["Built in", "PhoeCord replaces it with its own feature (for example crash recovery)"],
                  ["Not applicable", "Does not exist on a native client (DevTools, CSS fixes, web-only features)"],
                ],
              },
            },
          ],
        },
        {
          id: "vencord-migration",
          title: "Coming from Vencord",
          blocks: [
            { ul: [
              "`phoecord import-vencord` reads your Vencord, Vesktop or Equicord `settings.json` (found automatically, or `--path FILE`) and switches on the matching PhoeCord plugins. It tells you how many it enabled and how many have no counterpart.",
              "`phoecord theme import <theme.css>` converts the colour variables of a Vencord CSS theme into a `theme.toml`, and lists what does not carry over.",
              "Vencord plugins that need DOM or CSS (DevTools helpers, CSS fixes) do not exist on a native client and are marked Not applicable.",
              "Visual plugins and Vencord's panels are in scope for Phoenix: they arrive with the UI layer.",
            ] },
          ],
        },
        {
          id: "commands",
          title: "Command reference",
          blocks: [
            { p: "Run `phoecord --help` for the full list in your language. The main commands:" },
            { table: cmdTable("en") },
          ],
        },
        {
          id: "config",
          title: "Configuration",
          blocks: [
            { p: "`phoecord paths` prints where the config, the CA, the plugins and the logs are. The main file is `config.toml` in PhoeCord's data folder." },
            { code: "# config.toml (excerpt)\nlanguage = \"en\"                  # en | ru\nlisten   = \"127.0.0.1:18463\"     # the local proxy\nupstream = \"socks5://127.0.0.1:1080\"   # optional: send PhoeCord's own connections through a VPN or proxy\n\n[update]\nauto = true                      # false = never install updates by itself\n\n[phoenix]\ntransport   = \"auto\"             # auto | proxy | in-process\nfull_ready  = true               # ask the gateway for a full READY on Phoenix\nalso_inject = []                 # other mods' DLLs to load next to the loader\n\n[plugins.music_rich_presence]\nfetch_hosts = [\"my-listenbrainz.example\"]" },
            { ul: [
              "The proxy listens on `127.0.0.1` only and refuses everything else.",
              "`upstream` accepts `socks5://host:port` or `http://host:port`. Credentials in the address are not supported, and the update check does not use it.",
              "`fetch_hosts` lets you add exact host names to a plugin's allowed outside hosts; a plugin cannot write it.",
              "A table left in `config.toml` for a removed plugin is ignored.",
            ] },
          ],
        },
        {
          id: "themes",
          title: "Themes",
          blocks: [
            { p: "A theme is a small TOML file with a name and a palette. The loader reads it and paints the client; while you edit the file, the client follows within two seconds." },
            { code: "name = \"Midnight\"\n\n[colors]\nbackground = \"#101014\"\n\"background.secondary\" = \"rgb(20, 20, 26)\"\ntext = \"hsl(0, 0%, 93%)\"\n\"text.muted\" = \"#9a9aa6cc\"     # with alpha\nbrand = \"#8c9eff\"" },
            { ul: [
              "Colours: `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, `rgb()`/`rgba()`, `hsl()`/`hsla()`.",
              "A bad entry is skipped and reported; the rest of the theme loads.",
              "Built-in themes: `default`, `light` and `high-contrast` (every text colour at least 7:1).",
              "Fields every built-in sets: `background`, `background.secondary`, `background.tertiary`, `background.floating`, `text`, `text.muted`, `text.link`, `brand`, and the status dots `online`, `idle`, `dnd`, `offline`.",
            ] },
            { note: "Status: the format, the colour parser, the watcher and the built-in themes are in. The palette's field names are PhoeCord's own until Phoenix's real palette fields are mapped, so a theme does not repaint the client yet. The format may rename fields when it does." },
          ],
        },
      ],
    },
    {
      title: "Under the hood",
      sections: [
        {
          id: "how-it-works",
          title: "How it works",
          blocks: [
            { p: "**Network layer.** `phoecord launch` starts a local proxy on `127.0.0.1` and starts Discord pointed at it (`--proxy-server`). Discord's API and Gateway connections are terminated by the proxy with a certificate from PhoeCord's local CA, handed to the plugin chain as typed events, and re-encoded. Everything else (CDN, media, voice, other sites) goes through a plain tunnel." },
            { p: "**Plugin chain.** Each plugin subscribes to events (REST requests and responses, Gateway dispatches, outgoing frames, timers, message-menu clicks) and answers with a verdict: `Pass`, `Changed`, `Drop` or `Reply` (a local answer). Plugins can also call back into Discord as your client does (`Host::rest`) with a session header kept in memory only." },
            { p: "**UI layer.** On Windows the loader (`version.dll`, copied next to the client) hooks Phoenix's own functions found by signature tables compiled into PhoeCord. On Phoenix it can run the whole chain **in-process**, with no proxy; the proxy stays as the fallback. If a build is unknown or one signature is missing, only the UI layer turns off, with a notice \"waiting for a signature update\"." },
            { p: "**Plugins as code.** Built-in plugins are Rust crates (`phoecord-plugins`); third-party plugins are WASM components against `wit/phoecord.wit`, run with wasmtime in a sandbox, and are loaded, reloaded and unloaded without a restart. A broken new version leaves the old one running." },
            { p: "**Crates.** `phoecord-core` (config, API types), `phoecord-proxy` (TLS, codecs), `phoecord-transport`, `phoecord-plugins` (built-ins), `phoecord-plugin-runtime` (WASM), `phoecord-plugin-sdk` (+ macros), `phoecord-loader` (the in-process DLL), `phoecord-installer` and `-installer-gui`, `phoecord-catalog`, `phoecord-i18n`, `phoecord-cli`. Where every file lives: [CODE_MAP.md](" + DOCS + "/CODE_MAP.md); the design: [ARCHITECTURE.md](" + DOCS + "/ARCHITECTURE.md)." },
          ],
        },
        {
          id: "security",
          title: "Security model",
          blocks: [
            {
              table: {
                head: ["", ""],
                rows: [
                  ["**Local CA**", "Created on your computer on first run; unique to each install; never sent anywhere; 10-year life; limited by an X.509 name constraint to `discord.com`, `discordapp.com`, `discord.gg` and subdomains. `ca.key` is readable by its owner only"],
                  ["**Proxy**", "Accepts `127.0.0.1` only. Decrypts only Discord's API and Gateway"],
                  ["**Your session**", "The `Authorization` header is held in memory only, never written, never given to plugins; dropped after 30 minutes idle or on a 401. Calls go only to the Discord API host your client uses"],
                  ["**IPC pipe**", "The Windows named pipe `\\\\.\\pipe\\phoecord-<SID>` accepts only your user, only from this computer; it can set `[loader]` keys and report the client build, nothing else"],
                  ["**WASM plugins**", "No files, no network, no process; only the `rights` in `plugin.toml` (none by default); 50 ms soft / 1 s hard time limit per event and 32 MiB of memory"],
                  ["**Updates**", "Only published releases, only when `SHA256SUMS` is signed with the owner's offline minisign key for that exact version and the installer's SHA-256 matches. CI never sees the key"],
                  ["**Telemetry**", "None"],
                ],
              },
            },
            { p: "Remove the CA with `phoecord untrust`, then delete the data folder. A leaked `ca.key` only ever endangered Discord's domains. More: [SECURITY.md](" + REPO + "/blob/main/SECURITY.md) (it includes the audit, findings A1 to A6)." },
            { note: "Found a vulnerability or a leak? Report it privately by DM to `nandak070` on Discord, never in a public issue." },
          ],
        },
        {
          id: "data",
          title: "Files PhoeCord creates",
          blocks: [
            { p: "Run `phoecord paths` for the exact folders on your system. In the data folder:" },
            {
              table: {
                head: ["File", "What it is"],
                rows: [
                  ["`config.toml`", "Your settings. Kept by uninstall"],
                  ["`ca.pem`, `ca.key`", "The local certificate authority and its key (owner-only)"],
                  ["`plugins/<id>/`", "Installed third-party plugin packages. Kept by uninstall"],
                  ["`install.log`", "Every step `setup` made, so `uninstall` can undo it; keeps failed steps until they succeed"],
                  ["`launch.log`, `loader.log`", "What the launcher and the loader did"],
                  ["`updates/state.json`", "Update check state (`last_check`, `pending`, `applied`, `error`)"],
                  ["`phoenix-check.json`", "Cached result of the Phoenix signature check"],
                  ["`signatures/`", "Optional newer signature tables that replace the compiled-in ones"],
                  ["`dev-token`", "Token for `phoecord dev console`, new at every proxy start"],
                ],
              },
            },
            { p: "`phoecord uninstall` also removes what a running PhoeCord left in the data folder (the dev token, `launch.log`, update state with any downloaded installer, the onboarding marker). `config.toml` and plugin packages stay." },
          ],
        },
      ],
    },
    {
      title: "Troubleshooting",
      sections: [
        {
          id: "troubleshooting",
          title: "Troubleshooting",
          blocks: [
            { p: "**The first two minutes:**" },
            { code: "phoecord selftest     # REST + Gateway against the real Discord, no login\nphoecord paths        # where config, CA and logs are\nphoecord plugins      # which plugins are on\nPHOECORD_LOG=phoecord=debug phoecord run   # more logging" },
            { p: "In the log look for `refused a connection`, `gateway:`, `config not reloaded`, `no certificate for`, TLS errors (`UnknownIssuer`, `BadCertificate`) and panics." },
            {
              table: {
                head: ["Symptom", "What to do"],
                rows: [
                  ["Discord shows \"connecting\" forever", "Is the proxy running? Is the port free (`listen`)? Start Discord with `phoecord launch`"],
                  ["Certificate / TLS errors", "`phoecord trust`, then restart Discord. Linux: install NSS `certutil`; Flatpak and Snap have their own NSS database"],
                  ["Works in the browser, not in the app", "The app started without the proxy: quit it fully and use `phoecord launch`"],
                  ["Breaks after enabling a plugin", "`phoecord disable <id>`, reproduce, attach the debug log to an issue"],
                  ["A Gateway plugin does nothing", "Press Ctrl+R in Discord so it reconnects"],
                  ["Config change ignored", "`config not reloaded` in the log means a TOML error"],
                  ["UI plugins off, \"waiting for a signature update\"", "Expected: the client build is unknown. Update PhoeCord; or `phoecord signatures install` for a hot-fix"],
                  ["Loader missing after a Discord update (Windows)", "Discord moved to a new `app-x.y.z` folder. Start Discord with `phoecord launch`: it copies `version.dll` first. Quit Discord fully if the old DLL is locked"],
                  ["\"belongs to another program\"", "Another tool put its own `version.dll` there. Remove or rename it, then `phoecord launch`. PhoeCord never overwrites a file it did not write"],
                  ["Loader cannot reach the proxy", "`loader.log` shows `ipc: no proxy reachable`: the client started without `phoecord run`/`launch`. Harmless; start with `phoecord launch`"],
                  ["Client crashes on start with the loader", "Remove the loader (installer, Uninstall) and file a bug with the client version"],
                  ["Port used by another program", "Change `listen` in `config.toml`"],
                ],
              },
            },
            { p: "**Phoenix in-process limits.** A plugin's local reply and dropping an outgoing gateway frame are not applied in-process yet, and `READY` stays partial there. If this matters, launch with `--transport proxy`. Each hook logs its first call, so one that installed but never saw traffic is easy to spot." },
            { p: "**VPN, zapret, blocked network.** Set `upstream = \"socks5://127.0.0.1:1080\"` in `config.toml` and check with `phoecord selftest`." },
            { p: "**macOS.** \"Operation not permitted\" or \"Installation paused\": update to 0.5.1 or newer. \"Damaged and can't be opened\": System Settings, Privacy and Security, **Open Anyway**, or `xattr -dr com.apple.quarantine /Applications/PhoeCord.app`." },
            { p: "**Antivirus / SmartScreen.** Check the hash against `SHA256SUMS`. If it matches and Defender quarantines `version.dll`, it is a false positive: tell us the release and the detection name in [Discord](" + DC + ")." },
            { p: "**Update refused.** Messages such as \"does not match its line in SHA256SUMS\" or \"does not verify with PhoeCord's key\" mean nothing was installed. Run `phoecord upgrade` again; if the signature message persists, report it privately at once." },
            { p: "**Removing everything by hand** (program already gone): remove the certificate named \"PhoeCord local CA (this computer only)\" with your OS tool, and delete `~/Library/LaunchAgents/dev.phoecord.proxy.plist` (macOS) or `~/.config/systemd/user/phoecord.service` (Linux). Suspect a leak? `phoecord untrust`, delete the data folder, report privately." },
            { p: "The full runbook: [RUNBOOK.md](" + DOCS + "/RUNBOOK.md)." },
          ],
        },
      ],
    },
    {
      title: "For developers",
      sections: [
        {
          id: "plugin-dev",
          title: "Writing a plugin",
          blocks: [
            { p: "A third-party plugin is one **WASM component** built against `wit/phoecord.wit` (world `plugin`, package `phoecord:plugin@1.0.0`) plus a `plugin.toml` next to it, in `plugins/<id>/` of the data folder. The Plugin API is **frozen at v1**: a plugin built today keeps loading in every later 1.x." },
            { code: "my_plugin/\n├── plugin.toml        # id, api = 1, events, rights, fetch hosts, [[menu]], [[setting]]\n├── plugin.wasm        # the component\n└── i18n/              # optional\n    ├── en.ftl\n    └── ru.ftl" },
            { ul: [
              "The component exports `manifest()`, `wants(kind)` and `on-event(event)`; you answer each event with `Pass`, `Changed`, `Drop` or `Reply`.",
              "It has no sockets, files, clock or WASI. Everything goes through the `host` interface, and every call needs a **right** listed in `plugin.toml`.",
              "The user sees events and rights and approves before install. Ask for the least.",
              "Never log message text or what the user typed; do not log `request_query()`.",
            ] },
            {
              table: {
                head: ["", "Rust", "TypeScript"],
                rows: [
                  ["Tooling", "`phoecord-plugin-sdk`, `#[phoecord::plugin]`, target `wasm32-wasip2`", "componentize-js with `jco`, `tsc`, esbuild"],
                  ["Size", "tens of kilobytes", "about 12 MB (a JavaScript engine is inside)"],
                  ["Load time", "well under a second", "about 17 s per load in a release build"],
                  ["Template", "`examples/hello-plugin`", "`examples/hello-plugin-ts`"],
                ],
              },
            },
            { p: "Develop with `phoecord dev console` (live plugin logs, events, verdicts and timings; toggle, reload, replay) and `phoecord dev watch` (rebuild on change and hot-reload). Install your build with `phoecord install`. Guides: [quick start](" + DOCS + "/plugins/quick-start.md), [events](" + DOCS + "/plugins/events.md), [rights](" + DOCS + "/plugins/rights.md), [publishing](" + DOCS + "/plugins/publishing.md), [localization](" + DOCS + "/plugins/localization.md), [themes](" + DOCS + "/plugins/themes.md)." },
          ],
        },
        {
          id: "events",
          title: "Events and verdicts",
          blocks: [
            { ul: [
              "**REST** request and response events: filter with `\"METHOD route\"` (with a space), for example `\"POST /channels/*/messages\"`.",
              "**Gateway** dispatches (incoming) and outgoing frames, plus `PHOECORD_LOCAL_DELETE` (your own message deletes, before Discord confirms) and `PHOECORD_TIMER`.",
              "**UI events**: `ui:menu_action`, `ui:user_menu_action`, `ui:channel_menu_action`, `ui:server_menu_action` when a plugin's `[[menu]]` entry is clicked in Phoenix's context menus.",
              "Messages sent or edited with files (`multipart/form-data`) reach plugins with `payload_json` as the body.",
            ] },
            {
              table: {
                head: ["Verdict", "Effect"],
                rows: [
                  ["`Pass`", "Leave it as it is"],
                  ["`Changed`", "Replace the body or payload with yours"],
                  ["`Drop`", "Do not forward it"],
                  ["`Reply`", "Answer locally without reaching Discord (for example a `/phoecord` command)"],
                ],
              },
            },
            { p: "What each event carries and which verdicts it honours: [events.md](" + DOCS + "/plugins/events.md)." },
          ],
        },
        {
          id: "rights",
          title: "Rights and limits",
          blocks: [
            { p: "A call without its right returns an error to the plugin. Typical rights cover reading or changing events, calling Discord's API as the user (`rest`), sending frames (`emit_send`), fetching from allowed outside hosts (`fetch` with a host list), timers and settings. The exact table with the `Host` methods behind each right is in [rights.md](" + DOCS + "/plugins/rights.md)." },
            { ul: [
              "Time: 50 ms soft limit with strikes, 1 s hard limit per event. A plugin that hits its limit or panics is disabled and the log names it.",
              "Memory: 32 MiB.",
              "Outside fetches go only to hosts in the plugin's `fetch` list; the user's session is never attached.",
              "`rest` and `emit_send` act as the user: say so in your description if it could get an account flagged.",
            ] },
          ],
        },
        {
          id: "publishing",
          title: "Publishing",
          blocks: [
            { ul: [
              "Lay the package out as above, fill in `plugin.toml`, and version it with SemVer. v1 freeze means additive changes only.",
              "Describe settings with `[[setting]]` (kinds: bool, number, text, choice, list) so the settings tab can draw them; label menu entries and settings in `i18n/<lang>.ftl`.",
              "Users install with `phoecord install <id>` (catalog) or `phoecord install <https://...zip>`; `phoecord catalog` lists what is available, `phoecord update` updates.",
              "Community catalog: `catalog/plugins.json` in the repository. The review checks the rights you ask for.",
              "Licence: your plugin may be any licence; the SDK, `wit/` and templates are MIT.",
            ] },
          ],
        },
        {
          id: "porting",
          title: "Porting a Vencord plugin",
          blocks: [
            { p: "To build a plugin into PhoeCord itself as a native Rust plugin (`crates/phoecord-plugins/`), follow [PORTING_PLUGINS.md](" + DOCS + "/PORTING_PLUGINS.md). Both projects are GPL-3.0-or-later, so Vencord code may be adapted with attribution. Every plugin is mapped in [PLUGINS.md](" + DOCS + "/PLUGINS.md): name, layer (network, UI, theme), the version it targets and its status." },
          ],
        },
        {
          id: "build",
          title: "Building from source",
          blocks: [
            { code: "git clone https://github.com/nanda070/phoecord && cd phoecord\ncargo run                       # Rust 1.88+\nbash scripts/check.sh           # all checks\n# Windows: powershell -File scripts/check.ps1" },
            { p: "The website is an Astro project in `site/`: `npm ci`, `npm test`, `npm run build` there." },
          ],
        },
        {
          id: "contributing",
          title: "Contributing and governance",
          blocks: [
            { ul: [
              "Work through branches and pull requests: [CONTRIBUTING.md](" + REPO + "/blob/main/CONTRIBUTING.md). Plugins, tests, fixtures, docs and research are open to everyone.",
              "Every pull request is assigned to Nanda070 and goes through their review. Changes to the CA, TLS, trust stores, the proxy and the loader always need the owner's review.",
              "Disputed technical questions are decided by the project lead and recorded as a ruling in [DECISIONS.md](" + DOCS + "/DECISIONS.md).",
              "Bugs and ideas: [Issues](" + REPO + "/issues). Vulnerabilities and leaks: privately only.",
              "Rules for people and AI agents: [AGENTS.md](" + DOCS + "/AGENTS.md).",
            ] },
          ],
        },
        {
          id: "releases",
          title: "How releases work",
          blocks: [
            { ol: [
              "The owner bumps `[workspace.package] version` in `Cargo.toml` and adds a `## [<version>]` section to `docs/CHANGELOG.md`; it becomes the release notes.",
              "The owner pushes a tag `v<version>`. A tag with a `-` (for example `v0.5.0-rc.1`) is a prerelease. Contributors never push `v*` tags.",
              "CI builds the CLI for Linux, Windows and macOS (Intel and Apple Silicon), the Windows loader and the installers (`.exe`, `.dmg`, `.deb`, `.rpm`), writes one `SHA256SUMS` and leaves a **draft** release.",
              "The owner runs `scripts/sign-release.ps1 -Tag v<version>`: it verifies every file, signs `SHA256SUMS` with the offline minisign key, uploads `SHA256SUMS.minisig` and publishes.",
              "Publishing a release posts a short update to the community Discord with what changed and the download link.",
              "Installed copies pick up the signed release within a day.",
            ] },
          ],
        },
        {
          id: "glossary",
          title: "Glossary",
          blocks: [
            {
              table: {
                head: ["Term", "Meaning"],
                rows: [
                  ["Phoenix", "Discord's new native desktop client (Rust, GPUI)"],
                  ["Electron client", "Today's Discord Stable, PTB and Canary"],
                  ["CA", "Certificate authority: here, PhoeCord's own, local and limited to Discord's domains"],
                  ["Proxy", "The local program that sits between your client and Discord"],
                  ["Loader", "The in-process DLL (`version.dll`) for the UI layer"],
                  ["Transport", "How PhoeCord reaches traffic: through the proxy or in-process"],
                  ["Signature", "A byte pattern that finds a function in a given Phoenix build"],
                  ["Gateway", "Discord's real-time WebSocket connection"],
                  ["REST", "Discord's HTTP API"],
                  ["Verdict", "A plugin's answer to an event: Pass, Changed, Drop, Reply"],
                  ["Right", "A permission a WASM plugin declares in `plugin.toml`"],
                  ["WIT / WASM", "The interface language and binary format of third-party plugins"],
                  ["Phase 2", "The roadmap from Phoenix Canary to v1.0: in-process transport, themes, settings tab, full Vencord parity"],
                ],
              },
            },
          ],
        },
      ],
    },
  ],
  ru: [
    {
      title: "С чего начать",
      sections: [
        {
          id: "overview",
          title: "Что такое PhoeCord",
          blocks: [
            { p: "PhoeCord добавляет плагины и темы в нативный клиент Discord для компьютера (Project Phoenix, написан на Rust и GPUI) и в Discord, которым вы пользуетесь сейчас. Бесплатно, открытый код под GPL-3.0-or-later, без телеметрии и без аккаунта." },
            { p: "Phoenix — скомпилированная программа: JavaScript для патча нет, и нет стабильного бинарного интерфейса между сборками. Подход Vencord там не работает. PhoeCord решает задачу в два слоя, и обновление Discord может сломать самое большее меньший из них:" },
            {
              table: {
                head: ["Слой", "Что это", "Если обновление его сломало"],
                rows: [
                  ["**Сеть**", "Локальный прокси переписывает REST- и Gateway-трафик Discord. Здесь живёт большинство плагинов", "Ломается редко. Чинится в кодеке прокси, а не в плагинах"],
                  ["**UI**", "Небольшой загрузчик внутри процесса: темы, вкладка настроек, плагины на экране, пункты меню", "Отключается только UI, с уведомлением. Новые сигнатуры приходят с подписанными обновлениями"],
                  ["**Плагины**", "Один Plugin API; встроенные плагины на Rust и сторонние WASM-пакеты в песочнице", "Заморожен на v1, старые плагины продолжают загружаться"],
                ],
              },
            },
            { note: "Статус: ранний. Сетевой слой работает на нынешнем Discord с 60+ плагинами, WASM-плагины загружаются без перезапуска, релизы ставятся и обновляются сами в Windows, macOS и Linux. UI-слой на Phoenix в разработке (Фаза 2)." },
          ],
        },
        {
          id: "quick-start",
          title: "Быстрый старт",
          blocks: [
            { ol: [
              "Скачайте установщик для своей системы на странице [Скачать](/ru/download).",
              "Запустите его. В Windows при предупреждении SmartScreen нажмите **Подробнее**, **Выполнить в любом случае**; в macOS нажмите на приложении правой кнопкой и выберите **Открыть**.",
              "Откройте PhoeCord, выберите свой Discord и нажмите **Установить PhoeCord**. Система может попросить подтвердить локальный сертификат.",
              "Запустите Discord через PhoeCord: из его окна или через новый ярлык (например, «Discord Phoenix (PhoeCord)»).",
              "Включите плагины в настройках. По умолчанию включён только `no_track`.",
            ] },
            { p: "В терминале то же самое: `phoecord setup`, затем `phoecord launch`. Перед запуском полностью закройте Discord." },
            { note: "Моды клиента нарушают Условия использования Discord. Аккаунт могут ограничить или забанить: используйте PhoeCord на свой риск." },
          ],
        },
        {
          id: "requirements",
          title: "Требования",
          blocks: [
            {
              table: {
                head: ["Система", "Поддерживается", "Пакет"],
                rows: [
                  ["Windows", "Windows 10 и 11, x64", "`PhoeCord_<версия>_x64-setup.exe`"],
                  ["macOS", "macOS 13 Ventura и новее; Apple Silicon и Intel", "`..._aarch64.dmg`, `..._x64.dmg`"],
                  ["Linux", "x86_64", "`..._amd64.deb` (Debian, Ubuntu, Mint), `..._x86_64.rpm` (Fedora, openSUSE) или архив CLI"],
                ],
              },
            },
            { ul: [
              "Поддерживаемые клиенты Discord: Stable, PTB, Canary и Desktop Phoenix.",
              "UI-слой (загрузчик) делается сначала для Windows и Phoenix; в Electron-клиенте PhoeCord остаётся только сетевым.",
              "В Linux должна быть установлена NSS `certutil`, чтобы сертификат считался доверенным в хранилищах уровня Chromium.",
            ] },
          ],
        },
      ],
    },
    {
      title: "Работа с PhoeCord",
      sections: [
        {
          id: "install",
          title: "Установка и обновление",
          blocks: [
            { p: "**Какой файл:** Windows — `PhoeCord_<версия>_x64-setup.exe`; Apple Silicon — `PhoeCord_<версия>_aarch64.dmg`; Intel Mac — `PhoeCord_<версия>_x64.dmg`; семейство Debian — `PhoeCord_<версия>_amd64.deb` (`sudo apt install ./<файл>`); семейство Fedora — `PhoeCord_<версия>_x86_64.rpm` (`sudo dnf install ./<файл>`). Архивы `phoecord-*` — только консольная утилита." },
            { p: "**Проверьте перед запуском.** У релизов нет сертификата издателя. Сверьте файл с `SHA256SUMS` из того же релиза:" },
            { code: "# Windows\nGet-FileHash .\\<файл> -Algorithm SHA256\n# macOS\nshasum -a 256 <файл>\n# Linux\nsha256sum -c SHA256SUMS --ignore-missing" },
            { p: "Сам `SHA256SUMS` подписан ключом minisign владельца: `minisign -Vm SHA256SUMS -p update-key.pub` должен вывести «Signature and comment signature verified»." },
            { p: "**Автообновление.** Раз в сутки установленный PhoeCord проверяет на GitHub более новый опубликованный релиз (не черновик и не пре-релиз), скачивает установщик, `SHA256SUMS` и `SHA256SUMS.minisig`, проверяет их и устанавливает при следующем запуске окна или фонового прокси, но не посреди сессии. В Linux окно спросит пароль." },
            { ul: [
              "Отключить: настройки окна или `[update] auto = false` в `config.toml`.",
              "Проверить сейчас: `phoecord upgrade`. Состояние — в `updates/state.json` в папке данных.",
              "Обновить вручную: поставьте новый файл поверх старого. Настройки, CA и плагины остаются в папке данных.",
              "Консольные архивы и сборки из исходников автоматически не обновляются.",
            ] },
          ],
        },
        {
          id: "launch",
          title: "Запуск Discord",
          blocks: [
            { p: "Всегда запускайте Discord через PhoeCord: так он идёт через прокси или получает загрузчик." },
            { code: "phoecord launch                      # установлен один клиент\nphoecord launch --channel canary     # stable | ptb | canary | phoenix\nphoecord launch --channel phoenix --transport proxy" },
            { ul: [
              "Если клиентов несколько и `--channel` не указан, `launch` покажет список и остановится: он никогда не выбирает молча.",
              "`phoecord setup --channel canary` добавляет ярлык в меню «Пуск» «Discord Canary (PhoeCord)»; `uninstall` его убирает.",
              "Если прокси уже работает (автозапуск), `launch` просто запускает клиент с флагом прокси.",
              "В macOS нужна версия 13 (Ventura) и новее: клиент запускается через `open -n -a`.",
            ] },
            { p: "**Транспорты Phoenix.** На Desktop Phoenix `launch` печатает одну строку: какой транспорт выбран и почему, `in-process, no proxy` или `through the PhoeCord proxy`. Режим можно задать флагом `--transport proxy` или `--transport in-process` (или `[phoenix] transport` в `config.toml`). Сборка, помеченная как неудачная, остаётся на прокси до обновления сигнатур; если удалить `phoenix-check.json` из папки данных, следующий запуск проверит снова." },
            { p: "**Другие моды.** В Phoenix DLL другого мода может загрузиться рядом с загрузчиком PhoeCord: `--also-inject <DLL>` (можно повторять) или `[phoenix] also_inject` в `config.toml`. Если файла нет, запуск останавливается." },
          ],
        },
        {
          id: "plugins-use",
          title: "Работа с плагинами",
          blocks: [
            { p: "Названия плагинов совпадают с Vencord; id — форма `snake_case` (`MessageLogger` — это `message_logger`). По умолчанию включён только `no_track`, как и в Vencord." },
            { code: "phoecord plugins                # список с состоянием\nphoecord enable message_logger\nphoecord disable fake_nitro\nphoecord toggle game_activity_toggle" },
            { ul: [
              "**Плагин Gateway** вступает в силу при переподключении Discord: нажмите Ctrl+R в Discord.",
              "Настройки плагинов лежат в `config.toml` (раздел `[plugins.<id>]`) и в настройках окна; файл перечитывается при изменении, а ошибка отмечается в логе как `config not reloaded`.",
              "Многие плагины отвечают в чате на команду `/phoecord ...` локальным сообщением, видимым только вам.",
              "Все плагины со статусом и слоем — на странице [Плагины](/ru/plugins).",
            ] },
            {
              table: {
                head: ["Статус", "Значение"],
                rows: [
                  ["Готов", "Работает уже сегодня"],
                  ["Частично", "Работает не целиком; на странице сказано, чего не хватает"],
                  ["В планах", "Ещё не сделан, обычно ждёт UI-слоя на Phoenix"],
                  ["Встроено", "PhoeCord заменяет его своей функцией (например, восстановление после сбоя)"],
                  ["Неприменимо", "В нативном клиенте не существует (DevTools, правки CSS, веб-функции)"],
                ],
              },
            },
          ],
        },
        {
          id: "vencord-migration",
          title: "Переход с Vencord",
          blocks: [
            { ul: [
              "`phoecord import-vencord` читает `settings.json` вашего Vencord, Vesktop или Equicord (находит сам или `--path FILE`) и включает соответствующие плагины PhoeCord. Он сообщает, сколько включил и для скольких нет аналога.",
              "`phoecord theme import <theme.css>` превращает цветовые переменные CSS-темы Vencord в `theme.toml` и перечисляет, что не перенеслось.",
              "Плагины Vencord, которым нужны DOM или CSS (помощники DevTools, правки CSS), в нативном клиенте не существуют и помечены как «Неприменимо».",
              "Визуальные плагины и панели Vencord входят в план для Phoenix: они придут вместе с UI-слоем.",
            ] },
          ],
        },
        {
          id: "commands",
          title: "Справочник команд",
          blocks: [
            { p: "`phoecord --help` покажет полный список на вашем языке. Основные команды:" },
            { table: cmdTable("ru") },
          ],
        },
        {
          id: "config",
          title: "Настройка",
          blocks: [
            { p: "`phoecord paths` показывает, где лежат конфиг, CA, плагины и логи. Основной файл — `config.toml` в папке данных PhoeCord." },
            { code: "# config.toml (фрагмент)\nlanguage = \"ru\"                  # en | ru\nlisten   = \"127.0.0.1:18463\"     # локальный прокси\nupstream = \"socks5://127.0.0.1:1080\"   # необязательно: соединения самого PhoeCord через VPN или прокси\n\n[update]\nauto = true                      # false = никогда не ставить обновления самому\n\n[phoenix]\ntransport   = \"auto\"             # auto | proxy | in-process\nfull_ready  = true               # просить у gateway полный READY в Phoenix\nalso_inject = []                 # DLL других модов рядом с загрузчиком\n\n[plugins.music_rich_presence]\nfetch_hosts = [\"my-listenbrainz.example\"]" },
            { ul: [
              "Прокси слушает только `127.0.0.1` и отклоняет всё остальное.",
              "`upstream` принимает `socks5://host:port` или `http://host:port`. Логин и пароль в адресе не поддерживаются, проверка обновлений его не использует.",
              "`fetch_hosts` позволяет добавить точные имена хостов в список внешних хостов плагина; сам плагин записать туда ничего не может.",
              "Раздел в `config.toml` для удалённого плагина игнорируется.",
            ] },
          ],
        },
        {
          id: "themes",
          title: "Темы",
          blocks: [
            { p: "Тема — небольшой TOML-файл с названием и палитрой. Загрузчик читает его и красит клиент; пока вы правите файл, клиент меняется в течение двух секунд." },
            { code: "name = \"Midnight\"\n\n[colors]\nbackground = \"#101014\"\n\"background.secondary\" = \"rgb(20, 20, 26)\"\ntext = \"hsl(0, 0%, 93%)\"\n\"text.muted\" = \"#9a9aa6cc\"     # с альфой\nbrand = \"#8c9eff\"" },
            { ul: [
              "Цвета: `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, `rgb()`/`rgba()`, `hsl()`/`hsla()`.",
              "Неверная запись пропускается с сообщением; остальная тема загружается.",
              "Встроенные темы: `default`, `light` и `high-contrast` (каждый цвет текста контрастнее 7:1).",
              "Поля, которые задаёт каждая встроенная тема: `background`, `background.secondary`, `background.tertiary`, `background.floating`, `text`, `text.muted`, `text.link`, `brand` и индикаторы статуса `online`, `idle`, `dnd`, `offline`.",
            ] },
            { note: "Статус: формат, разбор цветов, наблюдатель за файлом и встроенные темы готовы. Названия полей палитры пока собственные у PhoeCord, пока не сопоставлены реальные поля Phoenix, поэтому тема клиент пока не перекрашивает. Когда это случится, поля могут быть переименованы." },
          ],
        },
      ],
    },
    {
      title: "Как это устроено",
      sections: [
        {
          id: "how-it-works",
          title: "Как это работает",
          blocks: [
            { p: "**Сетевой слой.** `phoecord launch` запускает локальный прокси на `127.0.0.1` и стартует Discord, направленный на него (`--proxy-server`). API- и Gateway-соединения Discord прокси завершает сам с сертификатом локального CA PhoeCord, передаёт цепочке плагинов как типизированные события и кодирует обратно. Всё остальное (CDN, медиа, голос, другие сайты) идёт через обычный туннель." },
            { p: "**Цепочка плагинов.** Каждый плагин подписывается на события (REST-запросы и ответы, Gateway-события, исходящие кадры, таймеры, клики в меню сообщения) и отвечает вердиктом: `Pass`, `Changed`, `Drop` или `Reply` (локальный ответ). Плагины могут и сами обращаться к Discord, как ваш клиент (`Host::rest`), с заголовком сессии, который хранится только в памяти." },
            { p: "**UI-слой.** В Windows загрузчик (`version.dll`, копируется рядом с клиентом) перехватывает функции Phoenix, найденные по таблицам сигнатур, встроенным в PhoeCord. В Phoenix он может выполнять всю цепочку **внутри процесса**, без прокси; прокси остаётся запасным вариантом. Если сборка неизвестна или не хватает одной сигнатуры, отключается только UI-слой, с уведомлением «waiting for a signature update»." },
            { p: "**Плагины как код.** Встроенные плагины — крейты Rust (`phoecord-plugins`); сторонние — WASM-компоненты по `wit/phoecord.wit`, работают под wasmtime в песочнице и загружаются, перезагружаются и выгружаются без перезапуска. Сломанная новая версия оставляет работать старую." },
            { p: "**Крейты.** `phoecord-core` (конфиг, типы API), `phoecord-proxy` (TLS, кодеки), `phoecord-transport`, `phoecord-plugins` (встроенные), `phoecord-plugin-runtime` (WASM), `phoecord-plugin-sdk` (+ макросы), `phoecord-loader` (DLL внутри процесса), `phoecord-installer` и `-installer-gui`, `phoecord-catalog`, `phoecord-i18n`, `phoecord-cli`. Где что лежит: [CODE_MAP.md](" + DOCS + "/CODE_MAP.md); устройство: [ARCHITECTURE.md](" + DOCS + "/ARCHITECTURE.md)." },
          ],
        },
        {
          id: "security",
          title: "Модель безопасности",
          blocks: [
            {
              table: {
                head: ["", ""],
                rows: [
                  ["**Локальный CA**", "Создаётся на вашем компьютере при первом запуске; уникален для каждой установки; никуда не отправляется; срок жизни 10 лет; ограничением имён X.509 привязан к `discord.com`, `discordapp.com`, `discord.gg` и поддоменам. `ca.key` читается только владельцем"],
                  ["**Прокси**", "Принимает только `127.0.0.1`. Расшифровывает только API и Gateway Discord"],
                  ["**Ваша сессия**", "Заголовок `Authorization` хранится только в памяти, не записывается и не передаётся плагинам; сбрасывается через 30 минут простоя или при 401. Вызовы идут только на хост API Discord, который использует ваш клиент"],
                  ["**IPC-канал**", "Именованный канал Windows `\\\\.\\pipe\\phoecord-<SID>` принимает только вашего пользователя и только с этого компьютера; через него можно менять ключи `[loader]` и сообщать сборку клиента, больше ничего"],
                  ["**WASM-плагины**", "Без файлов, сети и процесса; только права из `plugin.toml` (по умолчанию никаких); лимит времени на событие 50 мс мягкий / 1 с жёсткий и 32 МиБ памяти"],
                  ["**Обновления**", "Только опубликованные релизы, только если `SHA256SUMS` подписан офлайн-ключом minisign владельца для именно этой версии и SHA-256 установщика совпадает. CI ключа не видит"],
                  ["**Телеметрия**", "Нет"],
                ],
              },
            },
            { p: "Убрать CA: `phoecord untrust`, затем удалить папку данных. Утёкший `ca.key` угрожал бы только доменам Discord. Подробнее: [SECURITY.md](" + REPO + "/blob/main/SECURITY.md) (там же аудит, находки A1–A6)." },
            { note: "Нашли уязвимость или утечку? Сообщите приватно личным сообщением `nandak070` в Discord, не в публичном issue." },
          ],
        },
        {
          id: "data",
          title: "Какие файлы создаёт PhoeCord",
          blocks: [
            { p: "`phoecord paths` покажет точные папки в вашей системе. В папке данных:" },
            {
              table: {
                head: ["Файл", "Что это"],
                rows: [
                  ["`config.toml`", "Ваши настройки. При удалении сохраняется"],
                  ["`ca.pem`, `ca.key`", "Локальный центр сертификации и его ключ (только для владельца)"],
                  ["`plugins/<id>/`", "Установленные пакеты сторонних плагинов. При удалении сохраняются"],
                  ["`install.log`", "Каждый шаг `setup`, чтобы `uninstall` мог его отменить; неудавшиеся шаги хранятся, пока не выполнятся"],
                  ["`launch.log`, `loader.log`", "Что делали запускатор и загрузчик"],
                  ["`updates/state.json`", "Состояние проверки обновлений (`last_check`, `pending`, `applied`, `error`)"],
                  ["`phoenix-check.json`", "Кэш результата проверки сигнатур Phoenix"],
                  ["`signatures/`", "Необязательные более новые таблицы сигнатур, заменяющие встроенные"],
                  ["`dev-token`", "Токен для `phoecord dev console`, новый при каждом запуске прокси"],
                ],
              },
            },
            { p: "`phoecord uninstall` также удаляет то, что работающий PhoeCord оставил в папке данных (токен разработчика, `launch.log`, состояние обновлений со скачанным установщиком, отметку онбординга). `config.toml` и пакеты плагинов остаются." },
          ],
        },
      ],
    },
    {
      title: "Если что-то не так",
      sections: [
        {
          id: "troubleshooting",
          title: "Решение проблем",
          blocks: [
            { p: "**Первые две минуты:**" },
            { code: "phoecord selftest     # REST + Gateway на настоящем Discord, без входа\nphoecord paths        # где конфиг, CA и логи\nphoecord plugins      # какие плагины включены\nPHOECORD_LOG=phoecord=debug phoecord run   # подробнее в лог" },
            { p: "В логе ищите `refused a connection`, `gateway:`, `config not reloaded`, `no certificate for`, ошибки TLS (`UnknownIssuer`, `BadCertificate`) и паники." },
            {
              table: {
                head: ["Симптом", "Что делать"],
                rows: [
                  ["Discord вечно «подключается»", "Работает ли прокси? Свободен ли порт (`listen`)? Запускайте через `phoecord launch`"],
                  ["Ошибки сертификата / TLS", "`phoecord trust`, затем перезапустите Discord. Linux: поставьте NSS `certutil`; у Flatpak и Snap свои базы NSS"],
                  ["В браузере работает, в приложении нет", "Приложение запущено без прокси: закройте его полностью и используйте `phoecord launch`"],
                  ["Ломается после включения плагина", "`phoecord disable <id>`, воспроизведите, приложите отладочный лог к issue"],
                  ["Плагин Gateway ничего не делает", "Нажмите Ctrl+R в Discord, чтобы он переподключился"],
                  ["Изменение конфига не применилось", "`config not reloaded` в логе — ошибка в TOML"],
                  ["UI-плагины выключены, «waiting for a signature update»", "Ожидаемо: сборка клиента неизвестна. Обновите PhoeCord; либо `phoecord signatures install` для горячего исправления"],
                  ["Загрузчик пропал после обновления Discord (Windows)", "Discord переехал в новую папку `app-x.y.z`. Запустите через `phoecord launch`: он сначала скопирует `version.dll`. Если старая DLL занята, полностью закройте Discord"],
                  ["«belongs to another program»", "Другая программа положила свой `version.dll`. Удалите или переименуйте его, затем `phoecord launch`. PhoeCord никогда не перезаписывает файл, который не писал сам"],
                  ["Загрузчик не достучался до прокси", "`loader.log`: `ipc: no proxy reachable` — клиент запущен без `phoecord run`/`launch`. Это безвредно; запускайте через `phoecord launch`"],
                  ["Клиент падает при запуске с загрузчиком", "Удалите загрузчик (установщик, Uninstall) и сообщите об ошибке с версией клиента"],
                  ["Порт занят другой программой", "Измените `listen` в `config.toml`"],
                ],
              },
            },
            { p: "**Ограничения Phoenix in-process.** Локальный ответ плагина и отбрасывание исходящего кадра gateway пока в этом режиме не применяются, а `READY` остаётся частичным. Если это важно, запускайте с `--transport proxy`. Каждый хук пишет в лог первый вызов, поэтому хук, который встал, но не увидел трафика, легко заметить." },
            { p: "**VPN, zapret, заблокированная сеть.** Укажите `upstream = \"socks5://127.0.0.1:1080\"` в `config.toml` и проверьте `phoecord selftest`." },
            { p: "**macOS.** «Operation not permitted» или «Installation paused»: обновитесь до 0.5.1 или новее. «Повреждено и не может быть открыто»: «Системные настройки», «Конфиденциальность и безопасность», **Всё равно открыть**, либо `xattr -dr com.apple.quarantine /Applications/PhoeCord.app`." },
            { p: "**Антивирус / SmartScreen.** Сверьте хэш с `SHA256SUMS`. Если он совпал, а Defender помещает `version.dll` в карантин, это ложное срабатывание: сообщите нам релиз и название детекта в [Discord](" + DC + ")." },
            { p: "**Обновление отклонено.** Сообщения вроде «does not match its line in SHA256SUMS» или «does not verify with PhoeCord's key» означают, что ничего не установлено. Выполните `phoecord upgrade` ещё раз; если сообщение про подпись повторяется, сразу сообщите приватно." },
            { p: "**Удалить всё вручную** (программы уже нет): уберите сертификат «PhoeCord local CA (this computer only)» средствами ОС и удалите `~/Library/LaunchAgents/dev.phoecord.proxy.plist` (macOS) или `~/.config/systemd/user/phoecord.service` (Linux). Подозреваете утечку? `phoecord untrust`, удалите папку данных, сообщите приватно." },
            { p: "Полный runbook: [RUNBOOK.md](" + DOCS + "/RUNBOOK.md)." },
          ],
        },
      ],
    },
    {
      title: "Разработчикам",
      sections: [
        {
          id: "plugin-dev",
          title: "Пишем плагин",
          blocks: [
            { p: "Сторонний плагин — это один **WASM-компонент**, собранный по `wit/phoecord.wit` (world `plugin`, пакет `phoecord:plugin@1.0.0`), и `plugin.toml` рядом, в `plugins/<id>/` папки данных. Plugin API **заморожен на v1**: плагин, собранный сегодня, продолжит загружаться во всех следующих 1.x." },
            { code: "my_plugin/\n├── plugin.toml        # id, api = 1, events, rights, fetch hosts, [[menu]], [[setting]]\n├── plugin.wasm        # компонент\n└── i18n/              # необязательно\n    ├── en.ftl\n    └── ru.ftl" },
            { ul: [
              "Компонент экспортирует `manifest()`, `wants(kind)` и `on-event(event)`; на каждое событие вы отвечаете `Pass`, `Changed`, `Drop` или `Reply`.",
              "В нём нет сокетов, файлов, часов и WASI. Всё идёт через интерфейс `host`, и каждый вызов требует **права** (right), перечисленного в `plugin.toml`.",
              "Пользователь видит события и права и подтверждает перед установкой. Просите минимум.",
              "Никогда не пишите в лог текст сообщений и то, что набрал пользователь; не логируйте `request_query()`.",
            ] },
            {
              table: {
                head: ["", "Rust", "TypeScript"],
                rows: [
                  ["Инструменты", "`phoecord-plugin-sdk`, `#[phoecord::plugin]`, цель `wasm32-wasip2`", "componentize-js с `jco`, `tsc`, esbuild"],
                  ["Размер", "десятки килобайт", "около 12 МБ (внутри движок JavaScript)"],
                  ["Время загрузки", "заметно меньше секунды", "около 17 с на загрузку в релизной сборке"],
                  ["Шаблон", "`examples/hello-plugin`", "`examples/hello-plugin-ts`"],
                ],
              },
            },
            { p: "Разрабатывайте с `phoecord dev console` (живые логи плагинов, события, вердикты и тайминги; включение, перезагрузка, повтор) и `phoecord dev watch` (пересборка при изменении и горячая перезагрузка). Ставьте сборку командой `phoecord install`. Руководства: [быстрый старт](" + DOCS + "/plugins/quick-start.md), [события](" + DOCS + "/plugins/events.md), [права](" + DOCS + "/plugins/rights.md), [публикация](" + DOCS + "/plugins/publishing.md), [локализация](" + DOCS + "/plugins/localization.md), [темы](" + DOCS + "/plugins/themes.md)." },
          ],
        },
        {
          id: "events",
          title: "События и вердикты",
          blocks: [
            { ul: [
              "События **REST** (запрос и ответ): фильтр `\"METHOD route\"` (с пробелом), например `\"POST /channels/*/messages\"`.",
              "События **Gateway** (входящие) и исходящие кадры, а также `PHOECORD_LOCAL_DELETE` (ваши собственные удаления сообщений, до подтверждения Discord) и `PHOECORD_TIMER`.",
              "**События UI**: `ui:menu_action`, `ui:user_menu_action`, `ui:channel_menu_action`, `ui:server_menu_action` — когда в контекстном меню Phoenix нажат пункт `[[menu]]` плагина.",
              "Сообщения, отправленные или изменённые с файлами (`multipart/form-data`), доходят до плагинов с `payload_json` в качестве тела.",
            ] },
            {
              table: {
                head: ["Вердикт", "Действие"],
                rows: [
                  ["`Pass`", "Оставить как есть"],
                  ["`Changed`", "Заменить тело или данные своими"],
                  ["`Drop`", "Не пересылать"],
                  ["`Reply`", "Ответить локально, не доходя до Discord (например, команда `/phoecord`)"],
                ],
              },
            },
            { p: "Что несёт каждое событие и какие вердикты оно принимает: [events.md](" + DOCS + "/plugins/events.md)." },
          ],
        },
        {
          id: "rights",
          title: "Права и лимиты",
          blocks: [
            { p: "Вызов без своего права возвращает плагину ошибку. Типичные права: читать или менять события, вызывать API Discord от имени пользователя (`rest`), отправлять кадры (`emit_send`), запрашивать внешние разрешённые хосты (`fetch` со списком хостов), таймеры и настройки. Точная таблица с методами `Host` за каждым правом — в [rights.md](" + DOCS + "/plugins/rights.md)." },
            { ul: [
              "Время: мягкий лимит 50 мс со штрафными баллами, жёсткий 1 с на событие. Плагин, который выбил лимит или упал, отключается, а лог называет его.",
              "Память: 32 МиБ.",
              "Внешние запросы идут только на хосты из списка `fetch` плагина; сессия пользователя никогда не прикладывается.",
              "`rest` и `emit_send` действуют от имени пользователя: укажите это в описании, если из-за них аккаунт могут пометить.",
            ] },
          ],
        },
        {
          id: "publishing",
          title: "Публикация",
          blocks: [
            { ul: [
              "Разложите пакет как показано выше, заполните `plugin.toml` и версионируйте по SemVer. Заморозка v1 означает только добавляющие изменения.",
              "Описывайте настройки через `[[setting]]` (виды: bool, number, text, choice, list), чтобы вкладка настроек могла их нарисовать; подписи пунктов меню и настроек — в `i18n/<lang>.ftl`.",
              "Пользователи ставят командой `phoecord install <id>` (каталог) или `phoecord install <https://...zip>`; `phoecord catalog` показывает доступное, `phoecord update` обновляет.",
              "Каталог сообщества: `catalog/plugins.json` в репозитории. При проверке смотрят на запрошенные права.",
              "Лицензия: ваш плагин может быть под любой лицензией; SDK, `wit/` и шаблоны — MIT.",
            ] },
          ],
        },
        {
          id: "porting",
          title: "Порт плагина Vencord",
          blocks: [
            { p: "Чтобы встроить плагин в сам PhoeCord как нативный на Rust (`crates/phoecord-plugins/`), следуйте [PORTING_PLUGINS.md](" + DOCS + "/PORTING_PLUGINS.md). Оба проекта под GPL-3.0-or-later, поэтому код Vencord можно адаптировать с указанием авторства. Каждый плагин сопоставлен в [PLUGINS.md](" + DOCS + "/PLUGINS.md): название, слой (сеть, UI, тема), целевая версия и статус." },
          ],
        },
        {
          id: "build",
          title: "Сборка из исходников",
          blocks: [
            { code: "git clone https://github.com/nanda070/phoecord && cd phoecord\ncargo run                       # Rust 1.88+\nbash scripts/check.sh           # все проверки\n# Windows: powershell -File scripts/check.ps1" },
            { p: "Сайт — проект Astro в `site/`: там `npm ci`, `npm test`, `npm run build`." },
          ],
        },
        {
          id: "contributing",
          title: "Вклад и управление проектом",
          blocks: [
            { ul: [
              "Работайте через ветки и pull request'ы: [CONTRIBUTING.md](" + REPO + "/blob/main/CONTRIBUTING.md). Плагины, тесты, фикстуры, документация и исследования открыты для всех.",
              "Каждый pull request назначается на Nanda070 и проходит его проверку. Изменения в CA, TLS, хранилищах доверия, прокси и загрузчике всегда требуют проверки владельца.",
              "Спорные технические вопросы решает руководитель проекта и записывает как решение в [DECISIONS.md](" + DOCS + "/DECISIONS.md).",
              "Ошибки и идеи: [Issues](" + REPO + "/issues). Уязвимости и утечки: только приватно.",
              "Правила для людей и ИИ-агентов: [AGENTS.md](" + DOCS + "/AGENTS.md).",
            ] },
          ],
        },
        {
          id: "releases",
          title: "Как выходят релизы",
          blocks: [
            { ol: [
              "Владелец повышает `[workspace.package] version` в `Cargo.toml` и добавляет раздел `## [<версия>]` в `docs/CHANGELOG.md`; он становится описанием релиза.",
              "Владелец пушит тег `v<версия>`. Тег с `-` (например, `v0.5.0-rc.1`) — пре-релиз. Участники теги `v*` не пушат.",
              "CI собирает CLI для Linux, Windows и macOS (Intel и Apple Silicon), загрузчик для Windows и установщики (`.exe`, `.dmg`, `.deb`, `.rpm`), пишет один `SHA256SUMS` и оставляет **черновик** релиза.",
              "Владелец запускает `scripts/sign-release.ps1 -Tag v<версия>`: он проверяет каждый файл, подписывает `SHA256SUMS` офлайн-ключом minisign, загружает `SHA256SUMS.minisig` и публикует.",
              "При публикации релиза в Discord сообщества уходит короткое обновление: что изменилось и ссылка на скачивание.",
              "Установленные копии подхватывают подписанный релиз в течение суток.",
            ] },
          ],
        },
        {
          id: "glossary",
          title: "Глоссарий",
          blocks: [
            {
              table: {
                head: ["Термин", "Значение"],
                rows: [
                  ["Phoenix", "Новый нативный клиент Discord для компьютера (Rust, GPUI)"],
                  ["Electron-клиент", "Нынешние Discord Stable, PTB и Canary"],
                  ["CA", "Центр сертификации: здесь собственный, локальный PhoeCord, ограниченный доменами Discord"],
                  ["Прокси", "Локальная программа между вашим клиентом и Discord"],
                  ["Загрузчик", "DLL внутри процесса (`version.dll`) для UI-слоя"],
                  ["Транспорт", "Способ, которым PhoeCord достаёт трафик: через прокси или внутри процесса"],
                  ["Сигнатура", "Байтовый шаблон, находящий функцию в конкретной сборке Phoenix"],
                  ["Gateway", "Постоянное WebSocket-соединение Discord"],
                  ["REST", "HTTP API Discord"],
                  ["Вердикт", "Ответ плагина на событие: Pass, Changed, Drop, Reply"],
                  ["Право (right)", "Разрешение, которое WASM-плагин объявляет в `plugin.toml`"],
                  ["WIT / WASM", "Язык интерфейсов и бинарный формат сторонних плагинов"],
                  ["Фаза 2", "План от Phoenix Canary до v1.0: транспорт внутри процесса, темы, вкладка настроек, полный паритет с Vencord"],
                ],
              },
            },
          ],
        },
      ],
    },
  ],
};
