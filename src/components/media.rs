//! Image, gaze vector and eye tracker components.

use crate::cursor::Cursor;
use crate::error::{Error, Result};

/// Pixel format of a streamed camera image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImageFormat {
    #[default]
    RawGrayscale,
    RawBgr,
    Jpg,
    Png,
    Unknown(u32),
}

impl From<u32> for ImageFormat {
    fn from(value: u32) -> Self {
        match value {
            0 => ImageFormat::RawGrayscale,
            1 => ImageFormat::RawBgr,
            2 => ImageFormat::Jpg,
            3 => ImageFormat::Png,
            other => ImageFormat::Unknown(other),
        }
    }
}

/// A single camera image.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Image {
    pub camera_id: u32,
    pub format: ImageFormat,
    pub width: u32,
    pub height: u32,
    pub crop_left: f32,
    pub crop_top: f32,
    pub crop_right: f32,
    pub crop_bottom: f32,
    pub data: Vec<u8>,
}

impl std::fmt::Display for Image {
    /// Reports the payload length rather than dumping the bytes; a single video
    /// frame is easily megabytes and printing it is never useful.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "image camera {} {:?} {}x{} ({} bytes)",
            self.camera_id,
            self.format,
            self.width,
            self.height,
            self.data.len()
        )
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Images {
    pub images: Vec<Image>,
}

impl Images {
    /// id, format, width, height, four crop floats and the payload length.
    const HEADER_BYTES: usize = 36;

    pub(crate) fn decode(c: &mut Cursor<'_>) -> Result<Self> {
        if c.remaining() == 0 {
            return Ok(Self::default());
        }
        let image_count = c.u32()?;
        c.check_count(image_count, Self::HEADER_BYTES)?;

        let mut images = c.vec_with_capacity(image_count);
        for _ in 0..image_count {
            let camera_id = c.u32()?;
            let format = ImageFormat::from(c.u32()?);
            let width = c.u32()?;
            let height = c.u32()?;
            let crop_left = c.f32()?;
            let crop_top = c.f32()?;
            let crop_right = c.f32()?;
            let crop_bottom = c.f32()?;
            let size = c.u32()? as usize;

            if size > c.remaining() {
                return Err(Error::MalformedFrame(format!(
                    "image from camera {camera_id} claims {size} bytes, {} remaining",
                    c.remaining()
                )));
            }
            let data = c.bytes(size)?;

            images.push(Image {
                camera_id,
                format,
                width,
                height,
                crop_left,
                crop_top,
                crop_right,
                crop_bottom,
                data,
            });
        }
        Ok(Images { images })
    }
}

/// One gaze vector sample: a direction and the eye position it originates from.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct GazeVectorSample {
    pub gaze: crate::components::Point,
    pub position: crate::components::Point,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct GazeVector {
    pub sample_number: u32,
    pub samples: Vec<GazeVectorSample>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct GazeVectors {
    pub devices: Vec<GazeVector>,
}

impl GazeVectors {
    const SAMPLE_BYTES: usize = 24;

    /// Decodes gaze vectors.
    ///
    /// A device reporting zero samples omits the sample number field entirely,
    /// so its record is 4 bytes rather than 8. Getting that stride wrong
    /// desynchronises every device that follows.
    pub(crate) fn decode(c: &mut Cursor<'_>) -> Result<Self> {
        if c.remaining() == 0 {
            return Ok(Self::default());
        }
        let device_count = c.u32()?;
        c.check_count(device_count, 4)?;

        let mut devices = c.vec_with_capacity(device_count);
        for _ in 0..device_count {
            let sample_count = c.u32()?;
            if sample_count == 0 {
                devices.push(GazeVector::default());
                continue;
            }
            let sample_number = c.u32()?;
            c.check_count(sample_count, Self::SAMPLE_BYTES)?;

            let mut samples = Vec::with_capacity(sample_count as usize);
            for _ in 0..sample_count {
                samples.push(GazeVectorSample {
                    gaze: c.point()?,
                    position: c.point()?,
                });
            }
            devices.push(GazeVector {
                sample_number,
                samples,
            });
        }
        Ok(GazeVectors { devices })
    }
}

/// One eye tracker sample: left and right pupil diameters.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct EyeTrackerSample {
    pub left_pupil_diameter: f32,
    pub right_pupil_diameter: f32,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct EyeTracker {
    pub sample_number: u32,
    pub samples: Vec<EyeTrackerSample>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct EyeTrackers {
    pub devices: Vec<EyeTracker>,
}

impl EyeTrackers {
    const SAMPLE_BYTES: usize = 8;

    /// As with gaze vectors, a device reporting zero samples omits the sample
    /// number field.
    pub(crate) fn decode(c: &mut Cursor<'_>) -> Result<Self> {
        if c.remaining() == 0 {
            return Ok(Self::default());
        }
        let device_count = c.u32()?;
        c.check_count(device_count, 4)?;

        let mut devices = c.vec_with_capacity(device_count);
        for _ in 0..device_count {
            let sample_count = c.u32()?;
            if sample_count == 0 {
                devices.push(EyeTracker::default());
                continue;
            }
            let sample_number = c.u32()?;
            c.check_count(sample_count, Self::SAMPLE_BYTES)?;

            let mut samples = Vec::with_capacity(sample_count as usize);
            for _ in 0..sample_count {
                samples.push(EyeTrackerSample {
                    left_pupil_diameter: c.f32()?,
                    right_pupil_diameter: c.f32()?,
                });
            }
            devices.push(EyeTracker {
                sample_number,
                samples,
            });
        }
        Ok(EyeTrackers { devices })
    }
}
