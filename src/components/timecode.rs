//! Timecode component.

use crate::cursor::Cursor;
use crate::error::Result;

/// SMPTE timecode.
///
/// `sub_frame` counts camera frames within a single timecode frame and is what
/// makes the timecode usable above the timecode frequency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SmpteTime {
    pub hours: u32,
    pub minutes: u32,
    pub seconds: u32,
    pub frames: u32,
    pub sub_frame: u32,
}

impl SmpteTime {
    fn decode(low: u32) -> Self {
        SmpteTime {
            hours: low & 0x1F,
            minutes: (low >> 5) & 0x3F,
            seconds: (low >> 11) & 0x3F,
            frames: (low >> 17) & 0x1F,
            sub_frame: (low >> 22) & 0x1FF,
        }
    }

    /// Expresses `sub_frame` as a fraction of one timecode frame.
    ///
    /// Mirrors `CRTProtocol::SMPTENormalizedSubFrame` in the C++ SDK. Returns
    /// zero when either frequency is zero or the capture frequency is below the
    /// timestamp frequency, since the ratio is meaningless there.
    pub fn normalized_sub_frame(&self, capture_frequency: u32, timestamp_frequency: u32) -> f64 {
        if capture_frequency == 0
            || timestamp_frequency == 0
            || capture_frequency < timestamp_frequency
        {
            return 0.0;
        }
        let sub_frames_per_frame = capture_frequency / timestamp_frequency;
        f64::from(self.sub_frame) / f64::from(sub_frames_per_frame)
    }
}

impl std::fmt::Display for SmpteTime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:02}:{:02}:{:02}:{:02}",
            self.hours, self.minutes, self.seconds, self.frames
        )
    }
}

/// IRIG timecode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct IrigTime {
    pub year: u32,
    pub day: u32,
    pub hours: u32,
    pub minutes: u32,
    pub seconds: u32,
    pub tenths: u32,
}

impl IrigTime {
    fn decode(high: u32, low: u32) -> Self {
        IrigTime {
            year: high & 0x7F,
            day: (high >> 7) & 0x1FF,
            hours: low & 0x1F,
            minutes: (low >> 5) & 0x3F,
            seconds: (low >> 11) & 0x3F,
            tenths: (low >> 17) & 0xF,
        }
    }
}

impl std::fmt::Display for IrigTime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:02}:{:03}:{:02}:{:02}:{:02}.{}",
            self.year, self.day, self.hours, self.minutes, self.seconds, self.tenths
        )
    }
}

/// Camera time in 100 nanosecond ticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CameraTime(pub u64);

impl CameraTime {
    const TICKS_PER_SECOND: u64 = 10_000_000;

    pub fn as_duration(self) -> std::time::Duration {
        std::time::Duration::from_nanos(self.0.saturating_mul(100))
    }
}

impl std::fmt::Display for CameraTime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let seconds = self.0 / Self::TICKS_PER_SECOND;
        let nanos = (self.0 % Self::TICKS_PER_SECOND) * 100;
        write!(f, "{seconds}.{nanos:09}")
    }
}

/// A single timecode reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Timecode {
    Smpte(SmpteTime),
    Irig(IrigTime),
    CameraTime(CameraTime),
    Unknown { kind: u32, high: u32, low: u32 },
}

impl std::fmt::Display for Timecode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Timecode::Smpte(t) => write!(f, "{t}"),
            Timecode::Irig(t) => write!(f, "{t}"),
            Timecode::CameraTime(t) => write!(f, "{t}"),
            Timecode::Unknown { kind, .. } => write!(f, "unknown timecode type {kind}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Timecodes {
    pub timecodes: Vec<Timecode>,
}

impl Timecodes {
    /// type, high word and low word.
    const ENTRY_BYTES: usize = 12;

    pub(crate) fn decode(c: &mut Cursor<'_>) -> Result<Self> {
        if c.remaining() == 0 {
            return Ok(Self::default());
        }
        let count = c.u32()?;
        c.check_count(count, Self::ENTRY_BYTES)?;

        let mut timecodes = c.vec_with_capacity(count);
        for _ in 0..count {
            let kind = c.u32()?;
            let high = c.u32()?;
            let low = c.u32()?;
            timecodes.push(match kind {
                0 => Timecode::Smpte(SmpteTime::decode(low)),
                1 => Timecode::Irig(IrigTime::decode(high, low)),
                2 => Timecode::CameraTime(CameraTime((u64::from(high) << 32) | u64::from(low))),
                other => Timecode::Unknown {
                    kind: other,
                    high,
                    low,
                },
            });
        }
        Ok(Timecodes { timecodes })
    }
}
