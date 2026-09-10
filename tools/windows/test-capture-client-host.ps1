[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$launcher = Join-Path $PSScriptRoot 'capture-client-host.ps1'
$common = @{
    OutputDirectory = Join-Path ([System.IO.Path]::GetTempPath()) 'rlogs-capture-launcher-dry-run'
    CaptureId = 'marker-audit-dry-run'
    ClientIp = '192.0.2.10'
    Interface = 'test-interface'
    DurationSeconds = 30
    DryRun = $true
}

$marker = & $launcher @common -CapturePurpose marker-audit | ConvertFrom-Json
if ($marker.capture_purpose -ne 'marker-audit') { throw 'Marker-audit purpose was not retained.' }
if ($marker.transport_mode -ne 'all-ip') { throw 'Marker audit silently stopped requesting all-IP capture.' }
if ($marker.capture_filter -ne 'host 192.0.2.10') { throw "Unexpected marker-audit filter: $($marker.capture_filter)" }
if ($marker.filter_scope -ne 'explicit-client-only') { throw 'Marker audit lost its single-client filter scope.' }
if ($marker.capture_filter -match '(^|\s)tcp(\s|$)') { throw 'Marker audit silently became TCP-only.' }

$rejectedTcp = $false
try {
    & $launcher @common -CapturePurpose marker-audit -TransportMode tcp | Out-Null
} catch {
    $rejectedTcp = $_.Exception.Message -like 'Marker audits require all-ip capture*'
}
if (-not $rejectedTcp) { throw 'A requested TCP-only marker audit was not rejected.' }

$tcpFallback = & $launcher @common -CapturePurpose general -TransportMode tcp | ConvertFrom-Json
if ($tcpFallback.capture_filter -ne 'tcp and host 192.0.2.10') { throw 'Explicit general TCP fallback changed unexpectedly.' }

Write-Host 'capture-client-host marker-audit boundary tests passed.'
