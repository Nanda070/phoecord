// The latest GitHub release, read once at build time. The site rebuilds on every published
// release (.github/workflows/pages.yml), so the buttons always point at current files.
// Any failure (offline, rate limit, a missing asset) falls back to the releases/latest page.

export const REPO = "https://github.com/Nanda070/phoecord";
export const LATEST = `${REPO}/releases/latest`;

export type Target = "windows" | "macosArm" | "macosIntel" | "deb" | "rpm";

export interface Release {
  tag: string | null;
  url: string;
  assets: Record<Target, string>;
}

const PATTERNS: Record<Target, RegExp> = {
  windows: /_x64-setup\.exe$/,
  macosArm: /_aarch64\.dmg$/,
  macosIntel: /_x64\.dmg$/,
  deb: /_amd64\.deb$/,
  rpm: /_x86_64\.rpm$/,
};

function fallback(): Release {
  const assets = Object.fromEntries(Object.keys(PATTERNS).map((k) => [k, LATEST])) as Record<Target, string>;
  return { tag: null, url: LATEST, assets };
}

let cached: Promise<Release> | null = null;

export function getRelease(): Promise<Release> {
  cached ??= load();
  return cached;
}

async function load(): Promise<Release> {
  try {
    const headers: Record<string, string> = { Accept: "application/vnd.github+json" };
    if (process.env.GITHUB_TOKEN) headers.Authorization = `Bearer ${process.env.GITHUB_TOKEN}`;
    const res = await fetch("https://api.github.com/repos/Nanda070/phoecord/releases/latest", {
      headers,
      signal: AbortSignal.timeout(10_000),
    });
    if (!res.ok) return fallback();
    const data = (await res.json()) as { tag_name: string; html_url: string; assets: { name: string; browser_download_url: string }[] };
    const out = fallback();
    out.tag = data.tag_name;
    out.url = data.html_url;
    for (const [target, re] of Object.entries(PATTERNS) as [Target, RegExp][]) {
      const hit = data.assets.find((a) => re.test(a.name));
      if (hit) out.assets[target] = hit.browser_download_url;
    }
    return out;
  } catch {
    return fallback();
  }
}
