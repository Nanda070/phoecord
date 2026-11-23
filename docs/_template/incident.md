<!--
Breakage report template.
Copy to docs/incidents/YYYY-MM-DD-<short>.md (date = when it was noticed; name in lowercase with hyphens).
Write facts: what was seen, what was done, which commit. No blame and no guessing: mark a guess "assumption".
Delete all <!-- --> comments before committing. Do not reorder the sections.
-->

# <What broke, in one line>

Date: DD.MM.YYYY · Report by: <Nanda | contributor | AI> · Status: <being fixed | fixed | monitoring>

## What broke

<!-- What the user saw: which feature, which client, which message. One or two paragraphs. -->

## When it was noticed

<!-- When it started (if known), when it was noticed, by whom and how: an issue, CI, selftest. The gap between "started" and "noticed" is a finding in itself. -->

## Who was affected

<!-- Which clients and versions, which OSes, how many reports, whether any messages or settings were lost. "Nobody" is an answer too. -->

## Cause

<!-- What actually broke. File and function, if found. What was an assumption, what was verified. -->

## Fix

<!-- What was done and in what order: code change, signature update, release, rollback. For each step: who and when. -->

## How to avoid it next time

<!-- A test, a fixture, a RUNBOOK row, a template, a rule. Each item with an owner; the task goes into a plan's "Tasks" table. -->

## Links

- Commit or PR: <sha | link>
- Issue: <link>
- `docs/RUNBOOK.md` section: <§ or "none">
- Plan ledger line: <plan, date>
