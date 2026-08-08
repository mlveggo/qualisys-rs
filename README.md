# qualisys-rs

A Rust client for the Qualisys Track Manager (QTM) real time protocol.

Speaks RT protocol **1.28** and negotiates down to **1.22** against older QTM
installations, mirroring the official
[C++ SDK](https://github.com/qualisys/qualisys_cpp_sdk).

The library depends only on `log`. `env_logger` is used by the bundled command
line client and can be dropped with `--no-default-features`.

Every example below is compiled as part of the test suite.

## Quick start

```rust,no_run
use qualisys::{ComponentOptions, ComponentType, Packet, Protocol, StreamRate};

fn main() -> Result<(), qualisys::Error> {
    let mut rt = Protocol::connect("192.168.0.10", qualisys::DEFAULT_BASE_PORT)?;
    println!("negotiated protocol {:?}", rt.version());

    rt.stream_frames(
        StreamRate::AllFrames,
        &[ComponentType::Markers3d],
        &ComponentOptions::default(),
    )?;

    loop {
        match rt.receive()? {
            Packet::Data(frame) => {
                if let Some(markers) = frame.markers_3d() {
                    println!(
                        "frame {}: {} markers",
                        frame.frame_number,
                        markers.markers.len()
                    );
                }
            }
            // An idle socket is not an error.
            Packet::NoMoreData => continue,
            Packet::Event(event) => println!("event: {event}"),
            _ => {}
        }
    }
}
```

## Discovery

```rust,no_run
use std::time::Duration;

fn main() -> Result<(), qualisys::Error> {
    for server in qualisys::discover::discover(Duration::from_secs(1))? {
        println!("{server}");
    }
    Ok(())
}
```

## Version negotiation

`Protocol::connect` requests 1.28 and walks down to 1.22, returning
`Error::VersionNotSupported` only if QTM accepts none of them. Pin a version or
disable the fallback through the builder:

```rust,no_run
use qualisys::Protocol;

fn main() -> Result<(), qualisys::Error> {
    let rt = Protocol::builder()
        .version(1, 25)
        .without_version_negotiation()
        .connect("192.168.0.10", qualisys::DEFAULT_BASE_PORT)?;
    println!("{:?}", rt.version());
    Ok(())
}
```

The settings XML root element is named after the negotiated version, so it must
never be hard-coded:

```rust,no_run
use qualisys::{Parameter, Protocol};

fn main() -> Result<(), qualisys::Error> {
    let mut rt = Protocol::connect("192.168.0.10", qualisys::DEFAULT_BASE_PORT)?;
    let xml = rt.get_parameters(&[Parameter::Image])?;

    // Drops the <QTM_Parameters_Ver_X.Y> wrapper for whichever version was
    // actually negotiated.
    let fragment = rt.strip_parameters_element(&xml);
    rt.set_parameters(&fragment)?;
    Ok(())
}
```

## Component options

Analog channel selection and global skeleton coordinates:

```rust,no_run
use qualisys::{ComponentOptions, ComponentType, Protocol, StreamRate};

fn main() -> Result<(), qualisys::Error> {
    let mut rt = Protocol::connect("192.168.0.10", qualisys::DEFAULT_BASE_PORT)?;

    let options = ComponentOptions::default()
        .analog_channels("1,3,5-8") // sends "Analog:1,3,5-8"
        .skeleton_global(); // sends "Skeleton:global"

    rt.stream_frames(
        StreamRate::Frequency(100),
        &[ComponentType::Analog, ComponentType::Skeleton],
        &options,
    )?;
    Ok(())
}
```

## UDP streaming

Commands stay on TCP while data frames go to a UDP socket:

```rust,no_run
use qualisys::{ComponentOptions, ComponentType, Protocol, StreamRate};

fn main() -> Result<(), qualisys::Error> {
    let mut rt = Protocol::connect("192.168.0.10", qualisys::DEFAULT_BASE_PORT)?;

    let port = rt.enable_udp_stream(0)?; // 0 lets the OS choose
    rt.stream_frames_udp(
        StreamRate::AllFrames,
        &[ComponentType::Markers3d],
        &ComponentOptions::default(),
        port,
        None, // QTM replies to the TCP connection's address
    )?;

    let packet = rt.receive_udp()?;
    println!("{packet:?}");
    Ok(())
}
```

## Configuration

```rust,no_run
use qualisys::Protocol;
use std::time::Duration;

fn main() -> Result<(), qualisys::Error> {
    let rt = Protocol::builder()
        .read_timeout(Duration::from_millis(500))
        .connect_timeout(Duration::from_secs(10))
        .max_packet_size(64 * 1024 * 1024)
        .connect("192.168.0.10", qualisys::DEFAULT_BASE_PORT)?;
    println!("{:?}", rt.version());
    Ok(())
}
```

`calibrate` takes its own timeout, since a calibration runs for minutes:

```rust,no_run
use qualisys::Protocol;
use std::time::Duration;

fn main() -> Result<(), qualisys::Error> {
    let mut rt = Protocol::connect("192.168.0.10", qualisys::DEFAULT_BASE_PORT)?;
    rt.take_control("")?;
    let calibration_xml = rt.calibrate(false, Duration::from_secs(300))?;
    println!("{calibration_xml}");
    Ok(())
}
```

## Errors

| Variant                      | Meaning                                                    |
| ---------------------------- | ---------------------------------------------------------- |
| `Error::Timeout`             | No response within the timeout                              |
| `Error::Truncated`           | Body never fully arrived; **the stream is desynchronised**  |
| `Error::ShortPacket`         | A component payload ended early                             |
| `Error::VersionNotSupported` | QTM accepted no version this crate speaks                   |
| `Error::Qtm`                 | QTM replied with an error packet                            |
| `Error::PacketTooLarge`      | Size field exceeded the configured ceiling                  |

A read timeout is **not** an error: `receive` returns `Packet::NoMoreData` so a
polling loop can treat "nothing yet" as ordinary. `Error::Truncated` is
different, and means the connection has to be reopened.

```rust,no_run
use qualisys::{Error, Packet, Protocol};

fn read_one(rt: &mut Protocol) -> Result<Option<Packet>, Error> {
    match rt.receive() {
        Ok(Packet::NoMoreData) => Ok(None),
        Ok(packet) => Ok(Some(packet)),
        // Nothing to salvage: some of the packet was consumed and the rest is
        // still queued, so the next read would misinterpret it.
        Err(e @ Error::Truncated { .. }) => Err(e),
        Err(e) => Err(e),
    }
}
```

## Forward compatibility

A component type this build does not recognise is preserved as
`Component::Unknown` with its raw bytes, rather than failing the frame:

```rust
fn report_unknown(frame: &qualisys::DataFrame) {
    for unknown in frame.unknown_component_types() {
        eprintln!("QTM sent component type {unknown}, which this build cannot decode");
    }
}
```

## Supported components

3D (labelled, unlabelled, with and without residuals), 6DOF (rotation matrix
and Euler, with and without residuals), 2D and 2D linearized, analog and analog
single, force and force single, images, gaze vectors, eye trackers, timecode
(SMPTE with sub-frames, IRIG, camera time) and skeletons.

## Command line client

```text
cargo run -- discover
cargo run -- stream 192.168.0.10
cargo run -- stream 192.168.0.10 --udp
```

Examples:

```text
cargo run --example discover
cargo run --example streaming -- 192.168.0.10 --udp
```

## Testing

```text
cargo test --all
```

Tests run against an in-process fake QTM server; no hardware or network access
is required.

## Relationship to qualisys-go

[qualisys-go](https://github.com/mlveggo/qualisys-go) is the sibling Go client.
The two are kept feature-equivalent: same protocol version ladder, same
component coverage, same component and parameter options, same UDP support, and
the same treatment of undecodable components. Where the languages differ the
APIs follow local idiom — Rust models packets as an enum and returns a
connected client from `connect`, Go uses a struct with a type tag and separate
`NewProtocol`/`Connect` — but the wire behaviour is identical.

One deliberate asymmetry: the Go SDK ships two small XML helpers for pulling 3D
label names and 6D body names out of a settings response. This crate exposes
raw XML only, so that the library keeps its single `log` dependency rather than
pulling in an XML parser.

## Not implemented

- Typed settings (de)serialisation. `get_parameters` and `set_parameters`
  exchange raw XML. The C++ SDK additionally parses General, Calibration, 3D,
  6D, Analog, Force, Image, GazeVector, EyeTracker and Skeleton settings into
  structs, and offers typed `Set*Settings` writers.
- Protocol versions below 1.22, including the big-endian-only 1.0 mode on the
  base port.
