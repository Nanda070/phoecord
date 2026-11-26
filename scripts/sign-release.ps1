# Signs a release and publishes it (audit finding A1 of 2.4.6). Run by the owner after release.yml
# has left the release as a draft.
#
# Usage: powershell -File scripts/sign-release.ps1 -Tag v1.0.0 [-SecretKey <path>]
#
# 1. Downloads every file of the draft release and checks each against its SHA256SUMS: every file is
#    listed, every listed file is there, every hash matches.
# 2. Signs SHA256SUMS with the owner's key and the trusted comment "phoecord <version>", which
#    installed copies require (crates/phoecord-installer/src/update.rs, `trusted_comment`).
# 3. Verifies the signature with the public key committed in this tree, uploads SHA256SUMS.minisig
#    and publishes the release. Installed copies only see published releases, and refuse one
#    without a valid SHA256SUMS.minisig.
# Needs gh (signed in) and minisign. minisign asks for the key's password.
param(
    [Parameter(Mandatory = $true)][string]$Tag,
    [string]$SecretKey = (Join-Path $env:USERPROFILE '.minisign\phoecord-update.key')
)
$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')

if (-not (Get-Command gh -ErrorAction SilentlyContinue)) { throw 'gh not found' }
# minisign from PATH, or the x64 build winget unpacks (its alias is not always on PATH).
function Find-Minisign {
    $c = Get-Command minisign -ErrorAction SilentlyContinue
    if ($c) { return $c.Source }
    $w = Join-Path $env:LOCALAPPDATA 'Microsoft\WinGet\Packages\jedisct1.minisign_Microsoft.Winget.Source_8wekyb3d8bbwe\minisign-win64\x86_64\minisign.exe'
    if (Test-Path $w) { return $w }
    throw 'minisign not found: winget install jedisct1.minisign'
}
$minisign = Find-Minisign
if ($Tag -notmatch '^v(\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?)$') { throw "not a release tag: $Tag" }
$version = $Matches[1]
$pub = 'crates/phoecord-installer/update-key.pub'
if ((Get-Content $pub | Where-Object { $_.Trim() }).Count -lt 2) {
    throw "$pub holds no key yet: run scripts/new-update-key.ps1 first"
}
if (-not (Test-Path $SecretKey)) { throw "secret key not found: $SecretKey" }

$info = gh release view $Tag --json 'isDraft,assets' | ConvertFrom-Json
if ($LASTEXITCODE -ne 0) { throw "no release $Tag" }
if (-not $info.isDraft) { throw "$Tag is already published; signing must happen before it is" }
$names = @($info.assets | ForEach-Object { $_.name })
if ($names -contains 'SHA256SUMS.minisig') { throw "$Tag already has SHA256SUMS.minisig" }
if ($names -notcontains 'SHA256SUMS') { throw "$Tag has no SHA256SUMS" }

$dir = Join-Path ([System.IO.Path]::GetTempPath()) "phoecord-sign-$Tag"
if (Test-Path $dir) { Remove-Item -Recurse -Force $dir }
New-Item -ItemType Directory $dir | Out-Null
Write-Host "downloading $($names.Count) files of $Tag to $dir"
gh release download $Tag --dir $dir
if ($LASTEXITCODE -ne 0) { throw 'download failed' }

$sums = Join-Path $dir 'SHA256SUMS'
$listed = @{}
foreach ($line in Get-Content $sums) {
    if (-not $line.Trim()) { continue }
    if ($line -notmatch '^([0-9a-fA-F]{64}) [ *]?(.+)$') { throw "bad SHA256SUMS line: $line" }
    $listed[$Matches[2].Trim()] = $Matches[1].ToLower()
}
$bad = @()
foreach ($n in $names) {
    if ($n -eq 'SHA256SUMS') { continue }
    if (-not $listed.ContainsKey($n)) { $bad += "$n is not listed"; continue }
    $h = (Get-FileHash (Join-Path $dir $n) -Algorithm SHA256).Hash.ToLower()
    if ($h -ne $listed[$n]) { $bad += "$n does not match" }
}
foreach ($n in $listed.Keys) {
    if ($names -notcontains $n) { $bad += "$n is listed but not in the release" }
}
if ($bad) { throw ("refusing to sign:`n  " + ($bad -join "`n  ")) }
Write-Host "all $($listed.Count) files match SHA256SUMS"

$sig = "$sums.minisig"
& $minisign -S -s $SecretKey -m $sums -x $sig -t "phoecord $version"
if ($LASTEXITCODE -ne 0) { throw 'minisign -S failed' }
& $minisign -V -p $pub -m $sums -x $sig
if ($LASTEXITCODE -ne 0) { throw "the signature does not verify with $pub (wrong secret key?)" }

gh release upload $Tag $sig
if ($LASTEXITCODE -ne 0) { throw 'upload failed' }
gh release edit $Tag --draft=false
if ($LASTEXITCODE -ne 0) { throw 'publishing failed; the signature is uploaded, publish the draft by hand' }
Remove-Item -Recurse -Force $dir
Write-Host "$Tag signed and published"
