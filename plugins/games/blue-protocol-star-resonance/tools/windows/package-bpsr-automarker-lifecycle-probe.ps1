[CmdletBinding()]
param([Parameter(Mandatory)][string]$Destination)

$ErrorActionPreference = 'Stop'
if (Test-Path -LiteralPath $Destination) {
    throw 'Destination already exists; refusing to overwrite a package.'
}
$repoRoot = (& git -C $PSScriptRoot rev-parse --show-toplevel).Trim()
if ($LASTEXITCODE -ne 0 -or -not $repoRoot) { throw 'Could not resolve repository root.' }

& cargo build --release -p rlogs-game-bpsr --bin rlogs-bpsr-automarker-lifecycle-probe `
    --manifest-path (Join-Path $repoRoot 'Cargo.toml')
if ($LASTEXITCODE -ne 0) { throw 'Release build failed.' }

New-Item -ItemType Directory -Path $Destination | Out-Null
Copy-Item -LiteralPath (Join-Path $repoRoot 'target\release\rlogs-bpsr-automarker-lifecycle-probe.exe') -Destination $Destination
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'run-bpsr-automarker-lifecycle-probe.ps1') -Destination $Destination
Copy-Item -LiteralPath (Join-Path $repoRoot 'plugins\games\blue-protocol-star-resonance\research\automarker-read-only-runtime-probe.md') -Destination (Join-Path $Destination 'README.md')
Write-Host 'Built self-contained probe package; default mode is read-only and the reversible nudge requires explicit arming.'
