[CmdletBinding(SupportsShouldProcess, ConfirmImpact = 'Medium')]
param(
    [Parameter(Mandatory)]
    [string]$DataRoot,

    [Parameter(Mandatory)]
    [ValidatePattern('^[a-z0-9-]+$')]
    [string]$Region,

    [Parameter(Mandatory)]
    [ValidateNotNullOrEmpty()]
    [string[]]$CharacterId
)

$ErrorActionPreference = 'Stop'
$resolvedRoot = (Resolve-Path -LiteralPath $DataRoot).Path
if ((Split-Path -Leaf $resolvedRoot) -ne 'submission-service') {
    throw "Refusing to operate outside an explicitly named submission-service directory: $resolvedRoot"
}
$profilesRoot = Join-Path $resolvedRoot 'profiles'
if (-not (Test-Path -LiteralPath $profilesRoot -PathType Container)) {
    throw "Submission-service profile store is missing: $profilesRoot"
}

$requested = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
foreach ($id in $CharacterId) {
    if ($id -notmatch '^\d+$') {
        throw "Character IDs must be numeric: $id"
    }
    [void]$requested.Add($id)
}

$matches = foreach ($directory in Get-ChildItem -LiteralPath $profilesRoot -Directory) {
    $publicPath = Join-Path $directory.FullName 'public.json'
    if (-not (Test-Path -LiteralPath $publicPath -PathType Leaf)) {
        continue
    }
    $profile = Get-Content -LiteralPath $publicPath -Raw | ConvertFrom-Json
    if ($requested.Contains([string]$profile.character_id)) {
        [pscustomobject]@{ Directory = $directory.FullName; PublicPath = $publicPath; Profile = $profile }
    }
}
$observed = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
foreach ($match in $matches) { [void]$observed.Add([string]$match.Profile.character_id) }
foreach ($id in $requested) {
    if (-not $observed.Contains($id)) {
        throw "No published profile was found for character ID $id"
    }
}

$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$backupRoot = Join-Path $resolvedRoot "operator-backups\profile-region-$stamp"
$catalogPath = Join-Path $profilesRoot 'catalog.v1.json'
if (-not $PSCmdlet.ShouldProcess(($requested -join ', '), "set packet-confirmed profile region to $Region")) {
    return
}
New-Item -ItemType Directory -Path $backupRoot -Force | Out-Null
if (Test-Path -LiteralPath $catalogPath -PathType Leaf) {
    Copy-Item -LiteralPath $catalogPath -Destination (Join-Path $backupRoot 'catalog.v1.json')
}

foreach ($match in $matches) {
    $profileId = Split-Path -Leaf $match.Directory
    Copy-Item -LiteralPath $match.Directory -Destination (Join-Path $backupRoot $profileId) -Recurse

    $documents = @($match.PublicPath)
    $loadoutRoot = Join-Path $match.Directory 'loadouts'
    if (Test-Path -LiteralPath $loadoutRoot -PathType Container) {
        $documents += @(Get-ChildItem -LiteralPath $loadoutRoot -Filter '*.json' -File | Select-Object -ExpandProperty FullName)
    }
    foreach ($path in $documents) {
        $document = Get-Content -LiteralPath $path -Raw | ConvertFrom-Json
        if ($path -eq $match.PublicPath) {
            $document.region = $Region
        }
        if ($null -ne $document.envelope.routing.region) {
            $document.envelope.routing.region = $Region
        }
        if ($null -ne $document.envelope.body.character.region.region_id) {
            $document.envelope.body.character.region.region_id = $Region
        }
        $temporary = "$path.region-repair.tmp"
        $document | ConvertTo-Json -Depth 100 | Set-Content -LiteralPath $temporary -Encoding utf8NoBOM
        Move-Item -LiteralPath $temporary -Destination $path -Force
    }
}

# The receiver serves this prebuilt index directly. Update it in the same
# migration and collapse legacy region-forked rows by stable character UID,
# preferring the newest packet-observed profile.
if (Test-Path -LiteralPath $catalogPath -PathType Leaf) {
    $catalog = Get-Content -LiteralPath $catalogPath -Raw | ConvertFrom-Json
    foreach ($entry in @($catalog.profiles)) {
        if ($requested.Contains([string]$entry.character_id)) {
            $entry.region = $Region
        }
    }
    $catalog.profiles = @(
        $catalog.profiles |
            Sort-Object -Property @{ Expression = 'updated_unix_millis'; Descending = $true }, @{ Expression = 'profile_id'; Descending = $false } |
            Group-Object -Property character_id |
            ForEach-Object { $_.Group | Select-Object -First 1 } |
            Sort-Object -Property @{ Expression = 'updated_unix_millis'; Descending = $true }, @{ Expression = 'profile_id'; Descending = $false }
    )
    $temporary = "$catalogPath.region-repair.tmp"
    $catalog | ConvertTo-Json -Depth 100 | Set-Content -LiteralPath $temporary -Encoding utf8NoBOM
    Move-Item -LiteralPath $temporary -Destination $catalogPath -Force
}

[pscustomobject]@{
    DataRoot = $resolvedRoot
    BackupRoot = $backupRoot
    Region = $Region
    CharacterIds = @($requested)
    UpdatedProfileDirectories = @($matches).Count
    RestartRequired = $false
}
