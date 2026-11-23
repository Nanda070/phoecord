# All checks before a merge, in the same order as CI; stops at the first failure.
# Usage: powershell -File scripts/check.ps1
$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')

$py = $env:PYTHON
if (-not $py) { $py = 'python' }

function Invoke-Step([string]$name, [scriptblock]$body) {
    Write-Host ""
    Write-Host "==> $name"
    & $body
    if ($LASTEXITCODE -ne 0) { Write-Host "FAILED: $name"; exit $LASTEXITCODE }
}

Invoke-Step 'cargo fmt --all --check' { cargo fmt --all --check }
Invoke-Step 'cargo clippy --workspace --all-targets -- -D warnings' { cargo clippy --workspace --all-targets -- -D warnings }
Invoke-Step 'cargo test --workspace' { cargo test --workspace }
Invoke-Step 'gen_decisions.py --check' { & $py scripts/gen_decisions.py --check }
Invoke-Step 'check_i18n.py' { & $py -I scripts/check_i18n.py }

Write-Host ""
Write-Host 'all checks passed'
