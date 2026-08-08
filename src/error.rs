//! Error types for the QTM real time protocol client.

use std::fmt;

/// Convenience alias for results produced by this crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Everything that can go wrong talking to QTM.
///
/// Variants are deliberately specific so callers can react differently to, say,
/// a quiet socket versus a desynchronised stream, without matching on strings.
#[derive(Debug)]
pub enum Error {
    /// Underlying socket failure.
    Io(std::io::Error),

    /// An operation needing a live connection was attempted without one.
    NotConnected,

    /// Nothing arrived within the configured timeout.
    ///
    /// This is distinct from [`Packet::NoMoreData`](crate::Packet::NoMoreData),
    /// which a read returns when the socket is simply idle.
    Timeout,

    /// A packet header was read but its body never fully arrived.
    ///
    /// The stream is desynchronised at this point: some bytes of the packet have
    /// been consumed and the rest are still queued, so the next read would
    /// interpret payload bytes as a header. The connection must be rebuilt.
    Truncated { expected: usize, received: usize },

    /// A component payload ended before all its declared fields were present.
    ShortPacket {
        needed: usize,
        offset: usize,
        available: usize,
    },

    /// The packet size field was smaller than the 8 byte header.
    InvalidPacketSize(usize),

    /// The packet size field exceeded the configured ceiling.
    PacketTooLarge { size: usize, limit: usize },

    /// QTM accepted none of the protocol versions this crate is willing to
    /// speak.
    VersionNotSupported { tried: Vec<(u32, u32)> },

    /// A command returned something other than one of its expected responses.
    UnexpectedResponse {
        command: String,
        got: String,
        expected: Vec<String>,
    },

    /// A packet of the wrong kind arrived where a specific one was required.
    UnexpectedPacket {
        expected: &'static str,
        got: &'static str,
    },

    /// QTM replied with an error packet.
    Qtm(String),

    /// A data frame was structurally invalid.
    MalformedFrame(String),

    /// A string field was not valid UTF-8.
    Utf8(std::str::Utf8Error),

    /// A caller-supplied argument was rejected before anything was sent.
    InvalidArgument(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "io error: {e}"),
            Error::NotConnected => write!(f, "not connected"),
            Error::Timeout => write!(f, "timed out waiting for a response"),
            Error::Truncated { expected, received } => write!(
                f,
                "packet truncated: expected {expected} bytes, received {received}; \
                 the stream is desynchronised and the connection must be reopened"
            ),
            Error::ShortPacket {
                needed,
                offset,
                available,
            } => write!(
                f,
                "short packet: needed {needed} bytes at offset {offset}, {available} available"
            ),
            Error::InvalidPacketSize(size) => {
                write!(f, "invalid packet size {size}, must be at least 8")
            }
            Error::PacketTooLarge { size, limit } => {
                write!(f, "packet size {size} exceeds the configured limit {limit}")
            }
            Error::VersionNotSupported { tried } => {
                write!(f, "no mutually supported protocol version; tried ")?;
                for (i, (major, minor)) in tried.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{major}.{minor}")?;
                }
                Ok(())
            }
            Error::UnexpectedResponse {
                command,
                got,
                expected,
            } => write!(
                f,
                "command {command:?} returned {got:?}, expected one of {expected:?}"
            ),
            Error::UnexpectedPacket { expected, got } => {
                write!(f, "expected a {expected} packet, got {got}")
            }
            Error::Qtm(msg) => write!(f, "qtm returned an error: {msg}"),
            Error::MalformedFrame(msg) => write!(f, "malformed data frame: {msg}"),
            Error::Utf8(e) => write!(f, "invalid utf-8 in response: {e}"),
            Error::InvalidArgument(msg) => write!(f, "invalid argument: {msg}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            Error::Utf8(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<std::str::Utf8Error> for Error {
    fn from(e: std::str::Utf8Error) -> Self {
        Error::Utf8(e)
    }
}

impl Error {
    /// True when this error came from a socket read or write timing out.
    pub fn is_timeout(&self) -> bool {
        match self {
            Error::Timeout => true,
            Error::Io(e) => matches!(
                e.kind(),
                std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
            ),
            _ => false,
        }
    }
}
