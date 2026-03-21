//! DDX texture format parser for Halo Wars.
//!
//! DDX is a texture container format used by Ensemble Studios.
//! It wraps texture data in an ECF container with optional deflate compression.
//!
//! ## DDX Structure
//!
//! A DDX file is an ECF container with file ID `0x13CF5D01` containing:
//! - Header chunk (ID `0x1D8828C6ECAF45F2`): Texture metadata
//! - Mip0 chunk (ID `0x3F74B8E87D2B44BF`): Base mip level data
//! - MipChain chunk (ID `0x46F1FD3F394348B8`, optional): Additional mip levels
//!
//! ## Supported Formats
//!
//! - Raw: A8R8G8B8, A8B8G8R8, A8, A16B16G16R16F
//! - DXT: DXT1, DXT3, DXT5, DXT5N, DXT5Y, DXN, DXT5H
//! - DXTQ (custom quantized): DXT1Q, DXT5Q, DXT5HQ, DXNQ, DXT5YQ
//!
//! ## Example
//!
//! ```no_run
//! use ddx::DdxTexture;
//!
//! let data = std::fs::read("texture.ddx").unwrap();
//! let texture = DdxTexture::from_bytes(&data).unwrap();
//!
//! println!("Size: {}x{}", texture.info.width, texture.info.height);
//! println!("Format: {:?}", texture.info.data_format);
//! println!("Mip levels: {}", texture.info.num_mip_levels);
//! ```

mod decode;
mod error;
mod format;
mod header;
mod reader;
mod writer;

pub use decode::DecodedTexture;
pub use error::{Error, Result};
pub use format::DataFormat;
pub use header::{
    DDX_CURRENT_VERSION, DDX_ECF_FILE_ID, DDX_HEADER_CHUNK_ID, DDX_HEADER_MAGIC,
    DDX_MIN_REQUIRED_VERSION, DDX_MIP0_CHUNK_ID, DDX_MIPCHAIN_CHUNK_ID, DdxHeader, Platform,
    ResourceType, flags,
};
pub use reader::{DdxTexture, TextureInfo};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore] // Requires root.era to be present
    fn test_parse_ddx_from_era() {
        use era::EraArchive;

        // Try workspace root first, then current dir
        let era_path = if std::path::Path::new("root.era").exists() {
            "root.era".to_string()
        } else if std::path::Path::new("../../root.era").exists() {
            "../../root.era".to_string()
        } else {
            panic!("Cannot find root.era - run from workspace root");
        };

        let mut archive = EraArchive::open(&era_path).expect("Failed to open root.era");

        // Find a DDX file
        let ddx_idx = archive
            .iter()
            .position(|e| {
                e.filename
                    .as_ref()
                    .map(|n| n.ends_with(".ddx"))
                    .unwrap_or(false)
            })
            .expect("No DDX file found in archive");

        let entry = archive.entry(ddx_idx).unwrap();
        let filename = entry.filename.clone().unwrap();
        println!("Testing DDX file: {}", filename);

        let data = archive.read_entry(ddx_idx).expect("Failed to read DDX");
        println!("DDX data size: {} bytes", data.len());

        let texture = DdxTexture::from_bytes(&data).expect("Failed to parse DDX");
        println!("Parsed DDX texture:");
        println!("  Width: {}", texture.info.width);
        println!("  Height: {}", texture.info.height);
        println!("  Format: {:?}", texture.info.data_format);
        println!("  Resource Type: {:?}", texture.info.resource_type);
        println!("  Mip Levels: {}", texture.info.num_mip_levels);
        println!("  Has Alpha: {}", texture.info.has_alpha);
        println!("  Platform: {:?}", texture.info.platform);
        println!("  HDR Scale: {}", texture.info.hdr_scale);
        println!("  Decompressed Data Size: {} bytes", texture.data.len());

        // Basic sanity checks
        assert!(texture.info.width > 0);
        assert!(texture.info.height > 0);
        assert!(texture.info.num_mip_levels >= 1);
    }

    #[test]
    #[ignore] // Requires root.era to be present
    fn test_parse_all_ddx_from_era() {
        use era::EraArchive;

        let era_path = if std::path::Path::new("root.era").exists() {
            "root.era".to_string()
        } else if std::path::Path::new("../../root.era").exists() {
            "../../root.era".to_string()
        } else {
            panic!("Cannot find root.era - run from workspace root");
        };

        let mut archive = EraArchive::open(&era_path).expect("Failed to open root.era");

        let ddx_indices: Vec<usize> = archive
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                e.filename
                    .as_ref()
                    .map(|n| n.ends_with(".ddx"))
                    .unwrap_or(false)
            })
            .map(|(i, _)| i)
            .collect();

        println!("Found {} DDX files in archive", ddx_indices.len());

        let mut success = 0;
        let mut failed = 0;

        for idx in ddx_indices {
            let entry = archive.entry(idx).unwrap();
            let filename = entry.filename.clone().unwrap();

            match archive.read_entry(idx) {
                Ok(data) => match DdxTexture::from_bytes(&data) {
                    Ok(texture) => {
                        println!(
                            "OK: {} - {}x{} {:?}",
                            filename,
                            texture.info.width,
                            texture.info.height,
                            texture.info.data_format
                        );
                        success += 1;
                    }
                    Err(e) => {
                        println!("PARSE FAIL: {} - {}", filename, e);
                        failed += 1;
                    }
                },
                Err(e) => {
                    println!("READ FAIL: {} - {}", filename, e);
                    failed += 1;
                }
            }
        }

        println!("\nResults: {} success, {} failed", success, failed);
        assert_eq!(failed, 0, "Some DDX files failed to parse");
    }

    #[test]
    #[ignore] // Requires root.era to be present
    fn test_roundtrip_ddx() {
        use era::EraArchive;

        let era_path = if std::path::Path::new("root.era").exists() {
            "root.era".to_string()
        } else if std::path::Path::new("../../root.era").exists() {
            "../../root.era".to_string()
        } else {
            panic!("Cannot find root.era - run from workspace root");
        };

        let mut archive = EraArchive::open(&era_path).expect("Failed to open root.era");

        // Find DDX files
        let ddx_indices: Vec<usize> = archive
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                e.filename
                    .as_ref()
                    .map(|n| n.ends_with(".ddx"))
                    .unwrap_or(false)
            })
            .map(|(i, _)| i)
            .collect();

        println!("Testing roundtrip on {} DDX files", ddx_indices.len());

        for idx in ddx_indices {
            let entry = archive.entry(idx).unwrap();
            let filename = entry.filename.clone().unwrap();

            let original_data = archive.read_entry(idx).expect("Failed to read DDX");
            let texture = DdxTexture::from_bytes(&original_data).expect("Failed to parse DDX");

            // Write to DDS
            let dds_data = texture.to_dds().expect("Failed to write DDS");

            // Parse the written DDS
            let reparsed = DdxTexture::from_bytes(&dds_data).expect("Failed to reparse DDS");

            // Verify metadata matches
            assert_eq!(
                texture.info.width, reparsed.info.width,
                "{}: width mismatch",
                filename
            );
            assert_eq!(
                texture.info.height, reparsed.info.height,
                "{}: height mismatch",
                filename
            );
            assert_eq!(
                texture.data.len(),
                reparsed.data.len(),
                "{}: data size mismatch",
                filename
            );
            assert_eq!(texture.data, reparsed.data, "{}: data mismatch", filename);

            println!("OK: {} - roundtrip successful", filename);
        }
    }
}
