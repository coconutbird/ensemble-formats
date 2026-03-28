//! Parser for the UFXS container format used by Halo Wars 2 compiled shaders.
//!
//! A `.ufx` file wraps one or more DXBC blobs (Root Signature, Vertex Shader,
//! Pixel Shader) inside a proprietary Ensemble header. The pixel shader offset
//! is repeated across four quality-level slots that may point to the same blob.
//!
//! This crate uses [`d3dasm`] to parse the embedded DXBC containers.

#![no_std]
extern crate alloc;

pub mod cb_infer;

use alloc::vec::Vec;
use d3dasm::Shader;
use d3dasm::dxbc;
use nostdio::{ReadLe, Seek, SeekFrom, SliceCursor};

const UFXS_MAGIC: &[u8; 4] = b"UFXS";

/// Minimum header size to read all fixed fields.
const UFXS_MIN_HEADER: usize = 0x48;

/// A parsed UFXS container with metadata and structured shader stages.
#[derive(Debug)]
pub struct UfxFile<'a> {
    /// UFXS format version (expected: 9).
    pub version: u32,
    /// Permutation hash — matches the hash stored in UGX material data.
    pub hash: u32,
    /// Root Signature (RTS0 chunk inside a DXBC wrapper).
    pub root_signature: Option<Shader<'a>>,
    /// Vertex Shader.
    pub vertex_shader: Option<Shader<'a>>,
    /// Pixel Shader(s) — deduplicated across quality slots.
    pub pixel_shaders: Vec<Shader<'a>>,
    /// Raw header field: four PS quality-slot offsets.
    pub ps_offsets: [u32; 4],
}

/// Check whether `data` starts with the UFXS magic.
pub fn is_ufx(data: &[u8]) -> bool {
    data.len() >= 4 && &data[0..4] == UFXS_MAGIC
}

/// Try to parse `data` as a UFXS file.
///
/// Returns `None` if the magic bytes don't match or the header is truncated.
pub fn parse(data: &[u8]) -> Option<UfxFile<'_>> {
    if data.len() < UFXS_MIN_HEADER || &data[0..4] != UFXS_MAGIC {
        return None;
    }

    let mut c = SliceCursor::new(data);

    c.seek(SeekFrom::Start(0x04)).ok()?;
    let version = c.read_u32_le().ok()?;
    let hash = c.read_u32_le().ok()?;

    c.seek(SeekFrom::Start(0x10)).ok()?;
    let rts0_offset = c.read_u32_le().ok()?;
    let rts0_size = c.read_u32_le().ok()?;
    let vs_offset = c.read_u32_le().ok()?;
    let vs_size = c.read_u32_le().ok()?;

    // Four PS quality-level slots (each stored as u32 offset + u32 pad=0).
    c.seek(SeekFrom::Start(0x20)).ok()?;
    let ps_off_0 = c.read_u32_le().ok()?;
    c.seek(SeekFrom::Start(0x28)).ok()?;
    let ps_off_1 = c.read_u32_le().ok()?;
    c.seek(SeekFrom::Start(0x30)).ok()?;
    let ps_off_2 = c.read_u32_le().ok()?;
    c.seek(SeekFrom::Start(0x38)).ok()?;
    let ps_off_3 = c.read_u32_le().ok()?;
    let ps_size = c.read_u32_le().ok()?;
    let ps_offsets = [ps_off_0, ps_off_1, ps_off_2, ps_off_3];

    let root_signature = extract_shader(data, rts0_offset as usize, rts0_size as usize);
    let vertex_shader = extract_shader(data, vs_offset as usize, vs_size as usize);

    // Deduplicate PS slots — most files have all 4 pointing to the same blob.
    let mut pixel_shaders = Vec::new();
    let mut seen_offsets = Vec::new();
    for &ps_off in &ps_offsets {
        if ps_off == 0 || seen_offsets.contains(&ps_off) {
            continue;
        }
        seen_offsets.push(ps_off);
        if let Some(shader) = extract_shader(data, ps_off as usize, ps_size as usize) {
            pixel_shaders.push(shader);
        }
    }

    Some(UfxFile {
        version,
        hash,
        root_signature,
        vertex_shader,
        pixel_shaders,
        ps_offsets,
    })
}

/// Extract and parse a DXBC blob from a known region of the file.
fn extract_shader<'a>(data: &'a [u8], offset: usize, size: usize) -> Option<Shader<'a>> {
    if offset == 0 || size == 0 {
        return None;
    }
    let end = offset.checked_add(size)?;
    if end > data.len() {
        return None;
    }
    let region = &data[offset..end];
    let containers = dxbc::scan_dxbc(region);
    containers.into_iter().next().map(|mut container| {
        // Adjust offset to be relative to the original file.
        container.offset_in_file += offset;
        Shader::from_container(container)
    })
}
