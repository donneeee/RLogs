[CmdletBinding()]
param(
    [ValidateSet('DRY_RUN', 'RLOGS_WINDIVERT_BYTE_IDENTICAL_PASSTHROUGH_V1', 'RLOGS_WINDIVERT_BOOTSTRAP_AND_BYTE_IDENTICAL_PASSTHROUGH_V1')]
    [string]$Mode = 'DRY_RUN',
    [ValidateRange(1, 300)]
    [int]$SynWaitSeconds = 120,
    [ValidateRange(1, 60)]
    [int]$DurationSeconds = 20,
    [string]$ExecutablePath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if ([string]::IsNullOrWhiteSpace($ExecutablePath)) {
    $ExecutablePath = Join-Path $PSScriptRoot 'rlogs-bpsr-automarker-windivert-passthrough.exe'
}

$outputRoot = Join-Path $PSScriptRoot 'receipts'
New-Item -ItemType Directory -Path $outputRoot -Force | Out-Null
$suffix = [Guid]::NewGuid().ToString('N')
$outputPath = Join-Path $outputRoot "automarker-windivert-passthrough-$suffix.safe.v1.json"

if ($Mode -eq 'DRY_RUN') {
    & $ExecutablePath --dry-run --output $outputPath
    if ($LASTEXITCODE -ne 0) {
        throw "Dry-run exited with code $LASTEXITCODE."
    }
    Write-Host "Dry-run receipt: $outputPath"
    return
}

$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = [Security.Principal.WindowsPrincipal]::new($identity)
$administrator = $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $administrator) {
    $forward = @(
        '-NoProfile',
        '-ExecutionPolicy', 'Bypass',
        '-File', $PSCommandPath,
        '-Mode', $Mode,
        '-SynWaitSeconds', [string]$SynWaitSeconds,
        '-DurationSeconds', [string]$DurationSeconds,
        '-ExecutablePath', $ExecutablePath
    )
    Start-Process -FilePath 'powershell.exe' -Verb RunAs -ArgumentList $forward -Wait
    return
}

$games = @(Get-Process -Name 'BPSR_STEAM' -ErrorAction SilentlyContinue)
if ($games.Count -ne 1) {
    throw "Expected exactly one BPSR_STEAM process, found $($games.Count)."
}

Write-Warning 'RESEARCH PASS-THROUGH CANARY: for a short interval this diverts one exact game TCP connection and immediately reinjects every packet byte-identically.'
Write-Warning 'Do not close this console while the active interval is running. Stop here unless you accept a possible disconnect caused by process/driver failure.'
Write-Host 'Reconnect the game after the SYN wait begins. The canary refuses existing connections because it requires a fresh SYN-scoped ownership proof.'

$bootstrapArguments = @()
if ($Mode -eq 'RLOGS_WINDIVERT_BOOTSTRAP_AND_BYTE_IDENTICAL_PASSTHROUGH_V1') {
    Write-Warning 'EXPLICIT DRIVER BOOTSTRAP: a pinned false-filter handle may install/start WinDivert and will remain open until the pass-through canary ends.'
    $bootstrapArguments = @('--arm-driver-bootstrap', 'RLOGS_WINDIVERT_DRIVER_BOOTSTRAP_V1')
}

& $ExecutablePath `
    --arm-byte-identical-passthrough 'RLOGS_WINDIVERT_BYTE_IDENTICAL_PASSTHROUGH_V1' `
    @bootstrapArguments `
    --process-id $games[0].Id `
    --dependency-directory $PSScriptRoot `
    --syn-wait-seconds $SynWaitSeconds `
    --duration-seconds $DurationSeconds `
    --output $outputPath
if ($LASTEXITCODE -ne 0) {
    throw "Armed pass-through exited with code $LASTEXITCODE. Traffic should resume after the process closes its handle; restart the game if its connection reset."
}
Write-Host "Sanitized canary receipt: $outputPath"
