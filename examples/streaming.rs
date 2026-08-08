//! Connects to QTM and prints streamed data frames.
//!
//! Usage: `cargo run --example streaming -- [address] [--udp]`
//! With no address, the local network is searched by broadcast.

use qualisys::{ComponentOptions, ComponentType, Error, EventType, Packet, Protocol, StreamRate};
use std::time::Duration;

fn find_server() -> Option<(String, u16)> {
    match qualisys::discover::discover(Duration::from_secs(1)) {
        Ok(servers) => servers.into_iter().next().map(|s| {
            println!("Using the first QTM found: {s}");
            (s.address, s.base_port)
        }),
        Err(e) => {
            eprintln!("discovery failed: {e}");
            None
        }
    }
}

fn main() -> Result<(), Error> {
    env_logger::init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let use_udp = args.iter().any(|a| a == "--udp");
    let address = args.iter().find(|a| !a.starts_with("--")).cloned();

    let (host, base_port) = match address {
        Some(host) => (host, qualisys::DEFAULT_BASE_PORT),
        None => find_server().unwrap_or_else(|| ("127.0.0.1".into(), qualisys::DEFAULT_BASE_PORT)),
    };

    println!("Connecting to {host}:{base_port}");
    let mut rt = Protocol::connect(&host, base_port)?;

    let (major, minor) = rt.version();
    println!("Connected using RT protocol version {major}.{minor}");
    if let Ok(version) = rt.qtm_version() {
        println!("QTM version: {version}");
    }

    let components = [ComponentType::Bodies6dEuler, ComponentType::Markers3d];
    let options = ComponentOptions::default();

    if use_udp {
        let port = rt.enable_udp_stream(0)?;
        println!("Receiving data on UDP port {port}");
        rt.stream_frames_udp(StreamRate::AllFrames, &components, &options, port, None)?;
    } else {
        rt.stream_frames(StreamRate::AllFrames, &components, &options)?;
    }

    loop {
        let packet = if use_udp {
            rt.receive_udp()
        } else {
            rt.receive()
        };

        let packet = match packet {
            Ok(p) => p,
            // A truncated packet leaves the stream desynchronised, so there is
            // nothing to do but reconnect.
            Err(e @ Error::Truncated { .. }) => {
                eprintln!("stream desynchronised: {e}");
                return Err(e);
            }
            Err(e) => return Err(e),
        };

        match packet {
            Packet::NoMoreData => continue,
            Packet::Event(event) => {
                println!("Event: {event}");
                if event == EventType::QtmShuttingDown {
                    return Ok(());
                }
            }
            Packet::Data(frame) => {
                if let Some(markers) = frame.markers_3d() {
                    println!(
                        "frame {}: {} labelled markers",
                        frame.frame_number,
                        markers.markers.len()
                    );
                }
                if let Some(bodies) = frame.bodies_6d_euler() {
                    for (i, body) in bodies.bodies.iter().enumerate() {
                        println!("  body {i}: {} angles {:?}", body.position, body.angles);
                    }
                }
                // Components this build does not recognise are reported rather
                // than silently discarding the frame.
                for unknown in frame.unknown_component_types() {
                    println!("  skipped unknown component type {unknown}");
                }
            }
            other => println!("{other:?}"),
        }
    }
}
