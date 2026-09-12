[CmdletBinding()]
param([Parameter(Mandatory)][string]$Destination)

$ErrorActionPreference = 'Stop'
if (Test-Path -LiteralPath $Destination) {
    throw 'Destination already exists; refusing to overwrite a package.'
}
$repoRoot = (& git -C $PSScriptRoot rev-parse --show-toplevel).Trim()
if ($LASTEXITCODE -ne 0 -or -not $repoRoot) { throw 'Could not resolve repository root.' }

$tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
$isolatedTarget = [IO.Path]::GetFullPath((Join-Path $tempRoot ("rlogs-automarker-build-" + [Guid]::NewGuid().ToString('N'))))
if (-not $isolatedTarget.StartsWith($tempRoot, [StringComparison]::OrdinalIgnoreCase) -or
    -not (Split-Path -Leaf $isolatedTarget).StartsWith('rlogs-automarker-build-', [StringComparison]::Ordinal)) {
    throw 'Refusing an unexpected isolated build path.'
}
$previousTarget = [Environment]::GetEnvironmentVariable('CARGO_TARGET_DIR', 'Process')
try {
    [Environment]::SetEnvironmentVariable('CARGO_TARGET_DIR', $isolatedTarget, 'Process')
    & cargo build --release -p rlogs-game-bpsr --bin rlogs-bpsr-automarker-lifecycle-probe `
        --manifest-path (Join-Path $repoRoot 'Cargo.toml')
    if ($LASTEXITCODE -ne 0) { throw 'Release build failed.' }

    New-Item -ItemType Directory -Path $Destination | Out-Null
    Copy-Item -LiteralPath (Join-Path $isolatedTarget 'release\rlogs-bpsr-automarker-lifecycle-probe.exe') -Destination $Destination
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'run-bpsr-automarker-lifecycle-probe.ps1') -Destination $Destination
    Copy-Item -LiteralPath (Join-Path $repoRoot 'plugins\games\blue-protocol-star-resonance\research\automarker-read-only-runtime-probe.md') -Destination (Join-Path $Destination 'README.md')
    & (Join-Path $Destination 'run-bpsr-automarker-lifecycle-probe.ps1') -DryRun
    if ($LASTEXITCODE -ne 0) { throw 'Packaged no-process dry run failed.' }
    Write-Host 'Built and dry-run validated a self-contained probe package; armed modes remain opt-in.'
}
finally {
    [Environment]::SetEnvironmentVariable('CARGO_TARGET_DIR', $previousTarget, 'Process')
    $cleanupTarget = [IO.Path]::GetFullPath($isolatedTarget)
    if ($cleanupTarget.StartsWith($tempRoot, [StringComparison]::OrdinalIgnoreCase) -and
        (Split-Path -Leaf $cleanupTarget).StartsWith('rlogs-automarker-build-', [StringComparison]::Ordinal)) {
        Remove-Item -LiteralPath $cleanupTarget -Recurse -Force -ErrorAction SilentlyContinue
    }
}
