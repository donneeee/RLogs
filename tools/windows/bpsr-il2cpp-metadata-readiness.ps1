<#
.SYNOPSIS
Fail-closed, no-attach readiness gate for exact BPSR Steam build 25247556.

.DESCRIPTION
Validates the Steam receipt, reviewed executable hashes, unique running-process
name/path, zero-byte installed metadata placeholder, and a private output root.
It does not open a process handle, read process memory, create output, invoke
the metadata scanner, access the network, modify the game, or enable Place.

Both executable digests are pinned from the reviewed local Steam receipt for
build 25247556. A future build must update this gate rather than carrying these
values forward silently.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$InstallRoot,
    [Parameter(Mandatory = $true)][string]$SteamManifestPath,
    [Parameter(Mandatory = $true)][string]$PrivateOutputRoot,
    [ValidatePattern('^[0-9a-fA-F]{64}$')][string]$ExpectedExecutableSha256 = '90537dbd0e4c9d4b3ed2af06aa8bf7ffe7219fc94bb2f6963673f7b340288588',
    [ValidatePattern('^[0-9a-fA-F]{64}$')][string]$ExpectedGameAssemblySha256 = '4a079aec0a3e51a8023aa86bbd152e12068907b65b9aafb355d20bb6d6c41fe3',
    [ValidateSet('25247556')][string]$ExpectedBuild = '25247556',
    [ValidateSet('3681810')][string]$ExpectedAppId = '3681810',
    [ValidatePattern('^BPSR_STEAM(?:\.exe)?$')][string]$ProcessName = 'BPSR_STEAM',
    [ValidatePattern('^[A-Za-z0-9._-]+$')][string]$OutputFileName = 'global-metadata.dat',
    [switch]$TestOnlyAllowProcessFixture,
    [string]$TestOnlyProcessInventoryPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Require-File([string]$Path, [string]$Description) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "$Description was not found: $Path" }
    [System.IO.Path]::GetFullPath($Path)
}

function Get-Sha256([string]$Path) {
    $stream = [System.IO.File]::OpenRead($Path)
    $algorithm = [System.Security.Cryptography.SHA256]::Create()
    try { ([BitConverter]::ToString($algorithm.ComputeHash($stream))).Replace('-', '').ToLowerInvariant() }
    finally { $algorithm.Dispose(); $stream.Dispose() }
}

function Get-AcfValue([string]$Contents, [string]$Key) {
    foreach ($line in ($Contents -split "`r?`n")) {
        $match = [regex]::Match($line, '^\s*"([^"]+)"\s+"([^"]*)"\s*$')
        if ($match.Success -and $match.Groups[1].Value.Equals($Key, [System.StringComparison]::OrdinalIgnoreCase)) {
            return $match.Groups[2].Value
        }
    }
    $null
}

function Test-PathWithin([string]$Candidate, [string]$Root) {
    $separator = [System.IO.Path]::DirectorySeparatorChar
    $normalizedRoot = [System.IO.Path]::GetFullPath($Root).TrimEnd($separator) + $separator
    $normalizedCandidate = [System.IO.Path]::GetFullPath($Candidate)
    $normalizedCandidate.StartsWith($normalizedRoot, [System.StringComparison]::OrdinalIgnoreCase)
}

$resolvedInstallRoot = [System.IO.Path]::GetFullPath($InstallRoot)
if (-not (Test-Path -LiteralPath $resolvedInstallRoot -PathType Container)) { throw "Install root was not found: $InstallRoot" }
$resolvedManifest = Require-File $SteamManifestPath 'Steam app manifest'
$resolvedExecutable = Require-File (Join-Path $resolvedInstallRoot 'bpsr\BPSR_STEAM.exe') 'Game executable'
$resolvedAssembly = Require-File (Join-Path $resolvedInstallRoot 'bpsr\GameAssembly.dll') 'GameAssembly'
$resolvedMetadata = Require-File (Join-Path $resolvedInstallRoot 'bpsr\BPSR_STEAM_Data\il2cpp_data\Metadata\global-metadata.dat') 'Installed metadata placeholder'

$manifestContents = [System.IO.File]::ReadAllText($resolvedManifest)
$manifestBuild = Get-AcfValue $manifestContents 'buildid'
$manifestAppId = Get-AcfValue $manifestContents 'appid'
if ($manifestBuild -cne $ExpectedBuild) { throw "Steam manifest buildid $manifestBuild does not match required build $ExpectedBuild." }
if ($manifestAppId -cne $ExpectedAppId) { throw "Steam manifest appid $manifestAppId does not match required app $ExpectedAppId." }

$actualExecutableSha256 = Get-Sha256 $resolvedExecutable
$actualAssemblySha256 = Get-Sha256 $resolvedAssembly
if ($actualExecutableSha256 -cne $ExpectedExecutableSha256.ToLowerInvariant()) { throw 'BPSR_STEAM.exe SHA-256 does not match the explicitly reviewed digest.' }
if ($actualAssemblySha256 -cne $ExpectedGameAssemblySha256.ToLowerInvariant()) { throw 'GameAssembly.dll SHA-256 does not match exact build 25247556.' }
if ((Get-Item -LiteralPath $resolvedMetadata).Length -ne 0) { throw 'Installed global-metadata.dat is no longer the reviewed zero-byte placeholder; stop and re-audit the build.' }

$repositoryRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
$resolvedPrivateRoot = [System.IO.Path]::GetFullPath($PrivateOutputRoot)
if (Test-PathWithin $resolvedPrivateRoot $repositoryRoot -or $resolvedPrivateRoot.Equals($repositoryRoot, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw 'Private output root must be outside the repository.'
}
$plannedOutput = Join-Path $resolvedPrivateRoot $OutputFileName
if (-not (Test-PathWithin $plannedOutput $resolvedPrivateRoot)) { throw 'Planned metadata output escaped the private output root.' }
if (Test-Path -LiteralPath $plannedOutput) { throw 'Planned metadata output already exists; readiness never authorizes overwrite.' }

if ($TestOnlyAllowProcessFixture) {
    if ([string]::IsNullOrWhiteSpace($TestOnlyProcessInventoryPath)) { throw 'Test-only process fixture requires TestOnlyProcessInventoryPath.' }
    $fixturePath = Require-File $TestOnlyProcessInventoryPath 'Test-only process inventory'
    $fixture = Get-Content -LiteralPath $fixturePath -Raw | ConvertFrom-Json
    if ($fixture.authority.test_only -ne $true -or $fixture.schema_version -ne 1) { throw 'Test-only process fixture lacks explicit test authority.' }
    $processes = @($fixture.processes)
} else {
    if (-not [string]::IsNullOrWhiteSpace($TestOnlyProcessInventoryPath)) { throw 'TestOnlyProcessInventoryPath requires TestOnlyAllowProcessFixture.' }
    $processes = @(Get-CimInstance Win32_Process -Filter "Name = 'BPSR_STEAM.exe'" | Select-Object Name, ExecutablePath)
}
$expectedProcessName = 'BPSR_STEAM.exe'
$matchingProcesses = @($processes | Where-Object {
    ([string]$_.Name).Equals($expectedProcessName, [System.StringComparison]::OrdinalIgnoreCase) -and
    -not [string]::IsNullOrWhiteSpace([string]$_.ExecutablePath) -and
    ([System.IO.Path]::GetFullPath([string]$_.ExecutablePath)).Equals($resolvedExecutable, [System.StringComparison]::OrdinalIgnoreCase)
})
if ($matchingProcesses.Count -ne 1) { throw "Expected exactly one $expectedProcessName process at the reviewed executable path; found $($matchingProcesses.Count)." }

$result = [ordered]@{
    schema_version = 1
    status = 'readiness-checks-passed-no-scan-authority'
    exact_identity = [ordered]@{
        game = 'blue-protocol-star-resonance'
        deployment = 'global'
        channel = 'steam'
        app_id = $ExpectedAppId
        build_id = $ExpectedBuild
        process_name = $expectedProcessName
        unique_process_at_reviewed_path = $true
        executable = [ordered]@{bytes=(Get-Item -LiteralPath $resolvedExecutable).Length;sha256=$actualExecutableSha256}
        game_assembly = [ordered]@{bytes=(Get-Item -LiteralPath $resolvedAssembly).Length;sha256=$actualAssemblySha256}
        installed_metadata_placeholder_bytes = 0
    }
    privacy = [ordered]@{
        private_output_outside_repository = $true
        planned_output_file_name = $OutputFileName
        absolute_paths_disclosed = $false
        process_id_disclosed = $false
        memory_address_disclosed = $false
    }
    safety = [ordered]@{
        readiness_only = $true
        scanner_invoked = $false
        scan_authorized = $false
        process_handle_opened = $false
        process_memory_read = $false
        files_written = $false
        network_access = $false
        game_modified = $false
        place_enabled = $false
    }
}

$result | ConvertTo-Json -Depth 10
