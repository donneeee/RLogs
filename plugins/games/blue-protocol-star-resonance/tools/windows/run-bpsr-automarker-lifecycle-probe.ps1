[CmdletBinding()]
param(
    [string]$InstallRoot,
    [string]$SteamManifest,
    [ValidateRange(100, 60000)][int]$DurationMs = 15000,
    [ValidateRange(5, 1000)][int]$IntervalMs = 10,
    [switch]$ArmReversibleCalibration,
    [switch]$ArmSinglePlannerStep,
    [Nullable[double]]$TargetX,
    [Nullable[double]]$TargetY,
    [Nullable[double]]$TargetZ,
    [string]$RLogsBaseUrl,
    [switch]$DryRun
)

$ErrorActionPreference = 'Stop'
$expectedBuild = '25247556'
$expectedAppId = '3681810'
$probe = Join-Path $PSScriptRoot 'rlogs-bpsr-automarker-lifecycle-probe.exe'
if (-not (Test-Path -LiteralPath $probe -PathType Leaf)) {
    throw 'The probe executable is missing from this package.'
}
if ($DryRun) {
    if ($ArmReversibleCalibration -or $ArmSinglePlannerStep) {
        throw '-DryRun cannot be combined with an armed mode.'
    }
    $dryStamp = [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssfffZ')
    $dryReceipt = Join-Path $PSScriptRoot "automarker-dry-run-$dryStamp.v1.json"
    $value = [ordered]@{
        schemaVersion = 1
        evidenceKind = 'automarker-v9-packaged-dry-run'
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

function Get-AcfValue([string]$Text, [string]$Key) {
    $match = [regex]::Match($Text, '(?im)^\s*"' + [regex]::Escape($Key) + '"\s+"([^"]*)"')
    if ($match.Success) { return $match.Groups[1].Value }
    return $null
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
$receipt = Join-Path $PSScriptRoot "automarker-lifecycle-$stamp.v5.json"
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
if ($ArmReversibleCalibration -and $ArmSinglePlannerStep) {
    throw 'Choose only one armed canary mode.'
}
if ($ArmReversibleCalibration) {
    Write-Warning 'ARMED CALIBRATION: manually select Marker 1 and keep the game focused. This emits +X, -X, +Y, -Y six-pixel mouse moves and Escape. It never clicks.'
    Write-Host 'Return focus to the game now. The fail-closed canary starts in 5 seconds.'
    foreach ($remaining in 5..1) {
        Write-Host "$remaining..."
        Start-Sleep -Seconds 1
    }
    $arguments += @('--armed-mode', 'marker1-reversible-calibration-v1')
}
if ($ArmSinglePlannerStep) {
    if ($null -eq $TargetX -or $null -eq $TargetY -or $null -eq $TargetZ) {
        throw '-TargetX, -TargetY, and -TargetZ are required for -ArmSinglePlannerStep.'
    }
    foreach ($coordinate in @($TargetX.Value, $TargetY.Value, $TargetZ.Value)) {
        if ([double]::IsNaN($coordinate) -or [double]::IsInfinity($coordinate)) {
            throw 'Planner target coordinates must be finite.'
        }
    }
    if ([string]::IsNullOrWhiteSpace($RLogsBaseUrl) -or $RLogsBaseUrl -notmatch '^http://127\.0\.0\.1:\d+$') {
        throw '-RLogsBaseUrl must be the active loopback rLogs host, for example http://127.0.0.1:54221.'
    }
    Write-Warning 'ARMED ONE-STEP CANARY: manually select Marker 1 and keep the game focused. This calibrates, moves at most 4 pixels once, applies the exact inverse, then Escape. It never clicks or places.'
    Write-Host 'Return focus to the game now. The fail-closed canary starts in 5 seconds.'
    foreach ($remaining in 5..1) {
        Write-Host "$remaining..."
        Start-Sleep -Seconds 1
    }
    $arguments += @(
        '--armed-mode', 'marker1-single-planner-step-and-restore-v1',
        '--target-x', $TargetX.Value.ToString('R', [Globalization.CultureInfo]::InvariantCulture),
        '--target-y', $TargetY.Value.ToString('R', [Globalization.CultureInfo]::InvariantCulture),
        '--target-z', $TargetZ.Value.ToString('R', [Globalization.CultureInfo]::InvariantCulture),
        '--rlogs-base-url', $RLogsBaseUrl
    )
}

& $probe @arguments
if ($LASTEXITCODE -ne 0) { throw 'The exact-build probe or calibration canary failed closed.' }
Write-Host "Created sanitized receipt $(Split-Path -Leaf $receipt). Marker confirmation and Place remain disabled."
