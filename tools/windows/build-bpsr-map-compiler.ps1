param(
    [Parameter(Mandatory = $true)]
    [string]$OutputDirectory,

    [string]$PythonPath = "python"
)

$ErrorActionPreference = "Stop"
$repositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
$requirements = Join-Path $repositoryRoot "tools/bpsr-map-compiler-requirements.txt"
$source = Join-Path $repositoryRoot "tools/bpsr-local-map-asset.py"
$output = [System.IO.Path]::GetFullPath($OutputDirectory)
$buildTempRoot = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { [System.IO.Path]::GetTempPath() }
$work = Join-Path $buildTempRoot "rlogs-map-compiler-work"
$spec = Join-Path $buildTempRoot "rlogs-map-compiler-spec"

New-Item -ItemType Directory -Force -Path $output, $work, $spec | Out-Null
$pythonVersion = & $PythonPath -c "import sys; print(f'{sys.version_info.major}.{sys.version_info.minor}')"
if ($LASTEXITCODE -ne 0) {
    throw "Could not execute the requested Python runtime: $PythonPath"
}
$pythonVersionParts = @($pythonVersion.Trim().Split('.') | ForEach-Object { [int]$_ })
if ($pythonVersionParts.Count -ne 2 -or $pythonVersionParts[0] -lt 3 -or ($pythonVersionParts[0] -eq 3 -and $pythonVersionParts[1] -lt 10)) {
    throw "The map compiler requires Python 3.10 or newer; '$PythonPath' is Python $pythonVersion"
}
# Every transitive runtime/build dependency is pinned in the requirements file.
# --no-deps is intentional: UnityPy declares the optional FMOD audio backend as
# mandatory even though this dedicated helper only decodes Texture2D assets.
& $PythonPath -m pip install --disable-pip-version-check --no-deps -r $requirements
if ($LASTEXITCODE -ne 0) {
    throw "Failed to install the pinned map-compiler build environment"
}

& $PythonPath -m PyInstaller `
    --noconfirm `
    --clean `
    --onefile `
    --name rlogs-bpsr-map-compiler `
    --distpath $output `
    --workpath $work `
    --specpath $spec `
    --collect-all UnityPy `
    --collect-data archspec `
    --exclude-module fmod_toolkit `
    $source
if ($LASTEXITCODE -ne 0) {
    throw "Failed to build the local game-map compiler"
}

$helper = Join-Path $output "rlogs-bpsr-map-compiler.exe"
if (-not (Test-Path -LiteralPath $helper -PathType Leaf)) {
    throw "The map-compiler executable was not produced"
}
& $helper --self-check
if ($LASTEXITCODE -ne 0) {
    throw "The packaged map compiler failed its self-check"
}
