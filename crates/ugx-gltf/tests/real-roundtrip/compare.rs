use ugx::{
    GrannyBone, GrannyLocalTransform, HoganMaterialData, LegacyMaterialData, MapType, Material,
    MaterialData, UgxGeom, UgxVersion, UnpackedVertex,
};
use ugx_gltf::convert_ugx_version_to_bytes;

pub(super) enum RoundtripResult {
    Passed,
    ReadSkipped(String),
    Failed(String),
}

pub(super) fn roundtrip_bytes(label: &str, data: &[u8], version: UgxVersion) -> RoundtripResult {
    let original = match ugx::Reader::read(data) {
        Ok(geometry) => geometry,
        Err(error) => return RoundtripResult::ReadSkipped(format!("{label}: read: {error}")),
    };
    compare_converted_geometry(label, &original, version)
}

pub(super) fn compare_converted_geometry(
    label: &str,
    original: &UgxGeom,
    target: UgxVersion,
) -> RoundtripResult {
    let reread = match convert_write(original, target) {
        Ok(geometry) => geometry,
        Err(error) => return RoundtripResult::Failed(format!("{label}: {error}")),
    };
    let comparison = Comparison {
        label,
        original,
        reread: &reread,
        version: target,
    };
    match comparison.run() {
        Ok(()) => RoundtripResult::Passed,
        Err(error) => RoundtripResult::Failed(error),
    }
}

fn convert_write(original: &UgxGeom, version: UgxVersion) -> Result<UgxGeom, String> {
    let bytes = convert_ugx_version_to_bytes(original, version)
        .map_err(|error| format!("convert/write: {error}"))?;
    ugx::Reader::read(&bytes).map_err(|error| format!("re-read: {error}"))
}

struct Comparison<'a> {
    label: &'a str,
    original: &'a UgxGeom,
    reread: &'a UgxGeom,
    version: UgxVersion,
}

impl Comparison<'_> {
    fn run(&self) -> Result<(), String> {
        self.compare_structure()?;
        self.compare_materials()?;
        self.compare_vertices()?;
        self.compare_bones()?;
        self.compare_granny_bones()?;
        self.compare_bone_bindings()?;
        self.compare_indices()?;
        if self.version.has_aabb_tree()
            && !self.original.sections.is_empty()
            && self.reread.aabb_tree.is_none()
        {
            return Err(format!("{}: AABB tree lost during roundtrip", self.label));
        }
        Ok(())
    }

    fn compare_structure(&self) -> Result<(), String> {
        compare_count(
            self.label,
            "sections",
            self.original.sections.len(),
            self.reread.sections.len(),
        )?;
        compare_count(
            self.label,
            "bones",
            self.original.bones.len(),
            self.reread.bones.len(),
        )?;
        compare_count(
            self.label,
            "materials",
            self.original.materials.len(),
            self.reread.materials.len(),
        )?;
        for (section_index, (source, result)) in self
            .original
            .sections
            .iter()
            .zip(&self.reread.sections)
            .enumerate()
        {
            if source.num_verts != result.num_verts || source.num_tris != result.num_tris {
                return Err(format!(
                    "{}: section {section_index} dimensions changed from {}/{} vertices/triangles to {}/{}",
                    self.label,
                    source.num_verts,
                    source.num_tris,
                    result.num_verts,
                    result.num_tris,
                ));
            }
            if source.material_index != result.material_index {
                return Err(format!(
                    "{}: section {section_index} material changed from {} to {}",
                    self.label, source.material_index, result.material_index,
                ));
            }
        }
        Ok(())
    }

    fn compare_materials(&self) -> Result<(), String> {
        let skinned_materials = ugx::types::convert::material_skinned_flags(self.original);
        for (material_index, (source, result)) in self
            .original
            .materials
            .iter()
            .zip(&self.reread.materials)
            .enumerate()
        {
            if source.name != result.name {
                return Err(format!(
                    "{}: material {material_index} name changed from {:?} to {:?}",
                    self.label, source.name, result.name,
                ));
            }
            let converted = ugx::types::convert::convert_material(
                source,
                self.version == UgxVersion::Hw2,
                skinned_materials
                    .get(material_index)
                    .copied()
                    .unwrap_or(false),
            );
            compare_material(self.label, material_index, &converted, result)?;
        }
        Ok(())
    }

    fn compare_vertices(&self) -> Result<(), String> {
        for section_index in 0..self.original.sections.len() {
            let source_vertices = self
                .original
                .unpack_section_vertices(section_index)
                .map_err(|error| {
                    format!(
                        "{}: unpack source section {section_index}: {error}",
                        self.label
                    )
                })?;
            let result_vertices =
                self.reread
                    .unpack_section_vertices(section_index)
                    .map_err(|error| {
                        format!(
                            "{}: unpack result section {section_index}: {error}",
                            self.label
                        )
                    })?;
            compare_count(
                self.label,
                &format!("section {section_index} unpacked vertices"),
                source_vertices.len(),
                result_vertices.len(),
            )?;
            for (vertex_index, (source, result)) in
                source_vertices.iter().zip(&result_vertices).enumerate()
            {
                self.compare_vertex(section_index, vertex_index, source, result)?;
            }
        }
        Ok(())
    }

    fn compare_vertex(
        &self,
        section_index: usize,
        vertex_index: usize,
        source: &UnpackedVertex,
        result: &UnpackedVertex,
    ) -> Result<(), String> {
        let prefix = format!(
            "{}: section {section_index} vertex {vertex_index}",
            self.label
        );
        compare_floats(
            &format!("{prefix} position"),
            &source.position,
            &result.position,
            0.05,
        )?;
        compare_directions(&format!("{prefix} normal"), source.normal, result.normal)?;
        compare_directions(
            &format!("{prefix} tangent"),
            [source.tangent[0], source.tangent[1], source.tangent[2]],
            [result.tangent[0], result.tangent[1], result.tangent[2]],
        )?;
        compare_texcoords(&prefix, source, result)?;
        self.compare_skin(section_index, &prefix, source, result)
    }

    fn compare_skin(
        &self,
        section_index: usize,
        prefix: &str,
        source: &UnpackedVertex,
        result: &UnpackedVertex,
    ) -> Result<(), String> {
        let result_section = &self.reread.sections[section_index];
        if result_section.global_bones || result_section.rigid_only {
            return Ok(());
        }
        compare_floats(
            &format!("{prefix} bone weights"),
            &source.bone_weights,
            &result.bone_weights,
            0.01,
        )?;
        let source_remap = &self.original.sections[section_index].bone_remap;
        let result_remap = &result_section.bone_remap;
        for influence in 0..4 {
            if source.bone_weights[influence] <= f32::EPSILON {
                continue;
            }
            let source_index = resolve_bone(source.bone_indices[influence], source_remap);
            let result_index = resolve_bone(result.bone_indices[influence], result_remap);
            if source_index != result_index {
                return Err(format!(
                    "{prefix} bone index {influence} changed from {source_index} to {result_index}"
                ));
            }
        }
        Ok(())
    }

    fn compare_bones(&self) -> Result<(), String> {
        for (bone_index, (source, result)) in self
            .original
            .bones
            .iter()
            .zip(&self.reread.bones)
            .enumerate()
        {
            if source.name != result.name || source.parent_index != result.parent_index {
                return Err(format!(
                    "{}: bone {bone_index} changed from {:?}/{} to {:?}/{}",
                    self.label, source.name, source.parent_index, result.name, result.parent_index,
                ));
            }
        }
        Ok(())
    }

    fn compare_granny_bones(&self) -> Result<(), String> {
        compare_count(
            self.label,
            "Granny bones",
            self.original.granny_bones.len(),
            self.reread.granny_bones.len(),
        )?;
        for (bone_index, (source, result)) in self
            .original
            .granny_bones
            .iter()
            .zip(&self.reread.granny_bones)
            .enumerate()
        {
            compare_granny_identity(self.label, bone_index, source, result)?;
            compare_matrix(self.label, bone_index, source, result)?;
            compare_local_transform(self, bone_index, source, result)?;
            if source.lod_error.to_bits() != result.lod_error.to_bits() {
                return Err(format!(
                    "{}: Granny bone {bone_index} LOD error changed from {} to {}",
                    self.label, source.lod_error, result.lod_error,
                ));
            }
        }
        Ok(())
    }

    fn compare_bone_bindings(&self) -> Result<(), String> {
        for (mesh_index, source_mesh) in self.original.granny_meshes.iter().enumerate() {
            let Some(result_mesh) = self.reread.granny_meshes.get(mesh_index) else {
                return Err(format!("{}: Granny mesh {mesh_index} was lost", self.label));
            };
            for source_binding in source_mesh
                .bone_bindings
                .iter()
                .filter(|binding| has_nonzero_obb(binding))
            {
                let Some(result_binding) = result_mesh
                    .bone_bindings
                    .iter()
                    .find(|binding| binding.bone_name == source_binding.bone_name)
                else {
                    return Err(format!(
                        "{}: mesh {mesh_index} lost bone binding {:?}",
                        self.label, source_binding.bone_name,
                    ));
                };
                if !has_nonzero_obb(result_binding) {
                    return Err(format!(
                        "{}: mesh {mesh_index} bone {:?} lost its OBB",
                        self.label, source_binding.bone_name,
                    ));
                }
            }
        }
        Ok(())
    }

    fn compare_indices(&self) -> Result<(), String> {
        for section_index in 0..self.original.sections.len().min(self.reread.sections.len()) {
            let source = self
                .original
                .get_section_indices(section_index)
                .map_err(|error| {
                    format!(
                        "{}: source indices for section {section_index}: {error}",
                        self.label
                    )
                })?;
            let result = self
                .reread
                .get_section_indices(section_index)
                .map_err(|error| {
                    format!(
                        "{}: result indices for section {section_index}: {error}",
                        self.label
                    )
                })?;
            if source != result {
                return Err(format!(
                    "{}: section {section_index} indices changed",
                    self.label
                ));
            }
        }
        Ok(())
    }
}

fn compare_material(
    label: &str,
    material_index: usize,
    source: &Material,
    result: &Material,
) -> Result<(), String> {
    match (&source.data, &result.data) {
        (MaterialData::Legacy(source_data), MaterialData::Legacy(result_data)) => {
            compare_legacy(label, material_index, source_data, result_data)
        }
        (MaterialData::Hogan(source_data), MaterialData::Hogan(result_data)) => {
            compare_hogan(label, material_index, source_data, result_data)
        }
        _ => Err(format!(
            "{label}: material {material_index} changed representation"
        )),
    }
}

fn compare_legacy(
    label: &str,
    material_index: usize,
    source: &LegacyMaterialData,
    result: &LegacyMaterialData,
) -> Result<(), String> {
    if source.blend_type != result.blend_type || source.flags != result.flags {
        return Err(format!(
            "{label}: legacy material {material_index} blend type or flags changed"
        ));
    }
    if (source.opacity - result.opacity).abs() > 2.0 / 255.0 {
        return Err(format!(
            "{label}: material {material_index} opacity changed from {} to {}",
            source.opacity, result.opacity,
        ));
    }
    for (map_index, map_type) in MapType::ALL.into_iter().enumerate() {
        let source_names = source.maps[map_index]
            .iter()
            .map(|texture| &texture.name)
            .collect::<Vec<_>>();
        let result_names = result.maps[map_index]
            .iter()
            .map(|texture| &texture.name)
            .collect::<Vec<_>>();
        if source_names != result_names {
            return Err(format!(
                "{label}: material {material_index} {map_type:?} texture names changed"
            ));
        }
        compare_floats(
            &format!("{label}: material {material_index} {map_type:?} UVW velocity"),
            &source.uvw_velocity[map_index],
            &result.uvw_velocity[map_index],
            1.0e-4,
        )?;
    }
    Ok(())
}

fn compare_hogan(
    label: &str,
    material_index: usize,
    source: &HoganMaterialData,
    result: &HoganMaterialData,
) -> Result<(), String> {
    if source.ufx_version != result.ufx_version
        || source.blend_mode != result.blend_mode
        || source.textures != result.textures
    {
        return Err(format!(
            "{label}: Hogan material {material_index} metadata changed"
        ));
    }
    compare_count(
        label,
        &format!("material {material_index} shader permutations"),
        source.shader_permutations.len(),
        result.shader_permutations.len(),
    )?;
    for (permutation_index, (source_permutation, result_permutation)) in source
        .shader_permutations
        .iter()
        .zip(&result.shader_permutations)
        .enumerate()
    {
        if source_permutation.name != result_permutation.name
            || source_permutation.hash != result_permutation.hash
        {
            return Err(format!(
                "{label}: material {material_index} permutation {permutation_index} changed"
            ));
        }
    }
    Ok(())
}

fn compare_texcoords(
    prefix: &str,
    source: &UnpackedVertex,
    result: &UnpackedVertex,
) -> Result<(), String> {
    let count = source.num_texcoords.min(result.num_texcoords);
    for texture_index in 0..count {
        compare_floats(
            &format!("{prefix} texture coordinate {texture_index}"),
            &source.texcoords[texture_index],
            &result.texcoords[texture_index],
            0.01,
        )?;
    }
    Ok(())
}

fn resolve_bone(index: u16, remap: &[u8]) -> u16 {
    remap
        .get(usize::from(index))
        .copied()
        .map_or(index, u16::from)
}

fn compare_granny_identity(
    label: &str,
    bone_index: usize,
    source: &GrannyBone,
    result: &GrannyBone,
) -> Result<(), String> {
    if source.name == result.name && source.parent_index == result.parent_index {
        return Ok(());
    }
    Err(format!(
        "{label}: Granny bone {bone_index} changed from {:?}/{} to {:?}/{}",
        source.name, source.parent_index, result.name, result.parent_index,
    ))
}

fn compare_matrix(
    label: &str,
    bone_index: usize,
    source: &GrannyBone,
    result: &GrannyBone,
) -> Result<(), String> {
    for row in 0..4 {
        compare_floats(
            &format!("{label}: Granny bone {bone_index} matrix row {row}"),
            &source.inverse_world_matrix.rows[row],
            &result.inverse_world_matrix.rows[row],
            1.0e-4,
        )?;
    }
    Ok(())
}

fn compare_local_transform(
    comparison: &Comparison<'_>,
    bone_index: usize,
    source: &GrannyBone,
    result: &GrannyBone,
) -> Result<(), String> {
    let (Some(source_transform), Some(result_transform)) =
        (&source.local_transform, &result.local_transform)
    else {
        return if source.local_transform.is_some() {
            Err(format!(
                "{}: Granny bone {bone_index} lost its local transform",
                comparison.label
            ))
        } else {
            Ok(())
        };
    };
    if has_identity_ancestor(comparison.original, source.parent_index) {
        return Ok(());
    }
    compare_flagged_transform(
        comparison.label,
        bone_index,
        source_transform,
        result_transform,
    )
}

fn has_identity_ancestor(geometry: &UgxGeom, mut parent_index: i32) -> bool {
    while let Ok(index) = usize::try_from(parent_index) {
        let Some(parent) = geometry.granny_bones.get(index) else {
            break;
        };
        if parent
            .local_transform
            .as_ref()
            .is_some_and(|transform| transform.flags == 0)
        {
            return true;
        }
        parent_index = parent.parent_index;
    }
    false
}

fn compare_flagged_transform(
    label: &str,
    bone_index: usize,
    source: &GrannyLocalTransform,
    result: &GrannyLocalTransform,
) -> Result<(), String> {
    const TOLERANCE: f32 = 1.0e-3;
    if source.flags & 0x1 != 0 {
        compare_floats(
            &format!("{label}: Granny bone {bone_index} local position"),
            &source.position,
            &result.position,
            TOLERANCE,
        )?;
    }
    if source.flags & 0x2 != 0 {
        let dot = source
            .orientation
            .iter()
            .zip(result.orientation)
            .map(|(left, right)| left * right)
            .sum::<f32>();
        if 1.0 - dot.abs() > TOLERANCE {
            return Err(format!(
                "{label}: Granny bone {bone_index} local orientation changed"
            ));
        }
    }
    if source.flags & 0x4 != 0 {
        for row in 0..3 {
            compare_floats(
                &format!("{label}: Granny bone {bone_index} scale/shear row {row}"),
                &source.scale_shear[row],
                &result.scale_shear[row],
                TOLERANCE,
            )?;
        }
    }
    Ok(())
}

fn compare_directions(label: &str, source: [f32; 3], result: [f32; 3]) -> Result<(), String> {
    let (Some(source_normalized), Some(result_normalized)) = (normalize(source), normalize(result))
    else {
        return Ok(());
    };
    compare_floats(label, &source_normalized, &result_normalized, 0.02)
}

fn normalize(value: [f32; 3]) -> Option<[f32; 3]> {
    let length = value
        .iter()
        .map(|component| component * component)
        .sum::<f32>()
        .sqrt();
    (length > 1.0e-6).then(|| value.map(|component| component / length))
}

fn compare_floats(
    label: &str,
    source: &[f32],
    result: &[f32],
    tolerance: f32,
) -> Result<(), String> {
    for (component, (source_value, result_value)) in source.iter().zip(result).enumerate() {
        if (source_value - result_value).abs() > tolerance {
            return Err(format!(
                "{label}[{component}] changed from {source_value} to {result_value} (tolerance {tolerance})"
            ));
        }
    }
    Ok(())
}

fn compare_count(
    label: &str,
    item: &str,
    source_count: usize,
    result_count: usize,
) -> Result<(), String> {
    if source_count == result_count {
        Ok(())
    } else {
        Err(format!(
            "{label}: {item} count changed from {source_count} to {result_count}"
        ))
    }
}

fn has_nonzero_obb(binding: &ugx::GrannyBoneBinding) -> bool {
    binding
        .obb_min
        .iter()
        .chain(&binding.obb_max)
        .any(|value| value.to_bits() != 0.0f32.to_bits())
}
