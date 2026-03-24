//! BCachedData (chunk 0x700) sub-parsers.
//!
//! Reads packed sections, bones, bone bounds, and univert packers from the
//! cached data chunk using zerocopy overlays.

use alloc::string::String;
use alloc::vec::Vec;
use zerocopy::Ref;

use crate::bytes::{
    read_f32_le, read_i32_le, read_null_terminated_string, read_u32_le, read_u64_le,
};
use crate::error::{Error, Result};
use crate::raw::{AccessoryRaw, PackedArrayRaw, PackedBoneRaw, PackedSectionFixedRaw};
use crate::types::{AABB, Accessory, Bone, Matrix4x4, Section};
use crate::vertex::element::VertexElementType;
use crate::vertex::packer::UnivertPacker;

/// Read packed sections array from cached data.
///
/// Each section is 152 bytes (stride), located contiguously at `offset`.
pub(super) fn read_packed_sections(data: &[u8], pos: &mut usize) -> Result<Vec<Section>> {
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
        sections.push(read_packed_section(data, &mut sec_pos)?);
    }

    Ok(sections)
}

/// Read a single packed section (152 bytes).
fn read_packed_section(data: &[u8], pos: &mut usize) -> Result<Section> {
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

    // +0x28: LocalToGlobalBoneRemap packed array (16 bytes)
    let (remap_arr, _): (Ref<_, PackedArrayRaw>, _) =
        Ref::from_prefix(&data[*pos..]).map_err(|_| Error::UnexpectedEof {
            context: String::from("PackedArrayRaw bone_remap"),
        })?;
    let bone_remap_count = u32::from_le_bytes(remap_arr.count) as usize;
    let bone_remap_offset = u64::from_le_bytes(remap_arr.offset) as usize;
    *pos += core::mem::size_of::<PackedArrayRaw>();

    let bone_remap = if bone_remap_count > 0
        && bone_remap_offset != 0xFFFFFFFFFFFFFFFF
        && bone_remap_offset + bone_remap_count <= data.len()
    {
        data[bone_remap_offset..bone_remap_offset + bone_remap_count].to_vec()
    } else {
        Vec::new()
    };

    // +0x38: UnivertPacker (84 bytes)
    let base_vert_packer = read_packed_univert_packer(data, pos)?;

    let rigid_only = read_i32_le(data, pos)? != 0;
    let global_bones = read_i32_le(data, pos)? != 0;
    let _padding = read_i32_le(data, pos)?;

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

/// Read packed UnivertPacker (84 bytes on-disk).
fn read_packed_univert_packer(data: &[u8], pos: &mut usize) -> Result<UnivertPacker> {
    let pack_order_offset = read_u64_le(data, pos)? as usize;
    let decl_order_offset = read_u64_le(data, pos)? as usize;

    let pack_order = if pack_order_offset == 0xFFFFFFFFFFFFFFFF || pack_order_offset >= data.len() {
        String::new()
    } else {
        read_null_terminated_string(&data[pack_order_offset..])?
    };

    let decl_order = if decl_order_offset == 0xFFFFFFFFFFFFFFFF || decl_order_offset >= data.len() {
        String::new()
    } else {
        read_null_terminated_string(&data[decl_order_offset..])?
    };

    let pos_type = VertexElementType::from_u32(read_u32_le(data, pos)?);
    let basis_type = VertexElementType::from_u32(read_u32_le(data, pos)?);
    let basis_scale_type = VertexElementType::from_u32(read_u32_le(data, pos)?);
    let tangent_type = VertexElementType::from_u32(read_u32_le(data, pos)?);
    let normal_type = VertexElementType::from_u32(read_u32_le(data, pos)?);

    let mut uv_types = [VertexElementType::Ignore; 8];
    for uv_type in &mut uv_types {
        *uv_type = VertexElementType::from_u32(read_u32_le(data, pos)?);
    }

    let indices_type = VertexElementType::from_u32(read_u32_le(data, pos)?);
    let weights_type = VertexElementType::from_u32(read_u32_le(data, pos)?);
    let diffuse_type = VertexElementType::from_u32(read_u32_le(data, pos)?);
    let index_type = VertexElementType::from_u32(read_u32_le(data, pos)?);

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
    let name = if name_offset == 0xFFFFFFFFFFFFFFFF || name_offset >= data.len() {
        String::new()
    } else {
        read_null_terminated_string(&data[name_offset..])?
    };

    let mut rows = [[0.0f32; 4]; 4];
    for (i, row) in rows.iter_mut().enumerate() {
        for (j, col) in row.iter_mut().enumerate() {
            *col = f32::from_le_bytes(raw.model_to_bone[i * 4 + j]);
        }
    }
    let model_to_bone = Matrix4x4 { rows };

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

    let mut bounds = Vec::with_capacity(low_count);

    for i in 0..low_count {
        let mut low_pos = low_offset + i * 12;
        let mut high_pos = high_offset + i * 12;

        if low_pos + 12 > data.len() || high_pos + 12 > data.len() {
            break;
        }

        bounds.push(AABB {
            min: [
                read_f32_le(data, &mut low_pos)?,
                read_f32_le(data, &mut low_pos)?,
                read_f32_le(data, &mut low_pos)?,
            ],
            max: [
                read_f32_le(data, &mut high_pos)?,
                read_f32_le(data, &mut high_pos)?,
                read_f32_le(data, &mut high_pos)?,
            ],
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

    if count == 0 || offset == 0xFFFFFFFF || offset == 0xFFFFFFFFFFFFFFFF {
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
            && inner_offset != 0xFFFFFFFF
            && inner_offset != 0xFFFFFFFFFFFFFFFF
            && inner_offset + inner_count * 4 <= data.len()
        {
            let mut indices = Vec::with_capacity(inner_count);
            let mut idx_pos = inner_offset;
            for _ in 0..inner_count {
                indices.push(read_i32_le(data, &mut idx_pos)?);
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
