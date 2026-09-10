[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$OutputDirectory,
    [Parameter(Mandatory = $true)][ValidatePattern('^[A-Za-z0-9._-]{1,128}$')][string]$CaptureId,
    [Parameter(Mandatory = $true)][ValidateScript({ $parsed = $null; [System.Net.IPAddress]::TryParse($_, [ref]$parsed) -and $parsed.AddressFamily -eq [System.Net.Sockets.AddressFamily]::InterNetwork })][string]$ClientIp,
    [Parameter(Mandatory = $true)][string]$Interface,
    [ValidateRange(1, 3600)][int]$DurationSeconds = 180,
    [ValidateSet('all-ip', 'tcp')][string]$TransportMode = 'all-ip',
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
$transportsPath = Join-Path $outputRoot "$CaptureId.transports.json"
$transportsPartial = "$transportsPath.partial"
$captureFilter = if ($TransportMode -eq 'tcp') { "tcp and host $ClientIp" } else { "host $ClientIp" }
if ($DryRun) {
    [ordered]@{ capture_filter=$captureFilter; transport_mode=$TransportMode; client_ip=$ClientIp; interface=$Interface; duration_seconds=$DurationSeconds; capture_path=$capturePath; connections_path=$connectionsPath; transports_path=$transportsPath } | ConvertTo-Json
    return
}
foreach ($tool in @($DumpcapPath, $TsharkPath)) {
    if (-not (Test-Path -LiteralPath $tool -PathType Leaf)) { throw "Required capture tool was not found: $tool" }
}
[System.IO.Directory]::CreateDirectory($outputRoot) | Out-Null
foreach ($path in @($capturePath, $connectionsPath, $connectionsPartial, $transportsPath, $transportsPartial)) {
    if (Test-Path -LiteralPath $path) { throw "Refusing to overwrite existing research file: $path" }
}
Write-Host "Capturing $TransportMode traffic for explicit client $ClientIp; remote-server and transport migration remain in scope."
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
$document=[ordered]@{schema_version=1;connections=@($connections.Values|Sort-Object {"$($_.client.port)|$($_.server.address)|$($_.server.port)"})}
[System.IO.File]::WriteAllText($connectionsPartial,($document|ConvertTo-Json -Depth 6),[System.Text.UTF8Encoding]::new($false))
Move-Item -LiteralPath $connectionsPartial -Destination $connectionsPath

$transportInventory = @{}
& $TsharkPath -r $capturePath -Y "ip.addr==$ClientIp" -T fields -e frame.time_epoch -e ip.src -e ip.proto -e tcp.srcport -e udp.srcport -e ip.dst -e tcp.dstport -e udp.dstport -E 'separator=|' | ForEach-Object {
    $parts = $_ -split '\|', 8
    if ($parts.Count -lt 8) { return }
    $epoch=$parts[0];$source=$parts[1];$protocolNumber=$parts[2];$tcpSource=$parts[3];$udpSource=$parts[4];$destination=$parts[5];$tcpDestination=$parts[6];$udpDestination=$parts[7]
    if ($source -eq $ClientIp) { $direction='outbound';$remote=$destination;$sourcePort=$tcpSource;$destinationPort=$tcpDestination;if (-not $sourcePort) {$sourcePort=$udpSource};if (-not $destinationPort) {$destinationPort=$udpDestination} }
    elseif ($destination -eq $ClientIp) { $direction='inbound';$remote=$source;$sourcePort=$tcpDestination;$destinationPort=$tcpSource;if (-not $sourcePort) {$sourcePort=$udpDestination};if (-not $destinationPort) {$destinationPort=$udpSource} }
    else { return }
    $transport = if ($protocolNumber -eq '6') {'tcp'} elseif ($protocolNumber -eq '17') {'udp'} else {"ip-$protocolNumber"}
    $key="$direction|$transport|$remote|$sourcePort|$destinationPort"
    if (-not $transportInventory.ContainsKey($key)) {
        if ($transportInventory.Count -ge 4096) { throw 'Transport inventory exceeded the bounded 4096-flow limit.' }
        $transportInventory[$key]=[ordered]@{direction=$direction;transport=$transport;remote_address=$remote;client_port=if($sourcePort){[int]$sourcePort}else{$null};remote_port=if($destinationPort){[int]$destinationPort}else{$null};packet_count=0;first_epoch_seconds=$epoch;last_epoch_seconds=$epoch}
    }
    $entry=$transportInventory[$key]
    $entry.packet_count++
    $entry.last_epoch_seconds=$epoch
}
if ($LASTEXITCODE -ne 0) { throw "tshark transport inventory exited with code $LASTEXITCODE" }
if ($transportInventory.Count -eq 0) { throw 'Capture contained no IP packets for the explicit client; no transport inventory was written.' }
$transportDocument=[ordered]@{schema_version=1;client_ip=$ClientIp;capture_filter=$captureFilter;flows=@($transportInventory.Values|Sort-Object direction,transport,remote_address,client_port,remote_port)}
[System.IO.File]::WriteAllText($transportsPartial,($transportDocument|ConvertTo-Json -Depth 6),[System.Text.UTF8Encoding]::new($false))
Move-Item -LiteralPath $transportsPartial -Destination $transportsPath
Write-Host "Private capture: $capturePath"
Write-Host "Metadata-only discovered connection sidecar: $connectionsPath"
Write-Host "Metadata-only bounded transport inventory: $transportsPath"
