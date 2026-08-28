//! Skeleton/bone node construction for glTF export.
//!
//! Handles building glTF node hierarchies and inverse bind matrices from
//! both cached data bones (0x700 chunk) and granny bones (0x703 chunk).

use gltf_json as json;
use json::validation::Checked::Valid;

use crate::granny_json::{type_members_to_json, variant_to_json};
use ugx::{Bone, Error, GrannyBone, Result, UgxGeom};

/// Build a mapping from section index to mesh index.
/// Analyzes which bones each section uses and matches against `granny_mesh` `bone_bindings`.
///
/// When multiple `granny_meshes` have identical bone sets (common for multi-section
/// rigid models all bound to `GrannyRootBone`), sections are distributed among
/// the matching meshes in order rather than all mapping to the first match.
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

    // Track how many sections have been assigned to each mesh.
    // Used to distribute sections among meshes with identical bone sets.
    let mut mesh_usage_count: Vec<usize> = vec![0; geom.granny_meshes.len()];

    // For each section, find which mesh it belongs to by matching bone usage
    let mut section_to_mesh = Vec::with_capacity(geom.sections.len());

    for section_idx in 0..geom.sections.len() {
        // Get bones used by this section's vertices
        let section_bones = get_section_bone_names(geom, section_idx, granny_bones);

        // Find the mesh whose bone_bindings best matches this section's bones,
        // preferring meshes that haven't been used yet (for identical bone sets).
        let mesh_idx = find_best_matching_mesh(&section_bones, &mesh_bone_sets, &mesh_usage_count);
        if mesh_idx < mesh_usage_count.len() {
            mesh_usage_count[mesh_idx] += 1;
        }
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
    let section = &geom.sections[section_idx];
    let has_remap = !section.bone_remap.is_empty();

    // Try to unpack vertices; if that fails, fall back to rigid_bone_index
    if let Ok(vertices) = geom.unpack_section_vertices(section_idx) {
        for v in &vertices {
            for k in 0..4 {
                if v.bone_weights[k] > 0.0 {
                    let raw_idx = usize::from(v.bone_indices[k]);
                    // Resolve to global 0-based bone index.
                    let global_idx = if has_remap {
                        // Section-local 0-based → remap to global 0-based.
                        if raw_idx < section.bone_remap.len() {
                            usize::from(section.bone_remap[raw_idx])
                        } else {
                            continue;
                        }
                    } else {
                        // Already 0-based global.
                        raw_idx
                    };
                    if global_idx < granny_bones.len() {
                        used_bones.insert(granny_bones[global_idx].name.clone());
                    }
                }
            }
        }
    }

    // If no bones found from vertices (e.g., global_bones section), use rigid_bone_index.
    // rigid_bone_index is 0-based; a negative value means "none".
    if used_bones.is_empty() && section_idx < geom.sections.len() {
        let section = &geom.sections[section_idx];
        let rigid_idx = section.rigid_bone_index;
        if let Ok(rigid_idx) = usize::try_from(rigid_idx)
            && let Some(bone) = granny_bones.get(rigid_idx)
        {
            used_bones.insert(bone.name.clone());
        }
    }

    used_bones
}

/// Find the mesh index whose `bone_bindings` best matches the section's bones.
/// Prefers:
/// 1. Unused meshes over already-used ones (when bone sets are identical)
/// 2. Exact match (section bones == mesh bones)
/// 3. Smallest superset (mesh contains all section bones with fewest extras)
/// 4. Highest overlap if no complete containment
///
/// Returns the mesh index, or mesh count to create a new mesh if none match.
fn find_best_matching_mesh(
    section_bones: &std::collections::HashSet<String>,
    mesh_bone_sets: &[std::collections::HashSet<&str>],
    mesh_usage_count: &[usize],
) -> usize {
    if section_bones.is_empty() || mesh_bone_sets.is_empty() {
        return mesh_bone_sets.len(); // Fallback: create new mesh
    }

    let mut best_mesh = mesh_bone_sets.len();
    // (is_superset, already_used, mesh_size, overlap)
    // Lower `already_used` is better (prefer unused meshes first).
    let mut best_score: (bool, usize, usize, usize) = (false, usize::MAX, usize::MAX, 0);

    for (mesh_idx, mesh_bones) in mesh_bone_sets.iter().enumerate() {
        // Count how many section bones are in this mesh's bone_bindings
        let overlap = section_bones
            .iter()
            .filter(|b| mesh_bones.contains(b.as_str()))
            .count();

        // Check if mesh contains ALL section bones (is a superset)
        let is_superset = overlap == section_bones.len();
        let mesh_size = mesh_bones.len();
        let usage = mesh_usage_count.get(mesh_idx).copied().unwrap_or(0);

        // Better if: superset beats non-superset, then prefer unused, then smaller, then more overlap
        let is_better = match (is_superset, best_score.0) {
            (true, false) => true,
            (true, true) => match usage.cmp(&best_score.1) {
                core::cmp::Ordering::Less => true,
                core::cmp::Ordering::Equal => mesh_size < best_score.2,
                core::cmp::Ordering::Greater => false,
            },
            (false, false) => overlap > best_score.3,
            (false, true) => false,
        };

        if is_better {
            best_score = (is_superset, usage, mesh_size, overlap);
            best_mesh = mesh_idx;
        }
    }

    // If no overlap found at all, fall back to the least-used mesh rather
    // than creating an anonymous mesh.  The original granny_mesh often only
    // binds the root bone while the section's vertices are skinned to many
    // child bones, leading to zero overlap.
    if best_score.3 == 0 && !mesh_bone_sets.is_empty() {
        // Pick the mesh with the lowest usage count (round-robin).
        mesh_usage_count
            .iter()
            .enumerate()
            .min_by_key(|&(_, c)| *c)
            .map_or(mesh_bone_sets.len(), |(i, _)| i)
    } else {
        best_mesh
    }
}

/// Create skeleton nodes from cached data bones (0x700 chunk).
/// Fallback when granny bones aren't available.
/// Returns (`bone_nodes`, `inverse_bind_matrices_accessor_index`).
pub(crate) fn create_skeleton_nodes(
    bones: &[Bone],
    buffer_data: &mut Vec<u8>,
    accessors: &mut Vec<json::Accessor>,
    buffer_views: &mut Vec<json::buffer::View>,
) -> Result<(Vec<json::Node>, u32)> {
    let mut nodes = Vec::with_capacity(bones.len());

    // Build child lists for hierarchy
    let mut children_map: std::collections::HashMap<i32, Vec<u32>> =
        std::collections::HashMap::new();
    for (i, bone) in bones.iter().enumerate() {
        if bone.parent_index >= 0 {
            children_map
                .entry(bone.parent_index)
                .or_default()
                .push(checked_u32(i, "bone node index")?);
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
    for (i, (bone, bone_world)) in bones.iter().zip(&bone_world_dx).enumerate() {
        let children = children_for(i, &children_map)?;

        // DX: local = world * parent_world^{-1}
        // Use model_to_bone[parent] directly (= parent_world^{-1}) to avoid
        // double-inversion precision loss.
        let local_dx = if bone.parent_index < 0 {
            bone_world.clone()
        } else {
            let parent_idx = usize::try_from(bone.parent_index)
                .map_err(|_| Error::SizeOverflow("bone parent index"))?;
            let parent = bones.get(parent_idx).ok_or_else(|| {
                Error::UnsupportedFormat("Bone parent index is out of bounds".into())
            })?;
            bone_world.multiply(&parent.model_to_bone)
        };

        // Write DX rows flat = column-major of GL matrix
        let gltf_matrix = flatten_matrix(&local_dx);

        nodes.push(json::Node {
            camera: None,
            children: if children.as_ref().is_none_or(std::vec::Vec::is_empty) {
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

    let accessor = append_inverse_bind_matrices(
        bones.iter().map(|bone| &bone.model_to_bone),
        bones.len(),
        buffer_data,
        accessors,
        buffer_views,
    )?;
    Ok((nodes, accessor))
}

/// Create skeleton nodes from granny bones (0x703 chunk).
/// Uses hierarchical structure with local transforms.
/// Returns (`bone_nodes`, `inverse_bind_matrices_accessor_index`).
pub(crate) fn create_skeleton_nodes_from_granny(
    bones: &[GrannyBone],
    buffer_data: &mut Vec<u8>,
    accessors: &mut Vec<json::Accessor>,
    buffer_views: &mut Vec<json::buffer::View>,
) -> Result<(Vec<json::Node>, u32)> {
    let mut nodes = Vec::with_capacity(bones.len());

    // Build child lists for hierarchy
    let mut children_map: std::collections::HashMap<i32, Vec<u32>> =
        std::collections::HashMap::new();
    for (i, bone) in bones.iter().enumerate() {
        if bone.parent_index >= 0 {
            children_map
                .entry(bone.parent_index)
                .or_default()
                .push(checked_u32(i, "Granny bone node index")?);
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
    for (i, (bone, bone_world)) in bones.iter().zip(&bone_world_dx).enumerate() {
        let children = children_for(i, &children_map)?;

        // In DX row-vector convention: v_world = v_local * local_dx * parent_world_dx
        // So: world_dx = local_dx * parent_world_dx
        // Therefore: local_dx = world_dx * parent_world_dx^{-1}
        //
        // Optimization: parent_world_dx^{-1} = IWM[parent] (which we already have),
        // avoiding double-inversion precision loss from (IWM.inverse()).inverse().
        let local_dx = if bone.parent_index < 0 {
            bone_world.clone()
        } else {
            let parent_idx = usize::try_from(bone.parent_index)
                .map_err(|_| Error::SizeOverflow("Granny bone parent index"))?;
            let parent = bones.get(parent_idx).ok_or_else(|| {
                Error::UnsupportedFormat("Granny bone parent index is out of bounds".into())
            })?;
            bone_world.multiply(&parent.inverse_world_matrix)
        };

        // glTF column-major storage of M_gl = row-major storage of M_dx
        // (because M_gl = M_dx^T, and column-major(M^T) = row-major(M))
        // So just write DX matrix rows flat.
        let gltf_matrix = flatten_matrix(&local_dx);

        // Serialize extended data into node extras if present
        let extras = build_bone_extras(bone);

        nodes.push(json::Node {
            camera: None,
            children: if children.as_ref().is_none_or(std::vec::Vec::is_empty) {
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

    let accessor = append_inverse_bind_matrices(
        bones.iter().map(|bone| &bone.inverse_world_matrix),
        bones.len(),
        buffer_data,
        accessors,
        buffer_views,
    )?;
    Ok((nodes, accessor))
}

fn children_for(
    bone_index: usize,
    children_map: &std::collections::HashMap<i32, Vec<u32>>,
) -> Result<Option<Vec<json::Index<json::scene::Node>>>> {
    let key = i32::try_from(bone_index).map_err(|_| Error::SizeOverflow("bone node index"))?;
    Ok(children_map.get(&key).and_then(|children| {
        (!children.is_empty()).then(|| {
            children
                .iter()
                .map(|&index| json::Index::new(index))
                .collect()
        })
    }))
}

fn flatten_matrix(matrix: &ugx::Matrix4x4) -> [f32; 16] {
    let rows = &matrix.rows;
    [
        rows[0][0], rows[0][1], rows[0][2], rows[0][3], rows[1][0], rows[1][1], rows[1][2],
        rows[1][3], rows[2][0], rows[2][1], rows[2][2], rows[2][3], rows[3][0], rows[3][1],
        rows[3][2], rows[3][3],
    ]
}

fn append_inverse_bind_matrices<'a>(
    matrices: impl IntoIterator<Item = &'a ugx::Matrix4x4>,
    matrix_count: usize,
    buffer_data: &mut Vec<u8>,
    accessors: &mut Vec<json::Accessor>,
    buffer_views: &mut Vec<json::buffer::View>,
) -> Result<u32> {
    while !buffer_data.len().is_multiple_of(4) {
        buffer_data.push(0);
    }
    let view_index = checked_u32(buffer_views.len(), "inverse-bind-matrix view index")?;
    let byte_offset = buffer_data.len();
    for matrix in matrices {
        for row in &matrix.rows {
            for value in row {
                buffer_data.extend_from_slice(&value.to_le_bytes());
            }
        }
    }
    let byte_length = buffer_data.len() - byte_offset;
    buffer_views.push(json::buffer::View {
        buffer: json::Index::new(0),
        byte_length: json::validation::USize64(checked_u64(
            byte_length,
            "inverse-bind-matrix byte length",
        )?),
        byte_offset: Some(json::validation::USize64(checked_u64(
            byte_offset,
            "inverse-bind-matrix byte offset",
        )?)),
        byte_stride: None,
        extensions: None,
        extras: json::Extras::default(),
        name: None,
        target: None,
    });
    let accessor_index = checked_u32(accessors.len(), "inverse-bind-matrix accessor index")?;
    accessors.push(json::Accessor {
        buffer_view: Some(json::Index::new(view_index)),
        byte_offset: Some(json::validation::USize64(0)),
        count: json::validation::USize64(checked_u64(matrix_count, "inverse-bind-matrix count")?),
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
    Ok(accessor_index)
}

fn checked_u32(value: usize, context: &'static str) -> Result<u32> {
    u32::try_from(value).map_err(|_| Error::SizeOverflow(context))
}

fn checked_u64(value: usize, context: &'static str) -> Result<u64> {
    u64::try_from(value).map_err(|_| Error::SizeOverflow(context))
}

/// Build glTF node extras for a bone's Granny data.
///
/// Stores `lod_error` and extended data (type + variant) as JSON so they
/// survive a glTF roundtrip. Local transforms are NOT stored — the writer
/// recomputes them from inverse world matrices on import.
fn build_bone_extras(bone: &GrannyBone) -> json::Extras {
    let has_extended = bone.extended_data.is_some() && bone.extended_data_type.is_some();
    let has_lod_error = bone.lod_error.to_bits() & 0x7fff_ffff != 0;

    if !has_extended && !has_lod_error {
        return json::Extras::default();
    }

    let mut map = serde_json::Map::new();

    // Extended data (type definition + variant values)
    if let (Some(data), Some(type_members)) = (&bone.extended_data, &bone.extended_data_type) {
        map.insert("granny_ext_type".into(), type_members_to_json(type_members));
        map.insert("granny_ext_data".into(), variant_to_json(data));
    }

    // LOD error
    if has_lod_error {
        map.insert("granny_lod_error".into(), json_f32(bone.lod_error));
    }

    crate::extras::to_raw_value(&serde_json::Value::Object(map))
}

/// Convert an f32 to a JSON value, preserving exact float representation.
fn json_f32(v: f32) -> serde_json::Value {
    serde_json::Number::from_f64(f64::from(v))
        .map_or(serde_json::Value::Null, serde_json::Value::Number)
}
