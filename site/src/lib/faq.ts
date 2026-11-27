// FAQ copy, English and Russian. Facts come from README.md, TERMS.md, SECURITY.md and docs/RUNBOOK.md;
// when those change, update the answers here. Inline formatting: `code`, **bold**, [text](url).
import type { Lang } from "../i18n";

export interface QA { q: string; a: string }
export interface Category { id: string; title: string; items: QA[] }

type Bi<T> = Record<Lang, T>;

const REPO = "https://github.com/Nanda070/phoecord";
const DC = "https://discord.gg/cheterin";

export const FAQ: Bi<Category[]> = {
  en: [
    {
      id: "basics",
      title: "The basics",
      items: [
        {
          q: "What is PhoeCord?",
          a: "PhoeCord is a free, open-source mod for Discord's native desktop client (Project Phoenix, built with Rust and GPUI). It also works with today's Discord. It adds plugins and themes, the way Vencord does for the Electron client. License: GPL-3.0-or-later.",
        },
        {
          q: "What is Discord Phoenix?",
          a: "Phoenix is Discord's new native desktop client: a compiled Rust program instead of a JavaScript app. That is why Vencord cannot patch it, and why PhoeCord works differently from it.",
        },
        {
          q: "How is PhoeCord different from Vencord?",
          a: "Vencord patches JavaScript inside Electron. Phoenix has no JavaScript to patch, so PhoeCord works in two layers: a local **network** layer that rewrites Discord's REST and Gateway traffic, and a small in-process **UI** layer (the loader) for themes, a settings tab and on-screen extras. Plugin names match Vencord's, so your settings import one to one.",
        },
        {
          q: "Does it work with the Discord I use now (Electron)?",
          a: "Yes. The network layer works with the current Discord (Stable, PTB, Canary) and covers 60+ plugins. The on-screen UI layer is being built for Phoenix first ([Phase 2](" + REPO + "/blob/main/docs/phase-2/README.md)).",
        },
        {
          q: "Is it free?",
          a: "Yes, completely. No ads, no accounts, no subscriptions. The code is GPL-3.0-or-later; the plugin SDK and the plugin contract (`wit/`) are MIT.",
        },
        {
          q: "Who makes it?",
          a: "PhoeCord is made by Cheterin Group, led by Nanda, with contributors on GitHub. See the [Credits](/credits) page.",
        },
        {
          q: "Is it official or affiliated with Discord or Vencord?",
          a: "No. PhoeCord is unofficial and not affiliated with, endorsed by or supported by Discord Inc., Vencord or Zed Industries.",
        },
      ],
    },
    {
      id: "safety",
      title: "Safety and rules",
      items: [
        {
          q: "Can I get banned for using it?",
          a: "Modifying the Discord client is against Discord's Terms of Service, as with Vencord and every other client mod. Discord may limit, suspend or ban an account that uses one. You accept that risk when you install PhoeCord; the team cannot restore a banned account. Plugins that do many actions at once (mass delete, mass send, mass join) raise the risk.",
        },
        {
          q: "Does PhoeCord collect my data? Is there telemetry?",
          a: "No telemetry, no analytics, no accounts. Nothing is sent to the developers or to anyone else. The only network contact PhoeCord makes on its own is the daily update check (see below), and you can turn it off.",
        },
        {
          q: "Why does it install a certificate?",
          a: "To read Discord's API and Gateway traffic on your computer, PhoeCord runs a local proxy and shows the client a certificate from its own local certificate authority (CA). That CA is created on your machine on first run, never leaves it, is unique to your install, and is limited by an X.509 name constraint to `discord.com`, `discordapp.com`, `discord.gg` and their subdomains. It cannot impersonate your bank or any other site. Remove it any time with `phoecord untrust`.",
        },
        {
          q: "Can PhoeCord read my password or my token?",
          a: "It never asks for your password. To let plugins call Discord's API the way the client does, the proxy keeps the `Authorization` header of your own client's latest request in memory only: never on disk, never in logs, never given to a plugin (plugins get only the answer). It is dropped after 30 minutes without client traffic or when Discord answers 401.",
        },
        {
          q: "Is my traffic decrypted?",
          a: "Only Discord's API and Gateway traffic, on your computer, by the local proxy. Everything else (CDN, media, voice, other sites) goes through a plain tunnel that PhoeCord does not decrypt. The proxy accepts connections from `127.0.0.1` only.",
        },
        {
          q: "How do I know a download is genuine?",
          a: "Every release has a `SHA256SUMS` file, and `SHA256SUMS` is signed with the owner's offline minisign key. Check the hash of your file against it, and check the signature with `minisign -Vm SHA256SUMS -p update-key.pub` (the public key is `crates/phoecord-installer/update-key.pub` in the repository). Only download from [phoecord.dev](/download) or the [GitHub releases](" + REPO + "/releases).",
        },
        {
          q: "I found a vulnerability or a leak. What do I do?",
          a: "Report it privately, never in a public issue: DM `nandak070` on Discord, as described in [SECURITY.md](" + REPO + "/blob/main/SECURITY.md).",
        },
      ],
    },
    {
      id: "install",
      title: "Installing and updating",
      items: [
        {
          q: "Which systems are supported?",
          a: "Windows 10 and 11 (x64), macOS 13 Ventura or later (Apple Silicon and Intel) and Linux x86_64 (`.deb` for Debian, Ubuntu, Mint; `.rpm` for Fedora, openSUSE). Other Linux distributions can use the command-line archive.",
        },
        {
          q: "Which file do I download?",
          a: "Windows: `PhoeCord_<version>_x64-setup.exe`. Mac with Apple Silicon (M1 and newer): `..._aarch64.dmg`. Intel Mac: `..._x64.dmg`. Debian, Ubuntu, Mint: `..._amd64.deb`. Fedora, openSUSE: `..._x86_64.rpm`. The `phoecord-*` archives are the command-line tool only. The [Download](/download) page links the right file for you.",
        },
        {
          q: "Windows says \"Windows protected your PC\". Is that normal?",
          a: "Yes. Releases have no publisher certificate, so SmartScreen warns. Click **More info**, then **Run anyway**. First check the file against `SHA256SUMS` with `Get-FileHash .\\<file> -Algorithm SHA256`.",
        },
        {
          q: "macOS says the app is blocked or \"damaged\".",
          a: "The app is only ad-hoc signed. Right-click it, choose **Open**, then **Open** again. If that is not enough: System Settings, Privacy and Security, **Open Anyway**. As a last resort run `xattr -dr com.apple.quarantine /Applications/PhoeCord.app` in Terminal.",
        },
        {
          q: "My antivirus flags `version.dll`.",
          a: "`version.dll` is PhoeCord's loader (the UI layer). If the file's hash matches `SHA256SUMS`, it is a false positive: tell us the release and the detection name in [Discord](" + DC + ") so it can be submitted to the vendor. If the hash does not match, do not run it and report it privately.",
        },
        {
          q: "Does it update itself?",
          a: "Yes. Once a day an installed PhoeCord checks GitHub for a new published release, downloads the installer, checks it against `SHA256SUMS` and the owner's signature, and installs it the next time the PhoeCord window or its background proxy starts. It never updates in the middle of a session. Command-line archives and source builds are never updated automatically.",
        },
        {
          q: "How do I stop automatic updates?",
          a: "Turn off \"Install updates automatically\" in the window's Settings, or put `[update] auto = false` in `config.toml`. `phoecord upgrade` still checks when you ask.",
        },
        {
          q: "How do I update by hand?",
          a: "Install the new file over the old one: run the new setup (it keeps your settings), drag the new app into Applications, or install the new `.deb`/`.rpm`. Settings, the CA and plugins live in the data folder and stay.",
        },
        {
          q: "How do I uninstall it?",
          a: "Windows: Settings, Installed apps, remove PhoeCord. macOS and Linux: press **Uninstall** in the PhoeCord window (or run `phoecord uninstall`) first, then move the app to the Bin or run `sudo apt remove phoecord` / `sudo dnf remove phoecord`. It removes the certificate, the autostart entry, the shortcuts and the files PhoeCord added. Your `config.toml` and plugin packages stay until you delete them.",
        },
      ],
    },
    {
      id: "using",
      title: "Using it",
      items: [
        {
          q: "How do I start Discord with PhoeCord?",
          a: "Open Discord from the PhoeCord window or its shortcut (for example \"Discord Phoenix (PhoeCord)\"). From a terminal: `phoecord launch`, or `phoecord launch --channel canary` (also `stable`, `ptb`, `phoenix`). With several clients installed and no `--channel`, `launch` lists them and stops instead of guessing.",
        },
        {
          q: "How do I turn plugins on and off?",
          a: "In the PhoeCord window's Settings, or with `phoecord enable <id>`, `phoecord disable <id>` and `phoecord toggle <id>`. Only `no_track` is on by default; everything else is off until you turn it on. `phoecord plugins` lists them.",
        },
        {
          q: "What do the `/phoecord` commands do?",
          a: "Many plugins with a data side answer in chat through a `/phoecord ...` command (for example `/phoecord members`, `/phoecord serverinfo`, `/phoecord tags`, `/phoecord rpc`). The answer is a local message that only you see.",
        },
        {
          q: "Where are my settings?",
          a: "In `config.toml` in PhoeCord's data folder, plus the Settings in the PhoeCord window. `phoecord paths` prints where the config, the CA and the logs are.",
        },
        {
          q: "Can I move my Vencord settings over?",
          a: "Yes. Plugin names match Vencord's, so `phoecord import-vencord` brings your settings across one to one. Vencord themes convert with `phoecord theme import <theme.css>`; it also lists what does not carry over.",
        },
        {
          q: "Which plugins are available?",
          a: "Every Vencord plugin that makes sense on a native client, plus extra ones, mapped one by one with its status. See the [plugin list](/plugins) (searchable, always built from the repository).",
        },
        {
          q: "Why is a plugin marked Planned or Partial?",
          a: "The network side of many plugins is done, while the on-screen part waits for the UI layer on Phoenix (Phase 2). \"Ready\" works today, \"Partial\" works in part (the plugin's page says what is missing), \"Planned\" is not built yet.",
        },
        {
          q: "Can I use it with a VPN, zapret or behind a proxy?",
          a: "Yes, with one setting. PhoeCord's proxy connects to Discord itself, outside your VPN app's per-app rules. Set `upstream = \"socks5://127.0.0.1:1080\"` (or `http://host:port`) in `config.toml` to send its connections through the same tunnel, then check with `phoecord selftest`. Credentials in the upstream address are not supported. The update check does not use `upstream`.",
        },
        {
          q: "Does it work together with other mods?",
          a: "On Phoenix it can load another mod's DLL next to its own loader: `phoecord launch --channel phoenix --also-inject <DLL>` (repeatable) or `[phoenix] also_inject` in `config.toml`.",
        },
        {
          q: "What languages does it speak?",
          a: "The installer, the window and plugin settings come in English and Russian, and so does this site.",
        },
      ],
    },
    {
      id: "problems",
      title: "Something is broken",
      items: [
        {
          q: "Discord says \"connecting\" forever.",
          a: "Is the proxy running (`phoecord run`)? Is the port free (`listen` in `config.toml`)? If the message is \"address used by another program\", something else holds the port: change `listen`. Start Discord with `phoecord launch` rather than directly.",
        },
        {
          q: "Certificate or TLS errors in the client.",
          a: "The local CA is not trusted. Run `phoecord trust` and restart Discord. On Linux the NSS `certutil` tool must be installed; Flatpak and Snap have their own NSS databases. If Discord restarts in a loop or never finishes \"Starting\", this is the usual cause.",
        },
        {
          q: "It works in the browser but not in the app.",
          a: "The app was started without the proxy. Quit Discord completely and start it through PhoeCord (`phoecord launch` or the shortcut).",
        },
        {
          q: "A plugin does nothing.",
          a: "Gateway plugins start on reconnect: press Ctrl+R in Discord. Look for `config not reloaded` in the log: it means the TOML has an error. Check that the plugin is on in `phoecord plugins`.",
        },
        {
          q: "Plugins broke after a Discord update.",
          a: "Run `phoecord selftest` (it checks REST and Gateway against the real Discord with no login). Network plugins rarely break; if the client's build is unknown, only the UI layer turns off and the log says it is \"waiting for a signature update\". That is expected degradation, not a bug. Update PhoeCord: new signatures arrive with signed updates.",
        },
        {
          q: "The loader disappeared after a Discord update (Windows).",
          a: "Discord moved into a new `app-x.y.z` folder. Start Discord with `phoecord launch` (or let autostart do it): it copies `version.dll` into the newest folder first. If a running client keeps the old DLL locked, quit Discord fully and try again. If the message says the file \"belongs to another program\", another tool put its own `version.dll` there: PhoeCord never overwrites a file it did not write.",
        },
        {
          q: "The client crashes on start with the loader installed.",
          a: "Remove the loader (the installer's Uninstall) and file a bug with the client version: a hook is wrong for that build.",
        },
        {
          q: "macOS: \"Operation not permitted\" or \"Installation paused\".",
          a: "macOS denied a client PhoeCord started as its own child. Fixed since 0.5.1 (the client is started with `open -n -a`): update PhoeCord. On 0.5.0, allow PhoeCord under System Settings, Privacy and Security, App Management / Full Disk Access.",
        },
        {
          q: "An update was refused.",
          a: "Nothing was installed, and that is by design: the download did not match `SHA256SUMS`, or `SHA256SUMS` was not signed with the owner's key for that version. Run `phoecord upgrade` again; if it keeps failing, report it privately, with the release and file name.",
        },
        {
          q: "Where do I find the logs?",
          a: "`phoecord log` shows the recent log; `phoecord paths` shows where the files are. For more detail run with `PHOECORD_LOG=phoecord=debug phoecord run`. On Windows the loader writes `%LOCALAPPDATA%\\PhoeCord\\loader.log`, and `launch.log` sits in the data folder. Never post logs that contain private data without checking them first.",
        },
        {
          q: "How do I report a bug?",
          a: "Open an [issue](" + REPO + "/issues) or ask in [Discord](" + DC + "). Say when it started (UTC), your Discord client and version, your PhoeCord version, what you see and what the log says.",
        },
      ],
    },
    {
      id: "plugins",
      title: "Writing plugins",
      items: [
        {
          q: "Can I write my own plugin?",
          a: "Yes. Third-party plugins are WASM components, written in Rust or TypeScript (any language that can produce a component works). You do not rebuild PhoeCord: drop the package in `plugins/<id>/` or run `phoecord install`. See the [wiki](/wiki#plugin-dev) and the [plugin guide](" + REPO + "/tree/main/docs/plugins).",
        },
        {
          q: "Is the Plugin API stable?",
          a: "Plugin API v1 is frozen: a plugin built today keeps loading in every later 1.x PhoeCord. Additions are made without breaking existing plugins.",
        },
        {
          q: "Are third-party plugins safe?",
          a: "WASM plugins run in a sandbox with no files, network or process access, only through the host API and only with the `rights` their `plugin.toml` lists (none by default). You see what a plugin asks for and approve it before installing. Each event has a time limit and a 32 MiB memory limit. Native plugins have full rights: install them only from sources you trust.",
        },
        {
          q: "How do I port a Vencord plugin?",
          a: "Port it as a built-in Rust plugin, adapting the code with attribution (both projects are GPL-3.0-or-later). The step-by-step guide is [PORTING_PLUGINS.md](" + REPO + "/blob/main/docs/PORTING_PLUGINS.md).",
        },
        {
          q: "How do I contribute?",
          a: "Fork, branch, open a pull request: see [CONTRIBUTING.md](" + REPO + "/blob/main/CONTRIBUTING.md). Plugins, tests, fixtures, docs and research are open to everyone. Every pull request is reviewed and merged by Nanda.",
        },
        {
          q: "How do I get my plugin into the catalog?",
          a: "Package it, fill in `plugin.toml`, and follow the publishing guide in the repository (`docs/plugins/publishing.md`, `catalog/README.md`). The catalog review checks the rights you ask for: ask for the least.",
        },
      ],
    },
  ],
  ru: [
    {
      id: "basics",
      title: "Основы",
      items: [
        {
          q: "Что такое PhoeCord?",
          a: "PhoeCord — бесплатный мод с открытым кодом для нативного клиента Discord (Project Phoenix, написан на Rust и GPUI). Он работает и с нынешним Discord. Добавляет плагины и темы — так же, как Vencord для Electron-клиента. Лицензия: GPL-3.0-or-later.",
        },
        {
          q: "Что такое Discord Phoenix?",
          a: "Phoenix — новый нативный клиент Discord для компьютера: скомпилированная программа на Rust вместо приложения на JavaScript. Поэтому Vencord не может его патчить, и поэтому PhoeCord работает иначе.",
        },
        {
          q: "Чем PhoeCord отличается от Vencord?",
          a: "Vencord патчит JavaScript внутри Electron. В Phoenix JavaScript нет, поэтому PhoeCord работает в два слоя: локальный **сетевой** слой, который переписывает REST- и Gateway-трафик Discord, и небольшой **UI-слой** внутри процесса (загрузчик) для тем, вкладки настроек и элементов на экране. Названия плагинов совпадают с Vencord, поэтому настройки переносятся один в один.",
        },
        {
          q: "Работает ли он с моим нынешним Discord (Electron)?",
          a: "Да. Сетевой слой работает с текущим Discord (Stable, PTB, Canary) и покрывает 60+ плагинов. Экранный UI-слой сначала делается для Phoenix ([Фаза 2](" + REPO + "/blob/main/docs/phase-2/README.md)).",
        },
        {
          q: "Это бесплатно?",
          a: "Да, полностью. Без рекламы, аккаунтов и подписок. Код под GPL-3.0-or-later; SDK плагинов и контракт плагинов (`wit/`) — под MIT.",
        },
        {
          q: "Кто это делает?",
          a: "PhoeCord делает Cheterin Group во главе с Nanda, а также участники на GitHub. См. страницу [Авторы](/ru/credits).",
        },
        {
          q: "Это официальный проект? Связан ли он с Discord или Vencord?",
          a: "Нет. PhoeCord неофициальный и не связан с Discord Inc., Vencord и Zed Industries, не одобрен и не поддерживается ими.",
        },
      ],
    },
    {
      id: "safety",
      title: "Безопасность и правила",
      items: [
        {
          q: "Могут ли меня забанить?",
          a: "Модификация клиента Discord нарушает Условия использования Discord — как Vencord и любой другой мод. Discord может ограничить, заблокировать или забанить аккаунт, который использует мод. Ставя PhoeCord, вы принимаете этот риск; команда не может вернуть забаненный аккаунт. Плагины, которые делают много действий сразу (массовое удаление, рассылка, вступление), повышают риск.",
        },
        {
          q: "PhoeCord собирает мои данные? Есть ли телеметрия?",
          a: "Нет телеметрии, нет аналитики, нет аккаунтов. Ничего не отправляется ни разработчикам, ни кому-либо ещё. Единственное сетевое обращение, которое PhoeCord делает сам, — ежедневная проверка обновлений (см. ниже), и её можно отключить.",
        },
        {
          q: "Зачем он ставит сертификат?",
          a: "Чтобы читать API- и Gateway-трафик Discord на вашем компьютере, PhoeCord запускает локальный прокси и показывает клиенту сертификат от собственного локального центра сертификации (CA). Этот CA создаётся на вашей машине при первом запуске, никуда не уходит, уникален для вашей установки и ограничен ограничением имён X.509 доменами `discord.com`, `discordapp.com`, `discord.gg` и их поддоменами. Он не может подменить ваш банк или любой другой сайт. Убрать его можно в любой момент: `phoecord untrust`.",
        },
        {
          q: "Может ли PhoeCord прочитать мой пароль или токен?",
          a: "Пароль он не спрашивает никогда. Чтобы плагины могли вызывать API Discord так же, как клиент, прокси хранит заголовок `Authorization` последнего запроса вашего клиента только в памяти: не на диске, не в логах и не отдаёт плагинам (плагины получают лишь ответ). Он сбрасывается через 30 минут без трафика клиента или когда Discord отвечает 401.",
        },
        {
          q: "Мой трафик расшифровывается?",
          a: "Только API- и Gateway-трафик Discord, на вашем компьютере, локальным прокси. Всё остальное (CDN, медиа, голос, другие сайты) идёт через обычный туннель, который PhoeCord не расшифровывает. Прокси принимает соединения только с `127.0.0.1`.",
        },
        {
          q: "Как убедиться, что файл настоящий?",
          a: "У каждого релиза есть файл `SHA256SUMS`, и он подписан офлайн-ключом minisign владельца. Сверьте хэш файла с ним и проверьте подпись командой `minisign -Vm SHA256SUMS -p update-key.pub` (публичный ключ — `crates/phoecord-installer/update-key.pub` в репозитории). Скачивайте только с [phoecord.dev](/ru/download) или из [релизов GitHub](" + REPO + "/releases).",
        },
        {
          q: "Нашёл уязвимость или утечку. Что делать?",
          a: "Сообщите приватно, никогда не в публичном issue: напишите в личку `nandak070` в Discord, как описано в [SECURITY.md](" + REPO + "/blob/main/SECURITY.md).",
        },
      ],
    },
    {
      id: "install",
      title: "Установка и обновление",
      items: [
        {
          q: "Какие системы поддерживаются?",
          a: "Windows 10 и 11 (x64), macOS 13 Ventura и новее (Apple Silicon и Intel) и Linux x86_64 (`.deb` для Debian, Ubuntu, Mint; `.rpm` для Fedora, openSUSE). Для других дистрибутивов Linux есть архив с консольной утилитой.",
        },
        {
          q: "Какой файл скачать?",
          a: "Windows: `PhoeCord_<версия>_x64-setup.exe`. Mac с Apple Silicon (M1 и новее): `..._aarch64.dmg`. Intel Mac: `..._x64.dmg`. Debian, Ubuntu, Mint: `..._amd64.deb`. Fedora, openSUSE: `..._x86_64.rpm`. Архивы `phoecord-*` — только консольная утилита. Страница [Скачать](/ru/download) сама ведёт на нужный файл.",
        },
        {
          q: "Windows пишет «Система Windows защитила ваш компьютер». Это нормально?",
          a: "Да. У релизов нет сертификата издателя, поэтому SmartScreen предупреждает. Нажмите **Подробнее**, затем **Выполнить в любом случае**. Сначала сверьте файл с `SHA256SUMS`: `Get-FileHash .\\<файл> -Algorithm SHA256`.",
        },
        {
          q: "macOS блокирует приложение или пишет «повреждено».",
          a: "Приложение подписано только ad-hoc. Нажмите на нём правой кнопкой, выберите **Открыть**, затем ещё раз **Открыть**. Если не помогло: «Системные настройки», «Конфиденциальность и безопасность», **Всё равно открыть**. В крайнем случае выполните в Терминале `xattr -dr com.apple.quarantine /Applications/PhoeCord.app`.",
        },
        {
          q: "Антивирус ругается на `version.dll`.",
          a: "`version.dll` — это загрузчик PhoeCord (UI-слой). Если хэш файла совпадает с `SHA256SUMS`, это ложное срабатывание: сообщите нам релиз и название детекта в [Discord](" + DC + "), чтобы файл отправили вендору. Если хэш не совпадает, не запускайте файл и сообщите приватно.",
        },
        {
          q: "Он обновляется сам?",
          a: "Да. Раз в сутки установленный PhoeCord проверяет на GitHub новый опубликованный релиз, скачивает установщик, сверяет его с `SHA256SUMS` и подписью владельца и ставит при следующем запуске окна PhoeCord или его фонового прокси. В середине сессии он никогда не обновляется. Консольные архивы и сборки из исходников автоматически не обновляются.",
        },
        {
          q: "Как отключить автообновление?",
          a: "Выключите «Устанавливать обновления автоматически» в настройках окна или добавьте `[update] auto = false` в `config.toml`. `phoecord upgrade` по-прежнему проверяет по вашей просьбе.",
        },
        {
          q: "Как обновить вручную?",
          a: "Поставьте новый файл поверх старого: запустите новый установщик (он сохранит настройки), перетащите новое приложение в «Программы» или установите новый `.deb`/`.rpm`. Настройки, CA и плагины лежат в папке данных и остаются.",
        },
        {
          q: "Как удалить?",
          a: "Windows: «Параметры», «Установленные приложения», удалить PhoeCord. macOS и Linux: сначала нажмите **Удалить** в окне PhoeCord (или выполните `phoecord uninstall`), затем перенесите приложение в Корзину или выполните `sudo apt remove phoecord` / `sudo dnf remove phoecord`. Удаляются сертификат, автозапуск, ярлыки и файлы, которые добавил PhoeCord. Ваш `config.toml` и пакеты плагинов остаются, пока вы их не удалите.",
        },
      ],
    },
    {
      id: "using",
      title: "Использование",
      items: [
        {
          q: "Как запустить Discord с PhoeCord?",
          a: "Откройте Discord из окна PhoeCord или через его ярлык (например, «Discord Phoenix (PhoeCord)»). Из терминала: `phoecord launch` или `phoecord launch --channel canary` (также `stable`, `ptb`, `phoenix`). Если установлено несколько клиентов и `--channel` не указан, `launch` покажет список и остановится, а не будет угадывать.",
        },
        {
          q: "Как включать и выключать плагины?",
          a: "В настройках окна PhoeCord или командами `phoecord enable <id>`, `phoecord disable <id>`, `phoecord toggle <id>`. По умолчанию включён только `no_track`, остальное выключено, пока вы не включите. Список — `phoecord plugins`.",
        },
        {
          q: "Что делают команды `/phoecord`?",
          a: "Многие плагины с «данными» отвечают в чате командой `/phoecord ...` (например, `/phoecord members`, `/phoecord serverinfo`, `/phoecord tags`, `/phoecord rpc`). Ответ — локальное сообщение, которое видите только вы.",
        },
        {
          q: "Где мои настройки?",
          a: "В `config.toml` в папке данных PhoeCord и в настройках окна PhoeCord. `phoecord paths` покажет, где лежат конфиг, CA и логи.",
        },
        {
          q: "Можно перенести настройки Vencord?",
          a: "Да. Названия плагинов совпадают с Vencord, поэтому `phoecord import-vencord` переносит настройки один в один. Темы Vencord конвертируются командой `phoecord theme import <theme.css>`; она же перечислит, что не перенеслось.",
        },
        {
          q: "Какие плагины есть?",
          a: "Каждый плагин Vencord, который имеет смысл в нативном клиенте, и дополнительные — сопоставлены по одному, со статусом. Смотрите [список плагинов](/ru/plugins) (с поиском, всегда собирается из репозитория).",
        },
        {
          q: "Почему плагин помечен «В планах» или «Частично»?",
          a: "Сетевая часть многих плагинов готова, а экранная ждёт UI-слоя на Phoenix (Фаза 2). «Готов» работает уже сегодня, «Частично» работает не целиком (на странице плагина сказано, чего не хватает), «В планах» ещё не сделан.",
        },
        {
          q: "Работает ли с VPN, zapret или за прокси?",
          a: "Да, с одной настройкой. Прокси PhoeCord сам подключается к Discord, мимо правил вашего VPN-приложения для отдельных программ. Укажите `upstream = \"socks5://127.0.0.1:1080\"` (или `http://host:port`) в `config.toml`, чтобы его соединения шли через тот же туннель, и проверьте `phoecord selftest`. Логин и пароль в адресе upstream не поддерживаются. Проверка обновлений `upstream` не использует.",
        },
        {
          q: "Работает ли вместе с другими модами?",
          a: "В Phoenix он может загрузить DLL другого мода рядом со своим загрузчиком: `phoecord launch --channel phoenix --also-inject <DLL>` (можно повторять) или `[phoenix] also_inject` в `config.toml`.",
        },
        {
          q: "На каких языках он работает?",
          a: "Установщик, окно и настройки плагинов — на английском и русском. Этот сайт тоже.",
        },
      ],
    },
    {
      id: "problems",
      title: "Что-то сломалось",
      items: [
        {
          q: "Discord вечно «подключается».",
          a: "Запущен ли прокси (`phoecord run`)? Свободен ли порт (`listen` в `config.toml`)? Если сообщение «address used by another program», порт занят другой программой: поменяйте `listen`. Запускайте Discord через `phoecord launch`, а не напрямую.",
        },
        {
          q: "Ошибки сертификата или TLS в клиенте.",
          a: "Локальному CA не доверяют. Выполните `phoecord trust` и перезапустите Discord. В Linux должна быть установлена утилита NSS `certutil`; у Flatpak и Snap свои базы NSS. Если Discord перезапускается по кругу или не заканчивает «Starting», обычно причина в этом.",
        },
        {
          q: "В браузере работает, в приложении нет.",
          a: "Приложение запущено без прокси. Полностью закройте Discord и запустите его через PhoeCord (`phoecord launch` или ярлык).",
        },
        {
          q: "Плагин ничего не делает.",
          a: "Плагины Gateway стартуют при переподключении: нажмите Ctrl+R в Discord. Поищите в логе `config not reloaded`: значит, в TOML ошибка. Убедитесь, что плагин включён в `phoecord plugins`.",
        },
        {
          q: "Плагины сломались после обновления Discord.",
          a: "Выполните `phoecord selftest` (он проверяет REST и Gateway на настоящем Discord без входа в аккаунт). Сетевые плагины ломаются редко; если сборка клиента неизвестна, отключается только UI-слой, а лог пишет «waiting for a signature update». Это ожидаемая деградация, а не баг. Обновите PhoeCord: новые сигнатуры приходят с подписанными обновлениями.",
        },
        {
          q: "Загрузчик пропал после обновления Discord (Windows).",
          a: "Discord переехал в новую папку `app-x.y.z`. Запустите Discord через `phoecord launch` (или дайте сработать автозапуску): он сначала скопирует `version.dll` в новейшую папку. Если запущенный клиент держит старую DLL, полностью закройте Discord и повторите. Если сообщение говорит, что файл «belongs to another program», другая программа положила туда свой `version.dll`: PhoeCord никогда не перезаписывает файл, который не писал сам.",
        },
        {
          q: "Клиент падает при запуске с установленным загрузчиком.",
          a: "Удалите загрузчик (Uninstall в установщике) и сообщите об ошибке с версией клиента: для этой сборки хук неверен.",
        },
        {
          q: "macOS: «Operation not permitted» или «Installation paused».",
          a: "macOS запретила клиенту, который PhoeCord запустил как дочерний процесс. Исправлено с 0.5.1 (клиент запускается через `open -n -a`): обновите PhoeCord. На 0.5.0 разрешите PhoeCord в «Системные настройки», «Конфиденциальность и безопасность», «Управление приложениями» / «Полный доступ к диску».",
        },
        {
          q: "Обновление отклонено.",
          a: "Ничего не установлено, и так задумано: загрузка не совпала с `SHA256SUMS`, либо `SHA256SUMS` не подписан ключом владельца для этой версии. Выполните `phoecord upgrade` ещё раз; если снова отказ, сообщите приватно, указав релиз и имя файла.",
        },
        {
          q: "Где логи?",
          a: "`phoecord log` показывает последние строки лога; `phoecord paths` — где лежат файлы. Подробнее: `PHOECORD_LOG=phoecord=debug phoecord run`. В Windows загрузчик пишет `%LOCALAPPDATA%\\PhoeCord\\loader.log`, а `launch.log` лежит в папке данных. Не публикуйте логи с личными данными, не проверив их.",
        },
        {
          q: "Как сообщить об ошибке?",
          a: "Откройте [issue](" + REPO + "/issues) или спросите в [Discord](" + DC + "). Укажите, когда началось (UTC), ваш клиент Discord и его версию, версию PhoeCord, что видите и что пишет лог.",
        },
      ],
    },
    {
      id: "plugins",
      title: "Свои плагины",
      items: [
        {
          q: "Можно написать свой плагин?",
          a: "Да. Сторонние плагины — это WASM-компоненты на Rust или TypeScript (подойдёт любой язык, который собирает компонент). Пересобирать PhoeCord не нужно: положите пакет в `plugins/<id>/` или выполните `phoecord install`. См. [вики](/ru/wiki#plugin-dev) и [руководство](" + REPO + "/tree/main/docs/plugins).",
        },
        {
          q: "Стабилен ли Plugin API?",
          a: "Plugin API v1 заморожен: плагин, собранный сегодня, продолжит загружаться во всех следующих 1.x. Дополнения делаются без поломки существующих плагинов.",
        },
        {
          q: "Безопасны ли сторонние плагины?",
          a: "WASM-плагины работают в песочнице без доступа к файлам, сети и процессу, только через API хоста и только с теми правами (`rights`), что перечислены в `plugin.toml` (по умолчанию никаких). Перед установкой вы видите, что просит плагин, и подтверждаете. У каждого события есть лимит времени и лимит памяти 32 МиБ. Нативные плагины имеют полные права: ставьте их только из источников, которым доверяете.",
        },
        {
          q: "Как портировать плагин Vencord?",
          a: "Портируйте его как встроенный плагин на Rust, адаптируя код с указанием авторства (оба проекта под GPL-3.0-or-later). Пошаговое руководство — [PORTING_PLUGINS.md](" + REPO + "/blob/main/docs/PORTING_PLUGINS.md).",
        },
        {
          q: "Как принять участие в разработке?",
          a: "Форк, ветка, pull request: см. [CONTRIBUTING.md](" + REPO + "/blob/main/CONTRIBUTING.md). Плагины, тесты, фикстуры, документация и исследования открыты для всех. Каждый pull request проверяет и вливает Nanda.",
        },
        {
          q: "Как добавить плагин в каталог?",
          a: "Упакуйте его, заполните `plugin.toml` и следуйте руководству по публикации в репозитории (`docs/plugins/publishing.md`, `catalog/README.md`). При проверке каталога смотрят на запрошенные права: просите минимум.",
        },
      ],
    },
  ],
};
