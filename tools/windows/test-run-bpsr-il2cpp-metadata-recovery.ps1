[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Write-Utf8([string]$Path, [string]$Contents) {
    [System.IO.File]::WriteAllText($Path, $Contents, (New-Object System.Text.UTF8Encoding($false)))
}

function Get-TestSha256([string]$Path) {
    $stream = [System.IO.File]::OpenRead($Path)
    $algorithm = [System.Security.Cryptography.SHA256]::Create()
    try { ([BitConverter]::ToString($algorithm.ComputeHash($stream))).Replace('-', '').ToLowerInvariant() }
    finally { $algorithm.Dispose(); $stream.Dispose() }
}

$runner = Join-Path $PSScriptRoot 'run-bpsr-il2cpp-metadata-recovery.ps1'
$fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) "rlogs-metadata-runner-$([Guid]::NewGuid().ToString('N'))"
$installRoot = Join-Path $fixtureRoot 'install'
$privateRoot = Join-Path $fixtureRoot 'private'
$gameRoot = Join-Path $installRoot 'bpsr'
$metadataDirectory = Join-Path $gameRoot 'BPSR_STEAM_Data\il2cpp_data\Metadata'
[System.IO.Directory]::CreateDirectory($metadataDirectory) | Out-Null
[System.IO.Directory]::CreateDirectory($privateRoot) | Out-Null

try {
    $executable = Join-Path $gameRoot 'BPSR_STEAM.exe'
    $assembly = Join-Path $gameRoot 'GameAssembly.dll'
    $installedMetadata = Join-Path $metadataDirectory 'global-metadata.dat'
    $manifest = Join-Path $fixtureRoot 'appmanifest_3681810.acf'
    $processFixture = Join-Path $fixtureRoot 'processes.json'
    $scanner = Join-Path $fixtureRoot 'fake-scanner.ps1'
    $invocations = Join-Path $fixtureRoot 'scanner-invocations.jsonl'
    Write-Utf8 $executable 'fixture executable'
    Write-Utf8 $assembly 'fixture assembly'
    Write-Utf8 $installedMetadata ''
    Write-Utf8 $manifest "`"AppState`"`r`n{`r`n`t`"appid`"`t`t`"3681810`"`r`n`t`"buildid`"`t`t`"25247556`"`r`n}"
    Write-Utf8 $processFixture (([ordered]@{schema_version=1;authority=[ordered]@{test_only=$true};processes=@([ordered]@{Name='BPSR_STEAM.exe';ExecutablePath=$executable})} | ConvertTo-Json -Depth 5))
    Write-Utf8 $scanner @'
param([Parameter(ValueFromRemainingArguments=$true)][string[]]$ScannerArgs)
$options = @{}
for ($index = 0; $index -lt $ScannerArgs.Count; $index += 2) { $options[$ScannerArgs[$index]] = $ScannerArgs[$index + 1] }
[System.IO.File]::AppendAllText($env:RLOGS_TEST_INVOCATIONS, (($ScannerArgs | ConvertTo-Json -Compress) + [Environment]::NewLine))
$metadata = [System.Text.Encoding]::UTF8.GetBytes('validated fixture metadata')
[System.IO.File]::WriteAllBytes($options['--output'], $metadata)
$sha = [BitConverter]::ToString([System.Security.Cryptography.SHA256]::Create().ComputeHash($metadata)).Replace('-', '').ToLowerInvariant()
$identity = [ordered]@{schema_version=2;generated_by='rlogs-bpsr-il2cpp-metadata-scan';game='blue-protocol-star-resonance';deployment=$options['--deployment'];channel=$options['--channel'];game_build=$options['--build'];distribution_app_id=$options['--expected-app-id'];metadata=[ordered]@{byte_length=$metadata.Length;sha256=$sha;metadata_version=31};game_assembly=[ordered]@{byte_length=(Get-Item $options['--game-assembly']).Length;sha256=$options['--expected-game-assembly-sha256']};process_executable=[ordered]@{byte_length=(Get-Item $options['--process-executable']).Length;sha256=$options['--expected-process-executable-sha256']};steam_manifest=[ordered]@{byte_length=(Get-Item $options['--steam-manifest']).Length;sha256='fixture'}}
[System.IO.File]::WriteAllText($options['--identity-report'], ($identity | ConvertTo-Json -Depth 8), (New-Object System.Text.UTF8Encoding($false)))
'sensitive fake scanner stdout: pid=123 address=0xABC path=C:\private'
'@
    $env:RLOGS_TEST_INVOCATIONS = $invocations
    $common = @{
        InstallRoot=$installRoot;SteamManifestPath=$manifest;PrivateOutputRoot=$privateRoot
        TestOnlyAllowFixture=$true;TestOnlyProcessInventoryPath=$processFixture;TestOnlyScannerPath=$scanner
        TestOnlyExpectedExecutableSha256=(Get-TestSha256 $executable)
        TestOnlyExpectedGameAssemblySha256=(Get-TestSha256 $assembly)
    }

    $failed = $common.Clone()
    $failed.TestOnlyExpectedGameAssemblySha256 = '0' * 64
    try { & $runner @failed | Out-Null } catch { }
    if (Test-Path -LiteralPath $invocations) { throw 'Readiness failure invoked the scanner.' }

    $missingPrivateRoot = $common.Clone()
    $missingPrivateRoot.PrivateOutputRoot = Join-Path $fixtureRoot 'missing-private-root'
    $missingRootRejected = $false
    try { & $runner @missingPrivateRoot | Out-Null } catch { $missingRootRejected = $_.Exception.Message -like 'Private output root was not found*' }
    if (-not $missingRootRejected -or (Test-Path -LiteralPath $invocations)) { throw 'Missing private output root did not fail before scanner invocation.' }

    $rawReceipt = & $runner @common | Out-String
    if ($rawReceipt -match 'pid=123|0xABC|C:\\private') { throw 'Runner disclosed scanner stdout.' }
    $receipt = $rawReceipt | ConvertFrom-Json
    $calls = @(Get-Content -LiteralPath $invocations)
    if ($calls.Count -ne 1) { throw "Expected one scanner invocation; found $($calls.Count)." }
    $arguments = $calls[0] | ConvertFrom-Json
    $expected = @('--output',(Join-Path $privateRoot 'global-metadata.dat'),'--process-name','BPSR_STEAM','--scope','private','--chunk-mib','8','--exact-identity','true','--process-executable',$executable,'--expected-process-executable-sha256',(Get-TestSha256 $executable),'--game-assembly',$assembly,'--expected-game-assembly-sha256',(Get-TestSha256 $assembly),'--build','25247556','--expected-app-id','3681810','--steam-manifest',$manifest,'--identity-report',(Join-Path $privateRoot 'metadata-build-identity.v2.json'),'--deployment','global','--channel','steam')
    if (($arguments | ConvertTo-Json -Compress) -cne ($expected | ConvertTo-Json -Compress)) { throw 'Scanner arguments were not exact.' }
    if ($receipt.privacy.absolute_paths_disclosed -or $receipt.privacy.process_id_disclosed -or $receipt.privacy.memory_address_disclosed -or $receipt.privacy.scanner_stdout_disclosed) { throw 'Sanitized receipt privacy contract changed.' }
    if ($receipt.safety.scanner_invocations -ne 1 -or $receipt.safety.automarker_place_enabled -or $receipt.safety.native_transport_send_performed) { throw 'Recovery safety receipt changed.' }
    $repo = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..')).TrimEnd([System.IO.Path]::DirectorySeparatorChar) + [System.IO.Path]::DirectorySeparatorChar
    foreach ($name in @('global-metadata.dat','metadata-build-identity.v2.json','metadata-recovery-receipt.v1.json')) {
        $output = [System.IO.Path]::GetFullPath((Join-Path $privateRoot $name))
        if (-not $output.StartsWith(([System.IO.Path]::GetFullPath($privateRoot).TrimEnd([System.IO.Path]::DirectorySeparatorChar) + [System.IO.Path]::DirectorySeparatorChar), [System.StringComparison]::OrdinalIgnoreCase) -or $output.StartsWith($repo, [System.StringComparison]::OrdinalIgnoreCase)) { throw "Output escaped private root: $name" }
        if (-not (Test-Path -LiteralPath $output -PathType Leaf)) { throw "Missing recovery output: $name" }
    }
    $overwriteRejected = $false
    try { & $runner @common | Out-Null } catch { $overwriteRejected = $_.Exception.Message -like 'Recovery output already exists*' }
    if (-not $overwriteRejected) { throw 'Existing recovery outputs were not rejected.' }
} finally {
    Remove-Item Env:RLOGS_TEST_INVOCATIONS -ErrorAction SilentlyContinue
    $resolvedFixture = [System.IO.Path]::GetFullPath($fixtureRoot)
    $temp = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
    if (-not $resolvedFixture.StartsWith($temp, [System.StringComparison]::OrdinalIgnoreCase)) { throw "Refusing fixture cleanup outside temp: $resolvedFixture" }
    Remove-Item -LiteralPath $resolvedFixture -Recurse -Force
}

Write-Host 'BPSR IL2CPP metadata recovery runner tests passed.'
