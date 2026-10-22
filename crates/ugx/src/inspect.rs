//! Raw views of the `BCachedData` chunk for inspection tools.
//!
//! The normal reader converts flags to `bool` and LOD fields to `f32`. These
//! views keep the stored integer values and bit patterns. Use them to find
//! non-canonical flags and padding in game files.

use core::mem::size_of;

use zerocopy::FromBytes;

use crate::constants::{
    ECF_CACHED_DATA_CHUNK_ID, GEOM_HEADER_SIGNATURE_HW1, GEOM_HEADER_SIGNATURE_HW2,
    SECTION_STRIDE_HW1, SECTION_STRIDE_HW2,
};
use crate::types::UgxVersion;
use crate::types::raw::{
    GeomHeaderRaw, Hw1SectionFlagsRaw, Hw2SectionTailRaw, PackedArrayRaw, PackedSectionFixedRaw,
};

/// ECF chunk ID of the `BCachedData` chunk.
pub const CACHED_DATA_CHUNK_ID: u64 = ECF_CACHED_DATA_CHUNK_ID;

/// Byte offset of the HW2 section tail in a section record.
const HW2_TAIL_OFFSET: usize = size_of::<PackedSectionFixedRaw>();

/// Byte offset of the HW1 section flags in a section record.
const HW1_FLAGS_OFFSET: usize = SECTION_STRIDE_HW1 - size_of::<Hw1SectionFlagsRaw>();

/// The four flag bytes of the geometry header, as stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawHeaderFlags {
    /// `all_sections_rigid` byte at `+0x36`.
    pub all_sections_rigid: u8,
    /// `global_bones` byte at `+0x37`.
    pub global_bones: u8,
    /// `all_sections_skinned` byte at `+0x38`.
    pub all_sections_skinned: u8,
    /// `rigid_only` byte at `+0x39`.
    pub rigid_only: u8,
}

/// The padding fields at the end of the geometry header, as stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawHeaderPadding {
    /// Two-byte padding at `+0x3A`.
    pub padding: u16,
    /// Four-byte padding at `+0x3C`.
    pub padding2: u32,
}

/// The location of the section array in the chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SectionArray {
    /// Number of section records that the header declares.
    pub count: usize,
    /// Byte offset of the first section record from the chunk start.
    pub offset: usize,
}

/// The HW2 section fields after the fixed fields, as stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawHw2SectionTail {
    /// `rigid_only` value at `+0x28`. The game expects 0 or 1.
    pub rigid_only: i32,
    /// Bit pattern of the `f32` near LOD distance at `+0x2C`.
    pub lod_near_distance_bits: u32,
    /// Bit pattern of the `f32` far LOD distance at `+0x30`.
    pub lod_far_distance_bits: u32,
    /// Bit pattern of the `f32` LOD fade distance at `+0x34`.
    pub lod_fade_distance_bits: u32,
}

/// The HW1 section flags at the end of the record, as stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawHw1SectionFlags {
    /// `rigid_only` value at `+0x8C`. The game expects 0 or 1.
    pub rigid_only: i32,
    /// `global_bones` value at `+0x90`. The game expects 0 or 1.
    pub global_bones: i32,
    /// Padding at `+0x94`. The game expects 0.
    pub padding: i32,
}

/// A raw view of the decompressed `BCachedData` chunk.
#[derive(Debug, Clone, Copy)]
pub struct CachedDataView<'a> {
    data: &'a [u8],
    header: &'a GeomHeaderRaw,
    sections: &'a PackedArrayRaw,
}

impl<'a> CachedDataView<'a> {
    /// Makes a view of `data`.
    ///
    /// Returns `None` if `data` is shorter than the geometry header and the
    /// section array header.
    #[must_use]
    pub fn new(data: &'a [u8]) -> Option<Self> {
        let (header, rest) = GeomHeaderRaw::ref_from_prefix(data).ok()?;
        let (sections, _) = PackedArrayRaw::ref_from_prefix(rest).ok()?;
        Some(Self {
            data,
            header,
            sections,
        })
    }

    /// Returns the header signature.
    #[must_use]
    pub fn signature(&self) -> u32 {
        u32::from_le_bytes(self.header.signature)
    }

    /// Returns the version that the signature identifies, if the signature is known.
    #[must_use]
    pub fn version(&self) -> Option<UgxVersion> {
        match self.signature() {
            GEOM_HEADER_SIGNATURE_HW1 => Some(UgxVersion::Hw1),
            GEOM_HEADER_SIGNATURE_HW2 => Some(UgxVersion::Hw2),
            _ => None,
        }
    }

    /// Returns the flag bytes of the geometry header.
    #[must_use]
    pub fn header_flags(&self) -> RawHeaderFlags {
        RawHeaderFlags {
            all_sections_rigid: self.header.all_sections_rigid,
            global_bones: self.header.global_bones,
            all_sections_skinned: self.header.all_sections_skinned,
            rigid_only: self.header.rigid_only,
        }
    }

    /// Returns the padding fields of the geometry header.
    #[must_use]
    pub fn header_padding(&self) -> RawHeaderPadding {
        RawHeaderPadding {
            padding: u16::from_le_bytes(self.header.padding),
            padding2: u32::from_le_bytes(self.header.padding2),
        }
    }

    /// Returns the section count and offset.
    ///
    /// Returns `None` if a value does not fit in `usize`.
    #[must_use]
    pub fn section_array(&self) -> Option<SectionArray> {
        Some(SectionArray {
            count: usize::try_from(u32::from_le_bytes(self.sections.count)).ok()?,
            offset: usize::try_from(u64::from_le_bytes(self.sections.offset)).ok()?,
        })
    }

    /// Returns the HW2 tail fields of each section.
    ///
    /// The iterator uses the 72-byte HW2 record stride. It stops at the
    /// first record that is not fully in the chunk.
    pub fn hw2_section_tails(
        &self,
        array: SectionArray,
    ) -> impl Iterator<Item = RawHw2SectionTail> + 'a {
        records(self.data, array, SECTION_STRIDE_HW2, HW2_TAIL_OFFSET).map(
            |raw: &Hw2SectionTailRaw| RawHw2SectionTail {
                rigid_only: i32::from_le_bytes(raw.rigid_only),
                lod_near_distance_bits: u32::from_le_bytes(raw.lod_near_distance),
                lod_far_distance_bits: u32::from_le_bytes(raw.lod_far_distance),
                lod_fade_distance_bits: u32::from_le_bytes(raw.lod_fade_distance),
            },
        )
    }

    /// Returns the HW1 trailing flags of each section.
    ///
    /// The iterator uses the 152-byte HW1 record stride. It stops at the
    /// first record that is not fully in the chunk.
    pub fn hw1_section_flags(
        &self,
        array: SectionArray,
    ) -> impl Iterator<Item = RawHw1SectionFlags> + 'a {
        records(self.data, array, SECTION_STRIDE_HW1, HW1_FLAGS_OFFSET).map(
            |raw: &Hw1SectionFlagsRaw| RawHw1SectionFlags {
                rigid_only: i32::from_le_bytes(raw.rigid_only),
                global_bones: i32::from_le_bytes(raw.global_bones),
                padding: i32::from_le_bytes(raw.padding),
            },
        )
    }
}

/// Returns the field of type `T` at `field_offset` in each section record.
///
/// The iterator stops at the first record that is not fully in `data`.
fn records<'a, T>(
    data: &'a [u8],
    array: SectionArray,
    stride: usize,
    field_offset: usize,
) -> impl Iterator<Item = &'a T> + 'a
where
    T: FromBytes + zerocopy::KnownLayout + zerocopy::Immutable + 'a,
{
    (0..array.count).map_while(move |index| {
        let start = index
            .checked_mul(stride)
            .and_then(|relative| array.offset.checked_add(relative))?;
        let end = start.checked_add(stride)?;
        let record = data.get(start..end)?;
        let (field, _) = T::ref_from_prefix(record.get(field_offset..)?).ok()?;
        Some(field)
    })
}

#[cfg(test)]
mod tests {
    use alloc::vec;
    use alloc::vec::Vec;

    use super::{
        CachedDataView, RawHeaderFlags, RawHeaderPadding, RawHw1SectionFlags, RawHw2SectionTail,
        SectionArray,
    };
    use crate::types::UgxVersion;

    /// Builds a chunk prefix with the given signature, flags, padding and section array.
    fn chunk(signature: u32, section_count: u32, section_offset: u64) -> Vec<u8> {
        let mut data = vec![0_u8; 0x50];
        data[0..4].copy_from_slice(&signature.to_le_bytes());
        data[0x36..0x3A].copy_from_slice(&[2, 0, 1, 7]);
        data[0x3A..0x3C].copy_from_slice(&0xBEEF_u16.to_le_bytes());
        data[0x3C..0x40].copy_from_slice(&0x1234_5678_u32.to_le_bytes());
        data[0x40..0x44].copy_from_slice(&section_count.to_le_bytes());
        data[0x48..0x50].copy_from_slice(&section_offset.to_le_bytes());
        data
    }

    fn put_i32(data: &mut [u8], offset: usize, value: i32) {
        data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    #[test]
    fn rejects_short_chunk() {
        assert!(CachedDataView::new(&[0; 0x4F]).is_none());
    }

    #[test]
    fn exposes_header_flags_and_padding_as_stored() {
        let data = chunk(0x1111_1111, 0, 0x50);
        let view = CachedDataView::new(&data).unwrap();
        assert_eq!(view.signature(), 0x1111_1111);
        assert_eq!(view.version(), None);
        assert_eq!(
            view.header_flags(),
            RawHeaderFlags {
                all_sections_rigid: 2,
                global_bones: 0,
                all_sections_skinned: 1,
                rigid_only: 7,
            }
        );
        assert_eq!(
            view.header_padding(),
            RawHeaderPadding {
                padding: 0xBEEF,
                padding2: 0x1234_5678,
            }
        );
        assert_eq!(
            view.section_array(),
            Some(SectionArray {
                count: 0,
                offset: 0x50,
            })
        );
    }

    #[test]
    fn reads_hw2_tails_and_stops_at_truncated_section() {
        // Three sections are declared, but only two fit in the chunk.
        let mut data = chunk(0xC234_0006, 3, 0x50);
        data.resize(0x50 + 2 * 72 + 71, 0);
        put_i32(&mut data, 0x50 + 0x28, 1);
        put_i32(&mut data, 0x50 + 0x2C, 0);
        put_i32(&mut data, 0x50 + 0x30, 0x7F7F_FFFF);
        put_i32(&mut data, 0x50 + 72 + 0x28, -3);
        put_i32(&mut data, 0x50 + 72 + 0x34, 0x3F80_0000);
        let view = CachedDataView::new(&data).unwrap();
        assert_eq!(view.version(), Some(UgxVersion::Hw2));
        let tails: Vec<_> = view
            .hw2_section_tails(view.section_array().unwrap())
            .collect();
        assert_eq!(
            tails,
            [
                RawHw2SectionTail {
                    rigid_only: 1,
                    lod_near_distance_bits: 0,
                    lod_far_distance_bits: 0x7F7F_FFFF,
                    lod_fade_distance_bits: 0,
                },
                RawHw2SectionTail {
                    rigid_only: -3,
                    lod_near_distance_bits: 0,
                    lod_far_distance_bits: 0,
                    lod_fade_distance_bits: 0x3F80_0000,
                },
            ]
        );
    }

    #[test]
    fn reads_hw1_flags_with_noncanonical_values() {
        let mut data = chunk(0xC234_0004, 2, 0x50);
        data.resize(0x50 + 2 * 152, 0);
        put_i32(&mut data, 0x50 + 152 + 0x8C, 2);
        put_i32(&mut data, 0x50 + 152 + 0x90, 1);
        put_i32(&mut data, 0x50 + 152 + 0x94, -1);
        let view = CachedDataView::new(&data).unwrap();
        assert_eq!(view.version(), Some(UgxVersion::Hw1));
        let flags: Vec<_> = view
            .hw1_section_flags(view.section_array().unwrap())
            .collect();
        assert_eq!(
            flags,
            [
                RawHw1SectionFlags {
                    rigid_only: 0,
                    global_bones: 0,
                    padding: 0,
                },
                RawHw1SectionFlags {
                    rigid_only: 2,
                    global_bones: 1,
                    padding: -1,
                },
            ]
        );
    }

    #[test]
    fn stops_when_section_offset_is_outside_chunk() {
        let data = chunk(0xC234_0004, 1, u64::MAX);
        let view = CachedDataView::new(&data).unwrap();
        let array = view.section_array().unwrap();
        assert_eq!(view.hw1_section_flags(array).count(), 0);
    }
}
