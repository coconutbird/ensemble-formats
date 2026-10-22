//! Decoded LGT records.

use alloc::string::String;
use alloc::vec::Vec;

/// One decoded `.lgt` light scene.
#[derive(Clone, Debug, PartialEq)]
pub struct Scene {
    /// Version word, including the [`VERSION_FAMILY`](crate::VERSION_FAMILY) prefix.
    pub version: u32,
    /// Timed scene frames in file order.
    pub frames: Vec<Frame>,
}

impl Scene {
    /// Returns the time from the first frame to the last frame, in seconds.
    ///
    /// The value is zero for a scene without frames. It is never negative.
    #[must_use]
    pub fn duration(&self) -> f32 {
        let Some((first, last)) = self.frames.first().zip(self.frames.last()) else {
            return 0.0;
        };
        (last.time - first.time).max(0.0)
    }
}

/// One timed frame of a light scene.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    /// Exporter time in seconds.
    pub time: f32,
    /// Light keys in exporter order.
    pub lights: Vec<Light>,
    /// Object keys in exporter order.
    pub objects: Vec<Object>,
    /// Camera keys in exporter order.
    pub cameras: Vec<Camera>,
}

/// One authored light kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LightKind {
    /// Uninitialized exporter value.
    Invalid,
    /// Cone-shaped local light.
    Spot,
    /// Omnidirectional local light.
    Omni,
    /// Directional light.
    Directional,
    /// Value from a newer or malformed file.
    Unknown(u8),
}

impl From<u8> for LightKind {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Invalid,
            1 => Self::Spot,
            2 => Self::Omni,
            3 => Self::Directional,
            value => Self::Unknown(value),
        }
    }
}

/// Authored contribution switches of one light key.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LightFlags(u8);

impl LightFlags {
    /// The light casts a local shadow.
    pub const SHADOWS: u8 = 1 << 0;
    /// The light contributes diffuse color.
    pub const DIFFUSE: u8 = 1 << 1;
    /// The light contributes specular color.
    pub const SPECULAR: u8 = 1 << 2;
    /// The light contributes to fog lighting.
    pub const FOG: u8 = 1 << 3;
    /// Fog receives the shadow of the light.
    pub const FOG_SHADOWS: u8 = 1 << 4;
    /// The retail renderer routes the light into the 3D light buffer.
    pub const LIGHT_BUFFERED: u8 = 1 << 5;

    /// Makes flags from a combination of the `LightFlags` bit constants.
    #[must_use]
    pub const fn from_bits(bits: u8) -> Self {
        Self(bits)
    }

    /// Returns the combination of the `LightFlags` bit constants.
    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// Returns whether the light casts a local shadow.
    #[must_use]
    pub const fn shadows(self) -> bool {
        self.0 & Self::SHADOWS != 0
    }

    /// Returns whether the light contributes diffuse color.
    #[must_use]
    pub const fn diffuse(self) -> bool {
        self.0 & Self::DIFFUSE != 0
    }

    /// Returns whether the light contributes specular color.
    #[must_use]
    pub const fn specular(self) -> bool {
        self.0 & Self::SPECULAR != 0
    }

    /// Returns whether the light contributes to fog lighting.
    #[must_use]
    pub const fn fog(self) -> bool {
        self.0 & Self::FOG != 0
    }

    /// Returns whether fog receives the shadow of the light.
    #[must_use]
    pub const fn fog_shadows(self) -> bool {
        self.0 & Self::FOG_SHADOWS != 0
    }

    /// Returns whether the retail renderer routes the light into the 3D light buffer.
    #[must_use]
    pub const fn light_buffered(self) -> bool {
        self.0 & Self::LIGHT_BUFFERED != 0
    }
}

/// One keyframed light.
#[derive(Clone, Debug, PartialEq)]
pub struct Light {
    /// Exporter node name.
    pub name: String,
    /// Exporter mask name. The value can be empty.
    pub mask: String,
    /// Light kind.
    pub kind: LightKind,
    /// Position in 3ds Max units.
    pub position: [f32; 3],
    /// Orientation quaternion in 3ds Max axes.
    pub orientation: [f32; 4],
    /// Radius in 3ds Max units.
    pub radius: f32,
    /// Fraction of the radius at which the radial falloff starts.
    pub far_attenuation_start: f32,
    /// Inverse-distance decay distance in 3ds Max units.
    pub decay_distance: f32,
    /// Exporter decay mode.
    pub decay_kind: u8,
    /// Scalar light intensity.
    pub intensity: f32,
    /// Linear authored color.
    pub color: [f32; 3],
    /// Spot inner full angle in radians.
    pub spot_inner: f32,
    /// Spot outer full angle in radians.
    pub spot_outer: f32,
    /// Authored shadow, diffuse, specular, fog and light-buffer switches.
    pub flags: LightFlags,
    /// Authored fog density.
    pub fog_density: f32,
}

/// One keyframed scene object.
#[derive(Clone, Debug, PartialEq)]
pub struct Object {
    /// Exporter node name.
    pub name: String,
    /// Position in 3ds Max units.
    pub position: [f32; 3],
    /// Orientation quaternion in 3ds Max axes.
    pub orientation: [f32; 4],
    /// Exporter object kind.
    pub kind: u8,
    /// Visibility value.
    pub visibility: f32,
    /// Bounding sphere radius in 3ds Max units.
    pub sphere_radius: f32,
    /// Raw bytes of the 3ds Max user-defined properties.
    pub user_properties: Vec<u8>,
}

/// One keyframed camera.
#[derive(Clone, Debug, PartialEq)]
pub struct Camera {
    /// Exporter node name.
    pub name: String,
    /// Position in 3ds Max units.
    pub position: [f32; 3],
    /// Orientation quaternion in 3ds Max axes.
    pub orientation: [f32; 4],
    /// Field of view.
    pub fov: f32,
    /// Focus and environment ranges. Revisions before
    /// [`CAMERA_RANGE_REVISION`](crate::CAMERA_RANGE_REVISION) do not store them.
    pub range: Option<CameraRange>,
    /// Clip planes. Revisions before
    /// [`CAMERA_CLIP_REVISION`](crate::CAMERA_CLIP_REVISION) do not store them.
    pub clip: Option<CameraClip>,
}

/// Focus and environment ranges of a camera.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraRange {
    /// Focal depth.
    pub focal_depth: f32,
    /// Near environment range.
    pub near_range: f32,
    /// Far environment range.
    pub far_range: f32,
}

/// Clip planes of a camera.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraClip {
    /// Near clip distance.
    pub near_clip: f32,
    /// Far clip distance.
    pub far_clip: f32,
}
