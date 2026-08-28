//! Granny bone parsing from chunk 0x703.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::constants::{
    GRANNY_BONE_EXTENDED_DATA_OFFSET, GRANNY_BONE_INVERSE_WORLD_OFFSET, GRANNY_BONE_SIZE,
};
use crate::error::{Error, Result};
use crate::types::{GrannyBone, GrannyLocalTransform, Matrix4x4};
use nostdio::{Cursor, ReadLe, read_null_terminated_string};

use super::extended_data::{parse_type_def_array, parse_variant_data};
use super::{data_range, data_tail, pointer_offset};

/// Parse the fixed-size local transform embedded in a Granny bone.
fn parse_local_transform(data: &[u8]) -> Result<GrannyLocalTransform> {
    let mut cursor = Cursor::new(data);
    let flags = cursor.read_u32_le()?;
    let mut position = [0.0f32; 3];
    for component in &mut position {
        *component = cursor.read_f32_le()?;
    }
    let mut orientation = [0.0f32; 4];
    for component in &mut orientation {
        *component = cursor.read_f32_le()?;
    }
    let mut scale_shear = [[0.0f32; 3]; 3];
    for row in &mut scale_shear {
        for component in row {
            *component = cursor.read_f32_le()?;
        }
    }
    Ok(GrannyLocalTransform {
        flags,
        position,
        orientation,
        scale_shear,
    })
}

/// Parse granny bones from granny chunk (0x703).
///
/// Granny `file_info` layout (verified from IDA and real file dump):
///   +0x30: u32 `SkeletonCount`
///   +0x34: u64 Skeletons -> skeleton pointer array
///
/// Skeleton struct layout:
///   +0x00: u64 Name
///   +0x08: u32 `BoneCount`
///   +0x0C: u64 Bones -> bone array
///
/// Bone struct (164 bytes each):
///   +0x00: u64 nameOffs
///   +0x08: i32 parent
///   +0x0C: transform `LocalTransform` (68 bytes)
///   +0x50: `matrix_4x4` `InverseWorld4x4` (64 bytes)
///   +0x90: f32 `LODError`
///   +0x94: variant `ExtendedData` (16 bytes)
pub(in crate::reader) fn parse_granny_bones(granny: &[u8]) -> Result<(Vec<GrannyBone>, u32)> {
    if granny.len() < 0x40 {
        return Err(Error::UnsupportedFormat(format!(
            "Granny chunk too small for skeleton header ({} < 0x40)",
            granny.len()
        )));
    }

    let mut cursor = Cursor::new(&granny[0x30..]);
    let skeleton_count =
        crate::checked_usize(u64::from(cursor.read_u32_le()?), "Granny skeleton count")?;
    if skeleton_count == 0 {
        return Ok((Vec::new(), 0));
    }

    let skeleton_ptr_array_offs =
        pointer_offset(cursor.read_u64_le()?, "Granny skeleton pointer array")?;
    let skeleton_pointer_data = data_range(
        granny,
        skeleton_ptr_array_offs,
        8,
        "Granny skeleton pointer array",
    )?;

    let mut cursor = Cursor::new(skeleton_pointer_data);
    let skeleton_offs = pointer_offset(cursor.read_u64_le()?, "Granny skeleton")?;
    let skeleton_data = data_range(granny, skeleton_offs, 0x28, "Granny skeleton data")?;

    let mut cursor = Cursor::new(&skeleton_data[0x08..]);
    let bones_len = crate::checked_usize(u64::from(cursor.read_u32_le()?), "Granny bone count")?;
    let bones_offs = pointer_offset(cursor.read_u64_le()?, "Granny bone array")?;
    let skeleton_lod_type = cursor.read_u32_le()?;

    if bones_len == 0 {
        return Ok((Vec::new(), skeleton_lod_type));
    }

    let mut bones = Vec::with_capacity(bones_len);

    for index in 0..bones_len {
        let relative = index
            .checked_mul(GRANNY_BONE_SIZE)
            .ok_or(Error::SizeOverflow("Granny bone offset"))?;
        let bone_start = bones_offs
            .checked_add(relative)
            .ok_or(Error::SizeOverflow("Granny bone offset"))?;
        let bone_data = data_range(granny, bone_start, GRANNY_BONE_SIZE, "Granny bone")?;

        let mut cursor = Cursor::new(bone_data);
        let name_offs = pointer_offset(cursor.read_u64_le()?, "Granny bone name")?;
        let parent_index = cursor.read_i32_le()?;

        let name = data_tail(granny, name_offs, "Granny bone name")
            .map_or_else(|_| String::new(), read_null_terminated_string);

        // Parse local transform at bone+0x0C (68 bytes: flags + pos + quat + scale_shear)
        let local_transform = Some(parse_local_transform(&bone_data[0x0C..0x50])?);

        // Parse inverse world matrix at bone+0x50 (64 bytes)
        let iw_start = bone_start
            .checked_add(GRANNY_BONE_INVERSE_WORLD_OFFSET)
            .ok_or(Error::SizeOverflow("inverse-world offset"))?;

        let mut iw_pos = iw_start;
        let inverse_world_matrix = Matrix4x4::read(granny, &mut iw_pos)?;

        // Parse LOD error at bone+0x90 (4 bytes)
        let mut cursor = Cursor::new(&bone_data[0x90..]);
        let lod_error = cursor.read_f32_le()?;

        // Parse ExtendedData at bone+0x94: {type_def_ptr (u64), data_ptr (u64)}
        let ext_offset = bone_start
            .checked_add(GRANNY_BONE_EXTENDED_DATA_OFFSET)
            .ok_or(Error::SizeOverflow("extended-data offset"))?;
        let (extended_data, extended_data_type) = if let Ok(ext_data) =
            data_range(granny, ext_offset, 16, "Granny extended data")
        {
            let mut cursor = Cursor::new(ext_data);
            let type_ptr = pointer_offset(cursor.read_u64_le()?, "Granny type definition")?;
            let data_ptr = pointer_offset(cursor.read_u64_le()?, "Granny variant data")?;

            if type_ptr > 0 && type_ptr < granny.len() && data_ptr > 0 && data_ptr < granny.len() {
                let type_members = parse_type_def_array(granny, type_ptr)?;
                let variant = parse_variant_data(granny, data_ptr, &type_members)?;
                (Some(variant), Some(type_members))
            } else {
                (None, None)
            }
        } else {
            (None, None)
        };

        bones.push(GrannyBone {
            name,
            parent_index,
            local_transform,
            inverse_world_matrix,
            lod_error,
            extended_data,
            extended_data_type,
        });
    }

    Ok((bones, skeleton_lod_type))
}
