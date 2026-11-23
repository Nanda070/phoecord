# <Name>: ledger

Spec: `docs/<plan>/ai/spec.md` · Plan: `docs/<plan>/ai/plan.md`.

Line format: `YYYY-MM-DD HH:MM | <id> | claimed / paused / resumed / blocked / review / complete | <who> | <text>`, UTC, new lines at the bottom. Decisions: a `Ruling:` line, then `python scripts/gen_decisions.py`.

YYYY-MM-DD HH:MM | - | plan-created | <who> | docs/<plan>/ from main @ <sha>
