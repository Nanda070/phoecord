<#
.SYNOPSIS
    Create a throwaway Hyper-V virtual machine for PhoeCord's manual checks (tasks 1.4.6 and 1.5.7),
    so `phoecord setup`, reboots, `phoecord uninstall` and the loader never touch your own Discord.

.DESCRIPTION
    Run from an elevated PowerShell (Run as administrator) after Hyper-V is enabled:
        Enable-WindowsOptionalFeature -Online -FeatureName Microsoft-Hyper-V-All -All
    (then reboot once).

    The VM is Generation 2 with Secure Boot and a virtual TPM, so Windows 11 installs on it. It uses
    the built-in "Default Switch" for network (NAT, no setup needed). Its files go under -Root.

    After Windows is installed inside the VM and Discord Canary is set up there, take a checkpoint:
        Checkpoint-VM -Name PhoeCord-Test -SnapshotName clean
    and return to it before every new run of the checks:
        Restore-VMSnapshot -VMName PhoeCord-Test -Name clean -Confirm:$false

.EXAMPLE
    .\scripts\dev\New-TestVm.ps1 -Iso D:\ISO\Win11_24H2_English_x64.iso
#>
[CmdletBinding()]
param(
    # Path to a Windows 11 (or 10) installation ISO.
    [Parameter(Mandatory)] [string] $Iso,
    [string] $Name = 'PhoeCord-Test',
    # Where the VM's configuration and virtual disk go.
    [string] $Root = 'D:\HyperV',
    [int] $MemoryGB = 8,
    [int] $DiskGB = 80,
    [int] $Cpu = 4
)

$ErrorActionPreference = 'Stop'

$admin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole(
    [Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $admin) { throw 'Run this from an elevated PowerShell (Run as administrator).' }
if (-not (Get-Module -ListAvailable Hyper-V)) {
    throw 'Hyper-V is not enabled. Run: Enable-WindowsOptionalFeature -Online -FeatureName Microsoft-Hyper-V-All -All, then reboot.'
}
if (-not (Test-Path $Iso)) { throw "ISO not found: $Iso" }
if (Get-VM -Name $Name -ErrorAction SilentlyContinue) { throw "A VM named '$Name' already exists." }

$vmDir = Join-Path $Root $Name
New-Item -ItemType Directory -Force -Path $vmDir | Out-Null
$vhd = Join-Path $vmDir "$Name.vhdx"

New-VM -Name $Name -Generation 2 -Path $Root `
    -MemoryStartupBytes ([int64]$MemoryGB * 1GB) `
    -NewVHDPath $vhd -NewVHDSizeBytes ([int64]$DiskGB * 1GB) `
    -SwitchName 'Default Switch' | Out-Null

Set-VMProcessor -VMName $Name -Count $Cpu
Set-VMMemory -VMName $Name -DynamicMemoryEnabled $false
# Windows 11 needs Secure Boot and a TPM.
Set-VMFirmware -VMName $Name -EnableSecureBoot On -SecureBootTemplate MicrosoftWindows
Set-VMKeyProtector -VMName $Name -NewLocalKeyProtector
Enable-VMTPM -VMName $Name
# Boot from the ISO first.
Add-VMDvdDrive -VMName $Name -Path $Iso
Set-VMFirmware -VMName $Name -FirstBootDevice (Get-VMDvdDrive -VMName $Name)
Enable-VMIntegrationService -VMName $Name -Name 'Guest Service Interface'
# Keep the VM off until you start it, so nothing runs in the background.
Set-VM -Name $Name -AutomaticStartAction Nothing -AutomaticStopAction ShutDown -CheckpointType Standard

Start-VM -Name $Name
vmconnect.exe localhost $Name

Write-Host ''
Write-Host "VM '$Name' created in $vmDir and started. Press a key in the VM window to boot the installer."
Write-Host 'After Windows and Discord Canary are installed inside it:'
Write-Host "    Checkpoint-VM -Name $Name -SnapshotName clean"
