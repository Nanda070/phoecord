# Localization

Checked against code: `24e0362`, 26.11.2026.

A plugin can show text in the user's language: notification titles, answers to slash commands, labels of context menu entries. The strings live in Fluent files next to `plugin.toml`, and `Host::t` looks them up. Back to the [index](README.md).

Sources: `PluginStrings` in [`crates/phoecord-i18n/src/lib.rs`](../../crates/phoecord-i18n/src/lib.rs), `Host::t` in [`crates/phoecord-core/src/plugin.rs`](../../crates/phoecord-core/src/plugin.rs), the `t` call in [`wit/phoecord.wit`](../../wit/phoecord.wit), the loading in [`package.rs`](../../crates/phoecord-plugin-runtime/src/package.rs), and [`ARCHITECTURE.md`](../ARCHITECTURE.md) "Localization".

## Files

```
my_plugin/
├── plugin.toml
├── plugin.wasm
└── i18n/
    ├── en.ftl        the source language and the fallback: always ship it
    └── ru.ftl
```

- The format is [Fluent](https://projectfluent.org/): `key = text`, with `{ $name }` placeholders. One file per language, named by the language code.
- **Languages today: `en` and `ru`.** A file for another language is ignored with a warning in the log (an install also accepts names like `pt-BR`, but nothing would read them). English is the source of truth; Russian is the first translation.
- The files are loaded with the package and **again on every hot reload**; a deleted `.ftl` file disappears with the next reload. `phoecord dev watch` copies `i18n/*.ftl` for you.
- Limits: at most 16 files per package, each at most 256 KiB (a bigger one is skipped and reported).
- A syntax error in a file keeps the messages that did parse, so one bad line does not blank your strings.
- Messages are plain values. Attributes (`.label`) and terms are not read; use one key per string.
- Placeables are not wrapped in Unicode isolation marks, so `{ $name }` inserts exactly the text you pass.

```ftl
# i18n/en.ftl
greeting = Hello, { $name }!
deleted-notice = Messages deleted: { $count }
menu-copy-raw = Copy raw message
```

```ftl
# i18n/ru.ftl
greeting = Привет, { $name }!
deleted-notice = Удалено сообщений: { $count }
menu-copy-raw = Скопировать исходное сообщение
```

Arguments are strings (see below), so Fluent's plural selectors cannot work from a number: word a message so that it needs no plural form (`Messages deleted: { $count }`), or pass the finished phrase.

## `Host::t`

```
t(key: string, args: list<tuple<string, string>>) -> string
```

No right is needed. In Rust it is `phoecord::host::t(key, &[(name, value), …])` with `String` pairs; in TypeScript `t(key, [[name, value], …])`.

```rust
use phoecord::host;

let text = host::t("greeting", &[("name".into(), "Ada".into())]);
let _ = host::notify("My plugin", &text); // needs the `notify` right
```

**Lookup order**, for the calling plugin's own strings only (plugins never read each other's):

1. the user's language: `--lang` on the command line, else `language` in `config.toml`, else the operating system's language, else `en`;
2. your `en.ftl`;
3. the key itself, so a missing string shows as `greeting` rather than as nothing. The first miss per key is logged once (`missing plugin translation key`).

`args` are **string values** for the `{ $name }` placeholders. Pass every placeholder the message names. A host that has no string bundles (a bare test host) answers with the key, so a plugin keeps working there too.

Keep keys stable and ASCII (`kebab-case` is the convention in PhoeCord's own files). Do not build sentences from fragments: translators need whole messages.

## Menu labels

A context menu entry names a **key**, not a text:

```toml
[[menu]]
id = "copy_raw"
label = "menu-copy-raw"
```

PhoeCord shows the entry with `Host::t("menu-copy-raw")` in the user's language, falling back to English and then to the key itself. So every `label` needs a message in `en.ftl` (and `ru.ftl`), and the click arrives as `ui:menu_action` with `entry = "copy_raw"` ([Events](events.md#menu_action)).

## What is not localized

- `name` and `description` in `plugin.toml` are English; there is no translated-manifest table (`[i18n]` in `plugin.toml` is not a field and makes the package fail to load).
- Log lines (`host::log`) stay English: they are for people debugging, not for the user's screen.
- PhoeCord's CI checks that every key of `en` exists in `ru` for its own crates, but it does not look at third-party packages: check yours (every key of `en.ftl` in `ru.ftl`; a missing one falls back to English, not to an error).
