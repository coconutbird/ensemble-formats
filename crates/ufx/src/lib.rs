//! Zero-copy parser for the UFXS container format used by Halo Wars 2
//! compiled shaders.
//!
//! A `.ufx` file wraps one or more DXBC blobs (Root Signature, Vertex Shader,
//! Pixel Shader) inside a proprietary Ensemble header.  The pixel shader offset
//! is repeated across four quality-level slots that may point to the same blob.
//!
//! This crate uses [`d3dasm`] to parse the embedded DXBC containers.
//!
//! # Quick start
//!
//! ```ignore
//! let data = std::fs::read("shader.ufx")?;
//! let ufx = ufx::parse(&data)?;
//!
//! // Inspect vertex shader constant buffers
//! if let Some(vs) = &ufx.vertex_shader {
//!     if let Some(prog) = vs.program() {
//!         println!("VS: SM {}.{}", prog.major_version, prog.minor_version);
//!     }
//! }
//!
//! // Run semantic inference on the pixel shader
//! if let Some(ps) = ufx.pixel_shaders.first() {
//!     if let Some(prog) = ps.program() {
//!         use ufx::cb_infer::{infer_cb_params, HOGAN_PS_SLOT, HOGAN_VS_SLOT};
//!         let params = infer_cb_params(prog, HOGAN_PS_SLOT, HOGAN_VS_SLOT);
//!         for p in &params {
//!             println!("  {} => {} [{}]", p.components, p.semantic, p.confidence);
//!         }
//!     }
//! }
//! ```

#![no_std]
extern crate alloc;

pub mod cb_infer;

use alloc::vec::Vec;
use core::fmt;
use d3dasm::Shader;
use d3dasm::dxbc;
use nostdio::{ReadLe, Seek, SeekFrom, SliceCursor};

const UFXS_MAGIC: &[u8; 4] = b"UFXS";

/// Minimum header size to read all fixed fields.
const UFXS_MIN_HEADER: usize = 0x48;

/// Errors that can occur when parsing a UFX file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The input is shorter than the minimum UFXS header.
    TooShort {
        /// Length of the input data.
        len: usize,
    },
    /// The first four bytes are not `UFXS`.
    BadMagic {
        /// The bytes that were found.
        found: [u8; 4],
    },
    /// A required header field could not be read.
    TruncatedHeader,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort { len } => {
                write!(f, "input too short ({len} bytes, need {UFXS_MIN_HEADER})")
            }
            Self::BadMagic { found } => write!(
                f,
                "bad magic: expected UFXS, got {:?}",
                core::str::from_utf8(found).unwrap_or("????")
            ),
            Self::TruncatedHeader => f.write_str("truncated UFXS header"),
        }
    }
}

/// A parsed UFXS container with metadata and structured shader stages.
#[derive(Debug)]
pub struct UfxFile<'a> {
    /// UFXS format version (expected: 9 for Hogan ubershaders).
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

/// Check whether `data` starts with the UFXS magic bytes.
pub fn is_ufx(data: &[u8]) -> bool {
    data.len() >= 4 && &data[0..4] == UFXS_MAGIC
}

/// Parse `data` as a UFXS file.
///
/// Returns an [`Error`] if the magic bytes don't match or the header is
/// truncated.
pub fn parse(data: &[u8]) -> Result<UfxFile<'_>, Error> {
    if data.len() < UFXS_MIN_HEADER {
        return Err(Error::TooShort { len: data.len() });
    }
    if &data[0..4] != UFXS_MAGIC {
        let mut found = [0u8; 4];
        found.copy_from_slice(&data[0..4]);
        return Err(Error::BadMagic { found });
    }

    let mut c = SliceCursor::new(data);
    let e = |_| Error::TruncatedHeader;

    c.seek(SeekFrom::Start(0x04)).map_err(e)?;
    let version = c.read_u32_le().map_err(e)?;
    let hash = c.read_u32_le().map_err(e)?;

    c.seek(SeekFrom::Start(0x10)).map_err(e)?;
    let rts0_offset = c.read_u32_le().map_err(e)?;
    let rts0_size = c.read_u32_le().map_err(e)?;
    let vs_offset = c.read_u32_le().map_err(e)?;
    let vs_size = c.read_u32_le().map_err(e)?;

    // Four PS quality-level slots (each stored as u32 offset + u32 pad=0).
    c.seek(SeekFrom::Start(0x20)).map_err(e)?;
    let ps_off_0 = c.read_u32_le().map_err(e)?;
    c.seek(SeekFrom::Start(0x28)).map_err(e)?;
    let ps_off_1 = c.read_u32_le().map_err(e)?;
    c.seek(SeekFrom::Start(0x30)).map_err(e)?;
    let ps_off_2 = c.read_u32_le().map_err(e)?;
    c.seek(SeekFrom::Start(0x38)).map_err(e)?;
    let ps_off_3 = c.read_u32_le().map_err(e)?;
    let ps_size = c.read_u32_le().map_err(e)?;
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

    Ok(UfxFile {
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
