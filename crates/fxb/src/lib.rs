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
use nostdio::{ReadLe, Seek, SeekFrom, SliceCursor};

const FXB0_MAGIC: &[u8; 4] = b"fxb0";

/// Minimum header size (magic + version + unknown + name_len).
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
        }
    }
}

/// A single shader entry inside an FXB container.
#[derive(Debug)]
pub struct FxbEntry<'a> {
    /// Shader stage name (e.g. "VertexShader", "PixelShader").
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

impl<'a> FxbFile<'a> {
    /// Serialize this FXB file back to bytes.
    ///
    /// The output is byte-identical to the original input when the
    /// [`FxbFile`] was produced by [`parse`].
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();

        // Header.
        out.extend_from_slice(FXB0_MAGIC);
        out.extend_from_slice(&self.version.to_le_bytes());
        out.extend_from_slice(&self.unknown_08.to_le_bytes());
        out.extend_from_slice(&self.name_len.to_le_bytes());

        for (idx, entry) in self.entries.iter().enumerate() {
            if idx > 0 {
                // Subsequent entries: write the local_name_len prefix.
                out.extend_from_slice(&entry.local_name_len.to_le_bytes());
            }

            // Name field (raw bytes for round-trip fidelity).
            out.extend_from_slice(entry.raw_name);

            // DXBC size + blob.
            out.extend_from_slice(&(entry.dxbc_size as u32).to_le_bytes());
            out.extend_from_slice(entry.dxbc_data);

            // Annotation.
            out.extend_from_slice(entry.annotation);
        }

        out
    }
}

/// Check whether `data` starts with the FXB0 magic bytes.
pub fn is_fxb(data: &[u8]) -> bool {
    data.len() >= 4 && &data[0..4] == FXB0_MAGIC
}

/// Parse `data` as an FXB0 file.
pub fn parse(data: &[u8]) -> Result<FxbFile<'_>, Error> {
    if data.len() < FXB0_HEADER_SIZE {
        return Err(Error::TooShort { len: data.len() });
    }
    if &data[0..4] != FXB0_MAGIC {
        let mut found = [0u8; 4];
        found.copy_from_slice(&data[0..4]);
        return Err(Error::BadMagic { found });
    }

    let mut c = SliceCursor::new(data);
    let e = |_| Error::Truncated;

    c.seek(SeekFrom::Start(4)).map_err(e)?;
    let version = c.read_u32_le().map_err(e)?;
    let unknown_08 = c.read_u32_le().map_err(e)?;
    let name_len = c.read_u32_le().map_err(e)?;

    // Collect all DXBC blob positions by scanning for the magic.
    let dxbc_positions = find_dxbc_offsets(data);

    let nlen = name_len as usize;
    let mut entries = Vec::with_capacity(dxbc_positions.len());

    for (idx, &dxbc_off) in dxbc_positions.iter().enumerate() {
        // Read the DXBC total size from the DXBC header (offset +24 in DXBC).
        if dxbc_off + 28 > data.len() {
            return Err(Error::BlobOutOfBounds {
                entry: idx,
                offset: dxbc_off,
                size: 0,
            });
        }
        let dxbc_size =
            u32::from_le_bytes(data[dxbc_off + 24..dxbc_off + 28].try_into().unwrap()) as usize;
        if dxbc_off + dxbc_size > data.len() {
            return Err(Error::BlobOutOfBounds {
                entry: idx,
                offset: dxbc_off,
                size: dxbc_size,
            });
        }

        // Entry 0: name is padded to header `name_len`, no preceding length field.
        // Subsequent entries: a local u32 `local_name_len` precedes the name,
        // giving the actual byte length of the name string.
        let (name, raw_name, local_name_len) = if idx == 0 {
            // Entry 0: name field is right after the 16-byte header.
            let name_start = dxbc_off - nlen - 4; // nlen name + 4 dxbc_size
            let raw = &data[name_start..name_start + nlen];
            let s: String = core::str::from_utf8(raw).unwrap_or("?").trim().into();
            (s, raw, name_len)
        } else {
            // Subsequent entries: work backwards from DXBC to find the
            // local_name_len. The layout is: [local_name_len:u32] [name:local_name_len] [dxbc_size:u32] [DXBC...]
            // We know dxbc_size is at dxbc_off - 4. Before that is the name.
            // We need to find the local_name_len u32 that precedes the name.
            // Try reading candidate lengths and verify consistency.
            let dxbc_size_off = dxbc_off - 4;
            let mut found_len = nlen; // fallback
            // The local name can be longer than the header name_len (e.g.
            // "GeometryShader" = 14 chars with header name_len = 12), so
            // probe a generous range.
            let max_probe = 64.min(dxbc_size_off.saturating_sub(4));
            for candidate in 1..=max_probe {
                if dxbc_size_off < candidate + 4 {
                    break;
                }
                let len_off = dxbc_size_off - candidate - 4;
                let val =
                    u32::from_le_bytes(data[len_off..len_off + 4].try_into().unwrap()) as usize;
                if val == candidate {
                    found_len = candidate;
                    break;
                }
            }
            let local_nl = found_len;
            let name_start = dxbc_size_off - local_nl;
            let raw = &data[name_start..name_start + local_nl];
            let s: String = core::str::from_utf8(raw).unwrap_or("?").trim().into();
            (s, raw, local_nl as u32)
        };

        // DXBC blob bytes.
        let dxbc_data = &data[dxbc_off..dxbc_off + dxbc_size];

        // Annotation: bytes from end of DXBC blob to start of next entry header.
        let blob_end = dxbc_off + dxbc_size;
        let ann_end = if let Some(&next_off) = dxbc_positions.get(idx + 1) {
            // Next entry header: [local_name_len:4] [name:L] [dxbc_size:4] [DXBC...]
            // We need to find where that header starts. Use the same probing.
            let next_dxbc_size_off = next_off - 4;
            let mut next_local = nlen;
            let max_probe2 = 64.min(next_dxbc_size_off.saturating_sub(4));
            for candidate in 1..=max_probe2 {
                if next_dxbc_size_off < candidate + 4 {
                    break;
                }
                let len_off = next_dxbc_size_off - candidate - 4;
                let val =
                    u32::from_le_bytes(data[len_off..len_off + 4].try_into().unwrap()) as usize;
                if val == candidate {
                    next_local = candidate;
                    break;
                }
            }
            let header_start = next_off - 4 - next_local - 4;
            header_start.max(blob_end)
        } else {
            data.len()
        };
        let annotation = &data[blob_end..ann_end];

        // Parse the DXBC blob.
        let shader = {
            let containers = dxbc::scan_dxbc(dxbc_data);
            containers.into_iter().next().map(|mut container| {
                container.offset_in_file += dxbc_off;
                Shader::from_container(container)
            })
        };

        entries.push(FxbEntry {
            name,
            raw_name,
            local_name_len,
            dxbc_offset: dxbc_off,
            dxbc_size,
            dxbc_data,
            annotation,
            shader,
        });
    }

    Ok(FxbFile {
        version,
        unknown_08,
        name_len,
        entries,
    })
}

/// Scan `data` for all occurrences of the DXBC magic and return their byte offsets.
fn find_dxbc_offsets(data: &[u8]) -> Vec<usize> {
    let magic = b"DXBC";
    let mut offsets = Vec::new();
    let mut pos = 0;
    while pos + 4 <= data.len() {
        if &data[pos..pos + 4] == magic {
            offsets.push(pos);
            // Skip past this DXBC blob using its declared size if possible.
            if pos + 28 <= data.len() {
                let size =
                    u32::from_le_bytes(data[pos + 24..pos + 28].try_into().unwrap()) as usize;
                if size > 4 {
                    pos += size;
                    continue;
                }
            }
        }
        pos += 1;
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
                        let written = fxb.to_bytes();
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
