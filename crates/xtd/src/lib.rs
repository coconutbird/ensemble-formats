//! XTD (Terrain Displacement) format handling for Halo Wars.
//!
//! XTD files store terrain height/displacement data in an ECF container.
//!
//! ## File Structure
//!
//! XTD files contain the following chunks:
//! - `0x1111` - `XTDHeader`: Main header with terrain dimensions
//! - `0x2222` - `TerrainChunk`: Per-chunk visual headers (196 chunks typical)
//! - `0x8888` - `AtlasChunk`: Terrain atlas texture data
//! - `0xAAAA` - `TessChunk`: Tessellation data
//! - `0xBBBB` - `LightingChunk`: Lighting data
//! - `0xCCCC` - `AOChunk`: Ambient occlusion data
//! - `0xDDDD` - `AlphaChunk`: Alpha/transparency data

#![no_std]
extern crate alloc;

mod error;
pub use error::{Error, Result};

mod types;
pub use types::*;

mod reader;
pub use reader::{ReadOptions, Reader};

mod writer;
pub use writer::Writer;

mod decode;
pub use decode::{
    AlphaData, AmbientOcclusionData, AtlasHeader, LightingData, RawTerrainData, TerrainVertices,
    TessellatedMesh, unpack_normal, unpack_position,
};

// ============================================================================
// XTD Constants
// ============================================================================

/// XTD file version.
pub const XTD_VERSION: i32 = 0x000C;

/// ECF file identifier used by retail XTD files.
pub const XTD_FILE_ID: u32 = 0x0007_7826;

/// XTD header chunk ID.
pub const CHUNK_XTD_HEADER: u64 = 0x1111;

/// Terrain visual chunk header ID.
pub const CHUNK_TERRAIN: u64 = 0x2222;

/// Terrain atlas link chunk ID (not used in DE).
pub const CHUNK_ATLAS_LINK: u64 = 0x4444;

/// Atlas header chunk ID (not used in DE).
pub const CHUNK_ATLAS_HEADER: u64 = 0x6666;

/// Atlas chunk ID.
pub const CHUNK_ATLAS: u64 = 0x8888;

/// Tessellation chunk ID.
pub const CHUNK_TESS: u64 = 0xAAAA;

/// Lighting chunk ID.
pub const CHUNK_LIGHTING: u64 = 0xBBBB;

/// Ambient occlusion chunk ID.
pub const CHUNK_AO: u64 = 0xCCCC;

/// Alpha chunk ID.
pub const CHUNK_ALPHA: u64 = 0xDDDD;

#[cfg(test)]
mod tests {
    extern crate alloc;
    extern crate std;
    use super::*;
    use alloc::vec::Vec;
    use num_traits::ToPrimitive;
    use std::{print, println};

    // Test files are in the extracted test_extract directory (relative to workspace root)
    const TEST_XTD_PATH: &str =
        "../../test_extract/scenario/skirmish/design/blood_gulch/blood_gulch.xtd";

    fn assert_float_bits_eq(actual: f32, expected: f32) {
        assert_eq!(actual.to_bits(), expected.to_bits());
    }

    fn assert_float_array_bits_eq<const N: usize>(actual: [f32; N], expected: [f32; N]) {
        assert_eq!(actual.map(f32::to_bits), expected.map(f32::to_bits));
    }

    #[test]
    #[ignore = "requires extracted XTD file"]
    fn test_read_xtd() {
        let data = std::fs::read(TEST_XTD_PATH).expect("Failed to read XTD file");
        let file = Reader::read(&data).expect("Failed to parse XTD");

        println!("XTD Header:");
        println!("  Version: 0x{:04X}", file.header.version);
        println!("  NumXVerts: {}", file.header.num_x_verts);
        println!("  NumXChunks: {}", file.header.num_x_chunks);
        println!("  TileScale: {}", file.header.tile_scale);
        println!("  WorldMin: {:?}", file.header.world_min);
        println!("  WorldMax: {:?}", file.header.world_max);
        println!("\nVisual chunks: {}", file.visual_chunks.len());
        println!("Atlas data: {} bytes", file.atlas_data.len());
        println!("Tess data: {} bytes", file.tess_data.len());
        println!("Lighting data: {} bytes", file.lighting_data.len());
        println!("AO data: {} bytes", file.ao_data.len());
        println!("Alpha data: {} bytes", file.alpha_data.len());

        assert_eq!(file.header.version, XTD_VERSION);
        assert!(!file.visual_chunks.is_empty());
    }

    #[test]
    #[ignore = "requires extracted XTD file"]
    fn test_xtd_roundtrip() {
        let original = std::fs::read(TEST_XTD_PATH).expect("Failed to read XTD file");
        let file = Reader::read(&original).expect("Failed to parse XTD");
        let rewritten = Writer::write(&file).expect("Failed to write XTD");

        println!("Original size: {} bytes", original.len());
        println!("Rewritten size: {} bytes", rewritten.len());

        // Print first 64 bytes of both for comparison
        println!("\nOriginal header (first 64 bytes):");
        for (i, &byte) in original.iter().enumerate().take(64) {
            if i % 16 == 0 {
                print!("  {i:04X}: ");
            }
            print!("{byte:02X} ");
            if i % 16 == 15 {
                println!();
            }
        }
        println!("\nRewritten header (first 64 bytes):");
        for (i, &byte) in rewritten.iter().enumerate().take(64) {
            if i % 16 == 0 {
                print!("  {i:04X}: ");
            }
            print!("{byte:02X} ");
            if i % 16 == 15 {
                println!();
            }
        }

        // Compare byte-for-byte, skipping adler32 (bytes 8-11)
        let mut diff_count = 0;
        let min_len = original.len().min(rewritten.len());
        for i in 0..min_len {
            // Skip adler32 field (offset 8-11)
            if (8..=11).contains(&i) {
                continue;
            }

            if original[i] != rewritten[i] {
                println!(
                    "Diff at offset 0x{:X}: original=0x{:02X}, rewritten=0x{:02X}",
                    i, original[i], rewritten[i]
                );
                diff_count += 1;
                if diff_count >= 20 {
                    println!("... (more differences)");
                    break;
                }
            }
        }

        if diff_count == 0 {
            println!("\nXTD roundtrip: DATA IDENTICAL (only adler32 differs - recalculated)");
        } else {
            if original.len() != rewritten.len() {
                println!("Size mismatch: {} vs {}", original.len(), rewritten.len());
            }
            panic!("XTD roundtrip failed: {diff_count} non-checksum differences!");
        }
    }

    #[test]
    #[ignore = "requires extracted XTD file"]
    fn test_decode_vertices() {
        let data = std::fs::read(TEST_XTD_PATH).expect("Failed to read XTD file");
        let file = Reader::read(&data).expect("Failed to parse XTD");

        let vertices = file.decode_vertices().expect("Failed to decode vertices");

        println!("Atlas Header:");
        println!("  Mid: {:?}", vertices.header.mid);
        println!("  Range: {:?}", vertices.header.range);
        println!(
            "\nTerrain grid: {}x{}",
            vertices.num_verts_per_axis, vertices.num_verts_per_axis
        );
        println!("Total vertices: {}", vertices.positions.len());
        println!("Total normals: {}", vertices.normals.len());

        // Print first few vertices
        println!("\nFirst 5 vertices:");
        for i in 0..5.min(vertices.positions.len()) {
            println!(
                "  [{:3}] pos={:?} norm={:?}",
                i, vertices.positions[i], vertices.normals[i]
            );
        }

        // Generate indices
        let indices = vertices
            .generate_indices()
            .expect("Failed to generate indices");
        println!(
            "\nGenerated {} indices ({} triangles)",
            indices.len(),
            indices.len() / 3
        );

        // Sanity checks
        assert_eq!(vertices.positions.len(), vertices.normals.len());
        assert_eq!(
            vertices.positions.len(),
            vertices.num_verts_per_axis * vertices.num_verts_per_axis
        );

        // A non-diagonal source patch independently verifies the atlas layout,
        // the `.zyx` position swizzle, and the complete source-to-world X/Z
        // conversion. Source patch (1, 0) becomes viewer patch (0, 1).
        let tessellation = file
            .decode_tessellation()
            .expect("Failed to decode tessellation")
            .expect("Missing tessellation chunk");
        let bbox = tessellation
            .get_patch_bbox(1, 0)
            .expect("Missing source patch bounding box");
        let mut decoded_min = [f32::INFINITY; 3];
        let mut decoded_max = [f32::NEG_INFINITY; 3];
        for z in 16..=32 {
            for x in 0..=16 {
                let position = vertices.positions[z * vertices.num_verts_per_axis + x];
                for axis in 0..3 {
                    decoded_min[axis] = decoded_min[axis].min(position[axis]);
                    decoded_max[axis] = decoded_max[axis].max(position[axis]);
                }
            }
        }
        let source_axis_for_world = [2, 1, 0];
        for (world_axis, &source_axis) in source_axis_for_world.iter().enumerate() {
            assert!((decoded_min[world_axis] - bbox.min[source_axis]).abs() < 0.5);
            assert!((decoded_max[world_axis] - bbox.max[source_axis]).abs() < 0.5);
        }

        // Check normals are normalized (approximately)
        for (i, norm) in vertices.normals.iter().take(100).enumerate() {
            let len = (norm[0] * norm[0] + norm[1] * norm[1] + norm[2] * norm[2]).sqrt();
            assert!(
                (len - 1.0).abs() < 0.1,
                "Normal {i} not normalized: {norm:?} (len={len})"
            );
        }
    }

    #[test]
    #[ignore = "requires extracted XTD file"]
    fn test_decode_tessellation() {
        let data = std::fs::read(TEST_XTD_PATH).expect("Failed to read XTD file");
        let file = Reader::read(&data).expect("Failed to parse XTD");

        println!("Raw tess_data size: {} bytes", file.tess_data.len());

        let tess = file
            .decode_tessellation()
            .expect("Failed to decode tessellation")
            .expect("Missing tessellation chunk");

        println!("\nTessellation Data:");
        println!("  NumXPatches: {}", tess.num_x_patches);
        println!("  NumZPatches: {}", tess.num_z_patches);
        println!("  Total patches: {}", tess.num_patches());
        println!("  MaxTessLevel: {}", tess.max_tess_level);
        println!(
            "  Patch tess levels: {} entries",
            tess.patch_tess_levels.len()
        );
        println!(
            "  Patch bounding boxes: {} entries",
            tess.patch_bounding_boxes.len()
        );

        // Print tessellation level distribution
        let mut level_counts = std::collections::HashMap::new();
        for &level in &tess.patch_tess_levels {
            *level_counts.entry(level).or_insert(0) += 1;
        }
        println!("\n  Tess level distribution:");
        let mut sorted_levels: Vec<_> = level_counts.into_iter().collect();
        sorted_levels.sort_by_key(|(level, _)| *level);
        for (level, count) in sorted_levels {
            println!("    Level {level}: {count} patches");
        }

        // Print a few bounding boxes
        println!("\n  First 5 patch bounding boxes:");
        for i in 0..5.min(tess.patch_bounding_boxes.len()) {
            let bbox = &tess.patch_bounding_boxes[i];
            println!("    [{:3}] min={:?}, max={:?}", i, bbox.min, bbox.max);
        }

        // Sanity checks
        assert_eq!(tess.patch_tess_levels.len(), tess.num_patches());
        assert_eq!(tess.patch_bounding_boxes.len(), tess.num_patches());
        assert!(tess.max_tess_level > 0, "Max tess level should be > 0");
    }

    #[test]
    #[ignore = "requires extracted XTD file"]
    fn test_cpu_tessellation() {
        let data = std::fs::read(TEST_XTD_PATH).expect("Failed to read XTD file");
        let file = Reader::read(&data).expect("Failed to parse XTD");

        let vertices = file.decode_vertices().expect("Failed to decode vertices");
        let tess = file
            .decode_tessellation()
            .expect("Failed to decode tessellation")
            .expect("Missing tessellation chunk");

        println!("Original mesh:");
        println!("  Vertices: {}", vertices.positions.len());
        let original_indices = vertices
            .generate_indices()
            .expect("Failed to generate original indices");
        println!("  Triangles: {}", original_indices.len() / 3);

        // Generate tessellated mesh
        let tessellated = vertices
            .tessellate(&tess)
            .expect("Failed to tessellate terrain");

        println!("\nTessellated mesh:");
        println!("  Vertices: {}", tessellated.positions.len());
        println!("  Triangles: {}", tessellated.indices.len() / 3);
        println!(
            "  Vertex increase: {:.1}x",
            ratio(tessellated.positions.len(), vertices.positions.len())
        );
        println!(
            "  Triangle increase: {:.1}x",
            ratio(tessellated.indices.len(), original_indices.len())
        );

        // Verify tessellated mesh is valid
        assert!(
            tessellated.positions.len() >= vertices.positions.len(),
            "Tessellated mesh should have at least as many vertices"
        );
        assert!(
            tessellated.indices.len() >= original_indices.len(),
            "Tessellated mesh should have at least as many indices"
        );

        // Check all indices are valid
        for &idx in &tessellated.indices {
            assert!(
                usize::try_from(idx).expect("index must fit usize") < tessellated.positions.len(),
                "Invalid index {} (max {})",
                idx,
                tessellated.positions.len()
            );
        }
    }

    fn ratio(numerator: usize, denominator: usize) -> f32 {
        numerator.to_f32().expect("numerator must fit f32")
            / denominator.to_f32().expect("denominator must fit f32")
    }

    fn test_chunk_meta(id: u64) -> ChunkMeta {
        ChunkMeta {
            id,
            alignment_log2: 4,
            flags: 0,
            resource_flags: 0,
        }
    }

    /// Build a minimal but complete `XtdFile` for round-trip testing.
    fn make_test_xtd() -> XtdFile {
        let header = XtdHeader {
            version: XTD_VERSION,
            num_x_verts: 128,
            num_x_chunks: 2,
            tile_scale: 2.0,
            world_min: [-128.0, -10.0, -128.0],
            world_max: [128.0, 50.0, 128.0],
        };

        let chunks = alloc::vec![
            XtdVisualChunk {
                grid_x: 0,
                grid_z: 0,
                max_v_stride: 17,
                min: [-128.0, -10.0, -128.0],
                max: [-64.0, 50.0, -64.0],
                can_cast_shadows: true,
            },
            XtdVisualChunk {
                grid_x: 1,
                grid_z: 0,
                max_v_stride: 17,
                min: [-64.0, -5.0, -128.0],
                max: [0.0, 30.0, -64.0],
                can_cast_shadows: false,
            },
            XtdVisualChunk {
                grid_x: 0,
                grid_z: 1,
                max_v_stride: 65,
                min: [-128.0, -10.0, -64.0],
                max: [-64.0, 50.0, 0.0],
                can_cast_shadows: true,
            },
            XtdVisualChunk {
                grid_x: 1,
                grid_z: 1,
                max_v_stride: 65,
                min: [-64.0, -5.0, -64.0],
                max: [0.0, 30.0, 0.0],
                can_cast_shadows: false,
            },
        ];

        // 32-byte header plus two packed u32 values for every 128x128 vertex.
        let atlas_data = alloc::vec![0xAA; 32 + 128 * 128 * 8];
        // 8x8 patches: two counts, one level and one bbox per patch.
        let mut tess_data = Vec::new();
        tess_data.extend_from_slice(&8i32.to_be_bytes());
        tess_data.extend_from_slice(&8i32.to_be_bytes());
        tess_data.extend_from_slice(&[3; 64]);
        for i in 0..64u8 {
            for _ in 0..8 {
                tess_data.extend_from_slice(&f32::from(i).to_be_bytes());
            }
        }
        let mut lighting_data = Vec::new();
        lighting_data.extend_from_slice(&8192i32.to_be_bytes());
        lighting_data.extend_from_slice(&alloc::vec![0xBB; 8192]);
        let ao_data = alloc::vec![0xCC; 8192];
        let alpha_data = alloc::vec![0xDD; 8192];

        let chunk_order = alloc::vec![
            test_chunk_meta(CHUNK_XTD_HEADER),
            test_chunk_meta(CHUNK_TERRAIN),
            test_chunk_meta(CHUNK_TERRAIN),
            test_chunk_meta(CHUNK_TERRAIN),
            test_chunk_meta(CHUNK_TERRAIN),
            test_chunk_meta(CHUNK_ATLAS),
            test_chunk_meta(CHUNK_TESS),
            test_chunk_meta(CHUNK_LIGHTING),
            test_chunk_meta(CHUNK_AO),
            test_chunk_meta(CHUNK_ALPHA),
        ];

        XtdFile {
            ecf_file_id: 0x0007_7826,
            ecf_flags: 0,
            chunk_order,
            header,
            visual_chunks: chunks,
            atlas_data,
            tess_data,
            lighting_data,
            ao_data,
            alpha_data,
        }
    }

    #[test]
    fn roundtrip_header_fields() {
        let original = make_test_xtd();
        let bytes = Writer::write(&original).expect("write failed");
        let read = Reader::read(&bytes).expect("read failed");

        assert_eq!(read.header.version, original.header.version);
        assert_eq!(read.header.num_x_verts, original.header.num_x_verts);
        assert_eq!(read.header.num_x_chunks, original.header.num_x_chunks);
        assert_float_bits_eq(read.header.tile_scale, original.header.tile_scale);
        assert_float_array_bits_eq(read.header.world_min, original.header.world_min);
        assert_float_array_bits_eq(read.header.world_max, original.header.world_max);
    }

    #[test]
    fn roundtrip_visual_chunks() {
        let original = make_test_xtd();
        let bytes = Writer::write(&original).expect("write failed");
        let read = Reader::read(&bytes).expect("read failed");

        assert_eq!(read.visual_chunks.len(), original.visual_chunks.len());
        for (r, o) in read.visual_chunks.iter().zip(&original.visual_chunks) {
            assert_eq!(r.grid_x, o.grid_x);
            assert_eq!(r.grid_z, o.grid_z);
            assert_eq!(r.max_v_stride, o.max_v_stride);
            assert_float_array_bits_eq(r.min, o.min);
            assert_float_array_bits_eq(r.max, o.max);
            assert_eq!(r.can_cast_shadows, o.can_cast_shadows);
        }
    }

    #[test]
    fn roundtrip_raw_data_chunks() {
        let original = make_test_xtd();
        let bytes = Writer::write(&original).expect("write failed");
        let read = Reader::read(&bytes).expect("read failed");

        assert_eq!(read.atlas_data, original.atlas_data, "atlas mismatch");
        assert_eq!(read.tess_data, original.tess_data, "tess mismatch");
        assert_eq!(
            read.lighting_data, original.lighting_data,
            "lighting mismatch"
        );
        assert_eq!(read.ao_data, original.ao_data, "ao mismatch");
        assert_eq!(read.alpha_data, original.alpha_data, "alpha mismatch");
    }

    #[test]
    fn roundtrip_chunk_order() {
        let original = make_test_xtd();
        let bytes = Writer::write(&original).expect("write failed");
        let read = Reader::read(&bytes).expect("read failed");

        assert_eq!(read.chunk_order.len(), original.chunk_order.len());
        for (r, o) in read.chunk_order.iter().zip(&original.chunk_order) {
            assert_eq!(r.id, o.id, "chunk id mismatch");
            assert_eq!(
                r.alignment_log2, o.alignment_log2,
                "alignment mismatch for chunk {:#X}",
                o.id
            );
        }
    }

    #[test]
    fn roundtrip_ecf_file_id() {
        let original = make_test_xtd();
        let bytes = Writer::write(&original).expect("write failed");
        let read = Reader::read(&bytes).expect("read failed");

        assert_eq!(read.ecf_file_id, original.ecf_file_id);
    }

    #[test]
    fn double_roundtrip_identical_bytes() {
        let original = make_test_xtd();
        let bytes1 = Writer::write(&original).expect("write 1 failed");
        let read1 = Reader::read(&bytes1).expect("read 1 failed");
        let bytes2 = Writer::write(&read1).expect("write 2 failed");

        assert_eq!(
            bytes1,
            bytes2,
            "second write produced different bytes (len {} vs {})",
            bytes1.len(),
            bytes2.len()
        );
    }

    #[test]
    fn roundtrip_shadow_bool_values() {
        // Specifically test both true and false for can_cast_shadows
        let mut file = make_test_xtd();
        file.visual_chunks[0].can_cast_shadows = true;
        file.visual_chunks[1].can_cast_shadows = false;

        let bytes = Writer::write(&file).expect("write failed");
        let read = Reader::read(&bytes).expect("read failed");

        assert!(read.visual_chunks[0].can_cast_shadows);
        assert!(!read.visual_chunks[1].can_cast_shadows);
    }

    #[test]
    fn roundtrip_negative_coords() {
        let mut file = make_test_xtd();
        file.header.world_min = [-999.5, -0.001, -12345.0];
        file.header.world_max = [999.5, 0.001, 12345.0];
        file.visual_chunks[0].min = [-999.5, -0.001, -12345.0];
        file.visual_chunks[0].max = [0.0, 0.0, 0.0];

        let bytes = Writer::write(&file).expect("write failed");
        let read = Reader::read(&bytes).expect("read failed");

        assert_float_array_bits_eq(read.header.world_min, file.header.world_min);
        assert_float_array_bits_eq(read.header.world_max, file.header.world_max);
        assert_float_array_bits_eq(read.visual_chunks[0].min, file.visual_chunks[0].min);
        assert_float_array_bits_eq(read.visual_chunks[0].max, file.visual_chunks[0].max);
    }

    #[test]
    fn roundtrip_tessellation_decode() {
        let original = make_test_xtd();
        let bytes = Writer::write(&original).expect("write failed");
        let read = Reader::read(&bytes).expect("read failed");

        let tess_orig = original
            .decode_tessellation()
            .expect("original tess decode failed")
            .expect("original tess chunk missing");
        let tess_read = read
            .decode_tessellation()
            .expect("roundtrip tess decode failed")
            .expect("roundtrip tess chunk missing");

        assert_eq!(tess_read.num_x_patches, tess_orig.num_x_patches);
        assert_eq!(tess_read.num_z_patches, tess_orig.num_z_patches);
        assert_eq!(tess_read.max_tess_level, tess_orig.max_tess_level);
        assert_eq!(tess_read.patch_tess_levels, tess_orig.patch_tess_levels);
        assert_eq!(
            tess_read.patch_bounding_boxes.len(),
            tess_orig.patch_bounding_boxes.len()
        );
        for (r, o) in tess_read
            .patch_bounding_boxes
            .iter()
            .zip(&tess_orig.patch_bounding_boxes)
        {
            assert_float_array_bits_eq(r.min, o.min);
            assert_float_array_bits_eq(r.max, o.max);
        }
    }

    #[test]
    fn roundtrip_empty_optional_data() {
        let mut file = make_test_xtd();
        // Clear optional data blobs
        file.ao_data.clear();
        file.alpha_data.clear();
        file.lighting_data.clear();
        // Remove their chunk_order entries too
        file.chunk_order
            .retain(|m| m.id != CHUNK_AO && m.id != CHUNK_ALPHA && m.id != CHUNK_LIGHTING);

        let bytes = Writer::write(&file).expect("write failed");
        let read = Reader::read(&bytes).expect("read failed");

        assert!(read.ao_data.is_empty());
        assert!(read.alpha_data.is_empty());
        assert!(read.lighting_data.is_empty());
        // Header and visual chunks should still survive
        assert_eq!(read.header.version, XTD_VERSION);
        assert_eq!(read.visual_chunks.len(), 4);
    }

    #[test]
    fn writer_rejects_bad_signatures_and_unknown_chunks() {
        let mut file = make_test_xtd();
        file.ecf_file_id = 0xDEAD_BEEF;
        assert!(matches!(
            Writer::write(&file),
            Err(Error::InvalidFileId { .. })
        ));

        let mut file = make_test_xtd();
        file.chunk_order.push(test_chunk_meta(0xDEAD));
        assert!(matches!(
            Writer::write(&file),
            Err(Error::UnsupportedChunk(0xDEAD))
        ));
    }

    #[test]
    fn writer_rejects_inconsistent_game_layout() {
        let mut file = make_test_xtd();
        file.header.num_x_verts = 127;
        assert!(Writer::write(&file).is_err());

        let mut file = make_test_xtd();
        file.alpha_data.pop();
        assert!(Writer::write(&file).is_err());
    }

    #[test]
    fn roundtrip_preserves_ecf_metadata() {
        let mut file = make_test_xtd();
        file.ecf_flags = 0x1234;
        file.chunk_order[0].flags = 0x10;
        file.chunk_order[0].resource_flags = ecf::resource_flags::CONTIGUOUS;
        let bytes = Writer::write(&file).unwrap();
        let container = ecf::Reader::new(&bytes).unwrap();
        assert_eq!(container.header().flags, 0x1234);
        assert_eq!(container.chunks()[0].flags, 0x10);
        assert_eq!(
            container.chunks()[0].resource_flags,
            ecf::resource_flags::CONTIGUOUS
        );
    }
}
