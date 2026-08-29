//! Mesh helper functions for glTF import.
//!
//! Contains granny mesh generation from vertex skin data, bone OBB computation,
//! `global_bones` heuristic detection, and vertex packer construction.

use ugx::{
    GrannyBone, GrannyBoneBinding, GrannyMesh, MAX_UV, Section, UgxVersion, UnivertPacker,
    UnpackedVertex, VertexElementType,
};

/// Generate `GrannyMesh` entries from vertex skin data and section info.
///
/// When `ugx_granny_mesh_index` is present in the glTF extras (our exporter),
/// sections (glTF meshes) are grouped by that index into shared `GrannyMesh`
/// containers. This preserves the original mesh identity for multi-section
/// rigid models where all sections share the same bone set.
///
/// For third-party glTFs (no extras), each glTF mesh becomes its own `GrannyMesh`.
/// (name, `start_vertex`, `end_vertex`, `start_section`, `end_section`, `granny_mesh_index`)
pub(super) type MeshInfo = (String, usize, usize, usize, usize, Option<usize>);

struct MeshGroup {
    name: String,
    ranges: Vec<(usize, usize, usize, usize)>,
}

pub(super) fn generate_granny_meshes_from_vertices(
    vertices: &[UnpackedVertex],
    granny_bones: &[GrannyBone],
    mesh_infos: &[MeshInfo],
    sections: &[Section],
) -> Vec<GrannyMesh> {
    // Group mesh_infos by granny_mesh_index. If any entry has an explicit index,
    // use that to merge multiple glTF meshes into one GrannyMesh. Otherwise each
    // entry stays separate.
    let has_explicit_indices = mesh_infos.iter().any(|m| m.5.is_some());

    let groups: Vec<MeshGroup> = if has_explicit_indices {
        let mut map: std::collections::BTreeMap<usize, MeshGroup> =
            std::collections::BTreeMap::new();
        for (name, sv, ev, ss, es, idx_opt) in mesh_infos {
            let idx = idx_opt.unwrap_or(map.len() + 10000);
            let group = map.entry(idx).or_insert_with(|| MeshGroup {
                name: name.clone(),
                ranges: Vec::new(),
            });
            group.ranges.push((*sv, *ev, *ss, *es));
        }
        map.into_values().collect()
    } else {
        mesh_infos
            .iter()
            .map(|(name, sv, ev, ss, es, _)| MeshGroup {
                name: name.clone(),
                ranges: vec![(*sv, *ev, *ss, *es)],
            })
            .collect()
    };

    let mut granny_meshes = Vec::new();

    for group in &groups {
        let mut used_bones: std::collections::BTreeSet<u16> = std::collections::BTreeSet::new();
        let mut rigid_bone_indices: std::collections::BTreeSet<u16> =
            std::collections::BTreeSet::new();

        for &(sv, ev, ss, es) in &group.ranges {
            for v in &vertices[sv..ev] {
                for k in 0..4 {
                    if v.bone_weights[k] > 0.0 {
                        used_bones.insert(v.bone_indices[k]);
                    }
                }
            }
            for section in &sections[ss..es] {
                if (section.global_bones || section.rigid_only)
                    && let Ok(bone_index) = u16::try_from(section.rigid_bone_index)
                {
                    used_bones.insert(bone_index);
                }
                if (section.global_bones || section.rigid_only)
                    && let Ok(bone_index) = u16::try_from(section.rigid_bone_index)
                {
                    rigid_bone_indices.insert(bone_index);
                }
            }
        }

        if used_bones.is_empty() {
            continue;
        }

        // Always include the root bone (index 0) in the binding list.
        // The engine expects meshes to be bound to the skeleton root.
        if !granny_bones.is_empty() {
            used_bones.insert(0);
        }

        let all_group_verts: Vec<&UnpackedVertex> = group
            .ranges
            .iter()
            .flat_map(|&(sv, ev, _, _)| &vertices[sv..ev])
            .collect();

        let bone_bindings: Vec<GrannyBoneBinding> = used_bones
            .iter()
            .filter_map(|&idx| {
                let idx_0based = usize::from(idx);
                granny_bones.get(idx_0based).map(|b| {
                    let owns_all = rigid_bone_indices.contains(&idx);
                    let (obb_min, obb_max) =
                        compute_bone_obb(&all_group_verts, idx, &b.inverse_world_matrix, owns_all);
                    GrannyBoneBinding {
                        bone_name: b.name.clone(),
                        obb_min,
                        obb_max,
                        triangle_indices: Vec::new(),
                    }
                })
            })
            .collect();

        if !bone_bindings.is_empty() {
            granny_meshes.push(GrannyMesh {
                name: group.name.clone(),
                bone_bindings,
            });
        }
    }

    granny_meshes
}

/// Compute the OBB (oriented bounding box) for a bone from vertex data.
///
/// Finds all vertices weighted to `bone_idx`, transforms their positions
/// into bone-local space using the bone's `inverse_world_matrix`, and returns
/// the axis-aligned min/max in that space.
///
/// When `owns_all` is true (`rigid/global_bones` sections), all vertices are
/// considered bound to this bone regardless of their weight values.
///
/// If no vertices reference this bone, returns zeroed min/max.
fn compute_bone_obb(
    vertices: &[&UnpackedVertex],
    bone_idx: u16,
    inverse_world_matrix: &ugx::Matrix4x4,
    owns_all: bool,
) -> ([f32; 3], [f32; 3]) {
    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    let mut found = false;

    let m = &inverse_world_matrix.rows;

    for v in vertices {
        let weighted = if owns_all {
            true
        } else {
            (0..4).any(|k| v.bone_indices[k] == bone_idx && v.bone_weights[k] > 0.0)
        };
        if !weighted {
            continue;
        }

        let px = v.position[0];
        let py = v.position[1];
        let pz = v.position[2];
        let lx = px * m[0][0] + py * m[1][0] + pz * m[2][0] + m[3][0];
        let ly = px * m[0][1] + py * m[1][1] + pz * m[2][1] + m[3][1];
        let lz = px * m[0][2] + py * m[1][2] + pz * m[2][2] + m[3][2];

        for (i, &val) in [lx, ly, lz].iter().enumerate() {
            if val < min[i] {
                min[i] = val;
            }
            if val > max[i] {
                max[i] = val;
            }
        }
        found = true;
    }

    if found {
        (min, max)
    } else {
        ([0.0; 3], [0.0; 3])
    }
}

/// Detect whether a set of vertices forms a rigid or `global_bones` section.
///
/// When all vertices are bound to a single common bone with weight ≈ 1.0,
/// this is the pattern produced by the exporter when it synthesizes skin data
/// for originally rigid sections. In that case we return `rigid_only=true`
/// and `global_bones=false` to match the original UGX section flags.
///
/// Returns `(is_global_bones, is_rigid_only, rigid_bone_index, max_bones_per_vertex)`.
pub(super) fn detect_global_bones(
    vertices: &[UnpackedVertex],
    has_skin: bool,
) -> (bool, bool, i32, i32) {
    if !has_skin {
        return (false, false, i32::MAX, 1);
    }

    let mut all_single_bone = true;
    let mut common_bone: Option<u16> = None;
    let mut max_influences = 0i32;

    for v in vertices {
        let mut num_influences = 0;
        for k in 0..4 {
            if v.bone_weights[k] > 0.0 {
                num_influences += 1;
            }
        }
        max_influences = max_influences.max(num_influences);

        let is_single = v.bone_weights[0] > 0.99
            && v.bone_weights[1] < 0.01
            && v.bone_weights[2] < 0.01
            && v.bone_weights[3] < 0.01;

        if is_single {
            let bone = v.bone_indices[0];
            match common_bone {
                None => common_bone = Some(bone),
                Some(cb) if cb != bone => all_single_bone = false,
                _ => {}
            }
        } else {
            all_single_bone = false;
        }
    }

    let max_bones = max_influences.max(1);

    if let Some(bone) = common_bone.filter(|_| all_single_bone) {
        // All vertices bound to a single bone — this is a rigid section.
        // The original format uses rigid_only=true, global_bones=false for
        // this case (the vertex buffer has no skin element, and the section
        // is bound to rigid_bone_index).
        // bone_indices are 0-based global, same as rigid_bone_index.
        (false, true, i32::from(bone), 1)
    } else {
        (false, false, i32::MAX, max_bones)
    }
}

/// Build a `UnivertPacker` for the target version.
///
/// HW1 (v4) — `PNA0ST0` byte order, Float3 types:
///   Position(Float3, 12B) → Normal(Float3, 12B) → Tangent(Float3, 12B) →
///   Skin(UByte4+UByte4N, 8B) → UV(Float2, 8B)
///
/// HW2 (v6) — `PT0NA0S` byte order, compact types:
///   Position(HalfFloat4, 8B) → UV(HalfFloat2, 4B) → Normal(Dec3N, 4B) →
///   Tangent(Dec3N, 4B) → Skin(UByte4+UByte4N, 8B)
pub(super) fn build_packer(
    version: UgxVersion,
    max_texcoords: usize,
    has_tangents: bool,
    has_skin: bool,
    has_colors: bool,
) -> UnivertPacker {
    // HW2 vertex declarations live outside the UGX file. Keep the emitted
    // byte layout canonical so it can be inferred from the section stride:
    // PT0NA0, optional skin, extra UV sets, then optional diffuse color.
    // A diffuse element disambiguates multiple UV sets from a lone color.
    let emitted_texcoords = match version {
        UgxVersion::Hw1 => max_texcoords,
        UgxVersion::Hw2 => max_texcoords.max(1),
    }
    .min(MAX_UV);
    let emitted_tangents = has_tangents || version == UgxVersion::Hw2;
    let emitted_colors = has_colors || (version == UgxVersion::Hw2 && emitted_texcoords > 1);
    let mut uv_types = [VertexElementType::Ignore; MAX_UV];
    for uv_type in uv_types.iter_mut().take(emitted_texcoords) {
        *uv_type = VertexElementType::HalfFloat2;
    }

    let pack_order = match version {
        UgxVersion::Hw1 => {
            let mut po = String::from("P");
            po.push('N');
            if emitted_tangents {
                po.push_str("A0");
            }
            if has_skin {
                po.push('S');
            }
            for digit in "0123456789".chars().take(emitted_texcoords) {
                po.push('T');
                po.push(digit);
            }
            if emitted_colors {
                po.push('D');
            }
            po
        }
        UgxVersion::Hw2 => {
            let mut po = String::from("PT0NA0");
            if has_skin {
                po.push('S');
            }
            for digit in "123456789".chars().take(emitted_texcoords - 1) {
                po.push('T');
                po.push(digit);
            }
            if emitted_colors {
                po.push('D');
            }
            po
        }
    };

    UnivertPacker {
        pack_order,
        decl_order: String::new(),
        pos_type: version.default_pos_type(),
        basis_type: version.default_basis_type(),
        basis_scale_type: version.default_basis_scale_type(),
        tangent_type: version.default_tangent_type(),
        normal_type: version.default_normal_type(),
        uv_types,
        indices_type: VertexElementType::UByte4,
        weights_type: VertexElementType::UByte4N,
        diffuse_type: VertexElementType::D3DColor,
        index_type: VertexElementType::Ignore,
    }
}
