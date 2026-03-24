//! Mesh primitive import from glTF.

use ugx::{Error, MAX_UV, Result, UnpackedVertex};

use super::accessor::read_accessor_f32;

/// Import a single mesh primitive.
pub(crate) fn import_primitive(
    primitive: &gltf_json::mesh::Primitive,
    root: &gltf_json::Root,
    buffer_bytes: &[u8],
    has_skeleton: bool,
    bone_count: usize,
) -> Result<(Vec<UnpackedVertex>, Vec<u16>, i32)> {
    use gltf_json::mesh::Semantic;
    use gltf_json::validation::Checked::Valid;

    // Read positions
    let (positions, vertex_count) =
        if let Some(acc_idx) = primitive.attributes.get(&Valid(Semantic::Positions)) {
            let acc = &root.accessors[acc_idx.value()];
            let count = acc.count.0 as usize;
            (read_accessor_f32(acc, root, buffer_bytes)?, count)
        } else {
            return Err(Error::UnsupportedFormat(
                "Mesh primitive missing POSITION".into(),
            ));
        };

    // Read normals
    let normals = if let Some(acc_idx) = primitive.attributes.get(&Valid(Semantic::Normals)) {
        let acc = &root.accessors[acc_idx.value()];
        read_accessor_f32(acc, root, buffer_bytes)?
    } else {
        [0.0, 1.0, 0.0].repeat(vertex_count)
    };

    // Read tangents
    let tangents = if let Some(acc_idx) = primitive.attributes.get(&Valid(Semantic::Tangents)) {
        let acc = &root.accessors[acc_idx.value()];
        Some(read_accessor_f32(acc, root, buffer_bytes)?)
    } else {
        None
    };

    // Read UV sets
    let mut uv_sets: Vec<Vec<f32>> = Vec::new();
    for i in 0..MAX_UV {
        if let Some(acc_idx) = primitive
            .attributes
            .get(&Valid(Semantic::TexCoords(i as u32)))
        {
            let acc = &root.accessors[acc_idx.value()];
            uv_sets.push(read_accessor_f32(acc, root, buffer_bytes)?);
        } else {
            break;
        }
    }

    // Read joints
    let joints = if has_skeleton {
        if let Some(acc_idx) = primitive.attributes.get(&Valid(Semantic::Joints(0))) {
            let acc = &root.accessors[acc_idx.value()];
            Some(read_accessor_f32(acc, root, buffer_bytes)?)
        } else {
            None
        }
    } else {
        None
    };

    // Read weights
    let weights = if has_skeleton {
        if let Some(acc_idx) = primitive.attributes.get(&Valid(Semantic::Weights(0))) {
            let acc = &root.accessors[acc_idx.value()];
            Some(read_accessor_f32(acc, root, buffer_bytes)?)
        } else {
            None
        }
    } else {
        None
    };

    // Read vertex colors (COLOR_0)
    let colors = if let Some(acc_idx) = primitive.attributes.get(&Valid(Semantic::Colors(0))) {
        let acc = &root.accessors[acc_idx.value()];
        Some(read_accessor_f32(acc, root, buffer_bytes)?)
    } else {
        None
    };

    // Read indices
    let indices = if let Some(ref idx_accessor) = primitive.indices {
        let acc = &root.accessors[idx_accessor.value()];
        let raw = read_accessor_f32(acc, root, buffer_bytes)?;
        raw.iter().map(|&v| v as u16).collect::<Vec<_>>()
    } else {
        // No index buffer — generate sequential indices
        (0..vertex_count as u16).collect()
    };

    // Build vertices
    let mut vertices = Vec::with_capacity(vertex_count);
    let max_bone_idx = if bone_count > 0 {
        (bone_count - 1) as u16
    } else {
        0
    };

    #[allow(clippy::field_reassign_with_default)]
    for i in 0..vertex_count {
        let mut vertex = UnpackedVertex::default();

        // Position
        vertex.position = [positions[i * 3], positions[i * 3 + 1], positions[i * 3 + 2]];

        // Normal
        vertex.normal = [normals[i * 3], normals[i * 3 + 1], normals[i * 3 + 2]];

        // Tangent
        if let Some(ref t) = tangents {
            vertex.tangent = [t[i * 4], t[i * 4 + 1], t[i * 4 + 2], t[i * 4 + 3]];
        }

        // UVs
        vertex.num_texcoords = uv_sets.len();
        for (uv_idx, uv_data) in uv_sets.iter().enumerate() {
            if uv_idx < MAX_UV {
                vertex.texcoords[uv_idx] = [uv_data[i * 2], uv_data[i * 2 + 1]];
            }
        }

        // Joints and weights
        if let (Some(j), Some(w)) = (&joints, &weights) {
            let mut bone_indices = [0u16; 4];
            let mut bone_weights = [0.0f32; 4];

            // First pass: find the first valid bone index for padding
            let mut first_valid_bone: u16 = 1; // Default to bone 1 if no valid bones
            for k in 0..4 {
                if w[i * 4 + k] > 0.0 {
                    let joint_0based = j[i * 4 + k] as u16;
                    // Clamp to valid range and convert to 1-based
                    first_valid_bone = (joint_0based.min(max_bone_idx) + 1).max(1);
                    break;
                }
            }

            // Second pass: set bone indices and weights
            for k in 0..4 {
                let joint_0based = j[i * 4 + k] as u16;
                bone_weights[k] = w[i * 4 + k];

                if bone_weights[k] > 0.0 {
                    // Convert 0-based glTF joint to 1-based UGX bone index
                    // Clamp to valid range to prevent out-of-bounds
                    bone_indices[k] = (joint_0based.min(max_bone_idx) + 1).max(1);
                } else {
                    // Use first valid bone for padding (game expects valid indices)
                    bone_indices[k] = first_valid_bone;
                }
            }
            vertex.bone_indices = bone_indices;
            vertex.bone_weights = bone_weights;
        }

        // Vertex colors
        if let Some(ref c) = colors {
            // COLOR_0 can be Vec3 or Vec4; handle both
            let stride = if c.len() == vertex_count * 4 { 4 } else { 3 };
            vertex.diffuse[0] = c[i * stride];
            vertex.diffuse[1] = c[i * stride + 1];
            vertex.diffuse[2] = c[i * stride + 2];
            vertex.diffuse[3] = if stride == 4 { c[i * stride + 3] } else { 1.0 };
        }

        vertices.push(vertex);
    }

    // Material index from primitive
    let material_index = primitive.material.map(|m| m.value() as i32).unwrap_or(-1);

    Ok((vertices, indices, material_index))
}
