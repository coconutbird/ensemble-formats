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
    Error, GrannyBone, GrannyMesh, MAX_UV, Result, Section, UgxGeom, UnivertPacker, UnpackedVertex,
    VertexElementType,
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
}

impl Default for GltfImportOptions {
    fn default() -> Self {
        Self {
            include_skeleton: true,
            include_materials: true,
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
    let mut mesh_infos: Vec<(String, usize, usize, usize, usize)> = Vec::new();

    let has_skeleton = !bones.is_empty();

    for (mesh_idx, mesh) in root.meshes.iter().enumerate() {
        let mesh_name = mesh
            .name
            .clone()
            .unwrap_or_else(|| format!("mesh_{}", mesh_idx));
        let mesh_start_vertex = all_vertices.len();
        let mesh_start_section = sections.len();

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

            // Build pack order - game format: PNA0ST0 (skin before texcoords for skinned meshes)
            // For rigid meshes: PNA0T0 (no skin data)
            let mut pack_order = String::from("PN");
            if has_tangents {
                pack_order.push_str("A0");
            }
            // Skin data comes before texcoords in game format
            if has_skin {
                pack_order.push('S');
            }
            for i in 0..max_texcoords {
                pack_order.push('T');
                pack_order.push(char::from_digit(i as u32, 10).unwrap_or('0'));
            }
            if has_colors {
                pack_order.push('D');
            }

            // Use compact vertex types matching game format
            let mut uv_types = [VertexElementType::Ignore; MAX_UV];
            for uv_type in uv_types.iter_mut().take(max_texcoords.min(MAX_UV)) {
                *uv_type = VertexElementType::HalfFloat2; // Game uses HalfFloat2 for UVs
            }

            // UnivertPacker with game-standard defaults for ALL fields.
            // The game sets these defaults regardless of whether they're used in pack_order:
            //   setPos(eHALFFLOAT4), setNorm(eDEC3N), setBasis(eDEC3N), setBasisScales(eHALFFLOAT2)
            //   setTangent(eDEC3N), setIndices(eUBYTE4), setWeights(eUBYTE4N), setDiffuse(eD3DCOLOR)
            //   setUV(eHALFFLOAT2)
            // The actual marine_01.ugx uses Float3 for tangent/basis/normal instead of DEC3N.
            // We match the original file values exactly.
            let packer = UnivertPacker {
                pack_order: pack_order.clone(),
                decl_order: String::new(), // Original has empty decl_order
                pos_type: VertexElementType::HalfFloat4,
                basis_type: VertexElementType::Float3, // Game default, even if unused
                basis_scale_type: VertexElementType::Float2,
                tangent_type: VertexElementType::Float3, // Game default, even if unused
                normal_type: VertexElementType::Float3,
                uv_types,
                indices_type: VertexElementType::UByte4, // Game default, even if unused
                weights_type: VertexElementType::UByte4N, // Game default, even if unused
                diffuse_type: VertexElementType::D3DColor, // Game default, even if unused
                index_type: VertexElementType::Ignore,
            };

            // Analyze bone usage for this section
            // Detect global_bones sections:
            // If ALL vertices have weight[0]=1.0 and weights[1..3]=0.0 on the SAME bone,
            // this was originally a global_bones=true section with zero weights.
            // glTF export transforms zero-weight vertices to weight=1.0 on rigid bone.
            //
            // We detect this pattern and restore the original behavior:
            // - global_bones=true
            // - Zero weights on all vertices
            // - Pack order without skin data (PNT0 instead of PNST0)
            //
            // NOTE: max_bones is the maximum number of bone influences on ANY SINGLE VERTEX,
            // NOT the total unique bones in the section. This controls shader selection:
            // - max_bones=1 → ONE_BONE_REG=true (single bone optimization)
            // - max_bones=2 → neither flag set (2 bones)
            // - max_bones>2 → FOUR_BONES_REG=true (4 bones)
            let (is_global_bones, global_bone_idx, actual_max_bones) = if has_skin {
                let mut all_single_bone = true;
                let mut common_bone: Option<u16> = None;
                let mut max_influences_per_vertex = 0i32;

                for v in &vertices {
                    // Count how many non-zero weights this vertex has
                    let mut num_influences = 0;
                    for k in 0..4 {
                        if v.bone_weights[k] > 0.0 {
                            num_influences += 1;
                        }
                    }
                    max_influences_per_vertex = max_influences_per_vertex.max(num_influences);

                    // Check if this vertex has exactly weight[0]=1.0 and rest=0.0
                    let is_single_bone_vertex = v.bone_weights[0] > 0.99
                        && v.bone_weights[1] < 0.01
                        && v.bone_weights[2] < 0.01
                        && v.bone_weights[3] < 0.01;

                    if is_single_bone_vertex {
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

                let max_bones = max_influences_per_vertex.max(1);

                // If all vertices use the same single bone, this is a global_bones section
                if let Some(bone) = common_bone.filter(|_| all_single_bone) {
                    // Convert 1-based bone index to 0-based for rigid_bone_index
                    let bone_idx = (bone as i32) - 1;
                    (true, bone_idx, 1)
                } else {
                    (false, i32::MAX, max_bones)
                }
            } else {
                (false, i32::MAX, 1)
            };

            // For global_bones sections, restore zero weights and use simpler pack order
            let (final_packer, final_vertices) = if is_global_bones {
                // Rebuild packer without skin data for global_bones sections
                let mut global_pack_order = String::from("PN");
                if has_tangents {
                    global_pack_order.push_str("A0");
                }
                for i in 0..max_texcoords {
                    global_pack_order.push('T');
                    global_pack_order.push(char::from_digit(i as u32, 10).unwrap_or('0'));
                }
                if has_colors {
                    global_pack_order.push('D');
                }

                // Global bones packer - pack_order is PNT0 (no skin data), but we still
                // set game-standard defaults for ALL fields to match original file
                let global_packer = UnivertPacker {
                    pack_order: global_pack_order,
                    decl_order: String::new(), // Original has empty decl_order
                    pos_type: VertexElementType::HalfFloat4,
                    basis_type: VertexElementType::Float3, // Game default
                    basis_scale_type: VertexElementType::Float2,
                    tangent_type: VertexElementType::Float3, // Game default
                    normal_type: VertexElementType::Float3,
                    uv_types,
                    indices_type: VertexElementType::UByte4, // Game default, even for PNT0
                    weights_type: VertexElementType::UByte4N, // Game default, even for PNT0
                    diffuse_type: VertexElementType::D3DColor, // Game default
                    index_type: VertexElementType::Ignore,
                };

                // Restore zero weights for global_bones vertices
                let restored_vertices: Vec<UnpackedVertex> = vertices
                    .iter()
                    .map(|v| {
                        let mut rv = v.clone();
                        rv.bone_weights = [0.0, 0.0, 0.0, 0.0];
                        rv.bone_indices = [0, 0, 0, 0];
                        rv
                    })
                    .collect();

                (global_packer, restored_vertices)
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
                base_vert_packer: Some(final_packer),
                bone_remap: Vec::new(),
                // rigid_only is always false from glTF import
                // global_bones is true for sections where all vertices use same bone
                rigid_only: false,
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
        bone_bounds,
        sections,
        accessories: Vec::new(),
        valid_accessories: Vec::new(),
        vertex_buffer: all_vertex_buffer,
        index_buffer: all_index_buffer,
        rigid_only: all_rigid,
        rigid_bone_index: 0,
        max_instances: 1,
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
/// For glTF imports, we analyze which bones each vertex uses (via bone_weights > 0)
/// and create one mesh per glTF mesh, preserving the original mesh names.
/// For global_bones sections (zero weights), we use the rigid_bone_index instead.
/// This allows the game to properly skin the vertices.
fn generate_granny_meshes_from_vertices(
    vertices: &[UnpackedVertex],
    granny_bones: &[GrannyBone],
    mesh_infos: &[(String, usize, usize, usize, usize)], // (name, start_vertex, end_vertex, start_section, end_section)
    sections: &[Section],
) -> Vec<GrannyMesh> {
    let mut granny_meshes = Vec::new();

    for (mesh_name, start_vertex, end_vertex, start_section, end_section) in mesh_infos {
        // Collect all unique bone indices used by this mesh's vertices
        let mut used_bones: std::collections::BTreeSet<u16> = std::collections::BTreeSet::new();

        for v in &vertices[*start_vertex..*end_vertex] {
            for k in 0..4 {
                // bone_indices are 1-based, bone_weights[k] > 0 means the bone is used
                if v.bone_weights[k] > 0.0 && v.bone_indices[k] > 0 {
                    used_bones.insert(v.bone_indices[k]);
                }
            }
        }

        // For global_bones sections, vertices have zero weights but use rigid_bone_index
        // We need to include that bone in the mesh bindings
        for section in &sections[*start_section..*end_section] {
            if section.global_bones && section.rigid_bone_index >= 0 {
                // rigid_bone_index is 0-based, convert to 1-based for the set
                let bone_idx_1based = (section.rigid_bone_index as u16) + 1;
                used_bones.insert(bone_idx_1based);
            }
        }

        if used_bones.is_empty() {
            // No bones used in this mesh - skip it (fully rigid mesh with no bone reference)
            continue;
        }

        // Convert bone indices to bone names
        // bone_indices are 1-based, so subtract 1 to get the granny_bones index
        let bone_bindings: Vec<String> = used_bones
            .iter()
            .filter_map(|&idx| {
                let idx_0based = (idx as usize).saturating_sub(1);
                granny_bones.get(idx_0based).map(|b| b.name.clone())
            })
            .collect();

        if !bone_bindings.is_empty() {
            granny_meshes.push(GrannyMesh {
                name: mesh_name.clone(),
                bone_bindings,
            });
        }
    }

    granny_meshes
}
