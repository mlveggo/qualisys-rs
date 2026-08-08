//! 3D and 2D marker components.

use crate::components::Point;
use crate::cursor::Cursor;
use crate::error::Result;

/// A labelled or unlabelled 3D marker.
///
/// `id` is only populated for the NoLabels variants. Labelled markers carry no
/// identifier on the wire: they arrive in the same order as the labels in the
/// 3D settings XML, so index N corresponds to label N.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Marker {
    pub position: Point,
    pub residual: Option<f32>,
    pub id: Option<u32>,
}

/// The 3D marker component, shared by all four labelled/residual variants.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Markers3d {
    pub drop_rate: u16,
    pub out_of_sync_rate: u16,
    pub markers: Vec<Marker>,
}

impl Markers3d {
    pub(crate) fn decode(c: &mut Cursor<'_>, with_id: bool, with_residual: bool) -> Result<Self> {
        if c.remaining() == 0 {
            return Ok(Self::default());
        }
        let count = c.u32()?;
        let drop_rate = c.u16()?;
        let out_of_sync_rate = c.u16()?;

        // 3 floats for the position, plus an optional id and residual.
        let stride = 12 + usize::from(with_id) * 4 + usize::from(with_residual) * 4;
        c.check_count(count, stride)?;

        let mut markers = c.vec_with_capacity(count);
        for _ in 0..count {
            let position = c.point()?;
            let id = if with_id { Some(c.u32()?) } else { None };
            let residual = if with_residual { Some(c.f32()?) } else { None };
            markers.push(Marker {
                position,
                residual,
                id,
            });
        }
        Ok(Markers3d {
            drop_rate,
            out_of_sync_rate,
            markers,
        })
    }
}

/// A marker as seen by one camera, in sensor subpixel units.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Marker2d {
    pub x: u32,
    pub y: u32,
    pub diameter_x: u16,
    pub diameter_y: u16,
}

/// The 2D markers seen by a single camera.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Camera2d {
    pub status: u8,
    pub markers: Vec<Marker2d>,
}

/// The 2D marker component, used for both raw and linearized streams.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Markers2d {
    pub drop_rate: u16,
    pub out_of_sync_rate: u16,
    pub cameras: Vec<Camera2d>,
}

impl Markers2d {
    /// Each camera block is a 4 byte marker count plus a 1 byte status flag,
    /// followed by 12 bytes per marker.
    const CAMERA_HEADER: usize = 5;
    const MARKER_BYTES: usize = 12;

    pub(crate) fn decode(c: &mut Cursor<'_>) -> Result<Self> {
        if c.remaining() == 0 {
            return Ok(Self::default());
        }
        let camera_count = c.u32()?;
        let drop_rate = c.u16()?;
        let out_of_sync_rate = c.u16()?;
        c.check_count(camera_count, Self::CAMERA_HEADER)?;

        let mut cameras = c.vec_with_capacity(camera_count);
        for _ in 0..camera_count {
            let marker_count = c.u32()?;
            let status = c.u8()?;
            c.check_count(marker_count, Self::MARKER_BYTES)?;

            let mut markers = c.vec_with_capacity(marker_count);
            for _ in 0..marker_count {
                markers.push(Marker2d {
                    x: c.u32()?,
                    y: c.u32()?,
                    diameter_x: c.u16()?,
                    diameter_y: c.u16()?,
                });
            }
            cameras.push(Camera2d { status, markers });
        }
        Ok(Markers2d {
            drop_rate,
            out_of_sync_rate,
            cameras,
        })
    }
}
