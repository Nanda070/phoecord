# <Name>: plan for people

<!--
PhoeCord plan template. Copy the folder `docs/_template/plan/` to `docs/<plan>/`,
replace everything in <angle brackets>, delete the hints in comments.

Plan size:
- small: only this README.md (up to three tasks, one developer, one release);
- large: README.md + AGENTS.md + CLAUDE.md + ai/spec.md + ai/plan.md + ai/ledger.md,
  when there are more than three tasks, or two people work on it, or there are several releases.

The sections below are required and come in this order. Do not delete an empty section, write "none".
Dates DD.MM.YYYY. Task statuses: ⏳ not started · 🔄 in progress · ✅ done;
importance: 🔴 required · 🟡 desirable · 🟢 if possible.
-->

<Two or three lines: what we are building and why, who asked and when.>

> This is the plan for people. The spec, the step-by-step tasks and the ledger are in [`ai/`](ai/). If this file and [`ai/spec.md`](ai/spec.md) disagree, the spec wins. <!-- small plan: delete this line -->

## Status

<DD.MM.YYYY: where the code is (branch, `main` sha, release), what is done, what is next. A closed plan: "Plan closed DD.MM.YYYY, release `x.y.z`".>

## What we get

<How a user, a plugin author or a developer will see it. No code.>

## Decisions

<Decisions by the owner: date and quote.>

- <decision> (Nanda DD.MM.YYYY: "<quote>")

**Not doing:** <what was rejected and by whom; "none" if nothing>.

## Open questions

- <question, for whom> <!-- "none" if everything is decided -->

## Who does what

| | Nanda | Contributors |
|:---|:---|:---|
| Area | <area> | <area> |
| Tasks | <ids> | <ids> |

## Tasks

<Large plan: details, files and "done when" in [`ai/plan.md`](ai/plan.md).>

<!-- This table is collected into the root TASKS.md: after editing run `python scripts/gen_tasks.py`. Do not rename the columns: Task, What, Who, Status. -->

| Task | What | Who | Status |
|:--|:--|:--|:--|
| **<id>** 🔴 | <what> | <Nanda / anyone> | ⏳ |

## Risks

- **<risk>.** <what we do if it happens>

## Living docs and publications

<Which documents from the root `AGENTS.md` table change, release notes, README "Plugins" table.>

## How to start

<For a developer: which branch, what to read, which task is first. Large plan: "read [`AGENTS.md`](AGENTS.md)".>

## Where things are

| What | Where |
|:--|:--|
| <module> | `<path>` |
