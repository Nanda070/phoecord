// The credits page: the team, thanks and the other Cheterin Group projects, in English and
// Russian. Project descriptions follow the Cheterin Group credits pages (cheterin.online/credits,
// chetmedia.com/authors) so the three sites say the same thing.

import type { Lang } from "../i18n";

type Text = Record<Lang, string>;

export interface Link {
  label: string;
  href: string;
}

export interface Member {
  name: string;
  role: Text;
  /** What this person works on in PhoeCord. */
  area: Text;
  handle?: string;
  links: Link[];
}

export interface Project {
  name: string;
  text: Text;
  href: string;
}

export interface Group {
  title: Text;
  projects: Project[];
}

const gh = (path: string) => `https://github.com/${path}`;

export const GROUP_LINKS: Link[] = [
  { label: "GitHub", href: "https://github.com/nanda070" },
  { label: "ChetTeam", href: "https://github.com/orgs/ChetTeam" },
];

export const TEAM: Member[] = [
  {
    name: "Nanda",
    role: { en: "Main developer", ru: "Main developer" },
    area: {
      en: "Reverse engineering of Phoenix, the Plugin API, security and releases.",
      ru: "Реверс Phoenix, Plugin API, безопасность и релизы.",
    },
    handle: "nandak070",
    links: [
      { label: "GitHub", href: gh("Nanda070") },
      { label: "Telegram", href: "https://t.me/nanda070" },
    ],
  },
  {
    name: "ken",
    role: { en: "Main developer", ru: "Main developer" },
    area: {
      en: "The proxy, the transport and the in-process hooks inside Phoenix.",
      ru: "Прокси, транспорт и хуки внутри Phoenix.",
    },
    handle: "Kennurken",
    links: [{ label: "GitHub", href: gh("Kennurken") }],
  },
  {
    name: "matvee",
    role: { en: "Main developer", ru: "Main developer" },
    area: {
      en: "Reverse engineering, typed locators and the theme engine.",
      ru: "Реверс, типизированные локаторы и движок тем.",
    },
    handle: "matveee",
    links: [{ label: "GitHub", href: gh("matveee") }],
  },
  {
    name: "buslan",
    role: { en: "Main developer", ru: "Main developer" },
    area: {
      en: "Client discovery, tooling and signature updates.",
      ru: "Поиск клиентов, инструменты и обновления сигнатур.",
    },
    links: [],
  },
  {
    name: "Mark",
    role: { en: "Creative vision", ru: "Идейный вдохновитель" },
    area: {
      en: "Ideas and direction across Cheterin Group.",
      ru: "Идеи и направление для всей Cheterin Group.",
    },
    handle: "schizophrenogenic",
    links: [],
  },
];

/** The line under the team: everyone else who contributed, with the GitHub contributors list. */
export const OTHERS: Text = {
  en: "And everyone else who has contributed to the project",
  ru: "И все остальные, кто внёс вклад в проект",
};
export const CONTRIBUTORS = "https://github.com/Nanda070/phoecord/graphs/contributors";

export const HELPED: Record<Lang, string[]> = {
  en: [
    "coffee (industrial amounts)",
    "no sleep",
    "Ghidra, all 11,300 correlations of it",
    "that one green test",
    "one `git add -A` we don't talk about",
  ],
  ru: [
    "кофе (промышленными объёмами)",
    "недосып",
    "Ghidra и все её 11 300 сопоставлений",
    "тот самый зелёный тест",
    "один `git add -A`, о котором мы не говорим",
  ],
};

export const THANKS: { name: string; href: string; text: Text }[] = [
  {
    name: "Vencord",
    href: "https://vencord.dev",
    text: {
      en: "Plugin names, settings and parts of the plugin code come from Vencord and its plugin authors, with attribution (GPL-3.0).",
      ru: "Названия плагинов, настройки и часть кода плагинов взяты у Vencord и авторов его плагинов, с указанием авторства (GPL-3.0).",
    },
  },
  {
    name: "Zed · GPUI",
    href: "https://github.com/zed-industries/zed",
    text: {
      en: "The open-source UI framework Phoenix is built on; a reference build of it is how we read Phoenix.",
      ru: "Открытый UI-фреймворк, на котором построен Phoenix; по его эталонной сборке мы читаем Phoenix.",
    },
  },
  {
    name: "Wasmtime",
    href: "https://wasmtime.dev",
    text: {
      en: "The Bytecode Alliance runtime that keeps third-party plugins in their sandbox.",
      ru: "Рантайм Bytecode Alliance, который держит сторонние плагины в песочнице.",
    },
  },
  {
    name: "minisign",
    href: "https://jedisct1.github.io/minisign/",
    text: {
      en: "Signs every release, so an installed PhoeCord only takes updates from us.",
      ru: "Подписывает каждый релиз, поэтому установленный PhoeCord принимает обновления только от нас.",
    },
  },
];

export const PROJECTS: Group[] = [
  {
    title: { en: "Vencord plugins", ru: "Vencord-плагины" },
    projects: [
      {
        name: "SoundboardToVoice",
        href: gh("Nanda070/SoundboardToVoice"),
        text: {
          en: "Plays Discord soundboard sounds through your mic — no Nitro or soundboard permission needed.",
          ru: "Звуки саундборда Discord воспроизводятся через микрофон — без Nitro и права «Использовать саундборд».",
        },
      },
      {
        name: "Vencord-FakeDeafenDetector",
        href: gh("Nanda070/Vencord-FakeDeafenDetector"),
        text: {
          en: "Marks members who talk while self-muted or deafened, the sign of the fake-deafen trick.",
          ru: "Помечает участников, которые говорят при включённом self-mute/deafen — признак трюка с фейковым deafen.",
        },
      },
      {
        name: "Vencord-FakeDeafen",
        href: gh("Nanda070/Vencord-FakeDeafen"),
        text: {
          en: "Fake deafen: look deafened while still hearing the channel.",
          ru: "Фейковый deafen — выглядишь в наушниках, но продолжаешь слышать канал.",
        },
      },
      {
        name: "rmcord",
        href: gh("Nanda070/rmcord"),
        text: {
          en: "Bulk-delete your own Discord messages from a floating panel.",
          ru: "Массовое удаление своих сообщений Discord из плавающей панели.",
        },
      },
      {
        name: "SilentDelete",
        href: gh("Nanda070/SilentDelete"),
        text: {
          en: "Quietly delete a message by swapping it for a placeholder first, so message loggers keep the placeholder.",
          ru: "Тихое удаление сообщения: подменяет его заглушкой, чтобы обойти логгеры сообщений.",
        },
      },
      {
        name: "TrustMeBro",
        href: gh("Nanda070/TrustMeBro"),
        text: {
          en: "Gets past Discord's age check introduced for the UK Online Safety Act.",
          ru: "Обход проверки возраста Discord, введённой по британскому Online Safety Act.",
        },
      },
      {
        name: "vc-betterActivities",
        href: gh("Nanda070/vc-betterActivities"),
        text: {
          en: "Activity icons in the member list, and a way to show every activity.",
          ru: "Иконки активностей в списке участников и возможность показать все активности.",
        },
      },
      {
        name: "vc-followUser",
        href: gh("Nanda070/vc-followUser"),
        text: {
          en: "A Follow entry in the user menu: always be in the same voice channel.",
          ru: "Пункт «Следовать» в контекстном меню пользователя — всегда быть в том же голосовом канале.",
        },
      },
      {
        name: "vc-voiceChatUtilities",
        href: gh("Nanda070/vc-voiceChatUtilities"),
        text: {
          en: "Bulk actions on a whole voice channel: move, mute, disconnect and more.",
          ru: "Массовые действия над целым голосовым каналом: переместить, замутить, отключить и другое.",
        },
      },
      {
        name: "vencord-antirickroll",
        href: gh("Nanda070/vencord-antirickroll"),
        text: {
          en: "Warns about likely rickrolls in messages, masked links included (custom rules supported).",
          ru: "Предупреждает о возможных рикроллах в сообщениях, включая замаскированные ссылки (свои правила поддерживаются).",
        },
      },
      {
        name: "discord-quest-auto-completer",
        href: gh("Nanda070/discord-quest-auto-completer"),
        text: {
          en: "A Vencord plugin that completes every Discord quest on its own.",
          ru: "Плагин Vencord, который автоматически выполняет все квесты Discord.",
        },
      },
    ],
  },
  {
    title: { en: "Sites, bots and games", ru: "Сайты, боты и игры" },
    projects: [
      {
        name: "Cheterin",
        href: "https://cheterin.online/",
        text: {
          en: "Cheterin Group's Discord bot and panel — moderation, levels, economy and tools for community servers.",
          ru: "Discord-бот и панель Cheterin Group — модерация, уровни, экономика и инструменты для комьюнити-серверов.",
        },
      },
      {
        name: "ChetMedia",
        href: "https://chetmedia.com/",
        text: {
          en: "Cheterin Group anime site — catalog, releases, and media at chetmedia.com.",
          ru: "Аниме-сайт Cheterin Group — каталог, релизы и медиа на chetmedia.com.",
        },
      },
      {
        name: "Mirror · GTA5RP",
        href: gh("Nanda070/RP-State-Mirror"),
        text: {
          en: "Discord bot and web dashboard for GTA5RP state factions: documents, wanted boards, fines, transfers, and staff audits.",
          ru: "Discord-бот и веб-дашборд для государственных фракций GTA5RP: документы, розыск, штрафы, нашивки, переводы и кадровый аудит.",
        },
      },
      {
        name: "IDUS",
        href: gh("Nanda070/IDUS"),
        text: {
          en: "Local home voice assistant on your hardware — no cloud APIs or subscriptions. Voice + Telegram, memory and reminders; asks before acting outside.",
          ru: "Локальный домашний голосовой ассистент на вашем железе — без облачных API и подписок. Голос + Telegram, память и напоминания; перед внешними действиями спрашивает.",
        },
      },
      {
        name: "Prometheus",
        href: gh("Nanda070/prometheus"),
        text: {
          en: "Self-hosted AI workspace: chat, agents, research, documents, email, notes, calendar, and local models.",
          ru: "Self-hosted AI-пространство: чат, агенты, исследования, документы, почта, заметки, календарь и локальные модели.",
        },
      },
      {
        name: "Chettik",
        href: gh("Nanda070/Chettik"),
        text: {
          en: "Local-first messenger with email OTP, direct chats, groups, channels, and WebSocket delivery.",
          ru: "Local-first мессенджер: вход по email OTP, личные чаты, группы и каналы, доставка по WebSocket.",
        },
      },
      {
        name: "PepegaGo",
        href: gh("Nanda070/PepegaGo"),
        text: {
          en: "A mobile location-based game inspired by Pokémon GO — open source on GitHub.",
          ru: "Мобильная геолокационная игра в духе Pokémon GO — open-source на GitHub.",
        },
      },
      {
        name: "Neptun ELTE",
        href: gh("Nanda070/Neptun-ELTE"),
        text: {
          en: "Flutter mobile app for ELTE students, built on Neptun.",
          ru: "Мобильное приложение Neptun для студентов ELTE на Flutter.",
        },
      },
      {
        name: "GTA5RP Wiki API",
        href: gh("Nanda070/GTA5RP-Wiki-API"),
        text: {
          en: "Unofficial TypeScript client for the GTA5RP Wiki live map and property snapshots.",
          ru: "Неофициальный TypeScript-клиент для живой карты и данных недвижимости GTA5RP Wiki.",
        },
      },
      {
        name: "nanda.com",
        href: "https://nanda070.github.io/nanda.com/",
        text: {
          en: "Nanda's personal site — websites, Discord bots, and community tools; portfolio and selected work.",
          ru: "Личный сайт Nanda: сайты, Discord-боты и сервисы для комьюнити — портфолио и избранные работы.",
        },
      },
    ],
  },
  {
    title: { en: "Minecraft · Astra series", ru: "Minecraft · серия Astra" },
    projects: [
      {
        name: "AstraBuild",
        href: gh("Nanda070/AstraBuild"),
        text: {
          en: "Industrial PaperMC plugin — machines, energy, logistics. Built for Astra SMP; industrial-mod feel without client mods.",
          ru: "Индустриальный PaperMC-плагин — машины, энергия, логистика. Для Astra SMP: ощущение индустриального мода без клиентских модов.",
        },
      },
      {
        name: "AstraOP",
        href: gh("Nanda070/AstraOP"),
        text: {
          en: "Origins-style abilities for Paper servers. Pick an origin (or stay Human). Inspired by Origins (Fabric) by Apace.",
          ru: "Способности в духе Origins для Paper-серверов. Выбери происхождение (или останься человеком). Вдохновлено Origins (Fabric) от Apace.",
        },
      },
      {
        name: "AstraIP",
        href: gh("Nanda070/AstraIP"),
        text: {
          en: "High-performance parkour for Minecraft 1.21+ Paper — islands, sessions, menus, effects, and integrations.",
          ru: "Высокопроизводительный паркур для Minecraft 1.21+ Paper — острова, сессии, меню, эффекты и интеграции.",
        },
      },
      {
        name: "AstraSMP",
        href: gh("Nanda070/AstraSMP"),
        text: {
          en: "Minecraft plugin for Astra SMP — the server core behind the Astra series.",
          ru: "Minecraft-плагин для Astra SMP — ядро сервера, на котором работает серия Astra.",
        },
      },
      {
        name: "Casino Plugin",
        href: gh("Nanda070/Casino-Plugin"),
        text: {
          en: "Casino for Minecraft 1.21.11 — games and betting right on your server.",
          ru: "Казино для Minecraft 1.21.11 — игры и ставки прямо на сервере.",
        },
      },
      {
        name: "Arcana",
        href: gh("Nanda070/Arcana"),
        text: {
          en: "Unofficial Thaumcraft 6 port for Minecraft 1.20.1 (Forge): magic, research, and custom world generation.",
          ru: "Неофициальный порт Thaumcraft 6 на Minecraft 1.20.1 (Forge): магия, исследования и своя генерация мира.",
        },
      },
    ],
  },
];
