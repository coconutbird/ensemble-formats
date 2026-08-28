//! Mesh primitive import from glTF.

use gltf_json::mesh::Semantic;
use gltf_json::validation::Checked::Valid;
use num_traits::ToPrimitive;
use ugx::{Error, MAX_UV, Result, UnpackedVertex};

use super::accessor::read_accessor_f32;

struct PrimitiveAttributes {
    positions: Vec<f32>,
    normals: Vec<f32>,
    tangents: Option<Vec<f32>>,
    uv_sets: Vec<Vec<f32>>,
    joints: Option<Vec<f32>>,
    weights: Option<Vec<f32>>,
    colors: Option<Vec<f32>>,
    vertex_count: usize,
}

/// Import a single mesh primitive.
pub(crate) fn import_primitive(
    primitive: &gltf_json::mesh::Primitive,
    root: &gltf_json::Root,
    buffer_bytes: &[u8],
    has_skeleton: bool,
    bone_count: usize,
) -> Result<(Vec<UnpackedVertex>, Vec<u16>, i32)> {
    let attributes = read_attributes(primitive, root, buffer_bytes, has_skeleton)?;
    let indices = read_indices(primitive, root, buffer_bytes, attributes.vertex_count)?;
    let vertices = build_vertices(&attributes, bone_count)?;
    let material_index = primitive.material.map_or(Ok(-1), |material| {
        i32::try_from(material.value()).map_err(|_| Error::SizeOverflow("glTF material index"))
    })?;
    Ok((vertices, indices, material_index))
}

fn read_attributes(
    primitive: &gltf_json::mesh::Primitive,
    root: &gltf_json::Root,
    buffer_bytes: &[u8],
    has_skeleton: bool,
) -> Result<PrimitiveAttributes> {
    let position_index = primitive
        .attributes
        .get(&Valid(Semantic::Positions))
        .ok_or_else(|| Error::UnsupportedFormat("Mesh primitive missing POSITION".into()))?;
    let position_accessor = root.accessors.get(position_index.value()).ok_or_else(|| {
        Error::UnsupportedFormat("POSITION accessor index is out of bounds".into())
    })?;
    let vertex_count = usize::try_from(position_accessor.count.0)
        .map_err(|_| Error::SizeOverflow("primitive vertex count"))?;
    let positions = read_accessor_f32(position_accessor, root, buffer_bytes)?;
    let normals = read_attribute(primitive, root, buffer_bytes, Semantic::Normals)?
        .unwrap_or_else(|| [0.0, 1.0, 0.0].repeat(vertex_count));
    let tangents = read_attribute(primitive, root, buffer_bytes, Semantic::Tangents)?;

    let mut uv_sets = Vec::new();
    for index in 0..MAX_UV {
        let semantic_index = u32::try_from(index)
            .map_err(|_| Error::SizeOverflow("texture-coordinate set index"))?;
        let Some(values) = read_attribute(
            primitive,
            root,
            buffer_bytes,
            Semantic::TexCoords(semantic_index),
        )?
        else {
            break;
        };
        uv_sets.push(values);
    }

    let joints = has_skeleton
        .then(|| read_attribute(primitive, root, buffer_bytes, Semantic::Joints(0)))
        .transpose()?
        .flatten();
    let weights = has_skeleton
        .then(|| read_attribute(primitive, root, buffer_bytes, Semantic::Weights(0)))
        .transpose()?
        .flatten();
    let colors = read_attribute(primitive, root, buffer_bytes, Semantic::Colors(0))?;

    Ok(PrimitiveAttributes {
        positions,
        normals,
        tangents,
        uv_sets,
        joints,
        weights,
        colors,
        vertex_count,
    })
}

fn read_attribute(
    primitive: &gltf_json::mesh::Primitive,
    root: &gltf_json::Root,
    buffer_bytes: &[u8],
    semantic: Semantic,
) -> Result<Option<Vec<f32>>> {
    let Some(accessor_index) = primitive.attributes.get(&Valid(semantic)) else {
        return Ok(None);
    };
    let accessor = root.accessors.get(accessor_index.value()).ok_or_else(|| {
        Error::UnsupportedFormat("Primitive accessor index is out of bounds".into())
    })?;
    read_accessor_f32(accessor, root, buffer_bytes).map(Some)
}

fn read_indices(
    primitive: &gltf_json::mesh::Primitive,
    root: &gltf_json::Root,
    buffer_bytes: &[u8],
    vertex_count: usize,
) -> Result<Vec<u16>> {
    if let Some(index_accessor) = primitive.indices {
        let accessor = root
            .accessors
            .get(index_accessor.value())
            .ok_or_else(|| Error::UnsupportedFormat("Index accessor is out of bounds".into()))?;
        return read_accessor_f32(accessor, root, buffer_bytes)?
            .into_iter()
            .map(|value| checked_u16_float(value, "primitive index"))
            .collect();
    }
    (0..vertex_count)
        .map(|index| {
            u16::try_from(index).map_err(|_| Error::SizeOverflow("sequential primitive index"))
        })
        .collect()
}

fn build_vertices(
    attributes: &PrimitiveAttributes,
    bone_count: usize,
) -> Result<Vec<UnpackedVertex>> {
    let max_bone_index =
        u16::try_from(bone_count).map_err(|_| Error::SizeOverflow("primitive bone count"))?;
    (0..attributes.vertex_count)
        .map(|index| build_vertex(attributes, index, max_bone_index))
        .collect()
}

fn build_vertex(
    attributes: &PrimitiveAttributes,
    index: usize,
    max_bone_index: u16,
) -> Result<UnpackedVertex> {
    let position = components_at::<3>(&attributes.positions, index, "POSITION")?;
    let normal = components_at::<3>(&attributes.normals, index, "NORMAL")?;
    let tangent = attributes
        .tangents
        .as_ref()
        .map_or(Ok([0.0; 4]), |values| {
            components_at::<4>(values, index, "TANGENT")
        })?;

    let mut texcoords = [[0.0; 2]; MAX_UV];
    for (destination, values) in texcoords.iter_mut().zip(&attributes.uv_sets) {
        *destination = components_at::<2>(values, index, "TEXCOORD")?;
    }
    let (bone_indices, bone_weights) = read_skin(attributes, index, max_bone_index)?;
    let diffuse = read_color(attributes, index)?;

    Ok(UnpackedVertex {
        position,
        normal,
        tangent,
        texcoords,
        num_texcoords: attributes.uv_sets.len(),
        bone_indices,
        bone_weights,
        diffuse,
        ..Default::default()
    })
}

fn read_skin(
    attributes: &PrimitiveAttributes,
    index: usize,
    max_bone_index: u16,
) -> Result<([u16; 4], [f32; 4])> {
    let (Some(joints), Some(weights)) = (&attributes.joints, &attributes.weights) else {
        return Ok(([0; 4], [0.0; 4]));
    };
    let joint_values = components_at::<4>(joints, index, "JOINTS_0")?;
    let bone_weights = components_at::<4>(weights, index, "WEIGHTS_0")?;
    let mut converted = [0; 4];
    for (output, value) in converted.iter_mut().zip(joint_values) {
        *output = checked_u16_float(value, "joint index")?.min(max_bone_index);
    }
    let first_valid = converted
        .iter()
        .zip(bone_weights)
        .find_map(|(&bone, weight)| (weight > 0.0).then_some(bone))
        .unwrap_or(0);
    for (bone, weight) in converted.iter_mut().zip(bone_weights) {
        if weight <= 0.0 {
            *bone = first_valid;
        }
    }
    Ok((converted, bone_weights))
}

fn read_color(attributes: &PrimitiveAttributes, index: usize) -> Result<[f32; 4]> {
    let Some(colors) = &attributes.colors else {
        return Ok([0.0; 4]);
    };
    let vec4_length = attributes.vertex_count.checked_mul(4);
    if vec4_length == Some(colors.len()) {
        components_at::<4>(colors, index, "COLOR_0")
    } else {
        let color = components_at::<3>(colors, index, "COLOR_0")?;
        Ok([color[0], color[1], color[2], 1.0])
    }
}

fn components_at<const N: usize>(
    values: &[f32],
    index: usize,
    semantic: &'static str,
) -> Result<[f32; N]> {
    let start = index
        .checked_mul(N)
        .ok_or(Error::SizeOverflow("primitive attribute offset"))?;
    let end = start
        .checked_add(N)
        .ok_or(Error::SizeOverflow("primitive attribute range"))?;
    values
        .get(start..end)
        .ok_or_else(|| Error::UnsupportedFormat(format!("{semantic} accessor is too short")))?
        .try_into()
        .map_err(|_| Error::UnsupportedFormat(format!("Invalid {semantic} component count")))
}

fn checked_u16_float(value: f32, context: &'static str) -> Result<u16> {
    if !value.is_finite() || value.fract().abs() > f32::EPSILON {
        return Err(Error::UnsupportedFormat(format!(
            "{context} must be a finite integer, got {value}"
        )));
    }
    value.to_u16().ok_or(Error::SizeOverflow(context))
}
