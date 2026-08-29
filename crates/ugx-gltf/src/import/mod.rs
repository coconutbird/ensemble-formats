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
use ugx::{
    Bone, Error, GeometryFlags, GrannyBone, Material, Matrix4x4, Result, Section, UgxGeom,
    UgxVersion, UnpackedVertex,
};

use crate::extras::{MeshExtrasJson, SceneExtrasJson};
use accessor::resolve_buffer;
use bounds::compute_bounds;
use material::import_materials;
use mesh::{
    MeshInfo, VertexPackingFeatures, build_packer, detect_global_bones,
    generate_granny_meshes_from_vertices,
};
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
    ///   and an in-memory external packer matching the UFX declaration.
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

#[derive(Clone)]
struct MeshNodeInfo {
    has_skin: bool,
    parent_bone_index: Option<usize>,
    transform: Matrix4x4,
    name: Option<String>,
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
    use_skinning: bool,
    mesh_nodes: &'a std::collections::HashMap<usize, Vec<MeshNodeInfo>>,
    restrict_to_scene_nodes: bool,
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
    let (mut bones, mut granny_bones) = if options.include_skeleton {
        import_skeleton(&root, &buffer)?
    } else {
        (Vec::new(), Vec::new())
    };
    let use_skinning = !bones.is_empty();
    if bones.is_empty() && !root.meshes.is_empty() {
        let (bone, granny_bone) = synthetic_root_bone();
        bones.push(bone);
        granny_bones.push(granny_bone);
    }
    let mut materials = if options.include_materials {
        import_materials(&root)?
    } else {
        Vec::new()
    };
    if materials.is_empty() && !root.meshes.is_empty() {
        materials.push(Material {
            name: "default".into(),
            ..Material::default()
        });
    }
    let (mesh_nodes, restrict_to_scene_nodes) = build_mesh_node_map(&root, use_skinning)?;
    let world_matrices = granny_bones
        .iter()
        .map(|bone| {
            bone.inverse_world_matrix
                .inverse()
                .ok_or_else(|| Error::UnsupportedFormat("Bone transform is singular".into()))
        })
        .collect::<Result<Vec<_>>>()?;
    let context = ImportContext {
        root: &root,
        buffer: &buffer,
        options,
        bones: &bones,
        use_skinning,
        mesh_nodes: &mesh_nodes,
        restrict_to_scene_nodes,
        world_matrices: &world_matrices,
    };
    let imported = import_meshes(&context)?;
    if imported.sections.is_empty() {
        return Err(Error::UnsupportedFormat(
            "The active glTF scene contains no triangle meshes".into(),
        ));
    }
    finish_geometry(&root, options, bones, granny_bones, materials, imported)
}

fn synthetic_root_bone() -> (Bone, GrannyBone) {
    let inverse_world_matrix = Matrix4x4::identity();
    (
        Bone {
            name: "root".into(),
            parent_index: -1,
            model_to_bone: inverse_world_matrix.clone(),
        },
        GrannyBone {
            name: "root".into(),
            parent_index: -1,
            local_transform: None,
            inverse_world_matrix,
            lod_error: 0.0,
            extended_data: None,
            extended_data_type: None,
        },
    )
}

fn build_mesh_node_map(
    root: &gltf_json::Root,
    use_skinning: bool,
) -> Result<(std::collections::HashMap<usize, Vec<MeshNodeInfo>>, bool)> {
    let parents = build_node_parents(root)?;
    let world_matrices = build_node_world_matrices(root, &parents)?;
    let active_nodes = active_node_indices(root)?;
    let joint_indices: std::collections::HashMap<usize, usize> = if use_skinning {
        root.skins
            .first()
            .map(|skin| {
                skin.joints
                    .iter()
                    .enumerate()
                    .map(|(joint_index, node)| (node.value(), joint_index))
                    .collect()
            })
            .unwrap_or_default()
    } else {
        std::collections::HashMap::new()
    };
    let mut result: std::collections::HashMap<usize, Vec<MeshNodeInfo>> =
        std::collections::HashMap::new();
    for (node_index, node) in root.nodes.iter().enumerate() {
        if active_nodes
            .as_ref()
            .is_some_and(|indices| !indices.contains(&node_index))
        {
            continue;
        }
        let Some(mesh_index) = node.mesh.as_ref().map(gltf_json::Index::value) else {
            continue;
        };
        if mesh_index >= root.meshes.len() {
            return Err(Error::UnsupportedFormat(
                "Mesh node index is out of bounds".into(),
            ));
        }
        if use_skinning && node.skin.as_ref().is_some_and(|skin| skin.value() != 0) {
            return Err(Error::UnsupportedFormat(
                "UGX supports only one glTF skin".into(),
            ));
        }
        let has_skin = use_skinning && node.skin.is_some();
        let parent_joint = (!has_skin)
            .then(|| nearest_joint_parent(node_index, &parents, &joint_indices))
            .flatten();
        let parent_bone_index = parent_joint.map(|(_, bone_index)| bone_index);
        let transform = if has_skin {
            Matrix4x4::identity()
        } else if let Some((parent_node_index, _)) = parent_joint {
            let parent_inverse = world_matrices[parent_node_index].inverse().ok_or_else(|| {
                Error::UnsupportedFormat("Rigid mesh parent transform is singular".into())
            })?;
            world_matrices[node_index].multiply(&parent_inverse)
        } else {
            world_matrices[node_index].clone()
        };
        result.entry(mesh_index).or_default().push(MeshNodeInfo {
            has_skin,
            parent_bone_index,
            transform,
            name: node.name.clone(),
        });
    }
    Ok((result, active_nodes.is_some()))
}

fn active_node_indices(root: &gltf_json::Root) -> Result<Option<std::collections::HashSet<usize>>> {
    let Some(scene_index) = active_scene_index(root)? else {
        return Ok(None);
    };
    let scene = &root.scenes[scene_index];
    let mut pending = scene
        .nodes
        .iter()
        .map(gltf_json::Index::value)
        .collect::<Vec<_>>();
    let mut active = std::collections::HashSet::new();
    while let Some(node_index) = pending.pop() {
        let node = root
            .nodes
            .get(node_index)
            .ok_or_else(|| Error::UnsupportedFormat("Scene node index is out of bounds".into()))?;
        if !active.insert(node_index) {
            continue;
        }
        pending.extend(node.children.iter().flatten().map(gltf_json::Index::value));
    }
    Ok(Some(active))
}

fn active_scene_index(root: &gltf_json::Root) -> Result<Option<usize>> {
    if root.scenes.is_empty() {
        if root.scene.is_some() {
            return Err(Error::UnsupportedFormat(
                "Default scene index is out of bounds".into(),
            ));
        }
        return Ok(None);
    }
    let index = root.scene.as_ref().map_or(0, gltf_json::Index::value);
    if index >= root.scenes.len() {
        return Err(Error::UnsupportedFormat(
            "Default scene index is out of bounds".into(),
        ));
    }
    Ok(Some(index))
}

fn build_node_parents(root: &gltf_json::Root) -> Result<Vec<Option<usize>>> {
    let mut parents = vec![None; root.nodes.len()];
    for (parent_index, node) in root.nodes.iter().enumerate() {
        for child in node.children.iter().flatten() {
            let child_index = child.value();
            let slot = parents.get_mut(child_index).ok_or_else(|| {
                Error::UnsupportedFormat("Node child index is out of bounds".into())
            })?;
            if slot.replace(parent_index).is_some() {
                return Err(Error::UnsupportedFormat(
                    "glTF node has more than one parent".into(),
                ));
            }
        }
    }
    Ok(parents)
}

fn build_node_world_matrices(
    root: &gltf_json::Root,
    parents: &[Option<usize>],
) -> Result<Vec<Matrix4x4>> {
    let local = root
        .nodes
        .iter()
        .map(node_local_matrix)
        .collect::<Result<Vec<_>>>()?;
    let mut world: Vec<Option<Matrix4x4>> = vec![None; root.nodes.len()];
    let mut visiting = vec![false; root.nodes.len()];
    for start in 0..root.nodes.len() {
        if world[start].is_some() {
            continue;
        }
        let mut path = Vec::new();
        let mut current = start;
        while world[current].is_none() {
            if visiting[current] {
                return Err(Error::UnsupportedFormat(
                    "glTF node hierarchy contains a cycle".into(),
                ));
            }
            visiting[current] = true;
            path.push(current);
            let Some(parent) = parents[current] else {
                break;
            };
            current = parent;
        }
        while let Some(node_index) = path.pop() {
            let matrix = parents[node_index]
                .and_then(|parent| world[parent].as_ref())
                .map_or_else(
                    || local[node_index].clone(),
                    |parent_world| local[node_index].multiply(parent_world),
                );
            world[node_index] = Some(matrix);
            visiting[node_index] = false;
        }
    }
    world
        .into_iter()
        .map(|matrix| {
            matrix.ok_or_else(|| {
                Error::UnsupportedFormat("Cannot resolve glTF node transform".into())
            })
        })
        .collect()
}

fn node_local_matrix(node: &gltf_json::Node) -> Result<Matrix4x4> {
    let rows = node.matrix.map_or_else(
        || {
            gltf::scene::Transform::Decomposed {
                translation: node.translation.unwrap_or([0.0; 3]),
                rotation: node.rotation.unwrap_or_default().0,
                scale: node.scale.unwrap_or([1.0; 3]),
            }
            .matrix()
        },
        |matrix| {
            [
                [matrix[0], matrix[1], matrix[2], matrix[3]],
                [matrix[4], matrix[5], matrix[6], matrix[7]],
                [matrix[8], matrix[9], matrix[10], matrix[11]],
                [matrix[12], matrix[13], matrix[14], matrix[15]],
            ]
        },
    );
    if rows.iter().flatten().any(|value| !value.is_finite()) {
        return Err(Error::UnsupportedFormat(
            "glTF node transform contains a non-finite value".into(),
        ));
    }
    let affine_tolerance = 1.0e-6;
    if rows[0][3].abs() > affine_tolerance
        || rows[1][3].abs() > affine_tolerance
        || rows[2][3].abs() > affine_tolerance
        || (rows[3][3] - 1.0).abs() > affine_tolerance
    {
        return Err(Error::UnsupportedFormat(
            "glTF mesh node transform must be affine".into(),
        ));
    }
    Ok(Matrix4x4 { rows })
}

fn nearest_joint_parent(
    node_index: usize,
    parents: &[Option<usize>],
    joint_indices: &std::collections::HashMap<usize, usize>,
) -> Option<(usize, usize)> {
    let mut parent = parents.get(node_index).copied().flatten();
    while let Some(parent_index) = parent {
        if let Some(&joint_index) = joint_indices.get(&parent_index) {
            return Some((parent_index, joint_index));
        }
        parent = parents.get(parent_index).copied().flatten();
    }
    None
}

fn import_meshes(context: &ImportContext<'_>) -> Result<ImportedMeshes> {
    let mut imported = ImportedMeshes::default();
    for (mesh_index, mesh) in context.root.meshes.iter().enumerate() {
        import_mesh(context, mesh_index, mesh, &mut imported)?;
    }
    Ok(imported)
}

fn import_mesh(
    context: &ImportContext<'_>,
    mesh_index: usize,
    mesh: &gltf_json::Mesh,
    imported: &mut ImportedMeshes,
) -> Result<()> {
    let extras: Option<MeshExtrasJson> = mesh
        .extras
        .as_ref()
        .and_then(|raw| serde_json::from_str(raw.get()).ok());
    if let Some(instances) = context.mesh_nodes.get(&mesh_index) {
        for mesh_node in instances {
            import_mesh_instance(
                context,
                mesh_index,
                mesh,
                Some(mesh_node),
                extras.as_ref(),
                instances.len() == 1,
                imported,
            )?;
        }
        return Ok(());
    }
    if context.restrict_to_scene_nodes {
        return Ok(());
    }
    import_mesh_instance(
        context,
        mesh_index,
        mesh,
        None,
        extras.as_ref(),
        true,
        imported,
    )
}

fn import_mesh_instance(
    context: &ImportContext<'_>,
    mesh_index: usize,
    mesh: &gltf_json::Mesh,
    mesh_node: Option<&MeshNodeInfo>,
    extras: Option<&MeshExtrasJson>,
    preserve_granny_index: bool,
    imported: &mut ImportedMeshes,
) -> Result<()> {
    let name = mesh_node
        .and_then(|info| info.name.clone())
        .or_else(|| mesh.name.clone())
        .unwrap_or_else(|| format!("mesh_{mesh_index}"));
    let start_vertex = imported.vertices.len();
    let start_section = imported.sections.len();
    for primitive in &mesh.primitives {
        import_mesh_primitive(context, mesh_node, primitive, extras, imported)?;
    }
    let end_vertex = imported.vertices.len();
    if end_vertex > start_vertex {
        imported.mesh_info.push((
            name,
            start_vertex,
            end_vertex,
            start_section,
            imported.sections.len(),
            preserve_granny_index
                .then(|| extras.and_then(|value| value.granny_mesh_index))
                .flatten(),
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
    let has_skin = context.use_skinning && mesh_node.is_some_and(|info| info.has_skin);
    let (mut vertices, mut indices, source_material_index) = import_primitive(
        primitive,
        context.root,
        context.buffer,
        has_skin,
        context.bones.len(),
    )?;
    let material_index = if context.options.include_materials {
        source_material_index
    } else {
        0
    };
    if vertices.is_empty() || indices.is_empty() {
        return Ok(());
    }
    if let Some(mesh_node) = mesh_node {
        apply_node_transform(&mut vertices, &mut indices, &mesh_node.transform)?;
    }
    let features = primitive_features(&vertices, has_skin);
    let kind = classify_section(
        &vertices,
        features.has_skin,
        mesh_node,
        !context.bones.is_empty(),
    )?;
    let (packer, final_vertices) = prepare_section_vertices(
        vertices,
        &features,
        &kind,
        context.options.version,
        context.world_matrices,
        extras,
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

fn apply_node_transform(
    vertices: &mut [UnpackedVertex],
    indices: &mut [u16],
    transform: &Matrix4x4,
) -> Result<()> {
    let determinant = linear_determinant(transform);
    if !determinant.is_finite() || determinant.abs() < 1.0e-10 {
        return Err(Error::UnsupportedFormat(
            "Mesh node transform is singular".into(),
        ));
    }
    let normal_transform = transform
        .inverse()
        .ok_or_else(|| Error::UnsupportedFormat("Mesh node transform is singular".into()))?
        .transpose();
    let handedness = determinant.signum();
    for vertex in vertices {
        vertex.position = transform_point(vertex.position, transform);
        let normal = normalized_direction(
            transform_direction(vertex.normal, &normal_transform),
            [0.0, 1.0, 0.0],
        );
        vertex.normal = normal;
        let source_tangent = [vertex.tangent[0], vertex.tangent[1], vertex.tangent[2]];
        if squared_length(source_tangent) > 1.0e-12 {
            let transformed = transform_direction(source_tangent, transform);
            let fallback = fallback_tangent(normal);
            let tangent =
                normalized_direction(transformed, [fallback[0], fallback[1], fallback[2]]);
            vertex.tangent = [
                tangent[0],
                tangent[1],
                tangent[2],
                vertex.tangent[3] * handedness,
            ];
        }
    }
    if determinant < 0.0 {
        for triangle in indices.as_chunks_mut::<3>().0 {
            triangle.swap(1, 2);
        }
    }
    Ok(())
}

fn linear_determinant(matrix: &Matrix4x4) -> f32 {
    let rows = &matrix.rows;
    rows[0][0] * (rows[1][1] * rows[2][2] - rows[1][2] * rows[2][1])
        - rows[0][1] * (rows[1][0] * rows[2][2] - rows[1][2] * rows[2][0])
        + rows[0][2] * (rows[1][0] * rows[2][1] - rows[1][1] * rows[2][0])
}

fn normalized_direction(value: [f32; 3], fallback: [f32; 3]) -> [f32; 3] {
    let length_squared = squared_length(value);
    if !length_squared.is_finite() || length_squared < 1.0e-12 {
        fallback
    } else {
        let inverse_length = length_squared.sqrt().recip();
        value.map(|component| component * inverse_length)
    }
}

fn squared_length(value: [f32; 3]) -> f32 {
    value
        .iter()
        .map(|component| component * component)
        .sum::<f32>()
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
    has_bones: bool,
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
    if has_skin && mesh_node.is_some_and(|info| info.has_skin) {
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
    if has_bones && !has_skin {
        return Ok(SectionKind {
            global_bones: false,
            rigid_only: true,
            bone_index: 0,
            max_bones: 1,
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
    mut vertices: Vec<UnpackedVertex>,
    features: &PrimitiveFeatures,
    kind: &SectionKind,
    version: UgxVersion,
    world_matrices: &[Matrix4x4],
    extras: Option<&MeshExtrasJson>,
) -> (ugx::UnivertPacker, Vec<UnpackedVertex>) {
    let rigid = kind.global_bones || kind.rigid_only;
    let has_colors = features.has_colors || extras.is_some_and(|value| value.hw2_has_color);
    let hw2_skin_order = if extras.is_some_and(|value| value.hw2_color_before_skin) {
        ugx::Hw2SkinOrder::ColorThenSkin
    } else {
        ugx::Hw2SkinOrder::SkinThenColor
    };
    let packer = build_packer(
        version,
        VertexPackingFeatures {
            max_texcoords: features.max_texcoords,
            has_tangents: features.has_tangents,
            has_skin: features.has_skin && !rigid,
            has_colors,
            hw2_skin_order,
        },
    );
    if version == UgxVersion::Hw2 {
        canonicalize_hw2_vertices(&mut vertices, features);
    }
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

fn canonicalize_hw2_vertices(vertices: &mut [UnpackedVertex], features: &PrimitiveFeatures) {
    for vertex in vertices {
        vertex.num_texcoords = vertex.num_texcoords.max(1);
        if !features.has_tangents {
            vertex.tangent = fallback_tangent(vertex.normal);
        }
        if features.max_texcoords > 1 && !features.has_colors {
            vertex.diffuse = [1.0; 4];
        }
    }
}

fn fallback_tangent(normal: [f32; 3]) -> [f32; 4] {
    let normal = normalized_direction(normal, [0.0, 1.0, 0.0]);
    let reference = if normal[2].abs() < 0.999 {
        [0.0, 0.0, 1.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let tangent = normalized_direction(
        [
            reference[1] * normal[2] - reference[2] * normal[1],
            reference[2] * normal[0] - reference[0] * normal[2],
            reference[0] * normal[1] - reference[1] * normal[0],
        ],
        [1.0, 0.0, 0.0],
    );
    [tangent[0], tangent[1], tangent[2], 1.0]
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
        external_vert_packer: (input.version == UgxVersion::Hw2).then(|| input.packer.clone()),
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
    let mut granny_meshes = generate_granny_meshes_from_vertices(
        &imported.vertices,
        &granny_bones,
        &imported.mesh_info,
        &imported.sections,
    );
    let scene_extras = read_scene_extras(root)?;
    if let Some(extras) = &scene_extras {
        merge_preserved_granny_meshes(&mut granny_meshes, &extras.ugx_granny_meshes);
    }
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
        max_instances: scene_extras.map_or(1, |extras| extras.ugx_max_instances),
        // Recomputed by `rebuild_derived_data` after the geometry is assembled.
        instance_index_multiplier: 0,
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

fn read_scene_extras(root: &gltf_json::Root) -> Result<Option<SceneExtrasJson>> {
    Ok(active_scene_index(root)?
        .and_then(|index| root.scenes.get(index))
        .and_then(|scene| scene.extras.as_ref())
        .and_then(|raw| serde_json::from_str::<SceneExtrasJson>(raw.get()).ok()))
}

fn merge_preserved_granny_meshes(
    generated: &mut Vec<ugx::GrannyMesh>,
    preserved: &[crate::extras::GrannyMeshJson],
) {
    if generated.len() < preserved.len() {
        generated.resize_with(preserved.len(), ugx::GrannyMesh::default);
    }
    for (mesh_index, preserved_json) in preserved.iter().cloned().enumerate() {
        let preserved_mesh = ugx::GrannyMesh::from(preserved_json);
        let generated_mesh = &mut generated[mesh_index];
        if generated_mesh.bone_bindings.is_empty() {
            *generated_mesh = preserved_mesh;
            continue;
        }
        for preserved_binding in preserved_mesh.bone_bindings {
            if let Some(binding) = generated_mesh
                .bone_bindings
                .iter_mut()
                .find(|binding| binding.bone_name == preserved_binding.bone_name)
            {
                binding.triangle_indices = preserved_binding.triangle_indices;
                let generated_has_obb = binding
                    .obb_min
                    .iter()
                    .chain(&binding.obb_max)
                    .any(|value| value.to_bits() != 0.0f32.to_bits());
                if !generated_has_obb {
                    binding.obb_min = preserved_binding.obb_min;
                    binding.obb_max = preserved_binding.obb_max;
                }
            } else {
                generated_mesh.bone_bindings.push(preserved_binding);
            }
        }
    }
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
