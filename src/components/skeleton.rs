//! Skeleton component.

use crate::components::Point;
use crate::cursor::Cursor;
use crate::error::Result;

/// A skeleton segment.
///
/// Position and rotation are relative to the parent segment unless the stream
/// was requested with the `global` option, in which case they are in the global
/// coordinate system.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Segment {
    pub id: u32,
    pub position: Point,
    /// Rotation quaternion as (x, y, z, w).
    pub rotation: [f32; 4],
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Skeleton {
    pub segments: Vec<Segment>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Skeletons {
    pub skeletons: Vec<Skeleton>,
}

impl Skeletons {
    /// id, 3 position floats and 4 rotation floats.
    const SEGMENT_BYTES: usize = 32;

    pub(crate) fn decode(c: &mut Cursor<'_>) -> Result<Self> {
        if c.remaining() == 0 {
            return Ok(Self::default());
        }
        let skeleton_count = c.u32()?;
        c.check_count(skeleton_count, 4)?;

        let mut skeletons = c.vec_with_capacity(skeleton_count);
        for _ in 0..skeleton_count {
            let segment_count = c.u32()?;
            c.check_count(segment_count, Self::SEGMENT_BYTES)?;

            let mut segments = Vec::with_capacity(segment_count as usize);
            for _ in 0..segment_count {
                let id = c.u32()?;
                let position = c.point()?;
                let rotation = [c.f32()?, c.f32()?, c.f32()?, c.f32()?];
                segments.push(Segment {
                    id,
                    position,
                    rotation,
                });
            }
            skeletons.push(Skeleton { segments });
        }
        Ok(Skeletons { skeletons })
    }
}
