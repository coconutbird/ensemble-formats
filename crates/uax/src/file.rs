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
        read_i32_le(&self.chunk_data, file_info::ANIMATION_COUNT).ok_or(Error::UnexpectedEof)
    }

    /// Get the track group count from file_info.
    pub fn track_group_count(&self) -> Result<i32> {
        read_i32_le(&self.chunk_data, file_info::TRACK_GROUP_COUNT).ok_or(Error::UnexpectedEof)
    }

    /// Get animation name.
    pub fn animation_name(&self) -> Result<Option<String>> {
        let anim_off = self.animation_struct_offset()?;
        let fi = &self.chunk_data;
        Ok(read_ptr(fi, anim_off + animation::NAME_PTR).and_then(|p| read_cstring(fi, p)))
    }

    /// Get animation duration in seconds.
    pub fn duration(&self) -> Result<f32> {
        let off = self.animation_struct_offset()?;
        read_f32_le(&self.chunk_data, off + animation::DURATION).ok_or(Error::UnexpectedEof)
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
        read_f32_le(&self.chunk_data, off + animation::TIME_STEP).ok_or(Error::UnexpectedEof)
    }

    /// Get animation oversampling factor.
    pub fn oversampling(&self) -> Result<f32> {
        let off = self.animation_struct_offset()?;
        read_f32_le(&self.chunk_data, off + animation::OVERSAMPLING).ok_or(Error::UnexpectedEof)
    }

    /// Resolve the offset of the first animation struct within chunk_data.
    ///
    /// file_info has Animations** at +0x7C → ptr array → first animation struct.
    fn animation_struct_offset(&self) -> Result<usize> {
        let fi = &self.chunk_data;
        // Animations** → array of pointers
        let arr = read_ptr(fi, file_info::ANIMATIONS_PTR).ok_or(Error::NoAnimations)?;
        // First animation pointer
        read_ptr(fi, arr).ok_or(Error::NoAnimations)
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use std::eprintln;

    use test_utils::prelude::*;

    use super::*;

    /// Max files per source to keep tests fast.
    const MAX_FILES: usize = 50;

    // -----------------------------------------------------------------------
    // HW1 — extract .uax from ERA archives, roundtrip bytes
    // -----------------------------------------------------------------------

    #[test]
    fn test_hw1_era_roundtrip() {
        let game_dir = match load_game_dir("HW1_GAME_DIR") {
            Some(d) => d,
            None => return,
        };

        let era_paths = find_files_flat(&game_dir, "era");
        if era_paths.is_empty() {
            eprintln!("No .era files — skipping");
            return;
        }

        let mut tested = 0usize;
        let mut errors = std::vec::Vec::new();

        for era_path in &era_paths {
            let mut archive = match open_era(era_path) {
                Ok(a) => a,
                Err(_) => continue,
            };
            let entries = find_entries_in_era(&archive, ".uax");
            for (idx, filename) in &entries {
                if tested >= MAX_FILES {
                    break;
                }
                let Ok(data) = archive.read_entry(*idx) else {
                    continue;
                };
                match UaxFile::from_bytes(&data) {
                    Ok(uax) => {
                        let written = uax.to_bytes();
                        if data != written {
                            errors.push(std::format!(
                                "{filename}: byte mismatch (orig={}, written={})",
                                data.len(),
                                written.len()
                            ));
                        }
                        tested += 1;
                    }
                    Err(e) => errors.push(std::format!("{filename}: {e:?}")),
                }
            }
            if tested >= MAX_FILES {
                break;
            }
        }

        eprintln!("HW1 ERA roundtrip: {tested} files tested");
        assert!(tested > 0, "No HW1 UAX files found");
        assert!(
            errors.is_empty(),
            "Roundtrip failures:\n{}",
            errors.join("\n")
        );
    }

    // -----------------------------------------------------------------------
    // HW2 — read loose .uax files, roundtrip bytes
    // -----------------------------------------------------------------------

    #[test]
    fn test_hw2_loose_roundtrip() {
        let game_dir = match load_game_dir("HW2_GAME_DIR") {
            Some(d) => d,
            None => return,
        };

        let uax_files = find_files_by_ext(&game_dir, "uax");
        if uax_files.is_empty() {
            eprintln!("No .uax files — skipping");
            return;
        }

        let mut tested = 0usize;
        let mut errors = std::vec::Vec::new();

        for path in uax_files.iter().take(MAX_FILES) {
            let Ok(data) = std::fs::read(path) else {
                continue;
            };
            match UaxFile::from_bytes(&data) {
                Ok(uax) => {
                    let written = uax.to_bytes();
                    if data != written {
                        errors.push(std::format!(
                            "{}: byte mismatch (orig={}, written={})",
                            path.display(),
                            data.len(),
                            written.len()
                        ));
                    }
                    tested += 1;
                }
                Err(e) => errors.push(std::format!("{}: {e:?}", path.display())),
            }
        }

        eprintln!("HW2 loose roundtrip: {tested} files tested");
        assert!(tested > 0, "No HW2 UAX files tested");
        assert!(
            errors.is_empty(),
            "Roundtrip failures:\n{}",
            errors.join("\n")
        );
    }

    // -----------------------------------------------------------------------
    // Modify duration — uses first available file from either source
    // -----------------------------------------------------------------------

    #[test]
    fn test_modify_duration() {
        // Try HW2 loose files first, then HW1 ERA
        let data = if let Some(dir) = load_game_dir("HW2_GAME_DIR") {
            let files = find_files_by_ext(&dir, "uax");
            files.first().and_then(|p| std::fs::read(p).ok())
        } else {
            None
        }
        .or_else(|| {
            let dir = load_game_dir("HW1_GAME_DIR")?;
            let eras = find_files_flat(&dir, "era");
            for era_path in &eras {
                let mut archive = open_era(era_path).ok()?;
                let entries = find_entries_in_era(&archive, ".uax");
                for (idx, _) in &entries {
                    if let Ok(d) = archive.read_entry(*idx) {
                        return Some(d);
                    }
                }
            }
            None
        });

        let Some(data) = data else {
            eprintln!("No UAX files available — skipping");
            return;
        };

        let mut uax = UaxFile::from_bytes(&data).expect("Failed to parse UAX");

        let original_duration = uax.duration().unwrap();
        let new_duration = original_duration * 2.0;
        uax.set_duration(new_duration).unwrap();
        assert!((uax.duration().unwrap() - new_duration).abs() < 0.001);

        let written = uax.to_bytes();
        let reloaded = UaxFile::from_bytes(&written).expect("Failed to re-read UAX");
        assert!((reloaded.duration().unwrap() - new_duration).abs() < 0.001);

        eprintln!("Modify duration: {original_duration:.4} → {new_duration:.4} ✓");
    }
}
