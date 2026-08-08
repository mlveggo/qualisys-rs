//! Integration tests against an in-process fake QTM server.
//!
//! No hardware and no external network access is required.

use qualisys::{
    ComponentOptions, ComponentType, Error, EventType, Packet, PacketType, Protocol, StreamRate,
};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// Builds a framed packet.
fn encode(packet_type: PacketType, payload: &[u8]) -> Vec<u8> {
    let size = 8 + payload.len();
    let mut v = Vec::with_capacity(size);
    v.extend_from_slice(&(size as u32).to_le_bytes());
    v.extend_from_slice(&(packet_type as u32).to_le_bytes());
    v.extend_from_slice(payload);
    v
}

fn command(s: &str) -> Vec<u8> {
    let mut payload = s.as_bytes().to_vec();
    payload.push(0);
    encode(PacketType::Command, &payload)
}

fn error_packet(s: &str) -> Vec<u8> {
    let mut payload = s.as_bytes().to_vec();
    payload.push(0);
    encode(PacketType::Error, &payload)
}

fn xml(s: &str) -> Vec<u8> {
    let mut payload = s.as_bytes().to_vec();
    payload.push(0);
    encode(PacketType::Xml, &payload)
}

fn event(e: u8) -> Vec<u8> {
    encode(PacketType::Event, &[e])
}

/// A fake QTM server driven by a closure over each received command.
struct FakeQtm {
    base_port: u16,
    commands: Arc<Mutex<Vec<String>>>,
    _rx: Receiver<()>,
}

impl FakeQtm {
    /// Starts a server whose handler maps a command to an optional reply.
    fn start<F>(handler: F) -> FakeQtm
    where
        F: Fn(&str) -> Option<Vec<u8>> + Send + 'static,
    {
        Self::start_with_welcome(Some(command("QTM RT Interface connected")), handler)
    }

    fn start_with_welcome<F>(welcome: Option<Vec<u8>>, handler: F) -> FakeQtm
    where
        F: Fn(&str) -> Option<Vec<u8>> + Send + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().unwrap().port();
        let commands = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&commands);
        let (tx, rx) = mpsc::channel();

        thread::spawn(move || {
            // Keeping the sender alive ties the thread's lifetime to the test.
            let _tx = tx;
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                if let Some(welcome) = &welcome {
                    if stream.write_all(welcome).is_err() {
                        continue;
                    }
                }
                let recorded = Arc::clone(&recorded);
                loop {
                    let mut header = [0u8; 8];
                    if stream.read_exact(&mut header).is_err() {
                        break;
                    }
                    let size =
                        u32::from_le_bytes([header[0], header[1], header[2], header[3]]) as usize;
                    if size < 8 {
                        break;
                    }
                    let mut body = vec![0u8; size - 8];
                    if stream.read_exact(&mut body).is_err() {
                        break;
                    }
                    let text = String::from_utf8_lossy(&body)
                        .trim_end_matches('\0')
                        .to_string();
                    recorded.lock().unwrap().push(text.clone());

                    if let Some(reply) = handler(&text) {
                        if stream.write_all(&reply).is_err() {
                            break;
                        }
                    }
                }
            }
        });

        // The client connects to base_port + 1, so the base is one below the
        // port actually being listened on.
        FakeQtm {
            base_port: port - 1,
            commands,
            _rx: rx,
        }
    }

    /// Starts a server that hands the raw socket to `f` instead of running the
    /// command loop. Used to exercise framing edge cases.
    fn start_raw<F>(f: F) -> FakeQtm
    where
        F: Fn(TcpStream) + Send + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let _tx = tx;
            for stream in listener.incoming() {
                let Ok(stream) = stream else { break };
                f(stream);
            }
        });
        FakeQtm {
            base_port: port - 1,
            commands: Arc::new(Mutex::new(Vec::new())),
            _rx: rx,
        }
    }

    fn sent_commands(&self) -> Vec<String> {
        self.commands.lock().unwrap().clone()
    }

    /// Polls until `command` has been received, since fire-and-forget commands
    /// produce no reply to synchronise on.
    fn wait_for(&self, command: &str, within: Duration) -> bool {
        let deadline = Instant::now() + within;
        while Instant::now() < deadline {
            if self.sent_commands().iter().any(|c| c == command) {
                return true;
            }
            thread::sleep(Duration::from_millis(5));
        }
        false
    }
}

/// Answers the version handshake on a raw stream, then returns.
///
/// Framing tests need a fully connected client before they can exercise
/// `receive`, so the raw server has to get past `connect` first.
fn complete_handshake(stream: &mut TcpStream) -> bool {
    let mut header = [0u8; 8];
    loop {
        if stream.read_exact(&mut header).is_err() {
            return false;
        }
        let size = u32::from_le_bytes([header[0], header[1], header[2], header[3]]) as usize;
        if size < 8 {
            return false;
        }
        let mut body = vec![0u8; size - 8];
        if stream.read_exact(&mut body).is_err() {
            return false;
        }
        let text = String::from_utf8_lossy(&body)
            .trim_end_matches('\0')
            .to_string();
        if let Some(version) = text.strip_prefix("Version ") {
            if stream
                .write_all(&command(&format!("Version set to {version}")))
                .is_err()
            {
                return false;
            }
        } else if text == "GetState" {
            let _ = stream.write_all(&event(1));
            return true;
        }
    }
}

/// A handler that accepts exactly one protocol version, mimicking an older QTM.
fn accept_version(major: u32, minor: u32) -> impl Fn(&str) -> Option<Vec<u8>> + Send + 'static {
    let wanted = format!("Version {major}.{minor}");
    move |cmd: &str| {
        if cmd == wanted {
            Some(command(&format!("Version set to {major}.{minor}")))
        } else if cmd.starts_with("Version ") {
            Some(error_packet("Version not supported"))
        } else if cmd == "GetState" {
            Some(event(1))
        } else {
            None
        }
    }
}

#[test]
fn negotiates_down_to_an_older_version() {
    // QTM only speaks 1.25. The client should walk down from its 1.28 default
    // and settle there.
    let server = FakeQtm::start(accept_version(1, 25));
    let rt = Protocol::connect("127.0.0.1", server.base_port).expect("connect");

    assert_eq!(rt.version(), (1, 25));
    let commands = server.sent_commands();
    assert_eq!(
        commands.first().map(String::as_str),
        Some("Version 1.28"),
        "the newest version should be tried first, got {commands:?}"
    );
}

#[test]
fn stops_at_the_newest_accepted_version() {
    let server = FakeQtm::start(accept_version(1, 28));
    let rt = Protocol::connect("127.0.0.1", server.base_port).expect("connect");

    assert_eq!(rt.version(), qualisys::DEFAULT_VERSION);
    assert!(
        server.sent_commands().len() <= 2,
        "should stop after the first version is accepted"
    );
}

#[test]
fn fails_cleanly_against_a_too_old_qtm() {
    // Below the supported floor every version in the ladder is rejected.
    let server = FakeQtm::start(accept_version(1, 15));
    let result = Protocol::connect("127.0.0.1", server.base_port);

    match result {
        Err(Error::VersionNotSupported { tried }) => {
            assert!(tried.contains(&(1, 28)));
            assert!(tried.contains(&(1, 22)));
            assert!(
                !tried.contains(&(1, 21)),
                "should not negotiate below the documented floor"
            );
        }
        other => panic!("expected VersionNotSupported, got {other:?}"),
    }
}

#[test]
fn version_negotiation_can_be_disabled() {
    let server = FakeQtm::start(accept_version(1, 25));
    let result = Protocol::builder()
        .version(1, 28)
        .without_version_negotiation()
        .connect("127.0.0.1", server.base_port);

    assert!(result.is_err());
    assert_eq!(
        server.sent_commands().len(),
        1,
        "exactly one version should be attempted"
    );
}

#[test]
fn parameters_element_name_tracks_the_negotiated_version() {
    let server = FakeQtm::start(accept_version(1, 24));
    let rt = Protocol::connect("127.0.0.1", server.base_port).expect("connect");

    assert_eq!(rt.parameters_element_name(), "QTM_Parameters_Ver_1.24");
    let wrapped = "<QTM_Parameters_Ver_1.24><The_6D/></QTM_Parameters_Ver_1.24>";
    assert_eq!(rt.strip_parameters_element(wrapped), "<The_6D/>");
}

#[test]
fn handles_a_header_split_across_writes() {
    // TCP may deliver fewer than 8 bytes on the first read; a single read()
    // that assumes otherwise fails intermittently under load.
    let server = FakeQtm::start_raw(|mut stream| {
        let packet = command("QTM RT Interface connected");
        for chunk in [&packet[..3], &packet[3..6], &packet[6..]] {
            if stream.write_all(chunk).is_err() {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        complete_handshake(&mut stream);
        thread::sleep(Duration::from_secs(1));
    });

    let rt = Protocol::builder()
        .read_timeout(Duration::from_secs(2))
        .connect("127.0.0.1", server.base_port)
        .expect("connect should survive a fragmented header");
    assert_eq!(rt.version(), qualisys::DEFAULT_VERSION);
}

#[test]
fn a_truncated_body_is_reported_as_such() {
    // A header promising more data than ever arrives must be an error. Treating
    // it as "no more data" would leave the partial body queued to be misread as
    // the next packet header.
    let server = FakeQtm::start_raw(|mut stream| {
        let _ = stream.write_all(&command("QTM RT Interface connected"));
        if !complete_handshake(&mut stream) {
            return;
        }
        let mut truncated = command("a response that never fully arrives");
        truncated.truncate(12);
        let _ = stream.write_all(&truncated);
        thread::sleep(Duration::from_secs(2));
    });

    let mut rt = Protocol::builder()
        .read_timeout(Duration::from_millis(200))
        .connect("127.0.0.1", server.base_port)
        .expect("connect");

    match rt.receive() {
        Err(Error::Truncated { .. }) => {}
        other => panic!("expected Truncated, got {other:?}"),
    }
}

#[test]
fn an_idle_socket_yields_no_more_data() {
    let server = FakeQtm::start_raw(|mut stream| {
        let _ = stream.write_all(&command("QTM RT Interface connected"));
        if !complete_handshake(&mut stream) {
            return;
        }
        thread::sleep(Duration::from_secs(2));
    });

    let mut rt = Protocol::builder()
        .read_timeout(Duration::from_millis(50))
        .connect("127.0.0.1", server.base_port)
        .expect("connect");
    assert_eq!(rt.receive().unwrap(), Packet::NoMoreData);
}

#[test]
fn oversized_packets_are_rejected() {
    let server = FakeQtm::start_raw(|mut stream| {
        let _ = stream.write_all(&command("QTM RT Interface connected"));
        if !complete_handshake(&mut stream) {
            return;
        }
        let mut header = [0u8; 8];
        header[0..4].copy_from_slice(&0xFFFF_FFF0u32.to_le_bytes());
        header[4..8].copy_from_slice(&(PacketType::Command as u32).to_le_bytes());
        let _ = stream.write_all(&header);
        thread::sleep(Duration::from_secs(1));
    });

    let mut rt = Protocol::builder()
        .max_packet_size(1024 * 1024)
        .connect("127.0.0.1", server.base_port)
        .expect("connect");
    assert!(matches!(rt.receive(), Err(Error::PacketTooLarge { .. })));
}

#[test]
fn commands_skip_interleaved_events() {
    // QTM pushes events asynchronously. Treating the first packet as the
    // command response makes commands fail whenever an event is in flight.
    let server = FakeQtm::start(|cmd: &str| {
        if cmd.starts_with("Version ") {
            Some(command("Version set to 1.28"))
        } else if cmd == "GetState" {
            Some(event(1))
        } else if cmd == "TakeControl" {
            let mut reply = event(3); // CaptureStarted
            reply.extend_from_slice(&event(10)); // WaitingForTrigger
            reply.extend_from_slice(&command("You are now master"));
            Some(reply)
        } else {
            None
        }
    });

    let mut rt = Protocol::connect("127.0.0.1", server.base_port).expect("connect");
    rt.take_control("")
        .expect("take_control should skip interleaved events");
    assert_eq!(rt.last_event(), EventType::WaitingForTrigger);
}

#[test]
fn take_control_without_a_password_sends_no_trailing_space() {
    let server = FakeQtm::start(|cmd: &str| {
        if cmd.starts_with("Version ") {
            Some(command("Version set to 1.28"))
        } else if cmd == "GetState" {
            Some(event(1))
        } else {
            Some(command("You are now master"))
        }
    });

    let mut rt = Protocol::connect("127.0.0.1", server.base_port).expect("connect");
    rt.take_control("").expect("take_control");

    let commands = server.sent_commands();
    assert!(commands.iter().any(|c| c == "TakeControl"));
    assert!(
        !commands.iter().any(|c| c == "TakeControl "),
        "sent a trailing space: {commands:?}"
    );
}

#[test]
fn get_parameters_skips_events() {
    let server = FakeQtm::start(|cmd: &str| {
        if cmd.starts_with("Version ") {
            Some(command("Version set to 1.28"))
        } else if cmd == "GetState" {
            Some(event(1))
        } else if cmd.starts_with("GetParameters") {
            let mut reply = event(11); // CameraSettingsChanged
            reply.extend_from_slice(&xml(
                "<QTM_Parameters_Ver_1.28><The_3D/></QTM_Parameters_Ver_1.28>",
            ));
            Some(reply)
        } else {
            None
        }
    });

    let mut rt = Protocol::connect("127.0.0.1", server.base_port).expect("connect");
    let settings = rt
        .get_parameters(&[qualisys::Parameter::ThreeD])
        .expect("get_parameters");
    assert!(settings.contains("The_3D"), "got {settings}");
}

#[test]
fn skeleton_global_parameter_option() {
    let server = FakeQtm::start(|cmd: &str| {
        if cmd.starts_with("Version ") {
            Some(command("Version set to 1.28"))
        } else if cmd == "GetState" {
            Some(event(1))
        } else if cmd.starts_with("GetParameters") {
            Some(xml("<x/>"))
        } else {
            None
        }
    });

    let mut rt = Protocol::connect("127.0.0.1", server.base_port).expect("connect");
    rt.get_parameters_with(&[qualisys::Parameter::Skeleton], true)
        .expect("get_parameters_with");

    assert!(
        server.wait_for("GetParameters Skeleton:global", Duration::from_secs(2)),
        "commands were {:?}",
        server.sent_commands()
    );
}

#[test]
fn stream_frames_command_formatting() {
    let server = FakeQtm::start(|cmd: &str| {
        if cmd.starts_with("Version ") {
            Some(command("Version set to 1.28"))
        } else if cmd == "GetState" {
            Some(event(1))
        } else {
            None
        }
    });

    let mut rt = Protocol::connect("127.0.0.1", server.base_port).expect("connect");

    rt.stream_frames(
        StreamRate::Frequency(100),
        &[ComponentType::Markers3d],
        &ComponentOptions::default(),
    )
    .expect("stream_frames");

    rt.stream_frames_udp(
        StreamRate::AllFrames,
        &[ComponentType::Skeleton, ComponentType::Analog],
        &ComponentOptions::default()
            .skeleton_global()
            .analog_channels("1,3,5-8"),
        6734,
        Some("192.168.0.5"),
    )
    .expect("stream_frames_udp");

    for expected in [
        "StreamFrames Frequency:100 3D",
        "StreamFrames AllFrames UDP:192.168.0.5:6734 Skeleton:global Analog:1,3,5-8",
    ] {
        assert!(
            server.wait_for(expected, Duration::from_secs(2)),
            "missing {expected:?} in {:?}",
            server.sent_commands()
        );
    }
}

#[test]
fn stream_frames_rejects_an_empty_component_list() {
    let server = FakeQtm::start(accept_version(1, 28));
    let mut rt = Protocol::connect("127.0.0.1", server.base_port).expect("connect");

    assert!(matches!(
        rt.stream_frames(StreamRate::AllFrames, &[], &ComponentOptions::default()),
        Err(Error::InvalidArgument(_))
    ));
    assert!(matches!(
        rt.stream_frames_udp(
            StreamRate::AllFrames,
            &[ComponentType::Markers3d],
            &ComponentOptions::default(),
            0,
            None
        ),
        Err(Error::InvalidArgument(_))
    ));
}

#[test]
fn qtm_error_packets_become_errors() {
    let server = FakeQtm::start(|cmd: &str| {
        if cmd.starts_with("Version ") {
            Some(command("Version set to 1.28"))
        } else if cmd == "GetState" {
            Some(event(1))
        } else {
            Some(error_packet("Measurement is not running"))
        }
    });

    let mut rt = Protocol::connect("127.0.0.1", server.base_port).expect("connect");
    match rt.stop() {
        Err(Error::Qtm(message)) => assert_eq!(message, "Measurement is not running"),
        other => panic!("expected a Qtm error, got {other:?}"),
    }
}

#[test]
fn udp_socket_allocates_a_port_and_decodes_datagrams() {
    let server = FakeQtm::start(accept_version(1, 28));
    let mut rt = Protocol::connect("127.0.0.1", server.base_port).expect("connect");

    let port = rt.enable_udp_stream(0).expect("enable_udp_stream");
    assert_ne!(port, 0);
    assert_eq!(rt.udp_port(), Some(port));

    // A data frame carrying two labelled 3D markers.
    let mut payload = Vec::new();
    payload.extend_from_slice(&7u64.to_le_bytes()); // timestamp
    payload.extend_from_slice(&8u32.to_le_bytes()); // frame number
    payload.extend_from_slice(&1u32.to_le_bytes()); // component count

    let mut component = Vec::new();
    component.extend_from_slice(&2u32.to_le_bytes()); // marker count
    component.extend_from_slice(&1u16.to_le_bytes()); // drop rate
    component.extend_from_slice(&2u16.to_le_bytes()); // out of sync rate
    for i in 0..2 {
        for axis in 0..3 {
            component.extend_from_slice(&((i * 3 + axis) as f32).to_le_bytes());
        }
    }
    payload.extend_from_slice(&((component.len() + 8) as u32).to_le_bytes());
    payload.extend_from_slice(&1u32.to_le_bytes()); // ComponentType::Markers3d
    payload.extend_from_slice(&component);

    let datagram = encode(PacketType::Data, &payload);
    let sender = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    sender
        .send_to(&datagram, ("127.0.0.1", port))
        .expect("send datagram");

    match rt.receive_udp().expect("receive_udp") {
        Packet::Data(frame) => {
            assert_eq!(frame.frame_number, 8);
            assert_eq!(frame.timestamp, 7);
            let markers = frame.markers_3d().expect("3d component");
            assert_eq!(markers.markers.len(), 2);
            assert_eq!(markers.markers[1].position.x, 3.0);
        }
        other => panic!("got {other:?}"),
    }
}
