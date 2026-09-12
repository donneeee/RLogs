[CmdletBinding()]
param(
    [string]$InstallRoot,
    [string]$SteamManifest,
    [string]$Interface,
    [ValidateRange(20, 30)][int]$DurationSeconds = 25,
    [string]$DumpcapPath = 'C:\Program Files\Wireshark\dumpcap.exe',
    [switch]$DryRun
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$expectedAppId = '3681810'
$expectedBuild = '25247556'
$sourcePackBuild = '24687926'
$expectedExecutableBytes = 808496
$expectedExecutableSha256 = '90537dbd0e4c9d4b3ed2af06aa8bf7ffe7219fc94bb2f6963673f7b340288588'
$expectedGameAssemblyBytes = 218074672
$expectedGameAssemblySha256 = '4a079aec0a3e51a8023aa86bbd152e12068907b65b9aafb355d20bb6d6c41fe3'
$privateDirectoryName = 'PRIVATE-RAW-DO-NOT-SHARE'

function Get-Sha256([string]$Path) {
    $stream = [System.IO.File]::OpenRead($Path)
    $algorithm = [System.Security.Cryptography.SHA256]::Create()
    try { ([BitConverter]::ToString($algorithm.ComputeHash($stream))).Replace('-', '').ToLowerInvariant() }
    finally { $algorithm.Dispose(); $stream.Dispose() }
}

function Get-AcfValue([string]$Text, [string]$Key) {
    $match = [regex]::Match($Text, '(?im)^\s*"' + [regex]::Escape($Key) + '"\s+"([^"]*)"')
    if ($match.Success) { return $match.Groups[1].Value }
    $null
}

function Test-PackageIntegrity {
    $manifest = Join-Path $PSScriptRoot 'SHA256SUMS.txt'
    if (-not (Test-Path -LiteralPath $manifest -PathType Leaf)) { throw 'SHA256SUMS.txt is missing from this package.' }
    $checked = 0
    foreach ($line in Get-Content -LiteralPath $manifest) {
        if ([string]::IsNullOrWhiteSpace($line)) { continue }
        if ($line -notmatch '^([0-9a-f]{64}) \*(.+)$') { throw "Malformed package hash line: $line" }
        $path = Join-Path $PSScriptRoot $Matches[2]
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Package payload is missing: $($Matches[2])" }
        if ((Get-Sha256 $path) -cne $Matches[1]) { throw "Package payload hash mismatch: $($Matches[2])" }
        $checked++
    }
    if ($checked -lt 6) { throw 'Package hash manifest did not cover the required payload set.' }
}

function Find-ExactInstall {
    $steamRoots = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::OrdinalIgnoreCase)
    foreach ($candidate in @(
        (Get-ItemProperty -LiteralPath 'HKCU:\Software\Valve\Steam' -ErrorAction SilentlyContinue).SteamPath,
        (Get-ItemProperty -LiteralPath 'HKLM:\Software\WOW6432Node\Valve\Steam' -ErrorAction SilentlyContinue).InstallPath
    )) {
        if ($candidate) { [void]$steamRoots.Add($candidate) }
    }
    foreach ($root in @($steamRoots)) {
        $libraries = Join-Path $root 'steamapps\libraryfolders.vdf'
        if (Test-Path -LiteralPath $libraries -PathType Leaf) {
            $text = Get-Content -LiteralPath $libraries -Raw
            foreach ($match in [regex]::Matches($text, '(?im)^\s*"path"\s+"([^"]+)"')) {
                [void]$steamRoots.Add(($match.Groups[1].Value -replace '\\\\', '\'))
            }
        }
    }
    $matches = @()
    foreach ($root in $steamRoots) {
        $manifest = Join-Path $root "steamapps\appmanifest_$expectedAppId.acf"
        if (-not (Test-Path -LiteralPath $manifest -PathType Leaf)) { continue }
        $text = Get-Content -LiteralPath $manifest -Raw
        if ((Get-AcfValue $text 'appid') -ne $expectedAppId -or (Get-AcfValue $text 'buildid') -ne $expectedBuild) { continue }
        $installDir = Get-AcfValue $text 'installdir'
        if (-not $installDir) { continue }
        $matches += [pscustomobject]@{Manifest=$manifest;InstallRoot=(Join-Path $root "steamapps\common\$installDir\bpsr")}
    }
    if ($matches.Count -ne 1) { throw 'Exact-build Steam discovery did not find one unique build-25247556 installation. Supply -InstallRoot and -SteamManifest together.' }
    $matches[0]
}

function Resolve-CaptureInterface([int]$ProcessId) {
    $rows = @(Get-NetTCPConnection -OwningProcess $ProcessId -State Established -ErrorAction Stop | Where-Object {
        $_.RemotePort -gt 0 -and $_.LocalPort -gt 0 -and $_.RemoteAddress -notin @('0.0.0.0','::','127.0.0.1','::1')
    })
    if ($rows.Count -eq 0) {
        throw 'The game has no established external TCP socket. If a network accelerator exposes only loopback sockets, disable it for this 25-second evidence capture or supply a known process-owned Npcap interface.'
    }
    $guids = @(
        @(
            foreach ($row in $rows) {
                $ip = Get-NetIPAddress -IPAddress $row.LocalAddress -ErrorAction Stop | Select-Object -First 1
                (Get-NetAdapter -InterfaceIndex $ip.InterfaceIndex -ErrorAction Stop).InterfaceGuid
            }
        ) | Sort-Object -Unique
    )
    if ($guids.Count -ne 1) { throw 'The game maps to more than one capture adapter. Supply -Interface explicitly after reviewing the active route.' }
    "\Device\NPF_{$($guids[0].ToString().Trim('{}'))}"
}

Test-PackageIntegrity

if ($DryRun) {
    [ordered]@{
        mode='dry_run'
        expected_build=$expectedBuild
        duration_seconds=$DurationSeconds
        capture_scope='exact-process-owned-tcp-only'
        private_output_directory=$privateDirectoryName
        packet_send_available=$false
        packet_rewrite_available=$false
        process_memory_access=$false
    } | ConvertTo-Json
    return
}

if ([string]::IsNullOrWhiteSpace($InstallRoot) -xor [string]::IsNullOrWhiteSpace($SteamManifest)) {
    throw '-InstallRoot and -SteamManifest must be supplied together, or both omitted.'
}
if ([string]::IsNullOrWhiteSpace($InstallRoot)) {
    $install = Find-ExactInstall
    $InstallRoot = $install.InstallRoot
    $SteamManifest = $install.Manifest
}
$InstallRoot = [System.IO.Path]::GetFullPath($InstallRoot)
$SteamManifest = [System.IO.Path]::GetFullPath($SteamManifest)
if (-not (Test-Path -LiteralPath $SteamManifest -PathType Leaf)) { throw 'Steam app manifest was not found.' }
$manifestText = Get-Content -LiteralPath $SteamManifest -Raw
if ((Get-AcfValue $manifestText 'appid') -ne $expectedAppId -or (Get-AcfValue $manifestText 'buildid') -ne $expectedBuild) {
    throw 'Steam manifest is not exact app 3681810 build 25247556.'
}

$gameExecutable = Join-Path $InstallRoot 'BPSR_STEAM.exe'
$gameAssembly = Join-Path $InstallRoot 'GameAssembly.dll'
foreach ($identity in @(
    @($gameExecutable,$expectedExecutableBytes,$expectedExecutableSha256,'BPSR_STEAM.exe'),
    @($gameAssembly,$expectedGameAssemblyBytes,$expectedGameAssemblySha256,'GameAssembly.dll')
)) {
    if (-not (Test-Path -LiteralPath $identity[0] -PathType Leaf)) { throw "$($identity[3]) was not found in the exact install root." }
    $file = Get-Item -LiteralPath $identity[0]
    if ($file.Length -ne [long]$identity[1] -or (Get-Sha256 $file.FullName) -cne $identity[2]) { throw "$($identity[3]) does not match reviewed build 25247556." }
}

$processes = @(Get-Process -Name 'BPSR_STEAM' -ErrorAction SilentlyContinue)
if ($processes.Count -ne 1) { throw 'Start exactly one Global Steam game client, enter the dungeon as party leader, and try again.' }
$gameProcess = $processes[0]
if (-not $gameProcess.Path.Equals($gameExecutable, [System.StringComparison]::OrdinalIgnoreCase)) { throw 'The running game executable is not the exact hash-gated installation.' }

$npcapPresent = @(
    @(
        (Join-Path $env:WINDIR 'System32\Npcap\wpcap.dll'),
        (Join-Path $env:WINDIR 'SysWOW64\Npcap\wpcap.dll')
    ) | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf }
)
$dumpcapAvailable = Test-Path -LiteralPath $DumpcapPath -PathType Leaf
if ($npcapPresent.Count -eq 0 -and -not $dumpcapAvailable) {
    throw 'Neither a local Npcap runtime nor dumpcap.exe was found. Install Npcap, or install Wireshark/dumpcap and rerun.'
}
if ([string]::IsNullOrWhiteSpace($Interface)) { $Interface = Resolve-CaptureInterface $gameProcess.Id }

$privateRoot = Join-Path $PSScriptRoot $privateDirectoryName
[System.IO.Directory]::CreateDirectory($privateRoot) | Out-Null
$stamp = [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssfffZ')
$captureId = "automarker-wire-layout-$stamp"
$runRoot = Join-Path $privateRoot $captureId
if (Test-Path -LiteralPath $runRoot) { throw 'Private run folder already exists; refusing overwrite.' }
[System.IO.Directory]::CreateDirectory($runRoot) | Out-Null

$captureExe = Join-Path $PSScriptRoot 'rlogs-process-capture.exe'
$journalExe = Join-Path $PSScriptRoot 'rlogs-protocol-journal.exe'
$receiptExe = Join-Path $PSScriptRoot 'rlogs-bpsr-automarker-wire-layout-receipt.exe'
$pack = Join-Path $PSScriptRoot 'pack-24687926.json'
$capturePath = Join-Path $runRoot "$captureId.pcap"
$connectionsPath = Join-Path $runRoot "$captureId.connections.json"
$journalPath = Join-Path $runRoot "$captureId.protocol.jsonl"
$receiptPath = Join-Path $runRoot "$captureId.safe-wire-layout-receipt.json"

Write-Warning 'READ-ONLY CAPTURE: this package never sends, rewrites, injects, or modifies game traffic.'
Write-Host "Capture duration: $DurationSeconds seconds."
Write-Host 'After capture begins, wait about 8 seconds, place exactly ONE marker through the normal game UI, then do nothing until capture ends.'
Write-Host 'Starting in 5 seconds; return focus to the game now.'
foreach ($remaining in 5..1) { Write-Host "$remaining..."; Start-Sleep -Seconds 1 }

$captureArgs = @('--private-research','--process-id',[string]$gameProcess.Id,'--interface',$Interface,'--capture-id',$captureId,'--duration-seconds',[string]$DurationSeconds,'--output-directory',$runRoot)
if ($dumpcapAvailable) { $captureArgs += @('--dumpcap',$DumpcapPath) }
& $captureExe @captureArgs
if ($LASTEXITCODE -ne 0) { throw 'Process-owned Npcap/dumpcap capture failed closed. Raw partial files, if any, remain only in the private run folder.' }

& $journalExe --private-research --captured-build $expectedBuild --unverified-carry-forward-pack-source-build $sourcePackBuild --pack $pack --connections $connectionsPath --capture-id $captureId $capturePath $journalPath
if ($LASTEXITCODE -ne 0) { throw 'Offline protocol journal generation failed closed.' }

& $receiptExe --private-research --captured-build $expectedBuild --unverified-carry-forward-pack-source-build $sourcePackBuild --pack $pack --connections $connectionsPath --journal $journalPath --output $receiptPath $capturePath
if ($LASTEXITCODE -ne 0) { throw 'Offline safe receipt generation failed. The most likely cause is that exactly one normal marker request was not present.' }

$artifacts = @(
    foreach ($path in @($capturePath,$connectionsPath,$journalPath,$receiptPath)) {
        $file = Get-Item -LiteralPath $path
        [ordered]@{name=$file.Name;bytes=$file.Length;sha256=Get-Sha256 $file.FullName;safe_to_share=$file.Name.EndsWith('.safe-wire-layout-receipt.json')}
    }
)
$privateManifest = [ordered]@{schema_version=1;build_id=$expectedBuild;capture_mode='read-only-process-owned';artifacts=$artifacts}
$privateManifestPath = Join-Path $runRoot "$captureId.private-artifact-hashes.json"
[System.IO.File]::WriteAllText($privateManifestPath,($privateManifest|ConvertTo-Json -Depth 8),(New-Object System.Text.UTF8Encoding($false)))

Write-Host "Safe receipt: $receiptPath"
Write-Host "PRIVATE raw evidence (do not share): $runRoot"
Write-Host 'Capture complete. No traffic was sent or modified by RLogs.'
