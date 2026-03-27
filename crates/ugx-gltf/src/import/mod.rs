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

    for (mesh_idx, mesh) in root.meshes.iter().enumerate() {
        let mesh_name = mesh
            .name
            .clone()
            .unwrap_or_else(|| format!("mesh_{}", mesh_idx));
        let mesh_start_vertex = all_vertices.len();
        let mesh_start_section = sections.len();

        // Read section flags from mesh extras if present (written by our exporter).
        // None means third-party glTF (Blender etc.) → fall back to heuristic.
        let mesh_ext: Option<MeshExtrasJson> = mesh
            .extras
            .as_ref()
            .and_then(|raw| serde_json::from_str(raw.get()).ok());
        let extras_global_bones: Option<bool> = mesh_ext.as_ref().and_then(|e| e.ugx_global_bones);
        let extras_rigid_only: Option<bool> = mesh_ext.as_ref().and_then(|e| e.ugx_rigid_only);
        let extras_rigid_bone_index: Option<i32> =
            mesh_ext.as_ref().and_then(|e| e.ugx_rigid_bone_index);
        let extras_granny_mesh_index: Option<usize> =
            mesh_ext.as_ref().and_then(|e| e.ugx_granny_mesh_index);

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
