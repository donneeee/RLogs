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

# A valid PE header does not prove that the application can initialize. Keep
# the installed binary alive long enough to create its real Tauri window so
# startup stack overflows and other pre-WebView crashes fail the release.
$applicationProcess = Start-Process `
    -FilePath $application.FullName `
    -WorkingDirectory $installRoot `
    -PassThru
$launchDeadline = [DateTime]::UtcNow.AddSeconds(20)
$mainWindowObserved = $false
try {
    while ([DateTime]::UtcNow -lt $launchDeadline) {
        Start-Sleep -Milliseconds 250
        $applicationProcess.Refresh()
        if ($applicationProcess.HasExited) {
            throw "Installed $ApplicationName exited during startup with code $($applicationProcess.ExitCode)"
        }
        if ($applicationProcess.MainWindowHandle -ne 0 -and $applicationProcess.Responding) {
            $mainWindowObserved = $true
            break
        }
    }
    if (-not $mainWindowObserved) {
        throw "Installed $ApplicationName did not create a responsive main window within 20 seconds"
    }

    $stabilityDeadline = [DateTime]::UtcNow.AddSeconds(10)
    while ([DateTime]::UtcNow -lt $stabilityDeadline) {
        Start-Sleep -Milliseconds 250
        $applicationProcess.Refresh()
        if ($applicationProcess.HasExited) {
            throw "Installed $ApplicationName exited after opening its main window with code $($applicationProcess.ExitCode)"
        }
        if (-not $applicationProcess.Responding) {
            throw "Installed $ApplicationName stopped responding during the startup stability window"
        }
    }
}
finally {
    if (-not $applicationProcess.HasExited) {
        Stop-Process -Id $applicationProcess.Id -Force -ErrorAction SilentlyContinue
        $applicationProcess.WaitForExit(5000) | Out-Null
    }
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
    launch_result = "responsive_main_window"
    uninstaller = $uninstaller.Name
    uninstaller_bytes = $uninstaller.Length
    result = "passed"
} | ConvertTo-Json -Compress
