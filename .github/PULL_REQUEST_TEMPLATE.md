## What

<!-- Briefly: what changed and why. PR title as type(scope): description, e.g. feat(proxy): wire gateway into plugin chain. If it closes a plan task, name it: docs/phase-1, task 1.1.4. -->

## How to test

<!-- Steps for the reviewer: commands, Discord client (Stable / Canary / Phoenix), what should happen. -->

## Checks

<!-- The output summary of scripts/check.sh, or a link to the green CI run. -->

## Checklist

- [ ] Branch prefixed `feat/` `fix/` `refactor/` `chore/` `docs/`, commits in Conventional Commits format
- [ ] Rebased on a fresh `main` (`git rebase origin/main`)
- [ ] `scripts/check.sh` (or `check.ps1`) passes: fmt, clippy `-D warnings`, tests, `gen_decisions.py --check`, `check_i18n.py`
- [ ] Tests do not touch the network and need no Discord token
- [ ] Plugin API change (`Event`, `Verdict`, `Host`, `Manifest`): labelled `api`, `API_VERSION` bumped if needed
- [ ] New `unsafe` only in `phoecord-loader`, with a `// SAFETY:` comment
- [ ] Living docs from the `AGENTS.md` table updated in this PR: `docs/CODE_MAP.md`, `docs/CHANGELOG.md` ("Unreleased"), `docs/PLUGINS.md` row, task status in the plan's README
- [ ] New user-facing strings are in English and ready for translation (Russian `.ftl` once 1.7 lands)
- [ ] Touches the CA, TLS, trust stores or the loader: owners requested as reviewers (`SECURITY.md`)
- [ ] No tokens, `ca.key`, `ca.pem`, traffic dumps or client binaries

<!-- Review and merging into main are done by the owners. -->
