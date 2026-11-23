# Instructions for AI agents (Claude Code, Cursor): <plan name>

<!-- Only for a large plan. Replace <...>, delete comments. -->

<One or two lines: what the plan builds and who owns it.>

## Step 0: ask who the developer is

At the start of every new session, before reading code or changing anything, ask:

> Who is the developer: **Nanda** or a contributor?

| Developer | Who | Area | Tasks |
|:---|:---|:---|:---|
| **Nanda** | owner: decisions, Plugin API, reviews, merges, releases | <area> | <ids> |
| **Contributor** | anyone, through a PR that Nanda merges | <area> | <ids> |

If unsure, stop and ask the human to check `docs/<plan>/README.md`.

## Step 1: read

1. The root `AGENTS.md` and `CONTRIBUTING.md`.
2. The last 30 lines of `docs/<plan>/ai/ledger.md`.
3. `docs/<plan>/ai/spec.md`, the source of truth.
4. Your task in `docs/<plan>/ai/plan.md`.

## Step 2: work

- Branches: `feat/<short>` from a fresh `origin/main`. One task per PR; Nanda merges.
- Edit only the files of your area. A change outside it: a `blocked` line in the ledger, then tell your human.
- Living docs of the root `AGENTS.md` in the same commit.
- Do not change a decision of `README.md` silently: a `Ruling:` line in the ledger and the edit in the same commit, then `python scripts/gen_decisions.py`. Format: `docs/_template/ruling.md`.

## Ledger format

One line per event, new lines at the bottom, UTC:

```
YYYY-MM-DD HH:MM | <id> | claimed / paused / resumed / blocked / review / complete | <name> | <text>
Ruling: what was decided (owner Nanda, YYYY-MM-DD, "quote") — why: … — risk: …
```
