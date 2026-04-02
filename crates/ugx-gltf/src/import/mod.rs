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
mod mesh;
mod primitive;
mod skeleton;

use ugx::types::MaterialData;
use ugx::types::convert::convert_geom_materials;
use ugx::{Error, Result, Section, UgxGeom, UgxVersion, UnpackedVertex};

use crate::extras::{MeshExtrasJson, SceneExtrasJson};

use accessor::resolve_buffer;
use bounds::compute_bounds;
use material::import_materials;
use mesh::{build_packer, detect_global_bones, generate_granny_meshes_from_vertices};
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
    let scene_extras: Option<SceneExtrasJson> = root
        .scenes
        .first()
        .and_then(|s| s.extras.as_ref())
        .and_then(|raw| serde_json::from_str(raw.get()).ok());
    let extras_max_instances: Option<i16> = scene_extras.as_ref().map(|e| e.ugx_max_instances);

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

    // Build structural maps for detecting rigid sections from the glTF node
    // hierarchy. A mesh node that has no skin reference and is a child of a
    // joint node is a rigid section bound to that joint/bone.

    // Map from node index to its parent node index.
    let mut node_parent: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    for (i, node) in root.nodes.iter().enumerate() {
        if let Some(ref children) = node.children {
            for child in children {
                node_parent.insert(child.value(), i);
            }
        }
    }

    // Set of node indices that are joints in the skin.
    let joint_node_set: std::collections::HashSet<usize> = root
        .skins
        .first()
        .map(|skin| skin.joints.iter().map(|j| j.value()).collect())
        .unwrap_or_default();

    // Map from mesh index to the node that references it, and whether it has a skin.
    struct MeshNodeInfo {
        #[allow(dead_code)]
        node_idx: usize,
        has_skin: bool,
        parent_bone_idx: Option<usize>, // joint index (0-based) if parent is a bone
    }
    let mut mesh_node_map: std::collections::HashMap<usize, MeshNodeInfo> =
        std::collections::HashMap::new();
    for (node_idx, node) in root.nodes.iter().enumerate() {
        if let Some(ref mesh_ref) = node.mesh {
            let mi = mesh_ref.value();
            let has_skin = node.skin.is_some();
            let parent_bone = if !has_skin {
                // Check if this node's parent is a joint → rigid section
                node_parent.get(&node_idx).and_then(|&parent_idx| {
                    if joint_node_set.contains(&parent_idx) {
                        // The parent node is a joint. Find which bone index it maps to.
                        root.skins.first().and_then(|skin| {
                            skin.joints.iter().position(|j| j.value() == parent_idx)
                        })
                    } else {
                        None
                    }
                })
            } else {
                None
            };
            mesh_node_map.insert(
                mi,
                MeshNodeInfo {
                    node_idx,
                    has_skin,
                    parent_bone_idx: parent_bone,
                },
            );
        }
    }

    // Precompute bone world matrices (IWM⁻¹) for transforming rigid vertices
    // from bone-local space back to model space.
    let bone_world_matrices: Vec<ugx::Matrix4x4> = granny_bones
        .iter()
        .map(|b| {
            b.inverse_world_matrix
                .inverse()
                .unwrap_or_else(ugx::Matrix4x4::identity)
        })
        .collect();

    for (mesh_idx, mesh) in root.meshes.iter().enumerate() {
        let mesh_name = mesh
            .name
            .clone()
            .unwrap_or_else(|| format!("mesh_{}", mesh_idx));
        let mesh_start_vertex = all_vertices.len();
        let mesh_start_section = sections.len();

        // Read mesh extras for metadata that can't be inferred from structure.
        let mesh_ext: Option<MeshExtrasJson> = mesh
            .extras
            .as_ref()
            .and_then(|raw| serde_json::from_str(raw.get()).ok());
        let extras_granny_mesh_index: Option<usize> =
            mesh_ext.as_ref().and_then(|e| e.ugx_granny_mesh_index);

        // LOD distances: use values from extras (our exporter), default for third-party glTFs.
        let lod_near = mesh_ext.as_ref().map_or(0.0, |e| e.ugx_lod_near_distance);
        let lod_far = mesh_ext
            .as_ref()
            .map_or(f32::MAX, |e| e.ugx_lod_far_distance);
        let lod_fade = mesh_ext.as_ref().map_or(0.0, |e| e.ugx_lod_fade_distance);

        // Detect rigid section from glTF structure:
        // - Mesh node has no skin AND is parented to a bone node → rigid
        // - Mesh node has skin → skinned
        // - No node info (shouldn't happen) → fall back to heuristic
        let struct_rigid_bone: Option<usize> = mesh_node_map.get(&mesh_idx).and_then(|info| {
            if !info.has_skin {
                info.parent_bone_idx
            } else {
                None
            }
        });

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

            // Determine rigid/skinned from glTF structure:
            // - struct_rigid_bone is Some(bone_idx) → rigid (bone-parented, no skin)
            // - struct_rigid_bone is None → skinned or no skeleton
            // For third-party glTFs without our node structure, fall back to heuristic.
            let (is_global_bones, is_rigid_only, global_bone_idx, actual_max_bones) =
                if let Some(bone_idx) = struct_rigid_bone {
                    // Structurally rigid: mesh is parented to a bone, no skin.
                    (true, true, bone_idx as i32, 1)
                } else if mesh_node_map
                    .get(&mesh_idx)
                    .is_some_and(|info| info.has_skin)
                {
                    // Structurally skinned: mesh has a skin reference.
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
                } else {
                    // No node info or no skin/parent — third-party glTF, use heuristic.
                    detect_global_bones(&vertices, has_skin)
                };

            // For rigid sections: transform vertices from bone-local space back
            // to model space, and strip skin data (rigid sections have no skin element).
            let (final_packer, final_vertices) = if is_global_bones || is_rigid_only {
                let rigid_packer = build_packer(
                    options.version,
                    max_texcoords,
                    has_tangents,
                    false, // no skin
                    has_colors,
                );

                // Transform bone-local → model space if we have the bone's world matrix.
                let bone_idx = global_bone_idx as usize;
                let has_world_mat = bone_idx < bone_world_matrices.len();

                let restored_vertices: Vec<UnpackedVertex> = vertices
                    .iter()
                    .map(|v| {
                        let mut rv = v.clone();
                        if has_world_mat {
                            let m = &bone_world_matrices[bone_idx].rows;
                            // Position: v_model = v_local * bone_to_model
                            let px = v.position[0];
                            let py = v.position[1];
                            let pz = v.position[2];
                            rv.position = [
                                px * m[0][0] + py * m[1][0] + pz * m[2][0] + m[3][0],
                                px * m[0][1] + py * m[1][1] + pz * m[2][1] + m[3][1],
                                px * m[0][2] + py * m[1][2] + pz * m[2][2] + m[3][2],
                            ];
                            // Normal: rotate only
                            let nx = v.normal[0];
                            let ny = v.normal[1];
                            let nz = v.normal[2];
                            rv.normal = [
                                nx * m[0][0] + ny * m[1][0] + nz * m[2][0],
                                nx * m[0][1] + ny * m[1][1] + nz * m[2][1],
                                nx * m[0][2] + ny * m[1][2] + nz * m[2][2],
                            ];
                            // Tangent: rotate xyz, preserve w
                            let tx = v.tangent[0];
                            let ty = v.tangent[1];
                            let tz = v.tangent[2];
                            rv.tangent = [
                                tx * m[0][0] + ty * m[1][0] + tz * m[2][0],
                                tx * m[0][1] + ty * m[1][1] + tz * m[2][1],
                                tx * m[0][2] + ty * m[1][2] + tz * m[2][2],
                                v.tangent[3],
                            ];
                        }
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

            // For HW1, embed the UnivertPacker in the section.
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
                lod_near_distance: lod_near,
                lod_far_distance: lod_far,
                lod_fade_distance: lod_fade,
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

    // When targeting HW2, ensure all materials are Hogan format.
    // glTF files edited in third-party tools (Blender etc.) lose the
    // ugx_hogan extras, so the importer produces Legacy materials.
    // The engine requires Hogan shader permutations to render HW2 models.
    if options.version == UgxVersion::Hw2
        && geom
            .materials
            .iter()
            .any(|m| matches!(&m.data, MaterialData::Legacy(_)))
    {
        geom.materials = convert_geom_materials(&geom, true);
    }

    Ok(geom)
}
