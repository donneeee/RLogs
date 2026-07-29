[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$OutputDirectory,

    [Parameter(Mandatory = $true)]
    [ValidatePattern('^[A-Za-z0-9._-]{1,128}$')]
    [string]$CaptureId,

    [ValidateRange(0, 3600)]
    [int]$DurationSeconds = 0,

    [switch]$FollowProcessConnections,

    [string]$SeedConnectionsPath,

    [string]$DumpcapPath = 'C:\Program Files\Wireshark\dumpcap.exe'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if (-not (Test-Path -LiteralPath $DumpcapPath -PathType Leaf)) {
    throw "dumpcap was not found at $DumpcapPath"
}

$gameProcesses = @(Get-Process -Name 'BPSR_STEAM' -ErrorAction SilentlyContinue)
if ($gameProcesses.Count -ne 1) {
    throw 'Start the Global Steam client, log in, and enter the world before running this script.'
}

$outputRoot = [System.IO.Path]::GetFullPath($OutputDirectory)
[System.IO.Directory]::CreateDirectory($outputRoot) | Out-Null
$capturePath = Join-Path $outputRoot "$CaptureId.pcapng"
$connectionsPath = Join-Path $outputRoot "$CaptureId.connections.json"
$connectionsPartialPath = "$connectionsPath.partial"
foreach ($path in @($capturePath, $connectionsPath, $connectionsPartialPath)) {
    if (Test-Path -LiteralPath $path) {
        throw "Refusing to overwrite existing research file: $path"
    }
}

$rows = @(
    Get-NetTCPConnection -OwningProcess $gameProcesses[0].Id -State Established |
        Where-Object {
            $_.RemoteAddress -notin @('0.0.0.0', '::', '127.0.0.1', '::1') -and
            $_.RemotePort -gt 0 -and
            $_.LocalPort -gt 0
        } |
        Sort-Object LocalAddress, LocalPort, RemoteAddress, RemotePort -Unique
)
if ($rows.Count -eq 0) {
    throw 'No established game TCP connections were found. Confirm that the character is in the world.'
}

$localAddresses = @($rows.LocalAddress | Sort-Object -Unique)
$adapterGuids = @(
    foreach ($address in $localAddresses) {
        $ip = Get-NetIPAddress -IPAddress $address -ErrorAction Stop | Select-Object -First 1
        $adapter = Get-NetAdapter -InterfaceIndex $ip.InterfaceIndex -ErrorAction Stop
        $adapter.InterfaceGuid
    }
)
$adapterGuids = @($adapterGuids | Sort-Object -Unique)
if ($adapterGuids.Count -ne 1) {
    throw 'The game is using more than one network adapter. Capture each adapter separately.'
}

$connections = @(
    foreach ($row in $rows) {
        [ordered]@{
            client = [ordered]@{
                address = [string]$row.LocalAddress
                port = [int]$row.LocalPort
            }
            server = [ordered]@{
                address = [string]$row.RemoteAddress
                port = [int]$row.RemotePort
            }
        }
    }
)
$connectionFile = [ordered]@{
    schema_version = 1
    connections = $connections
}
$json = $connectionFile | ConvertTo-Json -Depth 6
[System.IO.File]::WriteAllText(
    $connectionsPath,
    $json,
    [System.Text.UTF8Encoding]::new($false)
)

$captureEndpointScopes = @(
    foreach ($row in $rows) {
        [pscustomobject]@{
            client_address = [string]$row.LocalAddress
            server_address = [string]$row.RemoteAddress
            server_port = [int]$row.RemotePort
        }
    }
)
if (-not [string]::IsNullOrWhiteSpace($SeedConnectionsPath)) {
    if (-not $FollowProcessConnections) {
        throw '-SeedConnectionsPath requires -FollowProcessConnections.'
    }
    $resolvedSeedPath = [System.IO.Path]::GetFullPath($SeedConnectionsPath)
    if (-not (Test-Path -LiteralPath $resolvedSeedPath -PathType Leaf)) {
        throw "Seed connection evidence was not found: $resolvedSeedPath"
    }
    $seedEvidence = Get-Content -LiteralPath $resolvedSeedPath -Raw | ConvertFrom-Json
    if ($seedEvidence.schema_version -ne 1) {
        throw "Unsupported seed connection schema: $($seedEvidence.schema_version)"
    }
    foreach ($connection in @($seedEvidence.connections)) {
        if (
            [string]::IsNullOrWhiteSpace([string]$connection.client.address) -or
            [string]::IsNullOrWhiteSpace([string]$connection.server.address) -or
            [int]$connection.server.port -le 0
        ) {
            throw 'Seed connection evidence contains an invalid endpoint.'
        }
        $captureEndpointScopes += [pscustomobject]@{
            client_address = [string]$connection.client.address
            server_address = [string]$connection.server.address
            server_port = [int]$connection.server.port
        }
    }
    $captureEndpointScopes = @(
        $captureEndpointScopes |
            Sort-Object client_address, server_address, server_port -Unique
    )
}

$flowFilters = @(
    if ($FollowProcessConnections) {
        foreach ($scope in $captureEndpointScopes) {
            $clientAddress = [string]$scope.client_address
            $serverAddress = [string]$scope.server_address
            $serverPort = [int]$scope.server_port
            "((src host $clientAddress and dst host $serverAddress and dst port $serverPort) or (src host $serverAddress and src port $serverPort and dst host $clientAddress))"
        }
    } else {
        foreach ($row in $rows) {
            $clientAddress = [string]$row.LocalAddress
            $clientPort = [int]$row.LocalPort
            $serverAddress = [string]$row.RemoteAddress
            $serverPort = [int]$row.RemotePort
            "((src host $clientAddress and src port $clientPort and dst host $serverAddress and dst port $serverPort) or (src host $serverAddress and src port $serverPort and dst host $clientAddress and dst port $clientPort))"
        }
    }
)
$captureFilter = 'tcp and (' + ($flowFilters -join ' or ') + ')'
$interfaceGuid = "{$($adapterGuids[0].ToString().Trim('{}'))}"
$captureInterface = "\Device\NPF_$interfaceGuid"

Write-Host "Capturing $($rows.Count) exact BPSR_STEAM TCP connection(s)."
if ($FollowProcessConnections) {
    Write-Host 'Following new game sockets to the same private server endpoints.'
    if (-not [string]::IsNullOrWhiteSpace($SeedConnectionsPath)) {
        Write-Host 'Previously observed game-owned endpoints are included in the private filter.'
    }
}
if ($DurationSeconds -gt 0) {
    Write-Host "Perform only the planned in-game scenario. Capture stops after $DurationSeconds seconds."
} else {
    Write-Host 'Perform only the planned in-game scenario. Press Ctrl+C when finished.'
}
Write-Host "Private capture: $capturePath"
Write-Host "Private connection evidence: $connectionsPath"

$dumpcapArguments = @('-q', '-i', $captureInterface, '-s', '0', '-f', $captureFilter, '-w', $capturePath)
if ($DurationSeconds -gt 0) {
    $dumpcapArguments += @('-a', "duration:$DurationSeconds")
}

$connectionMonitor = $null
if ($FollowProcessConnections) {
    $serverEndpointKeys = @(
        $captureEndpointScopes |
            ForEach-Object { "$($_.server_address)|$($_.server_port)" } |
            Sort-Object -Unique
    )
    $serverEndpointJson = $serverEndpointKeys | ConvertTo-Json -Compress
    $seedConnectionsJson = $connections | ConvertTo-Json -Depth 6 -Compress
    $connectionMonitor = Start-Job -ScriptBlock {
        param(
            [int]$GameProcessId,
            [string]$ServerEndpointJson,
            [string]$SeedConnectionsJson,
            [string]$ConnectionsPath,
            [string]$ConnectionsPartialPath
        )

        $serverEndpoints = @($ServerEndpointJson | ConvertFrom-Json)
        $known = @{}
        foreach ($connection in @($SeedConnectionsJson | ConvertFrom-Json)) {
            $key = "$($connection.client.address)|$($connection.client.port)|$($connection.server.address)|$($connection.server.port)"
            $known[$key] = $connection
        }

        while ($true) {
            try {
                $observed = @(
                    Get-NetTCPConnection -OwningProcess $GameProcessId -State Established -ErrorAction Stop |
                        Where-Object {
                            "$($_.RemoteAddress)|$($_.RemotePort)" -in $serverEndpoints -and
                            $_.LocalPort -gt 0
                        }
                )
            } catch {
                break
            }

            $changed = $false
            foreach ($row in $observed) {
                $key = "$($row.LocalAddress)|$($row.LocalPort)|$($row.RemoteAddress)|$($row.RemotePort)"
                if ($known.ContainsKey($key)) {
                    continue
                }
                $known[$key] = [ordered]@{
                    client = [ordered]@{
                        address = [string]$row.LocalAddress
                        port = [int]$row.LocalPort
                    }
                    server = [ordered]@{
                        address = [string]$row.RemoteAddress
                        port = [int]$row.RemotePort
                    }
                }
                $changed = $true
            }

            if ($changed) {
                $orderedConnections = @(
                    $known.Values |
                        Sort-Object {
                            "$($_.client.address)|$($_.client.port)|$($_.server.address)|$($_.server.port)"
                        }
                )
                $connectionFile = [ordered]@{
                    schema_version = 1
                    connections = $orderedConnections
                }
                $json = $connectionFile | ConvertTo-Json -Depth 6
                [System.IO.File]::WriteAllText(
                    $ConnectionsPartialPath,
                    $json,
                    [System.Text.UTF8Encoding]::new($false)
                )
                Move-Item -LiteralPath $ConnectionsPartialPath -Destination $ConnectionsPath -Force
            }

            Start-Sleep -Milliseconds 100
        }
    } -ArgumentList @(
        $gameProcesses[0].Id,
        $serverEndpointJson,
        $seedConnectionsJson,
        $connectionsPath,
        $connectionsPartialPath
    )
}

$dumpcapExitCode = $null
try {
    & $DumpcapPath @dumpcapArguments
    $dumpcapExitCode = $LASTEXITCODE
} finally {
    if ($null -ne $connectionMonitor) {
        Start-Sleep -Milliseconds 200
        Stop-Job -Job $connectionMonitor -ErrorAction SilentlyContinue | Out-Null
        Receive-Job -Job $connectionMonitor -ErrorAction SilentlyContinue | Out-Null
        Remove-Job -Job $connectionMonitor -Force -ErrorAction SilentlyContinue
    }
    if (Test-Path -LiteralPath $connectionsPartialPath) {
        Remove-Item -LiteralPath $connectionsPartialPath -Force
    }
}

if ($dumpcapExitCode -ne 0) {
    throw "dumpcap exited with code $dumpcapExitCode"
}
