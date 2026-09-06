param(
    [Parameter(Mandatory = $true)]
    [string]$OutputDirectory
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
python -m pip install --disable-pip-version-check -r $requirements
if ($LASTEXITCODE -ne 0) {
    throw "Failed to install the pinned map-compiler build environment"
}

python -m PyInstaller `
    --noconfirm `
    --clean `
    --onefile `
    --name rlogs-bpsr-map-compiler `
    --distpath $output `
    --workpath $work `
    --specpath $spec `
    --collect-all UnityPy `
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
