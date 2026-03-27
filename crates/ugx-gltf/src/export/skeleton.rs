//! Skeleton/bone node construction for glTF export.
//!
//! Handles building glTF node hierarchies and inverse bind matrices from
//! both cached data bones (0x700 chunk) and granny bones (0x703 chunk).

use gltf_json as json;
use json::validation::Checked::Valid;

use crate::granny_json::{type_members_to_json, variant_to_json};
use ugx::{Bone, GrannyBone, UgxGeom};

/// Build a mapping from section index to mesh index.
/// Analyzes which bones each section uses and matches against granny_mesh bone_bindings.
pub(crate) fn build_section_to_mesh_mapping(
    geom: &UgxGeom,
    granny_bones: &[GrannyBone],
) -> Vec<usize> {
    // If no granny_meshes, each section is its own mesh
    if geom.granny_meshes.is_empty() {
        return (0..geom.sections.len()).collect();
    }

    // Build a set of bone names for each granny_mesh
    let mesh_bone_sets: Vec<std::collections::HashSet<&str>> = geom
        .granny_meshes
        .iter()
        .map(|m| {
            m.bone_bindings
                .iter()
                .map(|b| b.bone_name.as_str())
                .collect()
        })
        .collect();

    // For each section, find which mesh it belongs to by matching bone usage
    let mut section_to_mesh = Vec::with_capacity(geom.sections.len());

    for section_idx in 0..geom.sections.len() {
        // Get bones used by this section's vertices
        let section_bones = get_section_bone_names(geom, section_idx, granny_bones);

        // Find the mesh whose bone_bindings best matches this section's bones
        let mesh_idx = find_best_matching_mesh(&section_bones, &mesh_bone_sets);
        section_to_mesh.push(mesh_idx);
    }

    section_to_mesh
}

/// Get the bone names used by a section's vertices.
fn get_section_bone_names(
    geom: &UgxGeom,
    section_idx: usize,
    granny_bones: &[GrannyBone],
) -> std::collections::HashSet<String> {
    let mut used_bones = std::collections::HashSet::new();

    // Try to unpack vertices; if that fails, fall back to rigid_bone_index
    if let Ok(vertices) = geom.unpack_section_vertices(section_idx) {
        for v in &vertices {
            for k in 0..4 {
                if v.bone_weights[k] > 0.0 {
                    let bone_idx = v.bone_indices[k] as usize;
                    if bone_idx > 0 && bone_idx <= granny_bones.len() {
                        // bone_indices are 1-based
                        used_bones.insert(granny_bones[bone_idx - 1].name.clone());
                    }
                }
            }
        }
    }

    // If no bones found from vertices (e.g., global_bones section), use rigid_bone_index
    if used_bones.is_empty() && section_idx < geom.sections.len() {
        let section = &geom.sections[section_idx];
        let rigid_idx = section.rigid_bone_index as usize;
        if rigid_idx > 0 && rigid_idx <= granny_bones.len() {
            used_bones.insert(granny_bones[rigid_idx - 1].name.clone());
        }
    }

    used_bones
}

/// Find the mesh index whose bone_bindings best matches the section's bones.
/// Prefers:
/// 1. Exact match (section bones == mesh bones)
/// 2. Smallest superset (mesh contains all section bones with fewest extras)
/// 3. Highest overlap if no complete containment
///
/// Returns the mesh index, or mesh count to create a new mesh if none match.
fn find_best_matching_mesh(
    section_bones: &std::collections::HashSet<String>,
    mesh_bone_sets: &[std::collections::HashSet<&str>],
) -> usize {
    if section_bones.is_empty() || mesh_bone_sets.is_empty() {
        return mesh_bone_sets.len(); // Fallback: create new mesh
    }

    let mut best_mesh = mesh_bone_sets.len();
    let mut best_score = (false, usize::MAX, 0usize); // (is_superset, mesh_size, overlap)

    for (mesh_idx, mesh_bones) in mesh_bone_sets.iter().enumerate() {
        // Count how many section bones are in this mesh's bone_bindings
        let overlap = section_bones
            .iter()
            .filter(|b| mesh_bones.contains(b.as_str()))
            .count();

        // Check if mesh contains ALL section bones (is a superset)
        let is_superset = overlap == section_bones.len();
        let mesh_size = mesh_bones.len();

        // Score: prefer superset, then smallest mesh, then highest overlap
        let score = (is_superset, mesh_size, overlap);

        // Better if: is superset when best isn't, OR both superset and smaller, OR more overlap
        let is_better = if is_superset && !best_score.0 {
            true // Superset beats non-superset
        } else if is_superset && best_score.0 {
            mesh_size < best_score.1 // Among supersets, prefer smaller
        } else if !is_superset && !best_score.0 {
            overlap > best_score.2 // Among non-supersets, prefer more overlap
        } else {
            false // Non-superset doesn't beat superset
        };

        if is_better {
            best_score = score;
            best_mesh = mesh_idx;
        }
    }

    // If no overlap found at all, return mesh count to create a new mesh
    if best_score.2 == 0 {
        mesh_bone_sets.len()
    } else {
        best_mesh
    }
}

/// Create skeleton nodes from cached data bones (0x700 chunk).
/// Fallback when granny bones aren't available.
/// Returns (bone_nodes, inverse_bind_matrices_accessor_index).
pub(crate) fn create_skeleton_nodes(
    bones: &[Bone],
    buffer_data: &mut Vec<u8>,
    accessors: &mut Vec<json::Accessor>,
    buffer_views: &mut Vec<json::buffer::View>,
) -> (Vec<json::Node>, u32) {
    let mut nodes = Vec::with_capacity(bones.len());

    // Build child lists for hierarchy
    let mut children_map: std::collections::HashMap<i32, Vec<u32>> =
        std::collections::HashMap::new();
    for (i, bone) in bones.iter().enumerate() {
        if bone.parent_index >= 0 {
            children_map
                .entry(bone.parent_index)
                .or_default()
                .push(i as u32);
        }
    }

    // Compute world transforms in DX row-major convention.
    // model_to_bone: model->bone (DX: v_bone = v_model * M)
    // Invert to get world transform (DX: v_model = v_bone * W_dx)
    let bone_world_dx: Vec<_> = bones
        .iter()
        .map(|b| {
            b.model_to_bone
                .inverse()
                .unwrap_or_else(ugx::Matrix4x4::identity)
        })
        .collect();

    // Create nodes with local transforms and parent-child hierarchy.
    for (i, bone) in bones.iter().enumerate() {
        let children = children_map.get(&(i as i32)).map(|c| {
            c.iter()
                .map(|&idx| json::Index::new(idx))
                .collect::<Vec<_>>()
        });

        // DX: local = world * parent_world^{-1}
        // Use model_to_bone[parent] directly (= parent_world^{-1}) to avoid
        // double-inversion precision loss.
        let local_dx = if bone.parent_index < 0 {
            bone_world_dx[i].clone()
        } else {
            let parent_idx = bone.parent_index as usize;
            bone_world_dx[i].multiply(&bones[parent_idx].model_to_bone)
        };

        // Write DX rows flat = column-major of GL matrix
        let m = &local_dx.rows;
        let gltf_matrix = [
            m[0][0], m[0][1], m[0][2], m[0][3], m[1][0], m[1][1], m[1][2], m[1][3], m[2][0],
            m[2][1], m[2][2], m[2][3], m[3][0], m[3][1], m[3][2], m[3][3],
        ];

        nodes.push(json::Node {
            camera: None,
            children: if children.as_ref().is_none_or(|c| c.is_empty()) {
                None
            } else {
                children
            },
            extensions: None,
            extras: json::Extras::default(),
            matrix: Some(gltf_matrix),
            mesh: None,
            name: Some(bone.name.clone()),
            rotation: None,
            scale: None,
            translation: None,
            skin: None,
            weights: None,
        });
    }

    // Write inverse bind matrices.
    // model_to_bone rows flat = column-major of GL IBM (same derivation as granny path).
    while !buffer_data.len().is_multiple_of(4) {
        buffer_data.push(0);
    }
    let ibm_view_idx = buffer_views.len() as u32;
    let ibm_offset = buffer_data.len();

    for bone in bones {
        let m = &bone.model_to_bone.rows;
        for row in m {
            for &val in row {
                buffer_data.extend_from_slice(&val.to_le_bytes());
            }
        }
    }

    let ibm_byte_length = buffer_data.len() - ibm_offset;

    buffer_views.push(json::buffer::View {
        buffer: json::Index::new(0),
        byte_length: json::validation::USize64(ibm_byte_length as u64),
        byte_offset: Some(json::validation::USize64(ibm_offset as u64)),
        byte_stride: None,
        extensions: None,
        extras: json::Extras::default(),
        name: None,
        target: None,
    });

    let ibm_accessor_idx = accessors.len() as u32;
    accessors.push(json::Accessor {
        buffer_view: Some(json::Index::new(ibm_view_idx)),
        byte_offset: Some(json::validation::USize64(0)),
        count: json::validation::USize64(bones.len() as u64),
        component_type: Valid(json::accessor::GenericComponentType(
            json::accessor::ComponentType::F32,
        )),
        extensions: None,
        extras: json::Extras::default(),
        type_: Valid(json::accessor::Type::Mat4),
        min: None,
        max: None,
        name: None,
        normalized: false,
        sparse: None,
    });

    (nodes, ibm_accessor_idx)
}

/// Create skeleton nodes from granny bones (0x703 chunk).
/// Uses hierarchical structure with local transforms.
/// Returns (bone_nodes, inverse_bind_matrices_accessor_index).
pub(crate) fn create_skeleton_nodes_from_granny(
    bones: &[GrannyBone],
    buffer_data: &mut Vec<u8>,
    accessors: &mut Vec<json::Accessor>,
    buffer_views: &mut Vec<json::buffer::View>,
) -> (Vec<json::Node>, u32) {
    let mut nodes = Vec::with_capacity(bones.len());

    // Build child lists for hierarchy
    let mut children_map: std::collections::HashMap<i32, Vec<u32>> =
        std::collections::HashMap::new();
    for (i, bone) in bones.iter().enumerate() {
        if bone.parent_index >= 0 {
            children_map
                .entry(bone.parent_index)
                .or_default()
                .push(i as u32);
        }
    }

    // Compute world transforms in DirectX row-major convention.
    // inverse_world_matrix: model->bone (DX: v_bone = v_model * IWM)
    // Invert to get: bone->model / world transform (DX: v_model = v_bone * W_dx)
    let bone_world_dx: Vec<_> = bones
        .iter()
        .map(|b| {
            b.inverse_world_matrix
                .inverse()
                .unwrap_or_else(ugx::Matrix4x4::identity)
        })
        .collect();

    // Create nodes with local transforms and parent-child hierarchy.
    for (i, bone) in bones.iter().enumerate() {
        let children = children_map.get(&(i as i32)).map(|c| {
            c.iter()
                .map(|&idx| json::Index::new(idx))
                .collect::<Vec<_>>()
        });

        // In DX row-vector convention: v_world = v_local * local_dx * parent_world_dx
        // So: world_dx = local_dx * parent_world_dx
        // Therefore: local_dx = world_dx * parent_world_dx^{-1}
        //
        // Optimization: parent_world_dx^{-1} = IWM[parent] (which we already have),
        // avoiding double-inversion precision loss from (IWM.inverse()).inverse().
        let local_dx = if bone.parent_index < 0 {
            bone_world_dx[i].clone()
        } else {
            let parent_idx = bone.parent_index as usize;
            bone_world_dx[i].multiply(&bones[parent_idx].inverse_world_matrix)
        };

        // glTF column-major storage of M_gl = row-major storage of M_dx
        // (because M_gl = M_dx^T, and column-major(M^T) = row-major(M))
        // So just write DX matrix rows flat.
        let m = &local_dx.rows;
        let gltf_matrix = [
            m[0][0], m[0][1], m[0][2], m[0][3], m[1][0], m[1][1], m[1][2], m[1][3], m[2][0],
            m[2][1], m[2][2], m[2][3], m[3][0], m[3][1], m[3][2], m[3][3],
        ];

        // Serialize extended data into node extras if present
        let extras = build_bone_extras(bone);

        nodes.push(json::Node {
            camera: None,
            children: if children.as_ref().is_none_or(|c| c.is_empty()) {
                None
            } else {
                children
            },
            extensions: None,
            extras,
            matrix: Some(gltf_matrix),
            mesh: None,
            name: Some(bone.name.clone()),
            rotation: None,
            scale: None,
            translation: None,
            skin: None,
            weights: None,
        });
    }

    // Write inverse bind matrices.
    // IWM is the model->bone transform in DX convention.
    // glTF IBM in GL convention = IWM^T.
    // column-major(IWM^T) = row-major(IWM), so just write IWM rows flat.
    while !buffer_data.len().is_multiple_of(4) {
        buffer_data.push(0);
    }
    let ibm_view_idx = buffer_views.len() as u32;
    let ibm_offset = buffer_data.len();

    for bone in bones {
        let m = &bone.inverse_world_matrix.rows;
        for row in m {
            for &val in row {
                buffer_data.extend_from_slice(&val.to_le_bytes());
            }
        }
    }

    let ibm_byte_length = buffer_data.len() - ibm_offset;

    buffer_views.push(json::buffer::View {
        buffer: json::Index::new(0),
        byte_length: json::validation::USize64(ibm_byte_length as u64),
        byte_offset: Some(json::validation::USize64(ibm_offset as u64)),
        byte_stride: None,
        extensions: None,
        extras: json::Extras::default(),
        name: None,
        target: None,
    });

    let ibm_accessor_idx = accessors.len() as u32;
    accessors.push(json::Accessor {
        buffer_view: Some(json::Index::new(ibm_view_idx)),
        byte_offset: Some(json::validation::USize64(0)),
        count: json::validation::USize64(bones.len() as u64),
        component_type: Valid(json::accessor::GenericComponentType(
            json::accessor::ComponentType::F32,
        )),
        extensions: None,
        extras: json::Extras::default(),
        type_: Valid(json::accessor::Type::Mat4),
        min: None,
        max: None,
        name: None,
        normalized: false,
        sparse: None,
    });

    (nodes, ibm_accessor_idx)
}

/// Build glTF node extras for a bone's extended data.
///
/// Stores the Granny2 type definition and variant data as JSON so they
/// survive a glTF roundtrip.
fn build_bone_extras(bone: &GrannyBone) -> json::Extras {
    let (Some(data), Some(type_members)) = (&bone.extended_data, &bone.extended_data_type) else {
        return json::Extras::default();
    };

    let mut map = serde_json::Map::new();
    map.insert("granny_ext_type".into(), type_members_to_json(type_members));
    map.insert("granny_ext_data".into(), variant_to_json(data));

    let raw = serde_json::to_string(&serde_json::Value::Object(map)).unwrap();
    Some(serde_json::value::RawValue::from_string(raw).unwrap())
}
