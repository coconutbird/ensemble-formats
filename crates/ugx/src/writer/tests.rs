use super::*;
use crate::types::*;
use crate::vertex::element::VertexElementType;
use crate::vertex::packer::{MAX_UV, UnivertPacker, UnpackedVertex};
use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec;

fn assert_float_array_bits_eq<const N: usize>(actual: &[f32; N], expected: &[f32; N]) {
    for (actual, expected) in actual.iter().zip(expected) {
        assert_eq!(actual.to_bits(), expected.to_bits());
    }
}

fn assert_float_bits_eq(actual: f32, expected: f32) {
    assert_eq!(actual.to_bits(), expected.to_bits());
}

fn make_test_packer() -> UnivertPacker {
    let mut uv_types = [VertexElementType::Ignore; MAX_UV];
    uv_types[0] = VertexElementType::HalfFloat2;
    UnivertPacker {
        pack_order: "PT0NA0".to_string(),
        decl_order: String::new(),
        pos_type: VertexElementType::HalfFloat4,
        basis_type: VertexElementType::Ignore,
        basis_scale_type: VertexElementType::Ignore,
        tangent_type: VertexElementType::Dec3N,
        normal_type: VertexElementType::Dec3N,
        uv_types,
        indices_type: VertexElementType::Ignore,
        weights_type: VertexElementType::Ignore,
        diffuse_type: VertexElementType::Ignore,
        index_type: VertexElementType::Ignore,
    }
}

fn make_test_vertex(position: [f32; 3], texcoord: [f32; 2]) -> UnpackedVertex {
    let mut texcoords = [[0.0; 2]; MAX_UV];
    texcoords[0] = texcoord;
    UnpackedVertex {
        position,
        normal: [0.0, 1.0, 0.0],
        tangent: [1.0, 0.0, 0.0, 1.0],
        texcoords,
        num_texcoords: 1,
        ..Default::default()
    }
}

fn make_test_vertex_buffer(packer: &UnivertPacker) -> Vec<u8> {
    let vertices = [
        make_test_vertex([0.0, 0.0, 0.0], [0.0, 0.0]),
        make_test_vertex([1.0, 0.0, 0.0], [1.0, 0.0]),
        make_test_vertex([0.0, 1.0, 0.0], [0.0, 1.0]),
    ];
    let mut buffer = Vec::new();
    for vertex in &vertices {
        packer.pack_vertex(&mut buffer, vertex, 1.0);
    }
    buffer
}

fn make_test_section(vert_size: i32, vb_bytes: i32) -> Section {
    Section {
        material_index: -1,
        accessory_index: -1,
        max_bones: 0,
        rigid_bone_index: -1,
        ib_offset: 0,
        num_tris: 1,
        vb_offset: 0,
        vb_bytes,
        vert_size,
        num_verts: 3,
        base_vert_packer: None,
        bone_remap: Vec::new(),
        rigid_only: true,
        global_bones: false,
        lod_near_distance: 0.0,
        lod_far_distance: f32::MAX,
        lod_fade_distance: 0.0,
    }
}

/// Create a minimal HW2-format test `UgxGeom` with one section and two bones.
///
/// Uses `HalfFloat4` positions, `Dec3N` normals, `HalfFloat2` UVs (20-byte vertex)
/// matching HW2 vertex layout. Section has `base_vert_packer: None`.
fn make_test_geom() -> UgxGeom {
    let packer = make_test_packer();
    let vertex_buffer = make_test_vertex_buffer(&packer);
    let vert_size = i32::try_from(packer.vertex_size()).unwrap();
    let vb_bytes = i32::try_from(vertex_buffer.len()).unwrap();
    let section = make_test_section(vert_size, vb_bytes);

    let bones = vec![
        Bone {
            name: "root".to_string(),
            parent_index: -1,
            model_to_bone: Matrix4x4::identity(),
        },
        Bone {
            name: "child".to_string(),
            parent_index: 0,
            model_to_bone: Matrix4x4::identity(),
        },
    ];

    UgxGeom {
        bounding_sphere: Sphere {
            center: [0.5, 0.5, 0.0],
            radius: 1.0,
        },
        bounds: AABB {
            min: [0.0, 0.0, 0.0],
            max: [1.0, 1.0, 0.0],
        },
        materials: Vec::new(),
        bones,
        granny_bones: Vec::new(),
        granny_meshes: Vec::new(),
        skeleton_lod_type: 0,
        bone_bounds: vec![
            AABB {
                min: [0.0, 0.0, 0.0],
                max: [1.0, 1.0, 0.0],
            },
            AABB {
                min: [-1.0, -1.0, -1.0],
                max: [1.0, 1.0, 1.0],
            },
        ],
        sections: vec![section],
        accessories: Vec::new(),
        valid_accessories: Vec::new(),
        vertex_buffer,
        index_buffer: vec![0, 1, 2],
        rigid_only: true,
        rigid_bone_index: 0,
        max_instances: 1,
        instance_index_multiplier: 4,
        large_geom_bone_index: i16::MAX,
        flags: GeometryFlags {
            all_sections_rigid: true,
            all_sections_skinned: false,
            global_bones: false,
        },
        aabb_tree: None,
    }
}

#[test]
fn test_write_read_roundtrip() {
    let original = make_test_geom();
    let bytes = write_ugx(&original, UgxVersion::Hw2).unwrap();
    let read_back = crate::Reader::read(&bytes).unwrap();

    assert_eq!(read_back.rigid_bone_index, original.rigid_bone_index);
    assert_eq!(read_back.rigid_only, original.rigid_only);
    assert_eq!(
        read_back.flags.all_sections_rigid,
        original.flags.all_sections_rigid
    );
    assert_eq!(
        read_back.flags.all_sections_skinned,
        original.flags.all_sections_skinned
    );
    assert_eq!(read_back.flags.global_bones, original.flags.global_bones);
    assert_float_array_bits_eq(
        &read_back.bounding_sphere.center,
        &original.bounding_sphere.center,
    );
    assert_float_bits_eq(
        read_back.bounding_sphere.radius,
        original.bounding_sphere.radius,
    );
    assert_float_array_bits_eq(&read_back.bounds.min, &original.bounds.min);
    assert_float_array_bits_eq(&read_back.bounds.max, &original.bounds.max);

    assert_eq!(read_back.sections.len(), original.sections.len());
    let s_orig = &original.sections[0];
    let s_read = &read_back.sections[0];
    assert_eq!(s_read.material_index, s_orig.material_index);
    assert_eq!(s_read.num_tris, s_orig.num_tris);
    assert_eq!(s_read.num_verts, s_orig.num_verts);
    assert_eq!(s_read.vert_size, s_orig.vert_size);
    assert_eq!(s_read.vb_offset, s_orig.vb_offset);
    assert_eq!(s_read.vb_bytes, s_orig.vb_bytes);
    assert_eq!(s_read.ib_offset, s_orig.ib_offset);
    // HW2 sections have no UnivertPacker
    assert!(s_read.base_vert_packer.is_none());
    assert!(s_orig.base_vert_packer.is_none());

    assert_eq!(read_back.bones.len(), original.bones.len());
    for (b_orig, b_read) in original.bones.iter().zip(read_back.bones.iter()) {
        assert_eq!(b_read.name, b_orig.name);
        assert_eq!(b_read.parent_index, b_orig.parent_index);
    }

    assert_eq!(read_back.bone_bounds.len(), original.bone_bounds.len());
    for (bb_orig, bb_read) in original
        .bone_bounds
        .iter()
        .zip(read_back.bone_bounds.iter())
    {
        assert_float_array_bits_eq(&bb_read.min, &bb_orig.min);
        assert_float_array_bits_eq(&bb_read.max, &bb_orig.max);
    }

    // HW2 uses inferred vertex unpacking — compare read-back vertices
    // with approximate equality (half-float and Dec3N lose precision).
    let read_verts = read_back.unpack_section_vertices(0).unwrap();
    assert_eq!(read_verts.len(), 3);
    // Vertex 0: position [0, 0, 0]
    assert!((read_verts[0].position[0]).abs() < 0.01);
    assert!((read_verts[0].position[1]).abs() < 0.01);
    // Vertex 1: position [1, 0, 0]
    assert!((read_verts[1].position[0] - 1.0).abs() < 0.01);
    assert!((read_verts[1].position[1]).abs() < 0.01);
    // Vertex 2: position [0, 1, 0]
    assert!((read_verts[2].position[0]).abs() < 0.01);
    assert!((read_verts[2].position[1] - 1.0).abs() < 0.01);

    let orig_indices = original.get_section_indices(0).unwrap();
    let read_indices = read_back.get_section_indices(0).unwrap();
    assert_eq!(read_indices, orig_indices);
}

#[test]
fn test_write_read_materials_roundtrip() {
    let mut geom = make_test_geom();
    geom.materials = vec![
        Material {
            name: "terrain_grass".to_string(),
            material_version: 4,
            data: MaterialData::Legacy(Box::new(LegacyMaterialData {
                spec_power: 25.0,
                flags: 3,
                blend_type: 1,
                opacity: 0.8,
                maps: {
                    let mut maps: [Vec<Map>; MapType::NUM_TYPES] = Default::default();
                    maps[MapType::Diffuse as usize] = vec![Map {
                        name: "art/textures/grass_diff.ddx".to_string(),
                        channel: 0,
                        flags: 7,
                    }];
                    maps[MapType::Normal as usize] = vec![Map {
                        name: "art/textures/grass_norm.ddx".to_string(),
                        channel: 0,
                        flags: 7,
                    }];
                    maps
                },
                ..LegacyMaterialData::default()
            })),
        },
        Material {
            name: "metal_plate".to_string(),
            material_version: 4,
            data: MaterialData::Legacy(Box::new(LegacyMaterialData {
                spec_power: 50.0,
                maps: {
                    let mut maps: [Vec<Map>; MapType::NUM_TYPES] = Default::default();
                    maps[MapType::Diffuse as usize] = vec![Map {
                        name: "art/textures/metal_diff.ddx".to_string(),
                        channel: 0,
                        flags: 7,
                    }];
                    maps[MapType::Gloss as usize] = vec![Map {
                        name: "art/textures/metal_gloss.ddx".to_string(),
                        channel: 1,
                        flags: 3,
                    }];
                    maps
                },
                ..LegacyMaterialData::default()
            })),
        },
    ];

    let bytes = write_ugx(&geom, UgxVersion::Hw2).unwrap();
    let read_back = crate::Reader::read(&bytes).unwrap();

    assert_eq!(read_back.materials.len(), 2);
    let m0 = &read_back.materials[0];
    let l0 = m0.legacy().expect("expected legacy material");
    assert_eq!(m0.name, "terrain_grass");
    assert!((l0.spec_power - 25.0).abs() < 0.1);
    assert_eq!(l0.flags, 3);
    assert_eq!(l0.blend_type, 1);
    assert!((l0.opacity - 0.8).abs() < 0.01);
    assert_eq!(l0.maps[MapType::Diffuse as usize].len(), 1);
    assert_eq!(
        l0.maps[MapType::Diffuse as usize][0].name,
        "art/textures/grass_diff.ddx"
    );
    assert_eq!(l0.maps[MapType::Normal as usize].len(), 1);

    let m1 = &read_back.materials[1];
    let l1 = m1.legacy().expect("expected legacy material");
    assert_eq!(m1.name, "metal_plate");
    assert!((l1.spec_power - 50.0).abs() < 0.1);
    assert_eq!(l1.maps[MapType::Gloss as usize].len(), 1);
    assert_eq!(
        l1.maps[MapType::Gloss as usize][0].name,
        "art/textures/metal_gloss.ddx"
    );
    assert_eq!(l1.maps[MapType::Gloss as usize][0].channel, 1);
    assert!(l0.maps[MapType::Gloss as usize].is_empty());
    assert!(l1.maps[MapType::Normal as usize].is_empty());
}

#[test]
fn test_write_read_granny_bones_roundtrip() {
    let mut geom = make_test_geom();
    geom.granny_bones = vec![
        GrannyBone {
            name: "root".to_string(),
            parent_index: -1,
            local_transform: None,
            inverse_world_matrix: Matrix4x4 {
                rows: [
                    [1.0, 0.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                    [0.0, 0.0, 0.0, 1.0],
                ],
            },
            lod_error: 0.0,
            extended_data: None,
            extended_data_type: None,
        },
        GrannyBone {
            name: "spine".to_string(),
            parent_index: 0,
            local_transform: None,
            inverse_world_matrix: Matrix4x4 {
                rows: [
                    [1.0, 0.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                    [0.0, -1.0, 0.0, 0.0],
                    [0.5, -2.0, 1.5, 1.0],
                ],
            },
            lod_error: 0.0,
            extended_data: None,
            extended_data_type: None,
        },
    ];

    let bytes = write_ugx(&geom, UgxVersion::Hw2).unwrap();
    let read_back = crate::Reader::read(&bytes).unwrap();

    assert_eq!(read_back.granny_bones.len(), 2);

    let gb0 = &read_back.granny_bones[0];
    assert_eq!(gb0.name, "root");
    assert_eq!(gb0.parent_index, -1);
    for row in 0..4 {
        for col in 0..4 {
            let expected = if row == col { 1.0 } else { 0.0 };
            assert!(
                (gb0.inverse_world_matrix.rows[row][col] - expected).abs() < 1e-6,
                "gb0 matrix[{}][{}] = {}, expected {}",
                row,
                col,
                gb0.inverse_world_matrix.rows[row][col],
                expected
            );
        }
    }

    let gb1 = &read_back.granny_bones[1];
    assert_eq!(gb1.name, "spine");
    assert_eq!(gb1.parent_index, 0);
    let expected_rows = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, -1.0, 0.0, 0.0],
        [0.5, -2.0, 1.5, 1.0],
    ];
    for (row, expected_row) in expected_rows.iter().enumerate() {
        for (col, expected_val) in expected_row.iter().enumerate() {
            assert!(
                (gb1.inverse_world_matrix.rows[row][col] - expected_val).abs() < 1e-6,
                "gb1 matrix[{}][{}] = {}, expected {}",
                row,
                col,
                gb1.inverse_world_matrix.rows[row][col],
                expected_val
            );
        }
    }
}
