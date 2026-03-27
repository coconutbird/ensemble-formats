//! Skeleton import from glTF skin data.

use ugx::{Bone, GrannyBone, Matrix4x4, Result};

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
            .unwrap_or_else(|| format!("bone_{}", joint_idx));

        // Find parent: look through all joint nodes to find one that has this node as a child
        let mut parent_index: i32 = -1;
        for (other_idx, other_node_idx) in skin.joints.iter().enumerate() {
            if other_idx == joint_idx {
                continue;
            }

            let other_node = &root.nodes[other_node_idx.value()];
            if let Some(ref children) = other_node.children
                && children.iter().any(|c| c.value() == joint_node_idx.value())
            {
                parent_index = other_idx as i32;
                break;
            }
        }

        // Read inverse bind matrix (16 floats)
        // Our export wrote DX row-major matrix rows flat into glTF column-major storage.
        // So on import we just read the 16 floats back as DX row-major.
        let ibm_offset = joint_idx * 16;
        let mut rows = [[0.0f32; 4]; 4];
        for r in 0..4 {
            for c in 0..4 {
                rows[r][c] = ibm_data[ibm_offset + r * 4 + c];
            }
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
            local_transform: bone_extras.local_transform,
            inverse_world_matrix: model_to_bone,
            lod_error: bone_extras.lod_error,
            extended_data: bone_extras.extended_data,
            extended_data_type: bone_extras.extended_data_type,
        });
    }

    Ok((bones, granny_bones))
}

/// Parsed bone extras from glTF node.
struct BoneExtras {
    extended_data: Option<ugx::GrannyVariant>,
    extended_data_type: Option<Vec<ugx::GrannyTypeMember>>,
    local_transform: Option<ugx::GrannyLocalTransform>,
    lod_error: f32,
}

impl Default for BoneExtras {
    fn default() -> Self {
        Self {
            extended_data: None,
            extended_data_type: None,
            local_transform: None,
            lod_error: 0.0,
        }
    }
}

/// Read bone Granny metadata from glTF node extras.
///
/// Reads extended data (type + variant), local_transform, and lod_error
/// written by our exporter.
fn read_bone_extras(extras: &gltf_json::Extras) -> BoneExtras {
    let raw = match extras.as_ref() {
        Some(raw) => raw,
        None => return BoneExtras::default(),
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

    // Local transform
    let local_transform = val
        .get("granny_local_transform")
        .and_then(parse_local_transform);

    // LOD error
    let lod_error = val
        .get("granny_lod_error")
        .and_then(|v| v.as_f64())
        .map(|v| v as f32)
        .unwrap_or(0.0);

    BoneExtras {
        extended_data,
        extended_data_type,
        local_transform,
        lod_error,
    }
}

/// Parse a `GrannyLocalTransform` from a JSON object.
fn parse_local_transform(val: &serde_json::Value) -> Option<ugx::GrannyLocalTransform> {
    let flags = val.get("flags")?.as_u64()? as u32;

    let pos_arr = val.get("position")?.as_array()?;
    if pos_arr.len() != 3 {
        return None;
    }

    let position = [
        pos_arr[0].as_f64()? as f32,
        pos_arr[1].as_f64()? as f32,
        pos_arr[2].as_f64()? as f32,
    ];

    let ori_arr = val.get("orientation")?.as_array()?;
    if ori_arr.len() != 4 {
        return None;
    }
    let orientation = [
        ori_arr[0].as_f64()? as f32,
        ori_arr[1].as_f64()? as f32,
        ori_arr[2].as_f64()? as f32,
        ori_arr[3].as_f64()? as f32,
    ];

    let ss_arr = val.get("scale_shear")?.as_array()?;
    if ss_arr.len() != 3 {
        return None;
    }
    let mut scale_shear = [[0.0f32; 3]; 3];
    for (r, row_val) in ss_arr.iter().enumerate() {
        let row = row_val.as_array()?;
        if row.len() != 3 {
            return None;
        }
        for (c, v) in row.iter().enumerate() {
            scale_shear[r][c] = v.as_f64()? as f32;
        }
    }

    Some(ugx::GrannyLocalTransform {
        flags,
        position,
        orientation,
        scale_shear,
    })
}
