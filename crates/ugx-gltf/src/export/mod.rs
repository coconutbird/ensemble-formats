//! glTF export for UGX models.
//!
//! UGX stores matrices using DirectX row-major, row-vector conventions, while
//! glTF uses column-major, column-vector conventions. Writing DirectX matrix
//! rows flat produces the required glTF column-major representation of the
//! transposed matrix.

mod material;
mod primitive;
mod skeleton;

use base64::{Engine, engine::general_purpose::STANDARD};
use gltf_json as json;
use ugx::{Error, Matrix4x4, Result, Section, UgxGeom, UnpackedVertex};

use crate::extras::{MeshExtrasJson, SceneExtrasJson, SectionModeJson, to_raw_value};
use material::build_materials;
use primitive::{PrimitiveInput, PrimitiveOutput, create_primitive};
use skeleton::{
    build_section_to_mesh_mapping, create_skeleton_nodes, create_skeleton_nodes_from_granny,
};

/// glTF export options.
#[derive(Debug, Clone)]
pub struct GltfExportOptions {
    /// Embed buffer data as base64 in the .gltf file (default: true).
    pub embed_buffers: bool,
    /// Include materials (default: true).
    pub include_materials: bool,
    /// Include skeleton/bones (default: true).
    pub include_skeleton: bool,
}

impl GltfExportOptions {
    #[must_use]
    pub fn new() -> Self {
        Self {
            embed_buffers: true,
            include_materials: true,
            include_skeleton: true,
        }
    }
}

impl Default for GltfExportOptions {
    fn default() -> Self {
        Self::new()
    }
}

/// Export result containing the glTF JSON and optional binary buffer.
#[derive(Debug)]
pub struct GltfExport {
    /// The glTF JSON document.
    pub json: String,
    /// Binary buffer data (if not embedded).
    pub buffer: Option<Vec<u8>>,
}

struct SkeletonInfo {
    use_granny_bones: bool,
    has_skeleton: bool,
    bone_count: usize,
    model_to_bone: Vec<Matrix4x4>,
}

impl SkeletonInfo {
    fn new(geometry: &UgxGeom, options: &GltfExportOptions) -> Self {
        let use_granny_bones = options.include_skeleton && !geometry.granny_bones.is_empty();
        let has_skeleton =
            options.include_skeleton && (!geometry.bones.is_empty() || use_granny_bones);
        let (bone_count, model_to_bone) = if use_granny_bones {
            (
                geometry.granny_bones.len(),
                geometry
                    .granny_bones
                    .iter()
                    .map(|bone| bone.inverse_world_matrix.clone())
                    .collect(),
            )
        } else {
            (
                geometry.bones.len(),
                geometry
                    .bones
                    .iter()
                    .map(|bone| bone.model_to_bone.clone())
                    .collect(),
            )
        };
        Self {
            use_granny_bones,
            has_skeleton,
            bone_count,
            model_to_bone,
        }
    }
}

struct SectionExportInfo {
    mesh_index: usize,
    rigid_parent_bone: Option<usize>,
}

#[derive(Default)]
struct ExportContent {
    buffer_data: Vec<u8>,
    accessors: Vec<json::Accessor>,
    buffer_views: Vec<json::buffer::View>,
    meshes: Vec<json::Mesh>,
    materials: Vec<json::Material>,
    images: Vec<json::Image>,
    textures: Vec<json::Texture>,
    section_info: Vec<SectionExportInfo>,
}

struct NodeContent {
    nodes: Vec<json::Node>,
    skins: Vec<json::Skin>,
    scene_nodes: Vec<json::Index<json::scene::Node>>,
}

/// Export UGX geometry to glTF format.
///
/// Uses `buffer.bin` as the external buffer filename when buffers are not embedded.
///
/// # Errors
///
/// Returns an error if geometry data is malformed, generated indices overflow
/// glTF representation limits, or the JSON document cannot be serialized.
pub fn export_to_gltf(geometry: &UgxGeom, options: &GltfExportOptions) -> Result<GltfExport> {
    export_to_gltf_with_buffer_name(geometry, options, "buffer.bin")
}

/// Export UGX geometry to glTF with a specific external buffer filename.
///
/// # Errors
///
/// Returns an error if geometry data is malformed, generated indices overflow
/// glTF representation limits, or the JSON document cannot be serialized.
pub fn export_to_gltf_with_buffer_name(
    geometry: &UgxGeom,
    options: &GltfExportOptions,
    buffer_name: &str,
) -> Result<GltfExport> {
    let skeleton = SkeletonInfo::new(geometry, options);
    let mut content = ExportContent::default();
    if options.include_materials {
        let built = build_materials(&geometry.materials)?;
        content.materials = built.materials;
        content.images = built.images;
        content.textures = built.textures;
    }
    export_sections(geometry, options, &skeleton, &mut content)?;
    let node_content = build_nodes(geometry, &skeleton, &mut content)?;
    finish_export(geometry, options, buffer_name, content, node_content)
}

fn export_sections(
    geometry: &UgxGeom,
    options: &GltfExportOptions,
    skeleton: &SkeletonInfo,
    content: &mut ExportContent,
) -> Result<()> {
    let section_to_mesh = build_section_to_mesh_mapping(geometry, &geometry.granny_bones);
    for (section_index, section) in geometry.sections.iter().enumerate() {
        let vertices = geometry.unpack_section_vertices(section_index)?;
        let indices = geometry.get_section_indices(section_index)?;
        if vertices.is_empty() || indices.is_empty() {
            continue;
        }
        let rigid_parent = rigid_parent_bone(section, skeleton.bone_count);
        let export_vertices = prepare_vertices(&vertices, rigid_parent, &skeleton.model_to_bone)?;
        let mesh_index = section_to_mesh.get(section_index).copied().ok_or_else(|| {
            Error::UnsupportedFormat("Section-to-mesh mapping is incomplete".into())
        })?;
        let primitive = create_primitive(
            &PrimitiveInput {
                vertices: &export_vertices,
                indices: &indices,
                material_index: section.material_index,
                has_materials: options.include_materials && !content.materials.is_empty(),
                has_skeleton: skeleton.has_skeleton && rigid_parent.is_none(),
                bone_count: skeleton.bone_count,
                rigid_bone_index: section.rigid_bone_index,
                bone_remap: &section.bone_remap,
            },
            &mut PrimitiveOutput {
                buffer_data: &mut content.buffer_data,
                accessors: &mut content.accessors,
                buffer_views: &mut content.buffer_views,
            },
        )?;
        content.section_info.push(SectionExportInfo {
            mesh_index: content.meshes.len(),
            rigid_parent_bone: rigid_parent,
        });
        content.meshes.push(json::Mesh {
            extensions: None,
            extras: build_mesh_extras(geometry, section, section_index, mesh_index)?,
            name: Some(mesh_name(geometry, mesh_index, section_index)),
            primitives: vec![primitive],
            weights: None,
        });
    }
    Ok(())
}

fn rigid_parent_bone(section: &Section, bone_count: usize) -> Option<usize> {
    if !section.global_bones && !section.rigid_only {
        return None;
    }
    usize::try_from(section.rigid_bone_index)
        .ok()
        .filter(|&index| index < bone_count)
}

fn prepare_vertices(
    vertices: &[UnpackedVertex],
    rigid_parent: Option<usize>,
    matrices: &[Matrix4x4],
) -> Result<Vec<UnpackedVertex>> {
    let Some(bone_index) = rigid_parent else {
        return Ok(vertices.to_vec());
    };
    let matrix = matrices.get(bone_index).ok_or_else(|| {
        Error::UnsupportedFormat("Rigid section references a missing bone matrix".into())
    })?;
    let normal_matrix = matrix
        .inverse()
        .ok_or_else(|| Error::UnsupportedFormat("Rigid bone transform is singular".into()))?
        .transpose();
    Ok(vertices
        .iter()
        .map(|vertex| transform_rigid_vertex(vertex, matrix, &normal_matrix))
        .collect())
}

fn transform_rigid_vertex(
    vertex: &UnpackedVertex,
    matrix: &Matrix4x4,
    normal_matrix: &Matrix4x4,
) -> UnpackedVertex {
    let mut transformed = vertex.clone();
    transformed.position = transform_point(vertex.position, matrix);
    transformed.normal = transform_direction(vertex.normal, normal_matrix);
    let tangent = transform_direction(
        [vertex.tangent[0], vertex.tangent[1], vertex.tangent[2]],
        matrix,
    );
    transformed.tangent = [tangent[0], tangent[1], tangent[2], vertex.tangent[3]];
    transformed.bone_weights = [0.0; 4];
    transformed.bone_indices = [0; 4];
    transformed
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

fn mesh_name(geometry: &UgxGeom, mesh_index: usize, section_index: usize) -> String {
    geometry
        .granny_meshes
        .get(mesh_index)
        .map_or_else(|| format!("mesh_{section_index}"), |mesh| mesh.name.clone())
}

fn build_mesh_extras(
    geometry: &UgxGeom,
    section: &Section,
    section_index: usize,
    mesh_index: usize,
) -> Result<json::Extras> {
    let hw2_has_color = section.base_vert_packer.is_none()
        && section.external_vert_packer.as_ref().map_or_else(
            || {
                if section.rigid_only {
                    section.vert_size >= 24
                } else {
                    section.vert_size >= 32
                }
            },
            |packer| packer.pack_order.contains('D'),
        );
    let hw2_color_before_skin = geometry
        .infer_hw2_skin_order(section_index)?
        .is_some_and(|order| order == ugx::Hw2SkinOrder::ColorThenSkin);
    let section_mode = if section.global_bones {
        SectionModeJson::Global
    } else if section.rigid_only {
        SectionModeJson::Rigid
    } else {
        SectionModeJson::Skinned
    };
    let binding_bone = (section.global_bones || section.rigid_only)
        .then(|| usize::try_from(section.rigid_bone_index).ok())
        .flatten()
        .and_then(|index| geometry.bones.get(index))
        .map_or_else(String::new, |bone| bone.name.clone());
    let mut extras = MeshExtrasJson {
        hw2_has_color,
        hw2_color_before_skin,
        section_mode,
        binding_bone,
        max_bones: Some(section.max_bones),
        lod_near_distance: section.lod_near_distance,
        lod_far_distance: section.lod_far_distance,
        lod_fade_distance: section.lod_fade_distance,
        ..Default::default()
    };
    if let Some(mesh) = geometry.granny_meshes.get(mesh_index) {
        extras.granny_mesh_index = Some(mesh_index);
        let triangle_indices: std::collections::BTreeMap<_, _> = mesh
            .bone_bindings
            .iter()
            .filter(|binding| !binding.triangle_indices.is_empty())
            .map(|binding| (binding.bone_name.clone(), binding.triangle_indices.clone()))
            .collect();
        if !triangle_indices.is_empty() {
            extras.triangle_indices = Some(triangle_indices);
        }
    }
    Ok((!extras.is_empty())
        .then(|| to_raw_value(&extras))
        .flatten())
}

fn build_nodes(
    geometry: &UgxGeom,
    skeleton: &SkeletonInfo,
    content: &mut ExportContent,
) -> Result<NodeContent> {
    if !skeleton.has_skeleton {
        return build_unskinned_nodes(content);
    }
    let (mut nodes, inverse_bind_accessor) = if skeleton.use_granny_bones {
        create_skeleton_nodes_from_granny(
            &geometry.granny_bones,
            &mut content.buffer_data,
            &mut content.accessors,
            &mut content.buffer_views,
        )?
    } else {
        create_skeleton_nodes(
            &geometry.bones,
            &mut content.buffer_data,
            &mut content.accessors,
            &mut content.buffer_views,
        )?
    };
    let root_bones = root_bone_indices(geometry, skeleton)?;
    let bone_count = checked_u32(skeleton.bone_count, "glTF joint count")?;
    let joints = (0..bone_count).map(json::Index::new).collect();
    let skins = vec![json::Skin {
        extensions: None,
        extras: json::Extras::default(),
        inverse_bind_matrices: Some(json::Index::new(inverse_bind_accessor)),
        joints,
        name: Some("Armature".to_string()),
        skeleton: root_bones.first().copied().map(json::Index::new),
    }];
    append_skinned_mesh_nodes(content, skeleton, &mut nodes)?;
    attach_rigid_mesh_nodes(content, skeleton, &mut nodes)?;
    let mut scene_nodes: Vec<_> = root_bones.into_iter().map(json::Index::new).collect();
    let mesh_start = checked_u32(skeleton.bone_count, "mesh node start")?;
    for info in &content.section_info {
        if info.rigid_parent_bone.is_none() {
            let mesh_index = checked_u32(info.mesh_index, "mesh node index")?;
            let node_index = mesh_start
                .checked_add(mesh_index)
                .ok_or(Error::SizeOverflow("mesh node index"))?;
            scene_nodes.push(json::Index::new(node_index));
        }
    }
    Ok(NodeContent {
        nodes,
        skins,
        scene_nodes,
    })
}

fn root_bone_indices(geometry: &UgxGeom, skeleton: &SkeletonInfo) -> Result<Vec<u32>> {
    let parents: Box<dyn Iterator<Item = i32> + '_> = if skeleton.use_granny_bones {
        Box::new(geometry.granny_bones.iter().map(|bone| bone.parent_index))
    } else {
        Box::new(geometry.bones.iter().map(|bone| bone.parent_index))
    };
    parents
        .enumerate()
        .filter(|(_, parent)| *parent < 0)
        .map(|(index, _)| checked_u32(index, "root bone node index"))
        .collect()
}

fn append_skinned_mesh_nodes(
    content: &ExportContent,
    skeleton: &SkeletonInfo,
    nodes: &mut Vec<json::Node>,
) -> Result<()> {
    for (mesh_index, (mesh, info)) in content.meshes.iter().zip(&content.section_info).enumerate() {
        let skin = info
            .rigid_parent_bone
            .is_none()
            .then(|| json::Index::new(0));
        nodes.push(mesh_node(mesh, mesh_index, skin)?);
    }
    let expected = skeleton
        .bone_count
        .checked_add(content.meshes.len())
        .ok_or(Error::SizeOverflow("node count"))?;
    if nodes.len() != expected {
        return Err(Error::UnsupportedFormat(
            "Skeleton and mesh node counts are inconsistent".into(),
        ));
    }
    Ok(())
}

fn attach_rigid_mesh_nodes(
    content: &ExportContent,
    skeleton: &SkeletonInfo,
    nodes: &mut [json::Node],
) -> Result<()> {
    let mesh_start = checked_u32(skeleton.bone_count, "mesh node start")?;
    for info in &content.section_info {
        let Some(bone_index) = info.rigid_parent_bone else {
            continue;
        };
        let mesh_index = checked_u32(info.mesh_index, "rigid mesh node index")?;
        let node_index = mesh_start
            .checked_add(mesh_index)
            .ok_or(Error::SizeOverflow("rigid mesh node index"))?;
        let bone_node = nodes.get_mut(bone_index).ok_or_else(|| {
            Error::UnsupportedFormat("Rigid mesh parent bone is out of bounds".into())
        })?;
        bone_node
            .children
            .get_or_insert_with(Vec::new)
            .push(json::Index::new(node_index));
    }
    Ok(())
}

fn build_unskinned_nodes(content: &ExportContent) -> Result<NodeContent> {
    let nodes = content
        .meshes
        .iter()
        .enumerate()
        .map(|(index, mesh)| mesh_node(mesh, index, None))
        .collect::<Result<Vec<_>>>()?;
    let scene_nodes = (0..nodes.len())
        .map(|index| checked_u32(index, "scene node index").map(json::Index::new))
        .collect::<Result<Vec<_>>>()?;
    Ok(NodeContent {
        nodes,
        skins: Vec::new(),
        scene_nodes,
    })
}

fn mesh_node(
    mesh: &json::Mesh,
    mesh_index: usize,
    skin: Option<json::Index<json::Skin>>,
) -> Result<json::Node> {
    Ok(json::Node {
        camera: None,
        children: None,
        extensions: None,
        extras: json::Extras::default(),
        matrix: None,
        mesh: Some(json::Index::new(checked_u32(mesh_index, "mesh index")?)),
        name: Some(
            mesh.name
                .clone()
                .unwrap_or_else(|| format!("mesh_{mesh_index}")),
        ),
        rotation: None,
        scale: None,
        translation: None,
        skin,
        weights: None,
    })
}

fn finish_export(
    geometry: &UgxGeom,
    options: &GltfExportOptions,
    buffer_name: &str,
    content: ExportContent,
    node_content: NodeContent,
) -> Result<GltfExport> {
    let buffer = build_buffer(&content.buffer_data, options.embed_buffers, buffer_name)?;
    let scene = json::Scene {
        extensions: None,
        extras: to_raw_value(&SceneExtrasJson {
            ugx_max_instances: geometry.max_instances,
            ugx_granny_meshes: geometry
                .granny_meshes
                .iter()
                .map(crate::extras::GrannyMeshJson::from)
                .collect(),
        }),
        name: None,
        nodes: node_content.scene_nodes,
    };
    let mut root = json::Root {
        accessors: content.accessors,
        buffers: vec![buffer],
        buffer_views: content.buffer_views,
        meshes: content.meshes,
        nodes: node_content.nodes,
        scenes: vec![scene],
        scene: Some(json::Index::new(0)),
        skins: node_content.skins,
        materials: content.materials,
        images: content.images,
        textures: content.textures,
        ..Default::default()
    };
    root.asset = json::Asset {
        copyright: None,
        extensions: None,
        extras: json::Extras::default(),
        generator: Some("ugx-rs".to_string()),
        min_version: None,
        version: "2.0".to_string(),
    };
    let json = serde_json::to_string_pretty(&root)
        .map_err(|error| Error::UnsupportedFormat(format!("Cannot serialize glTF: {error}")))?;
    Ok(GltfExport {
        json,
        buffer: (!options.embed_buffers).then_some(content.buffer_data),
    })
}

fn build_buffer(data: &[u8], embedded: bool, buffer_name: &str) -> Result<json::Buffer> {
    let uri = if embedded {
        Some(format!(
            "data:application/octet-stream;base64,{}",
            STANDARD.encode(data)
        ))
    } else {
        Some(buffer_name.to_string())
    };
    Ok(json::Buffer {
        byte_length: json::validation::USize64(checked_u64(data.len(), "glTF buffer length")?),
        uri,
        extensions: None,
        extras: json::Extras::default(),
        name: None,
    })
}

fn checked_u32(value: usize, context: &'static str) -> Result<u32> {
    u32::try_from(value).map_err(|_| Error::SizeOverflow(context))
}

fn checked_u64(value: usize, context: &'static str) -> Result<u64> {
    u64::try_from(value).map_err(|_| Error::SizeOverflow(context))
}

#[cfg(test)]
mod tests;
