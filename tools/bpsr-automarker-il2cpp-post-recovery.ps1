[CmdletBinding()]
param(
    [string]$IdentityReceiptPath,
    [string]$MetadataPath,
    [string]$GameAssemblyPath,
    [string]$ExtractorBundlePath,
    [string]$PrivateRoot,
    [string]$OutputReceiptPath,
    [string]$ArtifactManifestPath,
    [string]$NodeExecutablePath,
    [switch]$DryRun,
    [switch]$LibraryOnly
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Get-Sha256([string]$LiteralPath) {
    return (Get-FileHash -Algorithm SHA256 -LiteralPath $LiteralPath).Hash.ToLowerInvariant()
}

function Get-NormalizedTextSha256([string]$LiteralPath) {
    $text = [IO.File]::ReadAllText($LiteralPath).Replace("`r`n", "`n").Replace("`r", "`n")
    $bytes = [Text.UTF8Encoding]::new($false).GetBytes($text)
    return [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($bytes)).ToLowerInvariant()
}

function Resolve-RequiredFile([string]$Path, [string]$Description) {
    if ([string]::IsNullOrWhiteSpace($Path) -or -not [IO.Path]::IsPathFullyQualified($Path)) {
        throw "$Description must be an explicit absolute path"
    }
    $full = [IO.Path]::GetFullPath($Path)
    if (-not (Test-Path -LiteralPath $full -PathType Leaf)) { throw "$Description does not exist as a file" }
    if ((Get-Item -LiteralPath $full -Force).Attributes.HasFlag([IO.FileAttributes]::ReparsePoint)) {
        throw "$Description must not be a reparse point"
    }
    return $full
}

function Resolve-RequiredDirectory([string]$Path, [string]$Description) {
    if ([string]::IsNullOrWhiteSpace($Path) -or -not [IO.Path]::IsPathFullyQualified($Path)) {
        throw "$Description must be an explicit absolute path"
    }
    $full = [IO.Path]::GetFullPath($Path)
    if (-not (Test-Path -LiteralPath $full -PathType Container)) { throw "$Description does not exist as a directory" }
    if ((Get-Item -LiteralPath $full -Force).Attributes.HasFlag([IO.FileAttributes]::ReparsePoint)) {
        throw "$Description must not be a reparse point"
    }
    return $full
}

function Read-JsonObject([string]$LiteralPath, [string]$Description) {
    try { $value = [IO.File]::ReadAllText($LiteralPath) | ConvertFrom-Json -Depth 100 }
    catch { throw "$Description is not valid JSON: $($_.Exception.Message)" }
    if ($null -eq $value -or $value -isnot [pscustomobject]) { throw "$Description must be a JSON object" }
    return $value
}

function Test-JsonDocument([string]$LiteralPath, [string]$Description) {
    try {
        $json = [IO.File]::ReadAllText($LiteralPath)
        $document = [Text.Json.JsonDocument]::Parse($json, [Text.Json.JsonDocumentOptions]::new())
        $document.Dispose()
    } catch { throw "$Description is not valid JSON: $($_.Exception.Message)" }
}

function Get-FileArtifact([string]$LiteralPath) {
    $item = Get-Item -LiteralPath $LiteralPath -Force
    return [ordered]@{ byte_length = [long]$item.Length; sha256 = Get-Sha256 $LiteralPath }
}

function Get-ProductionIdentityContract {
    return [ordered]@{
        SchemaVersion = 2
        GeneratedBy = 'rlogs-bpsr-il2cpp-metadata-scan'
        Game = 'blue-protocol-star-resonance'
        Deployment = 'global'
        Channel = 'steam'
        Build = '25247556'
        AppId = '3681810'
        GameAssemblySha256 = '4a079aec0a3e51a8023aa86bbd152e12068907b65b9aafb355d20bb6d6c41fe3'
        ProcessExecutableSha256 = '90537dbd0e4c9d4b3ed2af06aa8bf7ffe7219fc94bb2f6963673f7b340288588'
        ProcessExecutableLength = 808496L
    }
}

function Get-ProductionArtifactContract {
    return [ordered]@{
        NormalizedManifestSha256 = '13fd18c82bf21689f2bef89d8065e2fb844faf05b5eceacfae4b8642105e1f49'
        Repository = 'https://github.com/Perfare/Il2CppDumper.git'
        Commit = '4741d46ba9cd6159c5d853eb9d6fc48b4bfa2b1a'
        Tree = 'e7dfe729915d031b6a8e4ebfd2984e23bfa694b5'
        DirectoryLabel = 'Il2CppDumper-4741d46-net8-win-x64'
        FileCount = 199
        TotalBytes = 74661504L
    }
}

function Test-MetadataHeader([string]$LiteralPath) {
    $stream = [IO.File]::Open($LiteralPath, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
    try {
        if ($stream.Length -lt 0x180) { throw 'metadata is too short for the IL2CPP header' }
        $header = [byte[]]::new(8)
        if ($stream.Read($header, 0, 8) -ne 8) { throw 'metadata header could not be read' }
    } finally { $stream.Dispose() }
    $metadataMagic = [uint32]::Parse('FAB11BAF', [Globalization.NumberStyles]::HexNumber)
    if ([BitConverter]::ToUInt32($header, 0) -ne $metadataMagic) { throw 'metadata lacks the IL2CPP metadata magic' }
    $version = [BitConverter]::ToInt32($header, 4)
    if ($version -lt 20 -or $version -gt 99) { throw "metadata version $version is implausible" }
    return $version
}

function Test-RecoveryIdentityReceipt(
    [string]$ReceiptPath,
    [string]$MetadataFile,
    [string]$AssemblyFile,
    [System.Collections.IDictionary]$Contract
) {
    $receipt = Read-JsonObject $ReceiptPath 'identity receipt'
    $metadata = Get-FileArtifact $MetadataFile
    $assembly = Get-FileArtifact $AssemblyFile
    $metadataVersion = Test-MetadataHeader $MetadataFile
    $validHash = { param($value) [string]$value -match '^[0-9a-fA-F]{64}$' }
    $ok =
        [int]$receipt.schema_version -eq [int]$Contract.SchemaVersion -and
        [string]$receipt.generated_by -ceq [string]$Contract.GeneratedBy -and
        [string]$receipt.game -ceq [string]$Contract.Game -and
        [string]$receipt.deployment -ceq [string]$Contract.Deployment -and
        [string]$receipt.channel -ceq [string]$Contract.Channel -and
        [string]$receipt.game_build -ceq [string]$Contract.Build -and
        [string]$receipt.distribution_app_id -ceq [string]$Contract.AppId -and
        [long]$receipt.metadata.byte_length -eq [long]$metadata.byte_length -and
        [string]$receipt.metadata.sha256 -ieq [string]$metadata.sha256 -and
        [int]$receipt.metadata.metadata_version -eq $metadataVersion -and
        [long]$receipt.game_assembly.byte_length -eq [long]$assembly.byte_length -and
        [string]$receipt.game_assembly.sha256 -ieq [string]$assembly.sha256 -and
        [string]$receipt.game_assembly.sha256 -ieq [string]$Contract.GameAssemblySha256 -and
        [long]$receipt.process_executable.byte_length -eq [long]$Contract.ProcessExecutableLength -and
        [string]$receipt.process_executable.sha256 -ieq [string]$Contract.ProcessExecutableSha256 -and
        [long]$receipt.steam_manifest.byte_length -gt 0 -and
        (& $validHash $receipt.steam_manifest.sha256)
    if (-not $ok) { throw "identity receipt does not bind the exact recovered build-$($Contract.Build) inputs" }
    return [ordered]@{ Receipt = $receipt; Metadata = $metadata; Assembly = $assembly; MetadataVersion = $metadataVersion }
}

function Test-ExtractorArtifact(
    [string]$ManifestPath,
    [string]$BundlePath,
    [System.Collections.IDictionary]$Contract
) {
    if ((Get-NormalizedTextSha256 $ManifestPath) -cne [string]$Contract.NormalizedManifestSha256) {
        throw 'extractor artifact manifest digest does not match the pinned manifest'
    }
    $manifest = Read-JsonObject $ManifestPath 'extractor artifact manifest'
    if ([int]$manifest.schema_version -ne 1 -or [string]$manifest.artifact -cne 'Il2CppDumper' -or
        [string]$manifest.provenance.repository -cne [string]$Contract.Repository -or
        [string]$manifest.provenance.commit -cne [string]$Contract.Commit -or
        [string]$manifest.provenance.tree -cne [string]$Contract.Tree -or
        [string]$manifest.publish.directory_label -cne [string]$Contract.DirectoryLabel -or
        [string]$manifest.publish.target_framework -cne 'net8.0' -or
        [string]$manifest.publish.runtime_identifier -cne 'win-x64' -or
        $manifest.publish.self_contained -ne $true -or
        [int]$manifest.publish.file_count -ne [int]$Contract.FileCount -or
        [long]$manifest.publish.total_bytes -ne [long]$Contract.TotalBytes) {
        throw 'extractor artifact manifest identity does not match the pinned self-contained build'
    }
    $entries = @($manifest.publish.files)
    if ($entries.Count -ne [int]$Contract.FileCount) { throw 'extractor artifact manifest file count is inconsistent' }
    $seen = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    $actualFiles = @(Get-ChildItem -LiteralPath $BundlePath -File -Force)
    $actualDirectories = @(Get-ChildItem -LiteralPath $BundlePath -Directory -Force)
    if ($actualDirectories.Count -ne 0 -or $actualFiles.Count -ne $entries.Count) {
        throw 'extractor bundle shape differs from its pinned manifest'
    }
    $total = 0L
    foreach ($entry in $entries) {
        $relative = [string]$entry.path
        if ([string]::IsNullOrWhiteSpace($relative) -or [IO.Path]::IsPathRooted($relative) -or
            $relative.Contains('/') -or $relative.Contains('\') -or $relative -in @('.', '..') -or
            -not $seen.Add($relative)) { throw 'extractor manifest contains an unsafe or duplicate file name' }
        $file = Join-Path $BundlePath $relative
        if (-not (Test-Path -LiteralPath $file -PathType Leaf)) { throw "extractor file is missing: $relative" }
        $item = Get-Item -LiteralPath $file -Force
        if ($item.Attributes.HasFlag([IO.FileAttributes]::ReparsePoint)) { throw "extractor file is a reparse point: $relative" }
        if ([long]$item.Length -ne [long]$entry.length -or (Get-Sha256 $file) -cne [string]$entry.sha256) {
            throw "extractor file failed length/SHA-256 validation: $relative"
        }
        $total += [long]$item.Length
    }
    if ($total -ne [long]$Contract.TotalBytes) { throw 'extractor bundle byte total differs from its pinned manifest' }
    foreach ($required in @('Il2CppDumper.exe', 'config.json')) {
        if (-not $seen.Contains($required)) { throw "extractor manifest lacks required file $required" }
    }
    return $manifest
}

function Copy-ValidatedBundle([pscustomobject]$Manifest, [string]$Source, [string]$Destination) {
    if (Test-Path -LiteralPath $Destination) { throw 'private bundle destination already exists' }
    [IO.Directory]::CreateDirectory($Destination) | Out-Null
    foreach ($entry in @($Manifest.publish.files)) {
        [IO.File]::Copy((Join-Path $Source ([string]$entry.path)), (Join-Path $Destination ([string]$entry.path)), $false)
    }
}

function Set-PrivateExtractorConfig([string]$ConfigPath) {
    $before = Read-JsonObject $ConfigPath 'private extractor config'
    $properties = @($before.PSObject.Properties.Name)
    foreach ($required in @('GenerateStruct', 'RequireAnyKey')) {
        if ($required -notin $properties -or $before.$required -isnot [bool]) { throw "extractor config lacks boolean $required" }
    }
    $snapshot = @{}
    foreach ($property in $before.PSObject.Properties) { $snapshot[$property.Name] = $property.Value }
    $before.GenerateStruct = $true
    $before.RequireAnyKey = $false
    [IO.File]::WriteAllText($ConfigPath, (($before | ConvertTo-Json -Depth 20) + "`n"), [Text.UTF8Encoding]::new($false))
    $after = Read-JsonObject $ConfigPath 'updated private extractor config'
    if ($after.GenerateStruct -ne $true -or $after.RequireAnyKey -ne $false) { throw 'private extractor config flags were not set' }
    foreach ($property in $after.PSObject.Properties) {
        if ($property.Name -notin @('GenerateStruct', 'RequireAnyKey') -and
            (($property.Value | ConvertTo-Json -Compress -Depth 20) -cne ($snapshot[$property.Name] | ConvertTo-Json -Compress -Depth 20))) {
            throw "private extractor config unexpectedly changed $($property.Name)"
        }
    }
    if (@($after.PSObject.Properties.Name).Count -ne $properties.Count) {
        throw 'private extractor config property set unexpectedly changed'
    }
}

function Invoke-CheckedProcess([string]$Executable, [string[]]$Arguments, [string]$WorkingDirectory) {
    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = $Executable
    $start.WorkingDirectory = $WorkingDirectory
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    foreach ($argument in $Arguments) { $start.ArgumentList.Add($argument) }
    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $start
    if (-not $process.Start()) { throw 'failed to start offline child process' }
    $stdoutTask = $process.StandardOutput.ReadToEndAsync()
    $stderrTask = $process.StandardError.ReadToEndAsync()
    $process.WaitForExit()
    $stdout = $stdoutTask.GetAwaiter().GetResult()
    $stderr = $stderrTask.GetAwaiter().GetResult()
    $exitCode = $process.ExitCode
    $process.Dispose()
    if ($exitCode -ne 0) { throw "offline child process failed with exit code $exitCode`: $stderr" }
    return [ordered]@{ ExitCode = $exitCode; Stdout = $stdout; Stderr = $stderr }
}

function Write-NewUtf8File([string]$LiteralPath, [string]$Contents) {
    $parent = Split-Path -Parent $LiteralPath
    if (-not (Test-Path -LiteralPath $parent -PathType Container)) { throw 'output receipt parent directory does not exist' }
    $stream = [IO.File]::Open($LiteralPath, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
    try {
        $bytes = [Text.UTF8Encoding]::new($false).GetBytes($Contents)
        $stream.Write($bytes, 0, $bytes.Length)
        $stream.Flush($true)
    } finally { $stream.Dispose() }
}

function Assert-PathFreeReceipt([string]$Json, [string[]]$SensitivePaths) {
    if ($Json -match '(?i)[a-z]:[\\/]' -or $Json -match '(?i)(?:^|["\s])/(?:users|home|tmp|var)/') {
        throw 'sanitized receipt contains an absolute path'
    }
    foreach ($path in $SensitivePaths) {
        if (-not [string]::IsNullOrWhiteSpace($path) -and $Json.Contains($path, [StringComparison]::OrdinalIgnoreCase)) {
            throw 'sanitized receipt contains a private input or output path'
        }
    }
}

function Invoke-PostRecoveryOrchestration {
    param(
        [string]$ReceiptFile, [string]$MetadataFile, [string]$AssemblyFile,
        [string]$BundleDirectory, [string]$ManifestFile, [string]$PrivateDirectory,
        [string]$ReceiptOutput, [string]$NodeFile, [string]$RouteFile,
        [System.Collections.IDictionary]$IdentityContract,
        [System.Collections.IDictionary]$ArtifactContract,
        [switch]$ValidationOnly
    )
    $receiptFileFull = Resolve-RequiredFile $ReceiptFile 'identity receipt'
    $metadataFull = Resolve-RequiredFile $MetadataFile 'metadata'
    $assemblyFull = Resolve-RequiredFile $AssemblyFile 'GameAssembly'
    $bundleFull = Resolve-RequiredDirectory $BundleDirectory 'extractor bundle'
    $manifestFull = Resolve-RequiredFile $ManifestFile 'extractor artifact manifest'
    $privateFull = Resolve-RequiredDirectory $PrivateDirectory 'private root'
    $nodeFull = Resolve-RequiredFile $NodeFile 'Node executable'
    $routeFull = Resolve-RequiredFile $RouteFile 'automarker route tool'
    if ([string]::IsNullOrWhiteSpace($ReceiptOutput) -or -not [IO.Path]::IsPathFullyQualified($ReceiptOutput)) {
        throw 'output receipt must be an explicit absolute path'
    }
    $receiptOutputFull = [IO.Path]::GetFullPath($ReceiptOutput)
    if (Test-Path -LiteralPath $receiptOutputFull) { throw 'output receipt already exists; refusing overwrite' }
    $distinct = @($receiptFileFull, $metadataFull, $assemblyFull, $manifestFull, $receiptOutputFull)
    if (($distinct | Sort-Object -Unique).Count -ne $distinct.Count) { throw 'input and output paths must be distinct' }

    $identity = Test-RecoveryIdentityReceipt $receiptFileFull $metadataFull $assemblyFull $IdentityContract
    $manifest = Test-ExtractorArtifact $manifestFull $bundleFull $ArtifactContract

    $runId = [Guid]::NewGuid().ToString('N')
    $runDirectory = Join-Path $privateFull "automarker-il2cpp-$runId"
    if (Test-Path -LiteralPath $runDirectory) { throw 'fresh private run directory unexpectedly exists' }
    [IO.Directory]::CreateDirectory($runDirectory) | Out-Null
    $privateBundle = Join-Path $runDirectory 'extractor'
    $privateInputs = Join-Path $runDirectory 'inputs'
    $privateOutput = Join-Path $runDirectory 'output'
    Copy-ValidatedBundle $manifest $bundleFull $privateBundle
    Test-ExtractorArtifact $manifestFull $privateBundle $ArtifactContract | Out-Null
    [IO.Directory]::CreateDirectory($privateInputs) | Out-Null
    [IO.Directory]::CreateDirectory($privateOutput) | Out-Null
    $privateReceipt = Join-Path $privateInputs 'recovery-identity.json'
    $privateMetadata = Join-Path $privateInputs 'global-metadata.dat'
    $privateAssembly = Join-Path $privateInputs 'GameAssembly.dll'
    [IO.File]::Copy($receiptFileFull, $privateReceipt, $false)
    [IO.File]::Copy($metadataFull, $privateMetadata, $false)
    [IO.File]::Copy($assemblyFull, $privateAssembly, $false)
    $identity = Test-RecoveryIdentityReceipt $privateReceipt $privateMetadata $privateAssembly $IdentityContract
    Set-PrivateExtractorConfig (Join-Path $privateBundle 'config.json')

    $dumpPath = Join-Path $privateOutput 'dump.cs'
    $scriptPath = Join-Path $privateOutput 'script.json'
    $routePath = Join-Path $privateOutput 'automarker-il2cpp-route.json'
    $outputs = $null
    $status = 'validated-dry-run'
    if (-not $ValidationOnly) {
        foreach ($path in @($dumpPath, $scriptPath, $routePath)) {
            if (Test-Path -LiteralPath $path) { throw 'fresh private output unexpectedly exists' }
        }
        Invoke-CheckedProcess (Join-Path $privateBundle 'Il2CppDumper.exe') @($privateAssembly, $privateMetadata, $privateOutput) $privateBundle | Out-Null
        foreach ($pair in @(@('dump.cs', $dumpPath), @('script.json', $scriptPath))) {
            if (-not (Test-Path -LiteralPath $pair[1] -PathType Leaf) -or (Get-Item -LiteralPath $pair[1]).Length -le 0) {
                throw "$($pair[0]) was not produced as a nonempty file"
            }
        }
        Test-JsonDocument $scriptPath 'extractor script.json'
        $identity = Test-RecoveryIdentityReceipt $privateReceipt $privateMetadata $privateAssembly $IdentityContract
        Invoke-CheckedProcess $nodeFull @($routeFull, 'build', '--build', [string]$IdentityContract.Build,
            '--metadata', $privateMetadata, '--identity', $privateReceipt, '--game-assembly', $privateAssembly,
            '--dump', $dumpPath, '--output', $routePath) $runDirectory | Out-Null
        if (-not (Test-Path -LiteralPath $routePath -PathType Leaf) -or (Get-Item -LiteralPath $routePath).Length -le 0) {
            throw 'automarker route tool did not produce a nonempty output'
        }
        $route = Read-JsonObject $routePath 'automarker route output'
        $dumpArtifact = Get-FileArtifact $dumpPath
        if ([int]$route.schema_version -ne 1 -or [string]$route.generated_by -cne 'tools/bpsr-automarker-il2cpp-route.mjs' -or
            [string]$route.build_id -cne [string]$IdentityContract.Build -or $route.scope.process_access -ne $false -or
            [long]$route.inputs.metadata.byte_length -ne [long]$identity.Metadata.byte_length -or
            [string]$route.inputs.metadata.sha256 -ine [string]$identity.Metadata.sha256 -or
            [long]$route.inputs.game_assembly.byte_length -ne [long]$identity.Assembly.byte_length -or
            [string]$route.inputs.game_assembly.sha256 -ine [string]$identity.Assembly.sha256 -or
            [string]$route.inputs.recovery_identity.sha256 -ine (Get-Sha256 $privateReceipt) -or
            [string]$route.inputs.il2cpp_dump.sha256 -ine [string]$dumpArtifact.sha256) {
            throw 'automarker route output identity or input binding is invalid'
        }
        $outputs = [ordered]@{
            il2cpp_dump = $dumpArtifact
            il2cpp_script = Get-FileArtifact $scriptPath
            automarker_route = Get-FileArtifact $routePath
        }
        $status = 'complete'
    }

    $manifestArtifact = Get-FileArtifact $manifestFull
    $receiptArtifact = Get-FileArtifact $receiptFileFull
    $result = [ordered]@{
        schema_version = 1
        generated_by = 'tools/bpsr-automarker-il2cpp-post-recovery.ps1'
        status = $status
        run_id = $runId
        game = [string]$IdentityContract.Game
        deployment = [string]$IdentityContract.Deployment
        channel = [string]$IdentityContract.Channel
        build_id = [string]$IdentityContract.Build
        scope = [ordered]@{ offline_only = $true; process_access = $false; process_attachment = $false; game_modification = $false; network_transmission = $false }
        recovery_identity = [ordered]@{ schema_version = 2; generated_by = [string]$IdentityContract.GeneratedBy; byte_length = $receiptArtifact.byte_length; sha256 = $receiptArtifact.sha256 }
        extractor = [ordered]@{
            repository = [string]$ArtifactContract.Repository
            commit = [string]$ArtifactContract.Commit
            artifact_manifest = $manifestArtifact
            file_count = [int]$ArtifactContract.FileCount
            total_bytes = [long]$ArtifactContract.TotalBytes
            private_config = [ordered]@{ GenerateStruct = $true; RequireAnyKey = $false; all_other_settings_preserved = $true }
        }
        inputs = [ordered]@{
            metadata = [ordered]@{ byte_length = $identity.Metadata.byte_length; sha256 = $identity.Metadata.sha256; metadata_version = $identity.MetadataVersion }
            game_assembly = $identity.Assembly
        }
        outputs = $outputs
    }
    $json = ($result | ConvertTo-Json -Depth 20) + "`n"
    Assert-PathFreeReceipt $json @($receiptFileFull, $metadataFull, $assemblyFull, $bundleFull, $manifestFull, $privateFull, $runDirectory, $receiptOutputFull)
    Write-NewUtf8File $receiptOutputFull $json
    return [ordered]@{ WorkDirectory = $runDirectory; Receipt = $result }
}

if ($LibraryOnly) { return }

foreach ($required in @('IdentityReceiptPath', 'MetadataPath', 'GameAssemblyPath', 'ExtractorBundlePath', 'PrivateRoot', 'OutputReceiptPath')) {
    if ([string]::IsNullOrWhiteSpace((Get-Variable -Name $required -ValueOnly))) { throw "-$required is required" }
}
if ([string]::IsNullOrWhiteSpace($ArtifactManifestPath)) {
    $ArtifactManifestPath = Join-Path (Split-Path -Parent $PSScriptRoot) 'plugins/games/blue-protocol-star-resonance/research/toolchain/il2cppdumper-4741d46-net8-win-x64.manifest.v1.json'
}
if ([string]::IsNullOrWhiteSpace($NodeExecutablePath)) {
    $nodeCommand = Get-Command node -CommandType Application -ErrorAction Stop
    $NodeExecutablePath = $nodeCommand.Source
}
$routeTool = Join-Path $PSScriptRoot 'bpsr-automarker-il2cpp-route.mjs'
$invokeParameters = @{
    ReceiptFile = $IdentityReceiptPath
    MetadataFile = $MetadataPath
    AssemblyFile = $GameAssemblyPath
    BundleDirectory = $ExtractorBundlePath
    ManifestFile = $ArtifactManifestPath
    PrivateDirectory = $PrivateRoot
    ReceiptOutput = $OutputReceiptPath
    NodeFile = $NodeExecutablePath
    RouteFile = $routeTool
    IdentityContract = Get-ProductionIdentityContract
    ArtifactContract = Get-ProductionArtifactContract
    ValidationOnly = $DryRun
}
$invocation = Invoke-PostRecoveryOrchestration @invokeParameters
Write-Output "post-recovery orchestration $($invocation.Receipt.status); sanitized receipt written"
