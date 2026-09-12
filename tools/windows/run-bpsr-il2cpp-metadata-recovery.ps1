<#
.SYNOPSIS
Runs the exact-build BPSR metadata readiness gate and read-only scanner.

.DESCRIPTION
This command is intentionally locked to global Steam build 25247556. It emits
only a sanitized receipt to stdout and never enables automarker placement.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$InstallRoot,
    [Parameter(Mandatory = $true)][string]$SteamManifestPath,
    [Parameter(Mandatory = $true)][string]$PrivateOutputRoot,
    [string]$ScannerPath,
    [switch]$TestOnlyAllowFixture,
    [string]$TestOnlyProcessInventoryPath,
    [string]$TestOnlyScannerPath,
    [ValidatePattern('^[0-9a-fA-F]{64}$')][string]$TestOnlyExpectedExecutableSha256,
    [ValidatePattern('^[0-9a-fA-F]{64}$')][string]$TestOnlyExpectedGameAssemblySha256
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$Build = '25247556'
$AppId = '3681810'
$Deployment = 'global'
$Channel = 'steam'
$ExecutableSha256 = '90537dbd0e4c9d4b3ed2af06aa8bf7ffe7219fc94bb2f6963673f7b340288588'
$GameAssemblySha256 = '4a079aec0a3e51a8023aa86bbd152e12068907b65b9aafb355d20bb6d6c41fe3'
$MetadataName = 'global-metadata.dat'
$IdentityName = 'metadata-build-identity.v2.json'
$ReceiptName = 'metadata-recovery-receipt.v1.json'

function Get-Sha256([string]$Path) {
    $stream = [System.IO.File]::OpenRead($Path)
    $algorithm = [System.Security.Cryptography.SHA256]::Create()
    try { ([BitConverter]::ToString($algorithm.ComputeHash($stream))).Replace('-', '').ToLowerInvariant() }
    finally { $algorithm.Dispose(); $stream.Dispose() }
}

function Write-NewUtf8([string]$Path, [string]$Contents) {
    $bytes = (New-Object System.Text.UTF8Encoding($false)).GetBytes($Contents)
    $stream = [System.IO.File]::Open($Path, [System.IO.FileMode]::CreateNew, [System.IO.FileAccess]::Write, [System.IO.FileShare]::None)
    try { $stream.Write($bytes, 0, $bytes.Length); $stream.Flush() }
    finally { $stream.Dispose() }
}

$repositoryRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
$readinessGate = Join-Path $PSScriptRoot 'bpsr-il2cpp-metadata-readiness.ps1'
if (-not (Test-Path -LiteralPath $readinessGate -PathType Leaf)) { throw "Readiness gate was not found: $readinessGate" }

$resolvedInstallRoot = [System.IO.Path]::GetFullPath($InstallRoot)
$resolvedManifest = [System.IO.Path]::GetFullPath($SteamManifestPath)
$resolvedPrivateRoot = [System.IO.Path]::GetFullPath($PrivateOutputRoot)
if (-not (Test-Path -LiteralPath $resolvedPrivateRoot -PathType Container)) {
    throw "Private output root was not found: $PrivateOutputRoot"
}
$resolvedExecutable = Join-Path $resolvedInstallRoot 'bpsr\BPSR_STEAM.exe'
$resolvedAssembly = Join-Path $resolvedInstallRoot 'bpsr\GameAssembly.dll'
$metadataOutput = Join-Path $resolvedPrivateRoot $MetadataName
$identityOutput = Join-Path $resolvedPrivateRoot $IdentityName
$receiptOutput = Join-Path $resolvedPrivateRoot $ReceiptName
foreach ($output in @($metadataOutput, $identityOutput, $receiptOutput)) {
    if (Test-Path -LiteralPath $output) { throw "Recovery output already exists; refusing overwrite: $([System.IO.Path]::GetFileName($output))" }
}

$usingTestHook = $TestOnlyAllowFixture.IsPresent
if ($usingTestHook) {
    foreach ($required in @($TestOnlyProcessInventoryPath, $TestOnlyScannerPath, $TestOnlyExpectedExecutableSha256, $TestOnlyExpectedGameAssemblySha256)) {
        if ([string]::IsNullOrWhiteSpace($required)) { throw 'TestOnlyAllowFixture requires process inventory, scanner path, and both fixture hashes.' }
    }
    $ExecutableSha256 = $TestOnlyExpectedExecutableSha256.ToLowerInvariant()
    $GameAssemblySha256 = $TestOnlyExpectedGameAssemblySha256.ToLowerInvariant()
    $resolvedScanner = [System.IO.Path]::GetFullPath($TestOnlyScannerPath)
} else {
    if (-not [string]::IsNullOrWhiteSpace($TestOnlyProcessInventoryPath) -or
        -not [string]::IsNullOrWhiteSpace($TestOnlyScannerPath) -or
        -not [string]::IsNullOrWhiteSpace($TestOnlyExpectedExecutableSha256) -or
        -not [string]::IsNullOrWhiteSpace($TestOnlyExpectedGameAssemblySha256)) {
        throw 'Test-only recovery parameters require TestOnlyAllowFixture.'
    }
    $resolvedScanner = if ([string]::IsNullOrWhiteSpace($ScannerPath)) {
        Join-Path $repositoryRoot 'target\release\rlogs-bpsr-il2cpp-metadata-scan.exe'
    } else { [System.IO.Path]::GetFullPath($ScannerPath) }
}
$readinessParameters = @{
    InstallRoot = $resolvedInstallRoot
    SteamManifestPath = $resolvedManifest
    PrivateOutputRoot = $resolvedPrivateRoot
    ExpectedExecutableSha256 = $ExecutableSha256
    ExpectedGameAssemblySha256 = $GameAssemblySha256
}
if ($usingTestHook) {
    $readinessParameters.TestOnlyAllowProcessFixture = $true
    $readinessParameters.TestOnlyProcessInventoryPath = $TestOnlyProcessInventoryPath
}
$readiness = (& $readinessGate @readinessParameters | Out-String) | ConvertFrom-Json
if ($readiness.status -cne 'readiness-checks-passed-no-scan-authority' -or
    $readiness.exact_identity.build_id -cne $Build -or
    $readiness.exact_identity.app_id -cne $AppId -or
    $readiness.exact_identity.deployment -cne $Deployment -or
    $readiness.exact_identity.channel -cne $Channel -or
    $readiness.exact_identity.executable.sha256 -cne $ExecutableSha256 -or
    $readiness.exact_identity.game_assembly.sha256 -cne $GameAssemblySha256) {
    throw 'Readiness receipt did not preserve the pinned build identity; scanner not invoked.'
}
if (-not (Test-Path -LiteralPath $resolvedScanner -PathType Leaf)) { throw "Metadata scanner was not found after readiness passed: $resolvedScanner" }

$scannerArguments = @(
    '--output', $metadataOutput,
    '--process-name', 'BPSR_STEAM',
    '--scope', 'private',
    '--chunk-mib', '8',
    '--exact-identity', 'true',
    '--process-executable', $resolvedExecutable,
    '--expected-process-executable-sha256', $ExecutableSha256,
    '--game-assembly', $resolvedAssembly,
    '--expected-game-assembly-sha256', $GameAssemblySha256,
    '--build', $Build,
    '--expected-app-id', $AppId,
    '--steam-manifest', $resolvedManifest,
    '--identity-report', $identityOutput,
    '--deployment', $Deployment,
    '--channel', $Channel
)
$scannerOutput = & $resolvedScanner @scannerArguments 2>&1 | Out-String
if (-not $usingTestHook -and $LASTEXITCODE -ne 0) { throw "Metadata scanner failed with exit code $LASTEXITCODE." }
if (-not (Test-Path -LiteralPath $metadataOutput -PathType Leaf) -or
    -not (Test-Path -LiteralPath $identityOutput -PathType Leaf)) {
    throw 'Metadata scanner did not create the complete exact-identity output pair.'
}
$identity = Get-Content -LiteralPath $identityOutput -Raw | ConvertFrom-Json
$metadataBytes = (Get-Item -LiteralPath $metadataOutput).Length
$metadataSha256 = Get-Sha256 $metadataOutput
if ($identity.schema_version -ne 2 -or $identity.generated_by -cne 'rlogs-bpsr-il2cpp-metadata-scan' -or
    $identity.game -cne 'blue-protocol-star-resonance' -or $identity.deployment -cne $Deployment -or
    $identity.channel -cne $Channel -or $identity.game_build -cne $Build -or
    $identity.distribution_app_id -cne $AppId -or
    $identity.metadata.byte_length -ne $metadataBytes -or $identity.metadata.sha256 -cne $metadataSha256 -or
    $identity.game_assembly.sha256 -cne $GameAssemblySha256 -or
    $identity.process_executable.sha256 -cne $ExecutableSha256) {
    throw 'Metadata scanner identity report does not match the pinned readiness identity.'
}

$receipt = [ordered]@{
    schema_version = 1
    status = 'exact-build-metadata-recovered'
    game = 'blue-protocol-star-resonance'
    deployment = $Deployment
    channel = $Channel
    app_id = $AppId
    build_id = $Build
    metadata = [ordered]@{file_name=$MetadataName;byte_length=$metadataBytes;sha256=$metadataSha256;metadata_version=$identity.metadata.metadata_version}
    game_assembly = [ordered]@{byte_length=$identity.game_assembly.byte_length;sha256=$GameAssemblySha256}
    process_executable = [ordered]@{byte_length=$identity.process_executable.byte_length;sha256=$ExecutableSha256}
    outputs = [ordered]@{identity_file_name=$IdentityName;receipt_file_name=$ReceiptName;private_output_outside_repository=$true;overwrite_performed=$false}
    safety = [ordered]@{readiness_passed_first=$true;scanner_invocations=1;scanner_scope='validated IL2CPP metadata image only';game_modified=$false;network_access=$false;automarker_place_enabled=$false;native_transport_send_performed=$false}
    privacy = [ordered]@{absolute_paths_disclosed=$false;process_id_disclosed=$false;memory_address_disclosed=$false;scanner_stdout_disclosed=$false}
}
$receiptJson = $receipt | ConvertTo-Json -Depth 10
Write-NewUtf8 $receiptOutput ($receiptJson + [Environment]::NewLine)
$receiptJson
