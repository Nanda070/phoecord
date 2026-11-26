# Themes

A PhoeCord theme is a small TOML file with a name and a palette. The Phoenix loader reads it and
paints the client with it (task 2.3.1); while you edit the file, the client follows within two
seconds.

> **Status.** The format, the colour parser, the file watcher and the three built-in themes are
> in. The palette's field names are PhoeCord's own for now: Phoenix's real palette fields and the
> hook that writes them are not found yet (2.3.1), so a theme does not change the client today.
> When they are, the names below move with them, and `phoecord theme import` with them.

## The file

```toml
name = "Midnight"

[colors]
background = "#101014"
"background.secondary" = "rgb(20, 20, 26)"
text = "hsl(0, 0%, 93%)"
"text.muted" = "#9a9aa6cc"     # with alpha
brand = "#8c9eff"
```

- `name`: shown in the themes picker. Without it, the file name is used.
- `[colors]`: field → colour. A field name is lower case: letters, digits, `_`, `.` and `-`; a
  dotted name needs quotes in TOML.
- Colours: `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, `rgb()` / `rgba()`, `hsl()` / `hsla()`.
- A bad entry (unknown shape, a colour that does not parse) is skipped and reported; the rest of
  the theme still loads.

The fields every built-in theme sets:

| Field | What it paints |
|:---|:---|
| `background` | The chat and the main panels |
| `background.secondary` | The channel list and side panels |
| `background.tertiary` | The server list |
| `background.floating` | Menus, popouts, tooltips |
| `text` | Normal text |
| `text.muted` | Secondary text, timestamps |
| `text.link` | Links |
| `brand` | Buttons and accents |
| `online`, `idle`, `dnd`, `offline` | Status dots |

## Built-in themes

In [`themes/`](../../themes/) of the repository:

| File | Theme |
|:---|:---|
| `default.toml` | Discord (default): Discord's dark surfaces |
| `light.toml` | Discord Light |
| `high-contrast.toml` | High Contrast: black surfaces, every text colour at least 7:1 (WCAG AAA) |

## From a Vencord theme

```
phoecord theme import Midnight.theme.css --out midnight.toml
```

A Vencord theme is CSS for the web client. What carries over to a native palette are its colour
variables (`--background-primary: #313338;` on `:root`, `.theme-dark` and so on); `var(--x)`
references inside the file are followed. Everything else (selectors, layout, fonts, images,
`@import`ed files) has no counterpart and is not read.

The command prints, on the error stream:

- **No palette field for:** colour variables that map to nothing in PhoeCord's palette;
- **Skipped:** mapped variables whose value is not a colour PhoeCord reads (`var()` to a variable
  the file does not define, a gradient, a keyword).

Without `--out` the theme is printed. `--name` sets its name (default: the file name).

The variables it knows, by field:

| Field | Discord CSS variables |
|:---|:---|
| `background` | `--background-primary`, `--background-base-lower`, `--bg-base-primary` |
| `background.secondary` | `--background-secondary`, `--background-secondary-alt`, `--background-base-lowest`, `--bg-base-secondary` |
| `background.tertiary` | `--background-tertiary`, `--bg-base-tertiary` |
| `background.floating` | `--background-floating`, `--bg-surface-overlay` |
| `text` | `--text-normal`, `--text-default`, `--text-primary` |
| `text.muted` | `--text-muted`, `--text-secondary` |
| `text.link` | `--text-link` |
| `brand` | `--brand-experiment`, `--brand-500`, `--brand-new-500` |
| `online` | `--status-positive`, `--green-360` |
| `idle` | `--status-warning`, `--yellow-300` |
| `dnd` | `--status-danger`, `--red-400` |
| `offline` | `--status-offline`, `--primary-400` |

When several of them are in one file, the last one wins, as in CSS.
