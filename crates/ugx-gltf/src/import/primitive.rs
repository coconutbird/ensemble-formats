//! Mesh primitive import from glTF.

use gltf_json::mesh::{Mode, Semantic};
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
    if primitive.mode != Valid(Mode::Triangles) {
        return Err(Error::UnsupportedFormat(
            "UGX import supports only glTF TRIANGLES primitives".into(),
        ));
    }
    if primitive
        .targets
        .as_ref()
        .is_some_and(|targets| !targets.is_empty())
    {
        return Err(Error::UnsupportedFormat(
            "UGX does not support glTF morph targets".into(),
        ));
    }
    let attributes = read_attributes(primitive, root, buffer_bytes, has_skeleton)?;
    let indices = read_indices(primitive, root, buffer_bytes, attributes.vertex_count)?;
    if indices.len() % 3 != 0 {
        return Err(Error::UnsupportedFormat(format!(
            "Triangle primitive has {} indices, which is not divisible by three",
            indices.len()
        )));
    }
    if indices
        .iter()
        .any(|&index| usize::from(index) >= attributes.vertex_count)
    {
        return Err(Error::UnsupportedFormat(
            "Primitive index is outside its POSITION accessor".into(),
        ));
    }
    let vertices = build_vertices(&attributes, bone_count)?;
    let material_index = primitive.material.map_or(Ok(0), |material| {
        if material.value() >= root.materials.len() {
            return Err(Error::UnsupportedFormat(
                "Primitive material index is out of bounds".into(),
            ));
        }
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
    validate_attribute_accessor(position_accessor, &Semantic::Positions)?;
    let vertex_count = usize::try_from(position_accessor.count.0)
        .map_err(|_| Error::SizeOverflow("primitive vertex count"))?;
    let positions = read_accessor_f32(position_accessor, root, buffer_bytes)?;
    validate_attribute(&positions, vertex_count, 3, "POSITION")?;
    let normals = read_attribute(primitive, root, buffer_bytes, &Semantic::Normals)?
        .unwrap_or_else(|| [0.0, 1.0, 0.0].repeat(vertex_count));
    validate_attribute(&normals, vertex_count, 3, "NORMAL")?;
    let tangents = read_attribute(primitive, root, buffer_bytes, &Semantic::Tangents)?;
    if let Some(values) = &tangents {
        validate_attribute(values, vertex_count, 4, "TANGENT")?;
    }

    let mut uv_sets = Vec::new();
    for index in 0..MAX_UV {
        let semantic_index = u32::try_from(index)
            .map_err(|_| Error::SizeOverflow("texture-coordinate set index"))?;
        let Some(values) = read_attribute(
            primitive,
            root,
            buffer_bytes,
            &Semantic::TexCoords(semantic_index),
        )?
        else {
            break;
        };
        validate_attribute(&values, vertex_count, 2, "TEXCOORD")?;
        uv_sets.push(values);
    }

    let joints = has_skeleton
        .then(|| read_attribute(primitive, root, buffer_bytes, &Semantic::Joints(0)))
        .transpose()?
        .flatten();
    let weights = has_skeleton
        .then(|| read_attribute(primitive, root, buffer_bytes, &Semantic::Weights(0)))
        .transpose()?
        .flatten();
    match (&joints, &weights) {
        (Some(joints), Some(weights)) => {
            validate_attribute(joints, vertex_count, 4, "JOINTS_0")?;
            validate_attribute(weights, vertex_count, 4, "WEIGHTS_0")?;
        }
        (None, None) if !has_skeleton => {}
        _ => {
            return Err(Error::UnsupportedFormat(
                "Skinned primitive must contain both JOINTS_0 and WEIGHTS_0".into(),
            ));
        }
    }
    let colors = read_attribute(primitive, root, buffer_bytes, &Semantic::Colors(0))?;
    if let Some(values) = &colors {
        let vec3_length = checked_attribute_length(vertex_count, 3, "COLOR_0")?;
        let vec4_length = checked_attribute_length(vertex_count, 4, "COLOR_0")?;
        if values.len() != vec3_length && values.len() != vec4_length {
            return Err(Error::UnsupportedFormat(format!(
                "COLOR_0 accessor has {} values; expected {vec3_length} or {vec4_length}",
                values.len()
            )));
        }
        validate_finite(values, "COLOR_0")?;
    }

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

fn validate_attribute(
    values: &[f32],
    vertex_count: usize,
    component_count: usize,
    semantic: &'static str,
) -> Result<()> {
    let expected = checked_attribute_length(vertex_count, component_count, semantic)?;
    if values.len() != expected {
        return Err(Error::UnsupportedFormat(format!(
            "{semantic} accessor has {} values; expected {expected}",
            values.len()
        )));
    }
    validate_finite(values, semantic)
}

fn checked_attribute_length(
    vertex_count: usize,
    component_count: usize,
    semantic: &'static str,
) -> Result<usize> {
    vertex_count
        .checked_mul(component_count)
        .ok_or(Error::SizeOverflow(semantic))
}

fn validate_finite(values: &[f32], semantic: &'static str) -> Result<()> {
    if values.iter().any(|value| !value.is_finite()) {
        return Err(Error::UnsupportedFormat(format!(
            "{semantic} accessor contains a non-finite value"
        )));
    }
    Ok(())
}

fn read_attribute(
    primitive: &gltf_json::mesh::Primitive,
    root: &gltf_json::Root,
    buffer_bytes: &[u8],
    semantic: &Semantic,
) -> Result<Option<Vec<f32>>> {
    let Some(accessor_index) = primitive.attributes.get(&Valid(semantic.clone())) else {
        return Ok(None);
    };
    let accessor = root.accessors.get(accessor_index.value()).ok_or_else(|| {
        Error::UnsupportedFormat("Primitive accessor index is out of bounds".into())
    })?;
    validate_attribute_accessor(accessor, semantic)?;
    read_accessor_f32(accessor, root, buffer_bytes).map(Some)
}

fn validate_attribute_accessor(accessor: &gltf_json::Accessor, semantic: &Semantic) -> Result<()> {
    use gltf_json::accessor::ComponentType::{F32, U8, U16};
    let component_type = accessor_component_type(accessor)?;
    let valid = match semantic {
        Semantic::Positions | Semantic::Normals | Semantic::Tangents => {
            component_type == F32 && !accessor.normalized
        }
        Semantic::TexCoords(_) | Semantic::Colors(_) => {
            (component_type == F32 && !accessor.normalized)
                || (matches!(component_type, U8 | U16) && accessor.normalized)
        }
        Semantic::Joints(_) => matches!(component_type, U8 | U16) && !accessor.normalized,
        Semantic::Weights(_) => {
            (component_type == F32 && !accessor.normalized)
                || (matches!(component_type, U8 | U16) && accessor.normalized)
        }
        Semantic::Extras(_) => false,
    };
    if !valid {
        return Err(Error::UnsupportedFormat(format!(
            "Unsupported component type or normalization for {semantic:?}"
        )));
    }
    Ok(())
}

fn accessor_component_type(
    accessor: &gltf_json::Accessor,
) -> Result<gltf_json::accessor::ComponentType> {
    match accessor.component_type {
        Valid(gltf_json::accessor::GenericComponentType(component_type)) => Ok(component_type),
        gltf_json::validation::Checked::Invalid => {
            Err(Error::UnsupportedFormat("Invalid component type".into()))
        }
    }
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
        validate_index_accessor(accessor)?;
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

fn validate_index_accessor(accessor: &gltf_json::Accessor) -> Result<()> {
    use gltf_json::accessor::{ComponentType, Type};
    let is_scalar = accessor.type_ == Valid(Type::Scalar);
    let valid_component = matches!(
        accessor_component_type(accessor)?,
        ComponentType::U8 | ComponentType::U16 | ComponentType::U32
    );
    if !is_scalar || !valid_component || accessor.normalized {
        return Err(Error::UnsupportedFormat(
            "Index accessor must use unnormalized unsigned scalar values".into(),
        ));
    }
    Ok(())
}

fn build_vertices(
    attributes: &PrimitiveAttributes,
    bone_count: usize,
) -> Result<Vec<UnpackedVertex>> {
    let max_bone_index = u16::try_from(bone_count.saturating_sub(1))
        .map_err(|_| Error::SizeOverflow("primitive bone count"))?;
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
    let mut bone_weights = components_at::<4>(weights, index, "WEIGHTS_0")?;
    if bone_weights
        .iter()
        .any(|weight| !weight.is_finite() || *weight < 0.0)
    {
        return Err(Error::UnsupportedFormat(
            "Skin weights must be finite and non-negative".into(),
        ));
    }
    let weight_sum = bone_weights.iter().sum::<f32>();
    if !weight_sum.is_finite() || weight_sum <= f32::EPSILON {
        return Err(Error::UnsupportedFormat(
            "Skinned vertex has no positive bone weight".into(),
        ));
    }
    for weight in &mut bone_weights {
        *weight /= weight_sum;
    }
    let mut converted = [0; 4];
    for (output, value) in converted.iter_mut().zip(joint_values) {
        *output = checked_u16_float(value, "joint index")?;
        if *output > max_bone_index {
            return Err(Error::UnsupportedFormat(format!(
                "Joint index {} exceeds the skin's maximum index {max_bone_index}",
                *output
            )));
        }
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
