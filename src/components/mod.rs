//! Decoders for the data components carried inside a QTM data frame.

mod analog;
mod bodies;
mod markers;
mod media;
mod skeleton;
mod timecode;

pub use analog::{Analog, AnalogDevice, Force, ForcePlate, ForceSample};
pub use bodies::{Bodies6d, Bodies6dEuler, Body6d, Body6dEuler};
pub use markers::{Camera2d, Marker, Marker2d, Markers2d, Markers3d};
pub use media::{
    EyeTracker, EyeTrackerSample, EyeTrackers, GazeVector, GazeVectorSample, GazeVectors, Image,
    ImageFormat, Images,
};
pub use skeleton::{Segment, Skeleton, Skeletons};
pub use timecode::{CameraTime, IrigTime, SmpteTime, Timecode, Timecodes};

use crate::cursor::Cursor;
use crate::error::Result;

/// A point in 3D space, in millimetres.
///
/// The protocol transmits single precision floats; storing them as `f64` would
/// imply a precision the hardware never provided.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Point {
    pub fn new(x: f32, y: f32, z: f32) -> Self {
        Point { x, y, z }
    }
}

impl std::fmt::Display for Point {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "({:.3}, {:.3}, {:.3})", self.x, self.y, self.z)
    }
}

/// Identifies a component both on the wire and in the streaming command.
///
/// The discriminants match `CRTPacket::EComponentType` in the C++ SDK.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum ComponentType {
    Markers3d = 1,
    Markers3dNoLabels = 2,
    Analog = 3,
    Force = 4,
    Bodies6d = 5,
    Bodies6dEuler = 6,
    Markers2d = 7,
    Markers2dLinearized = 8,
    Markers3dResidual = 9,
    Markers3dNoLabelsResidual = 10,
    Bodies6dResidual = 11,
    Bodies6dEulerResidual = 12,
    AnalogSingle = 13,
    Image = 14,
    ForceSingle = 15,
    GazeVector = 16,
    Timecode = 17,
    Skeleton = 18,
    EyeTracker = 19,
}

impl ComponentType {
    /// The token QTM accepts for this component in `StreamFrames` and
    /// `GetCurrentFrame`.
    pub fn wire_name(self) -> &'static str {
        match self {
            ComponentType::Markers2d => "2D",
            ComponentType::Markers2dLinearized => "2DLin",
            ComponentType::Markers3d => "3D",
            ComponentType::Markers3dResidual => "3DRes",
            ComponentType::Markers3dNoLabels => "3DNoLabels",
            ComponentType::Markers3dNoLabelsResidual => "3DNoLabelsRes",
            ComponentType::Bodies6d => "6D",
            ComponentType::Bodies6dResidual => "6DRes",
            ComponentType::Bodies6dEuler => "6DEuler",
            ComponentType::Bodies6dEulerResidual => "6DEulerRes",
            ComponentType::Analog => "Analog",
            ComponentType::AnalogSingle => "AnalogSingle",
            ComponentType::Force => "Force",
            ComponentType::ForceSingle => "ForceSingle",
            ComponentType::GazeVector => "GazeVector",
            ComponentType::EyeTracker => "EyeTracker",
            ComponentType::Image => "Image",
            ComponentType::Timecode => "Timecode",
            ComponentType::Skeleton => "Skeleton",
        }
    }
}

impl TryFrom<u32> for ComponentType {
    type Error = u32;

    fn try_from(value: u32) -> std::result::Result<Self, u32> {
        Ok(match value {
            1 => ComponentType::Markers3d,
            2 => ComponentType::Markers3dNoLabels,
            3 => ComponentType::Analog,
            4 => ComponentType::Force,
            5 => ComponentType::Bodies6d,
            6 => ComponentType::Bodies6dEuler,
            7 => ComponentType::Markers2d,
            8 => ComponentType::Markers2dLinearized,
            9 => ComponentType::Markers3dResidual,
            10 => ComponentType::Markers3dNoLabelsResidual,
            11 => ComponentType::Bodies6dResidual,
            12 => ComponentType::Bodies6dEulerResidual,
            13 => ComponentType::AnalogSingle,
            14 => ComponentType::Image,
            15 => ComponentType::ForceSingle,
            16 => ComponentType::GazeVector,
            17 => ComponentType::Timecode,
            18 => ComponentType::Skeleton,
            19 => ComponentType::EyeTracker,
            other => return Err(other),
        })
    }
}

impl std::fmt::Display for ComponentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.wire_name())
    }
}

/// One decoded component from a data frame.
///
/// Modelling this as a sum type rather than a struct with one optional field
/// per component means a frame can only ever hold data that is actually
/// present, and matching on it is exhaustive.
#[derive(Debug, Clone, PartialEq)]
pub enum Component {
    Markers3d(Markers3d),
    Markers3dResidual(Markers3d),
    Markers3dNoLabels(Markers3d),
    Markers3dNoLabelsResidual(Markers3d),
    Bodies6d(Bodies6d),
    Bodies6dResidual(Bodies6d),
    Bodies6dEuler(Bodies6dEuler),
    Bodies6dEulerResidual(Bodies6dEuler),
    Markers2d(Markers2d),
    Markers2dLinearized(Markers2d),
    Analog(Analog),
    AnalogSingle(Analog),
    Force(Force),
    ForceSingle(Force),
    Image(Images),
    GazeVector(GazeVectors),
    Timecode(Timecodes),
    Skeleton(Skeletons),
    EyeTracker(EyeTrackers),

    /// A component this build of the crate does not know how to decode.
    ///
    /// Newer QTM releases add components. Preserving the raw bytes instead of
    /// failing the whole frame means a client keeps working against a server
    /// that has moved ahead of it.
    Unknown {
        component_type: u32,
        data: Vec<u8>,
    },
}

impl Component {
    /// The component's type tag, or `None` for an unrecognised component.
    pub fn component_type(&self) -> Option<ComponentType> {
        Some(match self {
            Component::Markers3d(_) => ComponentType::Markers3d,
            Component::Markers3dResidual(_) => ComponentType::Markers3dResidual,
            Component::Markers3dNoLabels(_) => ComponentType::Markers3dNoLabels,
            Component::Markers3dNoLabelsResidual(_) => ComponentType::Markers3dNoLabelsResidual,
            Component::Bodies6d(_) => ComponentType::Bodies6d,
            Component::Bodies6dResidual(_) => ComponentType::Bodies6dResidual,
            Component::Bodies6dEuler(_) => ComponentType::Bodies6dEuler,
            Component::Bodies6dEulerResidual(_) => ComponentType::Bodies6dEulerResidual,
            Component::Markers2d(_) => ComponentType::Markers2d,
            Component::Markers2dLinearized(_) => ComponentType::Markers2dLinearized,
            Component::Analog(_) => ComponentType::Analog,
            Component::AnalogSingle(_) => ComponentType::AnalogSingle,
            Component::Force(_) => ComponentType::Force,
            Component::ForceSingle(_) => ComponentType::ForceSingle,
            Component::Image(_) => ComponentType::Image,
            Component::GazeVector(_) => ComponentType::GazeVector,
            Component::Timecode(_) => ComponentType::Timecode,
            Component::Skeleton(_) => ComponentType::Skeleton,
            Component::EyeTracker(_) => ComponentType::EyeTracker,
            Component::Unknown { .. } => return None,
        })
    }

    /// Decodes a component payload. `payload` must be exactly the component's
    /// own bytes, excluding its 8 byte size and type header.
    pub fn decode(
        component_type: u32,
        payload: &[u8],
        order: crate::cursor::ByteOrder,
    ) -> Result<Component> {
        let Ok(kind) = ComponentType::try_from(component_type) else {
            return Ok(Component::Unknown {
                component_type,
                data: payload.to_vec(),
            });
        };

        let mut c = Cursor::new(payload, order);
        Ok(match kind {
            ComponentType::Markers3d => {
                Component::Markers3d(Markers3d::decode(&mut c, false, false)?)
            }
            ComponentType::Markers3dResidual => {
                Component::Markers3dResidual(Markers3d::decode(&mut c, false, true)?)
            }
            ComponentType::Markers3dNoLabels => {
                Component::Markers3dNoLabels(Markers3d::decode(&mut c, true, false)?)
            }
            ComponentType::Markers3dNoLabelsResidual => {
                Component::Markers3dNoLabelsResidual(Markers3d::decode(&mut c, true, true)?)
            }
            ComponentType::Bodies6d => Component::Bodies6d(Bodies6d::decode(&mut c, false)?),
            ComponentType::Bodies6dResidual => {
                Component::Bodies6dResidual(Bodies6d::decode(&mut c, true)?)
            }
            ComponentType::Bodies6dEuler => {
                Component::Bodies6dEuler(Bodies6dEuler::decode(&mut c, false)?)
            }
            ComponentType::Bodies6dEulerResidual => {
                Component::Bodies6dEulerResidual(Bodies6dEuler::decode(&mut c, true)?)
            }
            ComponentType::Markers2d => Component::Markers2d(Markers2d::decode(&mut c)?),
            ComponentType::Markers2dLinearized => {
                Component::Markers2dLinearized(Markers2d::decode(&mut c)?)
            }
            ComponentType::Analog => Component::Analog(Analog::decode(&mut c)?),
            ComponentType::AnalogSingle => Component::AnalogSingle(Analog::decode_single(&mut c)?),
            ComponentType::Force => Component::Force(Force::decode(&mut c)?),
            ComponentType::ForceSingle => Component::ForceSingle(Force::decode_single(&mut c)?),
            ComponentType::Image => Component::Image(Images::decode(&mut c)?),
            ComponentType::GazeVector => Component::GazeVector(GazeVectors::decode(&mut c)?),
            ComponentType::Timecode => Component::Timecode(Timecodes::decode(&mut c)?),
            ComponentType::Skeleton => Component::Skeleton(Skeletons::decode(&mut c)?),
            ComponentType::EyeTracker => Component::EyeTracker(EyeTrackers::decode(&mut c)?),
        })
    }
}
