[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$orchestrator = Join-Path $PSScriptRoot 'bpsr-automarker-il2cpp-post-recovery.ps1'
. $orchestrator -LibraryOnly

$resolved = Resolve-NodeExecutablePath $null
if ([string]::IsNullOrWhiteSpace($resolved) -or -not [IO.Path]::IsPathFullyQualified($resolved) -or
    -not (Test-Path -LiteralPath $resolved -PathType Leaf) -or
    [IO.Path]::GetFileName($resolved) -ine 'node.exe') {
    throw 'Default Node resolution did not select one concrete executable.'
}

$explicit = Resolve-NodeExecutablePath $resolved
if ($explicit -cne [IO.Path]::GetFullPath($resolved)) {
    throw 'Explicit Node executable selection changed.'
}

Assert-PathFreeReceipt '{"repository":"https://github.com/Perfare/Il2CppDumper.git"}' @()
$windowsPathRejected = $false
try { Assert-PathFreeReceipt '{"private":"C:\\private\\metadata.dat"}' @() }
catch { $windowsPathRejected = $_.Exception.Message -eq 'sanitized receipt contains an absolute path' }
if (-not $windowsPathRejected) { throw 'Windows absolute path was not rejected from a sanitized receipt.' }
$unixPathRejected = $false
try { Assert-PathFreeReceipt '{"private":"/home/user/metadata.dat"}' @() }
catch { $unixPathRejected = $_.Exception.Message -eq 'sanitized receipt contains an absolute path' }
if (-not $unixPathRejected) { throw 'Unix absolute path was not rejected from a sanitized receipt.' }

Write-Host 'Automarker IL2CPP post-recovery host resolution tests passed.'
