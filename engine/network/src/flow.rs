use std::net::IpAddr;

use bytes::Bytes;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct IpEndpoint {
    pub address: IpAddr,
    pub port: u16,
}

impl IpEndpoint {
    pub const fn new(address: IpAddr, port: u16) -> Self {
        Self { address, port }
    }
}

/// One direction of a TCP connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TcpFlowKey {
    pub source: IpEndpoint,
    pub destination: IpEndpoint,
}

impl TcpFlowKey {
    pub const fn new(source: IpEndpoint, destination: IpEndpoint) -> Self {
        Self {
            source,
            destination,
        }
    }

    pub const fn reverse(self) -> Self {
        Self {
            source: self.destination,
            destination: self.source,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TcpFlags {
    pub ns: bool,
    pub fin: bool,
    pub syn: bool,
    pub rst: bool,
    pub psh: bool,
    pub ack: bool,
    pub urg: bool,
    pub ece: bool,
    pub cwr: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TcpSegment {
    pub flow: TcpFlowKey,
    pub sequence_number: u32,
    pub acknowledgment_number: u32,
    pub flags: TcpFlags,
    pub capture_sequence: u64,
    pub observed_micros: u64,
    /// An O(1) view into the owning captured frame.
    pub payload: Bytes,
}

impl TcpSegment {
    /// TCP SYN occupies one sequence number before any payload bytes.
    #[inline]
    pub const fn payload_sequence_number(&self) -> u32 {
        self.sequence_number.wrapping_add(self.flags.syn as u32)
    }
}
