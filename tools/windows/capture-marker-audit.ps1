[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$OutputDirectory,
    [Parameter(Mandatory = $true)][ValidatePattern('^[A-Za-z0-9._-]{1,128}$')][string]$CaptureId,
    [Parameter(Mandatory = $true)][ValidateScript({ $parsed = $null; [System.Net.IPAddress]::TryParse($_, [ref]$parsed) -and $parsed.AddressFamily -eq [System.Net.Sockets.AddressFamily]::InterNetwork })][string]$ClientIp,
    [Parameter(Mandatory = $true)][string]$Interface,
    [Parameter(Mandatory = $true)][ValidatePattern('^[A-Za-z0-9._-]{1,128}$')][string]$GameBuild,
    [Parameter(Mandatory = $true)][string]$GameExecutablePath,
    [string]$BuildFileManifestPath,
    [string]$DistributionSnapshotPath,
    [Parameter(Mandatory = $true)][string]$ProtocolPackPath,
    [Parameter(Mandatory = $true)][ValidatePattern('^sha256:[0-9a-f]{64}$')][string]$ProtocolPackDigest,
    [ValidatePattern('^[A-Za-z0-9._-]{1,128}$')][string]$ProtocolPackSourceBuild,
    [Parameter(Mandatory = $true)][ValidateRange(1, [int]::MaxValue)][int]$SceneId,
    [Parameter(Mandatory = $true)][ValidateLength(1, 256)][string]$SceneName,
    [Parameter(Mandatory = $true)][string]$ActionPlanPath,
    [ValidateRange(60, 3600)][int]$DurationSeconds = 180,
    [ValidateRange(10, 60)][int]$PrePlacementIdleSeconds = 15,
    [ValidateRange(5, 30)][int]$MarkerSpacingSeconds = 5,
    [ValidateRange(1, 10)][int]$ResponseWindowSeconds = 2,
    [ValidateRange(10, 60)][int]$PostPlacementIdleSeconds = 10,
    [ValidateRange(10, 60)][int]$PostEngagementSeconds = 10,
    [string]$DumpcapPath = 'C:\Program Files\Wireshark\dumpcap.exe',
    [string]$TsharkPath = 'C:\Program Files\Wireshark\tshark.exe',
    [switch]$RawCaptureWithUnverifiedProtocolCarryForward,
    [switch]$TestOnlyAllowIdentityFixture,
    [switch]$DryRun
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Get-Sha256([string]$Path) {
    $stream = [System.IO.File]::OpenRead($Path)
    $algorithm = [System.Security.Cryptography.SHA256]::Create()
    try { ([BitConverter]::ToString($algorithm.ComputeHash($stream))).Replace('-', '').ToLowerInvariant() }
    finally { $algorithm.Dispose(); $stream.Dispose() }
}

function Get-BytesSha256([byte[]]$Bytes) {
    $algorithm = [System.Security.Cryptography.SHA256]::Create()
    try { ([BitConverter]::ToString($algorithm.ComputeHash($Bytes))).Replace('-', '').ToLowerInvariant() }
    finally { $algorithm.Dispose() }
}

function Get-ProtocolPackDigest([object]$Pack) {
    $digestDefinition = [ordered]@{schema_version=$Pack.schema_version;pack_id=$Pack.pack_id;target=$Pack.target;provenance=$Pack.provenance;routes=$Pack.routes}
    $encoded = [System.Text.Encoding]::UTF8.GetBytes(($digestDefinition | ConvertTo-Json -Compress -Depth 100))
    "sha256:$(Get-BytesSha256 $encoded)"
}

function ConvertTo-UnixMicros([DateTimeOffset]$Timestamp) {
    [long](($Timestamp.UtcTicks - [DateTimeOffset]::UnixEpoch.UtcTicks) / 10)
}

function Write-Utf8Json([string]$Path, [object]$Value) {
    [System.IO.File]::WriteAllText($Path, ($Value | ConvertTo-Json -Depth 100), (New-Object System.Text.UTF8Encoding($false)))
}

function Require-File([string]$Path, [string]$Description) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "$Description was not found: $Path" }
    [System.IO.Path]::GetFullPath($Path)
}

function Test-FiniteNumber([object]$Value) {
    if ($null -eq $Value) { return $false }
    $number = 0.0
    if (-not [double]::TryParse([string]$Value, [System.Globalization.NumberStyles]::Float, [System.Globalization.CultureInfo]::InvariantCulture, [ref]$number)) { return $false }
    -not [double]::IsNaN($number) -and -not [double]::IsInfinity($number)
}

function ConvertTo-WindowsCommandLineArgument([string]$Argument) {
    if ($Argument.Length -gt 0 -and $Argument -notmatch '[\s"]') { return $Argument }
    $builder = New-Object System.Text.StringBuilder
    [void]$builder.Append('"')
    $slashes = 0
    foreach ($character in $Argument.ToCharArray()) {
        if ($character -eq '\') { $slashes++; continue }
        if ($character -eq '"') {
            [void]$builder.Append(('\' * (($slashes * 2) + 1)))
            [void]$builder.Append('"')
            $slashes = 0
            continue
        }
        if ($slashes -gt 0) { [void]$builder.Append(('\' * $slashes)); $slashes = 0 }
        [void]$builder.Append($character)
    }
    if ($slashes -gt 0) { [void]$builder.Append(('\' * ($slashes * 2))) }
    [void]$builder.Append('"')
    $builder.ToString()
}

function Get-AvailableArtifactHashes([string[]]$Paths) {
    $available = @()
    foreach ($path in $Paths) {
        if (Test-Path -LiteralPath $path -PathType Leaf) {
            try {
                $file = Get-Item -LiteralPath $path
                $available += [ordered]@{path=$file.FullName;bytes=$file.Length;sha256=Get-Sha256 $file.FullName}
            } catch {
                $available += [ordered]@{path=[System.IO.Path]::GetFullPath($path);hash_error=$_.Exception.Message}
            }
        }
    }
    $available
}

function Stop-CaptureProcess([System.Diagnostics.Process]$Process) {
    if ($null -eq $Process -or $Process.HasExited) { return }
    $taskkill = Join-Path $env:SystemRoot 'System32\taskkill.exe'
    & $taskkill /PID ([string]$Process.Id) /T /F | Out-Null
    try { $Process.WaitForExit(5000) | Out-Null } catch {}
}

$outputRoot = [System.IO.Path]::GetFullPath($OutputDirectory)
$capturePath = Join-Path $outputRoot "$CaptureId.pcapng"
$connectionsPath = Join-Path $outputRoot "$CaptureId.connections.json"
$connectionsPartial = "$connectionsPath.partial"
$transportsPath = Join-Path $outputRoot "$CaptureId.transports.json"
$transportsPartial = "$transportsPath.partial"
$actionsPath = Join-Path $outputRoot "$CaptureId.marker-actions.json"
$actionsPartial = "$actionsPath.partial"
$sessionPath = Join-Path $outputRoot "$CaptureId.marker-session.json"
$sessionPartial = "$sessionPath.partial"
$allArtifactPaths = @($capturePath,$connectionsPartial,$connectionsPath,$transportsPartial,$transportsPath,$actionsPartial,$actionsPath)

$resolvedExecutable = Require-File $GameExecutablePath 'Game executable'
$resolvedBuildManifest = $null
$resolvedDistributionSnapshot = $null
if ($RawCaptureWithUnverifiedProtocolCarryForward) {
    $resolvedDistributionSnapshot = Require-File $DistributionSnapshotPath 'Reviewed Steam distribution snapshot'
    if ([string]::IsNullOrWhiteSpace($ProtocolPackSourceBuild)) { throw 'Raw carry-forward capture requires ProtocolPackSourceBuild.' }
    if (-not [string]::IsNullOrWhiteSpace($BuildFileManifestPath)) { throw 'Raw carry-forward capture uses DistributionSnapshotPath, not BuildFileManifestPath.' }
} else {
    $resolvedBuildManifest = Require-File $BuildFileManifestPath 'Reviewed build/file manifest'
    if (-not [string]::IsNullOrWhiteSpace($DistributionSnapshotPath) -or -not [string]::IsNullOrWhiteSpace($ProtocolPackSourceBuild)) { throw 'DistributionSnapshotPath and ProtocolPackSourceBuild require RawCaptureWithUnverifiedProtocolCarryForward.' }
}
$resolvedPack = Require-File $ProtocolPackPath 'Protocol pack'
$resolvedPlan = Require-File $ActionPlanPath 'Marker action plan'
$initialExecutableHash = Get-Sha256 $resolvedExecutable
$initialPackHash = Get-Sha256 $resolvedPack
$initialPlanHash = Get-Sha256 $resolvedPlan
$initialBuildManifestHash = if ($null -ne $resolvedBuildManifest) { Get-Sha256 $resolvedBuildManifest } else { $null }
$initialDistributionSnapshotHash = if ($null -ne $resolvedDistributionSnapshot) { Get-Sha256 $resolvedDistributionSnapshot } else { $null }
$pack = Get-Content -LiteralPath $resolvedPack -Raw | ConvertFrom-Json
$plan = Get-Content -LiteralPath $resolvedPlan -Raw | ConvertFrom-Json
$buildManifest = if ($null -ne $resolvedBuildManifest) { Get-Content -LiteralPath $resolvedBuildManifest -Raw | ConvertFrom-Json } else { $null }
$distributionSnapshot = if ($null -ne $resolvedDistributionSnapshot) { Get-Content -LiteralPath $resolvedDistributionSnapshot -Raw | ConvertFrom-Json } else { $null }
$planSnapshot = $plan | ConvertTo-Json -Compress -Depth 100 | ConvertFrom-Json

if ($RawCaptureWithUnverifiedProtocolCarryForward) {
    if ($pack.target.build_id -ne $ProtocolPackSourceBuild) { throw "Protocol pack targets source build $($pack.target.build_id), not declared source build $ProtocolPackSourceBuild." }
    if ($ProtocolPackSourceBuild -eq $GameBuild) { throw 'Raw carry-forward capture requires a prior protocol-pack source build distinct from the captured game build.' }
} elseif ($pack.target.build_id -ne $GameBuild) { throw "Protocol pack targets build $($pack.target.build_id), not declared build $GameBuild." }
if ($pack.target.deployment_id -ne 'global' -or $pack.target.channel -ne 'steam') { throw 'Marker capture requires an exact global/steam protocol pack.' }
$computedProtocolPackDigest = Get-ProtocolPackDigest $pack
if ($computedProtocolPackDigest -cne $ProtocolPackDigest) { throw "Protocol pack digest mismatch: selected pack computes to $computedProtocolPackDigest." }

$isTestFixture = $false
if ($TestOnlyAllowIdentityFixture) {
    if (-not $DryRun) { throw 'Test-only identity fixtures are allowed only in dry-run mode.' }
    $identityFixture = if ($RawCaptureWithUnverifiedProtocolCarryForward) { $distributionSnapshot } else { $buildManifest }
    $isTestFixture = $identityFixture.authority.test_only -eq $true -and $identityFixture.game -eq 'capture-harness-test-fixture'
    if (-not $isTestFixture) { throw 'Test-only identity fixture lacks explicit test authority.' }
} elseif ($RawCaptureWithUnverifiedProtocolCarryForward) {
    $reviewedSnapshotPath = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot "..\..\plugins\games\blue-protocol-star-resonance\research\game-file-inventory\global\steam-$GameBuild\steam-distribution-snapshot.v1.json"))
    if (-not $resolvedDistributionSnapshot.Equals($reviewedSnapshotPath, [System.StringComparison]::OrdinalIgnoreCase)) { throw "Production raw capture requires the repository-reviewed Steam distribution snapshot: $reviewedSnapshotPath" }
    if ($distributionSnapshot.schemaVersion -ne 1 -or $distributionSnapshot.game -ne 'blue-protocol-star-resonance' -or $distributionSnapshot.deployment -ne 'global' -or $distributionSnapshot.channel -ne 'steam' -or $distributionSnapshot.app.buildId -ne $GameBuild -or $distributionSnapshot.app.targetBuildId -ne $GameBuild) { throw 'Reviewed Steam distribution snapshot does not match the declared global/steam game build.' }
    if ($distributionSnapshot.authority.steamAppManifest -ne 'installed-distribution-identity' -or @($distributionSnapshot.installedDepots).Count -lt 1) { throw 'Steam distribution snapshot is not authoritative installed-distribution identity.' }
} else {
    $reviewedManifestPath = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot "..\..\plugins\games\blue-protocol-star-resonance\research\game-file-inventory\global\steam-$GameBuild\installed-client-file-manifest.v1.json"))
    if (-not $resolvedBuildManifest.Equals($reviewedManifestPath, [System.StringComparison]::OrdinalIgnoreCase)) { throw "Production capture requires the repository-reviewed build/file manifest: $reviewedManifestPath" }
    if ($buildManifest.schemaVersion -ne 1 -or $buildManifest.game -ne 'blue-protocol-star-resonance' -or $buildManifest.deployment -ne 'global' -or $buildManifest.channel -ne 'steam' -or $buildManifest.gameBuild -ne $GameBuild) { throw 'Reviewed build/file manifest does not match the declared global/steam game build.' }
    if ($buildManifest.coverage.complete -ne $true -or $buildManifest.authority.localPhysicalSha256 -ne 'installed-file-content-proof') { throw 'Build/file manifest is not authoritative, complete installed-file content proof.' }
}
$manifestExecutable = @()
if (-not $RawCaptureWithUnverifiedProtocolCarryForward) {
    $executableName = [System.IO.Path]::GetFileName($resolvedExecutable)
    $manifestExecutable = @($buildManifest.files | Where-Object {[System.IO.Path]::GetFileName(([string]$_.relativePath).Replace('/', '\')) -eq $executableName -and $_.sha256 -eq $initialExecutableHash})
    if ($manifestExecutable.Count -ne 1) { throw 'Game executable hash is not uniquely authorized by the reviewed build/file manifest.' }
    if ([long]$manifestExecutable[0].bytes -ne (Get-Item -LiteralPath $resolvedExecutable).Length) { throw 'Game executable byte length does not match the reviewed build/file manifest.' }
}

if ($planSnapshot.schema_version -ne 1) { throw 'Marker action plan schema_version must be 1.' }
if ($planSnapshot.scene_id -ne $SceneId -or $planSnapshot.scene_name -ne $SceneName) { throw 'Marker action plan scene identity does not match the declared capture scene.' }
$characterIdKnownProperty = $planSnapshot.initiating_character.PSObject.Properties['character_id_known_before_capture']
$characterIdKnown = $null -eq $characterIdKnownProperty -or $characterIdKnownProperty.Value -eq $true
if ($characterIdKnown) {
    if ([string]::IsNullOrWhiteSpace([string]$planSnapshot.initiating_character.character_id)) { throw 'Marker action plan requires initiating_character.character_id when character_id_known_before_capture is true or omitted.' }
} else {
    if ($characterIdKnownProperty.Value -ne $false) { throw 'initiating_character.character_id_known_before_capture must be boolean.' }
    if ($null -ne $planSnapshot.initiating_character.PSObject.Properties['character_id']) { throw 'Marker action plan must omit initiating_character.character_id when it is not known before capture.' }
    if ([string]::IsNullOrWhiteSpace([string]$planSnapshot.initiating_character.character_id_acquisition_note)) { throw 'Marker action plan requires initiating_character.character_id_acquisition_note when the character ID is not known before capture.' }
}
$entityUuidKnownProperty = $planSnapshot.initiating_character.PSObject.Properties['entity_uuid_known_before_capture']
$entityUuidKnown = $null -eq $entityUuidKnownProperty -or $entityUuidKnownProperty.Value -eq $true
if ($entityUuidKnown) {
    if ([string]::IsNullOrWhiteSpace([string]$planSnapshot.initiating_character.entity_uuid)) { throw 'Marker action plan requires initiating_character.entity_uuid when entity_uuid_known_before_capture is true or omitted.' }
} else {
    if ($entityUuidKnownProperty.Value -ne $false) { throw 'initiating_character.entity_uuid_known_before_capture must be boolean.' }
    if ($null -ne $planSnapshot.initiating_character.PSObject.Properties['entity_uuid']) { throw 'Marker action plan must omit initiating_character.entity_uuid when it is not known before capture.' }
    if ([string]::IsNullOrWhiteSpace([string]$planSnapshot.initiating_character.entity_uuid_acquisition_note)) { throw 'Marker action plan requires initiating_character.entity_uuid_acquisition_note when the instance entity UUID is not known before capture.' }
}
$plannedActions = @($planSnapshot.actions)
if ($plannedActions.Count -lt 1 -or $plannedActions.Count -gt 6) { throw 'Marker action plan must contain 1 through 6 actions.' }
$plannedActionCount = $plannedActions.Count
for ($index = 0; $index -lt $plannedActionCount; $index++) {
    $action = $plannedActions[$index]
    $expectedMarker = $index + 1
    if ($action.marker_number -ne $expectedMarker) { throw "Marker action $($index + 1) must declare marker_number $expectedMarker." }
    if ([string]::IsNullOrWhiteSpace([string]$action.expected_action)) { throw "Marker $expectedMarker requires expected_action." }
    if ($action.marker_identity.slot_number -ne $expectedMarker) { throw "Marker $expectedMarker identity must bind slot_number $expectedMarker." }
    if ([string]::IsNullOrWhiteSpace([string]$action.marker_identity.icon_id) -and [string]::IsNullOrWhiteSpace([string]$action.marker_identity.icon_name)) { throw "Marker $expectedMarker identity requires icon_id or icon_name." }
    if ($action.target.kind -eq 'ground') {
        $coordinatesKnownProperty = $action.target.PSObject.Properties['coordinates_known_before_capture']
        $coordinatesProperty = $action.target.PSObject.Properties['coordinates']
        $coordinatesKnown = $null -eq $coordinatesKnownProperty -or $coordinatesKnownProperty.Value -eq $true
        if ($coordinatesKnown) {
            if ($null -eq $coordinatesProperty) { throw "Ground target for marker $expectedMarker requires finite x/y/z coordinates when coordinates_known_before_capture is true or omitted." }
            foreach ($axis in @('x','y','z')) { if (-not (Test-FiniteNumber $coordinatesProperty.Value.$axis)) { throw "Ground target for marker $expectedMarker requires finite x/y/z coordinates when coordinates_known_before_capture is true or omitted." } }
        } else {
            if ($coordinatesKnownProperty.Value -ne $false) { throw "Ground target for marker $expectedMarker coordinates_known_before_capture must be boolean." }
            if ([string]::IsNullOrWhiteSpace([string]$action.target.placement_description)) { throw "Ground target for marker $expectedMarker requires placement_description when coordinates are not known before capture." }
            if ($null -ne $coordinatesProperty) { throw "Ground target for marker $expectedMarker must omit coordinates when coordinates are not known before capture." }
        }
    } elseif ($action.target.kind -eq 'actor') {
        if ([string]::IsNullOrWhiteSpace([string]$action.target.target_id)) { throw "Actor target for marker $expectedMarker requires target_id." }
    } else { throw "Marker $expectedMarker target kind must be ground or actor." }
}

$minimumDuration = $PrePlacementIdleSeconds + (($plannedActionCount - 1) * $MarkerSpacingSeconds) + $PostPlacementIdleSeconds + $PostEngagementSeconds + 20
if ($DurationSeconds -lt $minimumDuration) { throw "DurationSeconds must be at least $minimumDuration for the requested evidence windows." }

$captureLauncher = Join-Path $PSScriptRoot 'capture-client-host.ps1'
$captureArguments = @('-NoProfile','-NonInteractive','-File',$captureLauncher,'-OutputDirectory',$outputRoot,'-CaptureId',$CaptureId,'-ClientIp',$ClientIp,'-Interface',$Interface,'-DurationSeconds',[string]$DurationSeconds,'-CapturePurpose','marker-audit','-TransportMode','all-ip','-DumpcapPath',$DumpcapPath,'-TsharkPath',$TsharkPath)
$hostExecutable = (Get-Process -Id $PID).Path
$captureCommandLine = (($captureArguments | ForEach-Object { ConvertTo-WindowsCommandLineArgument ([string]$_) }) -join ' ')
$protocolAuthority = if ($RawCaptureWithUnverifiedProtocolCarryForward) { [ordered]@{kind='unverified-carry-forward-decoder-hypothesis';source_build=$ProtocolPackSourceBuild;captured_build=$GameBuild;exact_for_captured_build=$false;runtime_authority=$false} } else { [ordered]@{kind='exact-build-protocol-pack';source_build=$GameBuild;captured_build=$GameBuild;exact_for_captured_build=$true;runtime_authority=$true} }
$identity = [ordered]@{game_build=$GameBuild;identity_mode=$(if ($RawCaptureWithUnverifiedProtocolCarryForward) {'raw-capture-unverified-protocol-carry-forward'} else {'exact-build'});executable_path=$resolvedExecutable;executable_version=[System.Diagnostics.FileVersionInfo]::GetVersionInfo($resolvedExecutable).FileVersion;executable_bytes=(Get-Item -LiteralPath $resolvedExecutable).Length;executable_sha256=$initialExecutableHash;build_file_manifest_path=$resolvedBuildManifest;build_file_manifest_sha256=$initialBuildManifestHash;build_file_manifest_entry=$(if ($manifestExecutable.Count -eq 1) {$manifestExecutable[0]} else {$null});distribution_snapshot_path=$resolvedDistributionSnapshot;distribution_snapshot_sha256=$initialDistributionSnapshotHash;protocol_pack_path=$resolvedPack;protocol_pack_id=$pack.pack_id;protocol_pack_target=$pack.target;protocol_pack_file_sha256=$initialPackHash;protocol_pack_digest=$ProtocolPackDigest;protocol_pack_authority=$protocolAuthority;test_only_identity_fixture=$isTestFixture}

if ($DryRun) {
    [ordered]@{schema_version=2;capture_purpose='marker-audit';starts_before_placement=$true;capture_filter="host $ClientIp";transport_mode='all-ip';capture_scope='explicit-client-ipv4-superset-including-process-owned-flow-changes';scene=[ordered]@{id=$SceneId;name=$SceneName};initiating_character=$planSnapshot.initiating_character;identity=$identity;action_plan=[ordered]@{path=$resolvedPlan;sha256=$initialPlanHash;snapshot=$planSnapshot};planned_marker_order=@($plannedActions|ForEach-Object{$_.marker_number});duration_seconds=$DurationSeconds;pre_placement_idle_seconds=$PrePlacementIdleSeconds;marker_spacing_seconds=$MarkerSpacingSeconds;response_window_seconds=$ResponseWindowSeconds;post_placement_idle_seconds=$PostPlacementIdleSeconds;post_engagement_seconds=$PostEngagementSeconds;capture_path=$capturePath;connections_path=$connectionsPath;transports_path=$transportsPath;actions_path=$actionsPath;session_path=$sessionPath;capture_command=[ordered]@{executable=$hostExecutable;arguments=$captureArguments;windows_command_line=$captureCommandLine}} | ConvertTo-Json -Depth 100
    return
}

foreach ($tool in @($DumpcapPath,$TsharkPath,$captureLauncher)) { if (-not (Test-Path -LiteralPath $tool -PathType Leaf)) { throw "Required capture tool was not found: $tool" } }
[System.IO.Directory]::CreateDirectory($outputRoot) | Out-Null
foreach ($path in @($capturePath,$connectionsPath,$transportsPath,$actionsPath,$actionsPartial,$sessionPath,$sessionPartial)) { if (Test-Path -LiteralPath $path) { throw "Refusing to overwrite existing research file: $path" } }

$sessionStarted = [DateTimeOffset]::UtcNow
$baseSession = [ordered]@{schema_version=2;status='capturing';capture_id=$CaptureId;capture_purpose='marker-audit';session_started_utc=$sessionStarted.ToString('o');capture_filter="host $ClientIp";capture_scope='explicit-client-ipv4-superset-including-process-owned-flow-changes';scene=[ordered]@{id=$SceneId;name=$SceneName};initiating_character=$planSnapshot.initiating_character;identity=$identity;action_plan=[ordered]@{path=$resolvedPlan;sha256=$initialPlanHash;snapshot=$planSnapshot}}
Write-Utf8Json $sessionPartial $baseSession
$partialActions = [ordered]@{schema_version=2;status='capturing';capture_id=$CaptureId;scene=[ordered]@{id=$SceneId;name=$SceneName};initiating_character=$planSnapshot.initiating_character;game_build=$GameBuild;protocol_pack_digest=$ProtocolPackDigest;action_plan_sha256=$initialPlanHash;action_plan_snapshot=$planSnapshot;actions=@()}
Write-Utf8Json $actionsPartial $partialActions

$processInfo = New-Object System.Diagnostics.ProcessStartInfo
$processInfo.FileName = $hostExecutable
$processInfo.Arguments = $captureCommandLine
$processInfo.UseShellExecute = $false
$processInfo.CreateNoWindow = $true
$captureProcess = $null
$captureStarted = $false
try {
    $captureProcess = [System.Diagnostics.Process]::Start($processInfo)
    $captureStarted = $true
    $readyDeadline = [DateTimeOffset]::UtcNow.AddSeconds(15)
    while (-not (Test-Path -LiteralPath $capturePath -PathType Leaf)) {
        if ($captureProcess.HasExited) { throw "Capture launcher exited before readiness with code $($captureProcess.ExitCode)." }
        if ([DateTimeOffset]::UtcNow -ge $readyDeadline) { throw 'Capture did not become ready within 15 seconds.' }
        Start-Sleep -Milliseconds 100
    }
    $captureReady = [DateTimeOffset]::UtcNow
    Write-Host "Marker capture is active with exact filter 'host $ClientIp'. Do not place a marker until prompted."
    Start-Sleep -Seconds $PrePlacementIdleSeconds

    $recordedActions = @()
    for ($index = 0; $index -lt $plannedActionCount; $index++) {
        if ($captureProcess.HasExited) { throw "Capture ended before marker $($index + 1) could be recorded." }
        $planned = $plannedActions[$index]
        $targetJson = $planned.target | ConvertTo-Json -Compress -Depth 20
        Read-Host "Prepare marker $($planned.marker_number) ($($planned.marker_identity | ConvertTo-Json -Compress)) for $targetJson; press Enter when ready to place" | Out-Null
        $placementReady = [DateTimeOffset]::UtcNow
        Write-Host 'PLACE THE MARKER NOW through the normal game UI, then press Enter immediately.'
        Read-Host | Out-Null
        $placementCompleted = [DateTimeOffset]::UtcNow
        $visibleResult = Read-Host 'Record the visible in-game UI result (placed/rejected/no-change plus concise detail)'
        $resultRecorded = [DateTimeOffset]::UtcNow
        if ([string]::IsNullOrWhiteSpace($visibleResult)) { throw 'Visible UI result must not be empty.' }
        if ($captureProcess.HasExited) { throw "Capture ended while marker $($index + 1) evidence was being recorded." }
        $recordedActions += [ordered]@{marker_number=$planned.marker_number;marker_identity=$planned.marker_identity;initiating_character=$planSnapshot.initiating_character;expected_action=$planned.expected_action;intended_target=$planned.target;placement_ready_utc=$placementReady.ToString('o');placement_ready_unix_micros=ConvertTo-UnixMicros $placementReady;placement_completed_utc=$placementCompleted.ToString('o');placement_completed_unix_micros=ConvertTo-UnixMicros $placementCompleted;result_recorded_utc=$resultRecorded.ToString('o');result_recorded_unix_micros=ConvertTo-UnixMicros $resultRecorded;visible_ui_result=$visibleResult.Trim()}
        $partialActions.actions = $recordedActions
        Write-Utf8Json $actionsPartial $partialActions
        if ($index -lt ($plannedActionCount - 1)) { Start-Sleep -Seconds $MarkerSpacingSeconds }
    }

    Start-Sleep -Seconds $PostPlacementIdleSeconds
    if ($captureProcess.HasExited) { throw 'Capture ended before boss engagement could be recorded.' }
    Read-Host 'Engage the boss through the normal game UI, then press Enter once engagement is visibly confirmed' | Out-Null
    $engagementTime = [DateTimeOffset]::UtcNow
    if ($captureProcess.HasExited) { throw 'Capture ended before the boss-engagement timestamp was retained.' }
    Start-Sleep -Seconds $PostEngagementSeconds
    if ($captureProcess.HasExited) { throw 'Capture ended before the post-engagement inbound-state window completed.' }
    $captureProcess.WaitForExit()
    if ($captureProcess.ExitCode -ne 0) { throw "Capture launcher exited with code $($captureProcess.ExitCode)." }

    $frozenInputs = @(@($resolvedExecutable,$initialExecutableHash),@($resolvedPack,$initialPackHash),@($resolvedPlan,$initialPlanHash))
    if ($null -ne $resolvedBuildManifest) { $frozenInputs += ,@($resolvedBuildManifest,$initialBuildManifestHash) }
    if ($null -ne $resolvedDistributionSnapshot) { $frozenInputs += ,@($resolvedDistributionSnapshot,$initialDistributionSnapshotHash) }
    foreach ($pair in $frozenInputs) { if ((Get-Sha256 $pair[0]) -cne $pair[1]) { throw "Capture input changed after start: $($pair[0])" } }
    $reparsedPlan = Get-Content -LiteralPath $resolvedPlan -Raw | ConvertFrom-Json | ConvertTo-Json -Compress -Depth 100
    $snapshotJson = $planSnapshot | ConvertTo-Json -Compress -Depth 100
    if ($reparsedPlan -cne $snapshotJson) { throw 'Parsed marker action plan changed after capture start.' }
    for ($index = 0; $index -lt $plannedActionCount; $index++) {
        $current = $recordedActions[$index]
        if ($current.marker_number -ne ($index + 1) -or $current.placement_ready_unix_micros -ge $current.placement_completed_unix_micros -or $current.placement_completed_unix_micros -gt $current.result_recorded_unix_micros) { throw "Marker $($index + 1) timestamp ordering is invalid." }
        if ($index -lt ($plannedActionCount - 1) -and $current.result_recorded_unix_micros -ge $recordedActions[$index + 1].placement_ready_unix_micros) { throw "Marker $($index + 1) overlaps the next placement window." }
    }

    $flowWindows = @()
    for ($index = 0; $index -lt $plannedActionCount; $index++) {
        $action = $recordedActions[$index]
        $startEpoch = $action.placement_ready_unix_micros / 1000000.0
        $actionEndEpoch = $action.placement_completed_unix_micros / 1000000.0
        $endEpoch = $actionEndEpoch + $ResponseWindowSeconds
        if ($index -lt ($plannedActionCount - 1)) { $endEpoch = [Math]::Min($endEpoch, ($recordedActions[$index + 1].placement_ready_unix_micros / 1000000.0) - 0.000001) }
        $filter = "(ip.src==$ClientIp || ip.dst==$ClientIp) && frame.time_epoch >= $($startEpoch.ToString([System.Globalization.CultureInfo]::InvariantCulture)) && frame.time_epoch <= $($endEpoch.ToString([System.Globalization.CultureInfo]::InvariantCulture))"
        $flowCounts = @{}
        & $TsharkPath -r $capturePath -Y $filter -T fields -e frame.time_epoch -e ip.src -e ip.dst -e ip.proto -e tcp.srcport -e tcp.dstport -e udp.srcport -e udp.dstport -E 'separator=|' | ForEach-Object {
            $fields = $_ -split '\|', 8
            if ($fields.Count -lt 8) { return }
            $timestamp = [double]::Parse($fields[0], [System.Globalization.CultureInfo]::InvariantCulture)
            $source=$fields[1];$destination=$fields[2];$protocol=$fields[3]
            if ($protocol -eq '6') {$sourcePort=$fields[4];$destinationPort=$fields[5];$transport='tcp'} elseif ($protocol -eq '17') {$sourcePort=$fields[6];$destinationPort=$fields[7];$transport='udp'} else {$sourcePort='';$destinationPort='';$transport="ip-$protocol"}
            if ($source -eq $ClientIp) {$direction='outbound';$remote=$destination;$clientPort=$sourcePort;$remotePort=$destinationPort} elseif ($destination -eq $ClientIp) {$direction='inbound';$remote=$source;$clientPort=$destinationPort;$remotePort=$sourcePort} else {return}
            $key="$transport|$clientPort|$remote|$remotePort"
            if (-not $flowCounts.ContainsKey($key)) {$flowCounts[$key]=[ordered]@{transport=$transport;client_port=$clientPort;remote_address=$remote;remote_port=$remotePort;outbound_packet_count=0;inbound_packet_count=0;action_outbound_packet_count=0;first_action_outbound_epoch=$null;inbound_after_action_outbound_packet_count=0;first_inbound_after_action_outbound_epoch=$null}}
            $flow = $flowCounts[$key]
            $flow["${direction}_packet_count"]++
            if ($direction -eq 'outbound' -and $timestamp -le $actionEndEpoch) {
                $flow.action_outbound_packet_count++
                if ($null -eq $flow.first_action_outbound_epoch) { $flow.first_action_outbound_epoch = $timestamp }
            } elseif ($direction -eq 'inbound' -and $flow.action_outbound_packet_count -gt 0 -and $timestamp -ge $flow.first_action_outbound_epoch) {
                $flow.inbound_after_action_outbound_packet_count++
                if ($null -eq $flow.first_inbound_after_action_outbound_epoch) { $flow.first_inbound_after_action_outbound_epoch = $timestamp }
            }
        }
        if ($LASTEXITCODE -ne 0) { throw "tshark action-window validation exited with code $LASTEXITCODE" }
        $matchedFlows = @($flowCounts.Values | Where-Object {$_.action_outbound_packet_count -gt 0 -and $_.inbound_after_action_outbound_packet_count -gt 0})
        if ($matchedFlows.Count -eq 0) { throw "Marker $($index + 1) window retained no outbound-during-placement then inbound same-flow IPv4 sequence." }
        $flowWindows += [ordered]@{marker_number=$index + 1;placement_start_epoch_seconds=$startEpoch;placement_completed_epoch_seconds=$actionEndEpoch;response_window_end_epoch_seconds=$endEpoch;ordered_bidirectional_flows=$matchedFlows}
    }

    foreach ($pair in $frozenInputs) { if ((Get-Sha256 $pair[0]) -cne $pair[1]) { throw "Capture input changed before final manifest: $($pair[0])" } }
    if ((Get-Content -LiteralPath $resolvedPlan -Raw | ConvertFrom-Json | ConvertTo-Json -Compress -Depth 100) -cne $snapshotJson) { throw 'Parsed marker action plan changed before final manifest.' }
    $partialActions.status = 'complete'
    $partialActions.action_flow_windows = $flowWindows
    Write-Utf8Json $actionsPath $partialActions
    $actionsHash = Get-Sha256 $actionsPath
    $completeSession = [ordered]@{schema_version=2;status='complete';capture_id=$CaptureId;capture_purpose='marker-audit';session_started_utc=$sessionStarted.ToString('o');capture_ready_utc=$captureReady.ToString('o');boss_engagement_utc=$engagementTime.ToString('o');session_ended_utc=[DateTimeOffset]::UtcNow.ToString('o');capture_filter="host $ClientIp";capture_scope='explicit-client-ipv4-superset-including-process-owned-flow-changes';scene=[ordered]@{id=$SceneId;name=$SceneName};initiating_character=$planSnapshot.initiating_character;identity=$identity;action_plan=[ordered]@{path=$resolvedPlan;sha256=$initialPlanHash;snapshot=$planSnapshot};evidence=[ordered]@{marker_actions=[ordered]@{path=$actionsPath;sha256=$actionsHash;count=$plannedActionCount};bidirectional_action_windows=$flowWindows};artifacts=@([ordered]@{kind='packet_capture';path=$capturePath;sha256=Get-Sha256 $capturePath},[ordered]@{kind='tcp_connection_sidecar';path=$connectionsPath;sha256=Get-Sha256 $connectionsPath},[ordered]@{kind='all_ipv4_transport_inventory';path=$transportsPath;sha256=Get-Sha256 $transportsPath})}
    Write-Utf8Json $sessionPath $completeSession
    Remove-Item -LiteralPath $sessionPartial
    Remove-Item -LiteralPath $actionsPartial
    Write-Host "Private marker capture session manifest: $sessionPath"
} catch {
    $failure = $_.Exception.Message
    if ($captureStarted) { Stop-CaptureProcess $captureProcess }
    if ($captureStarted) {
        $failedSession = [ordered]@{schema_version=2;status='failed';capture_id=$CaptureId;capture_purpose='marker-audit';session_started_utc=$sessionStarted.ToString('o');failed_utc=[DateTimeOffset]::UtcNow.ToString('o');failure_reason=$failure;scene=[ordered]@{id=$SceneId;name=$SceneName};initiating_character=$planSnapshot.initiating_character;identity=$identity;action_plan=[ordered]@{path=$resolvedPlan;sha256=$initialPlanHash;snapshot=$planSnapshot};available_artifacts=Get-AvailableArtifactHashes $allArtifactPaths}
        Write-Utf8Json $sessionPartial $failedSession
        if (Test-Path -LiteralPath $actionsPartial -PathType Leaf) {
            $partialActions.status = 'failed'
            $partialActions.failure_reason = $failure
            $partialActions.failed_utc = [DateTimeOffset]::UtcNow.ToString('o')
            Write-Utf8Json $actionsPartial $partialActions
        }
    }
    throw
} finally {
    if ($null -ne $captureProcess) { $captureProcess.Dispose() }
}
