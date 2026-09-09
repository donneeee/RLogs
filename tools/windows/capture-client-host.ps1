[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$OutputDirectory,
    [Parameter(Mandatory = $true)][ValidatePattern('^[A-Za-z0-9._-]{1,128}$')][string]$CaptureId,
    [Parameter(Mandatory = $true)][ValidateScript({ $parsed = $null; [System.Net.IPAddress]::TryParse($_, [ref]$parsed) -and $parsed.AddressFamily -eq [System.Net.Sockets.AddressFamily]::InterNetwork })][string]$ClientIp,
    [Parameter(Mandatory = $true)][string]$Interface,
    [ValidateRange(1, 3600)][int]$DurationSeconds = 180,
    [string]$DumpcapPath = 'C:\Program Files\Wireshark\dumpcap.exe',
    [string]$TsharkPath = 'C:\Program Files\Wireshark\tshark.exe',
    [switch]$DryRun
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$outputRoot = [System.IO.Path]::GetFullPath($OutputDirectory)
$capturePath = Join-Path $outputRoot "$CaptureId.pcapng"
$connectionsPath = Join-Path $outputRoot "$CaptureId.connections.json"
$connectionsPartial = "$connectionsPath.partial"
$captureFilter = "tcp and host $ClientIp"
if ($DryRun) {
    [ordered]@{ capture_filter=$captureFilter; client_ip=$ClientIp; interface=$Interface; duration_seconds=$DurationSeconds; capture_path=$capturePath; connections_path=$connectionsPath } | ConvertTo-Json
    return
}
foreach ($tool in @($DumpcapPath, $TsharkPath)) {
    if (-not (Test-Path -LiteralPath $tool -PathType Leaf)) { throw "Required capture tool was not found: $tool" }
}
[System.IO.Directory]::CreateDirectory($outputRoot) | Out-Null
foreach ($path in @($capturePath, $connectionsPath, $connectionsPartial)) {
    if (Test-Path -LiteralPath $path) { throw "Refusing to overwrite existing research file: $path" }
}
Write-Host "Capturing every TCP flow for explicit client $ClientIp; remote-server migration remains in scope."
& $DumpcapPath -q -i $Interface -s 0 -f $captureFilter -a "duration:$DurationSeconds" -w $capturePath
if ($LASTEXITCODE -ne 0) { throw "dumpcap exited with code $LASTEXITCODE" }
$connections = @{}
& $TsharkPath -r $capturePath -Y "tcp && ip.addr==$ClientIp" -T fields -e ip.src -e tcp.srcport -e ip.dst -e tcp.dstport -E 'separator=,' | ForEach-Object {
    $parts = $_ -split ','
    if ($parts.Count -lt 4) { return }
    if ($parts[0] -eq $ClientIp) { $ca=$parts[0];$cp=[int]$parts[1];$sa=$parts[2];$sp=[int]$parts[3] }
    elseif ($parts[2] -eq $ClientIp) { $ca=$parts[2];$cp=[int]$parts[3];$sa=$parts[0];$sp=[int]$parts[1] }
    else { return }
    if ($cp -le 0 -or $sp -le 0 -or $sa -eq $ClientIp) { return }
    $connections["$ca|$cp|$sa|$sp"]=[ordered]@{client=[ordered]@{address=$ca;port=$cp};server=[ordered]@{address=$sa;port=$sp}}
}
if ($LASTEXITCODE -ne 0) { throw "tshark tuple discovery exited with code $LASTEXITCODE" }
if ($connections.Count -eq 0) { throw 'Capture contained no complete TCP tuple for the explicit client IP; no sidecar was written.' }
$document=[ordered]@{schema_version=1;connections=@($connections.Values|Sort-Object {"$($_.client.port)|$($_.server.address)|$($_.server.port)"})}
[System.IO.File]::WriteAllText($connectionsPartial,($document|ConvertTo-Json -Depth 6),[System.Text.UTF8Encoding]::new($false))
Move-Item -LiteralPath $connectionsPartial -Destination $connectionsPath
Write-Host "Private capture: $capturePath"
Write-Host "Metadata-only discovered connection sidecar: $connectionsPath"
