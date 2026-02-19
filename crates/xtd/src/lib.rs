//! XTD (Terrain Displacement) format handling for Halo Wars.
//!
//! XTD files store terrain height/displacement data in an ECF container.
//!
//! ## File Structure
//!
//! XTD files contain the following chunks:
//! - `0x1111` - XTDHeader: Main header with terrain dimensions
//! - `0x2222` - TerrainChunk: Per-chunk visual headers (196 chunks typical)
//! - `0x8888` - AtlasChunk: Terrain atlas texture data
//! - `0xAAAA` - TessChunk: Tessellation data  
//! - `0xBBBB` - LightingChunk: Lighting data
//! - `0xCCCC` - AOChunk: Ambient occlusion data
//! - `0xDDDD` - AlphaChunk: Alpha/transparency data

mod error;
pub use error::{Error, Result};

mod types;
pub use types::*;

mod reader;
pub use reader::XtdReader;

mod writer;
pub use writer::XtdWriter;

mod decode;
pub use decode::{
    unpack_normal, unpack_position, AtlasHeader, RawTerrainData, TerrainVertices, TessellatedMesh,
};

// ============================================================================
// XTD Constants
// ============================================================================

/// XTD file version.
pub const XTD_VERSION: i32 = 0x000C;

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
    use super::*;

    // Test files are in the extracted test_extract directory (relative to workspace root)
    const TEST_XTD_PATH: &str =
        "../../test_extract/scenario/skirmish/design/blood_gulch/blood_gulch.xtd";

    #[test]
    #[ignore = "requires extracted XTD file"]
    fn test_read_xtd() {
        let data = std::fs::read(TEST_XTD_PATH).expect("Failed to read XTD file");
        let file = XtdReader::read(&data).expect("Failed to parse XTD");

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
        let file = XtdReader::read(&original).expect("Failed to parse XTD");
        let rewritten = XtdWriter::write(&file).expect("Failed to write XTD");

        println!("Original size: {} bytes", original.len());
        println!("Rewritten size: {} bytes", rewritten.len());

        // Print first 64 bytes of both for comparison
        println!("\nOriginal header (first 64 bytes):");
        for i in 0..64 {
            if i % 16 == 0 {
                print!("  {:04X}: ", i);
            }
            print!("{:02X} ", original[i]);
            if i % 16 == 15 {
                println!();
            }
        }
        println!("\nRewritten header (first 64 bytes):");
        for i in 0..64 {
            if i % 16 == 0 {
                print!("  {:04X}: ", i);
            }
            print!("{:02X} ", rewritten[i]);
            if i % 16 == 15 {
                println!();
            }
        }

        // Compare byte-for-byte, skipping adler32 (bytes 8-11)
        let mut diff_count = 0;
        let min_len = original.len().min(rewritten.len());
        for i in 0..min_len {
            // Skip adler32 field (offset 8-11)
            if i >= 8 && i <= 11 {
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
            panic!(
                "XTD roundtrip failed: {} non-checksum differences!",
                diff_count
            );
        }
    }

    #[test]
    #[ignore = "requires extracted XTD file"]
    fn test_decode_vertices() {
        let data = std::fs::read(TEST_XTD_PATH).expect("Failed to read XTD file");
        let file = XtdReader::read(&data).expect("Failed to parse XTD");

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
        let indices = vertices.generate_indices();
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

        // Check normals are normalized (approximately)
        for (i, norm) in vertices.normals.iter().take(100).enumerate() {
            let len = (norm[0] * norm[0] + norm[1] * norm[1] + norm[2] * norm[2]).sqrt();
            assert!(
                (len - 1.0).abs() < 0.1,
                "Normal {} not normalized: {:?} (len={})",
                i,
                norm,
                len
            );
        }
    }

    #[test]
    #[ignore = "requires extracted XTD file"]
    fn test_decode_tessellation() {
        let data = std::fs::read(TEST_XTD_PATH).expect("Failed to read XTD file");
        let file = XtdReader::read(&data).expect("Failed to parse XTD");

        println!("Raw tess_data size: {} bytes", file.tess_data.len());

        let tess = file
            .decode_tessellation()
            .expect("Failed to decode tessellation");

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
            println!("    Level {}: {} patches", level, count);
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
        let file = XtdReader::read(&data).expect("Failed to parse XTD");

        let vertices = file.decode_vertices().expect("Failed to decode vertices");
        let tess = file
            .decode_tessellation()
            .expect("Failed to decode tessellation");

        println!("Original mesh:");
        println!("  Vertices: {}", vertices.positions.len());
        let original_indices = vertices.generate_indices();
        println!("  Triangles: {}", original_indices.len() / 3);

        // Generate tessellated mesh
        let tessellated = vertices.tessellate(&tess);

        println!("\nTessellated mesh:");
        println!("  Vertices: {}", tessellated.positions.len());
        println!("  Triangles: {}", tessellated.indices.len() / 3);
        println!(
            "  Vertex increase: {:.1}x",
            tessellated.positions.len() as f32 / vertices.positions.len() as f32
        );
        println!(
            "  Triangle increase: {:.1}x",
            tessellated.indices.len() as f32 / original_indices.len() as f32
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
                (idx as usize) < tessellated.positions.len(),
                "Invalid index {} (max {})",
                idx,
                tessellated.positions.len()
            );
        }
    }
}
