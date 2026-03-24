//! Granny chunk (0x703) parser.
//!
//! Parses granny bones (inverse world matrices) and granny meshes
//! (bone bindings per mesh) from the Granny2-compatible serialized chunk.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::bytes::{
    read_f32_le, read_i32_le, read_null_terminated_string, read_u32_le, read_u64_le,
};
use crate::error::Result;
use crate::raw::{GRANNY_BONE_BINDING_SIZE, GRANNY_BONE_INVERSE_WORLD_OFFSET, GRANNY_BONE_SIZE};
use crate::types::{GrannyBone, GrannyMesh, Matrix4x4};

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
pub(super) fn parse_granny_bones(granny: &[u8]) -> Result<Vec<GrannyBone>> {
    if granny.len() < 0x40 {
        return Ok(Vec::new());
    }

    let mut p = 0x30usize;
    let skeleton_count = read_u32_le(granny, &mut p)? as usize;
    if skeleton_count == 0 {
        return Ok(Vec::new());
    }

    let skeleton_ptr_array_offs = read_u64_le(granny, &mut p)? as usize;
    if skeleton_ptr_array_offs + 8 > granny.len() {
        return Ok(Vec::new());
    }

    let mut p = skeleton_ptr_array_offs;
    let skeleton_offs = read_u64_le(granny, &mut p)? as usize;
    if skeleton_offs + 0x14 > granny.len() {
        return Ok(Vec::new());
    }

    let mut p = skeleton_offs + 0x08;
    let bones_len = read_u32_le(granny, &mut p)? as usize;
    let bones_offs = read_u64_le(granny, &mut p)? as usize;

    if bones_len == 0 {
        return Ok(Vec::new());
    }

    let mut bones = Vec::with_capacity(bones_len);

    for i in 0..bones_len {
        let bone_start = bones_offs + (i * GRANNY_BONE_SIZE);
        if bone_start + GRANNY_BONE_SIZE > granny.len() {
            break;
        }

        let mut p = bone_start;
        let name_offs = read_u64_le(granny, &mut p)? as usize;
        let parent_index = read_i32_le(granny, &mut p)?;

        let name = if name_offs < granny.len() {
            read_null_terminated_string(&granny[name_offs..])?
        } else {
            String::new()
        };

        let mut p = bone_start + GRANNY_BONE_INVERSE_WORLD_OFFSET;
        if p + 64 > granny.len() {
            break;
        }

        let mut rows = [[0.0f32; 4]; 4];
        for row in &mut rows {
            for col in row {
                *col = read_f32_le(granny, &mut p)?;
            }
        }
        let inverse_world_matrix = Matrix4x4 { rows };

        bones.push(GrannyBone {
            name,
            parent_index,
            inverse_world_matrix,
        });
    }

    Ok(bones)
}

/// Parse Granny mesh data from the Granny chunk (0x703).
///
/// file_info:
///   +0x60: i32 ModelCount (must be 1)
///   +0x64: u64 Models ptr (to array of model pointers)
///
/// Model:
///   +0x54: i32 MeshBindingCount
///   +0x58: u64 MeshBindings ptr (to array of mesh pointers)
///
/// Mesh (76 bytes = 0x4C):
///   +0x00: u64 Name ptr
///   +0x30: i32 BoneBindingCount
///   +0x34: u64 BoneBindings ptr
///
/// bone_binding (44 bytes = 0x2C):
///   +0x00: u64 BoneName ptr
pub(super) fn parse_granny_meshes(granny: &[u8]) -> Result<Vec<GrannyMesh>> {
    if granny.len() < 0x70 {
        return Ok(Vec::new());
    }

    let mut p = 0x60usize;
    let model_count = read_u32_le(granny, &mut p)? as usize;
    if model_count == 0 {
        return Ok(Vec::new());
    }

    let models_ptr_offs = read_u64_le(granny, &mut p)? as usize;
    if models_ptr_offs + 8 > granny.len() {
        return Ok(Vec::new());
    }

    let mut p = models_ptr_offs;
    let model_offs = read_u64_le(granny, &mut p)? as usize;
    if model_offs + 0x60 > granny.len() {
        return Ok(Vec::new());
    }

    let mut p = model_offs + 0x54;
    let mesh_binding_count = read_u32_le(granny, &mut p)? as usize;
    let mesh_bindings_ptr = read_u64_le(granny, &mut p)? as usize;

    if mesh_binding_count == 0 {
        return Ok(Vec::new());
    }

    let mut meshes = Vec::with_capacity(mesh_binding_count);

    for i in 0..mesh_binding_count {
        let mut bp = mesh_bindings_ptr + i * 8;
        if bp + 8 > granny.len() {
            break;
        }

        let mesh_offs = read_u64_le(granny, &mut bp)? as usize;
        if mesh_offs + 0x3C > granny.len() {
            continue;
        }

        let mut np = mesh_offs;
        let name_ptr = read_u64_le(granny, &mut np)? as usize;
        let name = if name_ptr < granny.len() {
            read_null_terminated_string(&granny[name_ptr..])?
        } else {
            format!("mesh_{}", i)
        };

        let mut bbp = mesh_offs + 0x30;
        let bone_binding_count = read_u32_le(granny, &mut bbp)? as usize;
        let bone_bindings_ptr = read_u64_le(granny, &mut bbp)? as usize;

        let mut bone_bindings = Vec::with_capacity(bone_binding_count);

        for j in 0..bone_binding_count {
            let mut bb_p = bone_bindings_ptr + j * GRANNY_BONE_BINDING_SIZE;
            if bb_p + 8 > granny.len() {
                break;
            }

            let bone_name_ptr = read_u64_le(granny, &mut bb_p)? as usize;
            let bone_name = if bone_name_ptr < granny.len() {
                read_null_terminated_string(&granny[bone_name_ptr..])?
            } else {
                String::new()
            };

            if !bone_name.is_empty() {
                bone_bindings.push(bone_name);
            }
        }

        meshes.push(GrannyMesh {
            name,
            bone_bindings,
        });
    }

    Ok(meshes)
}
