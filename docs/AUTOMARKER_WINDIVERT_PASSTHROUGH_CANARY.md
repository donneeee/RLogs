# Automarker WinDivert pass-through canary

Status: research-only; armed byte-identical pass-through is explicitly opt-in
and fail-closed; marker substitution disabled.

This package tests the narrow Windows interception boundary needed by future
Automarkers. It does not place or change a marker. Armed mode validates
elevation, the exact game process, the pinned official WinDivert dependencies,
and their signature. Before either its passive NETWORK discovery handle or its
active priority-0 NETWORK handle opens, it performs a serialized REFLECT
inventory. A false-filter NETWORK handle at priority -1000 provides an ordered
barrier: once its REFLECT OPEN event arrives, all earlier handle events have
been consumed. Any open NETWORK handle at the guarded handle's priority
(discovery +1, active 0) is conservatively treated as overlapping regardless
of filter text and produces
`blocked_same_priority_network_handle`. A fresh second inventory is required
after connection discovery because the first proof is no longer current. An
in-process arbitration lock remains held from inventory through the guarded
handle open, so rLogs cannot interleave its own handle creation across that
boundary.

The REFLECT proof inventories WinDivert handles only. It cannot identify
arbitrary WFP providers or prove ExitLag compatibility, and it is a point-in-time
pre-open gate rather than a machine-wide lock against another process opening a
handle later. Any malformed, duplicate, unmatched-close, missing-sentinel,
timeout, or driver-version observation aborts without opening the guarded
handle.

The pass-through implementation pins DLL ownership through every
handle and worker, closes WinDivert handles with `WinDivertClose`, compiles the
exact active filter before opening it, and makes every relay exit request receive
shutdown and join its timer before DLL unload. Its discovery assembler begins at
the observed SYN sequence, accepts exact/consistent retransmissions and gaps
within a bounded 64 KiB prefix, verifies process ownership only after a BPSR
signature is present, and retains a 250 ms uniqueness window. No substitution
or checksum rewrite is connected to this executable.

The low-level DLL lifetime, owned-handle operations, filter compilation, and
serialized REFLECT arbitration live in the private shared source
`automarker_windivert_backend.rs`. The standalone canary includes that file
directly, so a future in-process coordinator can reuse the same implementation
without creating a public library API or maintaining a second native backend.

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

The combined bootstrap-and-pass-through command is separately consented:

```powershell
.\run-bpsr-automarker-windivert-passthrough.ps1 `
  -Mode RLOGS_WINDIVERT_BOOTSTRAP_AND_BYTE_IDENTICAL_PASSTHROUGH_V1 `
  -DurationSeconds 20
```

It validates the dependencies, opens one false-filter bootstrap handle at
priority -999 to install/start only the pinned driver if necessary, and then
requires both REFLECT arbitration passes. The bootstrap filter is `false`, so
it cannot capture, block, or send traffic and is not an overlapping priority-0
handle.

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

## Explicit armed pass-through

The armed command performs the dependency/process checks, two affirmative
REFLECT arbitration passes, exact process-owned SYN discovery, and a bounded
byte-identical pass-through:

```powershell
.\run-bpsr-automarker-windivert-passthrough.ps1 `
  -Mode RLOGS_WINDIVERT_BYTE_IDENTICAL_PASSTHROUGH_V1 `
  -DurationSeconds 20
```

The launcher requests Administrator elevation because WinDivert requires it. It
validates the pinned official WinDivert 2.2.2 x64 DLL and driver hashes and the
driver signature before dynamically loading the DLL. This slice was not run
live and does not establish server acceptance, anti-cheat safety, or ExitLag
compatibility.

Receipts contain only gate results and aggregate packet/byte counts. They omit
endpoints, ports, sequence/ack values, payloads, timestamps, paths, RPC call
IDs, and account/action/session identifiers.

The bundled `WinDivert.dll` and `WinDivert64.sys` are unmodified files from
official WinDivert v2.2.2. WinDivert is copyright Basil Projects and is offered
under LGPL-3.0-or-later or GPL-2.0; see the official source and license at
<https://github.com/basil00/WinDivert/tree/v2.2.2>.
