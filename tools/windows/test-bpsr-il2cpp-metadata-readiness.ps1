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

function Write-Utf8([string]$Path, [string]$Contents) {
    [System.IO.File]::WriteAllText($Path, $Contents, (New-Object System.Text.UTF8Encoding($false)))
}

$gate = Join-Path $PSScriptRoot 'bpsr-il2cpp-metadata-readiness.ps1'
$fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) "rlogs-il2cpp-readiness-$([Guid]::NewGuid().ToString('N'))"
$installRoot = Join-Path $fixtureRoot 'install'
$privateRoot = Join-Path $fixtureRoot 'private-output'
$gameRoot = Join-Path $installRoot 'bpsr'
$metadataDirectory = Join-Path $gameRoot 'BPSR_STEAM_Data\il2cpp_data\Metadata'
[System.IO.Directory]::CreateDirectory($metadataDirectory) | Out-Null
[System.IO.Directory]::CreateDirectory($privateRoot) | Out-Null

try {
    $executable = Join-Path $gameRoot 'BPSR_STEAM.exe'
    $assembly = Join-Path $gameRoot 'GameAssembly.dll'
    $metadata = Join-Path $metadataDirectory 'global-metadata.dat'
    $manifest = Join-Path $fixtureRoot 'appmanifest_3681810.acf'
    $processFixture = Join-Path $fixtureRoot 'processes.json'
    Write-Utf8 $executable 'test executable bytes'
    Write-Utf8 $assembly 'test assembly bytes'
    Write-Utf8 $metadata ''
    Write-Utf8 $manifest "`"AppState`"`r`n{`r`n`t`"appid`"`t`t`"3681810`"`r`n`t`"buildid`"`t`t`"25247556`"`r`n}"
    Write-Utf8 $processFixture (([ordered]@{schema_version=1;authority=[ordered]@{test_only=$true};processes=@([ordered]@{Name='BPSR_STEAM.exe';ExecutablePath=$executable})} | ConvertTo-Json -Depth 5))

    $common = @{
        InstallRoot=$installRoot
        SteamManifestPath=$manifest
        PrivateOutputRoot=$privateRoot
        ExpectedExecutableSha256=(Get-TestSha256 $executable)
        ExpectedGameAssemblySha256=(Get-TestSha256 $assembly)
        TestOnlyAllowProcessFixture=$true
        TestOnlyProcessInventoryPath=$processFixture
    }
    $result = & $gate @common | ConvertFrom-Json
    if ($result.status -ne 'readiness-checks-passed-no-scan-authority') { throw 'Readiness did not pass a valid exact-build fixture.' }
    if (-not $result.safety.readiness_only -or $result.safety.scanner_invoked -or $result.safety.scan_authorized -or $result.safety.process_handle_opened -or $result.safety.process_memory_read -or $result.safety.files_written -or $result.safety.network_access -or $result.safety.game_modified -or $result.safety.place_enabled) { throw 'Readiness safety receipt changed.' }
    if ($result.privacy.absolute_paths_disclosed -or $result.privacy.process_id_disclosed -or $result.privacy.memory_address_disclosed) { throw 'Readiness receipt disclosed private identity data.' }
    if (Test-Path -LiteralPath (Join-Path $privateRoot 'global-metadata.dat')) { throw 'Readiness wrote the planned metadata output.' }

    $badAssembly = $common.Clone()
    $badAssembly.ExpectedGameAssemblySha256 = '0' * 64
    $assemblyRejected = $false
    try { & $gate @badAssembly | Out-Null } catch { $assemblyRejected = $_.Exception.Message -like 'GameAssembly.dll SHA-256 does not match*' }
    if (-not $assemblyRejected) { throw 'Mismatched GameAssembly digest was accepted.' }

    $badExecutable = $common.Clone()
    $badExecutable.ExpectedExecutableSha256 = '0' * 64
    $executableRejected = $false
    try { & $gate @badExecutable | Out-Null } catch { $executableRejected = $_.Exception.Message -like 'BPSR_STEAM.exe SHA-256 does not match*' }
    if (-not $executableRejected) { throw 'Mismatched executable digest was accepted.' }

    $insideRepository = $common.Clone()
    $insideRepository.PrivateOutputRoot = Join-Path $PSScriptRoot 'private-output'
    $privacyRejected = $false
    try { & $gate @insideRepository | Out-Null } catch { $privacyRejected = $_.Exception.Message -like 'Private output root must be outside the repository*' }
    if (-not $privacyRejected) { throw 'Repository-local private output was accepted.' }

    Write-Utf8 $processFixture (([ordered]@{schema_version=1;authority=[ordered]@{test_only=$true};processes=@(
        [ordered]@{Name='BPSR_STEAM.exe';ExecutablePath=$executable},
        [ordered]@{Name='BPSR_STEAM.exe';ExecutablePath=$executable}
    )} | ConvertTo-Json -Depth 5))
    $duplicateRejected = $false
    try { & $gate @common | Out-Null } catch { $duplicateRejected = $_.Exception.Message -like 'Expected exactly one BPSR_STEAM.exe process*' }
    if (-not $duplicateRejected) { throw 'Ambiguous process identity was accepted.' }
} finally {
    $resolvedFixtureRoot = [System.IO.Path]::GetFullPath($fixtureRoot)
    $resolvedTempRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
    if (-not $resolvedFixtureRoot.StartsWith($resolvedTempRoot, [System.StringComparison]::OrdinalIgnoreCase)) { throw "Refusing to remove test fixture outside the temporary directory: $resolvedFixtureRoot" }
    Remove-Item -LiteralPath $resolvedFixtureRoot -Recurse -Force
}

Write-Host "IL2CPP metadata readiness boundary tests passed under $($PSVersionTable.PSEdition) $($PSVersionTable.PSVersion)."
