//! 6DOF rigid body components.

use crate::components::Point;
use crate::cursor::Cursor;
use crate::error::Result;

/// A rigid body expressed as a position and a row-major 3x3 rotation matrix.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Body6d {
    pub position: Point,
    pub rotation: [f32; 9],
    pub residual: Option<f32>,
}

/// The 6DOF matrix component. Bodies arrive in the order they appear in the 6D
/// settings XML.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Bodies6d {
    pub drop_rate: u16,
    pub out_of_sync_rate: u16,
    pub bodies: Vec<Body6d>,
}

impl Bodies6d {
    pub(crate) fn decode(c: &mut Cursor<'_>, with_residual: bool) -> Result<Self> {
        if c.remaining() == 0 {
            return Ok(Self::default());
        }
        let count = c.u32()?;
        let drop_rate = c.u16()?;
        let out_of_sync_rate = c.u16()?;

        // 3 position floats plus 9 rotation floats.
        let stride = 48 + usize::from(with_residual) * 4;
        c.check_count(count, stride)?;

        let mut bodies = c.vec_with_capacity(count);
        for _ in 0..count {
            let position = c.point()?;
            let mut rotation = [0f32; 9];
            for slot in rotation.iter_mut() {
                *slot = c.f32()?;
            }
            let residual = if with_residual { Some(c.f32()?) } else { None };
            bodies.push(Body6d {
                position,
                rotation,
                residual,
            });
        }
        Ok(Bodies6d {
            drop_rate,
            out_of_sync_rate,
            bodies,
        })
    }
}

/// A rigid body expressed as a position and three Euler angles.
///
/// The angle convention is reported by the General settings XML, so it is not
/// fixed by this type.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Body6dEuler {
    pub position: Point,
    pub angles: [f32; 3],
    pub residual: Option<f32>,
}

/// The 6DOF Euler component.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Bodies6dEuler {
    pub drop_rate: u16,
    pub out_of_sync_rate: u16,
    pub bodies: Vec<Body6dEuler>,
}

impl Bodies6dEuler {
    pub(crate) fn decode(c: &mut Cursor<'_>, with_residual: bool) -> Result<Self> {
        if c.remaining() == 0 {
            return Ok(Self::default());
        }
        let count = c.u32()?;
        let drop_rate = c.u16()?;
        let out_of_sync_rate = c.u16()?;

        // 3 position floats plus 3 angle floats.
        let stride = 24 + usize::from(with_residual) * 4;
        c.check_count(count, stride)?;

        let mut bodies = c.vec_with_capacity(count);
        for _ in 0..count {
            let position = c.point()?;
            let mut angles = [0f32; 3];
            for slot in angles.iter_mut() {
                *slot = c.f32()?;
            }
            let residual = if with_residual { Some(c.f32()?) } else { None };
            bodies.push(Body6dEuler {
                position,
                angles,
                residual,
            });
        }
        Ok(Bodies6dEuler {
            drop_rate,
            out_of_sync_rate,
            bodies,
        })
    }
}
