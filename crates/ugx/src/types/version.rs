//! UGX format version enumeration.

use crate::vertex::VertexElementType;

/// UGX format version, derived from the geometry header signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UgxVersion {
    /// Halo Wars: Definitive Edition (signature 0xC2340004).
    /// 152-byte sections with embedded `UnivertPacker`, i32 index valid accessories,
    /// includes AABB tree chunk (0x705).
    Hw1,
    /// Halo Wars 2 (signature 0xC2340006).
    /// 72-byte sections (no `UnivertPacker`), i32 index valid accessories,
    /// omits AABB tree chunk.
    Hw2,
}

impl UgxVersion {
    /// `BCachedData` header signature for this version.
    #[must_use]
    pub fn signature(self) -> u32 {
        match self {
            Self::Hw1 => crate::constants::GEOM_HEADER_SIGNATURE_HW1,
            Self::Hw2 => crate::constants::GEOM_HEADER_SIGNATURE_HW2,
        }
    }

    /// Per-section stride in the cached-data chunk (bytes).
    #[must_use]
    pub fn section_stride(self) -> usize {
        match self {
            Self::Hw1 => crate::constants::SECTION_STRIDE_HW1,
            Self::Hw2 => crate::constants::SECTION_STRIDE_HW2,
        }
    }

    /// Whether this version includes an AABB tree chunk (0x705).
    #[must_use]
    pub fn has_aabb_tree(self) -> bool {
        matches!(self, Self::Hw1)
    }

    /// Whether sections embed a `UnivertPacker` (84 bytes).
    #[must_use]
    pub fn has_embedded_packer(self) -> bool {
        matches!(self, Self::Hw1)
    }

    /// Whether valid accessories are stored as full 24-byte structs
    /// rather than 4-byte i32 indices.
    ///
    /// IDA analysis: `BUGXGeomData::readCachedData` uses
    /// `BPackedArray_Simple__unpack` (NOT `BPackedArray_Accessories__unpack`)
    /// for validAccessories in BOTH HW1 and HW2. They are always i32 indices.
    #[must_use]
    pub fn accessory_is_struct(self) -> bool {
        false
    }

    /// Default position element type. Both versions use `HalfFloat4` (8 bytes).
    #[must_use]
    pub fn default_pos_type(self) -> VertexElementType {
        VertexElementType::HalfFloat4
    }

    /// Default normal element type.
    /// HW1: Float3 (12 bytes), HW2: `Dec3N` (4 bytes).
    #[must_use]
    pub fn default_normal_type(self) -> VertexElementType {
        match self {
            Self::Hw1 => VertexElementType::Float3,
            Self::Hw2 => VertexElementType::Dec3N,
        }
    }

    /// Default tangent element type.
    /// HW1: Float3 (12 bytes), HW2: `Dec3N` (4 bytes).
    #[must_use]
    pub fn default_tangent_type(self) -> VertexElementType {
        match self {
            Self::Hw1 => VertexElementType::Float3,
            Self::Hw2 => VertexElementType::Dec3N,
        }
    }

    /// Default basis (binormal) element type.
    /// HW1: Float3 (12 bytes), HW2: `Dec3N` (4 bytes).
    #[must_use]
    pub fn default_basis_type(self) -> VertexElementType {
        match self {
            Self::Hw1 => VertexElementType::Float3,
            Self::Hw2 => VertexElementType::Dec3N,
        }
    }

    /// Default basis scale element type.
    /// HW1: Ignore (not used), HW2: `HalfFloat2` (4 bytes).
    #[must_use]
    pub fn default_basis_scale_type(self) -> VertexElementType {
        match self {
            Self::Hw1 => VertexElementType::Ignore,
            Self::Hw2 => VertexElementType::HalfFloat2,
        }
    }

    /// Value to write for the W component of `HalfFloat4` positions.
    /// HW1 originals use `0.0`; HW2 originals use `1.0`.
    #[must_use]
    pub fn pos_w(self) -> f32 {
        match self {
            Self::Hw1 => 0.0,
            Self::Hw2 => 1.0,
        }
    }
}
