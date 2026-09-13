[CmdletBinding()]
param(
    [ValidateSet('DRY_RUN', 'RLOGS_WINDIVERT_DRIVER_REMOVE_V1')]
    [string]$Mode = 'DRY_RUN'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$dllPath = Join-Path $PSScriptRoot 'WinDivert.dll'
$driverPath = Join-Path $PSScriptRoot 'WinDivert64.sys'
$expectedDllHash = 'C1E060EE19444A259B2162F8AF0F3FE8C4428A1C6F694DCE20DE194AC8D7D9A2'
$expectedDriverHash = '8DA085332782708D8767BCACE5327A6EC7283C17CFB85E40B03CD2323A90DDC2'
$expectedSigner = '043589F75FCE2795E7F2CC3E526D46784D5DDAB3'

function Test-PinnedFiles {
    if (-not (Test-Path -LiteralPath $dllPath -PathType Leaf)) { throw "Missing pinned dependency: $dllPath" }
    if (-not (Test-Path -LiteralPath $driverPath -PathType Leaf)) { throw "Missing pinned dependency: $driverPath" }
    if ((Get-FileHash -LiteralPath $dllPath -Algorithm SHA256).Hash -ne $expectedDllHash) {
        throw 'Pinned WinDivert DLL hash mismatch.'
    }
    if ((Get-FileHash -LiteralPath $driverPath -Algorithm SHA256).Hash -ne $expectedDriverHash) {
        throw 'Pinned WinDivert driver hash mismatch.'
    }
    $signature = Get-AuthenticodeSignature -LiteralPath $driverPath
    if ($signature.Status -ne 'Valid' -or
        -not $signature.SignerCertificate -or
        $signature.SignerCertificate.Thumbprint -ne $expectedSigner) {
        throw 'Pinned WinDivert driver signature validation failed.'
    }
}

function Test-Administrator {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [Security.Principal.WindowsPrincipal]::new($identity)
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

Test-PinnedFiles

if ($Mode -eq 'DRY_RUN') {
    $service = Get-Service -Name 'WinDivert' -ErrorAction SilentlyContinue
    $state = if ($null -eq $service) { 'absent' } else { [string]$service.Status }
    Write-Host "Read-only preflight passed: pinned WinDivert 2.2.2 files and signature are valid; service state is $state."
    Write-Host 'No executable was started, no elevation was requested, no receipt was written, and no service or driver state was changed.'
    return
}

if (-not (Test-Administrator)) {
    $forward = @(
        '-NoProfile',
        '-ExecutionPolicy', 'Bypass',
        '-File', $PSCommandPath,
        '-Mode', $Mode
    )
    Start-Process -FilePath 'powershell.exe' -Verb RunAs -ArgumentList $forward -Wait
    return
}

$receipts = @(Get-ChildItem -LiteralPath (Join-Path $PSScriptRoot 'receipts') -Filter 'automarker-windivert-passthrough-*.safe.v1.json' -File -ErrorAction SilentlyContinue)
$ownedReceipt = $false
foreach ($receipt in $receipts) {
    try {
        $record = Get-Content -LiteralPath $receipt.FullName -Raw | ConvertFrom-Json
        if ($record.artifact_kind -eq 'sanitized-automarker-windivert-byte-identical-passthrough' -and
            $record.mode -eq 'explicitly-armed-bootstrap-and-passthrough' -and
            $record.outcome -eq 'pass') {
            $ownedReceipt = $true
            break
        }
    } catch {
        continue
    }
}
if (-not $ownedReceipt) {
    throw 'Refusing removal: this package has no successful combined bootstrap-and-pass-through receipt.'
}

Write-Warning 'EXPLICIT DRIVER REMOVAL: all WinDivert client handles must be closed. Stopping the shared WinDivert service can disrupt other WinDivert applications.'
$service = Get-Service -Name 'WinDivert' -ErrorAction SilentlyContinue
if ($null -eq $service) {
    Write-Host 'WinDivert service is already absent; no change was made.'
    return
}
& sc.exe stop WinDivert
if ($LASTEXITCODE -ne 0) { throw "sc.exe stop WinDivert failed with code $LASTEXITCODE. Close all WinDivert clients and retry." }
& sc.exe delete WinDivert
if ($LASTEXITCODE -ne 0 -and $LASTEXITCODE -ne 1060 -and $LASTEXITCODE -ne 1072) {
    throw "sc.exe delete WinDivert failed with code $LASTEXITCODE."
}
Write-Host 'WinDivert stop/delete was requested. Reboot if Windows defers final driver unload or service deletion.'
