//! Import tests for ordinary Blender-style meshes without armatures or materials.

use ugx::UgxVersion;
use ugx_gltf::{GltfImportOptions, import_from_gltf};

fn triangle_buffer() -> Vec<u8> {
    let mut buffer = Vec::new();
    for position in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
        for component in position {
            buffer.extend_from_slice(&component.to_le_bytes());
        }
    }
    for index in [0u16, 1, 2] {
        buffer.extend_from_slice(&index.to_le_bytes());
    }
    buffer
}

fn triangle_buffer_with_two_uv_sets() -> Vec<u8> {
    let mut buffer = triangle_buffer();
    buffer.extend_from_slice(&[0, 0]);
    for texcoord in [[0.0f32, 0.0], [1.0, 0.0], [0.0, 1.0]] {
        for component in texcoord {
            buffer.extend_from_slice(&component.to_le_bytes());
        }
    }
    for texcoord in [[0.25f32, 0.25], [0.75, 0.25], [0.25, 0.75]] {
        for component in texcoord {
            buffer.extend_from_slice(&component.to_le_bytes());
        }
    }
    buffer
}

fn triangle_gltf(include_material: bool) -> String {
    let material = include_material.then_some(serde_json::json!({"name": "material"}));
    let primitive_material = include_material.then_some(0);
    serde_json::json!({
        "asset": {"version": "2.0"},
        "buffers": [{"byteLength": 42}],
        "bufferViews": [
            {"buffer": 0, "byteOffset": 0, "byteLength": 36},
            {"buffer": 0, "byteOffset": 36, "byteLength": 6}
        ],
        "accessors": [
            {
                "bufferView": 0,
                "componentType": 5126,
                "count": 3,
                "type": "VEC3",
                "min": [0.0, 0.0, 0.0],
                "max": [1.0, 1.0, 0.0]
            },
            {
                "bufferView": 1,
                "componentType": 5123,
                "count": 3,
                "type": "SCALAR"
            }
        ],
        "materials": material.into_iter().collect::<Vec<_>>(),
        "meshes": [{
            "name": "triangle",
            "primitives": [{
                "attributes": {"POSITION": 0},
                "indices": 1,
                "material": primitive_material
            }]
        }],
        "nodes": [{"mesh": 0}],
        "scenes": [{"nodes": [0]}],
        "scene": 0
    })
    .to_string()
}

fn import_value(root: &serde_json::Value) -> ugx::UgxGeom {
    try_import_value(root).unwrap()
}

fn try_import_value(root: &serde_json::Value) -> ugx::Result<ugx::UgxGeom> {
    import_from_gltf(
        &root.to_string(),
        Some(&triangle_buffer()),
        &GltfImportOptions::default(),
    )
}

fn assert_near(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() < 0.02,
        "expected {expected}, got {actual}"
    );
}

#[test]
fn static_mesh_gets_required_root_binding() {
    let buffer = triangle_buffer();
    let geometry = import_from_gltf(
        &triangle_gltf(true),
        Some(&buffer),
        &GltfImportOptions::default(),
    )
    .unwrap();

    assert_eq!(geometry.bones.len(), 1);
    assert_eq!(geometry.granny_bones.len(), 1);
    assert_eq!(geometry.bone_bounds.len(), 1);
    assert_eq!(geometry.bones[0].name, "root");
    assert!(geometry.sections[0].rigid_only);
    assert_eq!(geometry.sections[0].rigid_bone_index, 0);
    assert_eq!(geometry.granny_meshes.len(), 1);
    ugx::Writer::write(&geometry, UgxVersion::Hw2).unwrap();
}

#[test]
fn materialless_mesh_gets_required_default_material() {
    let buffer = triangle_buffer();
    let geometry = import_from_gltf(
        &triangle_gltf(false),
        Some(&buffer),
        &GltfImportOptions {
            include_materials: false,
            ..GltfImportOptions::default()
        },
    )
    .unwrap();

    assert_eq!(geometry.materials.len(), 1);
    assert_eq!(geometry.sections[0].material_index, 0);
    ugx::Writer::write(&geometry, UgxVersion::Hw2).unwrap();
}

#[test]
fn hw2_canonical_layout_preserves_two_uv_sets() {
    let mut root: serde_json::Value = serde_json::from_str(&triangle_gltf(true)).unwrap();
    root["buffers"][0]["byteLength"] = serde_json::json!(92);
    root["bufferViews"].as_array_mut().unwrap().extend([
        serde_json::json!({"buffer": 0, "byteOffset": 44, "byteLength": 24}),
        serde_json::json!({"buffer": 0, "byteOffset": 68, "byteLength": 24}),
    ]);
    root["accessors"].as_array_mut().unwrap().extend([
        serde_json::json!({
            "bufferView": 2,
            "componentType": 5126,
            "count": 3,
            "type": "VEC2"
        }),
        serde_json::json!({
            "bufferView": 3,
            "componentType": 5126,
            "count": 3,
            "type": "VEC2"
        }),
    ]);
    root["meshes"][0]["primitives"][0]["attributes"]["TEXCOORD_0"] = serde_json::json!(2);
    root["meshes"][0]["primitives"][0]["attributes"]["TEXCOORD_1"] = serde_json::json!(3);

    let geometry = import_from_gltf(
        &root.to_string(),
        Some(&triangle_buffer_with_two_uv_sets()),
        &GltfImportOptions::default(),
    )
    .unwrap();
    let vertices = geometry.unpack_section_vertices(0).unwrap();

    assert_eq!(geometry.sections[0].vert_size, 28);
    assert_eq!(vertices[0].num_texcoords, 2);
    assert_near(vertices[1].texcoords[0][0], 1.0);
    assert_near(vertices[1].texcoords[1][0], 0.75);
    assert_near(vertices[1].diffuse[0], 1.0);
}

#[test]
fn applies_mesh_node_translation_rotation_and_scale() {
    let mut root: serde_json::Value = serde_json::from_str(&triangle_gltf(true)).unwrap();
    root["nodes"][0]["translation"] = serde_json::json!([5.0, 7.0, 0.0]);
    root["nodes"][0]["rotation"] = serde_json::json!([
        0.0,
        0.0,
        core::f32::consts::FRAC_1_SQRT_2,
        core::f32::consts::FRAC_1_SQRT_2
    ]);
    root["nodes"][0]["scale"] = serde_json::json!([2.0, 3.0, 1.0]);

    let geometry = import_value(&root);
    let vertices = geometry.unpack_section_vertices(0).unwrap();

    assert_eq!(geometry.sections[0].vert_size, 20);
    assert_near(vertices[0].position[0], 5.0);
    assert_near(vertices[0].position[1], 7.0);
    assert_near(vertices[1].position[0], 5.0);
    assert_near(vertices[1].position[1], 9.0);
    assert_near(vertices[2].position[0], 2.0);
    assert_near(vertices[2].position[1], 7.0);
    assert_near(vertices[0].normal[0], -1.0);
    assert_near(vertices[0].normal[1], 0.0);
}

#[test]
fn imports_each_mesh_node_instance() {
    let mut root: serde_json::Value = serde_json::from_str(&triangle_gltf(true)).unwrap();
    root["nodes"] = serde_json::json!([
        {"mesh": 0},
        {"mesh": 0, "translation": [10.0, 0.0, 0.0]}
    ]);
    root["scenes"][0]["nodes"] = serde_json::json!([0, 1]);

    let geometry = import_value(&root);

    assert_eq!(geometry.sections.len(), 2);
    assert_eq!(geometry.total_vertices(), 6);
    assert_near(geometry.bounds.min[0], 0.0);
    assert_near(geometry.bounds.max[0], 11.0);
}

#[test]
fn imports_only_instances_from_the_default_scene() {
    let mut root: serde_json::Value = serde_json::from_str(&triangle_gltf(true)).unwrap();
    root["nodes"] = serde_json::json!([
        {"mesh": 0},
        {"mesh": 0, "translation": [10.0, 0.0, 0.0]}
    ]);
    root["scenes"] = serde_json::json!([
        {"nodes": [0], "extras": {"ugx_max_instances": 2}},
        {"nodes": [1], "extras": {"ugx_max_instances": 7}}
    ]);
    root["scene"] = serde_json::json!(1);

    let geometry = import_value(&root);

    assert_eq!(geometry.sections.len(), 1);
    assert_eq!(geometry.max_instances, 7);
    assert_near(geometry.bounds.min[0], 10.0);
    assert_near(geometry.bounds.max[0], 11.0);
}

#[test]
fn composes_parent_and_mesh_node_transforms() {
    let mut root: serde_json::Value = serde_json::from_str(&triangle_gltf(true)).unwrap();
    root["nodes"] = serde_json::json!([
        {"translation": [2.0, 0.0, 0.0], "children": [1]},
        {"mesh": 0, "translation": [3.0, 0.0, 0.0]}
    ]);
    root["scenes"][0]["nodes"] = serde_json::json!([0]);

    let geometry = import_value(&root);
    let vertices = geometry.unpack_section_vertices(0).unwrap();

    assert_near(vertices[0].position[0], 5.0);
    assert_near(vertices[1].position[0], 6.0);
}

#[test]
fn reverses_winding_for_mirrored_mesh_nodes() {
    let mut root: serde_json::Value = serde_json::from_str(&triangle_gltf(true)).unwrap();
    root["nodes"][0]["scale"] = serde_json::json!([-1.0, 1.0, 1.0]);

    let geometry = import_value(&root);

    assert_eq!(geometry.index_buffer, [0, 2, 1]);
}

#[test]
fn rejects_non_triangle_primitives() {
    let mut root: serde_json::Value = serde_json::from_str(&triangle_gltf(true)).unwrap();
    root["meshes"][0]["primitives"][0]["mode"] = serde_json::json!(1);

    let error = try_import_value(&root).unwrap_err();

    assert!(error.to_string().contains("TRIANGLES"), "{error}");
}

#[test]
fn rejects_incomplete_triangles() {
    let mut root: serde_json::Value = serde_json::from_str(&triangle_gltf(true)).unwrap();
    root["accessors"][1]["count"] = serde_json::json!(2);

    let error = try_import_value(&root).unwrap_err();

    assert!(error.to_string().contains("divisible by three"), "{error}");
}

#[test]
fn rejects_non_affine_mesh_transform() {
    let mut root: serde_json::Value = serde_json::from_str(&triangle_gltf(true)).unwrap();
    root["nodes"][0]["matrix"] = serde_json::json!([
        1.0, 0.0, 0.0, 0.5, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0
    ]);

    let error = try_import_value(&root).unwrap_err();

    assert!(error.to_string().contains("must be affine"), "{error}");
}

#[test]
fn rejects_multiple_skins() {
    let mut root: serde_json::Value = serde_json::from_str(&triangle_gltf(true)).unwrap();
    root["skins"] = serde_json::json!([{"joints": []}, {"joints": []}]);

    let error = try_import_value(&root).unwrap_err();

    assert!(error.to_string().contains("only one glTF skin"), "{error}");
}

#[test]
fn disabled_material_import_maps_sections_to_the_default_material() {
    let mut root: serde_json::Value = serde_json::from_str(&triangle_gltf(true)).unwrap();
    root["materials"] = serde_json::json!([{"name": "first"}, {"name": "second"}]);
    root["meshes"][0]["primitives"][0]["material"] = serde_json::json!(1);

    let geometry = import_from_gltf(
        &root.to_string(),
        Some(&triangle_buffer()),
        &GltfImportOptions {
            include_materials: false,
            ..GltfImportOptions::default()
        },
    )
    .unwrap();

    assert_eq!(geometry.materials.len(), 1);
    assert_eq!(geometry.sections[0].material_index, 0);
}
