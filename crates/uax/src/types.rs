//! UAX Granny type definitions.
//!
//! UAX files contain Granny animation data. These structures match the
//! Granny SDK format from `granny.h` (RAD Game Tools, Granny 2.7).
//!
//! All pointers in the serialized format are 32-bit offsets that need
//! rebasing by subtracting 0x10 to get actual offsets within the chunk.

/// Granny file_info structure offsets (32-bit pointer layout).
/// Offsets are relative to the start of file_info (after 32-byte Granny header).
pub mod file_info {
    /// Offset of TrackGroupCount field (i32)
    pub const TRACK_GROUP_COUNT: usize = 0x4C;
    /// Offset of TrackGroups pointer (u32)
    pub const TRACK_GROUPS_PTR: usize = 0x50;
    /// Offset of AnimationCount field (i32)
    pub const ANIMATION_COUNT: usize = 0x58;
    /// Offset of Animations pointer (u32)
    pub const ANIMATIONS_PTR: usize = 0x5C;
}

/// Granny animation structure offsets (64-bit internal pointers).
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
    /// Offset of TrackGroups pointer (u64)
    pub const TRACK_GROUPS_PTR: usize = 0x18;
}

/// Granny track_group structure offsets (64-bit pointer layout).
pub mod track_group {
    /// Offset of Name pointer (u64)
    pub const NAME_PTR: usize = 0x00;
    /// Offset of VectorTrackCount field (i32)
    pub const VECTOR_TRACK_COUNT: usize = 0x08;
    // padding: 0x0C-0x0F
    /// Offset of VectorTracks pointer (u64)
    pub const VECTOR_TRACKS_PTR: usize = 0x10;
    /// Offset of TransformTrackCount field (i32)
    pub const TRANSFORM_TRACK_COUNT: usize = 0x18;
    // padding: 0x1C-0x1F
    /// Offset of TransformTracks pointer (u64)
    pub const TRANSFORM_TRACKS_PTR: usize = 0x20;
    /// Offset of TransformLODErrorCount field (i32)
    pub const TRANSFORM_LOD_ERROR_COUNT: usize = 0x28;
    // padding: 0x2C-0x2F
    /// Offset of TransformLODErrors pointer (u64)
    pub const TRANSFORM_LOD_ERRORS_PTR: usize = 0x30;
    /// Offset of TextTrackCount field (i32)
    pub const TEXT_TRACK_COUNT: usize = 0x38;
    // padding: 0x3C-0x3F
    /// Offset of TextTracks pointer (u64)
    pub const TEXT_TRACKS_PTR: usize = 0x40;
    /// Offset of InitialPlacement (granny_transform - 60 bytes)
    pub const INITIAL_PLACEMENT: usize = 0x48;
    /// Offset of Flags field (i32) - at 0x48 + 60 = 0x84
    pub const FLAGS: usize = 0x84;
}

/// Granny transform_track structure offsets (64-bit pointer layout).
pub mod transform_track {
    /// Offset of Name pointer (u64)
    pub const NAME_PTR: usize = 0x00;
    /// Offset of Flags field (i32)
    pub const FLAGS: usize = 0x08;
    // padding: 0x0C-0x0F
    /// Offset of OrientationCurve (granny_curve2 = granny_variant = 16 bytes)
    pub const ORIENTATION_CURVE: usize = 0x10;
    /// Offset of PositionCurve (16 bytes)
    pub const POSITION_CURVE: usize = 0x20;
    /// Offset of ScaleShearCurve (16 bytes)
    pub const SCALE_SHEAR_CURVE: usize = 0x30;
    /// Total size of transform_track structure
    pub const SIZE: usize = 0x40;
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
pub mod transform {
    /// Offset of Flags field (u32)
    pub const FLAGS: usize = 0x00;
    /// Offset of Position (triple - 12 bytes)
    pub const POSITION: usize = 0x04;
    /// Offset of Orientation (quad - 16 bytes)
    pub const ORIENTATION: usize = 0x10;
    /// Offset of ScaleShear (3x triple - 36 bytes)
    pub const SCALE_SHEAR: usize = 0x20;
    /// Total size of transform structure
    pub const SIZE: usize = 0x3C; // 60 bytes
}

/// Size of Granny section header in UAX chunk (before file_info).
pub const GRANNY_HEADER_SIZE: usize = 32;

/// Pointer rebasing offset used by Granny.
pub const POINTER_REBASE_OFFSET: u64 = 0x10;

/// Rebase a stored pointer to get actual offset in chunk data.
/// Granny pointers are stored with +0x10 offset.
#[inline]
pub fn rebase_pointer(stored: u64) -> u64 {
    stored.saturating_sub(POINTER_REBASE_OFFSET)
}

/// Convert an actual offset back to stored pointer format.
#[inline]
pub fn unrebase_pointer(actual: u64) -> u64 {
    actual + POINTER_REBASE_OFFSET
}

