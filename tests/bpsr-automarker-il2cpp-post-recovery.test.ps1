Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
trap { Write-Error $_; exit 1 }

$repoRoot = Split-Path -Parent $PSScriptRoot
. (Join-Path $repoRoot 'tools/bpsr-automarker-il2cpp-post-recovery.ps1') -LibraryOnly

function Assert-True([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}

function Assert-Throws([scriptblock]$Action, [string]$Fragment) {
    try { & $Action; throw "expected failure containing: $Fragment" }
    catch {
        if (-not $_.Exception.Message.Contains($Fragment, [StringComparison]::OrdinalIgnoreCase)) { throw }
    }
}

$tempBase = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
$tempRoot = [IO.Path]::GetFullPath((Join-Path $tempBase "rlogs-automarker-test-$([Guid]::NewGuid().ToString('N'))"))
[IO.Directory]::CreateDirectory($tempRoot) | Out-Null
try {
    $bundle = Join-Path $tempRoot 'bundle'
    $private = Join-Path $tempRoot 'private'
    [IO.Directory]::CreateDirectory($bundle) | Out-Null
    [IO.Directory]::CreateDirectory($private) | Out-Null
    $fakeExe = Join-Path $bundle 'Il2CppDumper.exe'
    $configPath = Join-Path $bundle 'config.json'
    [IO.File]::WriteAllBytes($fakeExe, [byte[]](1, 2, 3, 4))
    $config = [ordered]@{ DumpMethod = $true; GenerateStruct = $false; RequireAnyKey = $true; ForceVersion = 16 }
    [IO.File]::WriteAllText($configPath, (($config | ConvertTo-Json) + "`n"), [Text.UTF8Encoding]::new($false))

    $entries = @(@($fakeExe, $configPath) | ForEach-Object {
        [ordered]@{ path = Split-Path -Leaf $_; length = (Get-Item -LiteralPath $_).Length; sha256 = Get-Sha256 $_ }
    })
    $total = ($entries | ForEach-Object { [long]$_['length'] } | Measure-Object -Sum).Sum
    $manifestPath = Join-Path $tempRoot 'manifest.json'
    $manifest = [ordered]@{
        schema_version = 1; artifact = 'Il2CppDumper'
        provenance = [ordered]@{ repository = 'fixture'; commit = 'fixture-commit'; tree = 'fixture-tree' }
        publish = [ordered]@{ directory_label = 'fixture'; target_framework = 'net8.0'; runtime_identifier = 'win-x64'; self_contained = $true; file_count = 2; total_bytes = $total; files = $entries }
    }
    [IO.File]::WriteAllText($manifestPath, (($manifest | ConvertTo-Json -Depth 10) + "`n"), [Text.UTF8Encoding]::new($false))
    $artifactContract = [ordered]@{
        NormalizedManifestSha256 = Get-NormalizedTextSha256 $manifestPath
        Repository = 'fixture'; Commit = 'fixture-commit'; Tree = 'fixture-tree'; DirectoryLabel = 'fixture'
        FileCount = 2; TotalBytes = [long]$total
    }

    $metadataPath = Join-Path $tempRoot 'metadata.dat'
    $metadata = [byte[]]::new(0x180)
    [BitConverter]::GetBytes([uint32]4205910959).CopyTo($metadata, 0)
    [BitConverter]::GetBytes([int32]31).CopyTo($metadata, 4)
    [IO.File]::WriteAllBytes($metadataPath, $metadata)
    $assemblyPath = Join-Path $tempRoot 'GameAssembly.dll'
    [IO.File]::WriteAllBytes($assemblyPath, [byte[]](0x4d, 0x5a, 5, 6, 7, 8))
    $identityContract = [ordered]@{
        SchemaVersion = 2; GeneratedBy = 'rlogs-bpsr-il2cpp-metadata-scan'; Game = 'blue-protocol-star-resonance'
        Deployment = 'global'; Channel = 'steam'; Build = 'fixture-build'; AppId = '3681810'
        GameAssemblySha256 = Get-Sha256 $assemblyPath
        ProcessExecutableSha256 = '9' * 64; ProcessExecutableLength = 123L
    }
    $receiptPath = Join-Path $tempRoot 'identity.json'
    $identity = [ordered]@{
        schema_version = 2; generated_by = $identityContract.GeneratedBy; game = $identityContract.Game
        deployment = 'global'; channel = 'steam'; game_build = 'fixture-build'; distribution_app_id = '3681810'
        metadata = [ordered]@{ byte_length = (Get-Item $metadataPath).Length; sha256 = Get-Sha256 $metadataPath; metadata_version = 31 }
        game_assembly = [ordered]@{ byte_length = (Get-Item $assemblyPath).Length; sha256 = Get-Sha256 $assemblyPath }
        process_executable = [ordered]@{ byte_length = 123; sha256 = '9' * 64 }
        steam_manifest = [ordered]@{ byte_length = 10; sha256 = '8' * 64 }
    }
    [IO.File]::WriteAllText($receiptPath, (($identity | ConvertTo-Json -Depth 10) + "`n"), [Text.UTF8Encoding]::new($false))
    $fakeRoute = Join-Path $tempRoot 'fake-route.mjs'
    [IO.File]::WriteAllText($fakeRoute, '// validation-only fixture', [Text.UTF8Encoding]::new($false))
    $node = (Get-Command pwsh -CommandType Application).Source

    $fakeChild = Join-Path $tempRoot 'fake child.ps1'
    $fakeChildOutput = Join-Path $tempRoot 'fake child arguments.json'
    $fakeChildSource = @'
param([string]$First, [string]$Second, [string]$Output)
$json = @($First, $Second) | ConvertTo-Json -Compress
[IO.File]::WriteAllText($Output, $json, [Text.UTF8Encoding]::new($false))
'@
    [IO.File]::WriteAllText($fakeChild, $fakeChildSource, [Text.UTF8Encoding]::new($false))
    Invoke-CheckedProcess $node @('-NoLogo', '-NoProfile', '-File', $fakeChild, 'value with spaces', 'literal&metachar', $fakeChildOutput) $tempRoot | Out-Null
    $childArguments = [IO.File]::ReadAllText($fakeChildOutput) | ConvertFrom-Json
    Assert-True ($childArguments[0] -ceq 'value with spaces' -and $childArguments[1] -ceq 'literal&metachar') 'child-process argument array binding changed literal arguments'

    $arrayJson = Join-Path $tempRoot 'array-script.json'
    [IO.File]::WriteAllText($arrayJson, '[{"ok":true}]', [Text.UTF8Encoding]::new($false))
    Test-JsonDocument $arrayJson 'array-form script fixture'
    $outputReceipt = Join-Path $tempRoot 'sanitized-receipt.json'

    $invokeParameters = @{
        ReceiptFile = $receiptPath; MetadataFile = $metadataPath; AssemblyFile = $assemblyPath
        BundleDirectory = $bundle; ManifestFile = $manifestPath; PrivateDirectory = $private
        ReceiptOutput = $outputReceipt; NodeFile = $node; RouteFile = $fakeRoute
        IdentityContract = $identityContract; ArtifactContract = $artifactContract; ValidationOnly = $true
    }
    $run = Invoke-PostRecoveryOrchestration @invokeParameters
    Assert-True (Test-Path -LiteralPath $outputReceipt -PathType Leaf) 'dry-run receipt was not created'
    $sanitized = [IO.File]::ReadAllText($outputReceipt)
    Assert-True (-not $sanitized.Contains($tempRoot, [StringComparison]::OrdinalIgnoreCase)) 'receipt leaked a private path'
    Assert-True ($run.Receipt.status -ceq 'validated-dry-run') 'dry-run receipt has wrong status'
    $privateConfig = Read-JsonObject (Join-Path $run.WorkDirectory 'extractor/config.json') 'private test config'
    Assert-True ($privateConfig.GenerateStruct -eq $true -and $privateConfig.RequireAnyKey -eq $false) 'private config flags are wrong'
    Assert-True ($privateConfig.DumpMethod -eq $true -and $privateConfig.ForceVersion -eq 16) 'unrelated private config changed'

    Assert-Throws { Invoke-PostRecoveryOrchestration @invokeParameters } 'already exists'

    [IO.File]::AppendAllText($fakeExe, 'tamper')
    Assert-Throws { Test-ExtractorArtifact $manifestPath $bundle $artifactContract } 'failed length/SHA-256 validation'
    [IO.File]::WriteAllBytes($fakeExe, [byte[]](1, 2, 3, 4))

    $badIdentityPath = Join-Path $tempRoot 'bad-identity.json'
    $identity.metadata.sha256 = '0' * 64
    [IO.File]::WriteAllText($badIdentityPath, (($identity | ConvertTo-Json -Depth 10) + "`n"), [Text.UTF8Encoding]::new($false))
    Assert-Throws { Test-RecoveryIdentityReceipt $badIdentityPath $metadataPath $assemblyPath $identityContract } 'does not bind'

    Write-Output 'BPSR automarker IL2CPP post-recovery tests passed.'
} finally {
    $resolvedTempRoot = [IO.Path]::GetFullPath($tempRoot)
    $tempPrefix = $tempBase.TrimEnd([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
    if (-not $resolvedTempRoot.StartsWith($tempPrefix, [StringComparison]::OrdinalIgnoreCase) -or
        $resolvedTempRoot.Equals($tempBase, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'refusing recursive cleanup outside the strict system-temp child directory'
    }
    if (Test-Path -LiteralPath $resolvedTempRoot) { Remove-Item -LiteralPath $resolvedTempRoot -Recurse -Force }
}
