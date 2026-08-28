//! Granny mesh parsing from chunk 0x703.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::constants::GRANNY_BONE_BINDING_SIZE;
use crate::error::{Error, Result};
use crate::types::{GrannyBoneBinding, GrannyMesh};
use nostdio::{Cursor, ReadLe, read_null_terminated_string};

use super::{data_range, data_tail, pointer_offset};

/// Parse one fixed-size Granny bone binding.
fn parse_bone_binding(granny: &[u8], start: usize) -> Result<Option<GrannyBoneBinding>> {
    let binding_data = data_range(
        granny,
        start,
        GRANNY_BONE_BINDING_SIZE,
        "Granny bone binding",
    )?;
    let mut cursor = Cursor::new(binding_data);
    let bone_name_ptr = pointer_offset(cursor.read_u64_le()?, "Granny bone-binding name")?;
    let bone_name = data_tail(granny, bone_name_ptr, "Granny bone-binding name")
        .map_or_else(|_| String::new(), read_null_terminated_string);

    let obb_min = [
        cursor.read_f32_le()?,
        cursor.read_f32_le()?,
        cursor.read_f32_le()?,
    ];
    let obb_max = [
        cursor.read_f32_le()?,
        cursor.read_f32_le()?,
        cursor.read_f32_le()?,
    ];

    let triangle_count = usize::try_from(cursor.read_i32_le()?).unwrap_or_default();
    let triangle_pointer = cursor.read_u64_le()?;
    let triangle_indices = if triangle_count == 0 || triangle_pointer == 0 {
        Vec::new()
    } else {
        let triangle_offset = pointer_offset(triangle_pointer, "Granny triangle indices")?;
        let byte_count = triangle_count
            .checked_mul(core::mem::size_of::<i32>())
            .ok_or(Error::SizeOverflow("Granny triangle indices"))?;
        let mut cursor = Cursor::new(data_range(
            granny,
            triangle_offset,
            byte_count,
            "Granny triangle indices",
        )?);
        let mut indices = Vec::with_capacity(triangle_count);
        for _ in 0..triangle_count {
            indices.push(cursor.read_i32_le()?);
        }
        indices
    };

    Ok((!bone_name.is_empty()).then_some(GrannyBoneBinding {
        bone_name,
        obb_min,
        obb_max,
        triangle_indices,
    }))
}

/// Parse Granny mesh data from the Granny chunk (0x703).
///
/// `file_info`:
///   +0x60: i32 `ModelCount` (must be 1)
///   +0x64: u64 Models ptr (to array of model pointers)
///
/// Model:
///   +0x54: i32 `MeshBindingCount`
///   +0x58: u64 `MeshBindings` ptr (to array of mesh pointers)
///
/// Mesh (76 bytes = 0x4C):
///   +0x00: u64 Name ptr
///   +0x30: i32 `BoneBindingCount`
///   +0x34: u64 `BoneBindings` ptr
///
/// `bone_binding` (44 bytes = 0x2C):
///   +0x00: u64 `BoneName` ptr
pub(in crate::reader) fn parse_granny_meshes(granny: &[u8]) -> Result<Vec<GrannyMesh>> {
    if granny.len() < 0x70 {
        return Err(Error::UnsupportedFormat(format!(
            "Granny chunk too small for model header ({} < 0x70)",
            granny.len()
        )));
    }

    let mut cursor = Cursor::new(&granny[0x60..]);
    let model_count = crate::checked_usize(u64::from(cursor.read_u32_le()?), "Granny model count")?;
    if model_count == 0 {
        return Ok(Vec::new());
    }

    let models_ptr_offs = pointer_offset(cursor.read_u64_le()?, "Granny model pointer array")?;
    let model_pointer_data = data_range(granny, models_ptr_offs, 8, "Granny model pointer array")?;

    let mut cursor = Cursor::new(model_pointer_data);
    let model_offs = pointer_offset(cursor.read_u64_le()?, "Granny model")?;
    let model_data = data_range(granny, model_offs, 0x60, "Granny model data")?;

    let mut cursor = Cursor::new(&model_data[0x54..]);
    let mesh_binding_count = crate::checked_usize(
        u64::from(cursor.read_u32_le()?),
        "Granny mesh-binding count",
    )?;
    let mesh_bindings_ptr = pointer_offset(cursor.read_u64_le()?, "Granny mesh bindings")?;

    if mesh_binding_count == 0 {
        return Ok(Vec::new());
    }

    let mut meshes = Vec::with_capacity(mesh_binding_count);

    for mesh_index in 0..mesh_binding_count {
        let relative = mesh_index
            .checked_mul(core::mem::size_of::<u64>())
            .ok_or(Error::SizeOverflow("Granny mesh-binding offset"))?;
        let binding_pointer_offset = mesh_bindings_ptr
            .checked_add(relative)
            .ok_or(Error::SizeOverflow("Granny mesh-binding offset"))?;

        let mut binding_cursor = Cursor::new(data_range(
            granny,
            binding_pointer_offset,
            8,
            "Granny mesh-binding pointer",
        )?);
        let mesh_offs = pointer_offset(binding_cursor.read_u64_le()?, "Granny mesh")?;
        let mesh_data = data_range(granny, mesh_offs, 0x3C, "Granny mesh")?;

        let mut name_cursor = Cursor::new(mesh_data);
        let name_ptr = pointer_offset(name_cursor.read_u64_le()?, "Granny mesh name")?;
        let name = data_tail(granny, name_ptr, "Granny mesh name").map_or_else(
            |_| format!("mesh_{mesh_index}"),
            read_null_terminated_string,
        );

        let mut bone_binding_cursor = Cursor::new(&mesh_data[0x30..]);
        let bone_binding_count = crate::checked_usize(
            u64::from(bone_binding_cursor.read_u32_le()?),
            "Granny bone-binding count",
        )?;
        let bone_bindings_ptr = pointer_offset(
            bone_binding_cursor.read_u64_le()?,
            "Granny bone-binding array",
        )?;

        let mut bone_bindings = Vec::with_capacity(bone_binding_count);

        for binding_index in 0..bone_binding_count {
            let relative = binding_index
                .checked_mul(GRANNY_BONE_BINDING_SIZE)
                .ok_or(Error::SizeOverflow("Granny bone-binding offset"))?;
            let binding_start = bone_bindings_ptr
                .checked_add(relative)
                .ok_or(Error::SizeOverflow("Granny bone-binding offset"))?;
            if let Some(binding) = parse_bone_binding(granny, binding_start)? {
                bone_bindings.push(binding);
            }
        }

        meshes.push(GrannyMesh {
            name,
            bone_bindings,
        });
    }

    Ok(meshes)
}
