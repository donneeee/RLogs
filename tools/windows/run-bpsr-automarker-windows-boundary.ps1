[CmdletBinding()]
param(
    [ValidateRange(1, 600)]
    [int]$DurationSeconds = 30,

    [switch]$ExitLag,

    [switch]$Mirror,

    [string]$InterfaceName,

    [string]$OutputDirectory = (Join-Path $PSScriptRoot 'SAFE-RECEIPTS'),

    [string]$ExecutablePath = (Join-Path $PSScriptRoot 'rlogs-bpsr-automarker-windows-boundary.exe')
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if ($Mirror -and $ExitLag) {
    throw 'Mirror mode cannot prove whether ExitLag is active or establish WFP ordering; do not combine -Mirror and -ExitLag.'
}
$gameProcesses = @()
if (-not $Mirror) {
    $gameProcesses = @(Get-Process -Name 'BPSR_STEAM' -ErrorAction SilentlyContinue)
    if ($gameProcesses.Count -ne 1) {
        throw "Expected exactly one running BPSR_STEAM process; found $($gameProcesses.Count). Use -Mirror only on a separate passive capture PC."
    }
}
if (-not (Test-Path -LiteralPath $ExecutablePath -PathType Leaf)) {
    throw "The passive diagnostic executable was not found: $ExecutablePath"
}

$outputRoot = [System.IO.Path]::GetFullPath($OutputDirectory)
[System.IO.Directory]::CreateDirectory($outputRoot) | Out-Null
$timestamp = [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssfffZ')
$outputPath = Join-Path $outputRoot "automarker-windows-boundary-$timestamp.safe.v2.json"
if (Test-Path -LiteralPath $outputPath) {
    throw "Refusing to overwrite an existing receipt: $outputPath"
}

$arguments = @('--private-research', '--duration-seconds', [string]$DurationSeconds, '--output', $outputPath)
if ($Mirror) { $arguments += '--mirror' }
else {
    $arguments += @('--process-id', [string]$gameProcesses[0].Id)
    if ($ExitLag) { $arguments += '--exitlag' }
}
if (-not [string]::IsNullOrWhiteSpace($InterfaceName)) {
    $arguments += @('--interface-name', $InterfaceName)
}

Write-Host 'This diagnostic is passive: it cannot send, block, modify, or reinject traffic.'
if ($Mirror) {
    Write-Host 'Mirror mode: game-process ownership and ExitLag/WFP ordering will remain explicitly unproven.'
}
Write-Host 'Place exactly one marker through the normal game UI while it is running.'
& $ExecutablePath @arguments
if ($LASTEXITCODE -ne 0) {
    throw "Passive automarker boundary diagnostic exited with code $LASTEXITCODE."
}
Write-Host "Safe receipt: $outputPath"
