//! UAX file container for reading and writing.
//!
//! This module provides a container that preserves the raw Granny data
//! while exposing parsed animation metadata for inspection and modification.
//! The chunk data IS `file_info` directly — no separate header.

use alloc::string::String;
use alloc::vec::Vec;

use crate::types::{animation, file_info, read_cstring, read_f32_le, read_i32_le, read_u64_le};
use crate::{Error, Result, UAX_CHUNK_ID, UAX_FILE_ID, UAX_FROM_FILENAME};
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
    ///
    /// # Errors
    ///
    /// Returns an error if the ECF container is invalid, the animation chunk
    /// is absent or truncated, the file ID or `FromFileName` marker is wrong,
    /// or the first animation pointer chain is malformed.
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

        let result = Self {
            ecf_header,
            chunk_header,
            chunk_data,
        };
        result.validate_from_file_name()?;
        result.animation_struct_offset()?;
        Ok(result)
    }

    /// Write the UAX file to bytes, preserving original ECF structure.
    ///
    /// # Errors
    ///
    /// Returns [`Error::SizeOverflow`] if the chunk size or a stored offset
    /// cannot be represented by the ECF format or current platform.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.ecf_header.to_bytes());

        let mut chunk_header = self.chunk_header.clone();
        chunk_header.adler32 = ecf::adler32(&self.chunk_data);
        chunk_header.size =
            u32::try_from(self.chunk_data.len()).map_err(|_| Error::SizeOverflow("chunk size"))?;
        out.extend_from_slice(&chunk_header.to_bytes());

        let chunk_offset = usize::try_from(chunk_header.offset)
            .map_err(|_| Error::SizeOverflow("chunk offset"))?;
        if out.len() < chunk_offset {
            out.resize(chunk_offset, 0);
        }
        out.extend_from_slice(&self.chunk_data);

        let target_size = usize::try_from(self.ecf_header.file_size)
            .map_err(|_| Error::SizeOverflow("file size"))?;
        if out.len() < target_size {
            out.resize(target_size, 0);
        }

        Ok(out)
    }

    /// Get the raw chunk data (for debugging/inspection).
    #[must_use]
    pub fn chunk_data(&self) -> &[u8] {
        &self.chunk_data
    }

    /// Get the animation count.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnexpectedEof`] if the count field is truncated.
    pub fn animation_count(&self) -> Result<i32> {
        read_i32_le(&self.chunk_data, file_info::ANIMATION_COUNT).ok_or(Error::UnexpectedEof)
    }

    /// Get the track group count from `file_info`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnexpectedEof`] if the count field is truncated.
    pub fn track_group_count(&self) -> Result<i32> {
        read_i32_le(&self.chunk_data, file_info::TRACK_GROUP_COUNT).ok_or(Error::UnexpectedEof)
    }

    /// Get animation name.
    ///
    /// # Errors
    ///
    /// Returns an error if the animation pointer chain is absent or invalid.
    pub fn animation_name(&self) -> Result<Option<String>> {
        let animation_offset = self.animation_struct_offset()?;
        let field = self.field_offset(
            animation_offset,
            animation::NAME_PTR,
            "animation name pointer",
        )?;
        self.optional_pointer(field, "animation name")?
            .map(|pointer| self.string_at(pointer))
            .transpose()
    }

    /// Get animation duration in seconds.
    ///
    /// # Errors
    ///
    /// Returns an error if the animation pointer chain or duration field is
    /// absent or truncated.
    pub fn duration(&self) -> Result<f32> {
        self.animation_f32(animation::DURATION, "animation duration")
    }

    /// Set animation duration in seconds.
    ///
    /// # Errors
    ///
    /// Returns an error if the animation pointer chain or duration field is
    /// absent or truncated.
    pub fn set_duration(&mut self, duration: f32) -> Result<()> {
        let animation_offset = self.animation_struct_offset()?;
        let position =
            self.field_offset(animation_offset, animation::DURATION, "animation duration")?;
        let end = position
            .checked_add(4)
            .ok_or(Error::SizeOverflow("animation duration"))?;
        let chunk_size = self.chunk_data.len();
        self.chunk_data
            .get_mut(position..end)
            .ok_or(Error::InvalidRange {
                field: "animation duration",
                offset: position,
                size: 4,
                chunk_size,
            })?
            .copy_from_slice(&duration.to_bits().to_le_bytes());
        Ok(())
    }

    /// Get animation time step between keyframes.
    ///
    /// # Errors
    ///
    /// Returns an error if the animation pointer chain or time-step field is
    /// absent or truncated.
    pub fn time_step(&self) -> Result<f32> {
        self.animation_f32(animation::TIME_STEP, "animation time step")
    }

    /// Get animation oversampling factor.
    ///
    /// # Errors
    ///
    /// Returns an error if the animation pointer chain or oversampling field
    /// is absent or truncated.
    pub fn oversampling(&self) -> Result<f32> {
        self.animation_f32(animation::OVERSAMPLING, "animation oversampling")
    }

    /// Resolve the offset of the first animation struct within `chunk_data`.
    ///
    /// `file_info` has Animations** at +0x7C → ptr array → first animation struct.
    fn animation_struct_offset(&self) -> Result<usize> {
        let signed_count = self.animation_count()?;
        if signed_count < 0 {
            return Err(Error::InvalidCount("animation count", signed_count));
        }
        if signed_count == 0 {
            return Err(Error::NoAnimations);
        }
        let array = self.required_pointer(file_info::ANIMATIONS_PTR, "animation pointer array")?;
        self.range(array, 8, "first animation pointer")?;
        let animation = self.required_pointer(array, "first animation")?;
        self.range(animation, animation::SIZE, "animation")?;
        Ok(animation)
    }

    fn validate_from_file_name(&self) -> Result<()> {
        let pointer = self.required_pointer(file_info::FROM_FILE_NAME_PTR, "FromFileName")?;
        let value = self.string_at(pointer)?;
        if !value.eq_ignore_ascii_case(UAX_FROM_FILENAME) {
            return Err(Error::InvalidFromFileName(value));
        }
        Ok(())
    }

    fn animation_f32(&self, relative: usize, field: &'static str) -> Result<f32> {
        let animation = self.animation_struct_offset()?;
        let offset = self.field_offset(animation, relative, field)?;
        read_f32_le(&self.chunk_data, offset).ok_or(Error::InvalidRange {
            field,
            offset,
            size: 4,
            chunk_size: self.chunk_data.len(),
        })
    }

    fn field_offset(&self, base: usize, relative: usize, field: &'static str) -> Result<usize> {
        let offset = base
            .checked_add(relative)
            .ok_or(Error::SizeOverflow(field))?;
        if offset > self.chunk_data.len() {
            return Err(Error::InvalidRange {
                field,
                offset,
                size: 0,
                chunk_size: self.chunk_data.len(),
            });
        }
        Ok(offset)
    }

    fn range(&self, offset: usize, size: usize, field: &'static str) -> Result<&[u8]> {
        let end = offset.checked_add(size).ok_or(Error::SizeOverflow(field))?;
        self.chunk_data.get(offset..end).ok_or(Error::InvalidRange {
            field,
            offset,
            size,
            chunk_size: self.chunk_data.len(),
        })
    }

    fn optional_pointer(&self, offset: usize, field: &'static str) -> Result<Option<usize>> {
        let raw = read_u64_le(&self.chunk_data, offset).ok_or(Error::InvalidRange {
            field,
            offset,
            size: 8,
            chunk_size: self.chunk_data.len(),
        })?;
        if raw == 0 {
            return Ok(None);
        }
        let pointer = usize::try_from(raw)
            .map_err(|_| Error::InvalidPointerOffset(raw, self.chunk_data.len()))?;
        if pointer >= self.chunk_data.len() {
            return Err(Error::InvalidPointerOffset(raw, self.chunk_data.len()));
        }
        Ok(Some(pointer))
    }

    fn required_pointer(&self, offset: usize, field: &'static str) -> Result<usize> {
        self.optional_pointer(offset, field)?
            .ok_or(Error::NullPointer(field))
    }

    fn string_at(&self, offset: usize) -> Result<String> {
        read_cstring(&self.chunk_data, offset).ok_or(Error::StringReadError(
            u64::try_from(offset).unwrap_or(u64::MAX),
        ))
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
        let Some(game_dir) = load_game_dir("HW1_GAME_DIR") else {
            return;
        };

        let era_paths = find_files_flat(&game_dir, "era");
        if era_paths.is_empty() {
            eprintln!("No .era files — skipping");
            return;
        }

        let mut tested = 0usize;
        let mut errors = std::vec::Vec::new();

        for era_path in &era_paths {
            let Ok(mut archive) = open_era(era_path) else {
                continue;
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
                        let written = uax.to_bytes().expect("parsed UAX should serialize");
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
        let Some(game_dir) = load_game_dir("HW2_GAME_DIR") else {
            return;
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
                    let written = uax.to_bytes().expect("parsed UAX should serialize");
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

        let written = uax.to_bytes().expect("modified UAX should serialize");
        let reloaded = UaxFile::from_bytes(&written).expect("Failed to re-read UAX");
        assert!((reloaded.duration().unwrap() - new_duration).abs() < 0.001);

        eprintln!("Modify duration: {original_duration:.4} → {new_duration:.4} ✓");
    }
}
