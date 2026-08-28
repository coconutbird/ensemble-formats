//! Zero-copy parser for the FXB0 container format used by Halo Wars 1:
//! Definitive Edition compiled shaders.
//!
//! An `.bin` (FXB) file wraps one or more DXBC blobs (Vertex Shader, Pixel
//! Shader) inside a proprietary Ensemble header, along with per-entry
//! annotation data (constant buffer layouts).
//!
//! # Format
//!
//! ```text
//! Header (16 bytes):
//!   [0x00] magic      "fxb0" (4 bytes)
//!   [0x04] version    u32 LE (observed: 1)
//!   [0x08] unknown    u32 LE
//!   [0x0C] name_len   u32 LE (shader name field width in bytes)
//!
//! First entry (at offset 0x10):
//!   name       [name_len bytes, right-padded with spaces]
//!   dxbc_size  u32 LE
//!   dxbc_blob  [dxbc_size bytes]
//!   annotation [variable — constant buffer layouts, etc.]
//!
//! Subsequent entries:
//!   name_len   u32 LE (redundant, matches header)
//!   name       [name_len bytes]
//!   dxbc_size  u32 LE
//!   dxbc_blob  [dxbc_size bytes]
//!   annotation [variable]
//! ```
//!
//! This crate uses [`d3dasm`] to parse the embedded DXBC containers.
//!
//! # Quick start
//!
//! ```ignore
//! let data = std::fs::read("parametricshader0.bin")?;
//! let fxb = fxb::parse(&data)?;
//!
//! for entry in &fxb.entries {
//!     println!("{}: {} bytes", entry.name, entry.dxbc_size);
//! }
//! ```

#![no_std]
extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;
use d3dasm::Shader;
use d3dasm::dxbc;
use nostdio::{Cursor, ReadLe, Seek, SeekFrom, Write, WriteLe};

const FXB0_MAGIC: &[u8; 4] = b"fxb0";

/// Minimum header size (magic + version + unknown + `name_len`).
const FXB0_HEADER_SIZE: usize = 16;

/// Errors that can occur when parsing an FXB file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The input is shorter than the minimum header.
    TooShort { len: usize },
    /// The first four bytes are not `fxb0`.
    BadMagic { found: [u8; 4] },
    /// A required field could not be read.
    Truncated,
    /// A DXBC blob's declared size exceeds the file bounds.
    BlobOutOfBounds {
        entry: usize,
        offset: usize,
        size: usize,
    },
    /// A value cannot be represented by the on-disk format.
    SizeOverflow(&'static str),
    /// A cursor read or write failed.
    Cursor,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort { len } => {
                write!(f, "input too short ({len} bytes, need {FXB0_HEADER_SIZE})")
            }
            Self::BadMagic { found } => write!(
                f,
                "bad magic: expected fxb0, got {:?}",
                core::str::from_utf8(found).unwrap_or("????")
            ),
            Self::Truncated => f.write_str("truncated FXB data"),
            Self::BlobOutOfBounds {
                entry,
                offset,
                size,
            } => write!(
                f,
                "entry {entry}: DXBC blob at 0x{offset:X} size {size} exceeds file bounds"
            ),
            Self::SizeOverflow(field) => write!(f, "{field} is too large for the FXB format"),
            Self::Cursor => f.write_str("cursor read or write failed"),
        }
    }
}

impl From<nostdio::IoError> for Error {
    fn from(_: nostdio::IoError) -> Self {
        Self::Cursor
    }
}

/// A single shader entry inside an FXB container.
#[derive(Debug)]
pub struct FxbEntry<'a> {
    /// Shader stage name (e.g. "`VertexShader`", "`PixelShader`").
    pub name: String,
    /// Raw name field bytes as stored in the file (for round-trip fidelity).
    /// For entry 0 this is `header.name_len` bytes; for subsequent entries
    /// it is `local_name_len` bytes.
    pub raw_name: &'a [u8],
    /// Per-entry name length field (only meaningful for entries after the first;
    /// for entry 0 this equals the header `name_len`).
    pub local_name_len: u32,
    /// Byte offset of the DXBC blob within the original file.
    pub dxbc_offset: usize,
    /// Size of the DXBC blob in bytes.
    pub dxbc_size: usize,
    /// Raw DXBC blob bytes.
    pub dxbc_data: &'a [u8],
    /// Raw annotation data between this DXBC blob and the next entry header.
    pub annotation: &'a [u8],
    /// Parsed DXBC shader (if valid).
    pub shader: Option<Shader<'a>>,
}

/// A parsed FXB0 container.
#[derive(Debug)]
pub struct FxbFile<'a> {
    /// Format version (observed: 1).
    pub version: u32,
    /// Unknown header field at offset 0x08.
    pub unknown_08: u32,
    /// Width of the shader name field in bytes.
    pub name_len: u32,
    /// Shader entries.
    pub entries: Vec<FxbEntry<'a>>,
}

impl FxbFile<'_> {
    /// Serialize this FXB file back to bytes.
    ///
    /// The output is byte-identical to the original input when the
    /// [`FxbFile`] was produced by [`parse`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::SizeOverflow`] if an entry size cannot be represented
    /// by the 32-bit on-disk field, or [`Error::Cursor`] if writing fails.
    pub fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut out = Vec::new();

        // Header.
        out.write_all(FXB0_MAGIC)?;
        out.write_u32_le(self.version)?;
        out.write_u32_le(self.unknown_08)?;
        out.write_u32_le(self.name_len)?;

        for (idx, entry) in self.entries.iter().enumerate() {
            if idx > 0 {
                // Subsequent entries: write the local_name_len prefix.
                out.write_u32_le(entry.local_name_len)?;
            }

            // Name field (raw bytes for round-trip fidelity).
            out.write_all(entry.raw_name)?;

            // DXBC size + blob.
            let dxbc_size = u32::try_from(entry.dxbc_size)
                .map_err(|_| Error::SizeOverflow("DXBC blob size"))?;
            out.write_u32_le(dxbc_size)?;
            out.write_all(entry.dxbc_data)?;

            // Annotation.
            out.write_all(entry.annotation)?;
        }

        Ok(out)
    }
}

/// Check whether `data` starts with the FXB0 magic bytes.
#[must_use]
pub fn is_fxb(data: &[u8]) -> bool {
    data.len() >= 4 && &data[0..4] == FXB0_MAGIC
}

/// Parse `data` as an FXB0 file.
///
/// # Errors
///
/// Returns an error if the header is invalid or truncated, a declared DXBC
/// blob lies outside `data`, or an on-disk size cannot fit the current target.
pub fn parse(data: &[u8]) -> Result<FxbFile<'_>, Error> {
    if data.len() < FXB0_HEADER_SIZE {
        return Err(Error::TooShort { len: data.len() });
    }
    if &data[0..4] != FXB0_MAGIC {
        let mut found = [0u8; 4];
        found.copy_from_slice(&data[0..4]);
        return Err(Error::BadMagic { found });
    }

    let mut c = Cursor::new(data);
    let e = |_| Error::Truncated;

    c.seek(SeekFrom::Start(4)).map_err(e)?;
    let version = c.read_u32_le().map_err(e)?;
    let unknown_08 = c.read_u32_le().map_err(e)?;
    let name_len = c.read_u32_le().map_err(e)?;

    // Collect all DXBC blob positions by scanning for the magic.
    let dxbc_positions = find_dxbc_offsets(data);

    let nlen = usize::try_from(name_len).map_err(|_| Error::SizeOverflow("name length"))?;
    let mut entries = Vec::with_capacity(dxbc_positions.len());

    for index in 0..dxbc_positions.len() {
        entries.push(parse_entry(data, &dxbc_positions, index, nlen, name_len)?);
    }

    Ok(FxbFile {
        version,
        unknown_08,
        name_len,
        entries,
    })
}

fn parse_entry<'a>(
    data: &'a [u8],
    dxbc_positions: &[usize],
    index: usize,
    default_name_len: usize,
    header_name_len: u32,
) -> Result<FxbEntry<'a>, Error> {
    let dxbc_offset = dxbc_positions[index];
    let dxbc_size = checked_dxbc_size(data, dxbc_offset, index)?;
    let blob_end = dxbc_offset
        .checked_add(dxbc_size)
        .ok_or(Error::SizeOverflow("DXBC blob end"))?;
    let dxbc_data = data
        .get(dxbc_offset..blob_end)
        .ok_or(Error::BlobOutOfBounds {
            entry: index,
            offset: dxbc_offset,
            size: dxbc_size,
        })?;

    let (name, raw_name, local_name_len) =
        read_entry_name(data, dxbc_offset, index, default_name_len, header_name_len)?;
    let annotation_end = if let Some(&next_offset) = dxbc_positions.get(index + 1) {
        entry_header_start(data, next_offset, default_name_len)?.max(blob_end)
    } else {
        data.len()
    };
    let annotation = data.get(blob_end..annotation_end).ok_or(Error::Truncated)?;

    let shader = dxbc::scan_dxbc(dxbc_data)
        .into_iter()
        .next()
        .map(|mut container| {
            container.offset_in_file += dxbc_offset;
            Shader::from_container(container)
        });

    Ok(FxbEntry {
        name,
        raw_name,
        local_name_len,
        dxbc_offset,
        dxbc_size,
        dxbc_data,
        annotation,
        shader,
    })
}

fn checked_dxbc_size(data: &[u8], dxbc_offset: usize, entry: usize) -> Result<usize, Error> {
    let size_offset = dxbc_offset
        .checked_add(24)
        .ok_or(Error::SizeOverflow("DXBC size offset"))?;
    let size =
        usize::try_from(read_u32_at(data, size_offset)?).map_err(|_| Error::BlobOutOfBounds {
            entry,
            offset: dxbc_offset,
            size: usize::MAX,
        })?;
    let blob_end = dxbc_offset
        .checked_add(size)
        .ok_or(Error::SizeOverflow("DXBC blob end"))?;
    if blob_end > data.len() {
        return Err(Error::BlobOutOfBounds {
            entry,
            offset: dxbc_offset,
            size,
        });
    }
    Ok(size)
}

fn read_entry_name(
    data: &[u8],
    dxbc_offset: usize,
    index: usize,
    default_name_len: usize,
    header_name_len: u32,
) -> Result<(String, &[u8], u32), Error> {
    let name_len = if index == 0 {
        default_name_len
    } else {
        find_local_name_len(data, dxbc_offset, default_name_len)?
    };
    let dxbc_size_offset = dxbc_offset.checked_sub(4).ok_or(Error::Truncated)?;
    let name_start = dxbc_size_offset
        .checked_sub(name_len)
        .ok_or(Error::Truncated)?;
    let raw_name = data
        .get(name_start..dxbc_size_offset)
        .ok_or(Error::Truncated)?;
    let name = core::str::from_utf8(raw_name).unwrap_or("?").trim().into();
    let local_name_len = if index == 0 {
        header_name_len
    } else {
        u32::try_from(name_len).map_err(|_| Error::SizeOverflow("entry name length"))?
    };
    Ok((name, raw_name, local_name_len))
}

fn find_local_name_len(
    data: &[u8],
    dxbc_offset: usize,
    default_name_len: usize,
) -> Result<usize, Error> {
    let dxbc_size_offset = dxbc_offset.checked_sub(4).ok_or(Error::Truncated)?;
    let max_probe = 64.min(dxbc_size_offset.saturating_sub(4));
    for candidate in 1..=max_probe {
        let Some(length_offset) = dxbc_size_offset
            .checked_sub(candidate)
            .and_then(|offset| offset.checked_sub(4))
        else {
            break;
        };
        let value = usize::try_from(read_u32_at(data, length_offset)?)
            .map_err(|_| Error::SizeOverflow("entry name length"))?;
        if value == candidate {
            return Ok(candidate);
        }
    }
    Ok(default_name_len)
}

fn entry_header_start(
    data: &[u8],
    dxbc_offset: usize,
    default_name_len: usize,
) -> Result<usize, Error> {
    let name_len = find_local_name_len(data, dxbc_offset, default_name_len)?;
    dxbc_offset
        .checked_sub(4)
        .and_then(|offset| offset.checked_sub(name_len))
        .and_then(|offset| offset.checked_sub(4))
        .ok_or(Error::Truncated)
}

fn read_u32_at(data: &[u8], offset: usize) -> Result<u32, Error> {
    let mut cursor = Cursor::new(data.get(offset..).ok_or(Error::Truncated)?);
    cursor.read_u32_le().map_err(Error::from)
}

/// Scan `data` for all occurrences of the DXBC magic and return their byte offsets.
fn find_dxbc_offsets(data: &[u8]) -> Vec<usize> {
    let magic = b"DXBC";
    let mut offsets = Vec::new();
    let mut pos = 0;
    while let Some(window) = data.get(pos..).and_then(|remaining| remaining.get(..4)) {
        if window == magic {
            offsets.push(pos);
            // Skip past this DXBC blob using its declared size if possible.
            if let Some(size_offset) = pos.checked_add(24)
                && let Ok(size) = read_u32_at(data, size_offset).and_then(|value| {
                    usize::try_from(value).map_err(|_| Error::SizeOverflow("DXBC blob size"))
                })
                && size > 4
                && let Some(next) = pos.checked_add(size)
            {
                pos = next;
                continue;
            }
        }
        let Some(next) = pos.checked_add(1) else {
            break;
        };
        pos = next;
    }
    offsets
}

#[cfg(test)]
mod tests {
    extern crate std;
    use std::eprintln;

    use test_utils::prelude::*;

    use super::*;

    /// Max files per source to keep tests fast.
    const MAX_FILES: usize = 500;

    // -----------------------------------------------------------------------
    // HW1 — extract .bin (FXB) shaders from ERA archives, roundtrip bytes
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
            let entries = find_entries_in_era(&archive, ".bin");
            for (idx, filename) in &entries {
                if tested >= MAX_FILES {
                    break;
                }
                // Only test files in shader directories.
                let lower = filename.to_lowercase();
                if !lower.contains("shader") {
                    continue;
                }
                let Ok(data) = archive.read_entry(*idx) else {
                    continue;
                };
                // Skip non-FXB files.
                if !is_fxb(&data) {
                    continue;
                }
                match parse(&data) {
                    Ok(fxb) => {
                        let written = fxb.to_bytes().expect("parsed FXB should serialize");
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

        eprintln!("HW1 ERA FXB roundtrip: {tested} files tested");
        assert!(tested > 0, "No HW1 FXB files found");
        assert!(
            errors.is_empty(),
            "Roundtrip failures:\n{}",
            errors.join("\n")
        );
    }
}
