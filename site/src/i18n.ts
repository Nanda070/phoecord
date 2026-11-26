// Site copy in English and Russian. Plugin names and descriptions come from docs/PLUGINS.md
// and stay English on both versions.

export type Lang = "en" | "ru";

export const DISCORD = "https://discord.gg/cheterin";

const en = {
  htmlLang: "en",
  prefix: "",
  title: "PhoeCord: plugins for Discord's native client",
  description:
    "PhoeCord brings Vencord-style plugins to Discord's native Phoenix client and to today's Discord. Free, open source, GPL-3.0.",
  nav: { home: "Home", plugins: "Plugins", download: "Download", credits: "Credits", source: "Source", discord: "Discord" },
  switchLang: "Русский",
  hero: {
    tag: "For Discord's new native client",
    title1: "Your Discord,",
    title2: "rewired.",
    lead: "PhoeCord adds plugins and themes to Discord Phoenix and to the Discord you use today. No telemetry, no account, nothing patched that an update can brick.",
    download: "Download",
    plugins: "Browse plugins",
    version: "Latest release",
  },
  stats: { ready: "plugins working today", planned: "more on the way", systems: "Windows · macOS · Linux" },
  features: {
    title: "Built to survive updates",
    items: [
      { icon: "🌐", title: "Network first", text: "Most plugins run in a local proxy that rewrites Discord's traffic. A client update cannot break them." },
      { icon: "🎨", title: "UI when it can", text: "A small in-process loader adds themes, a settings tab and on-screen extras. If an update moves things, only the UI turns off." },
      { icon: "🧩", title: "Sandboxed plugins", text: "Third-party plugins are WASM packages with declared rights. Install them without rebuilding, remove them with one command." },
      { icon: "🔒", title: "Yours only", text: "The certificate is generated on your machine, limited to Discord's domains and never leaves it. No telemetry, ever." },
      { icon: "🔄", title: "Updates itself", text: "Installed copies pick up signed releases on their own and verify every file before installing." },
      { icon: "🗂️", title: "Bring your setup", text: "Plugin names match Vencord's, so your settings import one to one, and Vencord themes convert with one command." },
    ],
  },
  highlights: { title: "Popular plugins", all: "See all plugins" },
  community: {
    title: "Join the community",
    text: "Get help, share themes and plugins, follow development and vote on what comes next.",
    button: "Join our Discord",
  },
  footer: {
    license: "Free and open source under GPL-3.0-or-later.",
    disclaimer: "Not affiliated with Discord Inc. or Vencord. Client mods are against Discord's Terms of Service; use at your own risk.",
  },
  pluginsPage: {
    title: "Plugins",
    lead: "Every Vencord plugin that makes sense on a native client, mapped one by one. The list is built from the repository, so it is always current.",
    search: "Search plugins…",
    all: "All",
    empty: "No plugins match your search.",
    sections: { vencord: "Vencord plugins", userplugin: "Extra plugins", core: "Core modules" },
    status: { done: "Ready", partial: "Partial", planned: "Planned", replaced: "Built in", na: "Not applicable" },
    layer: { network: "Network", "network+ui": "Network + UI", ui: "UI", theme: "Theme", replaced: "Built in", na: "—" },
    community: "Community plugins",
    communityEmpty: "No community plugins in the catalog yet. Write one: the guide is in the repository.",
    writeOne: "Plugin author guide",
  },
  downloadPage: {
    title: "Download",
    lead: "Pick your system. The installer sets everything up and keeps PhoeCord up to date on its own.",
    release: "Release",
    allFiles: "All files and checksums",
    os: {
      windows: { name: "Windows", note: "Windows 10 and 11, x64", button: "Download installer (.exe)" },
      macos: { name: "macOS", note: "Apple Silicon and Intel", arm: "Apple Silicon (.dmg)", intel: "Intel (.dmg)" },
      linux: { name: "Linux", note: "x86_64", deb: "Debian / Ubuntu (.deb)", rpm: "Fedora / openSUSE (.rpm)" },
    },
    stepsTitle: "After installing",
    steps: [
      "Open PhoeCord from the Start menu, Launchpad or your app menu.",
      "Pick the Discord you use and press Install PhoeCord. Your system may ask you to confirm the local certificate.",
      "Open Discord from PhoeCord (or its new shortcut) and turn plugins on in Settings.",
    ],
    cliTitle: "Prefer the terminal?",
    cliText: "The release also has a plain CLI archive for every system. Or build from source:",
    uninstallTitle: "Removing it",
    uninstallText: "Uninstall from Installed apps (Windows), or run phoecord uninstall. Both remove the certificate, the shortcuts and every file PhoeCord added.",
    verify: "Every release is signed: SHA256SUMS comes with a minisign signature.",
  },
  creditsPage: {
    group: "Cheterin Group",
    title: "Credits",
    lead: "PhoeCord is part of Cheterin Group — a small development crew behind bots, Minecraft plugins, and open-source tools.",
    team: "Team",
    helped: "Also helped",
    thanks: "Thanks",
    thanksLead: "PhoeCord stands on other people's open-source work.",
    contact: "Contact",
    contactLead: "Questions about PhoeCord, plugins or the project — write to Nanda.",
    security: "A vulnerability or a leak? Report it privately, as SECURITY.md says.",
    projects: "Other Cheterin Group projects",
    projectsLead: "A little more from the same workshop.",
    open: "Open",
  },
};

const ru: typeof en = {
  htmlLang: "ru",
  prefix: "/ru",
  title: "PhoeCord: плагины для нативного клиента Discord",
  description:
    "PhoeCord добавляет плагины в стиле Vencord в нативный клиент Discord Phoenix и в нынешний Discord. Бесплатно, открытый код, GPL-3.0.",
  nav: { home: "Главная", plugins: "Плагины", download: "Скачать", credits: "Авторы", source: "Код", discord: "Discord" },
  switchLang: "English",
  hero: {
    tag: "Для нового нативного клиента Discord",
    title1: "Ваш Discord,",
    title2: "по-вашему.",
    lead: "PhoeCord добавляет плагины и темы в Discord Phoenix и в Discord, которым вы пользуетесь сейчас. Без телеметрии, без аккаунта и без патчей, которые сломает обновление.",
    download: "Скачать",
    plugins: "Все плагины",
    version: "Последний релиз",
  },
  stats: { ready: "плагинов работают уже сейчас", planned: "ещё в работе", systems: "Windows · macOS · Linux" },
  features: {
    title: "Переживает обновления",
    items: [
      { icon: "🌐", title: "Сначала сеть", text: "Большинство плагинов работают в локальном прокси, который переписывает трафик Discord. Обновление клиента их не ломает." },
      { icon: "🎨", title: "Интерфейс, где можно", text: "Небольшой загрузчик внутри клиента добавляет темы, вкладку настроек и элементы на экране. Если обновление что-то сдвинет, отключится только интерфейс." },
      { icon: "🧩", title: "Плагины в песочнице", text: "Сторонние плагины — это WASM-пакеты с заявленными правами. Ставятся без пересборки, удаляются одной командой." },
      { icon: "🔒", title: "Только ваше", text: "Сертификат создаётся на вашем компьютере, работает только для доменов Discord и никуда не уходит. Никакой телеметрии." },
      { icon: "🔄", title: "Обновляется сам", text: "Установленная копия сама получает подписанные релизы и проверяет каждый файл перед установкой." },
      { icon: "🗂️", title: "Переезд без потерь", text: "Названия плагинов совпадают с Vencord, поэтому настройки переносятся один в один, а темы Vencord конвертируются одной командой." },
    ],
  },
  highlights: { title: "Популярные плагины", all: "Все плагины" },
  community: {
    title: "Присоединяйтесь к сообществу",
    text: "Помощь, темы и плагины от других пользователей, новости разработки и голосования за то, что делать дальше.",
    button: "Зайти в наш Discord",
  },
  footer: {
    license: "Бесплатно и с открытым кодом, GPL-3.0-or-later.",
    disclaimer: "Не связан с Discord Inc. и Vencord. Моды клиента нарушают Условия использования Discord; используйте на свой риск.",
  },
  pluginsPage: {
    title: "Плагины",
    lead: "Каждый плагин Vencord, который имеет смысл в нативном клиенте, сопоставлен по одному. Список собирается из репозитория, поэтому всегда актуален.",
    search: "Поиск плагинов…",
    all: "Все",
    empty: "Ничего не найдено.",
    sections: { vencord: "Плагины Vencord", userplugin: "Дополнительные плагины", core: "Модули ядра" },
    status: { done: "Готов", partial: "Частично", planned: "В планах", replaced: "Встроено", na: "Неприменимо" },
    layer: { network: "Сеть", "network+ui": "Сеть + UI", ui: "UI", theme: "Тема", replaced: "Встроено", na: "—" },
    community: "Плагины сообщества",
    communityEmpty: "В каталоге пока нет плагинов сообщества. Напишите свой: руководство есть в репозитории.",
    writeOne: "Руководство для авторов",
  },
  downloadPage: {
    title: "Скачать",
    lead: "Выберите систему. Установщик всё настроит и дальше сам будет обновлять PhoeCord.",
    release: "Релиз",
    allFiles: "Все файлы и контрольные суммы",
    os: {
      windows: { name: "Windows", note: "Windows 10 и 11, x64", button: "Скачать установщик (.exe)" },
      macos: { name: "macOS", note: "Apple Silicon и Intel", arm: "Apple Silicon (.dmg)", intel: "Intel (.dmg)" },
      linux: { name: "Linux", note: "x86_64", deb: "Debian / Ubuntu (.deb)", rpm: "Fedora / openSUSE (.rpm)" },
    },
    stepsTitle: "После установки",
    steps: [
      "Откройте PhoeCord из меню «Пуск», Launchpad или меню приложений.",
      "Выберите свой Discord и нажмите «Установить PhoeCord». Система может попросить подтвердить локальный сертификат.",
      "Откройте Discord из PhoeCord (или через новый ярлык) и включите плагины в настройках.",
    ],
    cliTitle: "Больше нравится терминал?",
    cliText: "В релизе есть и обычный архив с CLI для каждой системы. Или соберите из исходников:",
    uninstallTitle: "Удаление",
    uninstallText: "Удалите через «Установленные приложения» (Windows) или выполните phoecord uninstall. Оба способа убирают сертификат, ярлыки и все файлы, которые добавил PhoeCord.",
    verify: "Каждый релиз подписан: к SHA256SUMS прилагается подпись minisign.",
  },
  creditsPage: {
    group: "Cheterin Group",
    title: "Авторы",
    lead: "PhoeCord — часть группы разработки Cheterin Group. Мы делаем ботов, Minecraft-плагины и open-source инструменты.",
    team: "Команда",
    helped: "Также помогали",
    thanks: "Спасибо",
    thanksLead: "PhoeCord стоит на чужой открытой работе.",
    contact: "Связь",
    contactLead: "По вопросам PhoeCord, плагинов и проекта — пишите Nanda.",
    security: "Нашли уязвимость или утечку? Сообщите приватно, как описано в SECURITY.md.",
    projects: "Другие проекты Cheterin Group",
    projectsLead: "Ещё немного из той же мастерской.",
    open: "Открыть",
  },
};

export function t(lang: Lang) {
  return lang === "ru" ? ru : en;
}
