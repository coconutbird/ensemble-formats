//! Integration tests for glTF ↔ UGX roundtrip conversions.
//!
//! These tests exercise the public API of ugx-gltf, verifying that
//! export → import and full UGX → glTF → UGX → binary roundtrips
//! preserve geometry, skeleton, material, and vertex format data.

use gltf_json::validation::Checked::Valid;
use ugx::*;
use ugx_gltf::{
    GltfExportOptions, GltfImportOptions, export_to_gltf, export_to_gltf_with_buffer_name,
    import_from_gltf,
};

/// Create a test UgxGeom with vertices, bones, and skin data.
fn make_test_geom() -> UgxGeom {
    let mut uv_types = [VertexElementType::Ignore; MAX_UV];
    uv_types[0] = VertexElementType::Float2;

    let packer = UnivertPacker {
        pack_order: "PNA0T0S".to_string(),
        decl_order: "PNA0T0S".to_string(),
        pos_type: VertexElementType::Float3,
        basis_type: VertexElementType::Float4,
        basis_scale_type: VertexElementType::Float2,
        tangent_type: VertexElementType::Float4,
        normal_type: VertexElementType::Float3,
        uv_types,
        indices_type: VertexElementType::UByte4,
        weights_type: VertexElementType::Float4,
        diffuse_type: VertexElementType::Ignore,
        index_type: VertexElementType::Ignore,
    };

    let vertices = vec![
        UnpackedVertex {
            position: [0.0, 0.0, 0.0],
            normal: [0.0, 1.0, 0.0],
            tangent: [1.0, 0.0, 0.0, 1.0],
            texcoords: {
                let mut tc = [[0.0; 2]; MAX_UV];
                tc[0] = [0.0, 0.0];
                tc
            },
            num_texcoords: 1,
            bone_indices: [1, 0, 0, 0],
            bone_weights: [1.0, 0.0, 0.0, 0.0],
            ..Default::default()
        },
        UnpackedVertex {
            position: [1.0, 0.0, 0.0],
            normal: [0.0, 1.0, 0.0],
            tangent: [1.0, 0.0, 0.0, 1.0],
            texcoords: {
                let mut tc = [[0.0; 2]; MAX_UV];
                tc[0] = [1.0, 0.0];
                tc
            },
            num_texcoords: 1,
            bone_indices: [1, 2, 0, 0],
            bone_weights: [0.7, 0.3, 0.0, 0.0],
            ..Default::default()
        },
        UnpackedVertex {
            position: [0.0, 1.0, 0.0],
            normal: [0.0, 1.0, 0.0],
            tangent: [1.0, 0.0, 0.0, -1.0],
            texcoords: {
                let mut tc = [[0.0; 2]; MAX_UV];
                tc[0] = [0.0, 1.0];
                tc
            },
            num_texcoords: 1,
            bone_indices: [2, 0, 0, 0],
            bone_weights: [1.0, 0.0, 0.0, 0.0],
            ..Default::default()
        },
    ];

    let mut vertex_buffer = Vec::new();
    for v in &vertices {
        packer.pack_vertex(&mut vertex_buffer, v);
    }

    let vert_size = packer.vertex_size() as i32;
    let vb_bytes = vertex_buffer.len() as i32;

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

    let granny_bones = vec![
        GrannyBone {
            name: "root".to_string(),
            parent_index: -1,
            inverse_world_matrix: Matrix4x4::identity(),
        },
        GrannyBone {
            name: "child".to_string(),
            parent_index: 0,
            inverse_world_matrix: Matrix4x4::identity(),
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
        granny_bones,
        granny_meshes: Vec::new(),
        bone_bounds: vec![
            AABB {
                min: [0.0, 0.0, 0.0],
                max: [1.0, 1.0, 0.0],
            },
            AABB {
                min: [0.0, 0.0, 0.0],
                max: [1.0, 1.0, 0.0],
            },
        ],
        sections: vec![Section {
            material_index: -1,
            accessory_index: -1,
            max_bones: 2,
            rigid_bone_index: -1,
            ib_offset: 0,
            num_tris: 1,
            vb_offset: 0,
            vb_bytes,
            vert_size,
            num_verts: 3,
            base_vert_packer: packer,
            bone_remap: Vec::new(),
            rigid_only: false,
            global_bones: false,
        }],
        vertex_buffer,
        index_buffer: vec![0, 1, 2],
        rigid_only: false,
        rigid_bone_index: -1,
        all_sections_rigid: false,
        all_sections_skinned: true,
        global_bones: false,
    }
}

#[test]
fn test_gltf_import_roundtrip_positions() {
    let original = make_test_geom();

    // Export to glTF with external buffer
    let opts = GltfExportOptions {
        embed_buffers: false,
        include_materials: false,
        include_skeleton: false,
    };
    let export = export_to_gltf(&original, &opts).unwrap();

    // Import back
    let import_opts = GltfImportOptions {
        include_skeleton: false,
        include_materials: false,
    };
    let imported = import_from_gltf(&export.json, export.buffer.as_deref(), &import_opts).unwrap();

    // Compare vertex positions
    assert_eq!(imported.sections.len(), 1);
    let orig_verts = original.unpack_section_vertices(0).unwrap();
    let imported_verts = imported.unpack_section_vertices(0).unwrap();
    assert_eq!(imported_verts.len(), orig_verts.len());

    for (orig, imp) in orig_verts.iter().zip(imported_verts.iter()) {
        for i in 0..3 {
            assert!(
                (orig.position[i] - imp.position[i]).abs() < 1e-5,
                "position[{}] mismatch: {} vs {}",
                i,
                orig.position[i],
                imp.position[i]
            );
        }
    }
}

#[test]
fn test_gltf_import_roundtrip_normals_uvs() {
    let original = make_test_geom();

    let opts = GltfExportOptions {
        embed_buffers: false,
        include_materials: false,
        include_skeleton: false,
    };
    let export = export_to_gltf(&original, &opts).unwrap();

    let import_opts = GltfImportOptions {
        include_skeleton: false,
        include_materials: false,
    };
    let imported = import_from_gltf(&export.json, export.buffer.as_deref(), &import_opts).unwrap();

    let orig_verts = original.unpack_section_vertices(0).unwrap();
    let imported_verts = imported.unpack_section_vertices(0).unwrap();

    for (orig, imp) in orig_verts.iter().zip(imported_verts.iter()) {
        // Normals
        for i in 0..3 {
            assert!(
                (orig.normal[i] - imp.normal[i]).abs() < 1e-4,
                "normal[{}] mismatch: {} vs {}",
                i,
                orig.normal[i],
                imp.normal[i]
            );
        }
        // UVs
        for i in 0..2 {
            assert!(
                (orig.texcoords[0][i] - imp.texcoords[0][i]).abs() < 1e-5,
                "uv[0][{}] mismatch: {} vs {}",
                i,
                orig.texcoords[0][i],
                imp.texcoords[0][i]
            );
        }
    }
}

#[test]
fn test_gltf_import_roundtrip_skeleton() {
    let original = make_test_geom();

    let opts = GltfExportOptions {
        embed_buffers: false,
        include_materials: false,
        include_skeleton: true,
    };
    let export = export_to_gltf(&original, &opts).unwrap();

    let import_opts = GltfImportOptions {
        include_skeleton: true,
        include_materials: false,
    };
    let imported = import_from_gltf(&export.json, export.buffer.as_deref(), &import_opts).unwrap();

    // Check bone count and names
    assert_eq!(imported.bones.len(), original.bones.len());
    for (orig, imp) in original.bones.iter().zip(imported.bones.iter()) {
        assert_eq!(imp.name, orig.name);
        assert_eq!(imp.parent_index, orig.parent_index);
    }
}

#[test]
fn test_gltf_import_roundtrip_skin_data() {
    let original = make_test_geom();

    let opts = GltfExportOptions {
        embed_buffers: false,
        include_materials: false,
        include_skeleton: true,
    };
    let export = export_to_gltf(&original, &opts).unwrap();

    let import_opts = GltfImportOptions {
        include_skeleton: true,
        include_materials: false,
    };
    let imported = import_from_gltf(&export.json, export.buffer.as_deref(), &import_opts).unwrap();

    let orig_verts = original.unpack_section_vertices(0).unwrap();
    let imported_verts = imported.unpack_section_vertices(0).unwrap();

    for (orig, imp) in orig_verts.iter().zip(imported_verts.iter()) {
        // Bone indices with non-zero weights should match
        // Indices with zero weights may be padded with valid bone indices (game requirement)
        for k in 0..4 {
            if orig.bone_weights[k] > 0.0 {
                assert_eq!(
                    imp.bone_indices[k], orig.bone_indices[k],
                    "bone_indices[{}] mismatch for weighted bone: {:?} vs {:?}",
                    k, imp.bone_indices, orig.bone_indices
                );
            } else {
                // For zero-weight slots, just verify the index is valid (non-zero for 1-based)
                // Game expects valid bone indices even for unused slots
                assert!(
                    imp.bone_indices[k] > 0,
                    "bone_indices[{}] should be valid (>0) for game compatibility, got {}",
                    k,
                    imp.bone_indices[k]
                );
            }
        }

        // Bone weights should be close (UByte4N has ~1/255 precision)
        for k in 0..4 {
            assert!(
                (orig.bone_weights[k] - imp.bone_weights[k]).abs() < 0.01,
                "bone_weight[{}] mismatch: {} vs {}",
                k,
                orig.bone_weights[k],
                imp.bone_weights[k]
            );
        }
    }
}

#[test]
fn test_gltf_import_embedded_base64() {
    let original = make_test_geom();

    // Export with embedded base64
    let opts = GltfExportOptions {
        embed_buffers: true,
        include_materials: false,
        include_skeleton: false,
    };
    let export = export_to_gltf(&original, &opts).unwrap();
    assert!(
        export.buffer.is_none(),
        "embedded export should not have separate buffer"
    );

    // Import with no external buffer (should decode from base64)
    let import_opts = GltfImportOptions {
        include_skeleton: false,
        include_materials: false,
    };
    let imported = import_from_gltf(&export.json, None, &import_opts).unwrap();

    let orig_verts = original.unpack_section_vertices(0).unwrap();
    let imported_verts = imported.unpack_section_vertices(0).unwrap();
    assert_eq!(imported_verts.len(), orig_verts.len());
    for (orig, imp) in orig_verts.iter().zip(imported_verts.iter()) {
        assert_eq!(orig.position, imp.position);
    }
}

#[test]
fn test_full_roundtrip_ugx_gltf_ugx() {
    let original = make_test_geom();

    // 1. Export to glTF
    let export_opts = GltfExportOptions {
        embed_buffers: false,
        include_materials: false,
        include_skeleton: true,
    };
    let export = export_to_gltf(&original, &export_opts).unwrap();

    // 2. Import from glTF
    let import_opts = GltfImportOptions {
        include_skeleton: true,
        include_materials: false,
    };
    let imported = import_from_gltf(&export.json, export.buffer.as_deref(), &import_opts).unwrap();

    // 3. Write to UGX bytes
    let ugx_bytes = ugx::Writer::write(&imported).unwrap();

    // 4. Read back
    let re_read = ugx::Reader::read(&ugx_bytes).unwrap();

    // 5. Verify
    assert_eq!(re_read.sections.len(), 1);
    assert_eq!(re_read.bones.len(), original.bones.len());

    let orig_verts = original.unpack_section_vertices(0).unwrap();
    let final_verts = re_read.unpack_section_vertices(0).unwrap();
    assert_eq!(final_verts.len(), orig_verts.len());

    for (vi, (orig, fin)) in orig_verts.iter().zip(final_verts.iter()).enumerate() {
        // Positions
        for i in 0..3 {
            assert!(
                (orig.position[i] - fin.position[i]).abs() < 1e-4,
                "vertex {} position[{}] mismatch: {} vs {}",
                vi,
                i,
                orig.position[i],
                fin.position[i]
            );
        }

        // Normals
        for i in 0..3 {
            assert!(
                (orig.normal[i] - fin.normal[i]).abs() < 1e-4,
                "vertex {} normal[{}] mismatch: {} vs {}",
                vi,
                i,
                orig.normal[i],
                fin.normal[i]
            );
        }

        // Tangents (only check xyz - game uses Float3 which doesn't store w/handedness)
        for i in 0..3 {
            assert!(
                (orig.tangent[i] - fin.tangent[i]).abs() < 1e-4,
                "vertex {} tangent[{}] mismatch: {} vs {}",
                vi,
                i,
                orig.tangent[i],
                fin.tangent[i]
            );
        }

        // UVs
        for i in 0..orig.num_texcoords {
            for j in 0..2 {
                assert!(
                    (orig.texcoords[i][j] - fin.texcoords[i][j]).abs() < 1e-4,
                    "vertex {} texcoord[{}][{}] mismatch: {} vs {}",
                    vi,
                    i,
                    j,
                    orig.texcoords[i][j],
                    fin.texcoords[i][j]
                );
            }
        }

        // Bone indices - only check indices with non-zero weights
        // Indices with zero weights may be padded with valid bone indices (game requirement)
        for i in 0..4 {
            if orig.bone_weights[i] > 0.0 {
                assert_eq!(
                    orig.bone_indices[i], fin.bone_indices[i],
                    "vertex {} bone_indices[{}] mismatch for weighted bone: {:?} vs {:?}",
                    vi, i, orig.bone_indices, fin.bone_indices
                );
            } else {
                // For zero-weight slots, just verify the index is valid (non-zero for 1-based)
                assert!(
                    fin.bone_indices[i] > 0,
                    "vertex {} bone_indices[{}] should be valid (>0) for game compatibility, got {}",
                    vi,
                    i,
                    fin.bone_indices[i]
                );
            }
        }

        // Bone weights (UByte4N has ~1/255 precision)
        for i in 0..4 {
            assert!(
                (orig.bone_weights[i] - fin.bone_weights[i]).abs() < 0.01,
                "vertex {} bone_weights[{}] mismatch: {} vs {}",
                vi,
                i,
                orig.bone_weights[i],
                fin.bone_weights[i]
            );
        }
    }

    // Verify indices survived
    let orig_indices = original.get_section_indices(0);
    let final_indices = re_read.get_section_indices(0);
    assert_eq!(final_indices, orig_indices);

    // Verify bone names and hierarchy survived
    assert_eq!(re_read.bones.len(), original.bones.len());
    for (bi, (orig_bone, fin_bone)) in original.bones.iter().zip(re_read.bones.iter()).enumerate() {
        assert_eq!(
            orig_bone.name, fin_bone.name,
            "bone {} name mismatch: {:?} vs {:?}",
            bi, orig_bone.name, fin_bone.name
        );
        assert_eq!(
            orig_bone.parent_index, fin_bone.parent_index,
            "bone {} parent_index mismatch: {} vs {}",
            bi, orig_bone.parent_index, fin_bone.parent_index
        );
    }

    // Verify granny bones survived the round trip
    assert_eq!(
        re_read.granny_bones.len(),
        original.granny_bones.len(),
        "granny_bones count mismatch: {} vs {}",
        re_read.granny_bones.len(),
        original.granny_bones.len()
    );
    for (bi, (orig_gb, fin_gb)) in original
        .granny_bones
        .iter()
        .zip(re_read.granny_bones.iter())
        .enumerate()
    {
        assert_eq!(
            orig_gb.name, fin_gb.name,
            "granny_bone {} name mismatch: {:?} vs {:?}",
            bi, orig_gb.name, fin_gb.name
        );
        assert_eq!(
            orig_gb.parent_index, fin_gb.parent_index,
            "granny_bone {} parent_index mismatch: {} vs {}",
            bi, orig_gb.parent_index, fin_gb.parent_index
        );
        // Compare inverse world matrices
        for row in 0..4 {
            for col in 0..4 {
                assert!(
                    (orig_gb.inverse_world_matrix.rows[row][col]
                        - fin_gb.inverse_world_matrix.rows[row][col])
                        .abs()
                        < 1e-4,
                    "granny_bone {} matrix[{}][{}] mismatch: {} vs {}",
                    bi,
                    row,
                    col,
                    orig_gb.inverse_world_matrix.rows[row][col],
                    fin_gb.inverse_world_matrix.rows[row][col]
                );
            }
        }
    }

    // Verify bounding volumes are reasonable
    assert!(
        re_read.bounding_sphere.radius > 0.0,
        "bounding sphere radius should be positive"
    );
    for i in 0..3 {
        assert!(
            re_read.bounds.max[i] >= re_read.bounds.min[i],
            "AABB max[{}] < min[{}]",
            i,
            i
        );
    }
}

/// Test that glTF import generates granny_meshes from vertex skin data.
/// This is critical for the game to properly skin the model.
#[test]
fn test_gltf_import_generates_granny_meshes() {
    let original = make_test_geom();

    // Export to glTF with skeleton
    let export_opts = GltfExportOptions {
        embed_buffers: false,
        include_materials: false,
        include_skeleton: true,
    };
    let export = export_to_gltf(&original, &export_opts).unwrap();

    // Import from glTF
    let import_opts = GltfImportOptions {
        include_skeleton: true,
        include_materials: false,
    };
    let imported = import_from_gltf(&export.json, export.buffer.as_deref(), &import_opts).unwrap();

    // Verify granny_meshes was generated
    assert!(
        !imported.granny_meshes.is_empty(),
        "granny_meshes should be generated from vertex skin data"
    );

    // Should have exactly one mesh for glTF imports
    assert_eq!(
        imported.granny_meshes.len(),
        1,
        "glTF import should create one mesh with all used bones"
    );

    let mesh = &imported.granny_meshes[0];
    // Mesh name is preserved from glTF export (mesh_0 since test geom has no granny_meshes)
    assert_eq!(mesh.name, "mesh_0");

    // The test geom uses bones 1 and 2 (1-based), which are "root" and "child"
    // Verify bone bindings contain the bones actually used by vertices
    assert!(
        !mesh.bone_bindings.is_empty(),
        "mesh should have bone bindings"
    );

    // Check that the bone names are from our test skeleton ("root" and "child")
    let valid_bones = ["root", "child"];
    for bone_name in &mesh.bone_bindings {
        assert!(
            valid_bones.contains(&bone_name.as_str()),
            "bone binding '{}' should be from test skeleton (root or child)",
            bone_name
        );
    }

    // Write to UGX and read back to verify it survives serialization
    let ugx_bytes = ugx::Writer::write(&imported).unwrap();
    let re_read = ugx::Reader::read(&ugx_bytes).unwrap();

    // Verify granny_meshes survived the round trip
    assert_eq!(
        re_read.granny_meshes.len(),
        1,
        "granny_meshes should survive UGX round trip"
    );
    assert_eq!(re_read.granny_meshes[0].name, "mesh_0");
    assert_eq!(
        re_read.granny_meshes[0].bone_bindings.len(),
        mesh.bone_bindings.len(),
        "bone binding count should survive UGX round trip"
    );
}

/// Round-trip a real Halo Wars UGX file: read → export to glTF → import → write UGX → read back.
/// Compares vertex positions, normals, indices, bone hierarchy, and granny bone data.
#[test]
fn test_real_file_roundtrip() {
    let paths = [
        "../../test_ugx/art/covenant/air/banshee_01/banshee_damage_01.ugx",
        "../../test_ugx/art/covenant/air/spirit_01/spirit_damaged_01.ugx",
        "../../test_ugx/art/covenant/building/barracks_01/barracks_damaged_01.ugx",
    ];

    let mut tested = false;
    for path in &paths {
        let data = match std::fs::read(path) {
            Ok(d) => d,
            Err(_) => continue,
        };

        let original = match ugx::Reader::read(&data) {
            Ok(g) => g,
            Err(e) => {
                eprintln!("Failed to read {}: {}", path, e);
                continue;
            }
        };

        eprintln!("\n=== Testing: {} ===", path);
        eprintln!(
            "  Sections: {}, Bones: {}, Granny bones: {}",
            original.sections.len(),
            original.bones.len(),
            original.granny_bones.len()
        );
        eprintln!(
            "  Total vertices: {}, Total triangles: {}",
            original.total_vertices(),
            original.total_triangles()
        );

        // 1. Export to glTF
        let export_opts = GltfExportOptions {
            embed_buffers: false,
            include_materials: false,
            include_skeleton: true,
        };
        let export = export_to_gltf(&original, &export_opts).unwrap();

        // 2. Import from glTF
        let import_opts = GltfImportOptions {
            include_skeleton: true,
            include_materials: false,
        };
        let imported =
            import_from_gltf(&export.json, export.buffer.as_deref(), &import_opts).unwrap();

        // 3. Verify imported matches original in structure
        assert_eq!(
            imported.sections.len(),
            original.sections.len(),
            "section count mismatch"
        );

        // Check vertex counts match per section
        for (si, (orig_sec, imp_sec)) in original
            .sections
            .iter()
            .zip(imported.sections.iter())
            .enumerate()
        {
            assert_eq!(
                imp_sec.num_verts, orig_sec.num_verts,
                "section {} vertex count mismatch: {} vs {}",
                si, imp_sec.num_verts, orig_sec.num_verts
            );
            assert_eq!(
                imp_sec.num_tris, orig_sec.num_tris,
                "section {} triangle count mismatch: {} vs {}",
                si, imp_sec.num_tris, orig_sec.num_tris
            );
        }

        // Check vertex data for each section
        for si in 0..original.sections.len() {
            let orig_verts = original.unpack_section_vertices(si).unwrap();
            let imp_verts = imported.unpack_section_vertices(si).unwrap();
            assert_eq!(
                imp_verts.len(),
                orig_verts.len(),
                "section {} unpacked vertex count mismatch",
                si
            );

            for (vi, (ov, iv)) in orig_verts.iter().zip(imp_verts.iter()).enumerate() {
                for c in 0..3 {
                    assert!(
                        (ov.position[c] - iv.position[c]).abs() < 0.01,
                        "section {} vertex {} position[{}] mismatch: {} vs {}",
                        si,
                        vi,
                        c,
                        ov.position[c],
                        iv.position[c]
                    );
                }
                // Normals (may lose some precision through half-float or dec3n packing in original)
                let ndot = ov.normal[0] * iv.normal[0]
                    + ov.normal[1] * iv.normal[1]
                    + ov.normal[2] * iv.normal[2];
                assert!(
                    ndot > 0.9 || (ov.normal == [0.0, 0.0, 0.0]),
                    "section {} vertex {} normal diverged too much: {:?} vs {:?} (dot={})",
                    si,
                    vi,
                    ov.normal,
                    iv.normal,
                    ndot
                );
            }

            // Check indices
            let orig_idx = original.get_section_indices(si);
            let imp_idx = imported.get_section_indices(si);
            assert_eq!(imp_idx, orig_idx, "section {} index mismatch", si);
        }

        // Check bone hierarchy
        if !original.bones.is_empty() {
            assert_eq!(
                imported.bones.len(),
                original.bones.len(),
                "bone count mismatch"
            );
            for (bi, (ob, ib)) in original.bones.iter().zip(imported.bones.iter()).enumerate() {
                assert_eq!(ob.name, ib.name, "bone {} name mismatch", bi);
                assert_eq!(
                    ob.parent_index, ib.parent_index,
                    "bone {} parent mismatch",
                    bi
                );
            }
        }

        // Check granny bones
        if !original.granny_bones.is_empty() {
            assert_eq!(
                imported.granny_bones.len(),
                original.granny_bones.len(),
                "granny bone count mismatch"
            );
            for (bi, (og, ig)) in original
                .granny_bones
                .iter()
                .zip(imported.granny_bones.iter())
                .enumerate()
            {
                assert_eq!(og.name, ig.name, "granny bone {} name mismatch", bi);
                assert_eq!(
                    og.parent_index, ig.parent_index,
                    "granny bone {} parent mismatch",
                    bi
                );
                // Compare inverse world matrices (may have some float precision loss)
                for r in 0..4 {
                    for c in 0..4 {
                        assert!(
                            (og.inverse_world_matrix.rows[r][c]
                                - ig.inverse_world_matrix.rows[r][c])
                                .abs()
                                < 1e-3,
                            "granny bone {} matrix[{}][{}] mismatch: {} vs {}",
                            bi,
                            r,
                            c,
                            og.inverse_world_matrix.rows[r][c],
                            ig.inverse_world_matrix.rows[r][c]
                        );
                    }
                }
            }
        }

        // 4. Write to UGX bytes and read back
        let ugx_bytes = ugx::Writer::write(&imported).unwrap();
        let re_read = ugx::Reader::read(&ugx_bytes).unwrap();

        // 5. Verify write→read preserved the data
        assert_eq!(re_read.sections.len(), imported.sections.len());
        for si in 0..imported.sections.len() {
            let imp_verts = imported.unpack_section_vertices(si).unwrap();
            let rr_verts = re_read.unpack_section_vertices(si).unwrap();
            assert_eq!(
                rr_verts.len(),
                imp_verts.len(),
                "write roundtrip section {} vertex count mismatch",
                si
            );

            for (vi, (iv, rv)) in imp_verts.iter().zip(rr_verts.iter()).enumerate() {
                for c in 0..3 {
                    assert!(
                        (iv.position[c] - rv.position[c]).abs() < 1e-4,
                        "write roundtrip section {} vertex {} position[{}] mismatch: {} vs {}",
                        si,
                        vi,
                        c,
                        iv.position[c],
                        rv.position[c]
                    );
                }
            }

            let imp_idx = imported.get_section_indices(si);
            let rr_idx = re_read.get_section_indices(si);
            assert_eq!(
                rr_idx, imp_idx,
                "write roundtrip section {} index mismatch",
                si
            );
        }

        // Verify granny bones survived write→read
        if !imported.granny_bones.is_empty() {
            assert_eq!(re_read.granny_bones.len(), imported.granny_bones.len());
            for (bi, (ig, rg)) in imported
                .granny_bones
                .iter()
                .zip(re_read.granny_bones.iter())
                .enumerate()
            {
                assert_eq!(
                    ig.name, rg.name,
                    "write roundtrip granny bone {} name mismatch",
                    bi
                );
                assert_eq!(
                    ig.parent_index, rg.parent_index,
                    "write roundtrip granny bone {} parent mismatch",
                    bi
                );
                for r in 0..4 {
                    for c in 0..4 {
                        assert!(
                            (ig.inverse_world_matrix.rows[r][c]
                                - rg.inverse_world_matrix.rows[r][c])
                                .abs()
                                < 1e-4,
                            "write roundtrip granny bone {} matrix[{}][{}] mismatch: {} vs {}",
                            bi,
                            r,
                            c,
                            ig.inverse_world_matrix.rows[r][c],
                            rg.inverse_world_matrix.rows[r][c]
                        );
                    }
                }
            }
        }

        eprintln!(
            "  PASSED: {} sections, {} bones, {} granny bones round-tripped",
            re_read.sections.len(),
            re_read.bones.len(),
            re_read.granny_bones.len()
        );
        tested = true;
    }

    if !tested {
        eprintln!("No real UGX files found in test_ugx/ - test skipped");
        eprintln!("Place UGX files in test_ugx/ directory to enable real file testing");
    }
}

/// Round-trip foxcannon UGX files and export glTF (with and without skeleton) to disk.
#[test]
fn test_foxcannon_roundtrip_and_export() {
    let dir = "../../foxcannon01";
    let files = [
        "mesh_barrel_0.ugx",
        "mesh_chassis_front_0.ugx",
        "mesh_foxcannon01.ugx",
        "mesh_turret_0.ugx",
    ];

    let mut tested = false;
    for filename in &files {
        let path = format!("{}/{}", dir, filename);
        let data = match std::fs::read(&path) {
            Ok(d) => d,
            Err(_) => continue,
        };

        let original = match ugx::Reader::read(&data) {
            Ok(g) => g,
            Err(e) => {
                eprintln!("Failed to read {}: {}", path, e);
                continue;
            }
        };

        let stem = filename.trim_end_matches(".ugx");
        eprintln!("\n=== Foxcannon: {} ===", filename);
        eprintln!(
            "  Sections: {}, Bones: {}, Granny bones: {}",
            original.sections.len(),
            original.bones.len(),
            original.granny_bones.len()
        );
        eprintln!(
            "  Total vertices: {}, Total triangles: {}",
            original.total_vertices(),
            original.total_triangles()
        );

        // Export glTF WITHOUT skeleton
        {
            let opts = GltfExportOptions {
                embed_buffers: false,
                include_materials: false,
                include_skeleton: false,
            };
            let buffer_name = format!("{}_no_bones.bin", stem);
            let export = export_to_gltf_with_buffer_name(&original, &opts, &buffer_name).unwrap();
            let json_path = format!("{}/{}_no_bones.gltf", dir, stem);
            let bin_path = format!("{}/{}", dir, buffer_name);
            std::fs::write(&json_path, &export.json).unwrap();
            if let Some(ref buf) = export.buffer {
                std::fs::write(&bin_path, buf).unwrap();
            }
            eprintln!("  Exported (no bones): {}", json_path);
        }

        // Export glTF WITH skeleton
        {
            let opts = GltfExportOptions {
                embed_buffers: false,
                include_materials: false,
                include_skeleton: true,
            };
            let buffer_name = format!("{}_with_bones.bin", stem);
            let export = export_to_gltf_with_buffer_name(&original, &opts, &buffer_name).unwrap();
            let json_path = format!("{}/{}_with_bones.gltf", dir, stem);
            let bin_path = format!("{}/{}", dir, buffer_name);
            std::fs::write(&json_path, &export.json).unwrap();
            if let Some(ref buf) = export.buffer {
                std::fs::write(&bin_path, buf).unwrap();
            }
            eprintln!("  Exported (with bones): {}", json_path);
        }

        // Round-trip: export → import → write UGX → read back
        let export_opts = GltfExportOptions {
            embed_buffers: false,
            include_materials: false,
            include_skeleton: true,
        };
        let export = export_to_gltf(&original, &export_opts).unwrap();

        let import_opts = GltfImportOptions {
            include_skeleton: true,
            include_materials: false,
        };
        let imported =
            import_from_gltf(&export.json, export.buffer.as_deref(), &import_opts).unwrap();

        // Verify section structure
        assert_eq!(
            imported.sections.len(),
            original.sections.len(),
            "{}: section count mismatch",
            filename
        );
        for (si, (orig_sec, imp_sec)) in original
            .sections
            .iter()
            .zip(imported.sections.iter())
            .enumerate()
        {
            assert_eq!(
                imp_sec.num_verts, orig_sec.num_verts,
                "{}: section {} vertex count mismatch",
                filename, si
            );
            assert_eq!(
                imp_sec.num_tris, orig_sec.num_tris,
                "{}: section {} triangle count mismatch",
                filename, si
            );
        }

        // Verify vertex data per section
        for si in 0..original.sections.len() {
            let orig_verts = original.unpack_section_vertices(si).unwrap();
            let imp_verts = imported.unpack_section_vertices(si).unwrap();
            for (vi, (ov, iv)) in orig_verts.iter().zip(imp_verts.iter()).enumerate() {
                for c in 0..3 {
                    assert!(
                        (ov.position[c] - iv.position[c]).abs() < 0.01,
                        "{}: section {} vertex {} position[{}] mismatch: {} vs {}",
                        filename,
                        si,
                        vi,
                        c,
                        ov.position[c],
                        iv.position[c]
                    );
                }
            }

            let orig_idx = original.get_section_indices(si);
            let imp_idx = imported.get_section_indices(si);
            assert_eq!(
                imp_idx, orig_idx,
                "{}: section {} index mismatch",
                filename, si
            );
        }

        // Verify bone hierarchy
        if !original.bones.is_empty() {
            assert_eq!(
                imported.bones.len(),
                original.bones.len(),
                "{}: bone count mismatch",
                filename
            );
            for (bi, (ob, ib)) in original.bones.iter().zip(imported.bones.iter()).enumerate() {
                assert_eq!(ob.name, ib.name, "{}: bone {} name mismatch", filename, bi);
                assert_eq!(
                    ob.parent_index, ib.parent_index,
                    "{}: bone {} parent mismatch",
                    filename, bi
                );
            }
        }

        // Write to UGX and read back
        let ugx_bytes = ugx::Writer::write(&imported).unwrap();
        let re_read = ugx::Reader::read(&ugx_bytes).unwrap();

        assert_eq!(
            re_read.sections.len(),
            imported.sections.len(),
            "{}: write roundtrip section count mismatch",
            filename
        );
        for si in 0..imported.sections.len() {
            let imp_verts = imported.unpack_section_vertices(si).unwrap();
            let rr_verts = re_read.unpack_section_vertices(si).unwrap();
            assert_eq!(
                rr_verts.len(),
                imp_verts.len(),
                "{}: write roundtrip section {} vertex count mismatch",
                filename,
                si
            );
            for (vi, (iv, rv)) in imp_verts.iter().zip(rr_verts.iter()).enumerate() {
                for c in 0..3 {
                    assert!(
                        (iv.position[c] - rv.position[c]).abs() < 1e-4,
                        "{}: write roundtrip section {} vertex {} position[{}]: {} vs {}",
                        filename,
                        si,
                        vi,
                        c,
                        iv.position[c],
                        rv.position[c]
                    );
                }
            }
        }

        eprintln!("  PASSED round-trip");
        tested = true;
    }

    if !tested {
        eprintln!("No foxcannon UGX files found in foxcannon01/ - test skipped");
    }
}

#[test]
fn test_material_texture_roundtrip() {
    // GltfExportOptions and export_to_gltf already imported at top level

    // Build a UGX with materials that have texture maps
    let mut geom = make_test_geom();
    geom.materials = vec![
        Material {
            name: "mat_diffuse_normal".into(),
            maps: {
                let mut maps: [Vec<Map>; MapType::NUM_TYPES] = Default::default();
                maps[MapType::Diffuse as usize].push(Map {
                    name: r"\textures\diffuse_01".into(),
                    channel: 0,
                    flags: 0,
                });
                maps[MapType::Normal as usize].push(Map {
                    name: r"\textures\normal_01".into(),
                    channel: 0,
                    flags: 0,
                });
                maps
            },
            spec_power: 50.0,
            opacity: 1.0,
            blend_type: 0,
            ..Default::default()
        },
        Material {
            name: "mat_emissive_ao".into(),
            maps: {
                let mut maps: [Vec<Map>; MapType::NUM_TYPES] = Default::default();
                maps[MapType::Emissive as usize].push(Map {
                    name: r"\textures\emissive_01".into(),
                    channel: 1,
                    flags: 0,
                });
                maps[MapType::AO as usize].push(Map {
                    name: r"\textures\ao_01".into(),
                    channel: 0,
                    flags: 0,
                });
                maps
            },
            spec_power: 10.0,
            opacity: 0.8,
            blend_type: 1,
            ..Default::default()
        },
    ];

    // Export to glTF with materials
    let export_opts = GltfExportOptions {
        embed_buffers: false,
        include_materials: true,
        include_skeleton: false,
    };
    let exported = export_to_gltf(&geom, &export_opts).unwrap();

    // Import back
    let import_opts = GltfImportOptions {
        include_skeleton: false,
        include_materials: true,
    };
    let imported =
        import_from_gltf(&exported.json, exported.buffer.as_deref(), &import_opts).unwrap();

    assert_eq!(imported.materials.len(), 2);

    // First material: diffuse + normal, opaque
    let m0 = &imported.materials[0];
    assert_eq!(m0.name, "mat_diffuse_normal");
    assert_eq!(m0.maps[MapType::Diffuse as usize].len(), 1);
    assert_eq!(
        m0.maps[MapType::Diffuse as usize][0].name,
        r"\textures\diffuse_01"
    );
    assert_eq!(m0.maps[MapType::Diffuse as usize][0].channel, 0);
    assert_eq!(m0.maps[MapType::Normal as usize].len(), 1);
    assert_eq!(
        m0.maps[MapType::Normal as usize][0].name,
        r"\textures\normal_01"
    );
    assert_eq!(m0.blend_type, 0);

    // Second material: emissive + AO, blend
    let m1 = &imported.materials[1];
    assert_eq!(m1.name, "mat_emissive_ao");
    assert_eq!(m1.maps[MapType::Emissive as usize].len(), 1);
    assert_eq!(
        m1.maps[MapType::Emissive as usize][0].name,
        r"\textures\emissive_01"
    );
    assert_eq!(m1.maps[MapType::Emissive as usize][0].channel, 1);
    assert_eq!(m1.maps[MapType::AO as usize].len(), 1);
    assert_eq!(m1.maps[MapType::AO as usize][0].name, r"\textures\ao_01");
    assert_eq!(m1.blend_type, 1);
    assert!((m1.opacity - 0.8).abs() < 0.01);
}

/// Test that round-tripped vertex formats match game expectations.
/// The game expects compact vertex types: HalfFloat4 for position, UByte4N for weights,
/// HalfFloat2 for UVs, and pack_order with skin before texcoords (PNA0ST0).
#[test]
fn test_gltf_import_uses_game_vertex_formats() {
    let original = make_test_geom();

    // Export to glTF with skeleton (to test skinned mesh format)
    let opts = GltfExportOptions {
        embed_buffers: false,
        include_materials: false,
        include_skeleton: true,
    };
    let export = export_to_gltf(&original, &opts).unwrap();

    // Import back
    let import_opts = GltfImportOptions {
        include_skeleton: true,
        include_materials: false,
    };
    let imported = import_from_gltf(&export.json, export.buffer.as_deref(), &import_opts).unwrap();

    assert_eq!(imported.sections.len(), 1);
    let section = &imported.sections[0];
    let packer = &section.base_vert_packer;

    // Verify game-compatible vertex element types
    assert_eq!(
        packer.pos_type,
        VertexElementType::HalfFloat4,
        "Position should use HalfFloat4 (8 bytes) for game compatibility"
    );
    assert_eq!(
        packer.weights_type,
        VertexElementType::UByte4N,
        "Weights should use UByte4N (4 bytes) for game compatibility"
    );
    assert_eq!(
        packer.uv_types[0],
        VertexElementType::HalfFloat2,
        "UVs should use HalfFloat2 (4 bytes) for game compatibility"
    );
    assert_eq!(
        packer.tangent_type,
        VertexElementType::Float3,
        "Tangent should use Float3 (12 bytes) for game compatibility"
    );

    // Verify pack_order has skin before texcoords (game format: PNA0ST0)
    let pack_order = &packer.pack_order;
    let s_pos = pack_order.find('S');
    let t_pos = pack_order.find('T');
    assert!(
        s_pos.is_some() && t_pos.is_some(),
        "Pack order should contain both S and T: {}",
        pack_order
    );
    assert!(
        s_pos.unwrap() < t_pos.unwrap(),
        "Skin (S) should come before texcoords (T) in pack_order: {}",
        pack_order
    );

    // Verify vertex size is compact (should be ~44 bytes for skinned mesh with tangent)
    // HalfFloat4(8) + Float3(12) + Float3(12) + UByte4(4) + UByte4N(4) + HalfFloat2(4) = 44
    let expected_size = 8 + 12 + 12 + 4 + 4 + 4; // 44 bytes
    assert_eq!(
        packer.vertex_size(),
        expected_size,
        "Vertex size should be {} bytes for game-compatible format, got {}",
        expected_size,
        packer.vertex_size()
    );
}

/// Test that rigid (non-skinned) meshes also use game-compatible formats.
#[test]
fn test_gltf_import_rigid_mesh_uses_game_formats() {
    // Create a rigid mesh (no skeleton)
    let mut uv_types = [VertexElementType::Ignore; MAX_UV];
    uv_types[0] = VertexElementType::Float2;

    let packer = UnivertPacker {
        pack_order: "PNA0T0".to_string(),
        decl_order: "PNA0T0".to_string(),
        pos_type: VertexElementType::Float3,
        basis_type: VertexElementType::Float4,
        basis_scale_type: VertexElementType::Float2,
        tangent_type: VertexElementType::Float4,
        normal_type: VertexElementType::Float3,
        uv_types,
        indices_type: VertexElementType::Ignore,
        weights_type: VertexElementType::Ignore,
        diffuse_type: VertexElementType::Ignore,
        index_type: VertexElementType::Ignore,
    };

    let vertices = vec![
        UnpackedVertex {
            position: [0.0, 0.0, 0.0],
            normal: [0.0, 1.0, 0.0],
            tangent: [1.0, 0.0, 0.0, 1.0],
            texcoords: {
                let mut tc = [[0.0; 2]; MAX_UV];
                tc[0] = [0.0, 0.0];
                tc
            },
            num_texcoords: 1,
            ..Default::default()
        },
        UnpackedVertex {
            position: [1.0, 0.0, 0.0],
            normal: [0.0, 1.0, 0.0],
            tangent: [1.0, 0.0, 0.0, 1.0],
            texcoords: {
                let mut tc = [[0.0; 2]; MAX_UV];
                tc[0] = [1.0, 0.0];
                tc
            },
            num_texcoords: 1,
            ..Default::default()
        },
        UnpackedVertex {
            position: [0.0, 1.0, 0.0],
            normal: [0.0, 1.0, 0.0],
            tangent: [1.0, 0.0, 0.0, -1.0],
            texcoords: {
                let mut tc = [[0.0; 2]; MAX_UV];
                tc[0] = [0.0, 1.0];
                tc
            },
            num_texcoords: 1,
            ..Default::default()
        },
    ];

    let mut vertex_buffer = Vec::new();
    for v in &vertices {
        packer.pack_vertex(&mut vertex_buffer, v);
    }

    let vert_size = packer.vertex_size() as i32;
    let vb_bytes = vertex_buffer.len() as i32;

    let geom = UgxGeom {
        bounding_sphere: Sphere {
            center: [0.5, 0.5, 0.0],
            radius: 1.0,
        },
        bounds: AABB {
            min: [0.0, 0.0, 0.0],
            max: [1.0, 1.0, 0.0],
        },
        materials: Vec::new(),
        bones: Vec::new(),
        granny_bones: Vec::new(),
        granny_meshes: Vec::new(),
        bone_bounds: Vec::new(),
        sections: vec![Section {
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
            base_vert_packer: packer,
            bone_remap: Vec::new(),
            rigid_only: true,
            global_bones: false,
        }],
        vertex_buffer,
        index_buffer: vec![0, 1, 2],
        rigid_only: true,
        rigid_bone_index: -1,
        all_sections_rigid: true,
        all_sections_skinned: false,
        global_bones: false,
    };

    // Export to glTF
    let opts = GltfExportOptions {
        embed_buffers: false,
        include_materials: false,
        include_skeleton: false,
    };
    let export = export_to_gltf(&geom, &opts).unwrap();

    // Import back
    let import_opts = GltfImportOptions {
        include_skeleton: false,
        include_materials: false,
    };
    let imported = import_from_gltf(&export.json, export.buffer.as_deref(), &import_opts).unwrap();

    assert_eq!(imported.sections.len(), 1);
    let section = &imported.sections[0];
    let imported_packer = &section.base_vert_packer;

    // Verify game-compatible vertex element types for rigid mesh
    assert_eq!(
        imported_packer.pos_type,
        VertexElementType::HalfFloat4,
        "Position should use HalfFloat4 for game compatibility"
    );
    assert_eq!(
        imported_packer.uv_types[0],
        VertexElementType::HalfFloat2,
        "UVs should use HalfFloat2 for game compatibility"
    );

    // Rigid mesh should not have skin data
    assert!(
        !imported_packer.pack_order.contains('S'),
        "Rigid mesh pack_order should not contain S: {}",
        imported_packer.pack_order
    );

    // Verify vertex size is compact for rigid mesh
    // HalfFloat4(8) + Float3(12) + Float3(12) + HalfFloat2(4) = 36 bytes
    let expected_size = 8 + 12 + 12 + 4; // 36 bytes
    assert_eq!(
        imported_packer.vertex_size(),
        expected_size,
        "Rigid vertex size should be {} bytes, got {}",
        expected_size,
        imported_packer.vertex_size()
    );
}

#[test]
fn test_material_names_and_textures_exported() {
    use ugx::*;

    // Build a minimal geometry with materials
    let packer = UnivertPacker {
        pack_order: "P".to_string(),
        decl_order: "P".to_string(),
        pos_type: VertexElementType::Float3,
        basis_type: VertexElementType::Ignore,
        basis_scale_type: VertexElementType::Ignore,
        tangent_type: VertexElementType::Ignore,
        normal_type: VertexElementType::Ignore,
        uv_types: [VertexElementType::Ignore; MAX_UV],
        indices_type: VertexElementType::Ignore,
        weights_type: VertexElementType::Ignore,
        diffuse_type: VertexElementType::Ignore,
        index_type: VertexElementType::Ignore,
    };

    let mut vb = Vec::new();
    for pos in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
        let v = UnpackedVertex {
            position: pos,
            ..Default::default()
        };
        packer.pack_vertex(&mut vb, &v);
    }

    let geom = UgxGeom {
        bounding_sphere: Sphere {
            center: [0.0; 3],
            radius: 1.0,
        },
        bounds: AABB {
            min: [0.0; 3],
            max: [1.0; 3],
        },
        materials: vec![
            Material {
                name: "grass_mat".to_string(),
                spec_power: 40.0,
                flags: 0,
                blend_type: 0,
                opacity: 1.0,
                maps: {
                    let mut maps: [Vec<Map>; MapType::NUM_TYPES] = Default::default();
                    maps[MapType::Diffuse as usize] = vec![Map {
                        name: "art/grass_diff.ddx".to_string(),
                        channel: 0,
                        flags: 7,
                    }];
                    maps[MapType::Normal as usize] = vec![Map {
                        name: "art/grass_norm.ddx".to_string(),
                        channel: 0,
                        flags: 7,
                    }];
                    maps
                },
                uvw_velocity: [[0.0; 3]; MapType::NUM_TYPES],
            },
            Material {
                name: "glass_mat".to_string(),
                spec_power: 80.0,
                flags: 0,
                blend_type: 1,
                opacity: 0.5,
                maps: {
                    let mut maps: [Vec<Map>; MapType::NUM_TYPES] = Default::default();
                    maps[MapType::Diffuse as usize] = vec![Map {
                        name: "art/glass_diff.ddx".to_string(),
                        channel: 0,
                        flags: 7,
                    }];
                    maps[MapType::Emissive as usize] = vec![Map {
                        name: "art/glass_emit.ddx".to_string(),
                        channel: 1,
                        flags: 3,
                    }];
                    maps
                },
                uvw_velocity: [[0.0; 3]; MapType::NUM_TYPES],
            },
        ],
        bones: Vec::new(),
        granny_bones: Vec::new(),
        granny_meshes: Vec::new(),
        bone_bounds: Vec::new(),
        sections: vec![Section {
            material_index: 0,
            accessory_index: -1,
            max_bones: 0,
            rigid_bone_index: -1,
            ib_offset: 0,
            num_tris: 1,
            vb_offset: 0,
            vb_bytes: vb.len() as i32,
            vert_size: packer.vertex_size() as i32,
            num_verts: 3,
            base_vert_packer: packer,
            bone_remap: Vec::new(),
            rigid_only: true,
            global_bones: false,
        }],
        vertex_buffer: vb,
        index_buffer: vec![0, 1, 2],
        rigid_only: true,
        rigid_bone_index: -1,
        all_sections_rigid: true,
        all_sections_skinned: false,
        global_bones: false,
    };

    let options = GltfExportOptions {
        embed_buffers: true,
        include_materials: true,
        include_skeleton: false,
    };
    let export = export_to_gltf(&geom, &options).unwrap();
    let root: gltf_json::Root = serde_json::from_str(&export.json).unwrap();

    // Verify material count and names
    assert_eq!(root.materials.len(), 2);
    assert_eq!(root.materials[0].name, Some("grass_mat".to_string()));
    assert_eq!(root.materials[1].name, Some("glass_mat".to_string()));

    // Verify first material is opaque
    assert_eq!(
        root.materials[0].alpha_mode,
        Valid(gltf_json::material::AlphaMode::Opaque)
    );

    // Verify second material uses blend (blend_type=1, opacity=0.5)
    assert_eq!(
        root.materials[1].alpha_mode,
        Valid(gltf_json::material::AlphaMode::Blend)
    );

    // Verify textures were created (4 unique textures)
    assert_eq!(root.textures.len(), 4);
    assert_eq!(root.images.len(), 4);

    // Verify image URIs
    let image_uris: Vec<_> = root
        .images
        .iter()
        .map(|img| img.uri.as_deref().unwrap())
        .collect();
    assert!(image_uris.contains(&"art/grass_diff.ddx"));
    assert!(image_uris.contains(&"art/grass_norm.ddx"));
    assert!(image_uris.contains(&"art/glass_diff.ddx"));
    assert!(image_uris.contains(&"art/glass_emit.ddx"));

    // Verify first material has diffuse and normal textures
    assert!(
        root.materials[0]
            .pbr_metallic_roughness
            .base_color_texture
            .is_some()
    );
    assert!(root.materials[0].normal_texture.is_some());
    assert!(root.materials[0].emissive_texture.is_none());

    // Verify second material has diffuse and emissive textures
    assert!(
        root.materials[1]
            .pbr_metallic_roughness
            .base_color_texture
            .is_some()
    );
    assert!(root.materials[1].normal_texture.is_none());
    assert!(root.materials[1].emissive_texture.is_some());

    // Verify emissive factor is [1,1,1] when emissive texture present
    assert_eq!(root.materials[1].emissive_factor.0, [1.0, 1.0, 1.0]);
    // And [0,0,0] when no emissive texture
    assert_eq!(root.materials[0].emissive_factor.0, [0.0, 0.0, 0.0]);

    // Verify emissive texture uses UV channel 1
    let emit_info = root.materials[1].emissive_texture.as_ref().unwrap();
    assert_eq!(emit_info.tex_coord, 1);
}
