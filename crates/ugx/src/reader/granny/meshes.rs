//! Granny mesh parsing from chunk 0x703.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::constants::GRANNY_BONE_BINDING_SIZE;
use crate::error::{Error, Result};
use crate::types::{GrannyBoneBinding, GrannyMesh};
use nostdio::{ReadLe, SliceCursor, read_null_terminated_string};

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
pub(in crate::reader) fn parse_granny_meshes(granny: &[u8]) -> Result<Vec<GrannyMesh>> {
    if granny.len() < 0x70 {
        return Err(Error::UnsupportedFormat(format!(
            "Granny chunk too small for model header ({} < 0x70)",
            granny.len()
        )));
    }

    let mut sc = SliceCursor::new(&granny[0x60..]);
    let model_count = sc.read_u32_le()? as usize;
    if model_count == 0 {
        return Ok(Vec::new());
    }

    let models_ptr_offs = sc.read_u64_le()? as usize;
    if models_ptr_offs + 8 > granny.len() {
        return Err(Error::UnsupportedFormat(format!(
            "Granny model pointer array out of bounds (0x{:X} + 8 > 0x{:X})",
            models_ptr_offs,
            granny.len()
        )));
    }

    let mut sc = SliceCursor::new(&granny[models_ptr_offs..]);
    let model_offs = sc.read_u64_le()? as usize;
    if model_offs + 0x60 > granny.len() {
        return Err(Error::UnsupportedFormat(format!(
            "Granny model data out of bounds (0x{:X} + 0x60 > 0x{:X})",
            model_offs,
            granny.len()
        )));
    }

    let mut sc = SliceCursor::new(&granny[model_offs + 0x54..]);
    let mesh_binding_count = sc.read_u32_le()? as usize;
    let mesh_bindings_ptr = sc.read_u64_le()? as usize;

    if mesh_binding_count == 0 {
        return Ok(Vec::new());
    }

    let mut meshes = Vec::with_capacity(mesh_binding_count);

    for i in 0..mesh_binding_count {
        let bp = mesh_bindings_ptr + i * 8;
        if bp + 8 > granny.len() {
            break;
        }

        let mut bsc = SliceCursor::new(&granny[bp..]);
        let mesh_offs = bsc.read_u64_le()? as usize;
        if mesh_offs + 0x3C > granny.len() {
            continue;
        }

        let mut nsc = SliceCursor::new(&granny[mesh_offs..]);
        let name_ptr = nsc.read_u64_le()? as usize;
        let name = if name_ptr < granny.len() {
            read_null_terminated_string(&granny[name_ptr..])
        } else {
            format!("mesh_{}", i)
        };

        let mut bbsc = SliceCursor::new(&granny[mesh_offs + 0x30..]);
        let bone_binding_count = bbsc.read_u32_le()? as usize;
        let bone_bindings_ptr = bbsc.read_u64_le()? as usize;

        let mut bone_bindings = Vec::with_capacity(bone_binding_count);

        for j in 0..bone_binding_count {
            let bb_start = bone_bindings_ptr + j * GRANNY_BONE_BINDING_SIZE;
            if bb_start + GRANNY_BONE_BINDING_SIZE > granny.len() {
                break;
            }

            let mut bbsc = SliceCursor::new(&granny[bb_start..]);
            let bone_name_ptr = bbsc.read_u64_le()? as usize;
            let bone_name = if bone_name_ptr < granny.len() {
                read_null_terminated_string(&granny[bone_name_ptr..])
            } else {
                String::new()
            };

            // OBBMin[3] at +0x08
            let obb_min = [
                bbsc.read_f32_le()?,
                bbsc.read_f32_le()?,
                bbsc.read_f32_le()?,
            ];

            // OBBMax[3] at +0x14
            let obb_max = [
                bbsc.read_f32_le()?,
                bbsc.read_f32_le()?,
                bbsc.read_f32_le()?,
            ];

            // TriangleIndices RTA at +0x20: count(i32) + ptr(u64)
            let tri_count = bbsc.read_i32_le()? as usize;
            let tri_ptr = bbsc.read_u64_le()? as usize;

            let triangle_indices =
                if tri_count > 0 && tri_ptr > 0 && tri_ptr + tri_count * 4 <= granny.len() {
                    let mut indices = Vec::with_capacity(tri_count);
                    let mut tsc = SliceCursor::new(&granny[tri_ptr..]);
                    for _ in 0..tri_count {
                        indices.push(tsc.read_i32_le()?);
                    }
                    indices
                } else {
                    Vec::new()
                };

            if !bone_name.is_empty() {
                bone_bindings.push(GrannyBoneBinding {
                    bone_name,
                    obb_min,
                    obb_max,
                    triangle_indices,
                });
            }
        }

        meshes.push(GrannyMesh {
            name,
            bone_bindings,
        });
    }

    Ok(meshes)
}
