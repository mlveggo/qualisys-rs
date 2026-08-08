//! The QTM real time protocol client.

use crate::cursor::ByteOrder;
use crate::error::{Error, Result};
use crate::packet::{EventType, Packet, PACKET_HEADER_SIZE};
use log::debug;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs, UdpSocket};
use std::time::{Duration, Instant};

/// Newest protocol version this crate knows about.
///
/// Matches `MAJOR_VERSION`/`MINOR_VERSION` in the C++ SDK's `RTPacket.h`.
pub const DEFAULT_VERSION: (u32, u32) = (1, 28);

/// Oldest protocol version [`Protocol::connect`] will negotiate down to.
pub const MIN_SUPPORTED_MINOR: u32 = 22;

/// QTM's base port. The little-endian listener is at `base + 1` and the
/// big-endian listener at `base + 2`; the base port itself speaks protocol
/// version 1.0, which this crate does not implement.
pub const DEFAULT_BASE_PORT: u16 = 22222;

/// Caps how large a packet may claim to be.
///
/// Image and file packets are legitimately large, but a size field taken
/// straight off the wire is an easy way to make a client allocate itself to
/// death.
pub const DEFAULT_MAX_PACKET_SIZE: usize = 512 * 1024 * 1024;

const WELCOME_MESSAGE: &str = "QTM RT Interface connected";

/// Configuration for a [`Protocol`] connection.
#[derive(Debug, Clone)]
pub struct Config {
    /// Protocol version to request first.
    pub version: (u32, u32),
    /// Whether to fall back to older versions when QTM rejects the request.
    pub negotiate_version: bool,
    /// Byte order, which also selects the port offset.
    pub byte_order: ByteOrder,
    /// How long [`Protocol::receive`] waits for a packet to start arriving.
    pub read_timeout: Duration,
    /// TCP connect timeout.
    pub connect_timeout: Duration,
    /// How long to wait for a command response.
    pub command_timeout: Duration,
    /// Largest packet that will be accepted.
    pub max_packet_size: usize,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            version: DEFAULT_VERSION,
            negotiate_version: true,
            byte_order: ByteOrder::Little,
            read_timeout: Duration::from_secs(1),
            connect_timeout: Duration::from_secs(5),
            command_timeout: Duration::from_secs(5),
            max_packet_size: DEFAULT_MAX_PACKET_SIZE,
        }
    }
}

/// Builder for a [`Protocol`] connection.
///
/// ```no_run
/// use qualisys::Protocol;
/// use std::time::Duration;
///
/// let rt = Protocol::builder()
///     .version(1, 25)
///     .read_timeout(Duration::from_millis(500))
///     .connect("192.168.0.10", qualisys::DEFAULT_BASE_PORT)?;
/// # Ok::<(), qualisys::Error>(())
/// ```
#[derive(Debug, Clone, Default)]
pub struct Builder {
    config: Config,
}

impl Builder {
    /// Requests a specific protocol version instead of the crate default.
    pub fn version(mut self, major: u32, minor: u32) -> Self {
        self.config.version = (major, minor);
        self
    }

    /// Disables falling back to older protocol versions.
    pub fn without_version_negotiation(mut self) -> Self {
        self.config.negotiate_version = false;
        self
    }

    /// Selects the big-endian port and byte order.
    pub fn big_endian(mut self) -> Self {
        self.config.byte_order = ByteOrder::Big;
        self
    }

    pub fn read_timeout(mut self, d: Duration) -> Self {
        self.config.read_timeout = d;
        self
    }

    pub fn connect_timeout(mut self, d: Duration) -> Self {
        self.config.connect_timeout = d;
        self
    }

    pub fn command_timeout(mut self, d: Duration) -> Self {
        self.config.command_timeout = d;
        self
    }

    pub fn max_packet_size(mut self, bytes: usize) -> Self {
        self.config.max_packet_size = bytes;
        self
    }

    /// Opens the connection and completes the handshake.
    pub fn connect(self, host: &str, base_port: u16) -> Result<Protocol> {
        Protocol::connect_with(host, base_port, self.config)
    }
}

/// A connected QTM real time client.
///
/// The connection is established by [`Protocol::connect`], so a `Protocol`
/// value always owns a live socket. There is no disconnected state to check
/// for, and dropping the value closes the connection.
#[derive(Debug)]
pub struct Protocol {
    stream: TcpStream,
    udp: Option<UdpSocket>,
    buffer: Vec<u8>,
    config: Config,
    version: (u32, u32),
    state: EventType,
    last_event: EventType,
}

impl Protocol {
    /// Starts building a connection.
    pub fn builder() -> Builder {
        Builder::default()
    }

    /// Connects with default settings, negotiating the newest protocol version
    /// QTM accepts.
    pub fn connect(host: &str, base_port: u16) -> Result<Protocol> {
        Protocol::connect_with(host, base_port, Config::default())
    }

    /// Connects using an explicit [`Config`].
    ///
    /// Every failure path drops the socket before returning, so a caller
    /// retrying in a loop always starts from a clean state.
    pub fn connect_with(host: &str, base_port: u16, config: Config) -> Result<Protocol> {
        let port = match config.byte_order {
            ByteOrder::Little => base_port.saturating_add(1),
            ByteOrder::Big => base_port.saturating_add(2),
        };
        let addr = resolve(host, port)?;
        debug!("connecting to {addr}");

        let stream = TcpStream::connect_timeout(&addr, config.connect_timeout)?;
        stream.set_read_timeout(Some(config.read_timeout))?;
        stream.set_write_timeout(Some(config.connect_timeout))?;
        stream.set_nodelay(true)?;

        let mut rt = Protocol {
            stream,
            udp: None,
            buffer: vec![0; 4096],
            config,
            version: (0, 0),
            state: EventType::None,
            last_event: EventType::None,
        };

        match rt.receive_timeout(rt.config.connect_timeout)? {
            Packet::Command(msg) if msg == WELCOME_MESSAGE => {}
            Packet::Error(msg) => return Err(Error::Qtm(msg)),
            other => {
                return Err(Error::UnexpectedResponse {
                    command: "<welcome>".into(),
                    got: format!("{other:?}"),
                    expected: vec![WELCOME_MESSAGE.into()],
                })
            }
        }

        let candidates = rt.version_candidates();
        for &(major, minor) in &candidates {
            match rt.set_version(major, minor) {
                Ok(()) => {
                    // Prime the cached state the way the C++ SDK does after a
                    // successful handshake. Not every configuration answers, so
                    // a failure here is not fatal.
                    let _ = rt.state();
                    return Ok(rt);
                }
                Err(e) => debug!("version {major}.{minor} rejected: {e}"),
            }
        }
        Err(Error::VersionNotSupported { tried: candidates })
    }

    /// Builds the ordered list of versions to try.
    ///
    /// This mirrors `RTVersion::VersionList` in the C++ SDK: the requested
    /// version first, then progressively older ones, skipping anything newer
    /// than or equal to what was asked for. The ladder stops at
    /// [`MIN_SUPPORTED_MINOR`] so that connecting to a QTM older than the
    /// documented floor fails cleanly rather than negotiating something this
    /// crate cannot parse.
    fn version_candidates(&self) -> Vec<(u32, u32)> {
        let (want_major, want_minor) = self.config.version;
        let mut out = vec![(want_major, want_minor)];
        if !self.config.negotiate_version {
            return out;
        }
        let (default_major, default_minor) = DEFAULT_VERSION;
        for minor in (MIN_SUPPORTED_MINOR..=default_minor).rev() {
            if want_major == default_major && minor >= want_minor {
                continue;
            }
            out.push((default_major, minor));
        }
        out
    }

    /// The protocol version actually negotiated.
    pub fn version(&self) -> (u32, u32) {
        self.version
    }

    /// The settings XML root element name for the negotiated version, for
    /// example `QTM_Parameters_Ver_1.28`.
    ///
    /// Callers editing settings XML should use this rather than hard-coding a
    /// version, since the element name changes with every protocol revision.
    pub fn parameters_element_name(&self) -> String {
        format!("QTM_Parameters_Ver_{}.{}", self.version.0, self.version.1)
    }

    /// The most recent event QTM reported, including events observed while
    /// waiting for a command response.
    pub fn last_event(&self) -> EventType {
        self.last_event
    }

    pub(crate) fn set_negotiated_version(&mut self, major: u32, minor: u32) {
        self.version = (major, minor);
    }

    pub(crate) fn config(&self) -> &Config {
        &self.config
    }

    /// The local address of the TCP connection.
    pub fn local_addr(&self) -> Result<SocketAddr> {
        Ok(self.stream.local_addr()?)
    }

    /// Sends a framed string packet.
    ///
    /// The header is written in the connection's byte order; writing it always
    /// little-endian would leave a big-endian connection able to receive but
    /// never send.
    pub(crate) fn send_string(
        &mut self,
        s: &str,
        packet_type: crate::packet::PacketType,
    ) -> Result<()> {
        let size = PACKET_HEADER_SIZE + s.len() + 1;
        let mut out = Vec::with_capacity(size);
        out.extend_from_slice(&self.config.byte_order.put_u32(size as u32));
        out.extend_from_slice(&self.config.byte_order.put_u32(packet_type as u32));
        out.extend_from_slice(s.as_bytes());
        out.push(0);
        self.stream.write_all(&out)?;
        self.stream.flush()?;
        Ok(())
    }

    /// Reads the next packet using the configured read timeout.
    ///
    /// An idle socket yields [`Packet::NoMoreData`] rather than an error, so a
    /// polling loop can treat "nothing yet" as an ordinary outcome. A body that
    /// starts but never finishes is a different matter and produces
    /// [`Error::Truncated`].
    pub fn receive(&mut self) -> Result<Packet> {
        self.receive_timeout(self.config.read_timeout)
    }

    /// Reads the next packet, waiting at most `timeout` for it to begin.
    pub fn receive_timeout(&mut self, timeout: Duration) -> Result<Packet> {
        self.stream.set_read_timeout(Some(timeout))?;

        // Read the fixed 8 byte header. `read_exact` matters here: TCP is free
        // to deliver fewer than 8 bytes on the first read, and treating a short
        // read as a fatal error makes the client fail intermittently under
        // load.
        match self
            .stream
            .read_exact(&mut self.buffer[..PACKET_HEADER_SIZE])
        {
            Ok(()) => {}
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) =>
            {
                return Ok(Packet::NoMoreData)
            }
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                return Err(Error::Io(std::io::Error::new(
                    std::io::ErrorKind::ConnectionAborted,
                    "connection closed by QTM",
                )))
            }
            Err(e) => return Err(Error::Io(e)),
        }

        let order = self.config.byte_order;
        let size = order.u32([
            self.buffer[0],
            self.buffer[1],
            self.buffer[2],
            self.buffer[3],
        ]) as usize;

        if size < PACKET_HEADER_SIZE {
            return Err(Error::InvalidPacketSize(size));
        }
        if size > self.config.max_packet_size {
            return Err(Error::PacketTooLarge {
                size,
                limit: self.config.max_packet_size,
            });
        }

        if self.buffer.len() < size {
            self.buffer.resize(size, 0);
        }

        // Once the header is consumed the rest of the packet must arrive.
        // Reporting a mid-packet timeout as "no more data" would leave the
        // remaining bytes queued to be misread as the next packet header.
        if size > PACKET_HEADER_SIZE {
            self.stream
                .set_read_timeout(Some(self.config.read_timeout))?;
            if let Err(e) = self
                .stream
                .read_exact(&mut self.buffer[PACKET_HEADER_SIZE..size])
            {
                return Err(match e.kind() {
                    std::io::ErrorKind::TimedOut
                    | std::io::ErrorKind::WouldBlock
                    | std::io::ErrorKind::UnexpectedEof => Error::Truncated {
                        expected: size,
                        received: PACKET_HEADER_SIZE,
                    },
                    _ => Error::Io(e),
                });
            }
        }

        let packet = Packet::decode(&self.buffer[..size], order)?;

        if let Packet::Event(event) = &packet {
            self.last_event = *event;
            // Camera settings changes are notifications rather than state
            // transitions, matching the C++ SDK.
            if *event != EventType::CameraSettingsChanged {
                self.state = *event;
            }
        }
        Ok(packet)
    }

    /// Reads until a non-event packet arrives or `timeout` elapses.
    ///
    /// QTM pushes events asynchronously, so a command response can be preceded
    /// by any number of event packets. Treating whatever arrives first as the
    /// response makes commands fail for no reason whenever an event happens to
    /// be in flight.
    pub(crate) fn receive_skipping_events(&mut self, timeout: Duration) -> Result<Packet> {
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(Error::Timeout);
            }
            match self.receive_timeout(remaining)? {
                Packet::Event(_) => continue,
                Packet::NoMoreData => {
                    if Instant::now() >= deadline {
                        return Err(Error::Timeout);
                    }
                    continue;
                }
                other => return Ok(other),
            }
        }
    }

    /// Opens a UDP socket for receiving streamed data.
    ///
    /// Pass port 0 to let the operating system choose. The returned port is
    /// what should be handed to
    /// [`stream_frames_udp`](Protocol::stream_frames_udp).
    pub fn enable_udp_stream(&mut self, port: u16) -> Result<u16> {
        let socket = UdpSocket::bind(("0.0.0.0", port))?;
        socket.set_read_timeout(Some(self.config.read_timeout))?;
        let local = socket.local_addr()?;
        self.udp = Some(socket);
        Ok(local.port())
    }

    /// The local port of the UDP stream socket, if one is open.
    pub fn udp_port(&self) -> Option<u16> {
        self.udp
            .as_ref()
            .and_then(|s| s.local_addr().ok())
            .map(|a| a.port())
    }

    /// Reads one datagram from the UDP stream socket.
    ///
    /// Each QTM datagram carries exactly one complete packet, so unlike the TCP
    /// path there is no reassembly to do.
    pub fn receive_udp(&mut self) -> Result<Packet> {
        let socket = self.udp.as_ref().ok_or(Error::NotConnected)?;
        let mut buf = vec![0u8; 65536];
        let received = match socket.recv(&mut buf) {
            Ok(n) => n,
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) =>
            {
                return Ok(Packet::NoMoreData)
            }
            Err(e) => return Err(Error::Io(e)),
        };
        if received < PACKET_HEADER_SIZE {
            return Err(Error::InvalidPacketSize(received));
        }
        Packet::decode(&buf[..received], self.config.byte_order)
    }
}

fn resolve(host: &str, port: u16) -> Result<SocketAddr> {
    (host, port)
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| Error::InvalidArgument(format!("could not resolve {host}:{port}")))
}
