[CmdletBinding(SupportsShouldProcess, ConfirmImpact = 'High')]
param(
    [Parameter(Mandatory)]
    [string]$DataRoot
)

$ErrorActionPreference = 'Stop'
$resolvedRoot = (Resolve-Path -LiteralPath $DataRoot).Path
if ((Split-Path -Leaf $resolvedRoot) -ne 'submission-service') {
    throw "Refusing to operate outside an explicitly named submission-service directory: $resolvedRoot"
}
foreach ($required in @('profiles', 'accounts', 'projections')) {
    if (-not (Test-Path -LiteralPath (Join-Path $resolvedRoot $required))) {
        throw "Submission-service marker is missing: $required"
    }
}

$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$backupRoot = Join-Path $resolvedRoot "operator-backups\clear-public-parses-$stamp"
$targets = @(
    'projections',
    'memberships',
    'reconciliations',
    'catalog.v1.json',
    'community-milestones.v1.json'
)
$resolvedPrefix = $resolvedRoot.TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
$existingTargets = foreach ($relative in $targets) {
    $candidate = Join-Path $resolvedRoot $relative
    if (Test-Path -LiteralPath $candidate) {
        $resolved = (Resolve-Path -LiteralPath $candidate).Path
        if (-not $resolved.StartsWith($resolvedPrefix, [StringComparison]::OrdinalIgnoreCase)) {
            throw "Resolved cleanup target escaped the submission-service root: $resolved"
        }
        [pscustomobject]@{ Relative = $relative; Path = $resolved }
    }
}

if (-not $PSCmdlet.ShouldProcess($resolvedRoot, "archive and clear all public parse projections")) {
    return
}

New-Item -ItemType Directory -Path $backupRoot -Force | Out-Null
foreach ($target in $existingTargets) {
    $destination = Join-Path $backupRoot $target.Relative
    $destinationParent = Split-Path -Parent $destination
    New-Item -ItemType Directory -Path $destinationParent -Force | Out-Null
    Move-Item -LiteralPath $target.Path -Destination $destination
}
foreach ($relative in @('projections', 'memberships', 'reconciliations')) {
    New-Item -ItemType Directory -Path (Join-Path $resolvedRoot $relative) -Force | Out-Null
}

# These catalogs are read directly on every public request. Replace them with
# valid empty documents after archiving so the live receiver never observes a
# missing or stale index while it is awaiting a restart.
@{
    schema_version = 6
    total_entries = 0
    offset = 0
    next_offset = $null
    entries = @()
    facets = @{
        deployments = @()
        regions = @()
        activities = @()
        scenes = @()
        difficulties = @()
        terminal_states = @()
    }
} | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $resolvedRoot 'catalog.v1.json') -Encoding utf8NoBOM
@{
    schema_version = 1
    total_entries = 0
    entries = @()
} | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $resolvedRoot 'community-milestones.v1.json') -Encoding utf8NoBOM

[pscustomobject]@{
    DataRoot = $resolvedRoot
    BackupRoot = $backupRoot
    ClearedProjectionCount = @(
        Get-ChildItem -LiteralPath (Join-Path $backupRoot 'projections') -File -ErrorAction SilentlyContinue
    ).Count
    PreservedArtifacts = Test-Path -LiteralPath (Join-Path $resolvedRoot 'artifacts')
    PreservedProfiles = Test-Path -LiteralPath (Join-Path $resolvedRoot 'profiles')
    RestartRequired = $false
}
