[CmdletBinding()]
param()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Get-TestSha256([string]$Path) {
    $stream = [System.IO.File]::OpenRead($Path)
    $algorithm = [System.Security.Cryptography.SHA256]::Create()
    try { ([BitConverter]::ToString($algorithm.ComputeHash($stream))).Replace('-', '').ToLowerInvariant() }
    finally { $algorithm.Dispose(); $stream.Dispose() }
}

$launcher = Join-Path $PSScriptRoot 'capture-marker-audit.ps1'
$repoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
$pack = Join-Path $repoRoot 'plugins\games\blue-protocol-star-resonance\protocol-packs\global\steam-24687926\pack.json'
$productionManifest = Join-Path $repoRoot 'plugins\games\blue-protocol-star-resonance\research\game-file-inventory\global\steam-24687926\installed-client-file-manifest.v1.json'
$fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) "rlogs-marker-capture-test-$PID"
[System.IO.Directory]::CreateDirectory($fixtureRoot) | Out-Null
try {
    $testExecutable = (Get-Process -Id $PID).Path
    $testExecutableFile = Get-Item -LiteralPath $testExecutable
    $testExecutableHash = Get-TestSha256 $testExecutable
    $manifestPath = Join-Path $fixtureRoot 'test-only-build-manifest.json'
    $testManifest = [ordered]@{schemaVersion=1;game='capture-harness-test-fixture';authority=[ordered]@{test_only=$true};files=@([ordered]@{relativePath=$testExecutableFile.Name;bytes=$testExecutableFile.Length;sha256=$testExecutableHash})}
    [System.IO.File]::WriteAllText($manifestPath, ($testManifest|ConvertTo-Json -Depth 10), (New-Object System.Text.UTF8Encoding($false)))

    $planPath = Join-Path $fixtureRoot 'plan.json'
    $actions = 1..6 | ForEach-Object {[ordered]@{marker_number=$_;marker_identity=[ordered]@{slot_number=$_;icon_id="marker-$_";icon_name="Marker $_"};expected_action="place marker $_";target=[ordered]@{kind='ground';coordinates=[ordered]@{x=[double]$_;y=2.0;z=3.0}}}}
    $plan = [ordered]@{schema_version=1;scene_id=1633;scene_name='Tina M1';initiating_character=[ordered]@{character_id='test-character';entity_uuid='test-entity'};actions=$actions}
    [System.IO.File]::WriteAllText($planPath, ($plan|ConvertTo-Json -Depth 10), (New-Object System.Text.UTF8Encoding($false)))
    $common = @{OutputDirectory=$fixtureRoot;CaptureId='marker-session-dry-run';ClientIp='192.0.2.10';Interface='test interface with spaces';GameBuild='24687926';GameExecutablePath=$testExecutable;BuildFileManifestPath=$manifestPath;ProtocolPackPath=$pack;ProtocolPackDigest='sha256:4372050d9d549808b229b16de315080f9bac427efe9602dabd9b93c4502dbbae';SceneId=1633;SceneName='Tina M1';ActionPlanPath=$planPath;DurationSeconds=90;TestOnlyAllowIdentityFixture=$true;DryRun=$true}
    $dryRun = & $launcher @common | ConvertFrom-Json
    if ($dryRun.capture_filter -ne 'host 192.0.2.10' -or $dryRun.transport_mode -ne 'all-ip') { throw 'Marker harness lost its explicit-client all-IP boundary.' }
    if (-not $dryRun.starts_before_placement) { throw 'Marker harness no longer starts before placement.' }
    if (($dryRun.planned_marker_order -join ',') -ne '1,2,3,4,5,6') { throw 'Marker order changed.' }
    if ($dryRun.initiating_character.character_id -ne 'test-character') { throw 'Initiating character was not retained.' }
    if ($dryRun.action_plan.sha256 -ne (Get-TestSha256 $planPath)) { throw 'Action plan snapshot/hash was not bound.' }
    if ($dryRun.action_plan.snapshot.actions[0].expected_action -ne 'place marker 1' -or $dryRun.action_plan.snapshot.actions[0].marker_identity.icon_id -ne 'marker-1') { throw 'Expected action or marker/icon identity was not retained in the frozen plan snapshot.' }
    if ($dryRun.identity.executable_sha256 -ne $testExecutableHash -or -not $dryRun.identity.test_only_identity_fixture) { throw 'Explicit test executable authority was not retained.' }
    if ($dryRun.identity.protocol_pack_digest -ne $common.ProtocolPackDigest) { throw 'Protocol digest was not retained.' }
    if ($dryRun.capture_command.windows_command_line -notmatch '"test interface with spaces"') { throw 'PS5.1-safe command-line quoting lost the spaced interface argument.' }

    $singleMarkerPlanPath = Join-Path $fixtureRoot 'single-marker-plan.json'
    $singleMarkerPlan = [ordered]@{schema_version=1;scene_id=1633;scene_name='Tina M1';initiating_character=$plan.initiating_character;actions=@($actions[0])}
    [System.IO.File]::WriteAllText($singleMarkerPlanPath, ($singleMarkerPlan|ConvertTo-Json -Depth 10), (New-Object System.Text.UTF8Encoding($false)))
    $singleMarkerCommon = $common.Clone()
    $singleMarkerCommon.ActionPlanPath = $singleMarkerPlanPath
    $singleMarkerDryRun = & $launcher @singleMarkerCommon | ConvertFrom-Json
    if (($singleMarkerDryRun.planned_marker_order -join ',') -ne '1') { throw 'Single-marker proof plan was not accepted.' }

    $emptyPlanPath = Join-Path $fixtureRoot 'empty-plan.json'
    [System.IO.File]::WriteAllText($emptyPlanPath, ([ordered]@{schema_version=1;scene_id=1633;scene_name='Tina M1';initiating_character=$plan.initiating_character;actions=@()}|ConvertTo-Json -Depth 10), (New-Object System.Text.UTF8Encoding($false)))
    $emptyPlanCommon = $common.Clone()
    $emptyPlanCommon.ActionPlanPath = $emptyPlanPath
    $emptyPlanRejected = $false
    try { & $launcher @emptyPlanCommon | Out-Null } catch { $emptyPlanRejected = $_.Exception.Message -like 'Marker action plan must contain 1 through 6 actions*' }
    if (-not $emptyPlanRejected) { throw 'Empty marker plan was not rejected.' }

    $tooManyPlanPath = Join-Path $fixtureRoot 'too-many-plan.json'
    $tooManyActions = @($actions) + @([ordered]@{marker_number=7;marker_identity=[ordered]@{slot_number=7;icon_id='marker-7';icon_name='Marker 7'};expected_action='place marker 7';target=[ordered]@{kind='ground';coordinates=[ordered]@{x=7.0;y=2.0;z=3.0}}})
    [System.IO.File]::WriteAllText($tooManyPlanPath, ([ordered]@{schema_version=1;scene_id=1633;scene_name='Tina M1';initiating_character=$plan.initiating_character;actions=$tooManyActions}|ConvertTo-Json -Depth 10), (New-Object System.Text.UTF8Encoding($false)))
    $tooManyPlanCommon = $common.Clone()
    $tooManyPlanCommon.ActionPlanPath = $tooManyPlanPath
    $tooManyPlanRejected = $false
    try { & $launcher @tooManyPlanCommon | Out-Null } catch { $tooManyPlanRejected = $_.Exception.Message -like 'Marker action plan must contain 1 through 6 actions*' }
    if (-not $tooManyPlanRejected) { throw 'Seven-marker plan was not rejected.' }

    $badPlanPath = Join-Path $fixtureRoot 'bad-plan.json'
    $actions[5].marker_number = 5
    [System.IO.File]::WriteAllText($badPlanPath, ([ordered]@{schema_version=1;scene_id=1633;scene_name='Tina M1';initiating_character=$plan.initiating_character;actions=$actions}|ConvertTo-Json -Depth 10), (New-Object System.Text.UTF8Encoding($false)))
    $badPlanCommon = $common.Clone()
    $badPlanCommon.ActionPlanPath = $badPlanPath
    $rejected = $false
    try { & $launcher @badPlanCommon | Out-Null } catch { $rejected = $_.Exception.Message -like 'Marker action 6 must declare marker_number 6*' }
    if (-not $rejected) { throw 'Out-of-order marker plan was not rejected.' }

    $missingCharacterPath = Join-Path $fixtureRoot 'missing-character-plan.json'
    $missingCharacterPlan = Get-Content -LiteralPath $planPath -Raw | ConvertFrom-Json
    $missingCharacterPlan.initiating_character.character_id = ''
    [System.IO.File]::WriteAllText($missingCharacterPath, ($missingCharacterPlan|ConvertTo-Json -Depth 10), (New-Object System.Text.UTF8Encoding($false)))
    $missingCharacterCommon = $common.Clone()
    $missingCharacterCommon.ActionPlanPath = $missingCharacterPath
    $missingCharacterRejected = $false
    try { & $launcher @missingCharacterCommon | Out-Null } catch { $missingCharacterRejected = $_.Exception.Message -like 'Marker action plan requires initiating_character.character_id*' }
    if (-not $missingCharacterRejected) { throw 'Plan without an initiating character was not rejected.' }

    $missingEntityUuidPath = Join-Path $fixtureRoot 'missing-entity-uuid-plan.json'
    $missingEntityUuidPlan = Get-Content -LiteralPath $planPath -Raw | ConvertFrom-Json
    $missingEntityUuidPlan.initiating_character.entity_uuid = '   '
    [System.IO.File]::WriteAllText($missingEntityUuidPath, ($missingEntityUuidPlan|ConvertTo-Json -Depth 10), (New-Object System.Text.UTF8Encoding($false)))
    $missingEntityUuidCommon = $common.Clone()
    $missingEntityUuidCommon.ActionPlanPath = $missingEntityUuidPath
    $missingEntityUuidRejected = $false
    try { & $launcher @missingEntityUuidCommon | Out-Null } catch { $missingEntityUuidRejected = $_.Exception.Message -like 'Marker action plan requires initiating_character.entity_uuid*' }
    if (-not $missingEntityUuidRejected) { throw 'Plan without a nonblank initiating entity UUID was not rejected.' }

    $missingExpectedActionPath = Join-Path $fixtureRoot 'missing-expected-action-plan.json'
    $missingExpectedActionPlan = Get-Content -LiteralPath $planPath -Raw | ConvertFrom-Json
    $missingExpectedActionPlan.actions[0].expected_action = ''
    [System.IO.File]::WriteAllText($missingExpectedActionPath, ($missingExpectedActionPlan|ConvertTo-Json -Depth 10), (New-Object System.Text.UTF8Encoding($false)))
    $missingExpectedActionCommon = $common.Clone()
    $missingExpectedActionCommon.ActionPlanPath = $missingExpectedActionPath
    $missingExpectedActionRejected = $false
    try { & $launcher @missingExpectedActionCommon | Out-Null } catch { $missingExpectedActionRejected = $_.Exception.Message -like 'Marker 1 requires expected_action*' }
    if (-not $missingExpectedActionRejected) { throw 'Plan without an expected action was not rejected.' }

    $missingMarkerIdentityPath = Join-Path $fixtureRoot 'missing-marker-identity-plan.json'
    $missingMarkerIdentityPlan = Get-Content -LiteralPath $planPath -Raw | ConvertFrom-Json
    $missingMarkerIdentityPlan.actions[0].marker_identity.icon_id = ''
    $missingMarkerIdentityPlan.actions[0].marker_identity.icon_name = ''
    [System.IO.File]::WriteAllText($missingMarkerIdentityPath, ($missingMarkerIdentityPlan|ConvertTo-Json -Depth 10), (New-Object System.Text.UTF8Encoding($false)))
    $missingMarkerIdentityCommon = $common.Clone()
    $missingMarkerIdentityCommon.ActionPlanPath = $missingMarkerIdentityPath
    $missingMarkerIdentityRejected = $false
    try { & $launcher @missingMarkerIdentityCommon | Out-Null } catch { $missingMarkerIdentityRejected = $_.Exception.Message -like 'Marker 1 identity requires icon_id or icon_name*' }
    if (-not $missingMarkerIdentityRejected) { throw 'Plan without marker/icon identity was not rejected.' }

    $buildMismatchCommon = $common.Clone()
    $buildMismatchCommon.GameBuild = 'wrong-build'
    $mismatch = $false
    try { & $launcher @buildMismatchCommon | Out-Null } catch { $mismatch = $_.Exception.Message -like 'Protocol pack targets build*' }
    if (-not $mismatch) { throw 'Protocol-pack/build mismatch was not rejected.' }
    $digestMismatchCommon = $common.Clone()
    $digestMismatchCommon.ProtocolPackDigest = "sha256:$('0' * 64)"
    $digestMismatch = $false
    try { & $launcher @digestMismatchCommon | Out-Null } catch { $digestMismatch = $_.Exception.Message -like 'Protocol pack digest mismatch*' }
    if (-not $digestMismatch) { throw 'Incorrect protocol-pack digest was not rejected.' }
    $implicitFixtureCommon = $common.Clone()
    $implicitFixtureCommon.TestOnlyAllowIdentityFixture = $false
    $fixtureNotExplicit = $false
    try { & $launcher @implicitFixtureCommon | Out-Null } catch { $fixtureNotExplicit = $_.Exception.Message -like 'Production capture requires the repository-reviewed build/file manifest*' }
    if (-not $fixtureNotExplicit) { throw 'Arbitrary executable fixture passed without explicit test-only authority.' }
    $forgedProductionManifestPath = Join-Path $fixtureRoot 'forged-production-manifest.json'
    $forgedProductionManifest = [ordered]@{schemaVersion=1;game='blue-protocol-star-resonance';deployment='global';channel='steam';gameBuild='24687926';coverage=[ordered]@{complete=$true};authority=[ordered]@{localPhysicalSha256='installed-file-content-proof'};files=@([ordered]@{relativePath=$testExecutableFile.Name;bytes=$testExecutableFile.Length;sha256=$testExecutableHash})}
    [System.IO.File]::WriteAllText($forgedProductionManifestPath, ($forgedProductionManifest|ConvertTo-Json -Depth 10), (New-Object System.Text.UTF8Encoding($false)))
    $forgedProductionCommon = $common.Clone()
    $forgedProductionCommon.TestOnlyAllowIdentityFixture = $false
    $forgedProductionCommon.BuildFileManifestPath = $forgedProductionManifestPath
    $forgedProductionRejected = $false
    try { & $launcher @forgedProductionCommon | Out-Null } catch { $forgedProductionRejected = $_.Exception.Message -like 'Production capture requires the repository-reviewed build/file manifest*' }
    if (-not $forgedProductionRejected) { throw 'Arbitrary production-shaped executable authority escaped the repository-reviewed manifest path.' }
    $productionAuthorityCommon = $common.Clone()
    $productionAuthorityCommon.TestOnlyAllowIdentityFixture = $false
    $productionAuthorityCommon.BuildFileManifestPath = $productionManifest
    $arbitraryExecutableRejected = $false
    try { & $launcher @productionAuthorityCommon | Out-Null } catch { $arbitraryExecutableRejected = $_.Exception.Message -like 'Game executable hash is not uniquely authorized*' }
    if (-not $arbitraryExecutableRejected) { throw 'Arbitrary PowerShell executable passed the reviewed production build/file manifest.' }
    $nonDryRunCommon = $common.Clone()
    $nonDryRunCommon.DryRun = $false
    $nonDryRunFixture = $false
    try { & $launcher @nonDryRunCommon | Out-Null } catch { $nonDryRunFixture = $_.Exception.Message -like 'Test-only identity fixtures are allowed only in dry-run mode*' }
    if (-not $nonDryRunFixture) { throw 'Test-only executable authority escaped dry-run mode.' }
} finally {
    $resolvedFixtureRoot = [System.IO.Path]::GetFullPath($fixtureRoot)
    $resolvedTempRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
    if (-not $resolvedFixtureRoot.StartsWith($resolvedTempRoot, [System.StringComparison]::OrdinalIgnoreCase)) { throw "Refusing to remove test fixture outside the temporary directory: $resolvedFixtureRoot" }
    Remove-Item -LiteralPath $resolvedFixtureRoot -Recurse -Force
}
Write-Host "capture-marker-audit dry-run boundary tests passed under $($PSVersionTable.PSEdition) $($PSVersionTable.PSVersion)."
