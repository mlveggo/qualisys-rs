//! Packet framing and data frame decoding.

use crate::components::{
    Analog, Bodies6d, Bodies6dEuler, Component, ComponentType, EyeTrackers, Force, GazeVectors,
    Images, Markers2d, Markers3d, Skeletons, Timecodes,
};
use crate::cursor::{ByteOrder, Cursor};
use crate::error::{Error, Result};

/// Size of the size-and-type header every packet begins with.
pub const PACKET_HEADER_SIZE: usize = 8;

/// The wire tag identifying what a packet carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum PacketType {
    Error = 0,
    Command = 1,
    Xml = 2,
    Data = 3,
    NoMoreData = 4,
    C3dFile = 5,
    Event = 6,
    Discover = 7,
    QtmFile = 8,
    None = 9,
}

impl TryFrom<u32> for PacketType {
    type Error = u32;

    fn try_from(value: u32) -> std::result::Result<Self, u32> {
        Ok(match value {
            0 => PacketType::Error,
            1 => PacketType::Command,
            2 => PacketType::Xml,
            3 => PacketType::Data,
            4 => PacketType::NoMoreData,
            5 => PacketType::C3dFile,
            6 => PacketType::Event,
            7 => PacketType::Discover,
            8 => PacketType::QtmFile,
            9 => PacketType::None,
            other => return Err(other),
        })
    }
}

/// State transitions and notifications QTM pushes asynchronously.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EventType {
    Connected,
    ConnectionClosed,
    CaptureStarted,
    CaptureStopped,
    /// Not used in protocol version 1.10 and later.
    CaptureFetchingFinished,
    CalibrationStarted,
    CalibrationStopped,
    RtFromFileStarted,
    RtFromFileStopped,
    WaitingForTrigger,
    CameraSettingsChanged,
    QtmShuttingDown,
    CaptureSaved,
    ReprocessingStarted,
    ReprocessingStopped,
    Trigger,
    #[default]
    None,
    Unknown(u8),
}

impl From<u8> for EventType {
    fn from(value: u8) -> Self {
        match value {
            1 => EventType::Connected,
            2 => EventType::ConnectionClosed,
            3 => EventType::CaptureStarted,
            4 => EventType::CaptureStopped,
            5 => EventType::CaptureFetchingFinished,
            6 => EventType::CalibrationStarted,
            7 => EventType::CalibrationStopped,
            8 => EventType::RtFromFileStarted,
            9 => EventType::RtFromFileStopped,
            10 => EventType::WaitingForTrigger,
            11 => EventType::CameraSettingsChanged,
            12 => EventType::QtmShuttingDown,
            13 => EventType::CaptureSaved,
            14 => EventType::ReprocessingStarted,
            15 => EventType::ReprocessingStopped,
            16 => EventType::Trigger,
            17 => EventType::None,
            other => EventType::Unknown(other),
        }
    }
}

impl std::fmt::Display for EventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            EventType::Connected => "Connected",
            EventType::ConnectionClosed => "ConnectionClosed",
            EventType::CaptureStarted => "CaptureStarted",
            EventType::CaptureStopped => "CaptureStopped",
            EventType::CaptureFetchingFinished => "CaptureFetchingFinished",
            EventType::CalibrationStarted => "CalibrationStarted",
            EventType::CalibrationStopped => "CalibrationStopped",
            EventType::RtFromFileStarted => "RtFromFileStarted",
            EventType::RtFromFileStopped => "RtFromFileStopped",
            EventType::WaitingForTrigger => "WaitingForTrigger",
            EventType::CameraSettingsChanged => "CameraSettingsChanged",
            EventType::QtmShuttingDown => "QtmShuttingDown",
            EventType::CaptureSaved => "CaptureSaved",
            EventType::ReprocessingStarted => "ReprocessingStarted",
            EventType::ReprocessingStopped => "ReprocessingStopped",
            EventType::Trigger => "Trigger",
            EventType::None => "None",
            EventType::Unknown(v) => return write!(f, "Unknown({v})"),
        };
        f.write_str(s)
    }
}

/// One data frame: a timestamp, a frame number and the components requested.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DataFrame {
    /// Capture timestamp in microseconds.
    pub timestamp: u64,
    pub frame_number: u32,
    pub components: Vec<Component>,
}

impl DataFrame {
    /// Timestamp and frame number, plus the component count.
    const HEADER_BYTES: usize = 16;

    /// Decodes a data frame payload, i.e. everything after the packet header.
    ///
    /// Exposed so callers can decode frames captured from another transport.
    ///
    /// Each component parser receives exactly its own slice rather than
    /// everything to the end of the buffer, so a wrong count in one component
    /// cannot make its parser wander into the next component's bytes.
    ///
    /// A component type this crate does not recognise is preserved as
    /// [`Component::Unknown`] rather than failing the frame, so a client keeps
    /// working when QTM starts sending something new.
    pub fn decode(payload: &[u8], order: ByteOrder) -> Result<Self> {
        if payload.len() < Self::HEADER_BYTES {
            return Err(Error::MalformedFrame(format!(
                "need {} header bytes, have {}",
                Self::HEADER_BYTES,
                payload.len()
            )));
        }
        let mut c = Cursor::new(payload, order);
        let timestamp = c.u64()?;
        let frame_number = c.u32()?;
        let component_count = c.u32()?;

        let mut components = Vec::with_capacity(component_count.min(64) as usize);
        let mut pos = Self::HEADER_BYTES;

        for index in 0..component_count {
            if pos + PACKET_HEADER_SIZE > payload.len() {
                return Err(Error::MalformedFrame(format!(
                    "component {index} header runs past the end of the frame"
                )));
            }
            let mut head = Cursor::new(&payload[pos..pos + PACKET_HEADER_SIZE], order);
            let size = head.u32()? as usize;
            let component_type = head.u32()?;

            // A zero or undersized length would never advance the cursor.
            if size < PACKET_HEADER_SIZE {
                return Err(Error::MalformedFrame(format!(
                    "component {index} declares an invalid size of {size}"
                )));
            }
            if pos + size > payload.len() {
                return Err(Error::MalformedFrame(format!(
                    "component {index} claims {size} bytes, only {} remain",
                    payload.len() - pos
                )));
            }

            let body = &payload[pos + PACKET_HEADER_SIZE..pos + size];
            components.push(Component::decode(component_type, body, order)?);
            pos += size;
        }

        Ok(DataFrame {
            timestamp,
            frame_number,
            components,
        })
    }

    /// Returns the first component matching `kind`, if the frame carries one.
    pub fn component(&self, kind: ComponentType) -> Option<&Component> {
        self.components
            .iter()
            .find(|c| c.component_type() == Some(kind))
    }

    /// Component types present in the frame that this crate could not decode.
    pub fn unknown_component_types(&self) -> Vec<u32> {
        self.components
            .iter()
            .filter_map(|c| match c {
                Component::Unknown { component_type, .. } => Some(*component_type),
                _ => None,
            })
            .collect()
    }
}

/// Typed accessors for the common components.
///
/// Matching on [`Component`] works too, but these cover the usual "give me the
/// 3D markers from this frame" case without the boilerplate.
macro_rules! frame_accessor {
    ($name:ident, $variant:ident, $ty:ty, $doc:literal) => {
        impl DataFrame {
            #[doc = $doc]
            pub fn $name(&self) -> Option<&$ty> {
                self.components.iter().find_map(|c| match c {
                    Component::$variant(v) => Some(v),
                    _ => None,
                })
            }
        }
    };
}

frame_accessor!(
    markers_3d,
    Markers3d,
    Markers3d,
    "Labelled 3D markers, if the frame carries them."
);
frame_accessor!(
    markers_3d_residual,
    Markers3dResidual,
    Markers3d,
    "Labelled 3D markers with residuals, if present."
);
frame_accessor!(
    markers_3d_no_labels,
    Markers3dNoLabels,
    Markers3d,
    "Unlabelled 3D markers, if present."
);
frame_accessor!(
    bodies_6d,
    Bodies6d,
    Bodies6d,
    "6DOF bodies as rotation matrices, if present."
);
frame_accessor!(
    bodies_6d_euler,
    Bodies6dEuler,
    Bodies6dEuler,
    "6DOF bodies as Euler angles, if present."
);
frame_accessor!(
    markers_2d,
    Markers2d,
    Markers2d,
    "Per-camera 2D markers, if present."
);
frame_accessor!(analog, Analog, Analog, "Analog samples, if present.");
frame_accessor!(force, Force, Force, "Force plate samples, if present.");
frame_accessor!(images, Image, Images, "Camera images, if present.");
frame_accessor!(
    gaze_vectors,
    GazeVector,
    GazeVectors,
    "Gaze vectors, if present."
);
frame_accessor!(
    eye_trackers,
    EyeTracker,
    EyeTrackers,
    "Eye tracker samples, if present."
);
frame_accessor!(timecodes, Timecode, Timecodes, "Timecodes, if present.");
frame_accessor!(skeletons, Skeleton, Skeletons, "Skeletons, if present.");

/// A decoded packet.
///
/// Modelling this as an enum rather than a struct with one field per packet
/// kind means a caller cannot read a field that was never populated.
#[derive(Debug, Clone, PartialEq)]
pub enum Packet {
    /// QTM reported an error.
    Error(String),
    /// A command response.
    Command(String),
    /// Settings XML.
    Xml(String),
    /// A data frame.
    Data(DataFrame),
    /// The socket was idle for the whole read timeout, or QTM signalled the end
    /// of a finite stream.
    NoMoreData,
    /// A C3D file transfer.
    C3dFile(Vec<u8>),
    /// An asynchronous event.
    Event(EventType),
    /// A discovery response.
    Discover(Vec<u8>),
    /// A QTM file transfer.
    QtmFile(Vec<u8>),
    /// An empty or unrecognised packet.
    None,
}

impl Packet {
    /// A short human-readable name, used in error messages.
    pub fn kind_name(&self) -> &'static str {
        match self {
            Packet::Error(_) => "error",
            Packet::Command(_) => "command",
            Packet::Xml(_) => "xml",
            Packet::Data(_) => "data",
            Packet::NoMoreData => "no-more-data",
            Packet::C3dFile(_) => "c3d-file",
            Packet::Event(_) => "event",
            Packet::Discover(_) => "discover",
            Packet::QtmFile(_) => "qtm-file",
            Packet::None => "none",
        }
    }

    /// True when the socket produced nothing within the read timeout.
    pub fn is_end_of_data(&self) -> bool {
        matches!(self, Packet::NoMoreData)
    }

    /// The data frame, if this is a data packet.
    pub fn data(&self) -> Option<&DataFrame> {
        match self {
            Packet::Data(frame) => Some(frame),
            _ => None,
        }
    }

    /// Decodes a complete packet, header included.
    ///
    /// Exposed so callers can decode packets received over a transport this
    /// crate does not manage.
    pub fn decode(buffer: &[u8], order: ByteOrder) -> Result<Packet> {
        if buffer.len() < PACKET_HEADER_SIZE {
            return Err(Error::InvalidPacketSize(buffer.len()));
        }
        let mut head = Cursor::new(&buffer[..PACKET_HEADER_SIZE], order);
        let _size = head.u32()?;
        let raw_type = head.u32()?;
        let payload = &buffer[PACKET_HEADER_SIZE..];

        let packet_type = match PacketType::try_from(raw_type) {
            Ok(t) => t,
            Err(_) => return Ok(Packet::None),
        };

        Ok(match packet_type {
            PacketType::Error => Packet::Error(decode_string(payload)?),
            PacketType::Command => Packet::Command(decode_string(payload)?),
            PacketType::Xml => Packet::Xml(decode_string(payload)?),
            PacketType::Data => Packet::Data(DataFrame::decode(payload, order)?),
            PacketType::NoMoreData => Packet::NoMoreData,
            // The payload of a file packet is the file content itself, starting
            // immediately after the packet header.
            PacketType::C3dFile => Packet::C3dFile(payload.to_vec()),
            PacketType::QtmFile => Packet::QtmFile(payload.to_vec()),
            PacketType::Event => {
                let first = payload
                    .first()
                    .ok_or_else(|| Error::MalformedFrame("event packet has no payload".into()))?;
                Packet::Event(EventType::from(*first))
            }
            PacketType::Discover => Packet::Discover(payload.to_vec()),
            PacketType::None => Packet::None,
        })
    }
}

/// Decodes a null-terminated string field.
///
/// Trailing NULs are stripped before the UTF-8 check so a well-formed response
/// is not rejected because of its terminator.
fn decode_string(payload: &[u8]) -> Result<String> {
    let end = payload
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(payload.len());
    Ok(std::str::from_utf8(&payload[..end])?.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(packet_type: PacketType, payload: &[u8]) -> Vec<u8> {
        let size = PACKET_HEADER_SIZE + payload.len();
        let mut v = Vec::with_capacity(size);
        v.extend_from_slice(&(size as u32).to_le_bytes());
        v.extend_from_slice(&(packet_type as u32).to_le_bytes());
        v.extend_from_slice(payload);
        v
    }

    #[test]
    fn decodes_a_command_response() {
        let raw = encode(PacketType::Command, b"QTM RT Interface connected\0");
        let p = Packet::decode(&raw, ByteOrder::Little).unwrap();
        assert_eq!(p, Packet::Command("QTM RT Interface connected".into()));
    }

    #[test]
    fn event_packet_without_payload_is_an_error() {
        let raw = encode(PacketType::Event, b"");
        assert!(Packet::decode(&raw, ByteOrder::Little).is_err());
    }

    #[test]
    fn file_packet_keeps_every_byte() {
        // The payload of a file transfer is the file itself; treating the first
        // bytes as a nested header would corrupt every capture.
        let content = b"C3D\x00\x01\x02\x03\x04and the rest";
        let raw = encode(PacketType::C3dFile, content);
        match Packet::decode(&raw, ByteOrder::Little).unwrap() {
            Packet::C3dFile(bytes) => assert_eq!(bytes, content.to_vec()),
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn header_only_buffer_is_rejected() {
        assert!(Packet::decode(&[0u8; 4], ByteOrder::Little).is_err());
    }

    #[test]
    fn non_utf8_command_is_an_error_not_a_panic() {
        let raw = encode(PacketType::Command, &[0xFF, 0xFE, 0x00]);
        assert!(Packet::decode(&raw, ByteOrder::Little).is_err());
    }
}
