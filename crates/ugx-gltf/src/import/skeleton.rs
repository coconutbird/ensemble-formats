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

    let skin = &root.skins[0];
    let joint_count = skin.joints.len();
    if joint_count == 0 {
        return Ok((Vec::new(), Vec::new()));
    }

    // Read inverse bind matrices
    let ibm_data = if let Some(ref ibm_accessor) = skin.inverse_bind_matrices {
        let acc = &root.accessors[ibm_accessor.value()];
        read_accessor_f32(acc, root, buffer_bytes)?
    } else {
        // Default to identity matrices
        let mut data = Vec::with_capacity(joint_count * 16);
        for _ in 0..joint_count {
            data.extend_from_slice(&[
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ]);
        }
        data
    };

    // Build a map from node index → joint index
    let mut node_to_joint: std::collections::HashMap<usize, usize> =
        std::collections::HashMap::new();
    for (joint_idx, joint_node) in skin.joints.iter().enumerate() {
        node_to_joint.insert(joint_node.value(), joint_idx);
    }

    // Build bones from joint nodes
    let mut bones = Vec::with_capacity(joint_count);
    let mut granny_bones = Vec::with_capacity(joint_count);

    for (joint_idx, joint_node_idx) in skin.joints.iter().enumerate() {
        let node = &root.nodes[joint_node_idx.value()];

        let name = node
            .name
            .clone()
            .unwrap_or_else(|| format!("bone_{joint_idx}"));

        let parent_index = find_parent_index(root, skin, joint_idx, joint_node_idx.value())?;

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

fn find_parent_index(
    root: &gltf_json::Root,
    skin: &gltf_json::Skin,
    joint_index: usize,
    joint_node_index: usize,
) -> Result<i32> {
    for (candidate_index, candidate_node_index) in skin.joints.iter().enumerate() {
        if candidate_index == joint_index {
            continue;
        }
        let candidate = &root.nodes[candidate_node_index.value()];
        if candidate.children.as_ref().is_some_and(|children| {
            children
                .iter()
                .any(|child| child.value() == joint_node_index)
        }) {
            return i32::try_from(candidate_index)
                .map_err(|_| Error::SizeOverflow("skeleton parent index"));
        }
    }
    Ok(-1)
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
