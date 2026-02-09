//! UAX file container for reading and writing.
//!
//! This module provides a container that preserves the raw Granny data
//! while exposing parsed animation metadata for inspection and modification.

use crate::types::{self, animation, file_info, GRANNY_HEADER_SIZE};
use crate::{Error, Result, UAX_CHUNK_ID, UAX_FILE_ID};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use ecf::{EcfChunkHeader, EcfHeader, EcfReader};
use std::io::{Cursor, Read, Seek, SeekFrom, Write};

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
    /// Raw Granny chunk data (includes 32-byte header + file_info + all data).
    chunk_data: Vec<u8>,
}

impl UaxFile {
    /// Read a UAX file from a reader.
    pub fn from_reader<R: Read + Seek>(reader: R) -> Result<Self> {
        let mut ecf = EcfReader::new(reader)?;

        // Validate file ID
        let file_id = ecf.header().id;
        if file_id != UAX_FILE_ID {
            return Err(Error::InvalidFileId(file_id));
        }

        // Find the UAX chunk (0x0700) by ID
        let chunk_index = ecf
            .chunks()
            .iter()
            .position(|c| c.id == UAX_CHUNK_ID)
            .ok_or(Error::ChunkNotFound)?;

        // Store original headers for round-trip
        let ecf_header = ecf.header().clone();
        let chunk_header = ecf.chunks()[chunk_index].clone();

        let chunk_data = ecf.read_chunk_data(chunk_index)?;

        if chunk_data.len() <= GRANNY_HEADER_SIZE {
            return Err(Error::ChunkTooSmall(
                chunk_data.len(),
                GRANNY_HEADER_SIZE + 1,
            ));
        }

        Ok(Self {
            ecf_header,
            chunk_header,
            chunk_data,
        })
    }

    /// Read a UAX file from bytes.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        Self::from_reader(Cursor::new(data))
    }

    /// Write the UAX file to a writer, preserving original ECF structure.
    pub fn write<W: Write + Seek>(&self, mut writer: W) -> Result<()> {
        // Write the original ECF header
        self.ecf_header.write(&mut writer)?;

        // Write the original chunk header (with updated checksum if data changed)
        let mut chunk_header = self.chunk_header.clone();
        chunk_header.adler32 = ecf::adler32(&self.chunk_data);
        chunk_header.size = self.chunk_data.len() as u32;
        chunk_header.write(&mut writer)?;

        // Seek to the chunk data offset and write the data
        writer.seek(SeekFrom::Start(chunk_header.offset as u64))?;
        writer.write_all(&self.chunk_data)?;

        // Pad to original file size if needed
        let current_pos = writer.stream_position()? as usize;
        let target_size = self.ecf_header.file_size as usize;
        if current_pos < target_size {
            writer.write_all(&vec![0u8; target_size - current_pos])?;
        }

        Ok(())
    }

    /// Write the UAX file to bytes.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut buf = Cursor::new(Vec::new());
        self.write(&mut buf)?;
        Ok(buf.into_inner())
    }

    /// Get the raw chunk data (for debugging/inspection).
    pub fn chunk_data(&self) -> &[u8] {
        &self.chunk_data
    }

    /// Get the file_info data (after 32-byte Granny header).
    fn file_info_data(&self) -> &[u8] {
        &self.chunk_data[GRANNY_HEADER_SIZE..]
    }

    /// Get the file_info data mutably.
    fn file_info_data_mut(&mut self) -> &mut [u8] {
        &mut self.chunk_data[GRANNY_HEADER_SIZE..]
    }

    /// Get the animation count.
    pub fn animation_count(&self) -> Result<i32> {
        self.read_i32_at(file_info::ANIMATION_COUNT)
    }

    /// Get the track group count from file_info.
    pub fn track_group_count(&self) -> Result<i32> {
        self.read_i32_at(file_info::TRACK_GROUP_COUNT)
    }

    /// Get animation name.
    pub fn animation_name(&self) -> Result<Option<String>> {
        let anim_offset = self.animation_offset()?;
        let data = self.file_info_data();

        if anim_offset + animation::NAME_PTR + 8 > data.len() {
            return Err(Error::InvalidPointerOffset(anim_offset as u64, data.len()));
        }

        let mut cursor = Cursor::new(&data[anim_offset..]);
        let name_ptr = cursor.read_u64::<LittleEndian>()?;

        if name_ptr == 0 || name_ptr as usize >= data.len() {
            return Ok(None);
        }

        read_cstring(data, name_ptr).map(Some)
    }

    /// Get animation duration in seconds.
    pub fn duration(&self) -> Result<f32> {
        let anim_offset = self.animation_offset()?;
        self.read_f32_at_offset(anim_offset + animation::DURATION)
    }

    /// Set animation duration in seconds.
    pub fn set_duration(&mut self, duration: f32) -> Result<()> {
        let anim_offset = self.animation_offset()?;
        self.write_f32_at_offset(anim_offset + animation::DURATION, duration)
    }

    /// Get animation time step between keyframes.
    pub fn time_step(&self) -> Result<f32> {
        let anim_offset = self.animation_offset()?;
        self.read_f32_at_offset(anim_offset + animation::TIME_STEP)
    }

    /// Get animation oversampling factor.
    pub fn oversampling(&self) -> Result<f32> {
        let anim_offset = self.animation_offset()?;
        self.read_f32_at_offset(anim_offset + animation::OVERSAMPLING)
    }

    // Helper to get the animation offset in file_info data
    fn animation_offset(&self) -> Result<usize> {
        let data = self.file_info_data();

        // Read Animations pointer (32-bit, needs rebasing)
        let mut cursor = Cursor::new(&data[file_info::ANIMATIONS_PTR..]);
        let stored = cursor.read_u32::<LittleEndian>()? as u64;
        let offset = types::rebase_pointer(stored) as usize;

        if offset == 0 || offset >= data.len() {
            return Err(Error::NoAnimations);
        }

        Ok(offset)
    }

    // Read i32 at offset in file_info data
    fn read_i32_at(&self, offset: usize) -> Result<i32> {
        let data = self.file_info_data();
        if offset + 4 > data.len() {
            return Err(Error::InvalidPointerOffset(offset as u64, data.len()));
        }
        let mut cursor = Cursor::new(&data[offset..]);
        Ok(cursor.read_i32::<LittleEndian>()?)
    }

    // Read f32 at offset in file_info data
    fn read_f32_at_offset(&self, offset: usize) -> Result<f32> {
        let data = self.file_info_data();
        if offset + 4 > data.len() {
            return Err(Error::InvalidPointerOffset(offset as u64, data.len()));
        }
        let mut cursor = Cursor::new(&data[offset..]);
        Ok(cursor.read_f32::<LittleEndian>()?)
    }

    // Write f32 at offset in file_info data
    fn write_f32_at_offset(&mut self, offset: usize, value: f32) -> Result<()> {
        let data = self.file_info_data_mut();
        if offset + 4 > data.len() {
            return Err(Error::InvalidPointerOffset(offset as u64, data.len()));
        }
        let mut cursor = Cursor::new(&mut data[offset..]);
        cursor.write_f32::<LittleEndian>(value)?;
        Ok(())
    }
}

/// Read a null-terminated C string from data at the given offset.
fn read_cstring(data: &[u8], offset: u64) -> Result<String> {
    let offset = offset as usize;
    if offset >= data.len() {
        return Err(Error::InvalidPointerOffset(offset as u64, data.len()));
    }

    let bytes = &data[offset..];
    let end = bytes
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(bytes.len().min(256));

    String::from_utf8(bytes[..end].to_vec())
        .map_err(|_| Error::StringReadError(offset as u64))
}

#[cfg(test)]
mod tests {
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
        let written_data = uax.to_bytes().expect("Failed to write UAX");

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

        println!("Round-trip test passed! All {} bytes identical.", original_data.len());
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
                let written_data = uax.to_bytes().expect("Failed to write UAX");

                assert_eq!(
                    original_data, written_data,
                    "Round-trip failed for {:?}",
                    path.file_name()
                );
                tested += 1;
                println!("✓ {:?} - {} bytes", path.file_name().unwrap(), original_data.len());
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
        let written = uax.to_bytes().expect("Failed to write UAX");
        let reloaded = UaxFile::from_bytes(&written).expect("Failed to re-read UAX");

        assert!((reloaded.duration().unwrap() - new_duration).abs() < 0.001);

        println!("Modify duration test passed!");
        println!("  Original: {}", original_duration);
        println!("  Modified: {}", reloaded.duration().unwrap());
    }
}
