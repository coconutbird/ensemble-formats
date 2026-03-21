//! XTT (Terrain Textures) format handling for Halo Wars.
//!
//! XTT files store terrain texture data in an ECF container.
//!
//! ## File Structure
//!
//! XTT files contain the following chunks:
//! - `0x1111` - XTTHeader: Main header with texture counts
//! - `0x2222` - TerrainAtlasLinkChunk: Per-chunk texture links (196 chunks typical)
//! - `0x6666` - AtlasChunkAlbedo: Albedo texture atlas
//! - `0x8888` - RoadChunk: Road data (optional)
//! - `0xAAAA` - FoliageHeaderChunk: Foliage header data
//! - `0xBBBB` - FoliageQNChunk: Foliage quantization data (multiple)

mod error;
pub use error::{Error, Result};

mod types;
pub use types::*;

mod reader;
pub use reader::XttReader;

mod writer;
pub use writer::XttWriter;

mod decode;
pub use decode::{
    ALPHA_TEXTURE_SIZE, AlbedoAtlas, AlbedoHeader, DecalAlphaData, SplatAlphaData, decode_road_data,
};

// ============================================================================
// XTT Constants
// ============================================================================

/// XTT file version.
pub const XTT_VERSION: i32 = 0x0004;

/// XTT header chunk ID.
pub const CHUNK_XTT_HEADER: u64 = 0x1111;

/// Terrain atlas link chunk ID.
pub const CHUNK_ATLAS_LINK: u64 = 0x2222;

/// Atlas chunk albedo ID.
pub const CHUNK_ATLAS_ALBEDO: u64 = 0x6666;

/// Road chunk ID.
pub const CHUNK_ROAD: u64 = 0x8888;

/// Foliage header chunk ID.
pub const CHUNK_FOLIAGE_HEADER: u64 = 0xAAAA;

/// Foliage quantization chunk ID.
pub const CHUNK_FOLIAGE_QN: u64 = 0xBBBB;

/// Maximum filename size in XTT.
pub const FILENAME_SIZE: usize = 256;

#[cfg(test)]
mod tests {
    use super::*;

    // Test files are in the extracted --filter directory (relative to workspace root)
    const TEST_XTT_PATH: &str = "../../--filter/scenario/skirmish/design/release/release.xtt";

    #[test]
    #[ignore = "requires extracted XTT file"]
    fn test_read_xtt() {
        let data = std::fs::read(TEST_XTT_PATH).expect("Failed to read XTT file");
        let file = XttReader::read(&data).expect("Failed to parse XTT");

        println!("XTT Header:");
        println!("  Version: 0x{:04X}", file.header.version);
        println!("  NumActiveTextures: {}", file.header.num_active_textures);
        println!("  NumActiveDecals: {}", file.header.num_active_decals);
        println!(
            "  NumActiveDecalInstances: {}",
            file.header.num_active_decal_instances
        );
        println!("\nLinkers: {}", file.linkers.len());
        println!("Header extra: {} bytes", file.header_extra.len());
        println!("Albedo data: {} bytes", file.albedo_data.len());
        println!("Road data: {} bytes", file.road_data.len());
        println!("Foliage sets: {}", file.foliage.sets.len());
        for (i, set) in file.foliage.sets.iter().enumerate() {
            println!("  [{}] {}", i, set.filename);
        }
        println!("Foliage QN chunks: {}", file.foliage.qn_chunks.len());

        assert_eq!(file.header.version, XTT_VERSION);
        assert!(!file.linkers.is_empty());
    }

    #[test]
    #[ignore = "requires extracted XTT file"]
    fn test_xtt_roundtrip() {
        let original = std::fs::read(TEST_XTT_PATH).expect("Failed to read XTT file");
        let file = XttReader::read(&original).expect("Failed to parse XTT");
        let rewritten = XttWriter::write(&file).expect("Failed to write XTT");

        println!("Original size: {} bytes", original.len());
        println!("Rewritten size: {} bytes", rewritten.len());

        // Print first 64 bytes of both for comparison
        println!("\nOriginal header (first 64 bytes):");
        for (i, &byte) in original.iter().enumerate().take(64) {
            if i % 16 == 0 {
                print!("  {:04X}: ", i);
            }
            print!("{:02X} ", byte);
            if i % 16 == 15 {
                println!();
            }
        }
        println!("\nRewritten header (first 64 bytes):");
        for (i, &byte) in rewritten.iter().enumerate().take(64) {
            if i % 16 == 0 {
                print!("  {:04X}: ", i);
            }
            print!("{:02X} ", byte);
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
            println!("\nXTT roundtrip: DATA IDENTICAL (only adler32 differs - recalculated)");
        } else {
            if original.len() != rewritten.len() {
                println!("Size mismatch: {} vs {}", original.len(), rewritten.len());
            }
            panic!(
                "XTT roundtrip failed: {} non-checksum differences!",
                diff_count
            );
        }
    }
}

#[test]
fn check_blood_gulch_layer_ids() {
    // Path is relative to workspace root (ensemble-rs/)
    let data =
        std::fs::read("../../test_extract/scenario/skirmish/design/blood_gulch/blood_gulch.xtt")
            .expect("Failed to read XTT file");
    let file = XttReader::read(&data).expect("Failed to parse XTT");

    println!("\n=== ACTIVE TEXTURES ===");
    for (i, tex) in file.active_textures.iter().enumerate() {
        println!("[{}] {}", i, tex.filename);
    }

    println!("\n=== FIRST 5 LINKER SPLAT_LAYER_IDS ===");
    for linker in file.linkers.iter().take(5) {
        println!(
            "Chunk ({}, {}): {:?}",
            linker.grid_x, linker.grid_z, linker.splat_layer_ids
        );
    }
}
