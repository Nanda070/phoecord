"""Build TASKS.md in the repository root from the "Tasks" tables of every plan in docs/.

Tasks are edited only in their plan's README.md; this file is regenerated from them.

    python scripts/gen_tasks.py          # rewrite TASKS.md
    python scripts/gen_tasks.py --check  # exit 1 if TASKS.md is stale (CI and scripts/check.sh)
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DOCS = ROOT / "docs"
OUT = ROOT / "TASKS.md"
SKIP_DIRS = {"_template", "incidents", "reports", "research"}
COLUMNS = ("task", "what", "who", "status")


def cells(line: str) -> list[str]:
    return [c.strip() for c in line.strip().strip("|").split("|")]


def plan_tasks(readme: Path) -> list[dict[str, str]]:
    lines = readme.read_text(encoding="utf-8").splitlines()
    try:
        start = next(i for i, l in enumerate(lines) if l.strip().lower() == "## tasks")
    except StopIteration:
        return []
    rows: list[dict[str, str]] = []
    header: list[str] | None = None
    for line in lines[start + 1 :]:
        if line.startswith("## "):
            break
        if not line.lstrip().startswith("|"):
            if header is not None and rows:
                break
            continue
        row = cells(line)
        if header is None:
            header = [c.lower() for c in row]
            continue
        if all(set(c) <= set(":- ") for c in row):
            continue
        record = dict(zip(header, row))
        if all(k in record for k in COLUMNS):
            rows.append({k: record[k] for k in COLUMNS})
    return rows


def task_id(cell: str) -> str:
    return re.sub(r"\*\*", "", cell).strip()


def build() -> str:
    plans = sorted(
        p for p in DOCS.glob("*/README.md") if p.parent.name not in SKIP_DIRS
    )
    all_rows: list[tuple[str, dict[str, str]]] = []
    for readme in plans:
        for row in plan_tasks(readme):
            all_rows.append((readme.parent.name, row))

    out = [
        "# Tasks",
        "",
        "Generated from the \"Tasks\" tables in `docs/<plan>/README.md` by `python scripts/gen_tasks.py`; never edit by hand. Change a task in its plan's README, then rerun the script. CI fails if this file is stale.",
        "",
        f"Tasks: {len(all_rows)}, plans with tasks: {len({p for p, _ in all_rows})}.",
        "",
        "## All tasks",
        "",
        "| Plan | Task | What | Who | Status |",
        "|:--|:--|:--|:--|:--|",
    ]
    for plan, r in all_rows:
        out.append(
            f"| [{plan}](docs/{plan}/README.md) | {task_id(r['task'])} | {r['what']} | {r['who']} | {r['status']} |"
        )

    people: dict[str, list[tuple[str, dict[str, str]]]] = {}
    for plan, r in all_rows:
        people.setdefault(r["who"], []).append((plan, r))
    for who in sorted(people, key=lambda w: (w != "Nanda", w.lower())):
        out += ["", f"## {who}", "", "| Plan | Task | What | Status |", "|:--|:--|:--|:--|"]
        for plan, r in people[who]:
            out.append(f"| {plan} | {task_id(r['task'])} | {r['what']} | {r['status']} |")
    return "\n".join(out) + "\n"


def main() -> int:
    text = build()
    if "--check" in sys.argv:
        current = OUT.read_text(encoding="utf-8") if OUT.exists() else ""
        if current != text:
            print("TASKS.md is stale: run python scripts/gen_tasks.py", file=sys.stderr)
            return 1
        return 0
    OUT.write_text(text, encoding="utf-8", newline="\n")
    print(f"wrote {OUT.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
