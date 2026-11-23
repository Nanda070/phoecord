<!--
Release record template. Releases are cut by Nanda from `main` only.
The list of changes lives in docs/CHANGELOG.md: on release, move "Unreleased" under the version and date.
Copy the "Record" block into the GitHub release notes. Delete the comments before use.
-->

# Release x.y.z, DD.MM.YYYY

## Record

| Field | Value | Where to get it |
|:---|:---|:---|
| `version` | `x.y.z` | `[workspace.package] version` in `Cargo.toml` |
| `sha` | `<short sha>` | `git log -1 --format=%h` on `main` |
| `prev` | `<previous version and sha>` | the previous GitHub release |
| `ci` | `<link to the green CI run on this sha>` | GitHub Actions |
| `artifacts` | Windows / macOS / Linux binaries, installer, `SHA256SUMS` | release workflow |
| `signed` | yes / no, with which certificate | `docs/SECRETS.md` |
| `api` | `API_VERSION` and whether it changed | `crates/phoecord-core/src/plugin.rs` |
| `languages` | `en`, `ru`, missing keys: none / list | `.ftl` check (1.7) |
| `checked_on` | Discord clients and versions from the release check | `release-check.md` |
| `approved_by` | Nanda | |

## What shipped

<!-- The CHANGELOG lines in human words: what users get, what plugin authors get, what is internal only. -->

## Release check

<!-- Result of the scenarios from release-check.md: ok, or what did not pass with a link to the issue. -->

## If it goes wrong

<!-- How to roll back: mark the release as pre-release, point users to `prev`, what to do with the loader and signatures. -->
