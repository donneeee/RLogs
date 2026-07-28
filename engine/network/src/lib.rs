//! Allocation-conscious link/IP/TCP decoding and stream reconstruction.

mod decode;
mod flow;
mod ip_fragments;
mod network_decoder;
mod reassembly;

pub use decode::{DecodeIssue, DecodeMetrics, DecodeResult, FrameDecoder};
pub use flow::{IpEndpoint, TcpFlags, TcpFlowKey, TcpSegment};
pub use ip_fragments::{
    IpFragment, IpFragmentConfig, IpFragmentConfigError, IpFragmentDrop, IpFragmentDropReason,
    IpFragmentEvent, IpFragmentKey, IpFragmentMetrics, IpFragmentReassembler,
    ReassembledIpDatagram,
};
pub use network_decoder::{NetworkDecodeEvent, NetworkDecoder};
pub use reassembly::{
    GapReason, ReassemblyConfig, ReassemblyConfigError, ReassemblyMetrics, TcpReassembler,
    TcpStreamChunk, TcpStreamEvent, TcpStreamGap,
};
