//! Skeleton import from glTF skin data.

use num_traits::ToPrimitive;
use ugx::{Bone, Error, GrannyBone, Matrix4x4, Result};

use crate::granny_json::{json_to_type_members, json_to_variant};

use super::accessor::read_accessor_f32;

/// Import skeleton from glTF skin.
pub(crate) fn import_skeleton(
    root: &gltf_json::Root,
    buffer_bytes: &[u8],
) -> Result<(Vec<Bone>, Vec<GrannyBone>)> {
    if root.skins.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    if root.skins.len() > 1 {
        return Err(Error::UnsupportedFormat(
            "UGX supports only one glTF skin".into(),
        ));
    }

    let skin = &root.skins[0];
    let joint_count = skin.joints.len();
    if joint_count == 0 {
        return Ok((Vec::new(), Vec::new()));
    }

    let ibm_data = read_inverse_bind_matrices(root, buffer_bytes, skin)?;

    // Build a map from node index → joint index
    let mut node_to_joint: std::collections::HashMap<usize, usize> =
        std::collections::HashMap::new();
    for (joint_idx, joint_node) in skin.joints.iter().enumerate() {
        let node_index = joint_node.value();
        if node_index >= root.nodes.len() {
            return Err(Error::UnsupportedFormat(
                "Skin joint node index is out of bounds".into(),
            ));
        }
        if node_to_joint.insert(node_index, joint_idx).is_some() {
            return Err(Error::UnsupportedFormat(
                "Skin contains the same joint node more than once".into(),
            ));
        }
    }
    let parents = super::build_node_parents(root)?;

    // Build bones from joint nodes
    let mut bones = Vec::with_capacity(joint_count);
    let mut granny_bones = Vec::with_capacity(joint_count);

    for (joint_idx, joint_node_idx) in skin.joints.iter().enumerate() {
        let node_index = joint_node_idx.value();
        let node = root.nodes.get(node_index).ok_or_else(|| {
            Error::UnsupportedFormat("Skin joint node index is out of bounds".into())
        })?;

        let name = node
            .name
            .clone()
            .unwrap_or_else(|| format!("bone_{joint_idx}"));

        let parent_index = find_parent_index(&parents, &node_to_joint, node_index)?;

        // Read inverse bind matrix (16 floats)
        // Our export wrote DX row-major matrix rows flat into glTF column-major storage.
        // So on import we just read the 16 floats back as DX row-major.
        let matrices = ibm_data.as_chunks::<16>().0;
        let matrix = matrices.get(joint_idx).ok_or_else(|| {
            Error::UnsupportedFormat("Inverse-bind-matrix accessor is too short".into())
        })?;
        let mut rows = [[0.0f32; 4]; 4];
        for (row, values) in rows.iter_mut().zip(matrix.as_chunks::<4>().0) {
            *row = *values;
        }
        let model_to_bone = Matrix4x4 { rows };
        validate_inverse_bind_matrix(&model_to_bone)?;

        bones.push(Bone {
            name: name.clone(),
            parent_index,
            model_to_bone: model_to_bone.clone(),
        });

        // Restore Granny bone metadata from node extras if present
        let bone_extras = read_bone_extras(&node.extras);

        granny_bones.push(GrannyBone {
            name,
            parent_index,
            local_transform: None, // recomputed by the writer from inverse world matrices
            inverse_world_matrix: model_to_bone,
            lod_error: bone_extras.lod_error,
            extended_data: bone_extras.extended_data,
            extended_data_type: bone_extras.extended_data_type,
        });
    }

    Ok((bones, granny_bones))
}

fn read_inverse_bind_matrices(
    root: &gltf_json::Root,
    buffer_bytes: &[u8],
    skin: &gltf_json::Skin,
) -> Result<Vec<f32>> {
    if skin
        .skeleton
        .as_ref()
        .is_some_and(|node| node.value() >= root.nodes.len())
    {
        return Err(Error::UnsupportedFormat(
            "Skin skeleton node index is out of bounds".into(),
        ));
    }
    let matrix_value_count = skin
        .joints
        .len()
        .checked_mul(16)
        .ok_or(Error::SizeOverflow("inverse-bind-matrix value count"))?;
    let Some(ibm_accessor) = &skin.inverse_bind_matrices else {
        let mut data = Vec::with_capacity(matrix_value_count);
        for _ in &skin.joints {
            data.extend_from_slice(&[
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ]);
        }
        return Ok(data);
    };
    let accessor = root.accessors.get(ibm_accessor.value()).ok_or_else(|| {
        Error::UnsupportedFormat("Inverse-bind-matrix accessor is out of bounds".into())
    })?;
    validate_inverse_bind_accessor(accessor)?;
    let values = read_accessor_f32(accessor, root, buffer_bytes)?;
    if values.len() != matrix_value_count {
        return Err(Error::UnsupportedFormat(format!(
            "Inverse-bind-matrix accessor has {} values; expected {matrix_value_count}",
            values.len()
        )));
    }
    Ok(values)
}

fn validate_inverse_bind_accessor(accessor: &gltf_json::Accessor) -> Result<()> {
    use gltf_json::accessor::{ComponentType, Type};
    use gltf_json::validation::Checked::Valid;
    let component_type = match accessor.component_type {
        Valid(gltf_json::accessor::GenericComponentType(component_type)) => component_type,
        gltf_json::validation::Checked::Invalid => {
            return Err(Error::UnsupportedFormat(
                "Invalid inverse-bind-matrix component type".into(),
            ));
        }
    };
    if accessor.type_ != Valid(Type::Mat4)
        || component_type != ComponentType::F32
        || accessor.normalized
    {
        return Err(Error::UnsupportedFormat(
            "Inverse bind matrices must use unnormalized MAT4 floats".into(),
        ));
    }
    Ok(())
}

fn validate_inverse_bind_matrix(matrix: &Matrix4x4) -> Result<()> {
    let rows = &matrix.rows;
    if rows.iter().flatten().any(|value| !value.is_finite()) {
        return Err(Error::UnsupportedFormat(
            "Inverse bind matrix contains a non-finite value".into(),
        ));
    }
    let affine_tolerance = 1.0e-6;
    if rows[0][3].abs() > affine_tolerance
        || rows[1][3].abs() > affine_tolerance
        || rows[2][3].abs() > affine_tolerance
        || (rows[3][3] - 1.0).abs() > affine_tolerance
        || matrix.inverse().is_none()
    {
        return Err(Error::UnsupportedFormat(
            "Inverse bind matrix must be invertible and affine".into(),
        ));
    }
    Ok(())
}

fn find_parent_index(
    parents: &[Option<usize>],
    node_to_joint: &std::collections::HashMap<usize, usize>,
    joint_node_index: usize,
) -> Result<i32> {
    let mut parent = parents.get(joint_node_index).copied().flatten();
    for _ in 0..parents.len() {
        let Some(parent_index) = parent else {
            return Ok(-1);
        };
        if let Some(&joint_index) = node_to_joint.get(&parent_index) {
            return i32::try_from(joint_index)
                .map_err(|_| Error::SizeOverflow("skeleton parent index"));
        }
        parent = parents.get(parent_index).copied().flatten();
    }
    Err(Error::UnsupportedFormat(
        "glTF node hierarchy contains a cycle".into(),
    ))
}

/// Parsed bone extras from glTF node.
struct BoneExtras {
    extended_data: Option<ugx::GrannyVariant>,
    extended_data_type: Option<Vec<ugx::GrannyTypeMember>>,
    lod_error: f32,
}

impl Default for BoneExtras {
    fn default() -> Self {
        Self {
            extended_data: None,
            extended_data_type: None,
            lod_error: 0.0,
        }
    }
}

/// Read bone Granny metadata from glTF node extras.
///
/// Reads extended data (type + variant) and `lod_error` written by our
/// exporter. Local transforms are NOT stored in extras — they are
/// recomputed from inverse world matrices by the writer.
fn read_bone_extras(extras: &gltf_json::Extras) -> BoneExtras {
    let Some(raw) = extras.as_ref() else {
        return BoneExtras::default();
    };

    let val: serde_json::Value = match serde_json::from_str(raw.get()) {
        Ok(v) => v,
        Err(_) => return BoneExtras::default(),
    };

    // Extended data
    let (extended_data, extended_data_type) = if let (Some(type_val), Some(data_val)) =
        (val.get("granny_ext_type"), val.get("granny_ext_data"))
    {
        if let Some(type_members) = json_to_type_members(type_val) {
            let variant = json_to_variant(data_val, &type_members);
            (variant, Some(type_members))
        } else {
            (None, None)
        }
    } else {
        (None, None)
    };

    // LOD error
    let lod_error = val
        .get("granny_lod_error")
        .and_then(gltf_json::Value::as_f64)
        .and_then(|value| value.to_f32())
        .unwrap_or(0.0);

    BoneExtras {
        extended_data,
        extended_data_type,
        lod_error,
    }
}
