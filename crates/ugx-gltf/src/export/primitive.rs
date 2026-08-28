//! Mesh primitive construction for glTF export.
//!
//! Converts UGX vertex/index data into glTF accessors, buffer views, and primitives.

use gltf_json as json;
use json::validation::Checked::Valid;
use ugx::{Error, MAX_UV, Result, UnpackedVertex};

/// Source data and format metadata for one exported primitive.
pub(crate) struct PrimitiveInput<'a> {
    pub vertices: &'a [UnpackedVertex],
    pub indices: &'a [u16],
    pub material_index: i32,
    pub has_materials: bool,
    pub has_skeleton: bool,
    pub bone_count: usize,
    pub rigid_bone_index: i32,
    pub bone_remap: &'a [u8],
}

/// Mutable glTF collections populated while primitives are exported.
pub(crate) struct PrimitiveOutput<'a> {
    pub buffer_data: &'a mut Vec<u8>,
    pub accessors: &'a mut Vec<json::Accessor>,
    pub buffer_views: &'a mut Vec<json::buffer::View>,
}

struct SkinData {
    joints: Vec<[u16; 4]>,
    weights: Vec<[f32; 4]>,
    use_u16_joints: bool,
}

/// Create a mesh primitive from vertices and indices.
///
/// # Errors
///
/// Returns an error when a generated glTF index, size, or bone index cannot
/// be represented by its destination type.
pub(crate) fn create_primitive(
    input: &PrimitiveInput<'_>,
    output: &mut PrimitiveOutput<'_>,
) -> Result<json::mesh::Primitive> {
    let mut attributes = std::collections::BTreeMap::new();
    let (minimum, maximum) = position_bounds(input.vertices);
    let positions = input.vertices.iter().map(|vertex| vertex.position);
    let position_accessor = append_f32_accessor(
        output,
        positions,
        input.vertices.len(),
        json::accessor::Type::Vec3,
        Some((minimum.to_vec(), maximum.to_vec())),
    )?;
    attributes.insert(
        Valid(json::mesh::Semantic::Positions),
        json::Index::new(position_accessor),
    );

    let normals = input
        .vertices
        .iter()
        .map(|vertex| normalized_vec3(vertex.normal, [0.0, 1.0, 0.0]));
    insert_f32_attribute(
        output,
        &mut attributes,
        Valid(json::mesh::Semantic::Normals),
        normals,
        input.vertices.len(),
        json::accessor::Type::Vec3,
    )?;
    append_texcoords(input, output, &mut attributes)?;
    append_tangents(input, output, &mut attributes)?;
    append_skin(input, output, &mut attributes)?;
    append_colors(input, output, &mut attributes)?;

    let index_accessor = append_indices(output, input.indices)?;
    let material = if input.has_materials && input.material_index >= 0 {
        Some(json::Index::new(
            u32::try_from(input.material_index)
                .map_err(|_| Error::SizeOverflow("primitive material index"))?,
        ))
    } else {
        None
    };
    Ok(json::mesh::Primitive {
        attributes,
        extensions: None,
        extras: json::Extras::default(),
        indices: Some(json::Index::new(index_accessor)),
        material,
        mode: Valid(json::mesh::Mode::Triangles),
        targets: None,
    })
}

fn position_bounds(vertices: &[UnpackedVertex]) -> ([f32; 3], [f32; 3]) {
    let mut minimum = [f32::MAX; 3];
    let mut maximum = [f32::MIN; 3];
    for vertex in vertices {
        for ((minimum, maximum), value) in minimum.iter_mut().zip(&mut maximum).zip(vertex.position)
        {
            *minimum = minimum.min(value);
            *maximum = maximum.max(value);
        }
    }
    (minimum, maximum)
}

fn normalized_vec3(value: [f32; 3], fallback: [f32; 3]) -> [f32; 3] {
    let length_squared = value
        .iter()
        .map(|component| component * component)
        .sum::<f32>();
    if length_squared < 1.0e-12 {
        fallback
    } else {
        let inverse_length = length_squared.sqrt().recip();
        value.map(|component| component * inverse_length)
    }
}

fn append_texcoords(
    input: &PrimitiveInput<'_>,
    output: &mut PrimitiveOutput<'_>,
    attributes: &mut std::collections::BTreeMap<
        json::validation::Checked<json::mesh::Semantic>,
        json::Index<json::Accessor>,
    >,
) -> Result<()> {
    let count = input
        .vertices
        .iter()
        .map(|vertex| vertex.num_texcoords)
        .max()
        .unwrap_or(0)
        .min(MAX_UV);
    for set_index in 0..count {
        let values = input
            .vertices
            .iter()
            .map(|vertex| vertex.texcoords[set_index]);
        let semantic_index = u32::try_from(set_index)
            .map_err(|_| Error::SizeOverflow("texture-coordinate set index"))?;
        insert_f32_attribute(
            output,
            attributes,
            Valid(json::mesh::Semantic::TexCoords(semantic_index)),
            values,
            input.vertices.len(),
            json::accessor::Type::Vec2,
        )?;
    }
    Ok(())
}

fn append_tangents(
    input: &PrimitiveInput<'_>,
    output: &mut PrimitiveOutput<'_>,
    attributes: &mut std::collections::BTreeMap<
        json::validation::Checked<json::mesh::Semantic>,
        json::Index<json::Accessor>,
    >,
) -> Result<()> {
    let has_tangents = input.vertices.iter().any(|vertex| {
        vertex.tangent[..3]
            .iter()
            .any(|value| value.to_bits() & 0x7fff_ffff != 0)
    });
    if !has_tangents {
        return Ok(());
    }
    let values = input.vertices.iter().map(|vertex| {
        let xyz = normalized_vec3(
            [vertex.tangent[0], vertex.tangent[1], vertex.tangent[2]],
            [1.0, 0.0, 0.0],
        );
        let handedness = if vertex.tangent[3].abs() <= f32::EPSILON {
            1.0
        } else {
            vertex.tangent[3]
        };
        [xyz[0], xyz[1], xyz[2], handedness]
    });
    insert_f32_attribute(
        output,
        attributes,
        Valid(json::mesh::Semantic::Tangents),
        values,
        input.vertices.len(),
        json::accessor::Type::Vec4,
    )
}

fn append_skin(
    input: &PrimitiveInput<'_>,
    output: &mut PrimitiveOutput<'_>,
    attributes: &mut std::collections::BTreeMap<
        json::validation::Checked<json::mesh::Semantic>,
        json::Index<json::Accessor>,
    >,
) -> Result<()> {
    if !input.has_skeleton || input.bone_count == 0 {
        return Ok(());
    }
    let skin = build_skin_data(input)?;
    let joints_accessor = append_joint_accessor(output, &skin)?;
    attributes.insert(
        Valid(json::mesh::Semantic::Joints(0)),
        json::Index::new(joints_accessor),
    );
    let weights_accessor = append_f32_accessor(
        output,
        skin.weights,
        input.vertices.len(),
        json::accessor::Type::Vec4,
        None,
    )?;
    attributes.insert(
        Valid(json::mesh::Semantic::Weights(0)),
        json::Index::new(weights_accessor),
    );
    Ok(())
}

fn build_skin_data(input: &PrimitiveInput<'_>) -> Result<SkinData> {
    let max_bone_index =
        u16::try_from(input.bone_count).map_err(|_| Error::SizeOverflow("primitive bone count"))?;
    let rigid_index = usize::try_from(input.rigid_bone_index)
        .ok()
        .filter(|&index| index < input.bone_count)
        .map_or(Ok(0), |index| {
            u16::try_from(index).map_err(|_| Error::SizeOverflow("rigid bone index"))
        })?;
    let mut joints = Vec::with_capacity(input.vertices.len());
    let mut weights = Vec::with_capacity(input.vertices.len());
    for vertex in input.vertices {
        let weight_sum = vertex.bone_weights.iter().sum::<f32>();
        let is_rigid = weight_sum.abs() <= f32::EPSILON;
        joints.push(resolve_joints(
            vertex,
            input.bone_remap,
            max_bone_index,
            rigid_index,
            is_rigid,
        ));
        let mut normalized_weights = vertex.bone_weights;
        if is_rigid {
            normalized_weights[0] = 1.0;
        } else if (weight_sum - 1.0).abs() > 0.001 {
            for weight in &mut normalized_weights {
                *weight /= weight_sum;
            }
        }
        weights.push(normalized_weights);
    }
    Ok(SkinData {
        joints,
        weights,
        use_u16_joints: input.bone_count >= 256,
    })
}

fn resolve_joints(
    vertex: &UnpackedVertex,
    bone_remap: &[u8],
    max_bone_index: u16,
    rigid_index: u16,
    is_rigid: bool,
) -> [u16; 4] {
    if is_rigid {
        return [rigid_index, 0, 0, 0];
    }
    let mut indices = vertex.bone_indices;
    for index in &mut indices {
        if !bone_remap.is_empty() {
            *index = bone_remap
                .get(usize::from(*index))
                .copied()
                .map_or(0, u16::from);
        }
        if *index > max_bone_index {
            *index = 0;
        }
    }
    indices
}

fn append_colors(
    input: &PrimitiveInput<'_>,
    output: &mut PrimitiveOutput<'_>,
    attributes: &mut std::collections::BTreeMap<
        json::validation::Checked<json::mesh::Semantic>,
        json::Index<json::Accessor>,
    >,
) -> Result<()> {
    let has_colors = input.vertices.iter().any(|vertex| {
        vertex
            .diffuse
            .iter()
            .any(|value| value.to_bits() & 0x7fff_ffff != 0)
    });
    if !has_colors {
        return Ok(());
    }
    insert_f32_attribute(
        output,
        attributes,
        Valid(json::mesh::Semantic::Colors(0)),
        input.vertices.iter().map(|vertex| vertex.diffuse),
        input.vertices.len(),
        json::accessor::Type::Vec4,
    )
}

fn insert_f32_attribute<const N: usize>(
    output: &mut PrimitiveOutput<'_>,
    attributes: &mut std::collections::BTreeMap<
        json::validation::Checked<json::mesh::Semantic>,
        json::Index<json::Accessor>,
    >,
    semantic: json::validation::Checked<json::mesh::Semantic>,
    values: impl IntoIterator<Item = [f32; N]>,
    count: usize,
    accessor_type: json::accessor::Type,
) -> Result<()> {
    let accessor = append_f32_accessor(output, values, count, accessor_type, None)?;
    attributes.insert(semantic, json::Index::new(accessor));
    Ok(())
}

fn append_f32_accessor<const N: usize>(
    output: &mut PrimitiveOutput<'_>,
    values: impl IntoIterator<Item = [f32; N]>,
    count: usize,
    accessor_type: json::accessor::Type,
    bounds: Option<(Vec<f32>, Vec<f32>)>,
) -> Result<u32> {
    align_buffer(output.buffer_data, 4);
    let byte_offset = output.buffer_data.len();
    for value in values {
        for component in value {
            output
                .buffer_data
                .extend_from_slice(&component.to_le_bytes());
        }
    }
    let byte_length = output.buffer_data.len() - byte_offset;
    let stride = N
        .checked_mul(4)
        .ok_or(Error::SizeOverflow("attribute byte stride"))?;
    let view_index = append_view(
        output,
        byte_offset,
        byte_length,
        Some(stride),
        json::buffer::Target::ArrayBuffer,
    )?;
    append_accessor(
        output,
        view_index,
        count,
        json::accessor::ComponentType::F32,
        accessor_type,
        bounds,
        false,
    )
}

fn append_joint_accessor(output: &mut PrimitiveOutput<'_>, skin: &SkinData) -> Result<u32> {
    let alignment = if skin.use_u16_joints { 2 } else { 1 };
    align_buffer(output.buffer_data, alignment);
    let byte_offset = output.buffer_data.len();
    for joints in &skin.joints {
        for &joint in joints {
            if skin.use_u16_joints {
                output.buffer_data.extend_from_slice(&joint.to_le_bytes());
            } else {
                output
                    .buffer_data
                    .push(u8::try_from(joint).map_err(|_| Error::SizeOverflow("u8 joint index"))?);
            }
        }
    }
    let byte_length = output.buffer_data.len() - byte_offset;
    let (stride, component_type) = if skin.use_u16_joints {
        (8, json::accessor::ComponentType::U16)
    } else {
        (4, json::accessor::ComponentType::U8)
    };
    let view_index = append_view(
        output,
        byte_offset,
        byte_length,
        Some(stride),
        json::buffer::Target::ArrayBuffer,
    )?;
    append_accessor(
        output,
        view_index,
        skin.joints.len(),
        component_type,
        json::accessor::Type::Vec4,
        None,
        false,
    )
}

fn append_indices(output: &mut PrimitiveOutput<'_>, indices: &[u16]) -> Result<u32> {
    align_buffer(output.buffer_data, 2);
    let byte_offset = output.buffer_data.len();
    for index in indices {
        output.buffer_data.extend_from_slice(&index.to_le_bytes());
    }
    let byte_length = output.buffer_data.len() - byte_offset;
    let view_index = append_view(
        output,
        byte_offset,
        byte_length,
        None,
        json::buffer::Target::ElementArrayBuffer,
    )?;
    append_accessor(
        output,
        view_index,
        indices.len(),
        json::accessor::ComponentType::U16,
        json::accessor::Type::Scalar,
        None,
        false,
    )
}

fn append_view(
    output: &mut PrimitiveOutput<'_>,
    byte_offset: usize,
    byte_length: usize,
    byte_stride: Option<usize>,
    target: json::buffer::Target,
) -> Result<u32> {
    let index = checked_u32(output.buffer_views.len(), "buffer-view index")?;
    output.buffer_views.push(json::buffer::View {
        buffer: json::Index::new(0),
        byte_length: json::validation::USize64(checked_u64(byte_length, "buffer-view length")?),
        byte_offset: Some(json::validation::USize64(checked_u64(
            byte_offset,
            "buffer-view offset",
        )?)),
        byte_stride: byte_stride.map(json::buffer::Stride),
        extensions: None,
        extras: json::Extras::default(),
        name: None,
        target: Some(Valid(target)),
    });
    Ok(index)
}

fn append_accessor(
    output: &mut PrimitiveOutput<'_>,
    view_index: u32,
    count: usize,
    component_type: json::accessor::ComponentType,
    accessor_type: json::accessor::Type,
    bounds: Option<(Vec<f32>, Vec<f32>)>,
    normalized: bool,
) -> Result<u32> {
    let index = checked_u32(output.accessors.len(), "accessor index")?;
    let (minimum, maximum) = bounds.map_or((None, None), |(minimum, maximum)| {
        (
            Some(json::Value::from(minimum)),
            Some(json::Value::from(maximum)),
        )
    });
    output.accessors.push(json::Accessor {
        buffer_view: Some(json::Index::new(view_index)),
        byte_offset: Some(json::validation::USize64(0)),
        count: json::validation::USize64(checked_u64(count, "accessor count")?),
        component_type: Valid(json::accessor::GenericComponentType(component_type)),
        extensions: None,
        extras: json::Extras::default(),
        type_: Valid(accessor_type),
        min: minimum,
        max: maximum,
        name: None,
        normalized,
        sparse: None,
    });
    Ok(index)
}

fn align_buffer(buffer: &mut Vec<u8>, alignment: usize) {
    while !buffer.len().is_multiple_of(alignment) {
        buffer.push(0);
    }
}

fn checked_u32(value: usize, context: &'static str) -> Result<u32> {
    u32::try_from(value).map_err(|_| Error::SizeOverflow(context))
}

fn checked_u64(value: usize, context: &'static str) -> Result<u64> {
    u64::try_from(value).map_err(|_| Error::SizeOverflow(context))
}
