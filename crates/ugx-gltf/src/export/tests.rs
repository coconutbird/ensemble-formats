//! Tests for glTF primitive export helpers.

use gltf_json as json;
use json::validation::Checked::Valid;
use ugx::UnpackedVertex;

use super::*;

struct BuiltPrimitive {
    primitive: json::mesh::Primitive,
    buffer: Vec<u8>,
    accessors: Vec<json::Accessor>,
    views: Vec<json::buffer::View>,
}

fn vertex(position: [f32; 3]) -> UnpackedVertex {
    UnpackedVertex {
        position,
        normal: [0.0, 1.0, 0.0],
        ..UnpackedVertex::default()
    }
}

fn build_primitive(
    vertices: &[UnpackedVertex],
    has_skin: bool,
    bone_count: usize,
    rigid_bone_index: i32,
) -> BuiltPrimitive {
    let indices = vec![0; vertices.len().max(3)];
    let mut buffer = Vec::new();
    let mut accessors = Vec::new();
    let mut views = Vec::new();
    let primitive = create_primitive(
        &PrimitiveInput {
            vertices,
            indices: &indices,
            material_index: -1,
            has_materials: false,
            has_skeleton: has_skin,
            bone_count,
            rigid_bone_index,
            bone_remap: &[],
        },
        &mut PrimitiveOutput {
            buffer_data: &mut buffer,
            accessors: &mut accessors,
            buffer_views: &mut views,
        },
    )
    .expect("test primitive should be representable as glTF");
    BuiltPrimitive {
        primitive,
        buffer,
        accessors,
        views,
    }
}

fn has_semantic(primitive: &json::mesh::Primitive, semantic: json::mesh::Semantic) -> bool {
    primitive.attributes.contains_key(&Valid(semantic))
}

fn accessor(built: &BuiltPrimitive, semantic: json::mesh::Semantic) -> &json::Accessor {
    let index = built
        .primitive
        .attributes
        .get(&Valid(semantic))
        .expect("semantic should have an accessor")
        .value();
    &built.accessors[index]
}

fn accessor_offset(built: &BuiltPrimitive, semantic: json::mesh::Semantic) -> usize {
    let accessor = accessor(built, semantic);
    let view_index = accessor
        .buffer_view
        .expect("accessor should reference a buffer view")
        .value();
    let offset = built.views[view_index]
        .byte_offset
        .expect("buffer view should have an explicit offset")
        .0;
    usize::try_from(offset).expect("test buffer offset should fit usize")
}

fn read_f32(buffer: &[u8], offset: usize) -> f32 {
    let bytes = buffer
        .get(offset..offset + 4)
        .expect("f32 must be inside the test buffer")
        .try_into()
        .expect("f32 has exactly four bytes");
    f32::from_le_bytes(bytes)
}

fn read_f32_array<const N: usize>(buffer: &[u8], offset: usize) -> [f32; N] {
    std::array::from_fn(|index| read_f32(buffer, offset + index * 4))
}

fn assert_near(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() < 1.0e-5,
        "expected {expected}, got {actual}"
    );
}

#[test]
fn exports_the_detected_uv_set_count() {
    let no_uvs = vec![vertex([0.0; 3]); 3];
    let built = build_primitive(&no_uvs, false, 0, -1);
    assert!(!has_semantic(
        &built.primitive,
        json::mesh::Semantic::TexCoords(0)
    ));

    let mut three_uvs = vec![vertex([0.0; 3]); 3];
    for value in &mut three_uvs {
        value.texcoords[..3].copy_from_slice(&[[0.1, 0.2], [0.3, 0.4], [0.5, 0.6]]);
        value.num_texcoords = 3;
    }
    let built = build_primitive(&three_uvs, false, 0, -1);
    for set_index in 0..3 {
        let semantic = json::mesh::Semantic::TexCoords(set_index);
        let value = accessor(&built, semantic);
        assert_eq!(value.type_, Valid(json::accessor::Type::Vec2));
        assert_eq!(value.count, json::validation::USize64(3));
    }
    assert!(!has_semantic(
        &built.primitive,
        json::mesh::Semantic::TexCoords(3)
    ));
}

#[test]
fn exports_tangents_only_when_present() {
    let no_tangents = vec![vertex([0.0; 3]); 3];
    let built = build_primitive(&no_tangents, false, 0, -1);
    assert!(!has_semantic(
        &built.primitive,
        json::mesh::Semantic::Tangents
    ));

    let mut tangents = no_tangents;
    for value in &mut tangents {
        value.tangent = [1.0, 0.0, 0.0, 1.0];
    }
    let built = build_primitive(&tangents, false, 0, -1);
    assert_eq!(
        accessor(&built, json::mesh::Semantic::Tangents).type_,
        Valid(json::accessor::Type::Vec4)
    );
}

#[test]
fn normalizes_tangents_and_preserves_handedness() {
    let mut positive = vec![vertex([0.0; 3])];
    positive[0].tangent = [0.5, 0.0, 0.0, 1.0];
    let built = build_primitive(&positive, false, 0, -1);
    let offset = accessor_offset(&built, json::mesh::Semantic::Tangents);
    let [x, y, z, handedness] = read_f32_array(&built.buffer, offset);
    assert_near((x * x + y * y + z * z).sqrt(), 1.0);
    assert_near(x, 1.0);
    assert_eq!(handedness.to_bits(), 1.0f32.to_bits());

    let mut negative = vec![vertex([0.0; 3])];
    negative[0].tangent = [0.0, 0.0, 0.5, -1.0];
    let built = build_primitive(&negative, false, 0, -1);
    let offset = accessor_offset(&built, json::mesh::Semantic::Tangents);
    let [_, _, _, handedness] = read_f32_array(&built.buffer, offset);
    assert_eq!(handedness.to_bits(), (-1.0f32).to_bits());
}

#[test]
fn normalizes_normals_and_uses_the_up_fallback() {
    let mut non_unit = vec![vertex([0.0; 3])];
    non_unit[0].normal = [2.0, 0.0, 0.0];
    let built = build_primitive(&non_unit, false, 0, -1);
    let offset = accessor_offset(&built, json::mesh::Semantic::Normals);
    let [x, y, z] = read_f32_array(&built.buffer, offset);
    assert_near((x * x + y * y + z * z).sqrt(), 1.0);
    assert_near(x, 1.0);

    let mut zero = vec![vertex([0.0; 3])];
    zero[0].normal = [0.0; 3];
    let built = build_primitive(&zero, false, 0, -1);
    let offset = accessor_offset(&built, json::mesh::Semantic::Normals);
    let [x, y, z] = read_f32_array(&built.buffer, offset);
    assert_near(x, 0.0);
    assert_near(y, 1.0);
    assert_near(z, 0.0);
}

#[test]
fn selects_joint_component_width_from_bone_count() {
    let mut vertices = vec![vertex([0.0; 3]); 3];
    for value in &mut vertices {
        value.bone_indices = [1, 2, 0, 0];
        value.bone_weights = [0.7, 0.3, 0.0, 0.0];
    }
    assert_eq!(
        joint_component_type(&vertices, 50),
        json::accessor::ComponentType::U8
    );
    assert_eq!(
        joint_component_type(&vertices, 300),
        json::accessor::ComponentType::U16
    );
}

fn joint_component_type(
    vertices: &[UnpackedVertex],
    bone_count: usize,
) -> json::accessor::ComponentType {
    let built = build_primitive(vertices, true, bone_count, -1);
    let Valid(json::accessor::GenericComponentType(component_type)) =
        accessor(&built, json::mesh::Semantic::Joints(0)).component_type
    else {
        panic!("joint accessor should have a valid generic component type");
    };
    component_type
}

#[test]
fn preserves_zero_based_joint_indices() {
    let mut vertices = vec![vertex([0.0; 3])];
    vertices[0].bone_indices = [3, 1, 0, 0];
    vertices[0].bone_weights = [0.8, 0.2, 0.0, 0.0];
    let built = build_primitive(&vertices, true, 10, -1);
    let offset = accessor_offset(&built, json::mesh::Semantic::Joints(0));
    assert_eq!(&built.buffer[offset..offset + 4], &[3, 1, 0, 0]);
}

#[test]
fn assigns_the_section_bone_to_a_rigid_vertex() {
    let vertices = vec![vertex([0.0; 3])];
    let built = build_primitive(&vertices, true, 10, 5);
    let offset = accessor_offset(&built, json::mesh::Semantic::Joints(0));
    assert_eq!(built.buffer[offset], 5);
}
