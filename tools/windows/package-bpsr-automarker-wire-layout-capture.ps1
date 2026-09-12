[CmdletBinding()]
param([Parameter(Mandatory)][string]$Destination)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if (Test-Path -LiteralPath $Destination) { throw 'Destination already exists; refusing overwrite.' }
$repoRoot = (& git -C $PSScriptRoot rev-parse --show-toplevel).Trim()
if ($LASTEXITCODE -ne 0 -or -not $repoRoot) { throw 'Could not resolve repository root.' }

$manifest = Join-Path $repoRoot 'Cargo.toml'
& cargo build --release --manifest-path $manifest -p rlogs-process-capture
if ($LASTEXITCODE -ne 0) { throw 'Process-capture release build failed.' }
& cargo build --release --manifest-path $manifest -p rlogs-protocol-journal
if ($LASTEXITCODE -ne 0) { throw 'Protocol-journal release build failed.' }
& cargo build --release --manifest-path $manifest -p rlogs-game-bpsr --bin rlogs-bpsr-automarker-wire-layout-receipt
if ($LASTEXITCODE -ne 0) { throw 'Wire-layout receipt release build failed.' }

[System.IO.Directory]::CreateDirectory($Destination) | Out-Null
$payloads = [ordered]@{
    'rlogs-process-capture.exe' = 'target\release\rlogs-process-capture.exe'
    'rlogs-protocol-journal.exe' = 'target\release\rlogs-protocol-journal.exe'
    'rlogs-bpsr-automarker-wire-layout-receipt.exe' = 'target\release\rlogs-bpsr-automarker-wire-layout-receipt.exe'
    'pack-24687926.json' = 'plugins\games\blue-protocol-star-resonance\protocol-packs\global\steam-24687926\pack.json'
    'steam-distribution-snapshot.v1.json' = 'plugins\games\blue-protocol-star-resonance\research\game-file-inventory\global\steam-25247556\steam-distribution-snapshot.v1.json'
    'run-bpsr-automarker-wire-layout-capture.ps1' = 'tools\windows\run-bpsr-automarker-wire-layout-capture.ps1'
    'README.md' = 'docs\AUTOMARKER_WIRE_LAYOUT_CAPTURE_PACKAGE.md'
}
foreach ($entry in $payloads.GetEnumerator()) {
    Copy-Item -LiteralPath (Join-Path $repoRoot $entry.Value) -Destination (Join-Path $Destination $entry.Key)
}
$lines = @(
    foreach ($name in $payloads.Keys) {
        $hash = (Get-FileHash -LiteralPath (Join-Path $Destination $name) -Algorithm SHA256).Hash.ToLowerInvariant()
        "$hash *$name"
    }
)
[System.IO.File]::WriteAllLines((Join-Path $Destination 'SHA256SUMS.txt'),$lines,(New-Object System.Text.UTF8Encoding($false)))
Write-Host "Created read-only automarker wire-layout capture package: $Destination"
