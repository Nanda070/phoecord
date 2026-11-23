# Phase 1: ledger

Spec: `docs/phase-1/ai/spec.md` · Plan: `docs/phase-1/ai/plan.md`.

Line format: `YYYY-MM-DD HH:MM | <id> | claimed / paused / resumed / blocked / review / complete | <who> | <text>`, UTC, new lines at the bottom. Decisions: a `Ruling:` line, then `python scripts/gen_decisions.py`.

2026-11-22 23:55 | -     | plan-created | Nanda/Claude | Phase 0 pushed: public repo nanda070/phoecord, main @ 6b3a7f5, CI green
2026-11-23 01:00 | 1.0.1 | claimed      | Nanda/Claude | branch docs/phase-1-plan from main @ 6b3a7f5
Ruling: the repository is public on Nanda070's account (owner Nanda, 2026-11-23, "Public" (translated)) — why: open-source MIT project — risk: everything pushed is published at once; secrets rules in AGENTS.md and SECRETS.md
Ruling: the copyright holder is Cheterin Group, next to the retained upstream MIT copyright line (owner Nanda, 2026-11-23, "Copyright in LICENSE" (translated)) — why: project ownership — risk: none
Ruling: remove every mention of the upstream project; keep only the copyright line and MIT text in LICENSE, which the license requires (owner Nanda, 2026-11-23, "remove all mentions of [the upstream project]" (translated)) — why: own brand — risk: the first commit message in public history still names it; rewriting history is an open question
Ruling: docs follow the VALORANT repository structure: docs/README.md index, CODE_MAP, PROJECT_MAP, GOVERNANCE, RUNBOOK, SECRETS, SUPPORT, DECISIONS, CHANGELOG, _template/, incidents/, reports/, plans as README + AGENTS + ai/ (owner Nanda, 2026-11-23, "make the docs like valorant" (translated)) — why: one way of working across projects — risk: none; VALORANT content itself is private and is not copied
Ruling: all documentation is English-only; the app gets Russian localization later, as stage 1.7 (owner Nanda, 2026-11-23, "translate all docs to English, no Russian at all, though the program should support Russian in the future" (translated)) — why: international audience — risk: Russian app strings drift; CI key check in 1.7.5
2026-11-23 02:30 | 1.0.1 | review       | Nanda/Claude | plan, docs structure, English-only docs, gen scripts on docs/phase-1-plan; PR opened
