//! glTF import for UGX models.
//!
//! Converts glTF 2.0 format back to UGX geometry.
//!
//! # Matrix convention notes
//!
//! Our glTF export writes DX row-major matrices as flat rows into glTF's column-major
//! storage. On import we reverse this: read 16 floats from glTF as DX row-major directly.

mod accessor;
mod bounds;
mod material;
mod primitive;
mod skeleton;

use ugx::{
    Error, GrannyBone, GrannyBoneBinding, GrannyMesh, MAX_UV, Result, Section, UgxGeom, UgxVersion,
    UnivertPacker, UnpackedVertex, VertexElementType,
};

use accessor::resolve_buffer;
use bounds::compute_bounds;
use material::import_materials;
use primitive::import_primitive;
use skeleton::import_skeleton;

/// Import options for glTF → UGX conversion.
#[derive(Debug, Clone)]
pub struct GltfImportOptions {
    /// Import skeleton/bones if present (default: true).
    pub include_skeleton: bool,
    /// Import materials if present (default: true).
    pub include_materials: bool,
    /// Target UGX version (default: HW2).
    ///
    /// - `Hw1`: Float3 positions, Float3 normals/tangents, PNA0ST0 byte order,
    ///   embedded `base_vert_packer` in sections.
    /// - `Hw2`: HalfFloat4 positions, Dec3N normals/tangents, PT0NA0S byte order,
    ///   `base_vert_packer: None`.
    pub version: UgxVersion,
}

impl Default for GltfImportOptions {
    fn default() -> Self {
        Self {
            include_skeleton: true,
            include_materials: true,
            version: UgxVersion::Hw2,
        }
    }
}

/// Import a glTF document (JSON string + optional binary buffer) into a UgxGeom.
///
/// When `buffer_data` is None, embedded base64 buffers in the JSON are decoded automatically.
pub fn import_from_gltf(
    json_str: &str,
    buffer_data: Option<&[u8]>,
    options: &GltfImportOptions,
) -> Result<UgxGeom> {
    let root: gltf_json::Root = serde_json::from_str(json_str)
        .map_err(|e| Error::UnsupportedFormat(format!("Invalid glTF JSON: {}", e)))?;

    // Resolve buffer data
    let buffer_bytes = resolve_buffer(&root, buffer_data)?;

    // Import skeleton if present
    let (bones, granny_bones) = if options.include_skeleton {
        import_skeleton(&root, &buffer_bytes)?
    } else {
        (Vec::new(), Vec::new())
    };

    // Read geom-level extras from the default scene (if present).
    let scene_extras_json: Option<serde_json::Value> = root
        .scenes
        .first()
        .and_then(|s| s.extras.as_ref())
        .and_then(|raw| serde_json::from_str(raw.get()).ok());
    let extras_max_instances: Option<i16> = scene_extras_json
        .as_ref()
        .and_then(|v| v.get("ugx_max_instances").and_then(|n| n.as_i64()))
        .map(|i| i as i16);

    // Import materials
    let materials = if options.include_materials {
        import_materials(&root)
    } else {
        Vec::new()
    };

    // Import mesh data
    let mut all_vertices: Vec<UnpackedVertex> = Vec::new();
    let mut all_vertex_buffer: Vec<u8> = Vec::new();
    let mut all_index_buffer: Vec<u16> = Vec::new();
    let mut sections: Vec<Section> = Vec::new();

    // Track mesh names, vertex ranges, and section ranges for granny_meshes generation
    // (name, start_vertex, end_vertex, start_section, end_section)
    let mut mesh_infos: Vec<(String, usize, usize, usize, usize, Option<usize>)> = Vec::new();

    let has_skeleton = !bones.is_empty();

    for (mesh_idx, mesh) in root.meshes.iter().enumerate() {
        let mesh_name = mesh
            .name
            .clone()
            .unwrap_or_else(|| format!("mesh_{}", mesh_idx));
        let mesh_start_vertex = all_vertices.len();
        let mesh_start_section = sections.len();

        // Read section flags from mesh extras if present (written by our exporter).
        // None means third-party glTF (Blender etc.) → fall back to heuristic.
        let mesh_extras_json: Option<serde_json::Value> = mesh
            .extras
            .as_ref()
            .and_then(|raw| serde_json::from_str(raw.get()).ok());
        let extras_global_bones: Option<bool> = mesh_extras_json
            .as_ref()
            .and_then(|v| v.get("ugx_global_bones").and_then(|b| b.as_bool()));
        let extras_rigid_only: Option<bool> = mesh_extras_json
            .as_ref()
            .and_then(|v| v.get("ugx_rigid_only").and_then(|b| b.as_bool()));
        let extras_rigid_bone_index: Option<i32> = mesh_extras_json
            .as_ref()
            .and_then(|v| v.get("ugx_rigid_bone_index").and_then(|b| b.as_i64()))
            .map(|i| i as i32);
        let extras_granny_mesh_index: Option<usize> = mesh_extras_json
            .as_ref()
            .and_then(|v| v.get("ugx_granny_mesh_index").and_then(|b| b.as_u64()))
            .map(|i| i as usize);

        for primitive in &mesh.primitives {
            let (vertices, indices, material_index) =
                import_primitive(primitive, &root, &buffer_bytes, has_skeleton, bones.len())?;

            if vertices.is_empty() || indices.is_empty() {
                continue;
            }

            // Choose vertex format based on what data is present
            let has_tangents = vertices
                .iter()
                .any(|v| v.tangent[0] != 0.0 || v.tangent[1] != 0.0 || v.tangent[2] != 0.0);
            let has_skin = has_skeleton
                && vertices
                    .iter()
                    .any(|v| v.bone_weights.iter().sum::<f32>() > 0.0);
            let has_colors = vertices.iter().any(|v| {
                v.diffuse[0] != 0.0
                    || v.diffuse[1] != 0.0
                    || v.diffuse[2] != 0.0
                    || v.diffuse[3] != 0.0
            });
            let max_texcoords = vertices.iter().map(|v| v.num_texcoords).max().unwrap_or(0);

            // Build pack order and vertex types based on target version.
            //
            // HW1 (v4) — PNA0ST0 byte order, Float3 types:
            //   Position(Float3) → Normal(Float3) → Tangent(Float3) → Skin → UV(Float2) → Color
            //
            // HW2 (v6) — PT0NA0S byte order, compact types:
            //   Position(HalfFloat4) → UV(HalfFloat2) → Normal(Dec3N) →
            //   Tangent(Dec3N) → Skin(UByte4+UByte4N) → Color
            let packer = build_packer(
                options.version,
                max_texcoords,
                has_tangents,
                has_skin,
                has_colors,
            );

            // Determine global_bones / rigid_only: use extras if present,
            // otherwise fall back to heuristic for third-party glTFs.
            let (is_global_bones, is_rigid_only, global_bone_idx, actual_max_bones) =
                if let Some(gb) = extras_global_bones {
                    let ro = extras_rigid_only.unwrap_or(false);
                    if gb || ro {
                        // Explicitly rigid — use extras rigid_bone_index if present,
                        // otherwise find the common bone from vertex data.
                        let bone_idx = extras_rigid_bone_index.unwrap_or_else(|| {
                            vertices
                                .iter()
                                .find(|v| v.bone_weights[0] > 0.0)
                                .map(|v| (v.bone_indices[0] as i32) - 1)
                                .unwrap_or(0)
                        });
                        (gb, ro, bone_idx, 1)
                    } else {
                        // Explicitly NOT global_bones/rigid — compute max influences.
                        let max_inf = if has_skin {
                            vertices
                                .iter()
                                .map(|v| v.bone_weights.iter().filter(|&&w| w > 0.0).count() as i32)
                                .max()
                                .unwrap_or(1)
                                .max(1)
                        } else {
                            1
                        };
                        (false, false, i32::MAX, max_inf)
                    }
                } else {
                    // No extras — third-party glTF, use heuristic.
                    let (gb, idx, mb) = detect_global_bones(&vertices, has_skin);
                    (gb, false, idx, mb)
                };

            // For global_bones or rigid_only sections, strip skin data and
            // restore zero weights (the original buffer had no skin element).
            let strip_skin = is_global_bones || is_rigid_only;
            let (final_packer, final_vertices) = if strip_skin {
                let rigid_packer = build_packer(
                    options.version,
                    max_texcoords,
                    has_tangents,
                    false, // no skin
                    has_colors,
                );

                let restored_vertices: Vec<UnpackedVertex> = vertices
                    .iter()
                    .map(|v| {
                        let mut rv = v.clone();
                        rv.bone_weights = [0.0, 0.0, 0.0, 0.0];
                        rv.bone_indices = [0, 0, 0, 0];
                        rv
                    })
                    .collect();

                (rigid_packer, restored_vertices)
            } else {
                (packer, vertices)
            };

            // Pack vertices into binary buffer
            let vb_offset = all_vertex_buffer.len() as i32;
            for v in &final_vertices {
                final_packer.pack_vertex(&mut all_vertex_buffer, v);
            }
            let vb_bytes = (all_vertex_buffer.len() as i32) - vb_offset;
            let vert_size = final_packer.vertex_size() as i32;

            // Add indices
            let ib_offset = all_index_buffer.len() as i32;
            all_index_buffer.extend_from_slice(&indices);
            let num_tris = (indices.len() / 3) as i32;

            // For HW1/DE, embed the UnivertPacker in the section.
            // For HW2, vertex format is inferred from vert_size — no packer stored.
            let base_vert_packer = match options.version {
                UgxVersion::Hw1 => Some(final_packer.clone()),
                UgxVersion::Hw2 => None,
            };

            sections.push(Section {
                material_index,
                accessory_index: 0, // Default accessory index (0 = none)
                max_bones: actual_max_bones,
                rigid_bone_index: global_bone_idx,
                ib_offset,
                num_tris,
                vb_offset,
                vb_bytes,
                vert_size,
                num_verts: final_vertices.len() as i32,
                base_vert_packer,
                bone_remap: Vec::new(),
                rigid_only: is_rigid_only,
                global_bones: is_global_bones,
            });

            all_vertices.extend(final_vertices);
        }

        // Record mesh info for granny_meshes generation
        // Include section indices to handle global_bones sections
        let mesh_end_section = sections.len();
        let mesh_end_vertex = all_vertices.len();
        if mesh_end_vertex > mesh_start_vertex {
            mesh_infos.push((
                mesh_name,
                mesh_start_vertex,
                mesh_end_vertex,
                mesh_start_section,
                mesh_end_section,
                extras_granny_mesh_index,
            ));
        }
    }

    // Compute bounding volumes from all vertices
    let (bounds, bounding_sphere) = compute_bounds(&all_vertices);

    // Compute bone bounds
    let bone_bounds = if !bones.is_empty() {
        bones.iter().map(|_| bounds.clone()).collect()
    } else {
        Vec::new()
    };

    let all_rigid = sections.iter().all(|s| s.rigid_only);
    let all_skinned = sections.iter().all(|s| !s.rigid_only);
    // Header globalBones should be true if any section uses global bones
    let any_global_bones = sections.iter().any(|s| s.global_bones);

    // allSectionsSkinned is true only if:
    // - globalBones is true
    // - Not all sections are rigid
    // - The model is not rigidOnly
    // - ALL sections are skinned (none are rigidOnly)
    // - No sections use global_bones (section-level flag)
    //
    // Note: The original game code sets this flag, but the original marine_01.ugx
    // has it as false even though it meets the criteria. This suggests either:
    // 1. The original file was generated before this flag was implemented
    // 2. Or sections with global_bones=true don't count as "skinned"
    //
    // We match the original behavior: if any section has global_bones=true,
    // don't set all_sections_skinned=true (these sections are transformed differently)
    let all_sections_skinned = !any_global_bones && !all_rigid && all_skinned;

    // Generate granny_meshes from vertex skin data and section info, preserving mesh names from glTF
    let granny_meshes =
        generate_granny_meshes_from_vertices(&all_vertices, &granny_bones, &mesh_infos, &sections);

    let max_vertex_index = sections
        .iter()
        .map(|s| s.num_verts as u32)
        .max()
        .unwrap_or(1);
    let instance_index_multiplier = max_vertex_index.next_power_of_two() as i16;

    let mut geom = UgxGeom {
        bounding_sphere,
        bounds,
        materials,
        bones,
        granny_bones,
        granny_meshes,
        skeleton_lod_type: 0,
        bone_bounds,
        sections,
        accessories: Vec::new(),
        valid_accessories: Vec::new(),
        vertex_buffer: all_vertex_buffer,
        index_buffer: all_index_buffer,
        rigid_only: all_rigid,
        rigid_bone_index: 0,
        max_instances: extras_max_instances.unwrap_or(1),
        instance_index_multiplier,
        large_geom_bone_index: i16::MAX,
        all_sections_rigid: all_rigid,
        all_sections_skinned,
        global_bones: any_global_bones,
        aabb_tree: None,
    };

    // Rebuild all derived data that doesn't survive the glTF round-trip:
    // bone bounds, accessories, metadata flags, and AABB tree.
    geom.rebuild_derived_data();

    Ok(geom)
}

/// Generate `GrannyMesh` entries from vertex skin data and section info.
///
/// When `ugx_granny_mesh_index` is present in the glTF extras (our exporter),
/// sections (glTF meshes) are grouped by that index into shared `GrannyMesh`
/// containers. This preserves the original mesh identity for multi-section
/// rigid models where all sections share the same bone set.
///
/// For third-party glTFs (no extras), each glTF mesh becomes its own `GrannyMesh`.
/// (name, start_vertex, end_vertex, start_section, end_section, granny_mesh_index)
type MeshInfo = (String, usize, usize, usize, usize, Option<usize>);

fn generate_granny_meshes_from_vertices(
    vertices: &[UnpackedVertex],
    granny_bones: &[GrannyBone],
    mesh_infos: &[MeshInfo],
    sections: &[Section],
) -> Vec<GrannyMesh> {
    // Group mesh_infos by granny_mesh_index. If any entry has an explicit index,
    // use that to merge multiple glTF meshes into one GrannyMesh. Otherwise each
    // entry stays separate.
    let has_explicit_indices = mesh_infos.iter().any(|m| m.5.is_some());

    // Build groups: Vec<(name, Vec<(start_vert, end_vert, start_sec, end_sec)>)>
    // ordered by granny_mesh_index.
    struct MeshGroup {
        name: String,
        ranges: Vec<(usize, usize, usize, usize)>, // (start_vert, end_vert, start_sec, end_sec)
    }

    let groups: Vec<MeshGroup> = if has_explicit_indices {
        // Collect by index, preserving order
        let mut map: std::collections::BTreeMap<usize, MeshGroup> =
            std::collections::BTreeMap::new();
        for (name, sv, ev, ss, es, idx_opt) in mesh_infos {
            let idx = idx_opt.unwrap_or(map.len() + 10000); // fallback: unique high idx
            let group = map.entry(idx).or_insert_with(|| MeshGroup {
                name: name.clone(),
                ranges: Vec::new(),
            });

            group.ranges.push((*sv, *ev, *ss, *es));
        }

        map.into_values().collect()
    } else {
        // No explicit indices — one group per mesh_info (original behavior)
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
                    if v.bone_weights[k] > 0.0 && v.bone_indices[k] > 0 {
                        used_bones.insert(v.bone_indices[k]);
                    }
                }
            }
            for section in &sections[ss..es] {
                if section.global_bones && section.rigid_bone_index >= 0 {
                    let bone_idx_1based = (section.rigid_bone_index as u16) + 1;
                    used_bones.insert(bone_idx_1based);
                }
                if (section.global_bones || section.rigid_only) && section.rigid_bone_index >= 0 {
                    let bone_idx_1based = (section.rigid_bone_index as u16) + 1;
                    rigid_bone_indices.insert(bone_idx_1based);
                }
            }
        }

        if used_bones.is_empty() {
            continue;
        }

        // Gather all vertices across all ranges in this group for OBB calculation
        let all_group_verts: Vec<&UnpackedVertex> = group
            .ranges
            .iter()
            .flat_map(|&(sv, ev, _, _)| &vertices[sv..ev])
            .collect();

        let bone_bindings: Vec<GrannyBoneBinding> = used_bones
            .iter()
            .filter_map(|&idx| {
                let idx_0based = (idx as usize).saturating_sub(1);
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
/// Finds all vertices weighted to `bone_idx_1based`, transforms their positions
/// into bone-local space using the bone's `inverse_world_matrix`, and returns
/// the axis-aligned min/max in that space.
///
/// When `owns_all` is true (rigid/global_bones sections), all vertices are
/// considered bound to this bone regardless of their weight values.
///
/// If no vertices reference this bone, returns zeroed min/max.
fn compute_bone_obb(
    vertices: &[&UnpackedVertex],
    bone_idx_1based: u16,
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
            (0..4).any(|k| v.bone_indices[k] == bone_idx_1based && v.bone_weights[k] > 0.0)
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

/// Detect whether a set of vertices forms a "global_bones" section.
///
/// A global_bones section is one where *all* vertices are bound to a single
/// common bone with weight ≈ 1.0 (the pattern produced when the exporter
/// converts zero-weight vertices to explicit single-bone weighting).
///
/// Returns `(is_global_bones, rigid_bone_index, max_bones_per_vertex)`.
///
/// `max_bones` is the maximum number of **non-zero** weight influences on any
/// single vertex (controls shader selection: 1 → ONE_BONE_REG, >2 →
/// FOUR_BONES_REG).
fn detect_global_bones(vertices: &[UnpackedVertex], has_skin: bool) -> (bool, i32, i32) {
    if !has_skin {
        return (false, i32::MAX, 1);
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
        // Convert 1-based bone index to 0-based for rigid_bone_index
        (true, (bone as i32) - 1, 1)
    } else {
        (false, i32::MAX, max_bones)
    }
}

/// Build a `UnivertPacker` for the target version.
///
/// HW1/DE (v4) — `PNA0ST0` byte order, Float3 types:
///   Position(Float3, 12B) → Normal(Float3, 12B) → Tangent(Float3, 12B) →
///   Skin(UByte4+UByte4N, 8B) → UV(Float2, 8B)
///
/// HW2 (v6) — `PT0NA0S` byte order, compact types:
///   Position(HalfFloat4, 8B) → UV(HalfFloat2, 4B) → Normal(Dec3N, 4B) →
///   Tangent(Dec3N, 4B) → Skin(UByte4+UByte4N, 8B)
fn build_packer(
    version: UgxVersion,
    max_texcoords: usize,
    has_tangents: bool,
    has_skin: bool,
    has_colors: bool,
) -> UnivertPacker {
    let mut uv_types = [VertexElementType::Ignore; MAX_UV];
    for uv_type in uv_types.iter_mut().take(max_texcoords.min(MAX_UV)) {
        *uv_type = VertexElementType::HalfFloat2;
    }

    // Pack order differs between versions — HW1 puts normals before UVs,
    // HW2 puts UVs before normals.
    let pack_order = match version {
        UgxVersion::Hw1 => {
            let mut po = String::from("P");
            po.push('N');

            if has_tangents {
                po.push_str("A0");
            }

            if has_skin {
                po.push('S');
            }

            for i in 0..max_texcoords {
                po.push('T');
                po.push(char::from_digit(i as u32, 10).unwrap_or('0'));
            }

            if has_colors {
                po.push('D');
            }

            po
        }
        UgxVersion::Hw2 => {
            let mut po = String::from("P");
            for i in 0..max_texcoords {
                po.push('T');
                po.push(char::from_digit(i as u32, 10).unwrap_or('0'));
            }

            po.push('N');

            if has_tangents {
                po.push_str("A0");
            }

            if has_skin {
                po.push('S');
            }

            if has_colors {
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
