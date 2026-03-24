//! Skeleton import from glTF skin data.

use ugx::{Bone, GrannyBone, Matrix4x4, Result};

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

        granny_bones.push(GrannyBone {
            name,
            parent_index,
            inverse_world_matrix: model_to_bone,
        });
    }

    Ok((bones, granny_bones))
}
