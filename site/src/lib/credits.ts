// The credits page: the team and thanks, in English and Russian.

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
    handle: "Buslan07",
    links: [{ label: "GitHub", href: gh("Buslan07") }],
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
