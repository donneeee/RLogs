# Automarker WinDivert pass-through canary

Status: research-only; armed traffic observation is fail-closed; marker
substitution disabled.

This package tests the non-network portions of the narrow Windows interception
boundary needed by future Automarkers. It does not place or change a marker.
Armed mode validates elevation, the exact game process, the pinned official
WinDivert dependencies, and their signature, then writes the explicit outcome
`blocked_reflect_arbitration_unimplemented`. It opens no discovery or active
traffic-observing handle. WinDivert defines ordering between overlapping handles
at the same priority as undefined, so live observation remains disabled until a
REFLECT-layer arbitration gate is implemented and tested.

The dormant pass-through implementation now pins DLL ownership through every
handle and worker, closes WinDivert handles with `WinDivertClose`, compiles the
exact active filter before opening it, and makes every relay exit request receive
shutdown and join its timer before DLL unload. Its discovery assembler begins at
the observed SYN sequence, accepts exact/consistent retransmissions and gaps
within a bounded 64 KiB prefix, verifies process ownership only after a BPSR
signature is present, and retains a 250 ms uniqueness window. These paths remain
unreachable in armed mode until REFLECT arbitration exists.

## Safe first check

From a normal PowerShell console in the package directory:

```powershell
.\run-bpsr-automarker-windivert-passthrough.ps1
```

That default is a dry-run. It does not load the WinDivert DLL, open a handle,
install a driver, divert traffic, or transmit anything.

## Driver bootstrap status

The setup script's default invocation is a read-only preflight: it checks the
packaged hashes/signature and reports service state without starting the
executable, requesting elevation, writing a receipt, or changing driver/service
state.

```powershell
.\setup-bpsr-automarker-windivert-driver.ps1
```

The former combined bootstrap-and-pass-through command is currently fail-closed:

```powershell
.\run-bpsr-automarker-windivert-passthrough.ps1 `
  -Mode RLOGS_WINDIVERT_BOOTSTRAP_AND_BYTE_IDENTICAL_PASSTHROUGH_V1 `
  -DurationSeconds 20
```

It validates the dependencies and then writes
`blocked_reflect_arbitration_unimplemented` before opening even the false-filter
bootstrap handle. It therefore cannot install or start the driver in this slice.

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

## Explicit armed dependency check

The armed command currently performs only dependency/process checks and writes
the REFLECT blocker receipt. It does not observe or interrupt game traffic:

```powershell
.\run-bpsr-automarker-windivert-passthrough.ps1 `
  -Mode RLOGS_WINDIVERT_BYTE_IDENTICAL_PASSTHROUGH_V1 `
  -DurationSeconds 20
```

The launcher requests Administrator elevation because the dependency boundary
still validates the environment expected by the future WinDivert path. It
validates the pinned official WinDivert 2.2.2 x64 DLL and driver hashes and the
driver signature before dynamically loading the DLL. It then writes the blocker
receipt and exits without opening a NETWORK or REFLECT handle.

Receipts contain only gate results and aggregate packet/byte counts. They omit
endpoints, ports, sequence/ack values, payloads, timestamps, paths, RPC call
IDs, and account/action/session identifiers.

The bundled `WinDivert.dll` and `WinDivert64.sys` are unmodified files from
official WinDivert v2.2.2. WinDivert is copyright Basil Projects and is offered
under LGPL-3.0-or-later or GPL-2.0; see the official source and license at
<https://github.com/basil00/WinDivert/tree/v2.2.2>.
