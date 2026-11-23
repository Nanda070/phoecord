# Contributing

We use GitHub Flow: every change is its own branch and Pull Request. The rules (branch prefixes, commits, who merges, checks, living docs) live in [`docs/AGENTS.md`](docs/AGENTS.md), the single source of truth; this page only lists the steps for a human. Which document to read for which task: [`docs/README.md`](docs/README.md).

Branch prefixes: `feat/`, `fix/`, `refactor/`, `chore/`, `docs/` (example: `feat/proxy-gateway`). Commits follow [Conventional Commits](https://www.conventionalcommits.org/); the scope is the crate name without `phoecord-`: `feat(proxy): wire gateway into plugin chain`, `fix(loader): reject ambiguous AOB matches`.

## Task cycle

1. **Branch from a fresh `main`.** Afterwards `git branch` shows the new prefixed branch.

   ```bash
   git checkout main
   git pull origin main
   git checkout -b feat/my-feature
   ```

2. **Develop in atomic commits.** One commit is one finished change that builds and passes tests.

   ```bash
   git add <files>
   git commit -m "feat(plugins): port silent_typing to Plugin API v1"
   ```

3. **Sync with `main` before pushing.**

   ```bash
   git fetch origin main
   git rebase origin/main
   ```

   On conflicts: fix the files, `git add <files>`, then `git rebase --continue`. After the rebase the checks must pass again.

4. **Push and open a Pull Request.**

   ```bash
   git push -u origin feat/my-feature
   ```

   On GitHub: **Compare & pull request** → fill in the template → add the owners under **Reviewers**.

5. **Review and merge are done by the owners.** They read **Files changed**, leave comments and **Approve**, then **Squash and merge**; the branch is deleted.

## Before a Pull Request

CI runs the same checks on Linux, Windows and macOS. Locally, in one command:

```bash
bash scripts/check.sh            # Windows: powershell -File scripts/check.ps1
```

It runs `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`, and checks that `docs/DECISIONS.md` is up to date and that every English string has a Russian translation.

## Code rules

- **Crate boundaries.** `phoecord-core` depends on none of our crates and does no I/O. Plugins never open sockets or files, only through `Host`. The loader (`phoecord-loader`) is not needed by network plugins and must never become their dependency.
- **Plugin API.** A change to `Event`, `Verdict`, `Host` or `Manifest` is its own PR, labelled `api`, with an entry in [`docs/CHANGELOG.md`](docs/CHANGELOG.md). A new variant is a minor change; changing an existing one bumps `API_VERSION`.
- **`unsafe`.** Allowed only in `phoecord-loader`, with a `// SAFETY:` comment above every block.
- **Tests without network.** Tests never call Discord and never need a token. Gateway tests use recorded payloads with personal data removed.
- **Plugins.** Port or write them by [`docs/PORTING_PLUGINS.md`](docs/PORTING_PLUGINS.md); adapted Vencord code is fine with its attribution header. Every plugin has a unique `snake_case` id, tests next to its code, and a row in the plugin table of [`ROADMAP.md`](docs/ROADMAP.md).
- **User-facing text.** The localization layer exists (`phoecord-i18n`, Fluent); moving the CLI, notification and installer strings into it is stages 1.7.2–1.7.3. Until a crate is moved, keep its strings in one place (for example `crates/phoecord-cli/src/text.rs`) so they are easy to extract. English is the source language, Russian is the first translation.
- **License.** PhoeCord is GPL-3.0-or-later ([`LICENSE`](LICENSE), [`NOTICE`](NOTICE)); by contributing you license your work under it. The plugin SDK and templates are MIT. Never remove the notices in [`LICENSES/`](LICENSES/).

## What not to send

- Discord tokens, `ca.key`, `ca.pem`, traffic dumps with personal data, Discord client binaries or disassembled pieces of them.

Problems go into [Issues](https://github.com/Nanda070/phoecord/issues). Leaks of personal data or secrets, and vulnerabilities, go privately, never into an issue: [DM `nandak070` on Discord](https://discord.com/users/1219803151180370021) (user ID `1219803151180370021`), see [`SECURITY.md`](SECURITY.md).
