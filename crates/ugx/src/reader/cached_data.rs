//! BCachedData (chunk 0x700) sub-parsers.
//!
//! Reads packed sections, bones, bone bounds, and univert packers from the
//! cached data chunk using zerocopy overlays. Supports both DE (152-byte
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
use nostdio::{ReadLe, SliceCursor, read_null_terminated_string};

/// Read packed sections array from cached data.
///
/// Section stride depends on version: 152 bytes (DE) or 72 bytes (HW2).
pub(super) fn read_packed_sections(
    data: &[u8],
    pos: &mut usize,
    version: UgxVersion,
) -> Result<Vec<Section>> {
    let (arr, _): (Ref<_, PackedArrayRaw>, _) =
        Ref::from_prefix(&data[*pos..]).map_err(|_| Error::UnexpectedEof {
            context: String::from("PackedArrayRaw sections"),
        })?;
    let count = u32::from_le_bytes(arr.count) as usize;
    let offset = u64::from_le_bytes(arr.offset) as usize;
    *pos += core::mem::size_of::<PackedArrayRaw>();

    if count == 0 {
        return Ok(Vec::new());
    }

    let mut sec_pos = offset;
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
/// - HW1/DE (112 bytes): bone_remap(16) + UnivertPacker(84) + flags(12)
/// - HW2 (32 bytes): flags(8) + unknown(8) + bone_remap(16)
fn read_packed_section(data: &[u8], pos: &mut usize, version: UgxVersion) -> Result<Section> {
    let (fixed, _): (Ref<_, PackedSectionFixedRaw>, _) =
        Ref::from_prefix(&data[*pos..]).map_err(|_| Error::UnexpectedEof {
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
    *pos += core::mem::size_of::<PackedSectionFixedRaw>();

    let (bone_remap, base_vert_packer, rigid_only, global_bones) = match version {
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
        base_vert_packer,
        bone_remap,
        rigid_only,
        global_bones,
    })
}

/// HW1/DE section tail: bone_remap(16) + UnivertPacker(84) + flags(12).
fn read_section_tail_hw1(
    data: &[u8],
    pos: &mut usize,
) -> Result<(Vec<u8>, Option<UnivertPacker>, bool, bool)> {
    let bone_remap = read_bone_remap(data, pos)?;
    let packer = read_packed_univert_packer(data, pos)?;

    let mut cur = SliceCursor::new(&data[*pos..]);
    let rigid_only = cur.read_i32_le()? != 0;
    let global_bones = cur.read_i32_le()? != 0;
    let _padding = cur.read_i32_le()?;
    *pos += cur.position();

    Ok((bone_remap, Some(packer), rigid_only, global_bones))
}

/// HW2 section tail: flags(8) + unknown(8) + bone_remap(16).
fn read_section_tail_hw2(
    data: &[u8],
    pos: &mut usize,
) -> Result<(Vec<u8>, Option<UnivertPacker>, bool, bool)> {
    let mut cur = SliceCursor::new(&data[*pos..]);
    let rigid_only = cur.read_i32_le()? != 0;
    let global_bones = cur.read_i32_le()? != 0;
    let _unknown1 = cur.read_i32_le()?;
    let _unknown2 = cur.read_i32_le()?;
    *pos += cur.position();

    let bone_remap = read_bone_remap(data, pos)?;

    Ok((bone_remap, None, rigid_only, global_bones))
}

/// Read a bone remap packed array: overlay `PackedArrayRaw`, resolve offset, copy bytes.
fn read_bone_remap(data: &[u8], pos: &mut usize) -> Result<Vec<u8>> {
    let (arr, _): (Ref<_, PackedArrayRaw>, _) =
        Ref::from_prefix(&data[*pos..]).map_err(|_| Error::UnexpectedEof {
            context: String::from("PackedArrayRaw bone_remap"),
        })?;
    let count = u32::from_le_bytes(arr.count) as usize;
    let offset = u64::from_le_bytes(arr.offset) as usize;
    *pos += core::mem::size_of::<PackedArrayRaw>();

    if count > 0 && offset != EMPTY_OFFSET_SENTINEL as usize && offset + count <= data.len() {
        Ok(data[offset..offset + count].to_vec())
    } else {
        Ok(Vec::new())
    }
}

/// Read packed UnivertPacker (84 bytes on-disk).
fn read_packed_univert_packer(data: &[u8], pos: &mut usize) -> Result<UnivertPacker> {
    let mut cur = SliceCursor::new(&data[*pos..]);
    let pack_order_offset = cur.read_u64_le()? as usize;
    let decl_order_offset = cur.read_u64_le()? as usize;

    let pack_order =
        if pack_order_offset == EMPTY_OFFSET_SENTINEL as usize || pack_order_offset >= data.len() {
            String::new()
        } else {
            read_null_terminated_string(&data[pack_order_offset..])
        };

    let decl_order =
        if decl_order_offset == EMPTY_OFFSET_SENTINEL as usize || decl_order_offset >= data.len() {
            String::new()
        } else {
            read_null_terminated_string(&data[decl_order_offset..])
        };

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
    *pos += cur.position();

    Ok(UnivertPacker {
        pack_order,
        decl_order,
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
    })
}

/// Read packed bones array from cached data.
pub(super) fn read_packed_bones(data: &[u8], pos: &mut usize) -> Result<Vec<Bone>> {
    let (arr, _): (Ref<_, PackedArrayRaw>, _) =
        Ref::from_prefix(&data[*pos..]).map_err(|_| Error::UnexpectedEof {
            context: String::from("PackedArrayRaw bones"),
        })?;
    let count = u32::from_le_bytes(arr.count) as usize;
    let offset = u64::from_le_bytes(arr.offset) as usize;
    *pos += core::mem::size_of::<PackedArrayRaw>();

    if count == 0 {
        return Ok(Vec::new());
    }

    let mut bone_pos = offset;
    let mut bones = Vec::with_capacity(count);

    for _ in 0..count {
        bones.push(read_packed_bone(data, &mut bone_pos)?);
    }

    Ok(bones)
}

/// Read a single packed bone (80 bytes).
fn read_packed_bone(data: &[u8], pos: &mut usize) -> Result<Bone> {
    let (raw, _): (Ref<_, PackedBoneRaw>, _) =
        Ref::from_prefix(&data[*pos..]).map_err(|_| Error::UnexpectedEof {
            context: String::from("PackedBoneRaw"),
        })?;
    *pos += core::mem::size_of::<PackedBoneRaw>();

    let name_offset = u64::from_le_bytes(raw.name_offset) as usize;
    let name = if name_offset == EMPTY_OFFSET_SENTINEL as usize || name_offset >= data.len() {
        String::new()
    } else {
        read_null_terminated_string(&data[name_offset..])
    };

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
    let packed_arr_size = core::mem::size_of::<PackedArrayRaw>();

    let (low_arr, _): (Ref<_, PackedArrayRaw>, _) =
        Ref::from_prefix(&data[*pos..]).map_err(|_| Error::UnexpectedEof {
            context: String::from("PackedArrayRaw boneBoundsLow"),
        })?;
    let low_count = u32::from_le_bytes(low_arr.count) as usize;
    let low_offset = u64::from_le_bytes(low_arr.offset) as usize;
    *pos += packed_arr_size;

    let (high_arr, _): (Ref<_, PackedArrayRaw>, _) =
        Ref::from_prefix(&data[*pos..]).map_err(|_| Error::UnexpectedEof {
            context: String::from("PackedArrayRaw boneBoundsHigh"),
        })?;
    let high_count = u32::from_le_bytes(high_arr.count) as usize;
    let high_offset = u64::from_le_bytes(high_arr.offset) as usize;
    *pos += packed_arr_size;

    if low_count == 0 || low_count != high_count {
        return Ok(Vec::new());
    }

    let vec3_size = core::mem::size_of::<BVector3Raw>();
    let mut bounds = Vec::with_capacity(low_count);

    for i in 0..low_count {
        let low_pos = low_offset + i * vec3_size;
        let high_pos = high_offset + i * vec3_size;

        if low_pos + vec3_size > data.len() || high_pos + vec3_size > data.len() {
            break;
        }

        let (lo, _): (Ref<_, BVector3Raw>, _) =
            Ref::from_prefix(&data[low_pos..]).map_err(|_| Error::UnexpectedEof {
                context: alloc::string::String::from("BVector3Raw boneBoundsLow"),
            })?;
        let (hi, _): (Ref<_, BVector3Raw>, _) =
            Ref::from_prefix(&data[high_pos..]).map_err(|_| Error::UnexpectedEof {
                context: alloc::string::String::from("BVector3Raw boneBoundsHigh"),
            })?;

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
    let (arr, _): (Ref<_, PackedArrayRaw>, _) =
        Ref::from_prefix(&data[*pos..]).map_err(|_| Error::UnexpectedEof {
            context: String::from("PackedArrayRaw accessories"),
        })?;
    let count = u32::from_le_bytes(arr.count) as usize;
    let offset = u64::from_le_bytes(arr.offset) as usize;
    *pos += core::mem::size_of::<PackedArrayRaw>();

    if count == 0
        || offset == EMPTY_OFFSET_SENTINEL_32 as usize
        || offset == EMPTY_OFFSET_SENTINEL as usize
    {
        return Ok(Vec::new());
    }

    let mut acc_pos = offset;
    let mut accessories = Vec::with_capacity(count);

    for _ in 0..count {
        let (raw, _): (Ref<_, AccessoryRaw>, _) =
            Ref::from_prefix(&data[acc_pos..]).map_err(|_| Error::UnexpectedEof {
                context: String::from("AccessoryRaw"),
            })?;

        let first_bone = i32::from_le_bytes(raw.first_bone);
        let num_bones = i32::from_le_bytes(raw.num_bones);

        // Nested packed array: mObjectIndices
        let inner_count = u32::from_le_bytes(raw.object_indices.count) as usize;
        let inner_offset = u64::from_le_bytes(raw.object_indices.offset) as usize;

        let object_indices = if inner_count > 0
            && inner_offset != EMPTY_OFFSET_SENTINEL_32 as usize
            && inner_offset != EMPTY_OFFSET_SENTINEL as usize
            && inner_offset + inner_count * 4 <= data.len()
        {
            let mut indices = Vec::with_capacity(inner_count);
            let mut idx_cur = SliceCursor::new(&data[inner_offset..]);
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

        acc_pos += core::mem::size_of::<AccessoryRaw>();
    }

    Ok(accessories)
}

/// Read HW2 valid-accessory indices (4-byte i32 per element).
///
/// In HW2, valid accessories are stored as indices into the accessories array
/// rather than full 24-byte `AccessoryRaw` structs. We resolve them by looking
/// up the corresponding accessory from the already-parsed list.
pub(super) fn read_valid_accessory_indices(
    data: &[u8],
    pos: &mut usize,
    accessories: &[Accessory],
) -> Result<Vec<Accessory>> {
    let (arr, _): (Ref<_, PackedArrayRaw>, _) =
        Ref::from_prefix(&data[*pos..]).map_err(|_| Error::UnexpectedEof {
            context: String::from("PackedArrayRaw valid_accessory_indices (HW2)"),
        })?;
    let count = u32::from_le_bytes(arr.count) as usize;
    let offset = u64::from_le_bytes(arr.offset) as usize;
    *pos += core::mem::size_of::<PackedArrayRaw>();

    if count == 0
        || offset == EMPTY_OFFSET_SENTINEL_32 as usize
        || offset == EMPTY_OFFSET_SENTINEL as usize
    {
        return Ok(Vec::new());
    }

    let mut valid = Vec::with_capacity(count);
    let mut idx_cur = SliceCursor::new(&data[offset..]);

    for _ in 0..count {
        let idx = idx_cur.read_i32_le()? as usize;
        if idx < accessories.len() {
            valid.push(accessories[idx].clone());
        }
    }

    Ok(valid)
}
