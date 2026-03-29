//! UAX file container for reading and writing.
//!
//! This module provides a container that preserves the raw Granny data
//! while exposing parsed animation metadata for inspection and modification.
//! The chunk data IS `file_info` directly — no separate header.

use alloc::string::String;
use alloc::vec::Vec;

use crate::types::{animation, file_info, read_cstring, read_f32_le, read_i32_le, read_ptr};
use crate::{Error, Result, UAX_CHUNK_ID, UAX_FILE_ID};
use ecf::{EcfChunkHeader, EcfHeader, Reader as EcfReader};

/// A parsed UAX animation file.
///
/// This struct holds the raw chunk data from the ECF container, allowing
/// perfect round-trip serialization while exposing parsed animation metadata.
#[derive(Debug, Clone)]
pub struct UaxFile {
    /// Original ECF header (for round-trip fidelity).
    ecf_header: EcfHeader,
    /// Original chunk header (for round-trip fidelity).
    chunk_header: EcfChunkHeader,
    /// Raw chunk data — this IS the `file_info` structure.
    chunk_data: Vec<u8>,
}

impl UaxFile {
    /// Read a UAX file from a byte slice.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let ecf = EcfReader::new(data)?;

        let hdr_id = ecf.header().id;
        if hdr_id != UAX_FILE_ID {
            return Err(Error::InvalidFileId(hdr_id));
        }

        let chunk_index = ecf
            .chunks()
            .iter()
            .position(|c| c.id == UAX_CHUNK_ID)
            .ok_or(Error::ChunkNotFound)?;

        let ecf_header = ecf.header().clone();
        let chunk_header = ecf.chunks()[chunk_index].clone();
        let chunk_data = ecf.chunk_data(chunk_index)?;

        if chunk_data.len() < file_info::MIN_SIZE {
            return Err(Error::ChunkTooSmall(chunk_data.len(), file_info::MIN_SIZE));
        }

        Ok(Self {
            ecf_header,
            chunk_header,
            chunk_data,
        })
    }

    /// Write the UAX file to bytes, preserving original ECF structure.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.ecf_header.to_bytes());

        let mut chunk_header = self.chunk_header.clone();
        chunk_header.adler32 = ecf::adler32(&self.chunk_data);
        chunk_header.size = self.chunk_data.len() as u32;
        out.extend_from_slice(&chunk_header.to_bytes());

        let chunk_offset = chunk_header.offset as usize;
        if out.len() < chunk_offset {
            out.resize(chunk_offset, 0);
        }
        out.extend_from_slice(&self.chunk_data);

        let target_size = self.ecf_header.file_size as usize;
        if out.len() < target_size {
            out.resize(target_size, 0);
        }

        out
    }

    /// Get the raw chunk data (for debugging/inspection).
    pub fn chunk_data(&self) -> &[u8] {
        &self.chunk_data
    }

    /// Get the animation count.
    pub fn animation_count(&self) -> Result<i32> {
        read_i32_le(&self.chunk_data, file_info::ANIMATION_COUNT)
            .ok_or(Error::UnexpectedEof)
    }

    /// Get the track group count from file_info.
    pub fn track_group_count(&self) -> Result<i32> {
        read_i32_le(&self.chunk_data, file_info::TRACK_GROUP_COUNT)
            .ok_or(Error::UnexpectedEof)
    }

    /// Get animation name.
    pub fn animation_name(&self) -> Result<Option<String>> {
        let anim_off = self.animation_struct_offset()?;
        let fi = &self.chunk_data;
        Ok(read_ptr(fi, anim_off + animation::NAME_PTR)
            .and_then(|p| read_cstring(fi, p)))
    }

    /// Get animation duration in seconds.
    pub fn duration(&self) -> Result<f32> {
        let off = self.animation_struct_offset()?;
        read_f32_le(&self.chunk_data, off + animation::DURATION)
            .ok_or(Error::UnexpectedEof)
    }

    /// Set animation duration in seconds.
    pub fn set_duration(&mut self, duration: f32) -> Result<()> {
        let off = self.animation_struct_offset()?;
        let pos = off + animation::DURATION;
        if pos + 4 > self.chunk_data.len() {
            return Err(Error::UnexpectedEof);
        }
        self.chunk_data[pos..pos + 4].copy_from_slice(&duration.to_le_bytes());
        Ok(())
    }

    /// Get animation time step between keyframes.
    pub fn time_step(&self) -> Result<f32> {
        let off = self.animation_struct_offset()?;
        read_f32_le(&self.chunk_data, off + animation::TIME_STEP)
            .ok_or(Error::UnexpectedEof)
    }

    /// Get animation oversampling factor.
    pub fn oversampling(&self) -> Result<f32> {
        let off = self.animation_struct_offset()?;
        read_f32_le(&self.chunk_data, off + animation::OVERSAMPLING)
            .ok_or(Error::UnexpectedEof)
    }

    /// Resolve the offset of the first animation struct within chunk_data.
    ///
    /// file_info has Animations** at +0x7C → ptr array → first animation struct.
    fn animation_struct_offset(&self) -> Result<usize> {
        let fi = &self.chunk_data;
        // Animations** → array of pointers
        let arr = read_ptr(fi, file_info::ANIMATIONS_PTR)
            .ok_or(Error::NoAnimations)?;
        // First animation pointer
        read_ptr(fi, arr)
            .ok_or(Error::NoAnimations)
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use std::{eprintln, println};

    use super::*;

    #[test]
    fn test_uax_file_roundtrip() {
        let test_path = "../../temp_uax/art/campaign/npc/forge_01/shotgun_attack_01.uax";
        if !std::path::Path::new(test_path).exists() {
            eprintln!("Skipping test - UAX file not found");
            return;
        }

        let original_data = std::fs::read(test_path).expect("Failed to read UAX file");

        // Parse the file
        let uax = UaxFile::from_bytes(&original_data).expect("Failed to parse UAX");

        // Check we can read animation properties
        assert!(uax.animation_count().unwrap() >= 1);
        let duration = uax.duration().unwrap();
        assert!(duration > 0.0);
        println!("Animation name: {:?}", uax.animation_name().unwrap());
        println!("Duration: {}", duration);
        println!("TimeStep: {}", uax.time_step().unwrap());
        println!("Oversampling: {}", uax.oversampling().unwrap());

        // Write back to bytes
        let written_data = uax.to_bytes();

        // Compare - they should be identical
        assert_eq!(
            original_data.len(),
            written_data.len(),
            "File sizes differ: original={}, written={}",
            original_data.len(),
            written_data.len()
        );

        // Find first difference if any
        let mut diff_count = 0;
        for (i, (a, b)) in original_data.iter().zip(written_data.iter()).enumerate() {
            if a != b {
                if diff_count < 5 {
                    println!(
                        "Byte diff at offset 0x{:04X}: original=0x{:02X}, written=0x{:02X}",
                        i, a, b
                    );
                }
                diff_count += 1;
            }
        }
        if diff_count > 0 {
            panic!("Found {} byte differences!", diff_count);
        }

        println!(
            "Round-trip test passed! All {} bytes identical.",
            original_data.len()
        );
    }

    #[test]
    fn test_uax_file_roundtrip_all() {
        let test_dir = "../../temp_uax/art/campaign/npc/forge_01";
        if !std::path::Path::new(test_dir).exists() {
            eprintln!("Skipping test - UAX directory not found");
            return;
        }

        let mut tested = 0;
        for entry in std::fs::read_dir(test_dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.extension().map(|e| e == "uax").unwrap_or(false) {
                let original_data = std::fs::read(&path).expect("Failed to read UAX file");
                let uax = UaxFile::from_bytes(&original_data).expect("Failed to parse UAX");
                let written_data = uax.to_bytes();

                assert_eq!(
                    original_data,
                    written_data,
                    "Round-trip failed for {:?}",
                    path.file_name()
                );
                tested += 1;
                println!(
                    "✓ {:?} - {} bytes",
                    path.file_name().unwrap(),
                    original_data.len()
                );
            }
        }
        println!("\nAll {} UAX files round-trip perfectly!", tested);
    }

    #[test]
    fn test_uax_modify_duration() {
        let test_path = "../../temp_uax/art/campaign/npc/forge_01/shotgun_attack_01.uax";
        if !std::path::Path::new(test_path).exists() {
            eprintln!("Skipping test - UAX file not found");
            return;
        }

        let original_data = std::fs::read(test_path).expect("Failed to read UAX file");

        // Parse the file
        let mut uax = UaxFile::from_bytes(&original_data).expect("Failed to parse UAX");

        // Modify duration
        let original_duration = uax.duration().unwrap();
        let new_duration = original_duration * 2.0;
        uax.set_duration(new_duration).unwrap();

        // Verify the change
        assert!((uax.duration().unwrap() - new_duration).abs() < 0.001);

        // Write and re-read
        let written = uax.to_bytes();
        let reloaded = UaxFile::from_bytes(&written).expect("Failed to re-read UAX");

        assert!((reloaded.duration().unwrap() - new_duration).abs() < 0.001);

        println!("Modify duration test passed!");
        println!("  Original: {}", original_duration);
        println!("  Modified: {}", reloaded.duration().unwrap());
    }
}
