[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string]$Destination,
    [string]$WinDivertSource
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
if ([string]::IsNullOrWhiteSpace($WinDivertSource)) {
    $WinDivertSource = Join-Path $root 'tmp-rdps-audit\upstream-resonance-logs-cn\src-tauri'
}
$destinationFull = [IO.Path]::GetFullPath($Destination)
if (-not $destinationFull.StartsWith([IO.Path]::GetFullPath('\\STREAM-PC\RLogs-transfer'), [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Destination must remain under \\STREAM-PC\RLogs-transfer.'
}

Push-Location $root
try {
    cargo build -p rlogs-game-bpsr --bin rlogs-bpsr-automarker-windivert-passthrough --release
    if ($LASTEXITCODE -ne 0) { throw 'Release build failed.' }
    node tools\validate-bpsr-automarker-windivert-passthrough.mjs
    if ($LASTEXITCODE -ne 0) { throw 'Static boundary validation failed.' }
} finally {
    Pop-Location
}

$dll = Join-Path $WinDivertSource 'WinDivert.dll'
$driver = Join-Path $WinDivertSource 'WinDivert64.sys'
$expected = @{
    $dll = 'C1E060EE19444A259B2162F8AF0F3FE8C4428A1C6F694DCE20DE194AC8D7D9A2'
    $driver = '8DA085332782708D8767BCACE5327A6EC7283C17CFB85E40B03CD2323A90DDC2'
}
foreach ($path in @($dll, $driver)) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Missing pinned dependency: $path" }
    if ((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash -ne $expected[$path]) {
        throw "Pinned dependency hash mismatch: $path"
    }
}
$signature = Get-AuthenticodeSignature -LiteralPath $driver
if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Thumbprint -ne '043589F75FCE2795E7F2CC3E526D46784D5DDAB3') {
    throw 'Pinned WinDivert driver signature validation failed.'
}

$license = Join-Path $root 'target\LICENSE.WinDivert.txt'
$licenseHash = '14A0CB5214D536E4FDAE6AA3F5696F981EEDA106CD026E9794BBA489EE79D628'
if (-not (Test-Path -LiteralPath $license -PathType Leaf)) {
    Invoke-WebRequest -UseBasicParsing `
        -Uri 'https://raw.githubusercontent.com/basil00/WinDivert/v2.2.2/LICENSE' `
        -OutFile $license
}
if ((Get-FileHash -LiteralPath $license -Algorithm SHA256).Hash -ne $licenseHash) {
    throw 'Pinned WinDivert license hash validation failed.'
}

if (Test-Path -LiteralPath $destinationFull) { throw "Refusing to overwrite $destinationFull" }
New-Item -ItemType Directory -Path $destinationFull | Out-Null
Copy-Item -LiteralPath (Join-Path $root 'target\release\rlogs-bpsr-automarker-windivert-passthrough.exe') -Destination $destinationFull
Copy-Item -LiteralPath (Join-Path $root 'tools\windows\run-bpsr-automarker-windivert-passthrough.ps1') -Destination $destinationFull
Copy-Item -LiteralPath (Join-Path $root 'tools\windows\setup-bpsr-automarker-windivert-driver.ps1') -Destination $destinationFull
Copy-Item -LiteralPath (Join-Path $root 'docs\AUTOMARKER_WINDIVERT_PASSTHROUGH_CANARY.md') -Destination (Join-Path $destinationFull 'README.md')
Copy-Item -LiteralPath $dll -Destination $destinationFull
Copy-Item -LiteralPath $driver -Destination $destinationFull
Copy-Item -LiteralPath $license -Destination (Join-Path $destinationFull 'LICENSE.WinDivert.txt')
Get-ChildItem -LiteralPath $destinationFull -File | Get-FileHash -Algorithm SHA256 | Format-Table -AutoSize
