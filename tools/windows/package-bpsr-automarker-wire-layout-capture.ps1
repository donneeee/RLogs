[CmdletBinding()]
param([Parameter(Mandatory)][string]$Destination)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if (Test-Path -LiteralPath $Destination) { throw 'Destination already exists; refusing overwrite.' }
$repoRoot = (& git -C $PSScriptRoot rev-parse --show-toplevel).Trim()
if ($LASTEXITCODE -ne 0 -or -not $repoRoot) { throw 'Could not resolve repository root.' }

$manifest = Join-Path $repoRoot 'Cargo.toml'
$isolatedTarget = Join-Path ([System.IO.Path]::GetTempPath()) ("rlogs-wire-layout-package-" + [guid]::NewGuid().ToString('N'))
$previousCargoTarget = $env:CARGO_TARGET_DIR
try {
    [System.IO.Directory]::CreateDirectory($isolatedTarget) | Out-Null
    $env:CARGO_TARGET_DIR = $isolatedTarget
    & cargo build --release --manifest-path $manifest -p rlogs-process-capture
    if ($LASTEXITCODE -ne 0) { throw 'Process-capture release build failed.' }
    & cargo build --release --manifest-path $manifest -p rlogs-protocol-journal
    if ($LASTEXITCODE -ne 0) { throw 'Protocol-journal release build failed.' }
    & cargo build --release --manifest-path $manifest -p rlogs-game-bpsr --bin rlogs-bpsr-automarker-wire-layout-receipt
    if ($LASTEXITCODE -ne 0) { throw 'Wire-layout receipt release build failed.' }

    $receiptExe = Join-Path $isolatedTarget 'release\rlogs-bpsr-automarker-wire-layout-receipt.exe'
    $schema = (& $receiptExe --offline-schema-check | ConvertFrom-Json)
    if ($LASTEXITCODE -ne 0 -or $schema.tcp_chunk_boundary_fields.Count -ne 4) { throw 'Wire-layout receipt schema self-check failed.' }
    if ($schema.packet_transmission_available -ne $false -or $schema.packet_modification_available -ne $false) { throw 'Wire-layout receipt crossed its read-only boundary.' }

    [System.IO.Directory]::CreateDirectory($Destination) | Out-Null
    $payloads = [ordered]@{
        'rlogs-process-capture.exe' = (Join-Path $isolatedTarget 'release\rlogs-process-capture.exe')
        'rlogs-protocol-journal.exe' = (Join-Path $isolatedTarget 'release\rlogs-protocol-journal.exe')
        'rlogs-bpsr-automarker-wire-layout-receipt.exe' = $receiptExe
        'pack-24687926.json' = (Join-Path $repoRoot 'plugins\games\blue-protocol-star-resonance\protocol-packs\global\steam-24687926\pack.json')
        'steam-distribution-snapshot.v1.json' = (Join-Path $repoRoot 'plugins\games\blue-protocol-star-resonance\research\game-file-inventory\global\steam-25247556\steam-distribution-snapshot.v1.json')
        'run-bpsr-automarker-wire-layout-capture.ps1' = (Join-Path $repoRoot 'tools\windows\run-bpsr-automarker-wire-layout-capture.ps1')
        'README.md' = (Join-Path $repoRoot 'docs\AUTOMARKER_WIRE_LAYOUT_CAPTURE_PACKAGE.md')
    }
    foreach ($entry in $payloads.GetEnumerator()) {
        Copy-Item -LiteralPath $entry.Value -Destination (Join-Path $Destination $entry.Key)
    }
    $lines = @(
        foreach ($name in $payloads.Keys) {
            $hash = (Get-FileHash -LiteralPath (Join-Path $Destination $name) -Algorithm SHA256).Hash.ToLowerInvariant()
            "$hash *$name"
        }
    )
    [System.IO.File]::WriteAllLines((Join-Path $Destination 'SHA256SUMS.txt'),$lines,(New-Object System.Text.UTF8Encoding($false)))
    Write-Host "Created read-only automarker wire-layout capture package: $Destination"
}
finally {
    if ($null -eq $previousCargoTarget) {
        Remove-Item Env:CARGO_TARGET_DIR -ErrorAction SilentlyContinue
    } else {
        $env:CARGO_TARGET_DIR = $previousCargoTarget
    }
    if (Test-Path -LiteralPath $isolatedTarget -PathType Container) {
        $resolvedTarget = [System.IO.Path]::GetFullPath((Resolve-Path -LiteralPath $isolatedTarget).Path)
        $resolvedTemp = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath()).TrimEnd('\')
        $targetParent = [System.IO.Directory]::GetParent($resolvedTarget).FullName.TrimEnd('\')
        $targetName = [System.IO.Path]::GetFileName($resolvedTarget)
        if ($targetParent -ne $resolvedTemp -or -not $targetName.StartsWith('rlogs-wire-layout-package-', [System.StringComparison]::Ordinal)) {
            throw "Refusing to recursively remove unexpected isolated target: $resolvedTarget"
        }
        Remove-Item -LiteralPath $resolvedTarget -Recurse -Force
    }
}
