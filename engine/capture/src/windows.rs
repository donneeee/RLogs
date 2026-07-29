use std::{
    ffi::c_void,
    mem::{size_of, size_of_val},
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    ptr,
};

use windows_sys::Win32::{
    Foundation::{ERROR_INSUFFICIENT_BUFFER, NO_ERROR},
    NetworkManagement::IpHelper::{
        GetExtendedTcpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCP6TABLE_OWNER_PID, MIB_TCPROW_OWNER_PID,
        MIB_TCPTABLE_OWNER_PID, TCP_TABLE_OWNER_PID_ALL,
    },
    Networking::WinSock::{AF_INET, AF_INET6},
};

use crate::dumpcap::DumpcapLiveCapture;
use crate::{
    CaptureError, CaptureSource, CaptureSourceMetadata, CapturedFrame, DumpcapLiveConfig,
    OwnedProcessCapture, OwnedProcessCaptureConfig, OwnedProcessCaptureMetrics, ProcessSocketOwner,
    TcpConnection, TcpEndpoint,
};

const MAX_TABLE_QUERY_ATTEMPTS: usize = 4;

#[derive(Debug, Clone)]
pub struct WindowsProcessSocketOwner {
    process_id: u32,
}

impl WindowsProcessSocketOwner {
    pub fn new(process_id: u32) -> Result<Self, CaptureError> {
        if process_id == 0 {
            return Err(socket_table_error("process ID must be greater than zero"));
        }
        Ok(Self { process_id })
    }

    pub const fn process_id(&self) -> u32 {
        self.process_id
    }

    fn snapshot_ipv4(&self) -> Result<Vec<TcpConnection>, CaptureError> {
        let buffer = query_tcp_table(u32::from(AF_INET))?;
        // SAFETY: `query_tcp_table` returns an aligned buffer initialized by
        // `GetExtendedTcpTable` for AF_INET and TCP_TABLE_OWNER_PID_ALL.
        let table = unsafe { &*buffer.as_ptr().cast::<MIB_TCPTABLE_OWNER_PID>() };
        let count = table.dwNumEntries as usize;
        let first = ptr::addr_of!(table.table).cast::<MIB_TCPROW_OWNER_PID>();
        checked_rows_fit(
            &buffer,
            first.cast(),
            count,
            size_of::<MIB_TCPROW_OWNER_PID>(),
        )?;

        let mut connections = Vec::new();
        for index in 0..count {
            // SAFETY: bounds were checked against the returned buffer above.
            let row = unsafe { ptr::read_unaligned(first.add(index)) };
            if row.dwOwningPid != self.process_id {
                continue;
            }
            let connection = TcpConnection::new(
                TcpEndpoint::new(
                    IpAddr::V4(Ipv4Addr::from(u32::from_be(row.dwLocalAddr))),
                    network_port(row.dwLocalPort),
                ),
                TcpEndpoint::new(
                    IpAddr::V4(Ipv4Addr::from(u32::from_be(row.dwRemoteAddr))),
                    network_port(row.dwRemotePort),
                ),
            );
            if usable_remote_connection(connection) {
                connections.push(connection);
            }
        }
        Ok(connections)
    }

    fn snapshot_ipv6(&self) -> Result<Vec<TcpConnection>, CaptureError> {
        let buffer = query_tcp_table(u32::from(AF_INET6))?;
        // SAFETY: `query_tcp_table` returns an aligned buffer initialized by
        // `GetExtendedTcpTable` for AF_INET6 and TCP_TABLE_OWNER_PID_ALL.
        let table = unsafe { &*buffer.as_ptr().cast::<MIB_TCP6TABLE_OWNER_PID>() };
        let count = table.dwNumEntries as usize;
        let first = ptr::addr_of!(table.table).cast::<MIB_TCP6ROW_OWNER_PID>();
        checked_rows_fit(
            &buffer,
            first.cast(),
            count,
            size_of::<MIB_TCP6ROW_OWNER_PID>(),
        )?;

        let mut connections = Vec::new();
        for index in 0..count {
            // SAFETY: bounds were checked against the returned buffer above.
            let row = unsafe { ptr::read_unaligned(first.add(index)) };
            if row.dwOwningPid != self.process_id {
                continue;
            }
            let connection = TcpConnection::new(
                TcpEndpoint::new(
                    IpAddr::V6(Ipv6Addr::from(row.ucLocalAddr)),
                    network_port(row.dwLocalPort),
                ),
                TcpEndpoint::new(
                    IpAddr::V6(Ipv6Addr::from(row.ucRemoteAddr)),
                    network_port(row.dwRemotePort),
                ),
            );
            if usable_remote_connection(connection) {
                connections.push(connection);
            }
        }
        Ok(connections)
    }
}

impl ProcessSocketOwner for WindowsProcessSocketOwner {
    fn snapshot(&mut self) -> Result<Vec<TcpConnection>, CaptureError> {
        let mut connections = self.snapshot_ipv4()?;
        connections.extend(self.snapshot_ipv6()?);
        connections.sort_unstable();
        connections.dedup();
        Ok(connections)
    }
}

/// Safe Windows live-capture entry point.
///
/// The broad dumpcap pipe is private to this wrapper, so callers cannot obtain
/// a frame until the exact TCP connection is attributed to `process_id`.
#[derive(Debug)]
pub struct WindowsOwnedDumpcapCapture {
    inner: OwnedProcessCapture<DumpcapLiveCapture, WindowsProcessSocketOwner>,
}

impl WindowsOwnedDumpcapCapture {
    pub fn spawn(
        process_id: u32,
        dumpcap: DumpcapLiveConfig,
        filter: OwnedProcessCaptureConfig,
    ) -> Result<Self, CaptureError> {
        let source = DumpcapLiveCapture::spawn(dumpcap)?;
        let owner = WindowsProcessSocketOwner::new(process_id)?;
        Ok(Self {
            inner: OwnedProcessCapture::new(source, owner, filter)?,
        })
    }

    pub fn metrics(&self) -> &OwnedProcessCaptureMetrics {
        self.inner.metrics()
    }

    pub fn confirmed_connections(&self) -> Vec<TcpConnection> {
        self.inner.confirmed_connections()
    }
}

impl CaptureSource for WindowsOwnedDumpcapCapture {
    fn metadata(&self) -> &CaptureSourceMetadata {
        self.inner.metadata()
    }

    fn next_frame(&mut self) -> Result<Option<CapturedFrame>, CaptureError> {
        self.inner.next_frame()
    }
}

fn query_tcp_table(address_family: u32) -> Result<Vec<usize>, CaptureError> {
    let mut required_bytes = 0_u32;
    // SAFETY: the first call deliberately supplies a null output pointer so
    // Windows reports the required buffer size in `required_bytes`.
    let status = unsafe {
        GetExtendedTcpTable(
            ptr::null_mut(),
            &mut required_bytes,
            0,
            address_family,
            TCP_TABLE_OWNER_PID_ALL,
            0,
        )
    };
    if status != ERROR_INSUFFICIENT_BUFFER && status != NO_ERROR {
        return Err(socket_table_status(status));
    }
    if required_bytes == 0 {
        return Err(socket_table_error(
            "Windows returned an empty TCP ownership table",
        ));
    }

    for _ in 0..MAX_TABLE_QUERY_ATTEMPTS {
        let word_bytes = size_of::<usize>();
        let word_count = (required_bytes as usize)
            .checked_add(word_bytes - 1)
            .ok_or_else(|| socket_table_error("TCP ownership table size overflowed"))?
            / word_bytes;
        let mut buffer = vec![0_usize; word_count];
        let mut supplied_bytes = u32::try_from(size_of_val(buffer.as_slice()))
            .map_err(|_| socket_table_error("TCP ownership table is too large"))?;
        // SAFETY: `buffer` is writable for `supplied_bytes`, aligned to at
        // least `usize`, and remains alive for the call.
        let status = unsafe {
            GetExtendedTcpTable(
                buffer.as_mut_ptr().cast::<c_void>(),
                &mut supplied_bytes,
                0,
                address_family,
                TCP_TABLE_OWNER_PID_ALL,
                0,
            )
        };
        if status == NO_ERROR {
            return Ok(buffer);
        }
        if status != ERROR_INSUFFICIENT_BUFFER {
            return Err(socket_table_status(status));
        }
        required_bytes = supplied_bytes;
    }

    Err(socket_table_error(
        "TCP ownership table kept changing during bounded retries",
    ))
}

fn checked_rows_fit(
    buffer: &[usize],
    first_row: *const c_void,
    count: usize,
    row_size: usize,
) -> Result<(), CaptureError> {
    let buffer_start = buffer.as_ptr() as usize;
    let buffer_end = buffer_start
        .checked_add(size_of_val(buffer))
        .ok_or_else(|| socket_table_error("TCP ownership table bounds overflowed"))?;
    let rows_start = first_row as usize;
    let rows_end = count
        .checked_mul(row_size)
        .and_then(|bytes| rows_start.checked_add(bytes))
        .ok_or_else(|| socket_table_error("TCP ownership row bounds overflowed"))?;
    if rows_start < buffer_start || rows_end > buffer_end {
        return Err(socket_table_error(
            "Windows returned a truncated TCP ownership table",
        ));
    }
    Ok(())
}

fn network_port(value: u32) -> u16 {
    u16::from_be(value as u16)
}

fn usable_remote_connection(connection: TcpConnection) -> bool {
    connection.client.port > 0
        && connection.server.port > 0
        && !connection.client.address.is_unspecified()
        && !connection.server.address.is_unspecified()
        && !connection.client.address.is_loopback()
        && !connection.server.address.is_loopback()
}

fn socket_table_status(status: u32) -> CaptureError {
    socket_table_error(format!(
        "GetExtendedTcpTable failed with Windows status {status}"
    ))
}

fn socket_table_error(message: impl Into<String>) -> CaptureError {
    CaptureError::Adapter {
        adapter: "windows-process-socket-owner".into(),
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_network_order_port_is_decoded() {
        assert_eq!(network_port(0x0000_50c3), 50_000);
        assert_eq!(network_port(0x0000_bb01), 443);
    }

    #[test]
    fn zero_and_loopback_connections_are_not_capture_candidates() {
        let remote = TcpEndpoint::new(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 1)), 443);
        assert!(!usable_remote_connection(TcpConnection::new(
            TcpEndpoint::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 50_000),
            remote,
        )));
        assert!(!usable_remote_connection(TcpConnection::new(
            TcpEndpoint::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
            remote,
        )));
    }
}
