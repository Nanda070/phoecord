# Secrets and access

Checked against code: `6b3a7f5`, 23.11.2026. Whoever re-checks this document against the code updates this line (rule in `AGENTS.md`).

Which secrets the project has, where they live, who holds them and how to rotate them. **Values never go here.** Living document: a new key, a new access or a new storage place goes here, in the same commit.

> A secret that reached the repository, a chat, a log or a screenshot counts as leaked: rotate it (§3), do not just delete it from the file. Git history, messages and logs stay.

---

## 1. Where secrets live

| Secret | Where | Who holds it | Who uses it |
|:---|:---|:---|:---|
| Local CA key (`ca.key`) of each user | The user's own PhoeCord data folder, owner-only permissions | Each user, never the project | The user's proxy |
| GitHub repository admin | GitHub account `Nanda070` | Nanda | Repository settings, secrets, releases |
| GitHub Actions secrets | Repository settings → Secrets; none yet | Nanda | CI |
| Dependabot | GitHub app on the repository, no secret | — | `dependabot/*` branches |

No Discord tokens are ever needed by the project: tests do not log in, and `selftest` runs without an account.

## 2. Rules

- Never commit `ca.key`, `ca.pem`, Discord tokens, `.env` files or traffic dumps (`.gitignore` covers the common names).
- A user's CA belongs to that user: the project never generates, collects or ships one.
- Releases are built only by the CI release workflow, never on a contributor's machine.

## 3. If a secret leaked

| Secret | What to do |
|:---|:---|
| A user's `ca.key` | `phoecord untrust`, delete the data folder, run again: a new CA is created. Only Discord domains were ever at risk |
| GitHub access | Revoke the token or remove the collaborator, review recent pushes and releases |

Tell Nanda right away in every case.
