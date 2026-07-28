use serde::{Deserialize, Serialize};

/// Direction matters because the same numeric IDs may have different meanings
/// on the client and server sides.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum PacketDirection {
    ClientToServer,
    ServerToClient,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", content = "wire_id", rename_all = "snake_case")]
pub enum FragmentKind {
    Call,
    Notify,
    Return,
    Echo,
    FrameUp,
    FrameDown,
    Unknown(u16),
}

impl FragmentKind {
    pub const fn from_wire_id(wire_id: u16) -> Self {
        match wire_id {
            1 => Self::Call,
            2 => Self::Notify,
            3 => Self::Return,
            4 => Self::Echo,
            5 => Self::FrameUp,
            6 => Self::FrameDown,
            unknown => Self::Unknown(unknown),
        }
    }

    pub const fn wire_id(self) -> u16 {
        match self {
            Self::Call => 1,
            Self::Notify => 2,
            Self::Return => 3,
            Self::Echo => 4,
            Self::FrameUp => 5,
            Self::FrameDown => 6,
            Self::Unknown(wire_id) => wire_id,
        }
    }
}

/// Stable protocol identity used by route catalogs and coverage reports.
///
/// BPSR routing is a tuple, not a single opcode. Stub and call IDs are
/// per-message metadata and therefore do not belong in this catalog key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RouteKey {
    pub direction: PacketDirection,
    pub fragment: FragmentKind,
    pub service_id: u64,
    pub method_id: u32,
}

impl RouteKey {
    pub const fn new(
        direction: PacketDirection,
        fragment: FragmentKind,
        service_id: u64,
        method_id: u32,
    ) -> Self {
        Self {
            direction,
            fragment,
            service_id,
            method_id,
        }
    }
}

/// Route metadata extracted from a packet header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutedMessage {
    pub key: RouteKey,
    pub stub_id: u32,
    pub call_id: Option<u32>,
}
