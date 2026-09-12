[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$source = Join-Path $PSScriptRoot 'run-bpsr-automarker-wire-layout-capture.ps1'
$temp = Join-Path ([System.IO.Path]::GetTempPath()) ("rlogs-wire-layout-package-test-" + [Guid]::NewGuid().ToString('N'))
try {
    [System.IO.Directory]::CreateDirectory($temp) | Out-Null
    Copy-Item -LiteralPath $source -Destination $temp
    foreach ($name in @('rlogs-process-capture.exe','rlogs-protocol-journal.exe','rlogs-bpsr-automarker-wire-layout-receipt.exe','pack-24687926.json','steam-distribution-snapshot.v1.json','README.md')) {
        [System.IO.File]::WriteAllText((Join-Path $temp $name),"fixture-$name",(New-Object System.Text.UTF8Encoding($false)))
    }
    $hashLines = @(
        foreach ($file in Get-ChildItem -LiteralPath $temp -File | Where-Object Name -ne 'SHA256SUMS.txt') {
            $hash = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
            "$hash *$($file.Name)"
        }
    )
    [System.IO.File]::WriteAllLines((Join-Path $temp 'SHA256SUMS.txt'),$hashLines,(New-Object System.Text.UTF8Encoding($false)))

    $result = & (Join-Path $temp 'run-bpsr-automarker-wire-layout-capture.ps1') -DryRun | ConvertFrom-Json
    if ($result.expected_build -ne '25247556') { throw 'Dry run lost the exact build gate.' }
    if ($result.duration_seconds -ne 25) { throw 'Dry run lost the bounded default duration.' }
    if ($result.capture_scope -ne 'exact-process-owned-tcp-only') { throw 'Dry run broadened capture scope.' }
    if ($result.private_output_directory -ne 'PRIVATE-RAW-DO-NOT-SHARE') { throw 'Private output boundary changed.' }
    if ($result.packet_send_available -ne $false -or $result.packet_rewrite_available -ne $false -or $result.process_memory_access -ne $false) { throw 'Read-only boundary changed.' }

    $text = Get-Content -LiteralPath $source -Raw
    if ($text -notmatch '(?s)\$guids\s*=\s*@\(\s*@\(.*?Sort-Object -Unique\s*\)') { throw 'Adapter GUID pipeline results are not force-wrapped as an array for StrictMode.' }
    if ($text -notmatch '(?s)\$npcapPresent\s*=\s*@\(\s*@\(.*?Where-Object.*?\)') { throw 'Npcap probe pipeline results are not force-wrapped as an array for StrictMode.' }
    foreach ($forbidden in @('WinDivertSend','pcap_sendpacket','WriteProcessMemory','CreateRemoteThread')) {
        if ($text.Contains($forbidden)) { throw "Launcher contains forbidden primitive $forbidden" }
    }
    Write-Host 'automarker wire-layout capture launcher tests passed.'
} finally {
    if (Test-Path -LiteralPath $temp) { Remove-Item -LiteralPath $temp -Recurse -Force }
}
