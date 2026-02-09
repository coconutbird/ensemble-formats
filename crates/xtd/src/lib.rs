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

    // Test files are in the extracted --filter directory (relative to workspace root)
    const TEST_XTD_PATH: &str = "../../--filter/scenario/skirmish/design/release/release.xtd";

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
            if i % 16 == 0 { print!("  {:04X}: ", i); }
            print!("{:02X} ", original[i]);
            if i % 16 == 15 { println!(); }
        }
        println!("\nRewritten header (first 64 bytes):");
        for i in 0..64 {
            if i % 16 == 0 { print!("  {:04X}: ", i); }
            print!("{:02X} ", rewritten[i]);
            if i % 16 == 15 { println!(); }
        }

        // Compare byte-for-byte, skipping adler32 (bytes 8-11)
        let mut diff_count = 0;
        let min_len = original.len().min(rewritten.len());
        for i in 0..min_len {
            // Skip adler32 field (offset 8-11)
            if i >= 8 && i <= 11 { continue; }

            if original[i] != rewritten[i] {
                println!("Diff at offset 0x{:X}: original=0x{:02X}, rewritten=0x{:02X}",
                         i, original[i], rewritten[i]);
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
            panic!("XTD roundtrip failed: {} non-checksum differences!", diff_count);
        }
    }
}

