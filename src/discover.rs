//! Discovery of QTM servers by UDP broadcast.

use crate::error::{Error, Result};
use crate::packet::{PacketType, PACKET_HEADER_SIZE};
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

/// The port QTM listens on for discovery broadcasts.
pub const DISCOVERY_PORT: u16 = 22226;

/// A QTM server that answered a discovery broadcast.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Server {
    /// Address the response came from.
    pub address: String,
    pub hostname: String,
    pub qtm_version: String,
    pub camera_count: u32,
    /// QTM's base port. Add 1 for the little-endian RT port.
    pub base_port: u16,
}

impl std::fmt::Display for Server {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} ({}) at {}:{} with {} cameras",
            self.hostname, self.qtm_version, self.address, self.base_port, self.camera_count
        )
    }
}

impl Server {
    /// Parses a discovery response payload.
    ///
    /// The layout is an 8 byte packet header, a comma separated information
    /// string, and a big-endian `u16` base port as the final two bytes.
    pub fn parse(address: &str, data: &[u8]) -> Result<Server> {
        // Header, at least one byte of information, and the trailing port.
        const MIN_LEN: usize = PACKET_HEADER_SIZE + 1 + 2;
        if data.len() < MIN_LEN {
            return Err(Error::MalformedFrame(format!(
                "discovery response is {} bytes, need at least {MIN_LEN}",
                data.len()
            )));
        }

        let info_end = data.len() - 2;
        let info_bytes = &data[PACKET_HEADER_SIZE..info_end];
        let info = std::str::from_utf8(info_bytes)?.trim_end_matches('\0');

        let parts: Vec<&str> = info.split(',').map(str::trim).collect();
        if parts.len() != 3 {
            return Err(Error::MalformedFrame(format!(
                "discovery information field has {} comma separated parts, expected 3",
                parts.len()
            )));
        }

        let camera_count = parts[2]
            .split_whitespace()
            .next()
            .and_then(|n| n.parse::<u32>().ok())
            .unwrap_or(0);

        let base_port = u16::from_be_bytes([data[info_end], data[info_end + 1]]);

        Ok(Server {
            address: address.to_string(),
            hostname: parts[0].to_string(),
            qtm_version: parts[1].to_string(),
            camera_count,
            base_port,
        })
    }
}

/// Broadcasts a discovery request and collects responses for `timeout`.
///
/// The socket binds to an ephemeral port; QTM is told which port to reply to in
/// the request payload.
pub fn discover(timeout: Duration) -> Result<Vec<Server>> {
    discover_on_port(0, timeout)
}

/// Discovers QTM servers, binding the response socket to a specific local port.
///
/// Pass 0 to let the operating system choose.
pub fn discover_on_port(local_port: u16, timeout: Duration) -> Result<Vec<Server>> {
    let socket = UdpSocket::bind(("0.0.0.0", local_port))?;
    socket.set_broadcast(true)?;
    socket.set_read_timeout(Some(timeout))?;

    let reply_port = socket.local_addr()?.port();
    let request = discovery_request(reply_port);

    socket.send_to(
        &request,
        SocketAddr::from(([255, 255, 255, 255], DISCOVERY_PORT)),
    )?;

    let deadline = Instant::now() + timeout;
    let mut servers = Vec::new();
    let mut buffer = vec![0u8; 2048];

    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        socket.set_read_timeout(Some(remaining))?;

        let (received, from) = match socket.recv_from(&mut buffer) {
            Ok(v) => v,
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) =>
            {
                break
            }
            Err(e) => return Err(Error::Io(e)),
        };

        let address = match from.ip() {
            IpAddr::V4(v4) => v4.to_string(),
            IpAddr::V6(v6) => v6.to_string(),
        };

        // A malformed response from one host should not abort discovery of the
        // rest of the network.
        match Server::parse(&address, &buffer[..received]) {
            Ok(server) => {
                if !servers.contains(&server) {
                    servers.push(server);
                }
            }
            Err(e) => log::debug!("ignoring discovery response from {address}: {e}"),
        }
    }
    Ok(servers)
}

/// Builds the 10 byte discovery request: header plus the big-endian port QTM
/// should reply to.
fn discovery_request(reply_port: u16) -> [u8; 10] {
    let mut request = [0u8; 10];
    request[0..4].copy_from_slice(&10u32.to_le_bytes());
    request[4..8].copy_from_slice(&(PacketType::Discover as u32).to_le_bytes());
    request[8..10].copy_from_slice(&reply_port.to_be_bytes());
    request
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(info: &str, base_port: u16) -> Vec<u8> {
        let mut v = vec![0u8; PACKET_HEADER_SIZE];
        v.extend_from_slice(info.as_bytes());
        v.extend_from_slice(&base_port.to_be_bytes());
        let len = v.len() as u32;
        v[0..4].copy_from_slice(&len.to_le_bytes());
        v[4..8].copy_from_slice(&(PacketType::Command as u32).to_le_bytes());
        v
    }

    #[test]
    fn parses_a_response() {
        let raw = response("TestHost, QTM 2025.1 32300, 1234 cameras", 22222);
        let server = Server::parse("10.0.0.5", &raw).unwrap();
        assert_eq!(server.hostname, "TestHost");
        assert_eq!(server.qtm_version, "QTM 2025.1 32300");
        assert_eq!(server.camera_count, 1234);
        assert_eq!(server.base_port, 22222);
        assert_eq!(server.address, "10.0.0.5");
    }

    #[test]
    fn rejects_a_short_response() {
        assert!(Server::parse("10.0.0.5", &[0u8; 4]).is_err());
    }

    #[test]
    fn rejects_a_malformed_information_field() {
        let raw = response("no commas here", 22222);
        assert!(Server::parse("10.0.0.5", &raw).is_err());
    }

    #[test]
    fn discovery_request_has_the_expected_layout() {
        let request = discovery_request(4545);
        assert_eq!(
            u32::from_le_bytes([request[0], request[1], request[2], request[3]]),
            10
        );
        assert_eq!(
            u32::from_le_bytes([request[4], request[5], request[6], request[7]]),
            7
        );
        assert_eq!(u16::from_be_bytes([request[8], request[9]]), 4545);
    }
}
