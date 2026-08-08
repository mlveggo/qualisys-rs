//! Commands the client can issue to QTM.

use crate::components::ComponentType;
use crate::error::{Error, Result};
use crate::packet::{EventType, Packet, PacketType};
use crate::protocol::Protocol;
use std::fmt::Write as _;
use std::time::{Duration, Instant};

/// How often QTM should send data frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamRate {
    /// Every captured frame.
    AllFrames,
    /// A fixed number of frames per second.
    Frequency(u32),
    /// Every Nth captured frame.
    FrequencyDivisor(u32),
}

impl StreamRate {
    fn as_argument(self) -> String {
        match self {
            StreamRate::AllFrames => "AllFrames".to_string(),
            StreamRate::Frequency(hz) => format!("Frequency:{hz}"),
            StreamRate::FrequencyDivisor(n) => format!("FrequencyDivisor:{n}"),
        }
    }
}

/// Per-component modifiers accepted after a colon in the component list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ComponentOptions {
    /// Restricts Analog and AnalogSingle to specific channels, for example
    /// `"1,3,5-8"`. `None` streams every channel.
    pub analog_channels: Option<String>,
    /// Requests skeleton segment transforms in the global coordinate system
    /// rather than relative to the parent segment.
    pub skeleton_global: bool,
}

impl ComponentOptions {
    /// Restricts analog streaming to the given channel specification.
    pub fn analog_channels(mut self, spec: impl Into<String>) -> Self {
        self.analog_channels = Some(spec.into());
        self
    }

    /// Requests global skeleton coordinates.
    pub fn skeleton_global(mut self) -> Self {
        self.skeleton_global = true;
        self
    }
}

/// Which settings sections to fetch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Parameter {
    All,
    General,
    Calibration,
    ThreeD,
    SixD,
    Analog,
    Force,
    Image,
    GazeVector,
    EyeTracker,
    Skeleton,
}

impl Parameter {
    fn wire_name(self) -> &'static str {
        match self {
            Parameter::All => "All",
            Parameter::General => "General",
            Parameter::Calibration => "Calibration",
            Parameter::ThreeD => "3D",
            Parameter::SixD => "6D",
            Parameter::Analog => "Analog",
            Parameter::Force => "Force",
            Parameter::Image => "Image",
            Parameter::GazeVector => "GazeVector",
            Parameter::EyeTracker => "EyeTracker",
            Parameter::Skeleton => "Skeleton",
        }
    }
}

/// LED modes for [`Protocol::led`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedMode {
    On,
    Off,
    Pulsing,
}

impl LedMode {
    fn as_str(self) -> &'static str {
        match self {
            LedMode::On => "On",
            LedMode::Off => "Off",
            LedMode::Pulsing => "Pulsing",
        }
    }
}

/// LED colours for [`Protocol::led`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedColor {
    Amber,
    Green,
    All,
}

impl LedColor {
    fn as_str(self) -> &'static str {
        match self {
            LedColor::Amber => "Amber",
            LedColor::Green => "Green",
            LedColor::All => "All",
        }
    }
}

/// Writes a downloaded capture, refusing to create an empty file.
///
/// An empty transfer means something went wrong upstream; writing a zero byte
/// capture would hide that until someone tried to open it.
fn write_capture(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
    if bytes.is_empty() {
        return Err(Error::Qtm("capture transfer returned no data".into()));
    }
    std::fs::write(path, bytes)?;
    Ok(())
}

/// Renders a component list with any applicable options.
pub(crate) fn component_string(
    components: &[ComponentType],
    options: &ComponentOptions,
) -> Result<String> {
    if components.is_empty() {
        return Err(Error::InvalidArgument(
            "at least one component must be requested".into(),
        ));
    }
    let mut parts = Vec::with_capacity(components.len());
    for &c in components {
        let mut name = c.wire_name().to_string();
        match c {
            ComponentType::Analog | ComponentType::AnalogSingle => {
                if let Some(channels) = &options.analog_channels {
                    name.push(':');
                    name.push_str(channels);
                }
            }
            ComponentType::Skeleton if options.skeleton_global => {
                name.push_str(":global");
            }
            _ => {}
        }
        parts.push(name);
    }
    Ok(parts.join(" "))
}

impl Protocol {
    /// Sends a raw command and returns QTM's response.
    ///
    /// Exposed so callers can reach protocol features this crate has not
    /// wrapped yet.
    pub fn send_command(&mut self, command: &str) -> Result<String> {
        self.send_string(command, PacketType::Command)?;
        let timeout = self.config().command_timeout;
        match self.receive_skipping_events(timeout)? {
            Packet::Command(response) => Ok(response),
            Packet::Error(message) => Err(Error::Qtm(message)),
            other => Err(Error::UnexpectedPacket {
                expected: "command",
                got: other.kind_name(),
            }),
        }
    }

    /// Sends a command and checks the response against a set of accepted
    /// replies.
    fn expect_response(&mut self, command: &str, accepted: &[&str]) -> Result<()> {
        let response = self.send_command(command)?;
        if accepted
            .iter()
            .any(|a| a.eq_ignore_ascii_case(response.trim()))
        {
            return Ok(());
        }
        Err(Error::UnexpectedResponse {
            command: command.to_string(),
            got: response,
            expected: accepted.iter().map(|s| s.to_string()).collect(),
        })
    }

    /// Negotiates a specific protocol version.
    pub(crate) fn set_version(&mut self, major: u32, minor: u32) -> Result<()> {
        let command = format!("Version {major}.{minor}");
        let expected = format!("Version set to {major}.{minor}");
        self.expect_response(&command, &[&expected])?;
        self.set_negotiated_version(major, minor);
        Ok(())
    }

    /// The QTM application version string.
    pub fn qtm_version(&mut self) -> Result<String> {
        self.send_command("QTMVersion")
    }

    /// Asks QTM which byte order the connection uses.
    pub fn byte_order_is_big_endian(&mut self) -> Result<bool> {
        Ok(self.send_command("ByteOrder")? == "Byte order is big endian")
    }

    /// Validates a license code.
    pub fn check_license(&mut self, license_code: &str) -> Result<()> {
        let response = self.send_command(&format!("CheckLicense {license_code}"))?;
        if response == "License pass" {
            return Ok(());
        }
        Err(Error::Qtm(format!("license rejected: {response}")))
    }

    /// Asks QTM for its current state and returns the resulting event.
    pub fn state(&mut self) -> Result<EventType> {
        let (major, minor) = self.version();
        let command = if major == 1 && minor <= 9 {
            "GetLastEvent"
        } else {
            "GetState"
        };
        self.send_string(command, PacketType::Command)?;

        let deadline = Instant::now() + self.config().command_timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(Error::Timeout);
            }
            if let Packet::Event(event) = self.receive_timeout(remaining)? {
                return Ok(event);
            }
        }
    }

    /// Requests a single frame containing the given components.
    pub fn get_current_frame(
        &mut self,
        components: &[ComponentType],
        options: &ComponentOptions,
    ) -> Result<()> {
        let list = component_string(components, options)?;
        self.send_string(&format!("GetCurrentFrame {list}"), PacketType::Command)
    }

    /// Starts streaming over the existing TCP connection.
    pub fn stream_frames(
        &mut self,
        rate: StreamRate,
        components: &[ComponentType],
        options: &ComponentOptions,
    ) -> Result<()> {
        self.stream_frames_inner(rate, components, options, None)
    }

    /// Starts streaming data frames to a UDP endpoint while keeping commands on
    /// the TCP connection.
    ///
    /// `address` may be `None`, in which case QTM sends to the address the TCP
    /// connection came from. Use
    /// [`enable_udp_stream`](Protocol::enable_udp_stream) to open a receiving
    /// socket first.
    pub fn stream_frames_udp(
        &mut self,
        rate: StreamRate,
        components: &[ComponentType],
        options: &ComponentOptions,
        port: u16,
        address: Option<&str>,
    ) -> Result<()> {
        if port == 0 {
            return Err(Error::InvalidArgument("udp port must not be zero".into()));
        }
        if let Some(addr) = address {
            if addr.len() > 64 {
                return Err(Error::InvalidArgument("udp address is too long".into()));
            }
        }
        self.stream_frames_inner(rate, components, options, Some((port, address)))
    }

    fn stream_frames_inner(
        &mut self,
        rate: StreamRate,
        components: &[ComponentType],
        options: &ComponentOptions,
        udp: Option<(u16, Option<&str>)>,
    ) -> Result<()> {
        let list = component_string(components, options)?;
        let mut command = String::from("StreamFrames ");
        command.push_str(&rate.as_argument());

        if let Some((port, address)) = udp {
            command.push_str(" UDP");
            if let Some(addr) = address {
                let _ = write!(command, ":{addr}");
            }
            let _ = write!(command, ":{port}");
        }

        let _ = write!(command, " {list}");
        self.send_string(&command, PacketType::Command)
    }

    /// Stops an active stream.
    pub fn stream_frames_stop(&mut self) -> Result<()> {
        self.send_string("StreamFrames Stop", PacketType::Command)
    }

    /// Takes control of QTM. Pass an empty password when none is configured.
    pub fn take_control(&mut self, password: &str) -> Result<()> {
        let command = if password.is_empty() {
            "TakeControl".to_string()
        } else {
            format!("TakeControl {password}")
        };
        self.expect_response(&command, &["You are now master", "You are already master"])
    }

    /// Releases control of QTM.
    pub fn release_control(&mut self) -> Result<()> {
        self.expect_response(
            "ReleaseControl",
            &[
                "You are now a regular client",
                "You are already a regular client",
            ],
        )
    }

    /// Creates a new measurement.
    pub fn new_measurement(&mut self) -> Result<()> {
        self.expect_response("New", &["Creating new connection", "Already connected"])
    }

    /// Closes the current measurement or file.
    pub fn close_measurement(&mut self) -> Result<()> {
        self.expect_response(
            "Close",
            &[
                "Closing connection",
                "Closing file",
                "File closed",
                "No connection to close",
            ],
        )
    }

    /// Starts a capture, or real time playback from the loaded file.
    pub fn start(&mut self, rt_from_file: bool) -> Result<()> {
        let command = if rt_from_file {
            "Start RTFromFile"
        } else {
            "Start"
        };
        self.expect_response(command, &["Starting measurement", "Starting RT from file"])
    }

    /// Stops the current capture.
    pub fn stop(&mut self) -> Result<()> {
        self.expect_response("Stop", &["Stopping measurement"])
    }

    /// Loads a measurement file.
    pub fn load(&mut self, filename: &str) -> Result<()> {
        self.expect_response(&format!("Load {filename}"), &["Measurement loaded"])
    }

    /// Saves the current measurement.
    pub fn save(&mut self, filename: &str, overwrite: bool) -> Result<()> {
        let command = if overwrite {
            format!("Save {filename} Overwrite")
        } else {
            format!("Save {filename}")
        };
        let saved_as = format!("Measurement saved as {filename}");
        self.expect_response(&command, &["Measurement saved", &saved_as])
    }

    /// Loads a QTM project.
    pub fn load_project(&mut self, path: &str) -> Result<()> {
        self.expect_response(&format!("LoadProject {path}"), &["Project loaded"])
    }

    /// Sends a software trigger.
    pub fn trig(&mut self) -> Result<()> {
        self.expect_response("Trig", &["Trig ok"])
    }

    /// Inserts a labelled event into the current measurement.
    pub fn set_qtm_event(&mut self, label: &str) -> Result<()> {
        // The command was renamed from "Event" in protocol version 1.8.
        let (major, minor) = self.version();
        let command = if major == 1 && minor <= 7 {
            format!("Event {label}")
        } else {
            format!("SetQTMEvent {label}")
        };
        self.expect_response(&command, &["Event set"])
    }

    /// Reprocesses the loaded file.
    pub fn reprocess(&mut self) -> Result<()> {
        self.expect_response("Reprocess", &["Reprocessing file"])
    }

    /// Runs a camera calibration and returns the resulting calibration XML.
    ///
    /// A calibration takes minutes, so this takes its own timeout rather than
    /// using the ordinary command timeout.
    pub fn calibrate(&mut self, refine: bool, timeout: Duration) -> Result<String> {
        let command = if refine {
            "Calibrate Refine"
        } else {
            "Calibrate"
        };
        self.expect_response(command, &["Starting calibration"])?;

        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(Error::Timeout);
            }
            match self.receive_timeout(remaining)? {
                Packet::Xml(xml) => return Ok(xml),
                Packet::Error(message) => return Err(Error::Qtm(message)),
                Packet::Event(EventType::ConnectionClosed) => {
                    return Err(Error::Qtm("connection closed during calibration".into()))
                }
                _ => continue,
            }
        }
    }

    /// Controls a camera's indicator LED.
    pub fn led(&mut self, camera: u32, mode: LedMode, color: LedColor) -> Result<()> {
        let command = format!("Led {camera} {} {}", mode.as_str(), color.as_str());
        self.send_string(&command, PacketType::Command)
    }

    /// Asks QTM to shut down.
    pub fn quit(&mut self) -> Result<()> {
        self.expect_response("Quit", &["Bye bye"])
    }

    /// Downloads the current capture as a C3D file.
    pub fn get_capture_c3d(&mut self, timeout: Duration) -> Result<Vec<u8>> {
        self.get_capture("GetCaptureC3D", timeout, true)
    }

    /// Downloads the current capture as a QTM file.
    pub fn get_capture_qtm(&mut self, timeout: Duration) -> Result<Vec<u8>> {
        self.get_capture("GetCaptureQTM", timeout, false)
    }

    fn get_capture(&mut self, command: &str, timeout: Duration, c3d: bool) -> Result<Vec<u8>> {
        self.expect_response(command, &["Sending capture"])?;
        match self.receive_skipping_events(timeout)? {
            Packet::C3dFile(bytes) if c3d => Ok(bytes),
            Packet::QtmFile(bytes) if !c3d => Ok(bytes),
            Packet::Error(message) => Err(Error::Qtm(message)),
            other => Err(Error::UnexpectedPacket {
                expected: if c3d { "c3d-file" } else { "qtm-file" },
                got: other.kind_name(),
            }),
        }
    }

    /// Downloads the current capture as a C3D file and writes it to `path`.
    pub fn save_capture_c3d(
        &mut self,
        path: impl AsRef<std::path::Path>,
        timeout: Duration,
    ) -> Result<()> {
        let bytes = self.get_capture_c3d(timeout)?;
        write_capture(path.as_ref(), &bytes)
    }

    /// Downloads the current capture as a QTM file and writes it to `path`.
    pub fn save_capture_qtm(
        &mut self,
        path: impl AsRef<std::path::Path>,
        timeout: Duration,
    ) -> Result<()> {
        let bytes = self.get_capture_qtm(timeout)?;
        write_capture(path.as_ref(), &bytes)
    }

    /// Fetches settings XML for the requested sections.
    ///
    /// Event packets arriving before the reply are skipped; treating the first
    /// packet as the response would return an empty string whenever QTM
    /// happened to emit an event.
    pub fn get_parameters(&mut self, parameters: &[Parameter]) -> Result<String> {
        self.get_parameters_with(parameters, false)
    }

    /// Fetches settings XML, optionally requesting global skeleton coordinates.
    pub fn get_parameters_with(
        &mut self,
        parameters: &[Parameter],
        skeleton_global: bool,
    ) -> Result<String> {
        let parameters = if parameters.is_empty() {
            &[Parameter::All][..]
        } else {
            parameters
        };
        let names: Vec<String> = parameters
            .iter()
            .map(|p| {
                if *p == Parameter::Skeleton && skeleton_global {
                    "Skeleton:global".to_string()
                } else {
                    p.wire_name().to_string()
                }
            })
            .collect();

        let command = format!("GetParameters {}", names.join(" "));
        self.send_string(&command, PacketType::Command)?;

        let deadline = Instant::now() + self.config().command_timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(Error::Timeout);
            }
            match self.receive_timeout(remaining)? {
                Packet::Xml(xml) => return Ok(xml),
                Packet::Error(message) => return Err(Error::Qtm(message)),
                _ => continue,
            }
        }
    }

    /// Sends a settings XML fragment, wrapping it in `<QTM_Settings>`.
    pub fn set_parameters(&mut self, xml: &str) -> Result<()> {
        let body = format!("<QTM_Settings>{xml}</QTM_Settings>");
        self.send_string(&body, PacketType::Xml)?;
        let timeout = self.config().command_timeout;
        match self.receive_skipping_events(timeout)? {
            Packet::Command(response) if response == "Setting parameters succeeded" => Ok(()),
            Packet::Command(response) => Err(Error::UnexpectedResponse {
                command: "<xml>".into(),
                got: response,
                expected: vec!["Setting parameters succeeded".into()],
            }),
            Packet::Error(message) => Err(Error::Qtm(message)),
            other => Err(Error::UnexpectedPacket {
                expected: "command",
                got: other.kind_name(),
            }),
        }
    }

    /// Removes the version-specific `QTM_Parameters_Ver_X.Y` wrapper from a
    /// [`get_parameters`](Protocol::get_parameters) response, leaving a
    /// fragment ready for [`set_parameters`](Protocol::set_parameters).
    pub fn strip_parameters_element(&self, xml: &str) -> String {
        let name = self.parameters_element_name();
        xml.replacen(&format!("<{name}>"), "", 1)
            .replacen(&format!("</{name}>"), "", 1)
            .trim()
            .to_string()
    }
}
