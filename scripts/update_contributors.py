"""Rewrite the contributors block in README.md from GitHub's contributors list.

    python scripts/update_contributors.py          # rewrite README.md
    python scripts/update_contributors.py --check  # exit 1 if the block is stale

Needs the GitHub CLI (`gh`) signed in, or GITHUB_TOKEN in the environment. Bots are left out.
The block sits between `<!-- contributors:start -->` and `<!-- contributors:end -->`.
"""
import json
import os
import subprocess
import sys
import urllib.request
from pathlib import Path

REPO = "Nanda070/phoecord"
README = Path(__file__).resolve().parent.parent / "README.md"
START, END = "<!-- contributors:start -->", "<!-- contributors:end -->"


def contributors():
    token = os.environ.get("GITHUB_TOKEN")
    if token:
        req = urllib.request.Request(
            f"https://api.github.com/repos/{REPO}/contributors?per_page=100",
            headers={"Authorization": f"Bearer {token}", "Accept": "application/vnd.github+json"},
        )
        with urllib.request.urlopen(req, timeout=30) as r:
            data = json.load(r)
    else:
        out = subprocess.run(
            ["gh", "api", f"repos/{REPO}/contributors?per_page=100"],
            check=True, capture_output=True, text=True,
        ).stdout
        data = json.loads(out)
    return [c["login"] for c in data if c.get("type") == "User"]


def block(logins):
    cells = "\n".join(
        f'  <a href="https://github.com/{l}" title="{l}">'
        f'<img src="https://github.com/{l}.png?size=96" width="64" height="64" alt="{l}" /></a>'
        for l in logins
    )
    return f"{START}\n<p>\n{cells}\n</p>\n{END}"


def main():
    text = README.read_text(encoding="utf-8")
    if START not in text or END not in text:
        sys.exit(f"README.md has no {START} ... {END} block")
    head, rest = text.split(START, 1)
    _, tail = rest.split(END, 1)
    new = head + block(contributors()) + tail
    if "--check" in sys.argv:
        if new != text:
            print("README.md contributors are stale: run python scripts/update_contributors.py", file=sys.stderr)
            sys.exit(1)
        return
    if new != text:
        README.write_text(new, encoding="utf-8", newline="\n")
        print("updated README.md")
    else:
        print("README.md is up to date")


if __name__ == "__main__":
    main()
