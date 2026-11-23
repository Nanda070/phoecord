# What to read, and when

One index page for `docs/`. The working rules are in the root [`AGENTS.md`](../AGENTS.md) (the single source of truth); this page only says where to go.

## Task → document

| What I am doing | What I read |
|:---|:---|
| Starting work | [`AGENTS.md`](../AGENTS.md) (rules), [`CONTRIBUTING.md`](../CONTRIBUTING.md) (git and PR steps), then the plan's `AGENTS.md` if the work belongs to a plan |
| Changing code | [`CODE_MAP.md`](CODE_MAP.md) (which file does what), [`ARCHITECTURE.md`](../ARCHITECTURE.md) (how the layers fit), [`PROJECT_MAP.md`](PROJECT_MAP.md) (what the product does) |
| Which plugins we build | [`PLUGINS.md`](PLUGINS.md): every Vencord plugin and userplugin, its layer and its task |
| Writing a plugin | [`ARCHITECTURE.md`](../ARCHITECTURE.md) "Plugin API v1", the template in [`_template/code/`](_template/code/), [`PROJECT_MAP.md`](PROJECT_MAP.md) (existing plugins) |
| Working on Phase 1 | [`phase-1/README.md`](phase-1/README.md) (for people), [`phase-1/AGENTS.md`](phase-1/AGENTS.md) (for AI agents), [`phase-1/ai/`](phase-1/ai/) (spec, plan, ledger) |
| Shipping a release | [`_template/release.md`](_template/release.md), then the checks in [`_template/release-check.md`](_template/release-check.md) |
| Something broke | [`RUNBOOK.md`](RUNBOOK.md); a breakage report from [`_template/incident.md`](_template/incident.md) goes into [`incidents/`](incidents/); keys and access: [`SECRETS.md`](SECRETS.md) |
| Writing a plan | [`_template/plan/`](_template/plan/) (small plan: one `README.md`; large: also `AGENTS.md`, `CLAUDE.md`, `ai/`) |
| Looking for a past decision | [`DECISIONS.md`](DECISIONS.md) (every `Ruling:` from the ledgers, by plan), [`CHANGELOG.md`](CHANGELOG.md) (what shipped and when) |
| Looking for open tasks | [`TASKS.md`](../TASKS.md) in the root: one table, then by person |
| Who owns what, where to write | [`GOVERNANCE.md`](GOVERNANCE.md), [`SUPPORT.md`](SUPPORT.md) |
| Security | [`SECURITY.md`](../SECURITY.md) |

## Housekeeping

- [`_template/`](_template/): templates for `plan/` (a plan), `incident.md`, `release.md`, `release-check.md`, `ruling.md`, and code skeletons in `code/` (a native plugin with tests, a plugin manifest). Copy them, do not write from scratch.
- [`incidents/`](incidents/): breakage reports, `YYYY-MM-DD-<short>.md`.
- [`reports/`](reports/): one-off reports (benchmarks, audits).
- [`research/`](research/): research notes (GPUI, Phoenix reconnaissance).
- Generated files are never edited by hand:

| File | Built by |
|:---|:---|
| [`TASKS.md`](../TASKS.md) | `python scripts/gen_tasks.py` |
| [`DECISIONS.md`](DECISIONS.md) | `python scripts/gen_decisions.py` |

Checks before merging: `bash scripts/check.sh` (Windows: `powershell -File scripts/check.ps1`). CI also fails if a generated file is stale.
