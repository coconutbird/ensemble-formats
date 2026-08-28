//! UAX Granny type definitions.
//!
//! UAX files contain packed x64 Granny structures. The layouts in this
//! module match the type descriptors used by the game's UAX loader. Internal
//! pointers are stored as 64-bit little-endian offsets from the start of the
//! UAX chunk until the engine rebases them.

use alloc::string::String;
use alloc::vec::Vec;
use zerocopy::{FromBytes, Immutable, KnownLayout};

// ============================================================================
// Raw layouts and offsets
// ============================================================================

/// Raw on-disk Granny animation structure (56 bytes, packed).
#[derive(FromBytes, KnownLayout, Immutable, Debug)]
#[repr(C)]
pub struct AnimationRaw {
    /// Pointer to the animation name.
    pub name_ptr: [u8; 8],
    /// Animation duration in seconds.
    pub duration: [u8; 4],
    /// Time between samples.
    pub time_step: [u8; 4],
    /// Oversampling factor.
    pub oversampling: [u8; 4],
    /// Number of track-group references.
    pub track_group_count: [u8; 4],
    /// Pointer to the track-group pointer array.
    pub track_groups_ptr: [u8; 8],
    /// Default number of loops.
    pub default_loop_count: [u8; 4],
    /// Granny animation flags.
    pub flags: [u8; 4],
    /// Extended-data type pointer.
    pub extended_data_type_ptr: [u8; 8],
    /// Extended-data object pointer.
    pub extended_data_object_ptr: [u8; 8],
}

/// Packed x64 `granny_file_info` layout.
pub mod file_info {
    /// Offset of `FromFileName` pointer.
    pub const FROM_FILE_NAME_PTR: usize = 0x10;
    /// Offset of `TrackGroupCount`.
    pub const TRACK_GROUP_COUNT: usize = 0x6C;
    /// Offset of `TrackGroups` pointer array.
    pub const TRACK_GROUPS_PTR: usize = 0x70;
    /// Offset of `AnimationCount`.
    pub const ANIMATION_COUNT: usize = 0x78;
    /// Offset of `Animations` pointer array.
    pub const ANIMATIONS_PTR: usize = 0x7C;
    /// Offset of the root extended-data variant.
    pub const EXTENDED_DATA: usize = 0x84;
    /// Full packed structure size required by the engine loader.
    pub const SIZE: usize = 0x94;
    /// Minimum valid chunk size.
    pub const MIN_SIZE: usize = SIZE;
}

/// Packed Granny animation offsets.
pub mod animation {
    /// Offset of the name pointer.
    pub const NAME_PTR: usize = 0x00;
    /// Offset of duration.
    pub const DURATION: usize = 0x08;
    /// Offset of time step.
    pub const TIME_STEP: usize = 0x0C;
    /// Offset of oversampling.
    pub const OVERSAMPLING: usize = 0x10;
    /// Offset of track-group count.
    pub const TRACK_GROUP_COUNT: usize = 0x14;
    /// Offset of track-group pointer array.
    pub const TRACK_GROUPS_PTR: usize = 0x18;
    /// Offset of default loop count.
    pub const DEFAULT_LOOP_COUNT: usize = 0x20;
    /// Offset of animation flags.
    pub const FLAGS: usize = 0x24;
    /// Offset of the extended-data variant.
    pub const EXTENDED_DATA: usize = 0x28;
    /// Full packed structure size.
    pub const SIZE: usize = 0x38;
}

/// Packed Granny track-group offsets.
pub mod track_group {
    /// Offset of the name pointer.
    pub const NAME_PTR: usize = 0x00;
    /// Offset of vector-track count.
    pub const VECTOR_TRACK_COUNT: usize = 0x08;
    /// Offset of vector-track array.
    pub const VECTOR_TRACKS_PTR: usize = 0x0C;
    /// Offset of transform-track count.
    pub const TRANSFORM_TRACK_COUNT: usize = 0x14;
    /// Offset of transform-track array.
    pub const TRANSFORM_TRACKS_PTR: usize = 0x18;
    /// Offset of transform LOD error count.
    pub const TRANSFORM_LOD_ERROR_COUNT: usize = 0x20;
    /// Offset of transform LOD error array.
    pub const TRANSFORM_LOD_ERRORS_PTR: usize = 0x24;
    /// Offset of text-track count.
    pub const TEXT_TRACK_COUNT: usize = 0x2C;
    /// Offset of text-track array.
    pub const TEXT_TRACKS_PTR: usize = 0x30;
    /// Offset of initial placement.
    pub const INITIAL_PLACEMENT: usize = 0x38;
    /// Offset of accumulation flags.
    pub const FLAGS: usize = 0x7C;
    /// Offset of loop translation (`f32[3]`).
    pub const LOOP_TRANSLATION: usize = 0x80;
    /// Offset of optional periodic-loop pointer.
    pub const PERIODIC_LOOP_PTR: usize = 0x8C;
    /// Offset of the extended-data variant.
    pub const EXTENDED_DATA: usize = 0x94;
    /// Full packed structure size.
    pub const SIZE: usize = 0xA4;
}

/// Packed Granny vector-track offsets.
pub mod vector_track {
    /// Offset of the name pointer.
    pub const NAME_PTR: usize = 0x00;
    /// Offset of the unsigned track key.
    pub const TRACK_KEY: usize = 0x08;
    /// Offset of the signed dimension.
    pub const DIMENSION: usize = 0x0C;
    /// Offset of the value curve.
    pub const VALUE_CURVE: usize = 0x10;
    /// Full packed structure size.
    pub const SIZE: usize = 0x20;
}

/// Packed Granny transform-track offsets.
pub mod transform_track {
    /// Offset of the name pointer.
    pub const NAME_PTR: usize = 0x00;
    /// Offset of flags.
    pub const FLAGS: usize = 0x08;
    /// Offset of orientation curve.
    pub const ORIENTATION_CURVE: usize = 0x0C;
    /// Offset of position curve.
    pub const POSITION_CURVE: usize = 0x1C;
    /// Offset of scale/shear curve.
    pub const SCALE_SHEAR_CURVE: usize = 0x2C;
    /// Full packed structure size.
    pub const SIZE: usize = 0x3C;
}

/// Packed Granny text-track offsets.
pub mod text_track {
    /// Offset of the name pointer.
    pub const NAME_PTR: usize = 0x00;
    /// Offset of entry count.
    pub const ENTRY_COUNT: usize = 0x08;
    /// Offset of entry array.
    pub const ENTRIES_PTR: usize = 0x0C;
    /// Full packed structure size.
    pub const SIZE: usize = 0x14;
}

/// Packed Granny text-track-entry offsets.
pub mod text_track_entry {
    /// Offset of timestamp.
    pub const TIME_STAMP: usize = 0x00;
    /// Offset of text pointer.
    pub const TEXT_PTR: usize = 0x04;
    /// Full packed structure size.
    pub const SIZE: usize = 0x0C;
}

/// Packed Granny periodic-loop offsets.
pub mod periodic_loop {
    /// Offset of radius.
    pub const RADIUS: usize = 0x00;
    /// Offset of angular delta.
    pub const D_ANGLE: usize = 0x04;
    /// Offset of Z delta.
    pub const D_Z: usize = 0x08;
    /// Offset of X basis vector.
    pub const BASIS_X: usize = 0x0C;
    /// Offset of Y basis vector.
    pub const BASIS_Y: usize = 0x18;
    /// Offset of axis vector.
    pub const AXIS: usize = 0x24;
    /// Full packed structure size.
    pub const SIZE: usize = 0x30;
}

/// Granny curve variant offsets.
pub mod curve2 {
    /// Offset of type-definition pointer.
    pub const TYPE_PTR: usize = 0x00;
    /// Offset of object pointer.
    pub const OBJECT_PTR: usize = 0x08;
    /// Full variant size.
    pub const SIZE: usize = 0x10;
}

/// Granny curve-data header offsets.
pub mod curve_data_header {
    /// Offset of format byte.
    pub const FORMAT: usize = 0x00;
    /// Offset of degree byte.
    pub const DEGREE: usize = 0x01;
    /// Header size.
    pub const SIZE: usize = 0x02;
}

/// Granny transform offsets.
pub mod transform {
    /// Offset of component flags.
    pub const FLAGS: usize = 0x00;
    /// Offset of position.
    pub const POSITION: usize = 0x04;
    /// Offset of orientation.
    pub const ORIENTATION: usize = 0x10;
    /// Offset of scale/shear matrix.
    pub const SCALE_SHEAR: usize = 0x20;
    /// Full packed structure size.
    pub const SIZE: usize = 0x44;
}

/// Granny variant size (`type_ptr`, `object_ptr`).
pub mod variant {
    /// Offset of type-definition pointer.
    pub const TYPE_PTR: usize = 0x00;
    /// Offset of object pointer.
    pub const OBJECT_PTR: usize = 0x08;
    /// Full packed structure size.
    pub const SIZE: usize = 0x10;
}

// ============================================================================
// High-level parsed types
// ============================================================================

/// A parsed UAX animation and its referenced track groups.
#[derive(Debug, Clone, PartialEq)]
pub struct Animation {
    /// Animation name.
    pub name: Option<String>,
    /// Duration in seconds.
    pub duration: f32,
    /// Time between samples.
    pub time_step: f32,
    /// Oversampling factor.
    pub oversampling: f32,
    /// Referenced track groups.
    pub track_groups: Vec<TrackGroup>,
    /// Default number of loops.
    pub default_loop_count: i32,
    /// Granny animation flags.
    pub flags: u32,
}

/// A group of animation tracks, usually one per animated skeleton.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackGroup {
    /// Track-group name.
    pub name: Option<String>,
    /// Arbitrary scalar/vector tracks.
    pub vector_tracks: Vec<VectorTrack>,
    /// Bone transform tracks.
    pub transform_tracks: Vec<TransformTrack>,
    /// Transform LOD errors.
    pub transform_lod_errors: Vec<f32>,
    /// Timed text/event tracks.
    pub text_tracks: Vec<TextTrack>,
    /// Initial placement transform.
    pub initial_placement: Transform,
    /// Motion accumulation flags.
    pub flags: u32,
    /// Translation accumulated by one loop.
    pub loop_translation: [f32; 3],
    /// Optional periodic-loop description.
    pub periodic_loop: Option<PeriodicLoop>,
}

/// A Granny vector track.
#[derive(Debug, Clone, PartialEq)]
pub struct VectorTrack {
    /// Track name.
    pub name: Option<String>,
    /// Application-defined track key.
    pub track_key: u32,
    /// Number of values produced per sample.
    pub dimension: i32,
    /// Animated values.
    pub value: CurveData,
}

/// A single bone transform track.
#[derive(Debug, Clone, PartialEq)]
pub struct TransformTrack {
    /// Bone name.
    pub name: Option<String>,
    /// Track flags.
    pub flags: i32,
    /// Orientation curve.
    pub orientation: CurveData,
    /// Position curve.
    pub position: CurveData,
    /// Scale/shear curve.
    pub scale_shear: CurveData,
}

/// A timed text/event track.
#[derive(Debug, Clone, PartialEq)]
pub struct TextTrack {
    /// Track name.
    pub name: Option<String>,
    /// Timed entries.
    pub entries: Vec<TextTrackEntry>,
}

/// One timed text/event entry.
#[derive(Debug, Clone, PartialEq)]
pub struct TextTrackEntry {
    /// Event time in seconds.
    pub time_stamp: f32,
    /// Event text. A null string pointer is represented by `None`.
    pub text: Option<String>,
}

/// Parameters describing a periodic motion loop.
#[derive(Debug, Clone, PartialEq)]
pub struct PeriodicLoop {
    /// Loop radius.
    pub radius: f32,
    /// Angular delta per loop.
    pub d_angle: f32,
    /// Z delta per loop.
    pub d_z: f32,
    /// X basis vector.
    pub basis_x: [f32; 3],
    /// Y basis vector.
    pub basis_y: [f32; 3],
    /// Loop axis.
    pub axis: [f32; 3],
}

/// Parsed Granny curve data.
#[derive(Debug, Clone, PartialEq)]
pub struct CurveData {
    /// Granny curve format ID.
    pub format: u8,
    /// Curve degree.
    pub degree: u8,
    /// Typed format payload.
    pub payload: CurvePayload,
}

/// Typed payloads for every Granny curve format used by the engine.
#[derive(Debug, Clone, PartialEq)]
pub enum CurvePayload {
    /// Format 0: dimension plus f32 keyframes.
    DaKeyframes32f { dimension: i16, controls: Vec<f32> },
    /// Format 1: f32 knots and f32 controls.
    DaK32fC32f {
        padding: i16,
        knots: Vec<f32>,
        controls: Vec<f32>,
    },
    /// Format 2: identity curve.
    Identity { dimension: i16 },
    /// Format 3: dimension-agnostic f32 constant.
    DaConstant32f { padding: i16, controls: Vec<f32> },
    /// Format 4: three-component f32 constant.
    D3Constant32f { padding: i16, controls: [f32; 3] },
    /// Format 5: four-component f32 constant.
    D4Constant32f { padding: i16, controls: [f32; 4] },
    /// Format 6: arbitrary-dimensional u16 knots and controls.
    DaK16uC16u {
        one_over_knot_scale_trunc: u16,
        control_scale_offsets: Vec<f32>,
        knots_controls: Vec<u16>,
    },
    /// Format 7: arbitrary-dimensional u8 knots and controls.
    DaK8uC8u {
        one_over_knot_scale_trunc: u16,
        control_scale_offsets: Vec<f32>,
        knots_controls: Vec<u8>,
    },
    /// Format 8: normalized four-component u16 curve.
    D4nK16uC15u {
        scale_offset_table_entries: u16,
        one_over_knot_scale: f32,
        knots_controls: Vec<u16>,
    },
    /// Format 9: normalized four-component u8 curve.
    D4nK8uC7u {
        scale_offset_table_entries: u16,
        one_over_knot_scale: f32,
        knots_controls: Vec<u8>,
    },
    /// Format 10: three-component u16 curve.
    D3K16uC16u {
        one_over_knot_scale_trunc: u16,
        control_scales: [f32; 3],
        control_offsets: [f32; 3],
        knots_controls: Vec<u16>,
    },
    /// Format 11: three-component u8 curve.
    D3K8uC8u {
        one_over_knot_scale_trunc: u16,
        control_scales: [f32; 3],
        control_offsets: [f32; 3],
        knots_controls: Vec<u8>,
    },
    /// Format 12: nine-component curve with one scale/offset pair and u16 data.
    D9I1K16uC16u {
        one_over_knot_scale_trunc: u16,
        control_scale: f32,
        control_offset: f32,
        knots_controls: Vec<u16>,
    },
    /// Format 13: nine-component curve with three scale/offset pairs and u16 data.
    D9I3K16uC16u {
        one_over_knot_scale_trunc: u16,
        control_scales: [f32; 3],
        control_offsets: [f32; 3],
        knots_controls: Vec<u16>,
    },
    /// Format 14: nine-component curve with one scale/offset pair and u8 data.
    D9I1K8uC8u {
        one_over_knot_scale_trunc: u16,
        control_scale: f32,
        control_offset: f32,
        knots_controls: Vec<u8>,
    },
    /// Format 15: nine-component curve with three scale/offset pairs and u8 data.
    D9I3K8uC8u {
        one_over_knot_scale_trunc: u16,
        control_scales: [f32; 3],
        control_offsets: [f32; 3],
        knots_controls: Vec<u8>,
    },
    /// Format 16: three-component identity-interleaved f32 curve.
    D3I1K32fC32f {
        padding: u16,
        control_scales: [f32; 3],
        control_offsets: [f32; 3],
        knots_controls: Vec<f32>,
    },
    /// Format 17: three-component identity-interleaved u16 curve.
    D3I1K16uC16u {
        one_over_knot_scale_trunc: u16,
        control_scales: [f32; 3],
        control_offsets: [f32; 3],
        knots_controls: Vec<u16>,
    },
    /// Format 18: three-component identity-interleaved u8 curve.
    D3I1K8uC8u {
        one_over_knot_scale_trunc: u16,
        control_scales: [f32; 3],
        control_offsets: [f32; 3],
        knots_controls: Vec<u8>,
    },
    /// Opaque payload supplied by a caller for an unknown future format.
    Unknown { raw: Vec<u8> },
}

impl CurvePayload {
    /// Return the format ID required by this payload, or `None` for `Unknown`.
    #[must_use]
    pub const fn format(&self) -> Option<u8> {
        match self {
            Self::DaKeyframes32f { .. } => Some(0),
            Self::DaK32fC32f { .. } => Some(1),
            Self::Identity { .. } => Some(2),
            Self::DaConstant32f { .. } => Some(3),
            Self::D3Constant32f { .. } => Some(4),
            Self::D4Constant32f { .. } => Some(5),
            Self::DaK16uC16u { .. } => Some(6),
            Self::DaK8uC8u { .. } => Some(7),
            Self::D4nK16uC15u { .. } => Some(8),
            Self::D4nK8uC7u { .. } => Some(9),
            Self::D3K16uC16u { .. } => Some(10),
            Self::D3K8uC8u { .. } => Some(11),
            Self::D9I1K16uC16u { .. } => Some(12),
            Self::D9I3K16uC16u { .. } => Some(13),
            Self::D9I1K8uC8u { .. } => Some(14),
            Self::D9I3K8uC8u { .. } => Some(15),
            Self::D3I1K32fC32f { .. } => Some(16),
            Self::D3I1K16uC16u { .. } => Some(17),
            Self::D3I1K8uC8u { .. } => Some(18),
            Self::Unknown { .. } => None,
        }
    }

    /// Return a stable name for this payload variant.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self.format() {
            Some(format) => match curve_type_name(format) {
                Some(name) => name,
                None => "Unknown",
            },
            None => "Unknown",
        }
    }
}

/// Return the engine's embedded Granny type name for a curve format.
#[must_use]
pub const fn curve_type_name(format: u8) -> Option<&'static str> {
    match format {
        0 => Some("CurveDataHeader_DaKeyframes32f"),
        1 => Some("CurveDataHeader_DaK32fC32f"),
        2 => Some("CurveDataHeader_DaIdentity"),
        3 => Some("CurveDataHeader_DaConstant32f"),
        4 => Some("CurveDataHeader_D3Constant32f"),
        5 => Some("CurveDataHeader_D4Constant32f"),
        6 => Some("CurveDataHeader_DaK16uC16u"),
        7 => Some("CurveDataHeader_DaK8uC8u"),
        8 => Some("CurveDataHeader_D4nK16uC15u"),
        9 => Some("CurveDataHeader_D4nK8uC7u"),
        10 => Some("CurveDataHeader_D3K16uC16u"),
        11 => Some("CurveDataHeader_D3K8uC8u"),
        12 => Some("CurveDataHeader_D9I1K16uC16u"),
        13 => Some("CurveDataHeader_D9I3K16uC16u"),
        14 => Some("CurveDataHeader_D9I1K8uC8u"),
        15 => Some("CurveDataHeader_D9I3K8uC8u"),
        16 => Some("CurveDataHeader_D3I1K32fC32f"),
        17 => Some("CurveDataHeader_D3I1K16uC16u"),
        18 => Some("CurveDataHeader_D3I1K8uC8u"),
        _ => None,
    }
}

/// A Granny transform.
#[derive(Debug, Clone, PartialEq)]
pub struct Transform {
    /// Valid-component flags.
    pub flags: u32,
    /// Position.
    pub position: [f32; 3],
    /// Orientation quaternion.
    pub orientation: [f32; 4],
    /// Row-major scale/shear matrix.
    pub scale_shear: [f32; 9],
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            flags: 0,
            position: [0.0; 3],
            orientation: [0.0, 0.0, 0.0, 1.0],
            scale_shear: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        }
    }
}

// ============================================================================
// Checked read helpers
// ============================================================================

fn bytes_at(data: &[u8], offset: usize, size: usize) -> Option<&[u8]> {
    data.get(offset..offset.checked_add(size)?)
}

fn read_f32_array<const N: usize>(data: &[u8], offset: usize) -> Option<[f32; N]> {
    let size = N.checked_mul(4)?;
    bytes_at(data, offset, size)?;
    let mut values = [0.0; N];
    for (index, value) in values.iter_mut().enumerate() {
        let relative = index.checked_mul(4)?;
        *value = read_f32_le(data, offset.checked_add(relative)?)?;
    }
    Some(values)
}

/// Read a little-endian `u64` at `offset`.
#[must_use]
pub fn read_u64_le(data: &[u8], offset: usize) -> Option<u64> {
    let bytes: [u8; 8] = bytes_at(data, offset, 8)?.try_into().ok()?;
    Some(u64::from_le_bytes(bytes))
}

/// Read a little-endian `u16` at `offset`.
#[must_use]
pub fn read_u16_le(data: &[u8], offset: usize) -> Option<u16> {
    let bytes: [u8; 2] = bytes_at(data, offset, 2)?.try_into().ok()?;
    Some(u16::from_le_bytes(bytes))
}

/// Read a little-endian `i16` at `offset`.
#[must_use]
pub fn read_i16_le(data: &[u8], offset: usize) -> Option<i16> {
    read_u16_le(data, offset).map(u16::cast_signed)
}

/// Read a little-endian `u32` at `offset`.
#[must_use]
pub fn read_u32_le(data: &[u8], offset: usize) -> Option<u32> {
    let bytes: [u8; 4] = bytes_at(data, offset, 4)?.try_into().ok()?;
    Some(u32::from_le_bytes(bytes))
}

/// Read a little-endian `i32` at `offset`.
#[must_use]
pub fn read_i32_le(data: &[u8], offset: usize) -> Option<i32> {
    read_u32_le(data, offset).map(u32::cast_signed)
}

/// Read a little-endian `f32` at `offset`.
#[must_use]
pub fn read_f32_le(data: &[u8], offset: usize) -> Option<f32> {
    let bytes: [u8; 4] = bytes_at(data, offset, 4)?.try_into().ok()?;
    Some(f32::from_le_bytes(bytes))
}

/// Read a null-terminated UTF-8 string at `offset`.
#[must_use]
pub fn read_cstring(data: &[u8], offset: usize) -> Option<String> {
    let bytes = data.get(offset..)?;
    let end = bytes.iter().position(|byte| *byte == 0)?;
    String::from_utf8(bytes[..end].to_vec()).ok()
}

/// Read a non-null Granny pointer and resolve it to an in-bounds offset.
#[must_use]
pub fn read_ptr(data: &[u8], offset: usize) -> Option<usize> {
    let pointer = usize::try_from(read_u64_le(data, offset)?).ok()?;
    (pointer != 0 && pointer < data.len()).then_some(pointer)
}

/// Read a complete Granny transform, returning `None` if it is truncated.
#[must_use]
pub fn read_transform(data: &[u8], offset: usize) -> Option<Transform> {
    bytes_at(data, offset, transform::SIZE)?;
    let position = read_f32_array(data, offset.checked_add(transform::POSITION)?)?;
    let orientation = read_f32_array(data, offset.checked_add(transform::ORIENTATION)?)?;
    let scale_shear = read_f32_array(data, offset.checked_add(transform::SCALE_SHEAR)?)?;
    Some(Transform {
        flags: read_u32_le(data, offset.checked_add(transform::FLAGS)?)?,
        position,
        orientation,
        scale_shear,
    })
}
