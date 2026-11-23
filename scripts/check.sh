#!/usr/bin/env bash
# All checks before a merge, in the same order as CI; stops at the first failure.
# Usage: bash scripts/check.sh
set -euo pipefail
cd "$(dirname "$0")/.."

PY="${PYTHON:-}"
if [ -z "$PY" ]; then
  if command -v python3 >/dev/null 2>&1; then PY=python3; else PY=python; fi
fi

step() { printf '\n==> %s\n' "$*"; }

step "cargo fmt --all --check"
cargo fmt --all --check
step "cargo clippy --workspace --all-targets -- -D warnings"
cargo clippy --workspace --all-targets -- -D warnings
step "cargo test --workspace"
cargo test --workspace
step "gen_decisions.py --check"
"$PY" scripts/gen_decisions.py --check
step "check_i18n.py"
"$PY" -I scripts/check_i18n.py

printf '\nall checks passed\n'
