# Automarker WinDivert pass-through canary

Status: research-only; byte-identical pass-through; marker substitution disabled.

This package tests the narrow Windows interception boundary needed by future
Automarkers. It does not place or change a marker. It discovers a new outbound
connection only after observing its SYN, verifies that the exact four-tuple is
owned by the sole `BPSR_STEAM.exe` process, confirms the BPSR wire signature,
then diverts only payload packets for that tuple for at most 60 seconds. Each
packet and its full WinDivert address metadata are synchronously reinjected
without changing a byte.

## Safe first check

From a normal PowerShell console in the package directory:

```powershell
.\run-bpsr-automarker-windivert-passthrough.ps1
```

That default is a dry-run. It does not load the WinDivert DLL, open a handle,
install a driver, divert traffic, or transmit anything.

## Explicit driver bootstrap (only when the service is absent)

The pass-through canary deliberately keeps `NO_INSTALL` on every handle that
can observe traffic. On a machine where the `WinDivert` service is absent, run
the separate setup script once. Its default invocation is a read-only
preflight: it checks the packaged hashes/signature and reports service state,
without starting the executable, requesting elevation, writing a receipt, or
changing driver/service state.

```powershell
.\setup-bpsr-automarker-windivert-driver.ps1
```

After reviewing that result, the only installation-capable command is the
combined bootstrap-and-pass-through mode:

```powershell
.\run-bpsr-automarker-windivert-passthrough.ps1 `
  -Mode RLOGS_WINDIVERT_BOOTSTRAP_AND_BYTE_IDENTICAL_PASSTHROUGH_V1 `
  -DurationSeconds 20
```

This requests Administrator elevation, revalidates the pinned official 2.2.2
x64 DLL and driver plus the driver signer, then calls `WinDivertOpen` without
`NO_INSTALL` exactly once using the filter `false` with `SNIFF | RECV_ONLY`.
That bootstrap handle cannot match, divert, or transmit a packet. WinDivert
creates and starts its demand-start kernel-driver service if needed; rLogs then
verifies the loaded driver reports version 2.2 and retains that harmless handle
through SYN discovery and the pass-through interval. This avoids relying on
service persistence between separate processes. All traffic-observing handles
still use `NO_INSTALL`, and none of these paths contains substitution logic.

WinDivert marks a newly created service for deletion, but the loaded driver can
remain available until it is stopped or Windows reboots. To explicitly request
the official stop/delete sequence after all WinDivert applications are closed:

```powershell
.\setup-bpsr-automarker-windivert-driver.ps1 `
  -Mode RLOGS_WINDIVERT_DRIVER_REMOVE_V1
```

Removal refuses unless this package has a successful combined-mode receipt. The
`WinDivert` service is shared system-wide, so stopping it can disrupt another
application using WinDivert; close those applications first. If Windows defers
unload/deletion, reboot. Deleting the standalone package files prevents future
on-demand installation from this package.

## Explicit armed canary

This can briefly interrupt the selected game connection if Windows, the
driver, or the process fails while a packet is in user space. Save anything
important first. Run with ExitLag **off** for the first receipt. Start the
command while the game process exists, then reconnect the game only after the
console says it is waiting for a SYN:

```powershell
.\run-bpsr-automarker-windivert-passthrough.ps1 `
  -Mode RLOGS_WINDIVERT_BYTE_IDENTICAL_PASSTHROUGH_V1 `
  -DurationSeconds 20
```

The canary launcher requests Administrator elevation because WinDivert requires it.
It validates the pinned official WinDivert 2.2.2 x64 DLL and driver hashes and
the driver signature before the executable dynamically loads the DLL. The
discovery and active handles always use `NO_INSTALL`; this canary will not
install a missing driver. If the driver is not already installed, it refuses
to run; use the separately named combined bootstrap mode above instead.

Do not close the elevated console during the 20-second active interval. On
normal completion rLogs stops receive, drains and byte-identically reinjects
the queue, then closes the handle. If the game disconnects, allow the command
to exit, restart the game, and retain the sanitized receipt or error text. To
roll back, delete this standalone folder; it does not change rLogs settings or
install a persistent rLogs component.

Receipts contain only gate results and aggregate packet/byte counts. They omit
endpoints, ports, sequence/ack values, payloads, timestamps, paths, RPC call
IDs, and account/action/session identifiers.

The bundled `WinDivert.dll` and `WinDivert64.sys` are unmodified files from
official WinDivert v2.2.2. WinDivert is copyright Basil Projects and is offered
under LGPL-3.0-or-later or GPL-2.0; see the official source and license at
<https://github.com/basil00/WinDivert/tree/v2.2.2>.
