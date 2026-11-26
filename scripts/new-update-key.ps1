# Makes the owner's minisign key for signing releases (audit finding A1 of 2.4.6). Run once, by the
# owner, on the owner's own computer. The secret key never goes into the repository, GitHub or CI.
#
# Usage: powershell -File scripts/new-update-key.ps1 [-SecretKey <path>]
#
# Writes the public key to crates/phoecord-installer/update-key.pub (commit it) and the secret key,
# encrypted with the password minisign asks for, to -SecretKey (default
# %USERPROFILE%\.minisign\phoecord-update.key). Back the secret key and its password up offline: a
# lost key means installed copies cannot be updated automatically until they are reinstalled by hand.
# Needs minisign (scoop install minisign, or winget install jedisct1.minisign).
param(
    [string]$SecretKey = (Join-Path $env:USERPROFILE '.minisign\phoecord-update.key')
)
$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')

if (-not (Get-Command minisign -ErrorAction SilentlyContinue)) {
    throw 'minisign not found: scoop install minisign, or winget install jedisct1.minisign'
}
if (Test-Path $SecretKey) {
    throw "$SecretKey already exists; refusing to replace a release key"
}
$pub = 'crates/phoecord-installer/update-key.pub'
if ((Get-Content $pub | Where-Object { $_.Trim() }).Count -gt 1) {
    throw "$pub already holds a key; replacing it cuts every installed copy off automatic updates"
}

New-Item -ItemType Directory -Force (Split-Path $SecretKey) | Out-Null
# -f: overwrite the placeholder public key file; minisign asks for the password twice.
minisign -G -f -p $pub -s $SecretKey
if ($LASTEXITCODE -ne 0) { throw 'minisign -G failed' }

Write-Host ""
Write-Host "public key: $pub (commit it)"
Write-Host "secret key: $SecretKey (back it up offline with its password; never commit or upload it)"
