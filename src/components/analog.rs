//! Analog and force plate components.

use crate::components::Point;
use crate::cursor::Cursor;
use crate::error::{Error, Result};

/// One analog device and its samples.
///
/// `channels[c][s]` is sample `s` of channel `c`. The wire format groups all
/// samples of a channel together rather than interleaving them.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AnalogDevice {
    pub id: u32,
    pub sample_number: u32,
    pub channels: Vec<Vec<f32>>,
}

/// The analog component, covering both the multi-sample and single-sample
/// variants.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Analog {
    pub devices: Vec<AnalogDevice>,
}

impl Analog {
    /// Decodes the multi-sample analog component.
    ///
    /// Device layout: id, channel count, sample count, sample number, then
    /// `sample_count` values for channel 0, then channel 1, and so on.
    pub(crate) fn decode(c: &mut Cursor<'_>) -> Result<Self> {
        if c.remaining() == 0 {
            return Ok(Self::default());
        }
        let device_count = c.u32()?;
        c.check_count(device_count, 16)?;

        let mut devices = c.vec_with_capacity(device_count);
        for _ in 0..device_count {
            let id = c.u32()?;
            let channel_count = c.u32()?;
            let sample_count = c.u32()?;
            let sample_number = c.u32()?;

            let total = (channel_count as u64)
                .saturating_mul(sample_count as u64)
                .saturating_mul(4);
            if total > c.remaining() as u64 {
                return Err(Error::MalformedFrame(format!(
                    "analog device {id} claims {channel_count} channels of {sample_count} samples, \
                     which does not fit in {} remaining bytes",
                    c.remaining()
                )));
            }

            let mut channels = Vec::with_capacity(channel_count as usize);
            for _ in 0..channel_count {
                let mut samples = Vec::with_capacity(sample_count as usize);
                for _ in 0..sample_count {
                    samples.push(c.f32()?);
                }
                channels.push(samples);
            }
            devices.push(AnalogDevice {
                id,
                sample_number,
                channels,
            });
        }
        Ok(Analog { devices })
    }

    /// Decodes the single-sample analog component: one value per channel, with
    /// no sample count or sample number field.
    pub(crate) fn decode_single(c: &mut Cursor<'_>) -> Result<Self> {
        if c.remaining() == 0 {
            return Ok(Self::default());
        }
        let device_count = c.u32()?;
        c.check_count(device_count, 8)?;

        let mut devices = c.vec_with_capacity(device_count);
        for _ in 0..device_count {
            let id = c.u32()?;
            let channel_count = c.u32()?;
            c.check_count(channel_count, 4)?;

            let mut channels = Vec::with_capacity(channel_count as usize);
            for _ in 0..channel_count {
                channels.push(vec![c.f32()?]);
            }
            devices.push(AnalogDevice {
                id,
                sample_number: 0,
                channels,
            });
        }
        Ok(Analog { devices })
    }
}

/// One force plate sample: force, moment and centre of pressure.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ForceSample {
    pub force: Point,
    pub moment: Point,
    pub center_of_pressure: Point,
}

/// One force plate and its samples for this frame.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ForcePlate {
    pub id: u32,
    pub force_number: u32,
    pub samples: Vec<ForceSample>,
}

/// The force component, covering both the multi-sample and single-sample
/// variants.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Force {
    pub plates: Vec<ForcePlate>,
}

impl Force {
    /// Nine `f32`: force, moment and centre of pressure.
    const SAMPLE_BYTES: usize = 36;

    fn sample(c: &mut Cursor<'_>) -> Result<ForceSample> {
        Ok(ForceSample {
            force: c.point()?,
            moment: c.point()?,
            center_of_pressure: c.point()?,
        })
    }

    pub(crate) fn decode(c: &mut Cursor<'_>) -> Result<Self> {
        if c.remaining() == 0 {
            return Ok(Self::default());
        }
        let plate_count = c.u32()?;
        // 12 byte plate header: id, sample count, force number.
        c.check_count(plate_count, 12)?;

        let mut plates = c.vec_with_capacity(plate_count);
        for _ in 0..plate_count {
            let id = c.u32()?;
            let sample_count = c.u32()?;
            let force_number = c.u32()?;
            c.check_count(sample_count, Self::SAMPLE_BYTES)?;

            let mut samples = Vec::with_capacity(sample_count as usize);
            for _ in 0..sample_count {
                samples.push(Self::sample(c)?);
            }
            plates.push(ForcePlate {
                id,
                force_number,
                samples,
            });
        }
        Ok(Force { plates })
    }

    pub(crate) fn decode_single(c: &mut Cursor<'_>) -> Result<Self> {
        if c.remaining() == 0 {
            return Ok(Self::default());
        }
        let plate_count = c.u32()?;
        c.check_count(plate_count, 4 + Self::SAMPLE_BYTES)?;

        let mut plates = c.vec_with_capacity(plate_count);
        for _ in 0..plate_count {
            let id = c.u32()?;
            plates.push(ForcePlate {
                id,
                force_number: 0,
                samples: vec![Self::sample(c)?],
            });
        }
        Ok(Force { plates })
    }
}
