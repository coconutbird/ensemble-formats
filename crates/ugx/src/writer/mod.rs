//! UGX file writer.
//!
//! Serializes a `UgxGeom` into UGX binary format (ECF container).
//! Writes chunks 0x700 (cached data), 0x701 (index buffer), 0x702 (vertex buffer),
//! 0x703 (granny bones), and 0x704 (materials).

mod cached_data;
mod granny;
mod material;
pub(crate) mod string_table;

use alloc::vec::Vec;

use crate::chunk_ids::*;
use crate::error::Result;
use crate::types::UgxGeom;

/// UGX file writer.
pub struct Writer;

impl Writer {
    /// Write a UGX geometry to a byte vector.
    pub fn write(geom: &UgxGeom) -> Result<Vec<u8>> {
        geom.to_bytes()
    }
}

impl UgxGeom {
    /// Serialize this geometry to UGX binary format (ECF container).
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        write_ugx(self)
    }
}

/// Write a UGX geometry to bytes (ECF container).
fn write_ugx(geom: &UgxGeom) -> Result<Vec<u8>> {
    let cached_data = cached_data::build_cached_data(geom)?;
    let ib_data = build_index_buffer(geom);

    // ECF file ID 0xAAC93746 is required for UGX files - the game validates this in BGrannyModel::load
    let mut ecf = ecf::Writer::new(0xAAC93746);

    ecf.add_chunk(ECF_CACHED_DATA_CHUNK_ID, cached_data);
    ecf.add_chunk(ECF_IB_CHUNK_ID, ib_data);
    ecf.add_chunk(ECF_VB_CHUNK_ID, geom.vertex_buffer.clone());

    // Write granny bones chunk if we have granny bone data
    if !geom.granny_bones.is_empty() {
        let granny_data = granny::build_granny_data(geom)?;
        ecf.add_chunk(ECF_GRANNY_CHUNK_ID, granny_data);
    }

    // Write materials chunk if we have materials
    if !geom.materials.is_empty() {
        let mat_data = material::build_material_data(geom)?;
        ecf.add_chunk(ECF_MATERIAL_CHUNK_ID, mat_data);
    }

    Ok(ecf.finalize()?)
}

/// Build the index buffer chunk (0x701).
fn build_index_buffer(geom: &UgxGeom) -> Vec<u8> {
    let mut buf = Vec::with_capacity(geom.index_buffer.len() * 2);
    for &idx in &geom.index_buffer {
        buf.extend_from_slice(&idx.to_le_bytes());
    }
    buf
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;
    use crate::vertex::element::VertexElementType;
    use crate::vertex::packer::{MAX_UV, UnivertPacker, UnpackedVertex};
    use alloc::string::ToString;
    use alloc::vec;

    /// Create a minimal test UgxGeom with one section and two bones.
    fn make_test_geom() -> UgxGeom {
        let packer = UnivertPacker {
            pack_order: "PNT0".to_string(),
            decl_order: "PNT0".to_string(),
            pos_type: VertexElementType::Float3,
            basis_type: VertexElementType::Float4,
            basis_scale_type: VertexElementType::Float2,
            tangent_type: VertexElementType::Ignore,
            normal_type: VertexElementType::Float3,
            uv_types: {
                let mut uv = [VertexElementType::Ignore; MAX_UV];
                uv[0] = VertexElementType::Float2;
                uv
            },
            indices_type: VertexElementType::UByte4,
            weights_type: VertexElementType::Float4,
            diffuse_type: VertexElementType::Ignore,
            index_type: VertexElementType::Ignore,
        };

        // Build vertex buffer
        let vertices = vec![
            UnpackedVertex {
                position: [0.0, 0.0, 0.0],
                normal: [0.0, 1.0, 0.0],
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

        let section = Section {
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
        };

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
            vertex_buffer,
            index_buffer: vec![0, 1, 2],
            rigid_only: true,
            rigid_bone_index: 0,
            all_sections_rigid: true,
            all_sections_skinned: false,
            global_bones: false,
        }
    }

    #[test]
    fn test_write_read_roundtrip() {
        let original = make_test_geom();
        let bytes = write_ugx(&original).unwrap();
        let read_back = crate::Reader::read(&bytes).unwrap();

        assert_eq!(read_back.rigid_bone_index, original.rigid_bone_index);
        assert_eq!(read_back.rigid_only, original.rigid_only);
        assert_eq!(read_back.all_sections_rigid, original.all_sections_rigid);
        assert_eq!(
            read_back.all_sections_skinned,
            original.all_sections_skinned
        );
        assert_eq!(read_back.global_bones, original.global_bones);
        assert_eq!(
            read_back.bounding_sphere.center,
            original.bounding_sphere.center
        );
        assert_eq!(
            read_back.bounding_sphere.radius,
            original.bounding_sphere.radius
        );
        assert_eq!(read_back.bounds.min, original.bounds.min);
        assert_eq!(read_back.bounds.max, original.bounds.max);

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
        assert_eq!(
            s_read.base_vert_packer.pack_order,
            s_orig.base_vert_packer.pack_order
        );

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
            assert_eq!(bb_read.min, bb_orig.min);
            assert_eq!(bb_read.max, bb_orig.max);
        }

        let orig_verts = original.unpack_section_vertices(0).unwrap();
        let read_verts = read_back.unpack_section_vertices(0).unwrap();
        assert_eq!(read_verts.len(), orig_verts.len());
        for (v_orig, v_read) in orig_verts.iter().zip(read_verts.iter()) {
            assert_eq!(v_read.position, v_orig.position);
            assert_eq!(v_read.normal, v_orig.normal);
            assert_eq!(v_read.texcoords[0], v_orig.texcoords[0]);
        }

        let orig_indices = original.get_section_indices(0);
        let read_indices = read_back.get_section_indices(0);
        assert_eq!(read_indices, orig_indices);
    }

    #[test]
    fn test_write_read_materials_roundtrip() {
        let mut geom = make_test_geom();
        geom.materials = vec![
            Material {
                name: "terrain_grass".to_string(),
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
                uvw_velocity: [[0.0; 3]; MapType::NUM_TYPES],
            },
            Material {
                name: "metal_plate".to_string(),
                spec_power: 50.0,
                flags: 0,
                blend_type: 0,
                opacity: 1.0,
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
                uvw_velocity: [[0.0; 3]; MapType::NUM_TYPES],
            },
        ];

        let bytes = write_ugx(&geom).unwrap();
        let read_back = crate::Reader::read(&bytes).unwrap();

        assert_eq!(read_back.materials.len(), 2);
        let m0 = &read_back.materials[0];
        assert_eq!(m0.name, "terrain_grass");
        assert!((m0.spec_power - 25.0).abs() < 0.1);
        assert_eq!(m0.flags, 3);
        assert_eq!(m0.blend_type, 1);
        assert!((m0.opacity - 0.8).abs() < 0.01);
        assert_eq!(m0.maps[MapType::Diffuse as usize].len(), 1);
        assert_eq!(
            m0.maps[MapType::Diffuse as usize][0].name,
            "art/textures/grass_diff.ddx"
        );
        assert_eq!(m0.maps[MapType::Normal as usize].len(), 1);

        let m1 = &read_back.materials[1];
        assert_eq!(m1.name, "metal_plate");
        assert!((m1.spec_power - 50.0).abs() < 0.1);
        assert_eq!(m1.maps[MapType::Gloss as usize].len(), 1);
        assert_eq!(
            m1.maps[MapType::Gloss as usize][0].name,
            "art/textures/metal_gloss.ddx"
        );
        assert_eq!(m1.maps[MapType::Gloss as usize][0].channel, 1);
        assert!(m0.maps[MapType::Gloss as usize].is_empty());
        assert!(m1.maps[MapType::Normal as usize].is_empty());
    }

    #[test]
    fn test_write_read_granny_bones_roundtrip() {
        let mut geom = make_test_geom();
        geom.granny_bones = vec![
            GrannyBone {
                name: "root".to_string(),
                parent_index: -1,
                inverse_world_matrix: Matrix4x4 {
                    rows: [
                        [1.0, 0.0, 0.0, 0.0],
                        [0.0, 1.0, 0.0, 0.0],
                        [0.0, 0.0, 1.0, 0.0],
                        [0.0, 0.0, 0.0, 1.0],
                    ],
                },
            },
            GrannyBone {
                name: "spine".to_string(),
                parent_index: 0,
                inverse_world_matrix: Matrix4x4 {
                    rows: [
                        [1.0, 0.0, 0.0, 0.0],
                        [0.0, 0.0, 1.0, 0.0],
                        [0.0, -1.0, 0.0, 0.0],
                        [0.5, -2.0, 1.5, 1.0],
                    ],
                },
            },
        ];

        let bytes = write_ugx(&geom).unwrap();
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
}
