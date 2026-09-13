[CmdletBinding()]
param(
    [ValidateRange(1, 600)]
    [int]$DurationSeconds = 30,

    [switch]$ExitLag,

    [string]$OutputDirectory = (Join-Path $PSScriptRoot 'SAFE-RECEIPTS'),

    [string]$ExecutablePath = (Join-Path $PSScriptRoot 'rlogs-bpsr-automarker-windows-boundary.exe')
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$gameProcesses = @(Get-Process -Name 'BPSR_STEAM' -ErrorAction SilentlyContinue)
if ($gameProcesses.Count -ne 1) {
    throw "Expected exactly one running BPSR_STEAM process; found $($gameProcesses.Count)."
}
if (-not (Test-Path -LiteralPath $ExecutablePath -PathType Leaf)) {
    throw "The passive diagnostic executable was not found: $ExecutablePath"
}

$outputRoot = [System.IO.Path]::GetFullPath($OutputDirectory)
[System.IO.Directory]::CreateDirectory($outputRoot) | Out-Null
$timestamp = [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssfffZ')
$outputPath = Join-Path $outputRoot "automarker-windows-boundary-$timestamp.safe.v1.json"
if (Test-Path -LiteralPath $outputPath) {
    throw "Refusing to overwrite an existing receipt: $outputPath"
}

$arguments = @(
    '--private-research',
    '--process-id', [string]$gameProcesses[0].Id,
    '--duration-seconds', [string]$DurationSeconds,
    '--output', $outputPath
)
if ($ExitLag) {
    $arguments += '--exitlag'
}

Write-Host 'This diagnostic is passive: it cannot send, block, modify, or reinject traffic.'
Write-Host 'Place exactly one marker through the normal game UI while it is running.'
& $ExecutablePath @arguments
if ($LASTEXITCODE -ne 0) {
    throw "Passive automarker boundary diagnostic exited with code $LASTEXITCODE."
}
Write-Host "Safe receipt: $outputPath"
