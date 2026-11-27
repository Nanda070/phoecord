#!/usr/bin/env python3
"""Post a short update about a published release to the community Discord channel.

Run by .github/workflows/discord-release.yml when a release is published (or by hand with
`workflow_dispatch`). The webhook URL is a secret (DISCORD_RELEASE_WEBHOOK): it is read from the
environment and never written to a file, a log or the repository.

    python scripts/notify_release.py --event "$GITHUB_EVENT_PATH"       # in CI, from a `release` event
    python scripts/notify_release.py --tag v0.7.0 --dry-run              # print the message, send nothing

What it posts: the version, what changed (the release notes' changelog part, trimmed), and download
links for every system plus the website and the full release page.
"""
import argparse
import json
import os
import re
import subprocess
import sys
import urllib.request

REPO = "Nanda070/phoecord"
SITE = "https://phoecord.dev"
COLOR = 0xFF4D1A
MAX_DESC = 1800
MAX_BULLETS = 8

# (label, regex over the asset file name), in the order shown in Discord.
DOWNLOADS = [
    ("Windows", r"_x64-setup\.exe$"),
    ("macOS (Apple Silicon)", r"_aarch64\.dmg$"),
    ("macOS (Intel)", r"_x64\.dmg$"),
    ("Debian / Ubuntu", r"_amd64\.deb$"),
    ("Fedora / openSUSE", r"_x86_64\.rpm$"),
]


def load_release(args):
    if args.event:
        with open(args.event, encoding="utf-8") as f:
            return json.load(f)["release"]
    out = subprocess.run(
        ["gh", "api", f"repos/{REPO}/releases/tags/{args.tag}"],
        check=True, capture_output=True, text=True,
    ).stdout
    return json.loads(out)


def short(item: str, limit: int = 200) -> str:
    """One bullet cut to its first sentence, without the internal task number in front."""
    item = re.sub(r"^\d+\.\d+\.\d+( \([^)]*\))?( / \d+\.\d+\.\d+)?[:.]?\s*", "", item)
    m = re.match(r"(.+?[.!?])(\s|$)", item)
    sentence = m.group(1) if m else item
    if len(sentence) > limit:
        sentence = sentence[:limit].rsplit(" ", 1)[0].rstrip(",;:") + "…"
    return sentence[:1].upper() + sentence[1:]


def changes(body: str) -> str:
    """A short digest of the changelog part of the notes: headings kept, each bullet cut to one sentence."""
    body = (body or "").replace("\r\n", "\n")
    body = re.split(r"\n## Which file do I download\?", body)[0]
    out, bullets = [], 0
    for line in body.splitlines():
        line = line.rstrip()
        m = re.match(r"^#{2,4}\s+(.*)$", line)
        if m:
            out.append(f"\n**{m.group(1).strip()}**")
        elif line.startswith("- "):
            if bullets < MAX_BULLETS:
                out.append("• " + short(line[2:].strip()))
            bullets += 1
        elif line.strip():
            out.append(line)
    if bullets > MAX_BULLETS:
        out.append(f"…and {bullets - MAX_BULLETS} more in the full notes.")
    text = re.sub(r"\n{3,}", "\n\n", "\n".join(out)).strip()
    if len(text) > MAX_DESC:
        cut = text[:MAX_DESC]
        text = cut[: cut.rfind("\n")].rstrip() + "\n…"
    return text or "Fixes and improvements. See the full notes on the release page."


def build_payload(rel) -> dict:
    tag = rel["tag_name"]
    version = tag.lstrip("v")
    page = rel["html_url"]
    assets = {a["name"]: a["browser_download_url"] for a in rel.get("assets", [])}
    links = []
    for label, pat in DOWNLOADS:
        url = next((u for n, u in assets.items() if re.search(pat, n)), None)
        if url:
            links.append(f"[{label}]({url})")
    downloads = " · ".join(links) if links else f"[All files]({page})"
    embed = {
        "title": f"PhoeCord {version} is out",
        "url": page,
        "color": COLOR,
        "description": changes(rel.get("body")),
        "fields": [
            {"name": "Download", "value": downloads, "inline": False},
            {"name": "More", "value": f"[All files and checksums]({page}) · [Website]({SITE}/download) · [Wiki]({SITE}/wiki)", "inline": False},
        ],
        "footer": {"text": "Installed copies update themselves within a day. Files are signed: check SHA256SUMS."},
        "timestamp": rel.get("published_at"),
    }
    # No pings from release notes, whatever they contain.
    return {"username": "PhoeCord Releases", "embeds": [embed], "allowed_mentions": {"parse": []}}


def send(url: str, payload: dict) -> None:
    req = urllib.request.Request(
        url + ("&" if "?" in url else "?") + "wait=true",
        data=json.dumps(payload).encode(),
        headers={"Content-Type": "application/json", "User-Agent": "PhoeCord-release-notifier"},
    )
    with urllib.request.urlopen(req, timeout=30) as r:
        print(f"posted to Discord (HTTP {r.status})")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--event", help="path to a GitHub `release` event payload (GITHUB_EVENT_PATH)")
    ap.add_argument("--tag", help="release tag to announce (uses the gh CLI)")
    ap.add_argument("--dry-run", action="store_true", help="print the message instead of sending it")
    args = ap.parse_args()
    if not args.event and not args.tag:
        ap.error("pass --event or --tag")
    rel = load_release(args)
    if rel.get("draft"):
        print("release is a draft: nothing to announce")
        return 0
    if rel.get("prerelease"):
        print("prerelease: not announced")
        return 0
    payload = build_payload(rel)
    if args.dry_run:
        print(json.dumps(payload, indent=2, ensure_ascii=False))
        return 0
    url = os.environ.get("DISCORD_RELEASE_WEBHOOK", "").strip()
    if not url:
        print("::error::DISCORD_RELEASE_WEBHOOK is not set", file=sys.stderr)
        return 1
    send(url, payload)
    return 0


if __name__ == "__main__":
    sys.exit(main())
