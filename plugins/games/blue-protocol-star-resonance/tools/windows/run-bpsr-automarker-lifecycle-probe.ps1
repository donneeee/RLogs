[CmdletBinding()]
param(
    [string]$InstallRoot,
    [string]$SteamManifest,
    [ValidateRange(100, 60000)][int]$DurationMs = 15000,
    [ValidateRange(5, 1000)][int]$IntervalMs = 10,
    [switch]$ArmReversibleCalibration,
    [switch]$ArmSinglePlannerStep,
    [switch]$ArmClosedLoopAim,
    [switch]$ArmOperatorPlacement,
    [Nullable[double]]$TargetX,
    [Nullable[double]]$TargetY,
    [Nullable[double]]$TargetZ,
    [string]$PresetId,
    [string]$PresetName,
    [switch]$ListPresets,
    [string]$RLogsBaseUrl,
    [switch]$SelfTest,
    [switch]$DryRun
)

$ErrorActionPreference = 'Stop'
$expectedBuild = '25247556'
$expectedAppId = '3681810'
$probe = Join-Path $PSScriptRoot 'rlogs-bpsr-automarker-lifecycle-probe.exe'
function Get-AcfValue([string]$Text, [string]$Key) {
    $match = [regex]::Match($Text, '(?im)^\s*"' + [regex]::Escape($Key) + '"\s+"([^"]*)"')
    if ($match.Success) { return $match.Groups[1].Value }
    return $null
}

function Assert-LoopbackBaseUrl([string]$BaseUrl) {
    $match = [regex]::Match($BaseUrl, '^http://127\.0\.0\.1:([0-9]{1,5})$')
    if (-not $match.Success) {
        throw '-RLogsBaseUrl must be an HTTP loopback URL such as http://127.0.0.1:54221.'
    }
    $port = [int]$match.Groups[1].Value
    if ($port -lt 1 -or $port -gt 65535) { throw '-RLogsBaseUrl contains an invalid port.' }
}

function Test-ExactPropertySet($Value, [string[]]$Expected) {
    if ($null -eq $Value) { return $false }
    $actualNames = @($Value.PSObject.Properties.Name | Sort-Object)
    $expectedNames = @($Expected | Sort-Object)
    return (($actualNames -join "`n") -ceq ($expectedNames -join "`n"))
}

function Test-FiniteJsonNumber($Value) {
    if ($Value -isnot [byte] -and $Value -isnot [sbyte] -and
        $Value -isnot [int16] -and $Value -isnot [uint16] -and
        $Value -isnot [int32] -and $Value -isnot [uint32] -and
        $Value -isnot [int64] -and $Value -isnot [uint64] -and
        $Value -isnot [single] -and $Value -isnot [double] -and $Value -isnot [decimal]) {
        return $false
    }
    $number = [double]$Value
    return (-not [double]::IsNaN($number) -and -not [double]::IsInfinity($number))
}

function Assert-PresetProjectionSchema($Projection) {
    $topLevel = @(
        'schemaVersion', 'context', 'presets', 'captureSupported', 'captureReason',
        'captureSessionId', 'deploymentId', 'protocolPackDigest', 'nativeLoadSupported',
        'nativeLoadReason', 'previewSessionId'
    )
    if (-not (Test-ExactPropertySet $Projection $topLevel) -or $Projection.schemaVersion -ne 4 -or
        $Projection.presets -isnot [array] -or $Projection.captureSupported -isnot [bool] -or
        @('native_waymark_state_unverified', 'observed_waymark_state_verified') -cnotcontains [string]$Projection.captureReason -or
        $Projection.captureSupported -ne ([string]$Projection.captureReason -ceq 'observed_waymark_state_verified') -or
        $Projection.nativeLoadSupported -isnot [bool] -or $Projection.nativeLoadSupported -or
        [string]$Projection.nativeLoadReason -cne 'native_waymark_transport_unavailable' -or
        [string]::IsNullOrWhiteSpace([string]$Projection.previewSessionId) -or
        ([string]$Projection.previewSessionId).Length -lt 8 -or ([string]$Projection.previewSessionId).Length -gt 128) {
        throw 'The loopback endpoint did not return the expected rLogs automarker presets schema.'
    }
    foreach ($preset in @($Projection.presets)) {
        if (-not (Test-ExactPropertySet $preset @('presetId', 'name', 'activityFamilyId', 'savedAtUnixMillis', 'points'))) {
            throw 'The loopback endpoint returned an unexpected automarker preset shape.'
        }
        if ([string]::IsNullOrWhiteSpace([string]$preset.presetId) -or ([string]$preset.presetId).Length -lt 8 -or
            [string]::IsNullOrWhiteSpace([string]$preset.name) -or ([string]$preset.name).Length -gt 80 -or
            [string]::IsNullOrWhiteSpace([string]$preset.activityFamilyId) -or
            -not (Test-FiniteJsonNumber $preset.savedAtUnixMillis) -or
            [double]$preset.savedAtUnixMillis % 1 -ne 0 -or $preset.points -isnot [array] -or
            @($preset.points).Count -lt 1 -or @($preset.points).Count -gt 6) {
            throw 'The loopback endpoint returned invalid automarker preset values.'
        }
        $markerNumbers = @()
        foreach ($point in @($preset.points)) {
            if (-not (Test-ExactPropertySet $point @('markerNumber', 'x', 'y', 'z')) -or
                -not (Test-FiniteJsonNumber $point.markerNumber) -or [double]$point.markerNumber % 1 -ne 0 -or
                [int]$point.markerNumber -lt 1 -or [int]$point.markerNumber -gt 6 -or
                -not (Test-FiniteJsonNumber $point.x) -or -not (Test-FiniteJsonNumber $point.y) -or
                -not (Test-FiniteJsonNumber $point.z)) {
                throw 'The loopback endpoint returned an unexpected automarker point shape.'
            }
            $markerNumbers += [int]$point.markerNumber
        }
        if (@($markerNumbers | Select-Object -Unique).Count -ne $markerNumbers.Count) {
            throw 'The loopback endpoint returned duplicate automarker marker numbers.'
        }
    }
}

function Get-LoopbackPresetProjection([string]$BaseUrl) {
    Assert-LoopbackBaseUrl $BaseUrl
    $handler = [Net.Http.HttpClientHandler]::new()
    $handler.AllowAutoRedirect = $false
    $handler.UseProxy = $false
    $client = [Net.Http.HttpClient]::new($handler)
    $client.Timeout = [TimeSpan]::FromSeconds(2)
    $response = $null
    try {
        try {
            $response = $client.GetAsync("$BaseUrl/api/automarkers/presets").GetAwaiter().GetResult()
        } catch {
            throw 'The local rLogs presets endpoint did not respond within the safety window.'
        }
        if ([int]$response.StatusCode -ne 200) { throw "rLogs presets endpoint returned HTTP $([int]$response.StatusCode)." }
        $body = $response.Content.ReadAsStringAsync().GetAwaiter().GetResult()
        if ([Text.Encoding]::UTF8.GetByteCount($body) -gt 524288) { throw 'rLogs presets response exceeded the 512 KiB safety limit.' }
        $projection = $body | ConvertFrom-Json
        Assert-PresetProjectionSchema $projection
        return $projection
    } finally {
        if ($null -ne $response) { $response.Dispose() }
        $client.Dispose()
        $handler.Dispose()
    }
}

function Find-RLogsLoopbackBaseUrl {
    try {
        $listeners = @(Get-NetTCPConnection -State Listen -ErrorAction Stop |
            Where-Object { $_.LocalAddress -eq '127.0.0.1' } |
            Select-Object -Property LocalPort, OwningProcess -Unique)
    } catch {
        throw 'Could not enumerate loopback listeners. Supply -RLogsBaseUrl http://127.0.0.1:<port>.'
    }
    $matches = @()
    foreach ($listener in $listeners) {
        $owner = Get-Process -Id $listener.OwningProcess -ErrorAction SilentlyContinue
        if ($null -eq $owner) { continue }
        $ownerName = $owner.ProcessName
        try {
            if ($owner.Path) { $ownerName = [IO.Path]::GetFileNameWithoutExtension($owner.Path) }
        } catch {
            # ProcessName still comes from the owning PID even when Windows
            # denies querying the executable path across an integrity boundary.
        }
        if ($ownerName -ine 'rlogs-app') { continue }
        $candidate = "http://127.0.0.1:$($listener.LocalPort)"
        try {
            [void](Get-LoopbackPresetProjection $candidate)
            $matches += $candidate
        } catch { continue }
    }
    if ($matches.Count -ne 1) {
        throw "Expected exactly one valid rLogs loopback host but found $($matches.Count). Supply -RLogsBaseUrl http://127.0.0.1:<port>."
    }
    return $matches[0]
}

function Get-ExactActivePresetContext($Projection) {
    Assert-PresetProjectionSchema $Projection
    if ($null -eq $Projection.context -or
        -not (Test-ExactPropertySet $Projection.context @('clientBuild', 'sceneId', 'mapId', 'activityFamilyId', 'sceneName'))) {
        throw 'rLogs does not have an exact active automarker context.'
    }
    $context = $Projection.context
    if ([string]$context.clientBuild -cne $expectedBuild -or
        -not (Test-FiniteJsonNumber $context.sceneId) -or
        -not (Test-FiniteJsonNumber $context.mapId) -or
        [double]$context.sceneId % 1 -ne 0 -or [double]$context.mapId % 1 -ne 0 -or
        [string]::IsNullOrWhiteSpace([string]$context.activityFamilyId)) {
        throw 'The active rLogs build/scene/map/family context is incomplete or does not match this canary.'
    }
    if (@($Projection.presets | Where-Object { [string]$_.activityFamilyId -cne [string]$context.activityFamilyId }).Count -ne 0) {
        throw 'The preset projection contains a preset outside the exact active activity family.'
    }
    return $context
}

function Resolve-MarkerOnePresetTarget($Projection, [string]$RequestedPresetId, [string]$RequestedPresetName) {
    $context = Get-ExactActivePresetContext $Projection
    if (-not [string]::IsNullOrWhiteSpace($RequestedPresetId)) {
        $selected = @($Projection.presets | Where-Object { [string]$_.presetId -ceq $RequestedPresetId })
        $selector = "ID '$([regex]::Replace($RequestedPresetId, '\p{C}', '?'))'"
    } else {
        $selected = @($Projection.presets | Where-Object { [string]$_.name -ceq $RequestedPresetName })
        $selector = "name '$([regex]::Replace($RequestedPresetName, '\p{C}', '?'))'"
    }
    if ($selected.Count -ne 1) { throw "Preset $selector matched $($selected.Count) presets; exactly one is required." }
    $preset = $selected[0]
    if ([string]$preset.activityFamilyId -cne [string]$context.activityFamilyId) {
        throw "Preset $selector does not belong to the exact active activity family."
    }
    $markerOne = @($preset.points | Where-Object { $_.markerNumber -eq 1 })
    if ($markerOne.Count -ne 1) { throw "Preset $selector must contain exactly one Marker 1 point." }
    $point = $markerOne[0]
    foreach ($coordinate in @($point.x, $point.y, $point.z)) {
        if (-not (Test-FiniteJsonNumber $coordinate)) { throw "Preset $selector has a non-finite Marker 1 coordinate." }
    }
    return [pscustomobject]@{
        X = [double]$point.x
        Y = [double]$point.y
        Z = [double]$point.z
        PresetId = [string]$preset.presetId
        PresetName = [string]$preset.name
    }
}

function Write-SanitizedPresetList($Projection) {
    $context = Get-ExactActivePresetContext $Projection
    $rows = @($Projection.presets | Sort-Object -Property name, presetId | ForEach-Object {
        [pscustomobject]@{
            Name = ([regex]::Replace([string]$_.name, '\p{C}', '?'))
            PresetId = ([regex]::Replace([string]$_.presetId, '\p{C}', '?'))
            Family = ([regex]::Replace([string]$context.activityFamilyId, '\p{C}', '?'))
            Markers = (@($_.points.markerNumber | Sort-Object) -join ',')
        }
    })
    if ($rows.Count -eq 0) {
        Write-Host 'No presets exist for the exact active activity family.'
        return
    }
    $rows | Format-Table -Property Name, PresetId, Family, Markers -AutoSize
}

function Assert-LauncherTargetSelection(
    [bool]$Calibration,
    [bool]$OneStep,
    [bool]$ClosedLoop,
    [bool]$OperatorPlacement,
    [string]$RequestedPresetId,
    [string]$RequestedPresetName,
    [bool]$ListingPresets,
    [Nullable[double]]$X,
    [Nullable[double]]$Y,
    [Nullable[double]]$Z
) {
    if (@($Calibration, $OneStep, $ClosedLoop, $OperatorPlacement).Where({ $_ }).Count -gt 1) {
        throw 'Choose only one armed canary mode.'
    }
    $hasPreset = -not [string]::IsNullOrWhiteSpace($RequestedPresetId)
    $hasPresetName = -not [string]::IsNullOrWhiteSpace($RequestedPresetName)
    $hasAnyCoordinate = ($null -ne $X -or $null -ne $Y -or $null -ne $Z)
    $hasAllCoordinates = ($null -ne $X -and $null -ne $Y -and $null -ne $Z)
    if (($hasPreset -or $hasPresetName) -and -not ($ClosedLoop -or $OperatorPlacement)) {
        throw '-PresetId and -PresetName require -ArmClosedLoopAim or -ArmOperatorPlacement.'
    }
    if (@($hasPreset, $hasPresetName, $hasAnyCoordinate).Where({ $_ }).Count -gt 1) {
        throw '-PresetId, -PresetName, and explicit -TargetX/-TargetY/-TargetZ are mutually exclusive.'
    }
    if (($OneStep -or $ClosedLoop) -and -not $hasPreset -and -not $hasPresetName -and -not $hasAllCoordinates) {
        throw 'Supply -PresetId, -PresetName (closed-loop only), or all of -TargetX/-TargetY/-TargetZ.'
    }
    if ($hasAnyCoordinate -and -not $hasAllCoordinates) {
        throw '-TargetX, -TargetY, and -TargetZ must be supplied together.'
    }
    if ($OperatorPlacement -and -not ($hasPreset -or $hasPresetName)) {
        throw '-ArmOperatorPlacement requires exactly one of -PresetId or -PresetName; explicit XYZ is not accepted.'
    }
    if (-not ($OneStep -or $ClosedLoop -or $OperatorPlacement) -and ($hasPreset -or $hasPresetName -or $hasAnyCoordinate)) {
        throw 'Target coordinates and presets require an armed planner mode.'
    }
    if ($ListingPresets -and ($Calibration -or $OneStep -or $ClosedLoop -or $OperatorPlacement -or $hasPreset -or $hasPresetName -or $hasAnyCoordinate)) {
        throw '-ListPresets cannot be combined with an armed mode or a target selector.'
    }
}

function Invoke-LauncherSelfTest {
    Assert-LoopbackBaseUrl 'http://127.0.0.1:54221'
    $rejected = $false
    try { Assert-LoopbackBaseUrl 'http://localhost:54221' } catch { $rejected = $true }
    if (-not $rejected) { throw 'Self-test failed: non-literal loopback host was accepted.' }
    $json = '{"schemaVersion":4,"context":{"clientBuild":"25247556","sceneId":100,"mapId":200,"activityFamilyId":"tina","sceneName":"Tina"},"presets":[{"presetId":"preset-test","name":"Test","activityFamilyId":"tina","savedAtUnixMillis":1,"points":[{"markerNumber":1,"x":1.5,"y":2.5,"z":3.5}]}],"captureSupported":false,"captureReason":"native_waymark_state_unverified","captureSessionId":null,"deploymentId":null,"protocolPackDigest":null,"nativeLoadSupported":false,"nativeLoadReason":"native_waymark_transport_unavailable","previewSessionId":"preview-test"}' | ConvertFrom-Json
    $target = Resolve-MarkerOnePresetTarget $json 'preset-test' $null
    if ($target.X -ne 1.5 -or $target.Y -ne 2.5 -or $target.Z -ne 3.5) { throw 'Self-test failed: target coordinates changed.' }
    $namedTarget = Resolve-MarkerOnePresetTarget $json $null 'Test'
    if ($namedTarget.PresetId -cne 'preset-test') { throw 'Self-test failed: unique exact preset name resolved the wrong ID.' }
    $listOutput = Write-SanitizedPresetList $json | Out-String
    foreach ($required in @('Test', 'preset-test', 'tina', '1')) {
        if ($listOutput -notmatch [regex]::Escape($required)) { throw "Self-test failed: sanitized list omitted '$required'." }
    }
    foreach ($forbidden in @('1.5', '2.5', '3.5', 'captureSessionId', 'deploymentId')) {
        if ($listOutput -match [regex]::Escape($forbidden)) { throw "Self-test failed: sanitized list exposed '$forbidden'." }
    }
    $rejected = $false
    try { [void](Resolve-MarkerOnePresetTarget $json $null 'test') } catch { $rejected = $true }
    if (-not $rejected) { throw 'Self-test failed: preset-name matching was not exact/case-sensitive.' }
    $json.presets[0].points += [pscustomobject]@{ markerNumber = 1; x = 4; y = 5; z = 6 }
    $rejected = $false
    try { [void](Resolve-MarkerOnePresetTarget $json 'preset-test' $null) } catch { $rejected = $true }
    if (-not $rejected) { throw 'Self-test failed: duplicate Marker 1 was accepted.' }
    $familyJson = '{"schemaVersion":4,"context":{"clientBuild":"25247556","sceneId":100,"mapId":200,"activityFamilyId":"other","sceneName":"Other"},"presets":[{"presetId":"preset-test","name":"Test","activityFamilyId":"tina","savedAtUnixMillis":1,"points":[{"markerNumber":1,"x":1.5,"y":2.5,"z":3.5}]}],"captureSupported":false,"captureReason":"native_waymark_state_unverified","captureSessionId":null,"deploymentId":null,"protocolPackDigest":null,"nativeLoadSupported":false,"nativeLoadReason":"native_waymark_transport_unavailable","previewSessionId":"preview-test"}' | ConvertFrom-Json
    $rejected = $false
    try { [void](Resolve-MarkerOnePresetTarget $familyJson 'preset-test' $null) } catch { $rejected = $true }
    if (-not $rejected) { throw 'Self-test failed: mismatched activity family was accepted.' }
    $nameJson = '{"schemaVersion":4,"context":{"clientBuild":"25247556","sceneId":100,"mapId":200,"activityFamilyId":"tina","sceneName":"Tina"},"presets":[{"presetId":"preset-one","name":"Same Name","activityFamilyId":"tina","savedAtUnixMillis":1,"points":[{"markerNumber":1,"x":1.5,"y":2.5,"z":3.5}]},{"presetId":"preset-two","name":"Same Name","activityFamilyId":"tina","savedAtUnixMillis":2,"points":[{"markerNumber":1,"x":4.5,"y":5.5,"z":6.5}]}],"captureSupported":false,"captureReason":"native_waymark_state_unverified","captureSessionId":null,"deploymentId":null,"protocolPackDigest":null,"nativeLoadSupported":false,"nativeLoadReason":"native_waymark_transport_unavailable","previewSessionId":"preview-test"}' | ConvertFrom-Json
    $rejected = $false
    try { [void](Resolve-MarkerOnePresetTarget $nameJson $null 'Same Name') } catch { $rejected = $true }
    if (-not $rejected) { throw 'Self-test failed: ambiguous exact preset name was accepted.' }
    $rejected = $false
    try { Assert-LauncherTargetSelection $false $false $true $false 'preset-test' $null $false 1 2 3 } catch { $rejected = $true }
    if (-not $rejected) { throw 'Self-test failed: preset and explicit XYZ were accepted together.' }
    Assert-LauncherTargetSelection $false $false $true $false 'preset-test' $null $false $null $null $null
    Assert-LauncherTargetSelection $false $false $true $false $null 'Test' $false $null $null $null
    Assert-LauncherTargetSelection $false $false $true $false $null $null $false 1 2 3
    Assert-LauncherTargetSelection $false $false $false $false $null $null $true $null $null $null
    Assert-LauncherTargetSelection $false $false $false $true 'preset-test' $null $false $null $null $null
    $rejected = $false
    try { Assert-LauncherTargetSelection $false $false $false $true $null $null $false 1 2 3 } catch { $rejected = $true }
    if (-not $rejected) { throw 'Self-test failed: operator placement accepted raw XYZ without a preset.' }
    Write-Host 'Launcher self-test passed: loopback policy, schema, family context, ID/name uniqueness, and target exclusivity.'
}

if ($SelfTest) {
    if ($DryRun -or $ListPresets -or $ArmReversibleCalibration -or $ArmSinglePlannerStep -or $ArmClosedLoopAim -or $ArmOperatorPlacement) {
        throw '-SelfTest cannot be combined with dry-run or armed modes.'
    }
    Invoke-LauncherSelfTest
    return
}

if ($DryRun) {
    if ($ListPresets -or $ArmReversibleCalibration -or $ArmSinglePlannerStep -or $ArmClosedLoopAim -or $ArmOperatorPlacement) {
        throw '-DryRun cannot be combined with an armed mode.'
    }
    if (-not (Test-Path -LiteralPath $probe -PathType Leaf)) {
        throw 'The probe executable is missing from this package.'
    }
    $dryStamp = [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssfffZ')
    $dryReceipt = Join-Path $PSScriptRoot "automarker-dry-run-$dryStamp.v1.json"
    $value = [ordered]@{
        schemaVersion = 1
        evidenceKind = 'automarker-v13-packaged-dry-run'
        exactBuild = $expectedBuild
        executablePresent = $true
        processOpened = $false
        processMemoryRead = $false
        inputEmitted = $false
        clickOrPlacementAttempted = $false
        outcome = 'passed'
    } | ConvertTo-Json
    [IO.File]::WriteAllText($dryReceipt, $value + [Environment]::NewLine, [Text.UTF8Encoding]::new($false))
    Write-Host "Created sanitized dry-run receipt $(Split-Path -Leaf $dryReceipt)."
    return
}

Assert-LauncherTargetSelection $ArmReversibleCalibration $ArmSinglePlannerStep $ArmClosedLoopAim $ArmOperatorPlacement $PresetId $PresetName $ListPresets $TargetX $TargetY $TargetZ

if ($ListPresets) {
    if ([string]::IsNullOrWhiteSpace($RLogsBaseUrl)) { $RLogsBaseUrl = Find-RLogsLoopbackBaseUrl }
    else { Assert-LoopbackBaseUrl $RLogsBaseUrl }
    Write-SanitizedPresetList (Get-LoopbackPresetProjection $RLogsBaseUrl)
    return
}

if (-not (Test-Path -LiteralPath $probe -PathType Leaf)) {
    throw 'The probe executable is missing from this package.'
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
        if ((Get-AcfValue $text 'appid') -ne $expectedAppId) { continue }
        if ((Get-AcfValue $text 'buildid') -ne $expectedBuild) { continue }
        $installDir = Get-AcfValue $text 'installdir'
        if (-not $installDir) { continue }
        $matches += [pscustomobject]@{
            Manifest = $manifest
            InstallRoot = Join-Path $root "steamapps\common\$installDir\bpsr"
        }
    }
    if ($matches.Count -ne 1) {
        throw 'Exact-build Steam auto-discovery did not produce one unique installation. Supply -InstallRoot and -SteamManifest explicitly.'
    }
    return $matches[0]
}

if ([string]::IsNullOrWhiteSpace($InstallRoot) -xor [string]::IsNullOrWhiteSpace($SteamManifest)) {
    throw '-InstallRoot and -SteamManifest must be supplied together, or both omitted for auto-discovery.'
}
if ([string]::IsNullOrWhiteSpace($InstallRoot)) {
    $install = Find-ExactInstall
    $InstallRoot = $install.InstallRoot
    $SteamManifest = $install.Manifest
}

$stamp = [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssfffZ')
$receipt = Join-Path $PSScriptRoot "automarker-lifecycle-$stamp.v7.json"
if (Test-Path -LiteralPath $receipt) { throw 'Refusing to overwrite an existing receipt.' }

$arguments = @(
    '--build', $expectedBuild,
    '--process-executable', (Join-Path $InstallRoot 'BPSR_STEAM.exe'),
    '--game-assembly', (Join-Path $InstallRoot 'GameAssembly.dll'),
    '--steam-manifest', $SteamManifest,
    '--duration-ms', $DurationMs,
    '--interval-ms', $IntervalMs,
    '--output', $receipt
)
if ($ArmReversibleCalibration) {
    Write-Warning 'ARMED CALIBRATION: manually select Marker 1 and keep the game focused. This emits +X, -X, +Y, -Y six-pixel mouse moves and Escape. It never clicks.'
    Write-Host 'Return focus to the game now. The fail-closed canary starts in 5 seconds.'
    foreach ($remaining in 5..1) {
        Write-Host "$remaining..."
        Start-Sleep -Seconds 1
    }
    $arguments += @('--armed-mode', 'marker1-reversible-calibration-v1')
}
if ($ArmSinglePlannerStep -or $ArmClosedLoopAim -or $ArmOperatorPlacement) {
    $hasPreset = -not [string]::IsNullOrWhiteSpace($PresetId)
    $hasPresetName = -not [string]::IsNullOrWhiteSpace($PresetName)
    if ([string]::IsNullOrWhiteSpace($RLogsBaseUrl)) {
        $RLogsBaseUrl = Find-RLogsLoopbackBaseUrl
        Write-Host "Resolved the unique rLogs loopback host at $RLogsBaseUrl."
    } else {
        Assert-LoopbackBaseUrl $RLogsBaseUrl
    }
    if ($hasPreset -or $hasPresetName) {
        $projection = Get-LoopbackPresetProjection $RLogsBaseUrl
        $resolved = Resolve-MarkerOnePresetTarget $projection $PresetId $PresetName
        $TargetX = [Nullable[double]]$resolved.X
        $TargetY = [Nullable[double]]$resolved.Y
        $TargetZ = [Nullable[double]]$resolved.Z
        $resolvedName = [regex]::Replace($resolved.PresetName, '\p{C}', '?')
        $resolvedId = [regex]::Replace($resolved.PresetId, '\p{C}', '?')
        Write-Host "Resolved Marker 1 from preset '$resolvedName' ($resolvedId) in the exact active activity family."
    }
    foreach ($coordinate in @($TargetX.Value, $TargetY.Value, $TargetZ.Value)) {
        if ([double]::IsNaN($coordinate) -or [double]::IsInfinity($coordinate)) {
            throw 'Planner target coordinates must be finite.'
        }
    }
    if ($ArmOperatorPlacement) {
        Write-Warning 'ARMED OPERATOR PLACEMENT EVIDENCE: manually select Marker 1 and keep the game focused. The canary aims without clicking, then asks you to click once. Do not move the mouse. It requires newer outbound and authoritative inbound Marker 1 evidence.'
    } elseif ($ArmClosedLoopAim) {
        Write-Warning 'ARMED CLOSED-LOOP CANARY: manually select Marker 1 and keep the game focused. This calibrates, makes at most four <=4-pixel moves (<=16 cumulative), reverses every move, then Escape. Do not touch the mouse. It never clicks or places.'
    } else {
        Write-Warning 'ARMED ONE-STEP CANARY: manually select Marker 1 and keep the game focused. This calibrates, moves at most 4 pixels once, applies the exact inverse, then Escape. It never clicks or places.'
    }
    Write-Host 'Return focus to the game now. The fail-closed canary starts in 5 seconds.'
    foreach ($remaining in 5..1) {
        Write-Host "$remaining..."
        Start-Sleep -Seconds 1
    }
    $armedToken = if ($ArmOperatorPlacement) {
        'marker1-operator-click-placement-evidence-v1'
    } elseif ($ArmClosedLoopAim) {
        'marker1-closed-loop-aim-and-rollback-v1'
    } else {
        'marker1-single-planner-step-and-restore-v1'
    }
    $arguments += @(
        '--armed-mode', $armedToken,
        '--target-x', $TargetX.Value.ToString('R', [Globalization.CultureInfo]::InvariantCulture),
        '--target-y', $TargetY.Value.ToString('R', [Globalization.CultureInfo]::InvariantCulture),
        '--target-z', $TargetZ.Value.ToString('R', [Globalization.CultureInfo]::InvariantCulture),
        '--rlogs-base-url', $RLogsBaseUrl
    )
}

& $probe @arguments
if ($LASTEXITCODE -ne 0) { throw 'The exact-build probe or calibration canary failed closed.' }
Write-Host "Created sanitized receipt $(Split-Path -Leaf $receipt). Marker confirmation and Place remain disabled."
