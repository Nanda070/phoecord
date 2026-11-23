#!/usr/bin/env python3
"""Check that every English message has a Russian counterpart with the same variables.

For every crates/*/i18n/en/*.ftl, the matching crates/*/i18n/ru/<same name> must contain
every message (and term) id of the English file, and each one must use exactly the same
set of `{ $variables }`. Ids that exist only in `ru` are reported as warnings.

Usage: python -I scripts/check_i18n.py
Exit code 1 and a list of problems when a check fails.
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ID_RE = re.compile(r"^(-?[A-Za-z][A-Za-z0-9_-]*)[ \t]*=(.*)$")
VAR_RE = re.compile(r"\$([A-Za-z][A-Za-z0-9_-]*)")


def parse(text):
    """Return {id: set(variables)} for the messages and terms of one .ftl file."""
    entries = {}
    current = None
    for line in text.splitlines():
        if line.startswith("#"):
            current = None
            continue
        m = ID_RE.match(line)
        if m:
            current = m.group(1)
            entries[current] = set(VAR_RE.findall(m.group(2)))
        elif current is not None and line[:1] in (" ", "\t"):
            entries[current] |= set(VAR_RE.findall(line))
        elif not line.strip():
            continue
        else:
            current = None
    return entries


def fmt(names):
    return "{ " + ", ".join("$" + n for n in sorted(names)) + " }" if names else "no variables"


def main():
    errors, warnings, checked = [], [], 0
    for en_path in sorted(ROOT.glob("crates/*/i18n/en/*.ftl")):
        ru_path = en_path.parent.parent / "ru" / en_path.name
        rel_en = en_path.relative_to(ROOT).as_posix()
        rel_ru = ru_path.relative_to(ROOT).as_posix()
        checked += 1
        if not ru_path.is_file():
            errors.append(f"{rel_ru}: file is missing (English source: {rel_en})")
            continue
        en = parse(en_path.read_text(encoding="utf-8"))
        ru = parse(ru_path.read_text(encoding="utf-8"))
        for key in en:
            if key not in ru:
                errors.append(f"{rel_ru}: missing message `{key}`")
            elif en[key] != ru[key]:
                errors.append(
                    f"{rel_ru}: `{key}` uses {fmt(ru[key])}, but {rel_en} uses {fmt(en[key])}"
                )
        for key in ru:
            if key not in en:
                warnings.append(f"{rel_ru}: `{key}` has no English source in {rel_en}")

    for w in warnings:
        print(f"warning: {w}")
    if errors:
        print(f"i18n check failed ({len(errors)} problem(s)):")
        for e in errors:
            print(f"  - {e}")
        return 1
    if checked == 0:
        print("i18n check failed: no crates/*/i18n/en/*.ftl files found")
        return 1
    print(f"i18n check passed ({checked} file(s))")
    return 0


if __name__ == "__main__":
    sys.exit(main())
