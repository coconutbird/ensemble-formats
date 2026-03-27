//! UGX format version enumeration.

/// UGX format version, derived from the geometry header signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UgxVersion {
    /// Halo Wars: Definitive Edition (signature 0xC2340004).
    /// 152-byte sections with embedded UnivertPacker, i32 index valid accessories,
    /// includes AABB tree chunk (0x705).
    Hw1,
    /// Halo Wars 2 (signature 0xC2340006).
    /// 72-byte sections (no UnivertPacker), i32 index valid accessories,
    /// omits AABB tree chunk.
    Hw2,
}

impl UgxVersion {
    /// BCachedData header signature for this version.
    pub fn signature(self) -> u32 {
        match self {
            Self::Hw1 => crate::constants::GEOM_HEADER_SIGNATURE_HW1,
            Self::Hw2 => crate::constants::GEOM_HEADER_SIGNATURE_HW2,
        }
    }

    /// Per-section stride in the cached-data chunk (bytes).
    pub fn section_stride(self) -> usize {
        match self {
            Self::Hw1 => crate::constants::SECTION_STRIDE_HW1,
            Self::Hw2 => crate::constants::SECTION_STRIDE_HW2,
        }
    }

    /// Whether this version includes an AABB tree chunk (0x705).
    pub fn has_aabb_tree(self) -> bool {
        matches!(self, Self::Hw1)
    }

    /// Whether sections embed a `UnivertPacker` (84 bytes).
    pub fn has_embedded_packer(self) -> bool {
        matches!(self, Self::Hw1)
    }

    /// Whether valid accessories are stored as full 24-byte structs
    /// rather than 4-byte i32 indices.
    ///
    /// IDA analysis: `BUGXGeomData::readCachedData` uses
    /// `BPackedArray_Simple__unpack` (NOT `BPackedArray_Accessories__unpack`)
    /// for validAccessories in BOTH HW1 and HW2. They are always i32 indices.
    pub fn accessory_is_struct(self) -> bool {
        false
    }
}
