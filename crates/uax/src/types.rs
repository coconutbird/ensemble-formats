//! UAX Granny type definitions.
//!
//! UAX files contain Granny animation data. These structures match the
//! x64 Granny SDK format from `granny.h` (RAD Game Tools, Granny 2.7).
//!
//! The chunk data IS the `granny_file_info` structure directly (no separate
//! Granny section header). All internal pointers are 64-bit little-endian
//! offsets from the start of the chunk data. No rebasing is needed.

use zerocopy::{FromBytes, Immutable, KnownLayout};

// ============================================================================
// Zerocopy raw overlays
// ============================================================================

/// Raw on-disk Granny animation structure (32 bytes, little-endian, x64).
///
/// ```text
/// +0x00: uint64 name_ptr          - Pointer to null-terminated name string
/// +0x08: float  duration          - Animation duration in seconds
/// +0x0C: float  time_step         - Time step between keyframes
/// +0x10: float  oversampling      - Oversampling factor
/// +0x14: int32  track_group_count - Number of track groups
/// +0x18: uint64 track_groups_ptr  - Pointer to track group pointer array
/// ```
#[derive(FromBytes, KnownLayout, Immutable, Debug)]
#[repr(C)]
pub struct AnimationRaw {
    pub name_ptr: [u8; 8],
    pub duration: [u8; 4],
    pub time_step: [u8; 4],
    pub oversampling: [u8; 4],
    pub track_group_count: [u8; 4],
    pub track_groups_ptr: [u8; 8],
}

/// x64 granny_file_info layout (verified from IDA: BGrannyAnimation::load).
///
/// The chunk data starts directly with file_info — no separate header.
/// Pointers are 64-bit LE offsets from the start of the chunk data.
///
/// ```text
/// +0x00: (zeroed / unused fields)
/// +0x10: uint64 FromFileName*
/// +0x18: (ArtToolInfo*, ExporterInfo*, etc.)
/// +0x6C: int32  TrackGroupCount
/// +0x70: uint64 TrackGroups**      (ptr to array of ptrs)
/// +0x78: int32  AnimationCount
/// +0x7C: uint64 Animations**       (ptr to array of ptrs)
/// ```
pub mod file_info {
    /// Offset of FromFileName pointer (u64)
    pub const FROM_FILE_NAME_PTR: usize = 0x10;
    /// Offset of TrackGroupCount field (i32)
    pub const TRACK_GROUP_COUNT: usize = 0x6C;
    /// Offset of TrackGroups** pointer (u64) — points to array of pointers
    pub const TRACK_GROUPS_PTR: usize = 0x70;
    /// Offset of AnimationCount field (i32)
    pub const ANIMATION_COUNT: usize = 0x78;
    /// Offset of Animations** pointer (u64) — points to array of pointers
    pub const ANIMATIONS_PTR: usize = 0x7C;
    /// Minimum file_info size to read animation/track group fields
    pub const MIN_SIZE: usize = 0x84;
}

/// Granny animation structure offsets (x64 native, 32 bytes total).
pub mod animation {
    /// Offset of Name pointer (u64)
    pub const NAME_PTR: usize = 0x00;
    /// Offset of Duration field (f32)
    pub const DURATION: usize = 0x08;
    /// Offset of TimeStep field (f32)
    pub const TIME_STEP: usize = 0x0C;
    /// Offset of Oversampling field (f32)
    pub const OVERSAMPLING: usize = 0x10;
    /// Offset of TrackGroupCount field (i32)
    pub const TRACK_GROUP_COUNT: usize = 0x14;
    /// Offset of TrackGroups** pointer (u64) — points to array of pointers
    pub const TRACK_GROUPS_PTR: usize = 0x18;
    /// Total size of animation structure
    pub const SIZE: usize = 0x20;
}

/// Granny track_group structure offsets (x64 packed layout).
///
/// Verified from IDA and hex dumps. The Granny serializer packs fields
/// sequentially without C alignment padding:
/// ```text
/// +0x00: Name*         (u64)
/// +0x08: VecCount      (i32)
/// +0x0C: VecTracks*    (u64)
/// +0x14: XformCount    (i32)
/// +0x18: XformTracks*  (u64)
/// +0x20: LODCount      (i32)
/// +0x24: LODErrors*    (u64)
/// +0x2C: TextCount     (i32)
/// +0x30: TextTracks*   (u64)
/// +0x38: InitialPlacement (granny_transform, 68 bytes)
/// +0x7C: Flags         (i32)
/// ```
pub mod track_group {
    /// Offset of Name pointer (u64)
    pub const NAME_PTR: usize = 0x00;
    /// Offset of VectorTrackCount field (i32)
    pub const VECTOR_TRACK_COUNT: usize = 0x08;
    /// Offset of VectorTracks pointer (u64)
    pub const VECTOR_TRACKS_PTR: usize = 0x0C;
    /// Offset of TransformTrackCount field (i32)
    pub const TRANSFORM_TRACK_COUNT: usize = 0x14;
    /// Offset of TransformTracks pointer (u64)
    pub const TRANSFORM_TRACKS_PTR: usize = 0x18;
    /// Offset of TransformLODErrorCount field (i32)
    pub const TRANSFORM_LOD_ERROR_COUNT: usize = 0x20;
    /// Offset of TransformLODErrors pointer (u64)
    pub const TRANSFORM_LOD_ERRORS_PTR: usize = 0x24;
    /// Offset of TextTrackCount field (i32)
    pub const TEXT_TRACK_COUNT: usize = 0x2C;
    /// Offset of TextTracks pointer (u64)
    pub const TEXT_TRACKS_PTR: usize = 0x30;
    /// Offset of InitialPlacement (granny_transform — 68 bytes)
    pub const INITIAL_PLACEMENT: usize = 0x38;
    /// Offset of Flags field (i32) — at 0x38 + 68 = 0x7C
    pub const FLAGS: usize = 0x7C;
    /// Total size of track_group structure
    pub const SIZE: usize = 0x80;
}

/// Granny transform_track structure offsets (packed layout).
///
/// ```text
/// +0x00: Name*             (u64)
/// +0x08: Flags             (i32)
/// +0x0C: OrientationCurve  (curve2 = 16 bytes: type_ptr u64 + obj_ptr u64)
/// +0x1C: PositionCurve     (curve2 = 16 bytes)
/// +0x2C: ScaleShearCurve   (curve2 = 16 bytes)
/// ```
pub mod transform_track {
    /// Offset of Name pointer (u64)
    pub const NAME_PTR: usize = 0x00;
    /// Offset of Flags field (i32)
    pub const FLAGS: usize = 0x08;
    /// Offset of OrientationCurve (granny_curve2 = 16 bytes)
    pub const ORIENTATION_CURVE: usize = 0x0C;
    /// Offset of PositionCurve (16 bytes)
    pub const POSITION_CURVE: usize = 0x1C;
    /// Offset of ScaleShearCurve (16 bytes)
    pub const SCALE_SHEAR_CURVE: usize = 0x2C;
    /// Total size of transform_track structure (packed)
    pub const SIZE: usize = 0x3C;
}

/// Granny curve2 structure (wraps granny_variant).
pub mod curve2 {
    /// Offset of Type pointer in variant (u64)
    pub const TYPE_PTR: usize = 0x00;
    /// Offset of Object pointer in variant (u64)
    pub const OBJECT_PTR: usize = 0x08;
    /// Total size of curve2/variant structure
    pub const SIZE: usize = 0x10;
}

/// Granny curve_data_header structure.
pub mod curve_data_header {
    /// Offset of Format field (u8)
    pub const FORMAT: usize = 0x00;
    /// Offset of Degree field (u8)
    pub const DEGREE: usize = 0x01;
    /// Total size of header
    pub const SIZE: usize = 0x02;
}

/// Granny transform structure (used in InitialPlacement).
///
/// ```text
/// +0x00: Flags       (u32)
/// +0x04: Position    (3 × f32 = 12 bytes)
/// +0x10: Orientation (4 × f32 = 16 bytes)
/// +0x20: ScaleShear  (9 × f32 = 36 bytes)
/// ```
pub mod transform {
    /// Offset of Flags field (u32)
    pub const FLAGS: usize = 0x00;
    /// Offset of Position (triple — 12 bytes)
    pub const POSITION: usize = 0x04;
    /// Offset of Orientation (quad — 16 bytes)
    pub const ORIENTATION: usize = 0x10;
    /// Offset of ScaleShear (3×3 matrix — 36 bytes)
    pub const SCALE_SHEAR: usize = 0x20;
    /// Total size of transform structure (4 + 12 + 16 + 36 = 68 bytes)
    pub const SIZE: usize = 0x44;
}


// ============================================================================
// High-level parsed types
// ============================================================================

use alloc::string::String;
use alloc::vec::Vec;

/// A fully parsed UAX animation with all track data.
#[derive(Debug, Clone)]
pub struct Animation {
    /// Animation name (e.g. path from Maya).
    pub name: Option<String>,
    /// Duration in seconds.
    pub duration: f32,
    /// Time step between keyframes.
    pub time_step: f32,
    /// Oversampling factor.
    pub oversampling: f32,
    /// Track groups containing the actual bone animation data.
    pub track_groups: Vec<TrackGroup>,
}

/// A group of animation tracks, usually one per animated skeleton.
#[derive(Debug, Clone)]
pub struct TrackGroup {
    /// Track group name (e.g. "GrannyRootBone_Warthog01").
    pub name: Option<String>,
    /// Transform tracks (one per bone).
    pub transform_tracks: Vec<TransformTrack>,
    /// Transform LOD errors (one per transform track, if present).
    pub transform_lod_errors: Vec<f32>,
    /// Initial placement transform.
    pub initial_placement: Transform,
    /// Track group flags (motion extraction mode, etc.).
    pub flags: u32,
}

/// A single bone's animation transform track.
#[derive(Debug, Clone)]
pub struct TransformTrack {
    /// Bone name.
    pub name: Option<String>,
    /// Track flags.
    pub flags: i32,
    /// Orientation curve (quaternion).
    pub orientation: CurveData,
    /// Position curve (vec3).
    pub position: CurveData,
    /// Scale/shear curve (3×3 matrix).
    pub scale_shear: CurveData,
}

/// Raw curve data from Granny, preserving format and degree.
#[derive(Debug, Clone)]
pub struct CurveData {
    /// Granny curve format ID (e.g. 2=DaIdentity, 4=DaConstant32f,
    /// 8=D3Constant32f, 10=D4nK16uC15p, 11=DaK32fC32f, etc.).
    pub format: u8,
    /// Curve degree (0=constant, 1=linear, 2=quadratic B-spline, etc.).
    pub degree: u8,
    /// Raw curve payload bytes (everything after the 2-byte header).
    pub payload: Vec<u8>,
}

/// A Granny transform (placement / rest pose).
#[derive(Debug, Clone)]
pub struct Transform {
    /// Flags indicating which components are valid.
    pub flags: u32,
    /// Position [x, y, z].
    pub position: [f32; 3],
    /// Orientation quaternion [x, y, z, w].
    pub orientation: [f32; 4],
    /// Scale/shear 3×3 matrix (row-major).
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
// Read helpers
// ============================================================================

/// Read a little-endian u64 at the given offset.
#[inline]
pub fn read_u64_le(data: &[u8], offset: usize) -> Option<u64> {
    data.get(offset..offset + 8)
        .map(|b| u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
}

/// Read a little-endian u32 at the given offset.
#[inline]
pub fn read_u32_le(data: &[u8], offset: usize) -> Option<u32> {
    data.get(offset..offset + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// Read a little-endian i32 at the given offset.
#[inline]
pub fn read_i32_le(data: &[u8], offset: usize) -> Option<i32> {
    read_u32_le(data, offset).map(|v| v as i32)
}

/// Read a little-endian f32 at the given offset.
#[inline]
pub fn read_f32_le(data: &[u8], offset: usize) -> Option<f32> {
    data.get(offset..offset + 4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// Read a null-terminated C string from data at the given offset.
pub fn read_cstring(data: &[u8], offset: usize) -> Option<String> {
    if offset >= data.len() {
        return None;
    }
    let bytes = &data[offset..];
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len().min(512));
    String::from_utf8(bytes[..end].to_vec()).ok()
}

/// Read a pointer (u64 LE) and resolve it as an offset into `data`.
/// Returns `None` if the pointer is null or out of bounds.
#[inline]
pub fn read_ptr(data: &[u8], offset: usize) -> Option<usize> {
    let ptr = read_u64_le(data, offset)? as usize;
    if ptr == 0 || ptr >= data.len() {
        None
    } else {
        Some(ptr)
    }
}

/// Read a Granny transform from `data` at `offset`.
pub fn read_transform(data: &[u8], offset: usize) -> Transform {
    let flags = read_u32_le(data, offset + transform::FLAGS).unwrap_or(0);
    let position = [
        read_f32_le(data, offset + transform::POSITION).unwrap_or(0.0),
        read_f32_le(data, offset + transform::POSITION + 4).unwrap_or(0.0),
        read_f32_le(data, offset + transform::POSITION + 8).unwrap_or(0.0),
    ];
    let orientation = [
        read_f32_le(data, offset + transform::ORIENTATION).unwrap_or(0.0),
        read_f32_le(data, offset + transform::ORIENTATION + 4).unwrap_or(0.0),
        read_f32_le(data, offset + transform::ORIENTATION + 8).unwrap_or(0.0),
        read_f32_le(data, offset + transform::ORIENTATION + 12).unwrap_or(1.0),
    ];
    let mut scale_shear = [0.0f32; 9];
    for i in 0..9 {
        scale_shear[i] = read_f32_le(data, offset + transform::SCALE_SHEAR + i * 4).unwrap_or(if i % 4 == 0 { 1.0 } else { 0.0 });
    }
    Transform { flags, position, orientation, scale_shear }
}