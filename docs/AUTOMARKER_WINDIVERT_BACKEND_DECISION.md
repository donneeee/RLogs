# Automarker WinDivert backend decision

Status: dependency-neutral policy implemented; live activation unavailable.

The reviewed Windows backend is the official WinDivert 2.2.2 x64 binary
distribution. This decision does not vendor, load, install, open, receive,
block, modify, reinject, or send through WinDivert. The executable hashes,
handle policies, refusal gates, and lifecycle contract are represented as inert
data and tests in `automarker_inline_transport.rs`. The machine-readable record
is `automarker-windivert-backend-decision.v1.json`.

## Distribution and privilege boundary

Pin the official `v2.2.2` tag at commit
`1789526ecfb9ff5397c94f9f54c1a3dc2fb60440`. For a 64-bit rLogs process on
64-bit Windows, upstream requires the x64 `WinDivert.dll` and
`WinDivert64.sys` beside the application. The official driver is signed; the
DLL is not Authenticode-signed. Packaging must validate the pinned hashes
before loading either file, ship upstream's license/notices, and complete a
release/legal review of the LGPLv3-or-later/GPLv2 choice.

Opening a handle installs the driver automatically when necessary and requires
Administrator privileges. rLogs must never do that because a user merely opens
the Automarkers page. Installation/elevation belongs behind a separate,
explicitly enabled setup action. A `REFLECT` handle with `NO_INSTALL` is the
read-only first preflight so inspection cannot install a missing driver.

Primary sources:

- [WinDivert 2.2 documentation](https://reqrypt.org/windivert-doc.html)
- [WinDivert v2.2.2 release](https://github.com/basil00/WinDivert/releases/tag/v2.2.2)
- [Tagged public ABI](https://github.com/basil00/WinDivert/blob/v2.2.2/include/windivert.h)
- [Tagged driver source](https://github.com/basil00/WinDivert/blob/v2.2.2/sys/windivert.c)
- [Microsoft WFP filter arbitration](https://learn.microsoft.com/en-us/windows/win32/fwp/filter-arbitration)

## Handle topology

The proposed backend uses four narrowly separated responsibilities:

1. A passive `NETWORK` observer at priority `+1`, with
   `SNIFF | RECV_ONLY | NO_INSTALL`, records SYN/ACK, tuple, loopback,
   impostor, and checksum evidence.
2. A passive `FLOW` observer at priority `0`, with mandatory
   `SNIFF | RECV_ONLY | NO_INSTALL`, records PID/five-tuple establishment and deletion.
   It must start before the game connection because FLOW cannot replay old
   events. An exact IP Helper snapshot is corroboration, not a substitute for
   the SYN-scoped epoch.
3. A `REFLECT` observer with `SNIFF | RECV_ONLY | NO_INSTALL` inventories
   existing WinDivert handles. It cannot inventory arbitrary WFP providers.
4. Only after every gate passes, a normal `NETWORK` handle at priority `0` with
   `NO_INSTALL` diverts one exact outbound IPv4/TCP payload four-tuple. The filter
   includes both addresses, both ports, and `tcp.PayloadLength > 0`. NETWORK
   does not expose PID, so it may never activate from protocol signature,
   process name, remote endpoint, or mirrored traffic alone.

The passive NETWORK handle sits above the active handle so an active-handle
reinject cannot return to that observer through WinDivert's own decreasing
priority chain. Any other WinDivert NETWORK handle at active priority `0` is a
refusal condition because equal-priority delivery order is undefined.

## Receive, modify, and send contract

The first canary uses synchronous single-packet `WinDivertRecv` and
`WinDivertSend`; batching and overlapped I/O remain disabled. This bounds the
user-mode crash window to one packet. For every received packet:

- retain the complete `WINDIVERT_ADDRESS`, including direction, interface,
  loopback, impostor, and checksum flags;
- validate the exact tuple/epoch and the complete carrier before changing a
  byte;
- use the offline rewrite ledger for every intersecting retransmission;
- recalculate IPv4 and TCP checksums after modification and update the address
  checksum flags;
- require the sent byte count to equal the received byte count; and
- on any parse, ledger, conservation, or substitution failure, synchronously
  reinject the original packet and disable placement for that epoch.

An unchanged packet/address pair returned by `WinDivertRecv` is valid for
reinjection even when checksum offload made the captured checksum bytes look
invalid; the associated checksum flags carry that meaning. The current offline
adapter rejects such ambiguity. Therefore the future live wrapper—not the
offline transformer—must distinguish unchanged pass-through from modified
output and only demand freshly valid checksums for modified output.

Loopback and impostor packets are not globally excluded. ExitLag may make the
authoritative plaintext leg loopback or injected by another driver. Those flags
are evidence and must be preserved; they are not ownership proof.

## Queue and shutdown contract

Start with upstream defaults: length `4096`, time `2000 ms`, size `4194304`
bytes. Set/read back the values explicitly and disable placement for the epoch
on any observed loss. Larger queues only enlarge latency and the crash window;
they are not the initial remedy for a slow userspace loop.

Graceful shutdown is ordered:

1. stop accepting placement operations;
2. call `WinDivertShutdown(RECV)` only;
3. drain until `ERROR_NO_DATA`, reinjecting every original or verified rewrite;
4. close the active handle and clear the epoch/ledger; then
5. close passive handles.

Do not call `Shutdown(BOTH)` while diverted originals remain because that
disables the send path needed to return them. The tagged driver source shows
that close cleanup reinjects non-sniff, nonexpired packets still in its packet
and work queues. It also shows queue/work overflow paths that drop packets.
No user-mode backend can guarantee delivery of a packet already handed to a
process that terminates before calling `Send`.

## ExitLag and WFP coexistence

WinDivert priorities order WinDivert handles. They do not prove ordering
against arbitrary WFP providers. Microsoft documents WFP arbitration across
sublayers and filters; upstream separately warns that two block-clone-reinject
drivers can form mutual loops and eventually fail with
`ERROR_HOST_UNREACHABLE`. Therefore neither priority `0`, an `Impostor` filter,
nor a mirrored packet capture proves ExitLag compatibility.

ExitLag remains a distinct opt-in mode. Before enabling it, an on-game-PC test
must identify exactly one process-owned plaintext leg with ExitLag off and on,
then run an explicitly authorized, time-bounded passthrough-only canary. Any
duplicate delivery, TTL decay, queue loss, send error, reset, ambiguous leg, or
changing callout behavior refuses activation. Mirrored traffic is useful for
protocol observation but cannot establish socket ownership or local WFP order.

## Remaining implementation sequence

1. Add hash-verified dynamic loading and ABI layout tests; do not add a Rust
   package that silently downloads or installs a driver.
2. Implement passive `REFLECT`, `FLOW`, and `NETWORK` probes and join their
   evidence to IP Helper ownership and a SYN-scoped epoch.
3. Implement exact-filter compilation and version/queue readback.
4. Implement the single-inflight synchronous pass-through loop with structured
   shutdown and fault injection tests, still without substitution enabled.
5. Run the explicit on-game-PC passthrough canary, separately with ExitLag off
   and on, and retain sanitized receipts.
6. Only after those receipts pass, connect the existing offline adapter behind
   a disabled-by-default canary setting and test one marker request.

Live placement remains unavailable until every machine-readable activation
gate is true. This audit does not establish server acceptance, anti-cheat
safety, or permission to place far-away coordinates.
