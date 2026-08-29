//! `BCachedData` (chunk 0x700) sub-parsers.
//!
//! Reads packed sections, bones, bone bounds, and univert packers from the
//! cached data chunk using zerocopy overlays. Supports both HW1 (152-byte
//! sections) and HW2 (72-byte sections) formats.

use alloc::string::String;
use alloc::vec::Vec;
use zerocopy::Ref;

use crate::constants::{EMPTY_OFFSET_SENTINEL, EMPTY_OFFSET_SENTINEL_32};
use crate::error::{Error, Result};
use crate::types::raw::{
    AccessoryRaw, BVector3Raw, PackedArrayRaw, PackedBoneRaw, PackedSectionFixedRaw,
};
use crate::types::{AABB, Accessory, Bone, Matrix4x4, Section, UgxVersion};
use crate::vertex::element::VertexElementType;
use crate::vertex::packer::UnivertPacker;
use nostdio::{Cursor, ReadLe, read_null_terminated_string};

/// Version-specific data stored after the fixed section fields.
struct SectionTail {
    bone_remap: Vec<u8>,
    base_vert_packer: Option<UnivertPacker>,
    rigid_only: bool,
    global_bones: bool,
    lod_near_distance: f32,
    lod_far_distance: f32,
    lod_fade_distance: f32,
}

/// Return the input starting at `position`, with a contextual truncation error.
fn data_tail<'a>(data: &'a [u8], position: usize, context: &str) -> Result<&'a [u8]> {
    data.get(position..).ok_or_else(|| Error::UnexpectedEof {
        context: String::from(context),
    })
}

/// Read a packed-array header and advance the sequential cursor past it.
fn read_packed_array_header(
    data: &[u8],
    pos: &mut usize,
    context: &'static str,
) -> Result<(usize, u64)> {
    let source = data_tail(data, *pos, context)?;
    let (array, _): (Ref<_, PackedArrayRaw>, _) =
        Ref::from_prefix(source).map_err(|_| Error::UnexpectedEof {
            context: String::from(context),
        })?;
    crate::advance_position(
        pos,
        u64::try_from(core::mem::size_of::<PackedArrayRaw>())
            .map_err(|_| Error::SizeOverflow("packed-array header"))?,
        "packed-array header",
    )?;
    let count = crate::checked_usize(u64::from(u32::from_le_bytes(array.count)), context)?;
    Ok((count, u64::from_le_bytes(array.offset)))
}

/// Resolve a packed pointer, treating both known sentinel values as empty.
fn resolve_offset(offset: u64, context: &'static str) -> Result<Option<usize>> {
    if offset == EMPTY_OFFSET_SENTINEL || offset == u64::from(EMPTY_OFFSET_SENTINEL_32) {
        Ok(None)
    } else {
        crate::checked_usize(offset, context).map(Some)
    }
}

/// Read packed sections array from cached data.
///
/// Section stride depends on version: 152 bytes (HW1) or 72 bytes (HW2).
pub(super) fn read_packed_sections(
    data: &[u8],
    pos: &mut usize,
    version: UgxVersion,
) -> Result<Vec<Section>> {
    let (count, offset) = read_packed_array_header(data, pos, "PackedArrayRaw sections")?;

    if count == 0 {
        return Ok(Vec::new());
    }

    let mut sec_pos = resolve_offset(offset, "section offset")?
        .ok_or_else(|| Error::UnsupportedFormat("non-empty section array has no data".into()))?;
    let mut sections = Vec::with_capacity(count);

    for _ in 0..count {
        sections.push(read_packed_section(data, &mut sec_pos, version)?);
    }

    Ok(sections)
}

/// Read a single packed section (version-branched tail).
///
/// The first 40 bytes are shared between versions (`PackedSectionFixedRaw`).
/// The trailing layout differs:
/// - HW1 (112 bytes): `bone_remap(16)` + UnivertPacker(84) + flags(12)
/// - HW2 (32 bytes): flags(8) + unknown(8) + `bone_remap(16)`
fn read_packed_section(data: &[u8], pos: &mut usize, version: UgxVersion) -> Result<Section> {
    let source = data_tail(data, *pos, "PackedSectionFixedRaw")?;
    let (fixed, _): (Ref<_, PackedSectionFixedRaw>, _) =
        Ref::from_prefix(source).map_err(|_| Error::UnexpectedEof {
            context: String::from("PackedSectionFixedRaw"),
        })?;
    let material_index = i32::from_le_bytes(fixed.material_index);
    let accessory_index = i32::from_le_bytes(fixed.accessory_index);
    let max_bones = i32::from_le_bytes(fixed.max_bones);
    let rigid_bone_index = i32::from_le_bytes(fixed.rigid_bone_index);
    let ib_offset = i32::from_le_bytes(fixed.ib_offset);
    let num_tris = i32::from_le_bytes(fixed.num_tris);
    let vb_offset = i32::from_le_bytes(fixed.vb_offset);
    let vb_bytes = i32::from_le_bytes(fixed.vb_bytes);
    let vert_size = i32::from_le_bytes(fixed.vert_size);
    let num_verts = i32::from_le_bytes(fixed.num_verts);
    crate::advance_position(
        pos,
        u64::try_from(core::mem::size_of::<PackedSectionFixedRaw>())
            .map_err(|_| Error::SizeOverflow("packed section"))?,
        "packed section",
    )?;

    let tail = match version {
        UgxVersion::Hw1 => read_section_tail_hw1(data, pos)?,
        UgxVersion::Hw2 => read_section_tail_hw2(data, pos)?,
    };

    Ok(Section {
        material_index,
        accessory_index,
        max_bones,
        rigid_bone_index,
        ib_offset,
        num_tris,
        vb_offset,
        vb_bytes,
        vert_size,
        num_verts,
        base_vert_packer: tail.base_vert_packer,
        external_vert_packer: None,
        bone_remap: tail.bone_remap,
        rigid_only: tail.rigid_only,
        global_bones: tail.global_bones,
        lod_near_distance: tail.lod_near_distance,
        lod_far_distance: tail.lod_far_distance,
        lod_fade_distance: tail.lod_fade_distance,
    })
}

/// HW1 section tail: `bone_remap(16)` + UnivertPacker(84) + flags(12).
fn read_section_tail_hw1(data: &[u8], pos: &mut usize) -> Result<SectionTail> {
    let bone_remap = read_bone_remap(data, pos)?;
    let packer = read_packed_univert_packer(data, pos)?;

    let mut cur = Cursor::new(data_tail(data, *pos, "HW1 section tail")?);
    let rigid_only = cur.read_i32_le()? != 0;
    let global_bones = cur.read_i32_le()? != 0;
    cur.read_i32_le()?;
    crate::advance_position(pos, cur.position(), "binary cursor position")?;

    Ok(SectionTail {
        bone_remap,
        base_vert_packer: Some(packer),
        rigid_only,
        global_bones,
        lod_near_distance: 0.0,
        lod_far_distance: f32::MAX,
        lod_fade_distance: 0.0,
    })
}

/// HW2 section tail: `rigid_only(4)` + `lod_near(4)` + `lod_far(4)` + `lod_fade(4)` + `bone_remap(16)`.
///
/// The three LOD fields form a distance-based LOD chain:
/// - `lod_near_distance` (+0x2C): near transition distance (0.0 = closest)
/// - `lod_far_distance`  (+0x30): far transition distance (`f32::MAX` = always visible)
/// - `lod_fade_distance`  (+0x34): vertical fade for atmospheric effects (0.0 = unused)
///
/// Note: HW2 sections do NOT have a serialised `global_bones` flag at +0x2C
/// (that field was repurposed as `lod_near_distance`).  The `global_bones`
/// flag is set to `false` here; the geom-level flag is derived from context.
fn read_section_tail_hw2(data: &[u8], pos: &mut usize) -> Result<SectionTail> {
    let mut cur = Cursor::new(data_tail(data, *pos, "HW2 section tail")?);
    let rigid_only = cur.read_i32_le()? != 0;
    let lod_near_distance = f32::from_bits(cur.read_u32_le()?);
    let lod_far_distance = f32::from_bits(cur.read_u32_le()?);
    let lod_fade_distance = f32::from_bits(cur.read_u32_le()?);
    crate::advance_position(pos, cur.position(), "binary cursor position")?;

    let bone_remap = read_bone_remap(data, pos)?;

    Ok(SectionTail {
        bone_remap,
        base_vert_packer: None,
        rigid_only,
        global_bones: false,
        lod_near_distance,
        lod_far_distance,
        lod_fade_distance,
    })
}

/// Read a bone remap packed array: overlay `PackedArrayRaw`, resolve offset, copy bytes.
fn read_bone_remap(data: &[u8], pos: &mut usize) -> Result<Vec<u8>> {
    let (count, raw_offset) = read_packed_array_header(data, pos, "PackedArrayRaw bone_remap")?;
    let Some(offset) = resolve_offset(raw_offset, "bone-remap offset")? else {
        return Ok(Vec::new());
    };
    let end = offset
        .checked_add(count)
        .ok_or(Error::SizeOverflow("bone-remap range"))?;
    data.get(offset..end)
        .map(<[u8]>::to_vec)
        .ok_or_else(|| Error::UnexpectedEof {
            context: "bone remap".into(),
        })
}

/// Read packed `UnivertPacker` (84 bytes on-disk).
fn read_packed_univert_packer(data: &[u8], pos: &mut usize) -> Result<UnivertPacker> {
    let mut cur = Cursor::new(data_tail(data, *pos, "UnivertPacker")?);
    let pack_order_offset = resolve_offset(cur.read_u64_le()?, "pack-order offset")?;
    let decl_order_offset = resolve_offset(cur.read_u64_le()?, "declaration-order offset")?;

    let pack_order = pack_order_offset
        .and_then(|offset| data.get(offset..))
        .map_or_else(String::new, read_null_terminated_string);

    let decl_order = decl_order_offset
        .and_then(|offset| data.get(offset..))
        .map_or_else(String::new, read_null_terminated_string);

    let pos_type = VertexElementType::from_u32(cur.read_u32_le()?);
    let basis_type = VertexElementType::from_u32(cur.read_u32_le()?);
    let basis_scale_type = VertexElementType::from_u32(cur.read_u32_le()?);
    let tangent_type = VertexElementType::from_u32(cur.read_u32_le()?);
    let normal_type = VertexElementType::from_u32(cur.read_u32_le()?);

    let mut uv_types = [VertexElementType::Ignore; 8];
    for uv_type in &mut uv_types {
        *uv_type = VertexElementType::from_u32(cur.read_u32_le()?);
    }

    let indices_type = VertexElementType::from_u32(cur.read_u32_le()?);
    let weights_type = VertexElementType::from_u32(cur.read_u32_le()?);
    let diffuse_type = VertexElementType::from_u32(cur.read_u32_le()?);
    let index_type = VertexElementType::from_u32(cur.read_u32_le()?);
    crate::advance_position(pos, cur.position(), "binary cursor position")?;

    Ok(UnivertPacker {
        pos_type,
        basis_type,
        basis_scale_type,
        tangent_type,
        normal_type,
        uv_types,
        indices_type,
        weights_type,
        diffuse_type,
        index_type,
        pack_order,
        decl_order,
    })
}

/// Read packed bones array from cached data.
pub(super) fn read_packed_bones(data: &[u8], pos: &mut usize) -> Result<Vec<Bone>> {
    let (count, raw_offset) = read_packed_array_header(data, pos, "PackedArrayRaw bones")?;

    if count == 0 {
        return Ok(Vec::new());
    }

    let mut bone_pos = resolve_offset(raw_offset, "bone-array offset")?
        .ok_or_else(|| Error::UnsupportedFormat("non-empty bone array has no data".into()))?;
    let mut bones = Vec::with_capacity(count);

    for _ in 0..count {
        bones.push(read_packed_bone(data, &mut bone_pos)?);
    }

    Ok(bones)
}

/// Read a single packed bone (80 bytes).
fn read_packed_bone(data: &[u8], pos: &mut usize) -> Result<Bone> {
    let source = data_tail(data, *pos, "PackedBoneRaw")?;
    let (raw, _): (Ref<_, PackedBoneRaw>, _) =
        Ref::from_prefix(source).map_err(|_| Error::UnexpectedEof {
            context: String::from("PackedBoneRaw"),
        })?;
    crate::advance_position(
        pos,
        u64::try_from(core::mem::size_of::<PackedBoneRaw>())
            .map_err(|_| Error::SizeOverflow("packed bone"))?,
        "packed bone",
    )?;

    let name = resolve_offset(u64::from_le_bytes(raw.name_offset), "bone-name offset")?
        .and_then(|offset| data.get(offset..))
        .map_or_else(String::new, read_null_terminated_string);

    let model_to_bone = Matrix4x4::from(&*raw);
    let parent_index = i32::from_le_bytes(raw.parent_index);

    Ok(Bone {
        name,
        parent_index,
        model_to_bone,
    })
}

/// Read bone bounds (low and high arrays).
pub(super) fn read_bone_bounds(data: &[u8], pos: &mut usize) -> Result<Vec<AABB>> {
    let (low_count, low_raw_offset) =
        read_packed_array_header(data, pos, "PackedArrayRaw boneBoundsLow")?;
    let (high_count, high_raw_offset) =
        read_packed_array_header(data, pos, "PackedArrayRaw boneBoundsHigh")?;

    if low_count == 0 || low_count != high_count {
        return Ok(Vec::new());
    }

    let low_offset = resolve_offset(low_raw_offset, "low bone-bound offset")?
        .ok_or_else(|| Error::UnsupportedFormat("low bone bounds have no data".into()))?;
    let high_offset = resolve_offset(high_raw_offset, "high bone-bound offset")?
        .ok_or_else(|| Error::UnsupportedFormat("high bone bounds have no data".into()))?;

    let vec3_size = core::mem::size_of::<BVector3Raw>();
    let mut bounds = Vec::with_capacity(low_count);

    for index in 0..low_count {
        let relative = index
            .checked_mul(vec3_size)
            .ok_or(Error::SizeOverflow("bone-bound offset"))?;
        let low_pos = low_offset
            .checked_add(relative)
            .ok_or(Error::SizeOverflow("low bone-bound offset"))?;
        let high_pos = high_offset
            .checked_add(relative)
            .ok_or(Error::SizeOverflow("high bone-bound offset"))?;

        let (lo, _): (Ref<_, BVector3Raw>, _) =
            Ref::from_prefix(data_tail(data, low_pos, "BVector3Raw boneBoundsLow")?).map_err(
                |_| Error::UnexpectedEof {
                    context: alloc::string::String::from("BVector3Raw boneBoundsLow"),
                },
            )?;
        let (hi, _): (Ref<_, BVector3Raw>, _) =
            Ref::from_prefix(data_tail(data, high_pos, "BVector3Raw boneBoundsHigh")?).map_err(
                |_| Error::UnexpectedEof {
                    context: alloc::string::String::from("BVector3Raw boneBoundsHigh"),
                },
            )?;

        bounds.push(AABB {
            min: <[f32; 3]>::from(&*lo),
            max: <[f32; 3]>::from(&*hi),
        });
    }

    Ok(bounds)
}

/// Read packed accessories array from cached data.
///
/// Each accessory is 24 bytes (`AccessoryRaw`), containing `first_bone`, `num_bones`,
/// and a nested `BPackedArray<int>` for `mObjectIndices`.
///
/// Verified from IDA `BPackedArray_Accessories__unpack` at `0x1406d8660`:
/// the outer array is fixed up first, then each accessory's inner packed array
/// offset is resolved (4-byte aligned for i32 elements).
pub(super) fn read_packed_accessories(data: &[u8], pos: &mut usize) -> Result<Vec<Accessory>> {
    let (count, raw_offset) = read_packed_array_header(data, pos, "PackedArrayRaw accessories")?;
    let Some(offset) = resolve_offset(raw_offset, "accessory-array offset")? else {
        return Ok(Vec::new());
    };
    if count == 0 {
        return Ok(Vec::new());
    }

    let mut acc_pos = offset;
    let mut accessories = Vec::with_capacity(count);

    for _ in 0..count {
        let source = data_tail(data, acc_pos, "AccessoryRaw")?;
        let (raw, _): (Ref<_, AccessoryRaw>, _) =
            Ref::from_prefix(source).map_err(|_| Error::UnexpectedEof {
                context: String::from("AccessoryRaw"),
            })?;

        let first_bone = i32::from_le_bytes(raw.first_bone);
        let num_bones = i32::from_le_bytes(raw.num_bones);

        // Nested packed array: mObjectIndices
        let inner_count = crate::checked_usize(
            u64::from(u32::from_le_bytes(raw.object_indices.count)),
            "accessory object-index count",
        )?;
        let inner_offset = resolve_offset(
            u64::from_le_bytes(raw.object_indices.offset),
            "accessory object-index offset",
        )?;

        let object_indices = if inner_count > 0
            && let Some(inner_offset) = inner_offset
        {
            let byte_count = inner_count
                .checked_mul(core::mem::size_of::<i32>())
                .ok_or(Error::SizeOverflow("accessory object-index data"))?;
            let end = inner_offset
                .checked_add(byte_count)
                .ok_or(Error::SizeOverflow("accessory object-index range"))?;
            let source = data
                .get(inner_offset..end)
                .ok_or_else(|| Error::UnexpectedEof {
                    context: "accessory object indices".into(),
                })?;
            let mut indices = Vec::with_capacity(inner_count);
            let mut idx_cur = Cursor::new(source);
            for _ in 0..inner_count {
                indices.push(idx_cur.read_i32_le()?);
            }
            indices
        } else {
            Vec::new()
        };

        accessories.push(Accessory {
            first_bone,
            num_bones,
            object_indices,
        });

        acc_pos = acc_pos
            .checked_add(core::mem::size_of::<AccessoryRaw>())
            .ok_or(Error::SizeOverflow("accessory array"))?;
    }

    Ok(accessories)
}

/// Read valid-accessory indices (4-byte i32 per element).
///
/// Both supported versions store raw indices into the accessories array. They
/// are deliberately preserved without resolution so duplicates and malformed
/// or sentinel values remain observable to permissive readers.
pub(super) fn read_valid_accessory_indices(data: &[u8], pos: &mut usize) -> Result<Vec<i32>> {
    let (count, raw_offset) =
        read_packed_array_header(data, pos, "PackedArrayRaw valid_accessory_indices")?;
    let Some(offset) = resolve_offset(raw_offset, "valid-accessory index offset")? else {
        return Ok(Vec::new());
    };
    if count == 0 {
        return Ok(Vec::new());
    }

    let mut valid = Vec::with_capacity(count);
    let mut idx_cur = Cursor::new(data_tail(data, offset, "valid-accessory indices")?);

    for _ in 0..count {
        valid.push(idx_cur.read_i32_le()?);
    }

    Ok(valid)
}
