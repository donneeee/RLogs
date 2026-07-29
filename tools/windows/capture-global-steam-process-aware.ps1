[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$OutputDirectory,

    [Parameter(Mandatory = $true)]
    [ValidatePattern('^[A-Za-z0-9._-]{1,128}$')]
    [string]$CaptureId,

    [ValidateRange(1, 3600)]
    [int]$DurationSeconds = 180,

    [string]$DumpcapPath = 'C:\Program Files\Wireshark\dumpcap.exe'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if (-not (Test-Path -LiteralPath $DumpcapPath -PathType Leaf)) {
    throw "dumpcap was not found at $DumpcapPath"
}

$gameProcesses = @(Get-Process -Name 'BPSR_STEAM' -ErrorAction SilentlyContinue)
if ($gameProcesses.Count -ne 1) {
    throw 'Start the Global Steam client and finish authentication before capturing.'
}
$gameProcess = $gameProcesses[0]

$rows = @(
    Get-NetTCPConnection -OwningProcess $gameProcess.Id -State Established |
        Where-Object {
            $_.RemoteAddress -notin @('0.0.0.0', '::', '127.0.0.1', '::1') -and
            $_.RemotePort -gt 0 -and
            $_.LocalPort -gt 0
        } |
        Sort-Object LocalAddress -Unique
)
if ($rows.Count -eq 0) {
    throw 'No established external game TCP connection was found.'
}

$adapterGuids = @(
    foreach ($row in $rows) {
        $ip = Get-NetIPAddress -IPAddress $row.LocalAddress -ErrorAction Stop |
            Select-Object -First 1
        $adapter = Get-NetAdapter -InterfaceIndex $ip.InterfaceIndex -ErrorAction Stop
        $adapter.InterfaceGuid
    }
)
$adapterGuids = @($adapterGuids | Sort-Object -Unique)
if ($adapterGuids.Count -ne 1) {
    throw 'The game is using more than one network adapter. Capture each adapter separately.'
}

$outputRoot = [System.IO.Path]::GetFullPath($OutputDirectory)
[System.IO.Directory]::CreateDirectory($outputRoot) | Out-Null
$finalCapture = Join-Path $outputRoot "$CaptureId.pcap"
$partialCapture = Join-Path $outputRoot "$CaptureId.partial.pcap"
$finalConnections = Join-Path $outputRoot "$CaptureId.connections.json"
$partialConnections = Join-Path $outputRoot "$CaptureId.connections.partial.json"
foreach ($path in @(
    $finalCapture,
    $partialCapture,
    $finalConnections,
    $partialConnections
)) {
    if (Test-Path -LiteralPath $path) {
        throw "Refusing to overwrite existing research file: $path"
    }
}

$interfaceGuid = "{$($adapterGuids[0].ToString().Trim('{}'))}"
$captureInterface = "\Device\NPF_$interfaceGuid"
$repoRoot = [System.IO.Path]::GetFullPath(
    (Join-Path $PSScriptRoot '..\..')
)
$manifestPath = Join-Path $repoRoot 'Cargo.toml'

Write-Host 'Starting process-aware BPSR_STEAM capture.'
Write-Host 'Unrelated TCP frames are discarded in bounded memory before persistence.'
Write-Host "Capture stops automatically after $DurationSeconds seconds."
Write-Host "Private output directory: $outputRoot"

$cargoArguments = @(
    'run',
    '--quiet',
    '--release',
    '--manifest-path',
    $manifestPath,
    '-p',
    'rlogs-process-capture',
    '--',
    '--private-research',
    '--process-id',
    [string]$gameProcess.Id,
    '--interface',
    $captureInterface,
    '--dumpcap',
    $DumpcapPath,
    '--capture-id',
    $CaptureId,
    '--duration-seconds',
    [string]$DurationSeconds,
    '--output-directory',
    $outputRoot
)

& cargo @cargoArguments
if ($LASTEXITCODE -ne 0) {
    throw "Process-aware capture exited with code $LASTEXITCODE"
}
