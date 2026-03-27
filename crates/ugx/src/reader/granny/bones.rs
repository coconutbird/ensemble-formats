//! Granny bone parsing from chunk 0x703.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::constants::{
    GRANNY_BONE_EXTENDED_DATA_OFFSET, GRANNY_BONE_INVERSE_WORLD_OFFSET, GRANNY_BONE_SIZE,
};
use crate::error::{Error, Result};
use crate::types::{GrannyBone, GrannyLocalTransform, Matrix4x4};
use nostdio::{ReadLe, SliceCursor, read_null_terminated_string};

use super::extended_data::{parse_type_def_array, parse_variant_data};

/// Parse granny bones from granny chunk (0x703).
///
/// Granny file_info layout (verified from IDA and real file dump):
///   +0x30: u32 SkeletonCount
///   +0x34: u64 Skeletons -> skeleton pointer array
///
/// Skeleton struct layout:
///   +0x00: u64 Name
///   +0x08: u32 BoneCount
///   +0x0C: u64 Bones -> bone array
///
/// Bone struct (164 bytes each):
///   +0x00: u64 nameOffs
///   +0x08: i32 parent
///   +0x0C: transform LocalTransform (68 bytes)
///   +0x50: matrix_4x4 InverseWorld4x4 (64 bytes)
///   +0x90: f32 LODError
///   +0x94: variant ExtendedData (16 bytes)
pub(in crate::reader) fn parse_granny_bones(granny: &[u8]) -> Result<(Vec<GrannyBone>, u32)> {
    if granny.len() < 0x40 {
        return Err(Error::UnsupportedFormat(format!(
            "Granny chunk too small for skeleton header ({} < 0x40)",
            granny.len()
        )));
    }

    let mut sc = SliceCursor::new(&granny[0x30..]);
    let skeleton_count = sc.read_u32_le()? as usize;
    if skeleton_count == 0 {
        return Ok((Vec::new(), 0));
    }

    let skeleton_ptr_array_offs = sc.read_u64_le()? as usize;
    if skeleton_ptr_array_offs + 8 > granny.len() {
        return Err(Error::UnsupportedFormat(format!(
            "Granny skeleton pointer array out of bounds (0x{:X} + 8 > 0x{:X})",
            skeleton_ptr_array_offs,
            granny.len()
        )));
    }

    let mut sc = SliceCursor::new(&granny[skeleton_ptr_array_offs..]);
    let skeleton_offs = sc.read_u64_le()? as usize;
    if skeleton_offs + 0x28 > granny.len() {
        return Err(Error::UnsupportedFormat(format!(
            "Granny skeleton data out of bounds (0x{:X} + 0x28 > 0x{:X})",
            skeleton_offs,
            granny.len()
        )));
    }

    let mut sc = SliceCursor::new(&granny[skeleton_offs + 0x08..]);
    let bones_len = sc.read_u32_le()? as usize;
    let bones_offs = sc.read_u64_le()? as usize;
    let skeleton_lod_type = sc.read_u32_le()?;

    if bones_len == 0 {
        return Ok((Vec::new(), skeleton_lod_type));
    }

    let mut bones = Vec::with_capacity(bones_len);

    for i in 0..bones_len {
        let bone_start = bones_offs + (i * GRANNY_BONE_SIZE);
        if bone_start + GRANNY_BONE_SIZE > granny.len() {
            break;
        }

        let mut sc = SliceCursor::new(&granny[bone_start..]);
        let name_offs = sc.read_u64_le()? as usize;
        let parent_index = sc.read_i32_le()?;

        let name = if name_offs < granny.len() {
            read_null_terminated_string(&granny[name_offs..])
        } else {
            String::new()
        };

        // Parse local transform at bone+0x0C (68 bytes: flags + pos + quat + scale_shear)
        let mut sc = SliceCursor::new(&granny[bone_start + 0x0C..]);
        let lt_flags = sc.read_u32_le()?;
        let mut lt_position = [0.0f32; 3];
        for v in &mut lt_position {
            *v = sc.read_f32_le()?;
        }
        let mut lt_orientation = [0.0f32; 4];
        for v in &mut lt_orientation {
            *v = sc.read_f32_le()?;
        }
        let mut lt_scale_shear = [[0.0f32; 3]; 3];
        for row in &mut lt_scale_shear {
            for v in row {
                *v = sc.read_f32_le()?;
            }
        }
        let local_transform = Some(GrannyLocalTransform {
            flags: lt_flags,
            position: lt_position,
            orientation: lt_orientation,
            scale_shear: lt_scale_shear,
        });

        // Parse inverse world matrix at bone+0x50 (64 bytes)
        let iw_start = bone_start + GRANNY_BONE_INVERSE_WORLD_OFFSET;
        if iw_start + 64 > granny.len() {
            break;
        }

        let mut iw_pos = iw_start;
        let inverse_world_matrix = Matrix4x4::read(granny, &mut iw_pos)?;

        // Parse LOD error at bone+0x90 (4 bytes)
        let mut sc = SliceCursor::new(&granny[bone_start + 0x90..]);
        let lod_error = sc.read_f32_le()?;

        // Parse ExtendedData at bone+0x94: {type_def_ptr (u64), data_ptr (u64)}
        let ext_offset = bone_start + GRANNY_BONE_EXTENDED_DATA_OFFSET;
        let (extended_data, extended_data_type) = if ext_offset + 16 <= granny.len() {
            let mut sc = SliceCursor::new(&granny[ext_offset..]);
            let type_ptr = sc.read_u64_le()? as usize;
            let data_ptr = sc.read_u64_le()? as usize;

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
