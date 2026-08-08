//! A client for the Qualisys Track Manager (QTM) real time protocol.
//!
//! The crate speaks RT protocol 1.28 and negotiates down to 1.22 when talking
//! to an older QTM, mirroring the negotiation in the official
//! [C++ SDK](https://github.com/qualisys/qualisys_cpp_sdk).
//!
//! # Discovering and connecting
//!
//! ```no_run
//! use qualisys::{discover, Protocol};
//! use std::time::Duration;
//!
//! let found = discover::discover(Duration::from_secs(1))?;
//! let server = found.first().expect("no QTM on the network");
//!
//! let mut rt = Protocol::connect(&server.address, server.base_port)?;
//! println!("negotiated protocol {:?}", rt.version());
//! # Ok::<(), qualisys::Error>(())
//! ```
//!
//! # Streaming
//!
//! ```no_run
//! use qualisys::{ComponentOptions, ComponentType, Packet, Protocol, StreamRate};
//!
//! let mut rt = Protocol::connect("192.168.0.10", qualisys::DEFAULT_BASE_PORT)?;
//! rt.stream_frames(
//!     StreamRate::AllFrames,
//!     &[ComponentType::Markers3d],
//!     &ComponentOptions::default(),
//! )?;
//!
//! loop {
//!     match rt.receive()? {
//!         Packet::Data(frame) => {
//!             if let Some(markers) = frame.markers_3d() {
//!                 println!("frame {}: {} markers", frame.frame_number, markers.markers.len());
//!             }
//!         }
//!         // An idle socket is not an error; it just means nothing arrived
//!         // within the read timeout.
//!         Packet::NoMoreData => continue,
//!         Packet::Event(event) => println!("event: {event}"),
//!         _ => {}
//!     }
//! }
//! # Ok::<(), qualisys::Error>(())
//! ```
//!
//! # Error handling
//!
//! [`Error::Truncated`] deserves special mention: it means a packet header was
//! read but the body never fully arrived, which leaves the stream
//! desynchronised. Unlike other errors it is not recoverable in place and the
//! connection has to be reopened.

#![warn(missing_debug_implementations)]
// Compiles every example in the README as part of the test suite, so the
// documentation cannot drift away from the API.
#![doc = include_str!("../README.md")]

pub mod commands;
pub mod components;
mod cursor;
pub mod discover;
pub mod error;
pub mod packet;
pub mod protocol;

pub use commands::{ComponentOptions, LedColor, LedMode, Parameter, StreamRate};
pub use components::{Component, ComponentType, Point};
pub use cursor::ByteOrder;
pub use error::{Error, Result};
pub use packet::{DataFrame, EventType, Packet, PacketType};
pub use protocol::{
    Builder, Config, Protocol, DEFAULT_BASE_PORT, DEFAULT_MAX_PACKET_SIZE, DEFAULT_VERSION,
    MIN_SUPPORTED_MINOR,
};
