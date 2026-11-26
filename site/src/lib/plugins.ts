// Reads the plugin tables of docs/PLUGINS.md at build time, so the site's plugin list
// never drifts from the repository's single source of truth.
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

export type Section = "vencord" | "core" | "userplugin";
export type Layer = "network" | "network+ui" | "ui" | "theme" | "replaced" | "na";
export type Status = "done" | "partial" | "planned" | "replaced" | "na";

export interface Plugin {
  name: string;
  description: string;
  layer: Layer;
  /** "v1.0", "1.x" or "—" (not in any release). */
  target: string;
  status: Status;
  section: Section;
}

const SECTIONS: Record<string, Section> = {
  "vencord plugins": "vencord",
  "vencord core modules": "core",
  "owner's userplugins": "userplugin",
};

/** Splits a Markdown table row on unescaped pipes; `\|` stays inside its cell. */
function cells(row: string): string[] {
  const inner = row.trim().replace(/^\|/, "").replace(/\|$/, "");
  return inner.split(/(?<!\\)\|/).map((c) => c.replace(/\\\|/g, "|").trim());
}

function layerOf(text: string): Layer {
  if (text.includes("⛔")) return "na";
  if (text.includes("♻️")) return "replaced";
  if (text.includes("🖌️")) return "theme";
  if (text.includes("🌐") && text.includes("🎨")) return "network+ui";
  if (text.includes("🌐")) return "network";
  return "ui";
}

function statusOf(layer: Layer, target: string, where: string): Status {
  if (layer === "na") return "na";
  if (layer === "replaced") return "replaced";
  const w = where.toLowerCase();
  if (w.startsWith("done")) return "done";
  if (w.startsWith("partial")) return "partial";
  if (target === "—") return "na";
  return "planned";
}

/** Strips Markdown code ticks and links from a description cell. */
function plain(text: string): string {
  return text.replace(/`([^`]*)`/g, "$1").replace(/\[([^\]]*)\]\([^)]*\)/g, "$1");
}

export function parsePluginsMd(md: string): Plugin[] {
  const out: Plugin[] = [];
  let section: Section | null = null;
  for (const line of md.split(/\r?\n/)) {
    const heading = /^##\s+(.+)$/.exec(line);
    if (heading) {
      section = SECTIONS[heading[1].trim().toLowerCase()] ?? null;
      continue;
    }
    if (!section || !line.startsWith("|")) continue;
    const c = cells(line);
    if (c.length < 5 || /^:?-+/.test(c[0]) || c[0] === "Plugin" || c[0] === "Module") continue;
    const layer = layerOf(c[2]);
    out.push({
      name: c[0],
      description: plain(c[1]),
      layer,
      target: c[3],
      status: statusOf(layer, c[3], c[4]),
      section,
    });
  }
  return out;
}

// Resolved from the working directory (site/), which holds for `astro build` and `npm test`;
// import.meta.url would point into the bundle at build time.
const PLUGINS_MD = resolve(process.cwd(), "../docs/PLUGINS.md");

export function loadPlugins(): Plugin[] {
  return parsePluginsMd(readFileSync(PLUGINS_MD, "utf8"));
}
