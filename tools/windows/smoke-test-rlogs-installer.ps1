[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string] $InstallerPath,

    [Parameter(Mandatory = $true)]
    [string] $ExpectedVersion,

    [Parameter(Mandatory = $true)]
    [string] $ApplicationName,

    [Parameter(Mandatory = $true)]
    [string] $InstallDirectory
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$installer = Get-Item -LiteralPath $InstallerPath -ErrorAction Stop
if ($installer.PSIsContainer -or $installer.Extension -ne ".exe") {
    throw "InstallerPath must identify an executable file"
}

if ([string]::IsNullOrWhiteSpace($ExpectedVersion)) {
    throw "ExpectedVersion must not be empty"
}

if ([string]::IsNullOrWhiteSpace($ApplicationName) -or
    $ApplicationName -ne [System.IO.Path]::GetFileName($ApplicationName) -or
    [System.IO.Path]::GetExtension($ApplicationName) -ne ".exe") {
    throw "ApplicationName must be an executable leaf filename"
}

if (-not [System.IO.Path]::IsPathFullyQualified($InstallDirectory)) {
    throw "InstallDirectory must be an absolute path"
}

$installRoot = [System.IO.Path]::GetFullPath($InstallDirectory)
$volumeRoot = [System.IO.Path]::GetPathRoot($installRoot)
if ($installRoot.TrimEnd('\') -eq $volumeRoot.TrimEnd('\')) {
    throw "InstallDirectory must not be a volume root"
}
if ($installRoot -match '\s') {
    throw "InstallDirectory must not contain whitespace because NSIS /D must be passed as its final unquoted argument"
}
if (Test-Path -LiteralPath $installRoot) {
    throw "InstallDirectory already exists: $installRoot"
}

New-Item -ItemType Directory -Path $installRoot -ErrorAction Stop | Out-Null

$process = Start-Process `
    -FilePath $installer.FullName `
    -ArgumentList @("/S", "/D=$installRoot") `
    -WindowStyle Hidden `
    -Wait `
    -PassThru
if ($process.ExitCode -ne 0) {
    throw "Installer exited with code $($process.ExitCode)"
}

$application = Get-Item -LiteralPath (Join-Path $installRoot $ApplicationName) -ErrorAction Stop
if ($application.Length -le 0) {
    throw "Installed $ApplicationName is empty"
}

$stream = [System.IO.File]::OpenRead($application.FullName)
try {
    $first = $stream.ReadByte()
    $second = $stream.ReadByte()
}
finally {
    $stream.Dispose()
}
if ($first -ne 0x4d -or $second -ne 0x5a) {
    throw "Installed $ApplicationName does not have a Windows PE MZ signature"
}

$versionInfo = [System.Diagnostics.FileVersionInfo]::GetVersionInfo($application.FullName)
$observedVersions = @($versionInfo.ProductVersion, $versionInfo.FileVersion) |
    Where-Object { -not [string]::IsNullOrWhiteSpace($_) }
if (-not ($observedVersions | Where-Object { $_ -eq $ExpectedVersion -or $_ -like "$ExpectedVersion.*" })) {
    throw "Installed executable version '$($observedVersions -join ', ')' does not match expected version '$ExpectedVersion'"
}

$uninstaller = Get-Item -LiteralPath (Join-Path $installRoot "uninstall.exe") -ErrorAction Stop
if ($uninstaller.Length -le 0) {
    throw "Installed uninstaller is empty"
}

[pscustomobject]@{
    schema_version = 1
    installer = $installer.Name
    installer_bytes = $installer.Length
    install_directory = $installRoot
    application = $application.Name
    application_bytes = $application.Length
    expected_version = $ExpectedVersion
    product_version = $versionInfo.ProductVersion
    file_version = $versionInfo.FileVersion
    pe_signature = "MZ"
    uninstaller = $uninstaller.Name
    uninstaller_bytes = $uninstaller.Length
    result = "passed"
} | ConvertTo-Json -Compress
