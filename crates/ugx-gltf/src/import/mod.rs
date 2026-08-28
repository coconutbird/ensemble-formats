//! glTF import for UGX models.
//!
//! The exporter writes DirectX row-major matrices as flat rows into glTF's
//! column-major storage. Import reads those 16 values back as DirectX rows.

mod accessor;
mod bounds;
mod material;
mod mesh;
mod primitive;
mod skeleton;

use ugx::types::MaterialData;
use ugx::types::convert::convert_geom_materials;
use ugx::{Error, GeometryFlags, Matrix4x4, Result, Section, UgxGeom, UgxVersion, UnpackedVertex};

use crate::extras::{MeshExtrasJson, SceneExtrasJson};
use accessor::resolve_buffer;
use bounds::compute_bounds;
use material::import_materials;
use mesh::{MeshInfo, build_packer, detect_global_bones, generate_granny_meshes_from_vertices};
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
    /// - `Hw2`: `HalfFloat4` positions, `Dec3N` normals/tangents, PT0NA0S byte order,
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

struct MeshNodeInfo {
    has_skin: bool,
    parent_bone_index: Option<usize>,
}

#[derive(Default)]
struct ImportedMeshes {
    vertices: Vec<UnpackedVertex>,
    vertex_buffer: Vec<u8>,
    index_buffer: Vec<u16>,
    sections: Vec<Section>,
    mesh_info: Vec<MeshInfo>,
}

struct PrimitiveFeatures {
    has_tangents: bool,
    has_skin: bool,
    has_colors: bool,
    max_texcoords: usize,
}

struct SectionKind {
    global_bones: bool,
    rigid_only: bool,
    bone_index: i32,
    max_bones: i32,
}

struct ImportContext<'a> {
    root: &'a gltf_json::Root,
    buffer: &'a [u8],
    options: &'a GltfImportOptions,
    bones: &'a [ugx::Bone],
    mesh_nodes: &'a std::collections::HashMap<usize, MeshNodeInfo>,
    world_matrices: &'a [Matrix4x4],
}

struct SectionInput<'a> {
    vertices: Vec<UnpackedVertex>,
    indices: &'a [u16],
    material_index: i32,
    packer: &'a ugx::UnivertPacker,
    kind: &'a SectionKind,
    version: UgxVersion,
    extras: Option<&'a MeshExtrasJson>,
}

/// Import a glTF document and optional binary buffer into a `UgxGeom`.
///
/// Embedded base64 buffers are decoded when `buffer_data` is `None`.
///
/// # Errors
///
/// Returns an error for malformed JSON, invalid or out-of-range glTF data,
/// unrepresentable UGX offsets/counts, or derived-data rebuild failures.
pub fn import_from_gltf(
    json: &str,
    buffer_data: Option<&[u8]>,
    options: &GltfImportOptions,
) -> Result<UgxGeom> {
    let root: gltf_json::Root = serde_json::from_str(json)
        .map_err(|error| Error::UnsupportedFormat(format!("Invalid glTF JSON: {error}")))?;
    let buffer = resolve_buffer(&root, buffer_data)?;
    let (bones, granny_bones) = if options.include_skeleton {
        import_skeleton(&root, &buffer)?
    } else {
        (Vec::new(), Vec::new())
    };
    let materials = if options.include_materials {
        import_materials(&root)?
    } else {
        Vec::new()
    };
    let mesh_nodes = build_mesh_node_map(&root);
    let world_matrices = granny_bones
        .iter()
        .map(|bone| {
            bone.inverse_world_matrix
                .inverse()
                .unwrap_or_else(Matrix4x4::identity)
        })
        .collect::<Vec<_>>();
    let imported = import_meshes(
        &root,
        &buffer,
        options,
        &bones,
        &mesh_nodes,
        &world_matrices,
    )?;
    finish_geometry(&root, options, bones, granny_bones, materials, imported)
}

fn build_mesh_node_map(root: &gltf_json::Root) -> std::collections::HashMap<usize, MeshNodeInfo> {
    let mut node_parent = std::collections::HashMap::new();
    for (parent_index, node) in root.nodes.iter().enumerate() {
        if let Some(children) = &node.children {
            for child in children {
                node_parent.insert(child.value(), parent_index);
            }
        }
    }
    let joint_nodes: std::collections::HashSet<_> = root
        .skins
        .first()
        .map(|skin| skin.joints.iter().map(gltf_json::Index::value).collect())
        .unwrap_or_default();
    root.nodes
        .iter()
        .enumerate()
        .filter_map(|(node_index, node)| {
            let mesh_index = node.mesh?.value();
            let has_skin = node.skin.is_some();
            let parent_bone_index = (!has_skin)
                .then(|| node_parent.get(&node_index).copied())
                .flatten()
                .filter(|parent| joint_nodes.contains(parent))
                .and_then(|parent| {
                    root.skins
                        .first()?
                        .joints
                        .iter()
                        .position(|joint| joint.value() == parent)
                });
            Some((
                mesh_index,
                MeshNodeInfo {
                    has_skin,
                    parent_bone_index,
                },
            ))
        })
        .collect()
}

fn import_meshes(
    root: &gltf_json::Root,
    buffer: &[u8],
    options: &GltfImportOptions,
    bones: &[ugx::Bone],
    mesh_nodes: &std::collections::HashMap<usize, MeshNodeInfo>,
    world_matrices: &[Matrix4x4],
) -> Result<ImportedMeshes> {
    let context = ImportContext {
        root,
        buffer,
        options,
        bones,
        mesh_nodes,
        world_matrices,
    };
    let mut imported = ImportedMeshes::default();
    for (mesh_index, mesh) in root.meshes.iter().enumerate() {
        import_mesh(&context, mesh_index, mesh, &mut imported)?;
    }
    Ok(imported)
}

fn import_mesh(
    context: &ImportContext<'_>,
    mesh_index: usize,
    mesh: &gltf_json::Mesh,
    imported: &mut ImportedMeshes,
) -> Result<()> {
    let name = mesh
        .name
        .clone()
        .unwrap_or_else(|| format!("mesh_{mesh_index}"));
    let start_vertex = imported.vertices.len();
    let start_section = imported.sections.len();
    let extras: Option<MeshExtrasJson> = mesh
        .extras
        .as_ref()
        .and_then(|raw| serde_json::from_str(raw.get()).ok());
    for primitive in &mesh.primitives {
        import_mesh_primitive(
            context,
            context.mesh_nodes.get(&mesh_index),
            primitive,
            extras.as_ref(),
            imported,
        )?;
    }
    let end_vertex = imported.vertices.len();
    if end_vertex > start_vertex {
        imported.mesh_info.push((
            name,
            start_vertex,
            end_vertex,
            start_section,
            imported.sections.len(),
            extras.and_then(|value| value.granny_mesh_index),
        ));
    }
    Ok(())
}

fn import_mesh_primitive(
    context: &ImportContext<'_>,
    mesh_node: Option<&MeshNodeInfo>,
    primitive: &gltf_json::mesh::Primitive,
    extras: Option<&MeshExtrasJson>,
    imported: &mut ImportedMeshes,
) -> Result<()> {
    let (vertices, indices, material_index) = import_primitive(
        primitive,
        context.root,
        context.buffer,
        !context.bones.is_empty(),
        context.bones.len(),
    )?;
    if vertices.is_empty() || indices.is_empty() {
        return Ok(());
    }
    let features = primitive_features(&vertices, !context.bones.is_empty());
    let kind = classify_section(&vertices, features.has_skin, mesh_node)?;
    let (packer, final_vertices) = prepare_section_vertices(
        vertices,
        &features,
        &kind,
        context.options.version,
        context.world_matrices,
    );
    append_section(
        imported,
        SectionInput {
            vertices: final_vertices,
            indices: &indices,
            material_index,
            packer: &packer,
            kind: &kind,
            version: context.options.version,
            extras,
        },
    )
}

fn primitive_features(vertices: &[UnpackedVertex], has_skeleton: bool) -> PrimitiveFeatures {
    let has_tangents = vertices.iter().any(|vertex| {
        vertex.tangent[..3]
            .iter()
            .any(|value| value.abs() > f32::EPSILON)
    });
    let has_skin = has_skeleton
        && vertices
            .iter()
            .any(|vertex| vertex.bone_weights.iter().sum::<f32>() > f32::EPSILON);
    let has_colors = vertices.iter().any(|vertex| {
        vertex
            .diffuse
            .iter()
            .any(|value| value.abs() > f32::EPSILON)
    });
    PrimitiveFeatures {
        has_tangents,
        has_skin,
        has_colors,
        max_texcoords: vertices
            .iter()
            .map(|vertex| vertex.num_texcoords)
            .max()
            .unwrap_or(0),
    }
}

fn classify_section(
    vertices: &[UnpackedVertex],
    has_skin: bool,
    mesh_node: Option<&MeshNodeInfo>,
) -> Result<SectionKind> {
    if let Some(bone_index) =
        mesh_node.and_then(|info| (!info.has_skin).then_some(info.parent_bone_index).flatten())
    {
        return Ok(SectionKind {
            global_bones: true,
            rigid_only: true,
            bone_index: checked_i32(bone_index, "rigid bone index")?,
            max_bones: 1,
        });
    }
    if mesh_node.is_some_and(|info| info.has_skin) {
        let max_bones = vertices
            .iter()
            .map(|vertex| {
                vertex
                    .bone_weights
                    .iter()
                    .filter(|&&weight| weight > 0.0)
                    .count()
            })
            .max()
            .unwrap_or(1)
            .max(1);
        return Ok(SectionKind {
            global_bones: false,
            rigid_only: false,
            bone_index: i32::MAX,
            max_bones: checked_i32(max_bones, "maximum bone influences")?,
        });
    }
    let (global_bones, rigid_only, bone_index, max_bones) = detect_global_bones(vertices, has_skin);
    Ok(SectionKind {
        global_bones,
        rigid_only,
        bone_index,
        max_bones,
    })
}

fn prepare_section_vertices(
    vertices: Vec<UnpackedVertex>,
    features: &PrimitiveFeatures,
    kind: &SectionKind,
    version: UgxVersion,
    world_matrices: &[Matrix4x4],
) -> (ugx::UnivertPacker, Vec<UnpackedVertex>) {
    let rigid = kind.global_bones || kind.rigid_only;
    let packer = build_packer(
        version,
        features.max_texcoords,
        features.has_tangents,
        features.has_skin && !rigid,
        features.has_colors,
    );
    if !rigid {
        return (packer, vertices);
    }
    let matrix = usize::try_from(kind.bone_index)
        .ok()
        .and_then(|index| world_matrices.get(index));
    let restored = vertices
        .iter()
        .map(|vertex| restore_rigid_vertex(vertex, matrix))
        .collect();
    (packer, restored)
}

fn restore_rigid_vertex(vertex: &UnpackedVertex, matrix: Option<&Matrix4x4>) -> UnpackedVertex {
    let mut restored = vertex.clone();
    if let Some(matrix) = matrix {
        restored.position = transform_point(vertex.position, matrix);
        restored.normal = transform_direction(vertex.normal, matrix);
        let tangent = transform_direction(
            [vertex.tangent[0], vertex.tangent[1], vertex.tangent[2]],
            matrix,
        );
        restored.tangent = [tangent[0], tangent[1], tangent[2], vertex.tangent[3]];
    }
    restored.bone_weights = [0.0; 4];
    restored.bone_indices = [0; 4];
    restored
}

fn transform_point(value: [f32; 3], matrix: &Matrix4x4) -> [f32; 3] {
    let rows = &matrix.rows;
    [
        value[0] * rows[0][0] + value[1] * rows[1][0] + value[2] * rows[2][0] + rows[3][0],
        value[0] * rows[0][1] + value[1] * rows[1][1] + value[2] * rows[2][1] + rows[3][1],
        value[0] * rows[0][2] + value[1] * rows[1][2] + value[2] * rows[2][2] + rows[3][2],
    ]
}

fn transform_direction(value: [f32; 3], matrix: &Matrix4x4) -> [f32; 3] {
    let rows = &matrix.rows;
    [
        value[0] * rows[0][0] + value[1] * rows[1][0] + value[2] * rows[2][0],
        value[0] * rows[0][1] + value[1] * rows[1][1] + value[2] * rows[2][1],
        value[0] * rows[0][2] + value[1] * rows[1][2] + value[2] * rows[2][2],
    ]
}

fn append_section(imported: &mut ImportedMeshes, input: SectionInput<'_>) -> Result<()> {
    let vertex_start = imported.vertex_buffer.len();
    let vertex_offset = checked_i32(vertex_start, "vertex-buffer offset")?;
    for vertex in &input.vertices {
        input
            .packer
            .pack_vertex(&mut imported.vertex_buffer, vertex, input.version.pos_w());
    }
    let vertex_bytes = checked_i32(
        imported.vertex_buffer.len() - vertex_start,
        "section vertex-buffer size",
    )?;
    let index_offset = checked_i32(imported.index_buffer.len(), "index-buffer offset")?;
    imported.index_buffer.extend_from_slice(input.indices);
    imported.sections.push(Section {
        material_index: input.material_index,
        accessory_index: 0,
        max_bones: input.kind.max_bones,
        rigid_bone_index: input.kind.bone_index,
        ib_offset: index_offset,
        num_tris: checked_i32(input.indices.len() / 3, "section triangle count")?,
        vb_offset: vertex_offset,
        vb_bytes: vertex_bytes,
        vert_size: checked_i32(input.packer.vertex_size(), "packed vertex size")?,
        num_verts: checked_i32(input.vertices.len(), "section vertex count")?,
        base_vert_packer: (input.version == UgxVersion::Hw1).then(|| input.packer.clone()),
        bone_remap: Vec::new(),
        rigid_only: input.kind.rigid_only,
        global_bones: input.kind.global_bones,
        lod_near_distance: input.extras.map_or(0.0, |value| value.lod_near_distance),
        lod_far_distance: input
            .extras
            .map_or(f32::MAX, |value| value.lod_far_distance),
        lod_fade_distance: input.extras.map_or(0.0, |value| value.lod_fade_distance),
    });
    imported.vertices.extend(input.vertices);
    Ok(())
}

fn finish_geometry(
    root: &gltf_json::Root,
    options: &GltfImportOptions,
    bones: Vec<ugx::Bone>,
    granny_bones: Vec<ugx::GrannyBone>,
    materials: Vec<ugx::Material>,
    imported: ImportedMeshes,
) -> Result<UgxGeom> {
    let (bounds, bounding_sphere) = compute_bounds(&imported.vertices);
    let bone_bounds = bones.iter().map(|_| bounds.clone()).collect();
    let all_rigid = imported.sections.iter().all(|section| section.rigid_only);
    let all_skinned = imported.sections.iter().all(|section| !section.rigid_only);
    let global_bones = imported.sections.iter().any(|section| section.global_bones);
    let granny_meshes = generate_granny_meshes_from_vertices(
        &imported.vertices,
        &granny_bones,
        &imported.mesh_info,
        &imported.sections,
    );
    let max_vertex_count = imported
        .sections
        .iter()
        .map(|section| {
            u32::try_from(section.num_verts)
                .map_err(|_| Error::UnsupportedFormat("Section has a negative vertex count".into()))
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .max()
        .unwrap_or(1);
    let multiplier = max_vertex_count
        .checked_next_power_of_two()
        .ok_or(Error::SizeOverflow("instance-index multiplier"))?;
    let mut geometry = UgxGeom {
        bounding_sphere,
        bounds,
        materials,
        bones,
        granny_bones,
        granny_meshes,
        skeleton_lod_type: 0,
        bone_bounds,
        sections: imported.sections,
        accessories: Vec::new(),
        valid_accessories: Vec::new(),
        vertex_buffer: imported.vertex_buffer,
        index_buffer: imported.index_buffer,
        rigid_only: all_rigid,
        rigid_bone_index: 0,
        max_instances: scene_max_instances(root),
        instance_index_multiplier: i16::try_from(multiplier)
            .map_err(|_| Error::SizeOverflow("instance-index multiplier"))?,
        large_geom_bone_index: i16::MAX,
        flags: GeometryFlags {
            all_sections_rigid: all_rigid,
            all_sections_skinned: !global_bones && !all_rigid && all_skinned,
            global_bones,
        },
        aabb_tree: None,
    };
    geometry.rebuild_derived_data()?;
    convert_materials_for_version(&mut geometry, options.version);
    Ok(geometry)
}

fn scene_max_instances(root: &gltf_json::Root) -> i16 {
    root.scenes
        .first()
        .and_then(|scene| scene.extras.as_ref())
        .and_then(|raw| serde_json::from_str::<SceneExtrasJson>(raw.get()).ok())
        .map_or(1, |extras| extras.ugx_max_instances)
}

fn convert_materials_for_version(geometry: &mut UgxGeom, version: UgxVersion) {
    let convert_to_hogan = version == UgxVersion::Hw2
        && geometry
            .materials
            .iter()
            .any(|material| matches!(&material.data, MaterialData::Legacy(_)));
    let convert_to_legacy = version == UgxVersion::Hw1
        && geometry
            .materials
            .iter()
            .any(|material| matches!(&material.data, MaterialData::Hogan(_)));
    if convert_to_hogan {
        geometry.materials = convert_geom_materials(geometry, true);
    } else if convert_to_legacy {
        geometry.materials = convert_geom_materials(geometry, false);
    }
}

fn checked_i32(value: usize, context: &'static str) -> Result<i32> {
    i32::try_from(value).map_err(|_| Error::SizeOverflow(context))
}
