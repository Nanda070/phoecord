import { test } from "node:test";
import assert from "node:assert/strict";
import { parsePluginsMd, loadPlugins } from "./plugins.ts";

const md = `# Plugins

## Vencord plugins

| Plugin | What it does | Layer | v1.0 | Where |
|:---|:---|:---|:---|:---|
| ClearURLs | Strip tracking parameters | 🌐 Network | v1.0 | done |
| ConsoleJanitor | Silence console noise | ⛔ N/A | — | web console only |
| FakeDeafen | Look deafened | 🌐 Network | v1.0 | done: data — \`/phoecord fakedeafen [on\|off]\` |
| BetterFolders | Server folders | 🎨 UI | v1.0 | 2 |
| Detector | Flag members | 🌐 + 🎨 | — | partial: data |

## Vencord core modules

| Module | What it does | Layer | v1.0 | PhoeCord |
|:---|:---|:---|:---|:---|
| Settings | Vencord settings UI | ♻️ Replaced | — | settings tab |

## Owner's userplugins

| Plugin | What it does | Layer | v1.0 | Where |
|:---|:---|:---|:---|:---|
| rmcord | Bulk-delete | 🌐 + 🎨 | v1.0 | done: data |
`;

test("parses rows of every section", () => {
  const p = parsePluginsMd(md);
  assert.equal(p.length, 7);
  assert.deepEqual(p.map((x) => x.section), ["vencord", "vencord", "vencord", "vencord", "vencord", "core", "userplugin"]);
});

test("maps status", () => {
  const by = Object.fromEntries(parsePluginsMd(md).map((x) => [x.name, x]));
  assert.equal(by.ClearURLs.status, "done");
  assert.equal(by.ConsoleJanitor.status, "na");
  assert.equal(by.BetterFolders.status, "planned");
  assert.equal(by.Detector.status, "partial");
  assert.equal(by.Settings.status, "replaced");
  assert.equal(by.ClearURLs.layer, "network");
  assert.equal(by.Detector.layer, "network+ui");
});

test("escaped pipes stay inside a cell", () => {
  const fd = parsePluginsMd(md).find((x) => x.name === "FakeDeafen")!;
  assert.equal(fd.status, "done");
  assert.equal(fd.target, "v1.0");
});

test("the real docs/PLUGINS.md has plugins in every section", () => {
  const p = loadPlugins();
  assert.ok(p.filter((x) => x.section === "vencord").length > 100);
  assert.ok(p.some((x) => x.section === "userplugin"));
  assert.ok(p.some((x) => x.status === "done"));
});
