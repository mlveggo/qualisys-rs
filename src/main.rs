//! A small command line client for QTM.
//!
//! ```text
//! qualisys discover [seconds]
//! qualisys stream [address] [--udp]
//! ```

use qualisys::{ComponentOptions, ComponentType, Error, EventType, Packet, Protocol, StreamRate};
use std::time::Duration;

fn usage() -> ! {
    eprintln!("usage: qualisys <discover [seconds] | stream [address] [--udp]>");
    std::process::exit(2);
}

fn main() -> Result<(), Error> {
    env_logger::init();
    let args: Vec<String> = std::env::args().skip(1).collect();

    match args.first().map(String::as_str) {
        Some("discover") => {
            let seconds = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(1);
            for server in qualisys::discover::discover(Duration::from_secs(seconds))? {
                println!("{server}");
            }
            Ok(())
        }
        Some("stream") => {
            let use_udp = args.iter().any(|a| a == "--udp");
            let host = args
                .iter()
                .skip(1)
                .find(|a| !a.starts_with("--"))
                .cloned()
                .unwrap_or_else(|| "127.0.0.1".to_string());
            stream(&host, use_udp)
        }
        _ => usage(),
    }
}

fn stream(host: &str, use_udp: bool) -> Result<(), Error> {
    let mut rt = Protocol::connect(host, qualisys::DEFAULT_BASE_PORT)?;
    let (major, minor) = rt.version();
    println!("connected to {host} using RT protocol {major}.{minor}");

    let components = [ComponentType::Markers3d];
    let options = ComponentOptions::default();

    if use_udp {
        let port = rt.enable_udp_stream(0)?;
        println!("receiving on UDP port {port}");
        rt.stream_frames_udp(StreamRate::AllFrames, &components, &options, port, None)?;
    } else {
        rt.stream_frames(StreamRate::AllFrames, &components, &options)?;
    }

    loop {
        let packet = if use_udp {
            rt.receive_udp()?
        } else {
            rt.receive()?
        };
        match packet {
            Packet::NoMoreData => continue,
            Packet::Event(EventType::QtmShuttingDown) => return Ok(()),
            Packet::Event(event) => println!("event: {event}"),
            Packet::Data(frame) => {
                if let Some(markers) = frame.markers_3d() {
                    println!(
                        "frame {}: {} markers",
                        frame.frame_number,
                        markers.markers.len()
                    );
                }
            }
            other => println!("{other:?}"),
        }
    }
}
