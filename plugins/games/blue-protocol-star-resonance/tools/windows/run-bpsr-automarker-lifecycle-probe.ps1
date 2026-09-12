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

function Initialize-SystemNetHttp {
    try {
        Add-Type -AssemblyName System.Net.Http -ErrorAction Stop
        if ($null -eq ('System.Net.Http.HttpClientHandler' -as [type]) -or
            $null -eq ('System.Net.Http.HttpClient' -as [type])) {
            throw 'required HTTP client types were not registered'
        }
    } catch {
        throw 'The required System.Net.Http runtime could not be loaded. Install or repair .NET Framework 4.7.2 or newer, then retry.'
    }
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

function ConvertTo-InvariantFiniteCoordinate($Value, [string]$Label) {
    if ($null -eq $Value) { throw "Planner target coordinate $Label is missing." }
    try {
        $number = [double]$Value
    } catch {
        throw "Planner target coordinate $Label is not numeric."
    }
    if ([double]::IsNaN($number) -or [double]::IsInfinity($number)) {
        throw "Planner target coordinate $Label must be finite."
    }
    return $number.ToString('R', [Globalization.CultureInfo]::InvariantCulture)
}

function New-PlannerTargetArguments($X, $Y, $Z, [string]$BaseUrl) {
    Assert-LoopbackBaseUrl $BaseUrl
    return @(
        '--target-x', (ConvertTo-InvariantFiniteCoordinate $X 'X'),
        '--target-y', (ConvertTo-InvariantFiniteCoordinate $Y 'Y'),
        '--target-z', (ConvertTo-InvariantFiniteCoordinate $Z 'Z'),
        '--rlogs-base-url', $BaseUrl
    )
}

function Get-ArmedPreparationCountdownSeconds([bool]$OperatorPlacement) {
    if ($OperatorPlacement) { return 10 }
    return 5
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
    Initialize-SystemNetHttp
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

function Get-LoopbackEndpointRejectionCategory($Failure) {
    $message = [string]$Failure.Exception.Message
    if ($message -like 'The required System.Net.Http runtime could not be loaded.*' -or
        $message -match '^Unable to find type \[Net\.Http\.') { return 'http-runtime-unavailable' }
    if ($message -like 'The local rLogs presets endpoint did not respond*') { return 'connect-or-timeout' }
    if ($message -like 'rLogs presets endpoint returned HTTP *') { return 'http-status' }
    if ($message -like 'rLogs presets response exceeded *') { return 'response-too-large' }
    if ($message -like 'The loopback endpoint returned *' -or
        $message -like 'rLogs does not have an exact active *') { return 'schema-rejected' }
    return 'unexpected'
}

function Test-RLogsDesktopOwnerName([string]$Name) {
    if ([string]::IsNullOrWhiteSpace($Name)) { return $false }
    $baseName = [IO.Path]::GetFileNameWithoutExtension($Name)
    return $baseName -ieq 'rlogs-app' -or $baseName -ieq 'rlogs'
}

function Find-RLogsLoopbackBaseUrl {
    # Load this before listener enumeration so Windows PowerShell 5.1 reports a
    # missing framework runtime directly instead of reducing it to zero matches.
    Initialize-SystemNetHttp
    try {
        $listeners = @(Get-NetTCPConnection -State Listen -ErrorAction Stop |
            Where-Object { $_.LocalAddress -eq '127.0.0.1' } |
            Select-Object -Property LocalPort, OwningProcess -Unique)
    } catch {
        throw 'Could not enumerate loopback listeners. Supply -RLogsBaseUrl http://127.0.0.1:<port>.'
    }
    $matches = @()
    $ownedListenerCount = 0
    $unresolvedOwnerCount = 0
    $rejectedEndpointCount = 0
    $rejectionCategories = @{}
    foreach ($listener in $listeners) {
        $owner = Get-Process -Id $listener.OwningProcess -ErrorAction SilentlyContinue
        if ($null -eq $owner) {
            $unresolvedOwnerCount += 1
            continue
        }
        $ownerNames = @([string]$owner.ProcessName)
        try {
            if ($owner.Path) { $ownerNames += [IO.Path]::GetFileNameWithoutExtension($owner.Path) }
        } catch {
            # ProcessName still comes from the owning PID even when Windows
            # denies querying the executable path across an integrity boundary.
        }
        if (@($ownerNames | Where-Object { Test-RLogsDesktopOwnerName $_ }).Count -eq 0) { continue }
        $ownedListenerCount += 1
        $candidate = "http://127.0.0.1:$($listener.LocalPort)"
        try {
            [void](Get-LoopbackPresetProjection $candidate)
            $matches += $candidate
        } catch {
            $rejectedEndpointCount += 1
            $category = Get-LoopbackEndpointRejectionCategory $_
            if (-not $rejectionCategories.ContainsKey($category)) { $rejectionCategories[$category] = 0 }
            $rejectionCategories[$category] += 1
            continue
        }
    }
    if ($matches.Count -ne 1) {
        $categorySummary = if ($rejectionCategories.Count -eq 0) {
            'none'
        } else {
            (@($rejectionCategories.Keys | Sort-Object | ForEach-Object { "$_=$($rejectionCategories[$_])" }) -join ',')
        }
        throw "Expected exactly one valid rLogs loopback host but found $($matches.Count). Discovery inspected $($listeners.Count) literal-loopback listener(s); $ownedListenerCount belonged to an audited rLogs desktop executable, $rejectedEndpointCount rLogs endpoint(s) failed the schema/response gate (categories: $categorySummary), and $unresolvedOwnerCount listener owner(s) could not be resolved. Supply -RLogsBaseUrl http://127.0.0.1:<port>."
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

function Assert-ExactBoolean($Value, [bool]$Expected, [string]$Field) {
    if ($Value -isnot [bool] -or $Value -ne $Expected) {
        throw "The sanitized lifecycle receipt has an invalid $Field value."
    }
}

function Assert-FiniteNumber($Value, [string]$Field) {
    if (-not (Test-FiniteJsonNumber $Value)) {
        throw "The sanitized lifecycle receipt has an invalid $Field value."
    }
}

function Assert-ExactPosition($Value, [string]$Field) {
    if (-not (Test-ExactPropertySet $Value @('x', 'y', 'z'))) {
        throw "The sanitized lifecycle receipt has an invalid $Field envelope."
    }
    foreach ($axis in @('x', 'y', 'z')) { Assert-FiniteNumber $Value.$axis "$Field.$axis" }
}

function Assert-SettledObservation($Value, [string]$Field) {
    $fields = @(
        'elapsed_micros', 'position', 'current_velocity', 'stability_sample_gap_millis',
        'stability_position_delta', 'velocity_norm', 'settled'
    )
    if (-not (Test-ExactPropertySet $Value $fields)) {
        throw "The sanitized lifecycle receipt has an invalid $Field envelope."
    }
    Assert-ExactPosition $Value.position "$Field.position"
    Assert-ExactPosition $Value.current_velocity "$Field.current_velocity"
    foreach ($numberField in @('elapsed_micros', 'stability_sample_gap_millis', 'stability_position_delta', 'velocity_norm')) {
        Assert-FiniteNumber $Value.$numberField "$Field.$numberField"
        if ([decimal]$Value.$numberField -lt 0) { throw "The sanitized lifecycle receipt has an invalid $Field.$numberField value." }
    }
    if ([decimal]$Value.elapsed_micros % 1 -ne 0 -or [decimal]$Value.stability_sample_gap_millis % 1 -ne 0) {
        throw "The sanitized lifecycle receipt has a non-integral $Field timestamp or interval."
    }
    Assert-ExactBoolean $Value.settled $true "$Field.settled"
}

function Assert-CalibrationSuccess($Canary, [bool]$RequireCancel = $true) {
    foreach ($field in @('marker_1_state_validated', 'foreground_validated_before_every_input',
        'root_context_unchanged', 'lifecycle_context_unchanged', 'rank_2_input_excitation',
        'approximately_returned')) {
        Assert-ExactBoolean $Canary.$field $true "canary.$field"
    }
    Assert-ExactBoolean $Canary.escape_emitted $RequireCancel 'canary.escape_emitted'
    Assert-ExactBoolean $Canary.cancelled $RequireCancel 'canary.cancelled'
    if ($Canary.calibration_pixels -ne 6 -or @($Canary.transitions).Count -ne 4) {
        throw 'The sanitized lifecycle receipt lacks the four-step calibration proof.'
    }
    Assert-SettledObservation $Canary.baseline 'canary.baseline'
    $expectedDeltas = @(@(6, 0), @(-6, 0), @(0, 6), @(0, -6))
    $maximumDisplacement = 0.0
    for ($index = 0; $index -lt 4; $index++) {
        $transition = @($Canary.transitions)[$index]
        if (-not (Test-ExactPropertySet $transition @(
            'sequence_index', 'emitted_integer_mouse_delta', 'input_elapsed_micros',
            'subsequent_observation', 'displacement'
        )) -or $transition.sequence_index -ne $index -or
            @($transition.emitted_integer_mouse_delta).Count -ne 2 -or
            [int]$transition.emitted_integer_mouse_delta[0] -ne $expectedDeltas[$index][0] -or
            [int]$transition.emitted_integer_mouse_delta[1] -ne $expectedDeltas[$index][1]) {
            throw 'The sanitized lifecycle receipt has an invalid calibration transition envelope.'
        }
        Assert-FiniteNumber $transition.input_elapsed_micros "canary.transitions[$index].input_elapsed_micros"
        Assert-FiniteNumber $transition.displacement "canary.transitions[$index].displacement"
        if ([decimal]$transition.input_elapsed_micros % 1 -ne 0 -or [decimal]$transition.input_elapsed_micros -lt 0 -or
            [double]$transition.displacement -lt 0.001) {
            throw 'The sanitized lifecycle receipt has invalid calibration transition measurements.'
        }
        Assert-SettledObservation $transition.subsequent_observation "canary.transitions[$index].subsequent_observation"
        $maximumDisplacement = [Math]::Max($maximumDisplacement, [double]$transition.displacement)
    }
    Assert-FiniteNumber $Canary.final_return_error 'canary.final_return_error'
    if ([double]$Canary.final_return_error -lt 0 -or
        [double]$Canary.final_return_error -gt [Math]::Max($maximumDisplacement * 0.35, 0.15)) {
        throw 'The sanitized lifecycle receipt has an invalid canary.final_return_error value.'
    }
}

function Assert-PlannerStepSuccess($PlannerStep) {
    $fields = @(
        'target', 'player_origin', 'target_player_distance', 'proposed_integer_mouse_delta',
        'predicted_distance', 'observed_distance', 'actual_to_predicted_improvement_ratio',
        'strict_distance_reduction', 'rollback_attempted', 'rollback_not_safe',
        'rollback_cancel_emitted', 'inverse_emitted', 'inverse_return_error', 'outcome'
    )
    if (-not (Test-ExactPropertySet $PlannerStep $fields) -or [string]$PlannerStep.outcome -cne 'passed') {
        throw 'The sanitized lifecycle receipt lacks a passing planner-step proof.'
    }
    Assert-ExactPosition $PlannerStep.target 'canary.planner_step.target'
    Assert-ExactPosition $PlannerStep.player_origin 'canary.planner_step.player_origin'
    foreach ($field in @('target_player_distance', 'predicted_distance', 'observed_distance',
        'actual_to_predicted_improvement_ratio', 'inverse_return_error')) {
        Assert-FiniteNumber $PlannerStep.$field "canary.planner_step.$field"
    }
    if (@($PlannerStep.proposed_integer_mouse_delta).Count -ne 2 -or
        @($PlannerStep.proposed_integer_mouse_delta).Where({ -not (Test-FiniteJsonNumber $_) -or [decimal]$_ % 1 -ne 0 }).Count -ne 0 -or
        [double]$PlannerStep.target_player_distance -lt 0 -or [double]$PlannerStep.target_player_distance -gt 18 -or
        [double]$PlannerStep.predicted_distance -lt 0 -or [double]$PlannerStep.observed_distance -lt 0 -or
        [double]$PlannerStep.actual_to_predicted_improvement_ratio -lt 0.20 -or
        [double]$PlannerStep.inverse_return_error -lt 0 -or [double]$PlannerStep.inverse_return_error -gt 0.002) {
        throw 'The sanitized lifecycle receipt has inconsistent planner-step measurements.'
    }
    Assert-ExactBoolean $PlannerStep.strict_distance_reduction $true 'canary.planner_step.strict_distance_reduction'
    Assert-ExactBoolean $PlannerStep.rollback_attempted $true 'canary.planner_step.rollback_attempted'
    Assert-ExactBoolean $PlannerStep.rollback_not_safe $false 'canary.planner_step.rollback_not_safe'
    Assert-ExactBoolean $PlannerStep.rollback_cancel_emitted $false 'canary.planner_step.rollback_cancel_emitted'
    Assert-ExactBoolean $PlannerStep.inverse_emitted $true 'canary.planner_step.inverse_emitted'
}

function Assert-ClosedLoopEnvelope($ClosedLoop) {
    $fields = @(
        'target', 'player_origin', 'target_player_distance', 'arrived', 'arrival_distance', 'steps',
        'emitted_move_count', 'cumulative_motion_pixels', 'input_observer_started',
        'foreign_mouse_moves_observed', 'rollback_attempted', 'rollback_inverse_count',
        'rollback_not_safe', 'rollback_cancel_emitted', 'rollback_return_error',
        'operator_placement', 'outcome'
    )
    if (-not (Test-ExactPropertySet $ClosedLoop $fields) -or [string]$ClosedLoop.outcome -cne 'passed' -or
        $ClosedLoop.steps -isnot [array]) {
        throw 'The sanitized lifecycle receipt lacks a passing closed-loop proof.'
    }
    Assert-ExactPosition $ClosedLoop.target 'canary.closed_loop.target'
    Assert-ExactPosition $ClosedLoop.player_origin 'canary.closed_loop.player_origin'
    foreach ($field in @('target_player_distance', 'arrival_distance', 'cumulative_motion_pixels')) {
        Assert-FiniteNumber $ClosedLoop.$field "canary.closed_loop.$field"
    }
    foreach ($field in @('emitted_move_count', 'foreign_mouse_moves_observed', 'rollback_inverse_count')) {
        Assert-FiniteNumber $ClosedLoop.$field "canary.closed_loop.$field"
        if ([decimal]$ClosedLoop.$field % 1 -ne 0 -or [decimal]$ClosedLoop.$field -lt 0) {
            throw "The sanitized lifecycle receipt has an invalid canary.closed_loop.$field value."
        }
    }
    if ([decimal]$ClosedLoop.emitted_move_count -ne @($ClosedLoop.steps).Count) {
        throw 'The sanitized lifecycle receipt has an inconsistent closed-loop step count.'
    }
    foreach ($step in @($ClosedLoop.steps)) {
        if (-not (Test-ExactPropertySet $step @(
            'command_id', 'emitted_integer_mouse_delta', 'distance_before', 'predicted_distance',
            'observed_distance', 'input_ownership_verified'
        )) -or @($step.emitted_integer_mouse_delta).Count -ne 2 -or
            @($step.emitted_integer_mouse_delta).Where({ -not (Test-FiniteJsonNumber $_) -or [decimal]$_ % 1 -ne 0 }).Count -ne 0) {
            throw 'The sanitized lifecycle receipt has an invalid closed-loop step envelope.'
        }
        foreach ($field in @('command_id', 'distance_before', 'predicted_distance', 'observed_distance')) {
            Assert-FiniteNumber $step.$field "canary.closed_loop.steps.$field"
        }
        if ([decimal]$step.command_id % 1 -ne 0 -or [decimal]$step.command_id -lt 0 -or
            [double]$step.distance_before -lt 0 -or [double]$step.predicted_distance -lt 0 -or
            [double]$step.observed_distance -lt 0) {
            throw 'The sanitized lifecycle receipt has invalid closed-loop step measurements.'
        }
        Assert-ExactBoolean $step.input_ownership_verified $true 'canary.closed_loop.steps.input_ownership_verified'
    }
    Assert-ExactBoolean $ClosedLoop.arrived $true 'canary.closed_loop.arrived'
    Assert-ExactBoolean $ClosedLoop.input_observer_started $true 'canary.closed_loop.input_observer_started'
    Assert-ExactBoolean $ClosedLoop.rollback_not_safe $false 'canary.closed_loop.rollback_not_safe'
    Assert-ExactBoolean $ClosedLoop.rollback_cancel_emitted $false 'canary.closed_loop.rollback_cancel_emitted'
    if ([double]$ClosedLoop.target_player_distance -lt 0 -or [double]$ClosedLoop.target_player_distance -gt 18 -or
        [double]$ClosedLoop.arrival_distance -lt 0 -or [double]$ClosedLoop.arrival_distance -gt 0.075 -or
        [double]$ClosedLoop.cumulative_motion_pixels -lt 0 -or [double]$ClosedLoop.cumulative_motion_pixels -gt 16 -or
        [decimal]$ClosedLoop.emitted_move_count -gt 4 -or [decimal]$ClosedLoop.foreign_mouse_moves_observed -ne 0) {
        throw 'The sanitized lifecycle receipt has inconsistent closed-loop measurements.'
    }
}

function Assert-ClosedLoopSuccess($ClosedLoop) {
    Assert-ClosedLoopEnvelope $ClosedLoop
    if ($null -ne $ClosedLoop.operator_placement -or
        [decimal]$ClosedLoop.rollback_inverse_count -ne [decimal]$ClosedLoop.emitted_move_count) {
        throw 'The sanitized lifecycle receipt has an invalid closed-loop rollback proof.'
    }
    Assert-ExactBoolean $ClosedLoop.rollback_attempted ([decimal]$ClosedLoop.emitted_move_count -gt 0) 'canary.closed_loop.rollback_attempted'
    Assert-FiniteNumber $ClosedLoop.rollback_return_error 'canary.closed_loop.rollback_return_error'
    if ([double]$ClosedLoop.rollback_return_error -lt 0 -or [double]$ClosedLoop.rollback_return_error -gt 0.01) {
        throw 'The sanitized lifecycle receipt has an invalid closed-loop return error.'
    }
}

function Assert-OperatorPlacementSuccess($Receipt) {
    Assert-ExactBoolean $Receipt.summary.placement_attempted $true 'summary.placement_attempted'
    Assert-CalibrationSuccess $Receipt.canary $false
    $closed = $Receipt.canary.closed_loop
    Assert-ClosedLoopEnvelope $closed
    $placement = $closed.operator_placement
    $fields = @(
        'human_click_observed', 'programmatic_click_emitted', 'injected_click_observed',
        'other_click_observed', 'outbound_marker_1_newer', 'outbound_observed_micros',
        'inbound_marker_1_newer', 'inbound_observed_micros', 'inbound_target_distance',
        'context_continuous', 'timed_out', 'escape_emitted', 'outcome'
    )
    if (-not (Test-ExactPropertySet $placement $fields) -or [string]$placement.outcome -cne 'passed') {
        throw 'The sanitized lifecycle receipt lacks a passing operator-placement proof.'
    }
    foreach ($field in @('human_click_observed', 'outbound_marker_1_newer', 'inbound_marker_1_newer', 'context_continuous')) {
        Assert-ExactBoolean $placement.$field $true "canary.closed_loop.operator_placement.$field"
    }
    foreach ($field in @('programmatic_click_emitted', 'injected_click_observed', 'other_click_observed', 'timed_out', 'escape_emitted')) {
        Assert-ExactBoolean $placement.$field $false "canary.closed_loop.operator_placement.$field"
    }
    Assert-ExactBoolean $closed.rollback_attempted $false 'canary.closed_loop.rollback_attempted'
    if ([decimal]$closed.rollback_inverse_count -ne 0 -or $null -ne $closed.rollback_return_error) {
        throw 'The sanitized operator-placement receipt claims an unsafe post-click aim rollback.'
    }
    foreach ($field in @('outbound_observed_micros', 'inbound_observed_micros', 'inbound_target_distance')) {
        Assert-FiniteNumber $placement.$field "canary.closed_loop.operator_placement.$field"
    }
    if ([decimal]$placement.outbound_observed_micros % 1 -ne 0 -or [decimal]$placement.outbound_observed_micros -lt 0 -or
        [decimal]$placement.inbound_observed_micros % 1 -ne 0 -or
        [decimal]$placement.inbound_observed_micros -le [decimal]$placement.outbound_observed_micros -or
        [double]$placement.inbound_target_distance -lt 0 -or [double]$placement.inbound_target_distance -gt 0.075) {
        throw 'The sanitized lifecycle receipt has inconsistent operator-placement timing or distance evidence.'
    }
}

function Assert-ArmedModeSuccess($Receipt, [string]$ExpectedMode) {
    if ([string]$Receipt.canary.outcome -cne 'passed') { return }
    switch ($ExpectedMode) {
        'marker1-reversible-calibration-v1' { Assert-CalibrationSuccess $Receipt.canary }
        'marker1-single-planner-step-and-restore-v1' {
            Assert-CalibrationSuccess $Receipt.canary
            Assert-PlannerStepSuccess $Receipt.canary.planner_step
        }
        'marker1-closed-loop-aim-and-rollback-v1' {
            Assert-CalibrationSuccess $Receipt.canary
            Assert-ClosedLoopSuccess $Receipt.canary.closed_loop
        }
        'marker1-operator-click-placement-evidence-v1' { Assert-OperatorPlacementSuccess $Receipt }
        default { throw 'The sanitized lifecycle receipt has an unsupported armed mode.' }
    }
}

function Read-ValidatedLifecycleReceipt(
    [string]$Path,
    [string]$ExpectedMode,
    [bool]$ExpectedArmed,
    [int]$ExpectedDurationMs,
    [int]$ExpectedIntervalMs,
    [DateTime]$NotBeforeUtc,
    [DateTime]$NotAfterUtc
) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw 'The native probe returned without creating the expected sanitized lifecycle receipt.'
    }
    $file = Get-Item -LiteralPath $Path -ErrorAction Stop
    if ($file.Length -le 0) {
        throw 'The sanitized lifecycle receipt was empty.'
    }
    try {
        $receipt = Get-Content -LiteralPath $Path -Raw -ErrorAction Stop | ConvertFrom-Json -ErrorAction Stop
    } catch {
        throw 'The sanitized lifecycle receipt is not valid JSON.'
    }
    $topLevel = @(
        'schema_version', 'generated_by', 'game', 'deployment', 'channel', 'game_build',
        'distribution_app_id', 'observed_unix_millis', 'duration_millis', 'interval_millis',
        'identities', 'acquisition', 'events', 'summary', 'policy', 'canary'
    )
    if (-not (Test-ExactPropertySet $receipt $topLevel) -or $receipt.schema_version -ne 7 -or
        [string]$receipt.generated_by -cne 'rlogs-bpsr-automarker-lifecycle-probe' -or
        [string]$receipt.game -cne 'blue-protocol-star-resonance' -or
        [string]$receipt.deployment -cne 'global' -or [string]$receipt.channel -cne 'steam' -or
        [string]$receipt.game_build -cne $expectedBuild -or
        [string]$receipt.distribution_app_id -cne $expectedAppId -or
        -not (Test-FiniteJsonNumber $receipt.observed_unix_millis) -or
        [decimal]$receipt.observed_unix_millis % 1 -ne 0 -or
        [decimal]$receipt.observed_unix_millis -lt ([DateTimeOffset]$NotBeforeUtc).ToUnixTimeMilliseconds() -or
        [decimal]$receipt.observed_unix_millis -gt ([DateTimeOffset]$NotAfterUtc).ToUnixTimeMilliseconds() -or
        $receipt.duration_millis -ne $ExpectedDurationMs -or
        $receipt.interval_millis -ne $ExpectedIntervalMs -or $receipt.events -isnot [array]) {
        throw 'The sanitized lifecycle receipt envelope does not match this exact probe invocation.'
    }

    if (-not (Test-ExactPropertySet $receipt.identities @('process_executable', 'game_assembly', 'steam_manifest'))) {
        throw 'The sanitized lifecycle receipt identities envelope is invalid.'
    }
    foreach ($identityName in @('process_executable', 'game_assembly', 'steam_manifest')) {
        $identity = $receipt.identities.$identityName
        if (-not (Test-ExactPropertySet $identity @('byte_length', 'sha256')) -or
            -not (Test-FiniteJsonNumber $identity.byte_length) -or [decimal]$identity.byte_length % 1 -ne 0 -or
            [decimal]$identity.byte_length -lt 1 -or [string]$identity.sha256 -cnotmatch '^[0-9a-f]{64}$') {
            throw "The sanitized lifecycle receipt has an invalid $identityName identity."
        }
    }

    $acquisitionFields = @(
        'root_kind', 'class_identity_validation_required', 'validated_classes',
        'roots_double_read_per_sample', 'lifecycle_state_double_read_per_sample'
    )
    $expectedValidatedClasses = @(
        'ZUtil.ZSingleton`1', 'Panda.ZGame.ZEntityMgr', 'Panda.ZGame.PlayerEnt',
        'Panda.ZGame.PlayerEnt__Storage', 'Panda.ZGame.PlayerSkillInputComp',
        'Panda.ZGame.ZSkillInputMgr', 'Panda.ZGame.ZIndicatorMgr'
    )
    if (-not (Test-ExactPropertySet $receipt.acquisition $acquisitionFields) -or
        [string]$receipt.acquisition.root_kind -cne 'reviewed-singleton-method-info' -or
        (@($receipt.acquisition.validated_classes) -join "`n") -cne ($expectedValidatedClasses -join "`n")) {
        throw 'The sanitized lifecycle receipt acquisition envelope is invalid.'
    }
    Assert-ExactBoolean $receipt.acquisition.class_identity_validation_required $true 'acquisition.class_identity_validation_required'
    Assert-ExactBoolean $receipt.acquisition.roots_double_read_per_sample $true 'acquisition.roots_double_read_per_sample'
    Assert-ExactBoolean $receipt.acquisition.lifecycle_state_double_read_per_sample $true 'acquisition.lifecycle_state_double_read_per_sample'

    $summaryFields = @(
        'poll_attempts', 'accepted_samples', 'emitted_transitions', 'unavailable_samples',
        'rejected_identity_samples', 'torn_samples', 'placement_attempted',
        'programmatic_activation_proven'
    )
    if (-not (Test-ExactPropertySet $receipt.summary $summaryFields)) {
        throw 'The sanitized lifecycle receipt summary envelope is invalid.'
    }
    if ($receipt.summary.placement_attempted -isnot [bool]) {
        throw 'The sanitized lifecycle receipt has an invalid summary.placement_attempted value.'
    }
    Assert-ExactBoolean $receipt.summary.programmatic_activation_proven $false 'summary.programmatic_activation_proven'

    $policyFields = @(
        'exact_build_and_hashes_required', 'process_rights', 'allowlisted_pointer_chain_only',
        'allowlisted_fields_only', 'heap_or_process_scan_performed', 'process_identifiers_emitted',
        'raw_addresses_emitted', 'filesystem_paths_emitted', 'debugger_attached', 'threads_suspended',
        'code_injected_or_invoked', 'process_memory_written', 'remote_process_write_rights_requested',
        'remote_memory_allocated', 'remote_thread_created', 'dll_injected',
        'internal_game_function_invoked', 'packets_observed_or_modified', 'packet_synthesis_performed',
        'ordinary_foreground_input_only', 'place_enabled', 'mouse_click_emitted',
        'reversible_mouse_move_enabled', 'escape_cancel_enabled'
    )
    if (-not (Test-ExactPropertySet $receipt.policy $policyFields) -or
        @($receipt.policy.process_rights).Count -ne 2 -or
        [string]$receipt.policy.process_rights[0] -cne 'PROCESS_QUERY_INFORMATION' -or
        [string]$receipt.policy.process_rights[1] -cne 'PROCESS_VM_READ') {
        throw 'The sanitized lifecycle receipt policy envelope is invalid.'
    }
    foreach ($field in @('exact_build_and_hashes_required', 'allowlisted_pointer_chain_only', 'allowlisted_fields_only', 'ordinary_foreground_input_only')) {
        Assert-ExactBoolean $receipt.policy.$field $true "policy.$field"
    }
    foreach ($field in @('heap_or_process_scan_performed', 'process_identifiers_emitted', 'raw_addresses_emitted',
        'filesystem_paths_emitted', 'debugger_attached', 'threads_suspended', 'code_injected_or_invoked',
        'process_memory_written', 'remote_process_write_rights_requested', 'remote_memory_allocated',
        'remote_thread_created', 'dll_injected', 'internal_game_function_invoked',
        'packets_observed_or_modified', 'packet_synthesis_performed', 'place_enabled', 'mouse_click_emitted')) {
        Assert-ExactBoolean $receipt.policy.$field $false "policy.$field"
    }
    Assert-ExactBoolean $receipt.policy.reversible_mouse_move_enabled $ExpectedArmed 'policy.reversible_mouse_move_enabled'
    Assert-ExactBoolean $receipt.policy.escape_cancel_enabled $ExpectedArmed 'policy.escape_cancel_enabled'

    $canaryFields = @(
        'armed', 'mode', 'calibration_pixels', 'marker_1_state_validated',
        'foreground_validated_before_every_input', 'root_context_unchanged',
        'lifecycle_context_unchanged', 'rank_2_input_excitation', 'escape_emitted', 'baseline',
        'transitions', 'final_return_error', 'approximately_returned', 'cancelled', 'outcome',
        'preflight', 'planner_step', 'closed_loop'
    )
    if (-not (Test-ExactPropertySet $receipt.canary $canaryFields) -or
        $receipt.canary.transitions -isnot [array] -or
        [string]$receipt.canary.mode -cne $ExpectedMode -or
        [string]$receipt.canary.outcome -cnotmatch '^[a-z0-9_-]{1,80}$') {
        throw 'The sanitized lifecycle receipt canary envelope does not match the requested mode.'
    }
    Assert-ExactBoolean $receipt.canary.armed $ExpectedArmed 'canary.armed'
    if (-not $ExpectedArmed -and [string]$receipt.canary.outcome -cne 'not-armed-read-only') {
        throw 'The sanitized lifecycle receipt has an unexpected read-only outcome.'
    }
    if ($ExpectedArmed) { Assert-ArmedModeSuccess $receipt $ExpectedMode }
    return $receipt
}

function Assert-ArmedCanaryPassed($Receipt) {
    $outcome = [string]$Receipt.canary.outcome
    Write-Host "Sanitized armed canary outcome: $outcome"
    if ($outcome -cne 'passed') {
        throw 'The armed canary failed closed. Its sanitized receipt was retained for diagnosis.'
    }
}

function New-SyntheticLifecycleReceipt([bool]$Armed, [string]$Mode, [string]$Outcome) {
    $identity = [ordered]@{ byte_length = 1; sha256 = ('a' * 64) }
    $receipt = [ordered]@{
        schema_version = 7; generated_by = 'rlogs-bpsr-automarker-lifecycle-probe'
        game = 'blue-protocol-star-resonance'; deployment = 'global'; channel = 'steam'
        game_build = $expectedBuild; distribution_app_id = $expectedAppId
        observed_unix_millis = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
        duration_millis = 100; interval_millis = 10
        identities = [ordered]@{ process_executable = $identity; game_assembly = $identity; steam_manifest = $identity }
        acquisition = [ordered]@{
            root_kind = 'reviewed-singleton-method-info'; class_identity_validation_required = $true
            validated_classes = @('ZUtil.ZSingleton`1', 'Panda.ZGame.ZEntityMgr', 'Panda.ZGame.PlayerEnt', 'Panda.ZGame.PlayerEnt__Storage', 'Panda.ZGame.PlayerSkillInputComp', 'Panda.ZGame.ZSkillInputMgr', 'Panda.ZGame.ZIndicatorMgr')
            roots_double_read_per_sample = $true; lifecycle_state_double_read_per_sample = $true
        }
        events = @()
        summary = [ordered]@{ poll_attempts = 0; accepted_samples = 0; emitted_transitions = 0; unavailable_samples = 0; rejected_identity_samples = 0; torn_samples = 0; placement_attempted = $false; programmatic_activation_proven = $false }
        policy = [ordered]@{
            exact_build_and_hashes_required = $true; process_rights = @('PROCESS_QUERY_INFORMATION', 'PROCESS_VM_READ')
            allowlisted_pointer_chain_only = $true; allowlisted_fields_only = $true; heap_or_process_scan_performed = $false
            process_identifiers_emitted = $false; raw_addresses_emitted = $false; filesystem_paths_emitted = $false
            debugger_attached = $false; threads_suspended = $false; code_injected_or_invoked = $false
            process_memory_written = $false; remote_process_write_rights_requested = $false; remote_memory_allocated = $false
            remote_thread_created = $false; dll_injected = $false; internal_game_function_invoked = $false
            packets_observed_or_modified = $false; packet_synthesis_performed = $false
            ordinary_foreground_input_only = $true; place_enabled = $false; mouse_click_emitted = $false
            reversible_mouse_move_enabled = $Armed; escape_cancel_enabled = $Armed
        }
        canary = [ordered]@{
            armed = $Armed; mode = $Mode; calibration_pixels = $(if ($Armed) { 6 } else { 0 })
            marker_1_state_validated = $false; foreground_validated_before_every_input = $false
            root_context_unchanged = $false; lifecycle_context_unchanged = $false
            rank_2_input_excitation = $false; escape_emitted = $false; baseline = $null; transitions = @()
            final_return_error = $null; approximately_returned = $false; cancelled = $false; outcome = $Outcome
            preflight = $null; planner_step = $null; closed_loop = $null
        }
    }
    if ($Armed -and $Outcome -ceq 'passed') {
        $settled = [ordered]@{
            elapsed_micros = 10; position = [ordered]@{ x = 1; y = 2; z = 3 }
            current_velocity = [ordered]@{ x = 0; y = 0; z = 0 }; stability_sample_gap_millis = 50
            stability_position_delta = 0.001; velocity_norm = 0.001; settled = $true
        }
        $receipt.canary.marker_1_state_validated = $true
        $receipt.canary.foreground_validated_before_every_input = $true
        $receipt.canary.root_context_unchanged = $true
        $receipt.canary.lifecycle_context_unchanged = $true
        $receipt.canary.rank_2_input_excitation = $true
        $receipt.canary.baseline = $settled
        $receipt.canary.transitions = @(
            [ordered]@{ sequence_index = 0; emitted_integer_mouse_delta = @(6, 0); input_elapsed_micros = 20; subsequent_observation = $settled; displacement = 0.5 },
            [ordered]@{ sequence_index = 1; emitted_integer_mouse_delta = @(-6, 0); input_elapsed_micros = 30; subsequent_observation = $settled; displacement = 0.5 },
            [ordered]@{ sequence_index = 2; emitted_integer_mouse_delta = @(0, 6); input_elapsed_micros = 40; subsequent_observation = $settled; displacement = 0.5 },
            [ordered]@{ sequence_index = 3; emitted_integer_mouse_delta = @(0, -6); input_elapsed_micros = 50; subsequent_observation = $settled; displacement = 0.5 }
        )
        $receipt.canary.final_return_error = 0.01
        $receipt.canary.approximately_returned = $true
        $receipt.canary.escape_emitted = $true
        $receipt.canary.cancelled = $true
        if ($Mode -ceq 'marker1-single-planner-step-and-restore-v1') {
            $receipt.canary.planner_step = [ordered]@{
                target = [ordered]@{ x = 1; y = 2; z = 3 }; player_origin = [ordered]@{ x = 0; y = 0; z = 0 }
                target_player_distance = 3.75; proposed_integer_mouse_delta = @(1, -1)
                predicted_distance = 2.5; observed_distance = 3.0; actual_to_predicted_improvement_ratio = 0.6
                strict_distance_reduction = $true; rollback_attempted = $true; rollback_not_safe = $false
                rollback_cancel_emitted = $false; inverse_emitted = $true; inverse_return_error = 0.001; outcome = 'passed'
            }
        }
        if ($Mode -ceq 'marker1-closed-loop-aim-and-rollback-v1' -or
            $Mode -ceq 'marker1-operator-click-placement-evidence-v1') {
            $receipt.canary.closed_loop = [ordered]@{
                target = [ordered]@{ x = 1; y = 2; z = 3 }; player_origin = [ordered]@{ x = 0; y = 0; z = 0 }
                target_player_distance = 3.75; arrived = $true; arrival_distance = 0.05
                steps = @([ordered]@{
                    command_id = 1; emitted_integer_mouse_delta = @(1, -1); distance_before = 1.0
                    predicted_distance = 0.5; observed_distance = 0.4; input_ownership_verified = $true
                })
                emitted_move_count = 1; cumulative_motion_pixels = 4.0; input_observer_started = $true
                foreign_mouse_moves_observed = 0; rollback_attempted = $true; rollback_inverse_count = 1
                rollback_not_safe = $false; rollback_cancel_emitted = $false; rollback_return_error = 0.005
                operator_placement = $null; outcome = 'passed'
            }
        }
        if ($Mode -ceq 'marker1-operator-click-placement-evidence-v1') {
            $receipt.summary.placement_attempted = $true
            $receipt.canary.escape_emitted = $false
            $receipt.canary.cancelled = $false
            $receipt.canary.closed_loop.rollback_attempted = $false
            $receipt.canary.closed_loop.rollback_inverse_count = 0
            $receipt.canary.closed_loop.rollback_return_error = $null
            $receipt.canary.closed_loop.operator_placement = [ordered]@{
                human_click_observed = $true; programmatic_click_emitted = $false
                injected_click_observed = $false; other_click_observed = $false
                outbound_marker_1_newer = $true; outbound_observed_micros = 200
                inbound_marker_1_newer = $true; inbound_observed_micros = 250
                inbound_target_distance = 0.01; context_continuous = $true; timed_out = $false
                escape_emitted = $false; outcome = 'passed'
            }
        }
    }
    return $receipt
}

function Invoke-LauncherSelfTest {
    Initialize-SystemNetHttp
    $httpHandler = [Net.Http.HttpClientHandler]::new()
    $httpClient = [Net.Http.HttpClient]::new($httpHandler)
    $httpClient.Dispose()
    $httpHandler.Dispose()
    $syntheticHttpRuntimeFailure = [pscustomobject]@{
        Exception = [InvalidOperationException]::new('Unable to find type [Net.Http.HttpClientHandler].')
    }
    if ((Get-LoopbackEndpointRejectionCategory $syntheticHttpRuntimeFailure) -cne 'http-runtime-unavailable') {
        throw 'Self-test failed: missing System.Net.Http was not classified distinctly.'
    }
    # Windows PowerShell 5.1 unwraps Nullable[Double] assignments to ordinary
    # Double values. Exercise the exact native argument construction without
    # opening a process or emitting input, and ensure no `.Value` access exists.
    $nullableX = [Nullable[double]]1.25
    $nullableY = [Nullable[double]]-2.5
    $nullableZ = [Nullable[double]]3.75
    $targetArguments = @(New-PlannerTargetArguments $nullableX $nullableY $nullableZ 'http://127.0.0.1:54221')
    $expectedTargetArguments = @('--target-x', '1.25', '--target-y', '-2.5', '--target-z', '3.75', '--rlogs-base-url', 'http://127.0.0.1:54221')
    if (($targetArguments -join "`n") -cne ($expectedTargetArguments -join "`n")) {
        throw 'Self-test failed: PS 5.1-safe target argument construction changed coordinate values.'
    }
    $rejected = $false
    try { [void](New-PlannerTargetArguments ([double]::NaN) 2 3 'http://127.0.0.1:54221') } catch { $rejected = $true }
    if (-not $rejected) { throw 'Self-test failed: non-finite native target argument was accepted.' }
    if ((Get-ArmedPreparationCountdownSeconds $true) -ne 10 -or
        (Get-ArmedPreparationCountdownSeconds $false) -ne 5) {
        throw 'Self-test failed: armed preparation countdown selection changed.'
    }
    Assert-LoopbackBaseUrl 'http://127.0.0.1:54221'
    foreach ($acceptedOwner in @('rlogs-app', 'rlogs-app.exe', 'rLogs', 'rLogs.exe')) {
        if (-not (Test-RLogsDesktopOwnerName $acceptedOwner)) {
            throw "Self-test failed: audited desktop owner '$acceptedOwner' was rejected."
        }
    }
    foreach ($rejectedOwner in @('', 'rlogs-app-helper', 'rlogs-desktop-host', 'not-rlogs.exe')) {
        if (Test-RLogsDesktopOwnerName $rejectedOwner) {
            throw "Self-test failed: unaudited desktop owner '$rejectedOwner' was accepted."
        }
    }
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

    $receiptTestPath = Join-Path ([IO.Path]::GetTempPath()) ("rlogs-lifecycle-launcher-self-test-" + [Guid]::NewGuid().ToString('N') + '.json')
    try {
        $notBefore = [DateTime]::UtcNow.AddSeconds(-1)
        $synthetic = New-SyntheticLifecycleReceipt $true 'marker1-closed-loop-aim-and-rollback-v1' 'passed'
        [IO.File]::WriteAllText($receiptTestPath, ($synthetic | ConvertTo-Json -Depth 30), [Text.UTF8Encoding]::new($false))
        $validated = Read-ValidatedLifecycleReceipt $receiptTestPath 'marker1-closed-loop-aim-and-rollback-v1' $true 100 10 $notBefore ([DateTime]::UtcNow.AddSeconds(1))
        Assert-ArmedCanaryPassed $validated

        foreach ($passingMode in @(
            'marker1-reversible-calibration-v1',
            'marker1-single-planner-step-and-restore-v1',
            'marker1-operator-click-placement-evidence-v1'
        )) {
            $passing = New-SyntheticLifecycleReceipt $true $passingMode 'passed'
            [IO.File]::WriteAllText($receiptTestPath, ($passing | ConvertTo-Json -Depth 30), [Text.UTF8Encoding]::new($false))
            [void](Read-ValidatedLifecycleReceipt $receiptTestPath $passingMode $true 100 10 $notBefore ([DateTime]::UtcNow.AddSeconds(1)))
        }

        foreach ($nestedFailure in @(
            'calibration-return', 'missing-planner-step', 'failed-closed-loop',
            'operator-not-attempted', 'operator-missing', 'operator-failed',
            'operator-programmatic-click', 'operator-injected-click', 'operator-other-click',
            'operator-stale-outbound', 'operator-missing-inbound', 'operator-context-lost',
            'operator-timeout', 'operator-distance', 'operator-timestamp-order',
            'operator-calibration', 'operator-rollback'
        )) {
            $failureMode = if ($nestedFailure -eq 'calibration-return') { 'marker1-reversible-calibration-v1' } `
                elseif ($nestedFailure -eq 'missing-planner-step') { 'marker1-single-planner-step-and-restore-v1' } `
                elseif ($nestedFailure -eq 'failed-closed-loop') { 'marker1-closed-loop-aim-and-rollback-v1' } `
                else { 'marker1-operator-click-placement-evidence-v1' }
            $candidate = New-SyntheticLifecycleReceipt $true $failureMode 'passed'
            switch ($nestedFailure) {
                'calibration-return' { $candidate.canary.approximately_returned = $false }
                'missing-planner-step' { $candidate.canary.planner_step = $null }
                'failed-closed-loop' { $candidate.canary.closed_loop.outcome = 'rollback_not_safe' }
                'operator-not-attempted' { $candidate.summary.placement_attempted = $false }
                'operator-missing' { $candidate.canary.closed_loop.operator_placement = $null }
                'operator-failed' { $candidate.canary.closed_loop.operator_placement.outcome = 'failed-closed' }
                'operator-programmatic-click' { $candidate.canary.closed_loop.operator_placement.programmatic_click_emitted = $true }
                'operator-injected-click' { $candidate.canary.closed_loop.operator_placement.injected_click_observed = $true }
                'operator-other-click' { $candidate.canary.closed_loop.operator_placement.other_click_observed = $true }
                'operator-stale-outbound' { $candidate.canary.closed_loop.operator_placement.outbound_marker_1_newer = $false }
                'operator-missing-inbound' { $candidate.canary.closed_loop.operator_placement.inbound_marker_1_newer = $false }
                'operator-context-lost' { $candidate.canary.closed_loop.operator_placement.context_continuous = $false }
                'operator-timeout' { $candidate.canary.closed_loop.operator_placement.timed_out = $true }
                'operator-distance' { $candidate.canary.closed_loop.operator_placement.inbound_target_distance = 0.076 }
                'operator-timestamp-order' { $candidate.canary.closed_loop.operator_placement.inbound_observed_micros = 200 }
                'operator-calibration' { $candidate.canary.rank_2_input_excitation = $false }
                'operator-rollback' {
                    $candidate.canary.closed_loop.rollback_attempted = $true
                    $candidate.canary.closed_loop.rollback_inverse_count = 1
                    $candidate.canary.closed_loop.rollback_return_error = 0.001
                }
            }
            [IO.File]::WriteAllText($receiptTestPath, ($candidate | ConvertTo-Json -Depth 30), [Text.UTF8Encoding]::new($false))
            $rejected = $false
            try { [void](Read-ValidatedLifecycleReceipt $receiptTestPath $failureMode $true 100 10 $notBefore ([DateTime]::UtcNow.AddSeconds(1))) } catch { $rejected = $true }
            if (-not $rejected) { throw "Self-test failed: $nestedFailure nested proof was accepted." }
        }

        $synthetic.canary.outcome = 'failed-closed'
        [IO.File]::WriteAllText($receiptTestPath, ($synthetic | ConvertTo-Json -Depth 30), [Text.UTF8Encoding]::new($false))
        $validatedFailure = Read-ValidatedLifecycleReceipt $receiptTestPath 'marker1-closed-loop-aim-and-rollback-v1' $true 100 10 $notBefore ([DateTime]::UtcNow.AddSeconds(1))
        $rejected = $false
        try { Assert-ArmedCanaryPassed $validatedFailure } catch { $rejected = $true }
        if (-not $rejected) { throw 'Self-test failed: a non-passing armed receipt returned success.' }

        $readOnly = New-SyntheticLifecycleReceipt $false 'read-only' 'not-armed-read-only'
        [IO.File]::WriteAllText($receiptTestPath, ($readOnly | ConvertTo-Json -Depth 30), [Text.UTF8Encoding]::new($false))
        [void](Read-ValidatedLifecycleReceipt $receiptTestPath 'read-only' $false 100 10 $notBefore ([DateTime]::UtcNow.AddSeconds(1)))
        $readOnly.canary.outcome = 'passed'
        [IO.File]::WriteAllText($receiptTestPath, ($readOnly | ConvertTo-Json -Depth 30), [Text.UTF8Encoding]::new($false))
        $rejected = $false
        try { [void](Read-ValidatedLifecycleReceipt $receiptTestPath 'read-only' $false 100 10 $notBefore ([DateTime]::UtcNow.AddSeconds(1))) } catch { $rejected = $true }
        if (-not $rejected) { throw 'Self-test failed: a mismatched read-only outcome was accepted.' }

        foreach ($invalid in @('wrong-schema', 'missing-critical', 'extra-critical', 'mismatched-mode', 'mismatched-invocation', 'stale-receipt', 'unsafe-policy')) {
            $candidate = New-SyntheticLifecycleReceipt $true 'marker1-closed-loop-aim-and-rollback-v1' 'passed'
            switch ($invalid) {
                'wrong-schema' { $candidate.schema_version = 6 }
                'missing-critical' { [void]$candidate.Remove('summary') }
                'extra-critical' { $candidate.unexpected = 'rejected' }
                'mismatched-mode' { $candidate.canary.mode = 'marker1-reversible-calibration-v1' }
                'mismatched-invocation' { $candidate.duration_millis = 101 }
                'stale-receipt' { $candidate.observed_unix_millis = ([DateTimeOffset]$notBefore).AddSeconds(-1).ToUnixTimeMilliseconds() }
                'unsafe-policy' { $candidate.policy.place_enabled = $true }
            }
            [IO.File]::WriteAllText($receiptTestPath, ($candidate | ConvertTo-Json -Depth 30), [Text.UTF8Encoding]::new($false))
            $rejected = $false
            try { [void](Read-ValidatedLifecycleReceipt $receiptTestPath 'marker1-closed-loop-aim-and-rollback-v1' $true 100 10 $notBefore ([DateTime]::UtcNow.AddSeconds(1))) } catch { $rejected = $true }
            if (-not $rejected) { throw "Self-test failed: $invalid receipt was accepted." }
        }

        [IO.File]::WriteAllText($receiptTestPath, '{malformed', [Text.UTF8Encoding]::new($false))
        $rejected = $false
        try { [void](Read-ValidatedLifecycleReceipt $receiptTestPath 'read-only' $false 100 10 $notBefore ([DateTime]::UtcNow.AddSeconds(1))) } catch { $rejected = $true }
        if (-not $rejected) { throw 'Self-test failed: malformed receipt JSON was accepted.' }
    } finally {
        Remove-Item -LiteralPath $receiptTestPath -Force -ErrorAction SilentlyContinue
    }
    Write-Host 'Launcher self-test passed: loopback/preset gates and strict synthetic sanitized-receipt validation.'
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
    foreach ($remaining in (Get-ArmedPreparationCountdownSeconds $false)..1) {
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
        $TargetX = [double]$resolved.X
        $TargetY = [double]$resolved.Y
        $TargetZ = [double]$resolved.Z
        $resolvedName = [regex]::Replace($resolved.PresetName, '\p{C}', '?')
        $resolvedId = [regex]::Replace($resolved.PresetId, '\p{C}', '?')
        Write-Host "Resolved Marker 1 from preset '$resolvedName' ($resolvedId) in the exact active activity family."
    }
    $targetArguments = @(New-PlannerTargetArguments $TargetX $TargetY $TargetZ $RLogsBaseUrl)
    if ($ArmOperatorPlacement) {
        Write-Warning 'ARMED OPERATOR PLACEMENT EVIDENCE: during the countdown, return to the game, open the marker menu, select Marker 1, and leave its reticle active. Do not move the mouse after selecting it. The canary aims without clicking; click exactly once only after the reticle visibly stops moving. It requires newer outbound and authoritative inbound Marker 1 evidence.'
    } elseif ($ArmClosedLoopAim) {
        Write-Warning 'ARMED CLOSED-LOOP CANARY: manually select Marker 1 and keep the game focused. This calibrates, makes at most four <=4-pixel moves (<=16 cumulative), reverses every move, then Escape. Do not touch the mouse. It never clicks or places.'
    } else {
        Write-Warning 'ARMED ONE-STEP CANARY: manually select Marker 1 and keep the game focused. This calibrates, moves at most 4 pixels once, applies the exact inverse, then Escape. It never clicks or places.'
    }
    $preparationSeconds = Get-ArmedPreparationCountdownSeconds ([bool]$ArmOperatorPlacement)
    if ($ArmOperatorPlacement) {
        Write-Host "Return to the game and select Marker 1 now. Leave its reticle active; the fail-closed canary starts in $preparationSeconds seconds."
    } else {
        Write-Host "Return focus to the game now. The fail-closed canary starts in $preparationSeconds seconds."
    }
    foreach ($remaining in $preparationSeconds..1) {
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
    $arguments += @('--armed-mode', $armedToken)
    $arguments += $targetArguments
}

$expectedCanaryMode = if ($ArmOperatorPlacement) {
    'marker1-operator-click-placement-evidence-v1'
} elseif ($ArmClosedLoopAim) {
    'marker1-closed-loop-aim-and-rollback-v1'
} elseif ($ArmSinglePlannerStep) {
    'marker1-single-planner-step-and-restore-v1'
} elseif ($ArmReversibleCalibration) {
    'marker1-reversible-calibration-v1'
} else {
    'read-only'
}
$armedCanary = [bool]($ArmReversibleCalibration -or $ArmSinglePlannerStep -or $ArmClosedLoopAim -or $ArmOperatorPlacement)
$probeStartedUtc = [DateTime]::UtcNow
& $probe @arguments
$probeExitCode = $LASTEXITCODE
$probeFinishedUtc = [DateTime]::UtcNow
if ($probeExitCode -ne 0) { throw 'The exact-build probe or calibration canary failed closed.' }
$validatedReceipt = Read-ValidatedLifecycleReceipt `
    $receipt $expectedCanaryMode $armedCanary $DurationMs $IntervalMs $probeStartedUtc $probeFinishedUtc
if ($armedCanary) {
    Assert-ArmedCanaryPassed $validatedReceipt
} else {
    Write-Host "Created sanitized receipt $(Split-Path -Leaf $receipt). Marker confirmation and Place remain disabled."
}
