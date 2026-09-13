# Automarker WinDivert checksum boundary

Status: research-only library boundary; no live handle, capture, driver install,
send, process-memory access, or normal application wiring.

`automarker_windivert_checksum.rs` represents the WinDivert 2.2.2 x64
`WINDIVERT_ADDRESS` as an exact 80-byte `#[repr(C)]` value. Compile-time and
runtime checks pin its size, alignment, field offsets, NETWORK layer, outbound
direction, and IPv4 family before an approved changed packet can proceed.

The non-candidate branch returns owned byte-for-byte packet and address copies
without calling any helper. The changed branch requires the same packet length,
loads `WinDivertHelperCalcChecksums` only from the pinned official 2.2.2 DLL,
and calls it on owned packet/address copies. Loading is gated by both official
file hashes, the reviewed 2.2.2 release identity, and the valid pinned driver
signature. (The official DLL has no ProductVersion resource.) It does not open
WinDivert or load the driver. The future live bridge must retain the existing
handle major/minor version gate before capture is enabled.

Send authorization is returned only when the helper reports success, changes
no packet bytes outside the IPv4 and TCP checksum fields, changes no address
state outside the IPv4/TCP checksum flags, sets both flags, and leaves valid
IPv4 and TCP checksums. Any discrepancy fails closed and returns no packet
authorized as modified.

This closes only the checksum/ABI prerequisite for the one-marker canary. A
future separate bridge still must supply exact connection/scene gates, hold a
fresh game-authorized `World.UseSlot` carrier, manage retransmission and ACK state, obtain
post-rewrite RPC and authoritative marker confirmation, and recover safely on
every interception/send failure before any live test is allowed.
