//! Granny chunk (0x703) parser.
//!
//! Parses granny bones (inverse world matrices), granny meshes
//! (bone bindings per mesh), and bone ExtendedData (Granny2 variant system)
//! from the Granny2-compatible serialized chunk.

mod bones;
mod extended_data;
mod meshes;

use alloc::string::String;

use crate::error::{Error, Result};
use nostdio::{ReadLe, SliceCursor, read_null_terminated_string};

pub(super) use bones::parse_granny_bones;
pub(super) use meshes::parse_granny_meshes;

/// Validate the Granny chunk (0x703) by reading `FromFileName` at +0x10.
///
/// The engine (`BGrannyModel::load`) rejects the chunk unless this string is `"gr2ugx"`.
/// Returns `Err(InvalidGrannyChunk)` if the chunk is present but invalid.
pub(super) fn validate_granny_chunk(data: &[u8]) -> Result<()> {
    if data.len() < 0x18 {
        return Err(Error::InvalidGrannyChunk {
            actual: String::from("<chunk too small>"),
        });
    }

    let mut sc = SliceCursor::new(&data[0x10..]);
    let ptr = sc.read_u64_le()? as usize;
    if ptr == 0 || ptr >= data.len() {
        return Err(Error::InvalidGrannyChunk {
            actual: String::from("<null or OOB pointer>"),
        });
    }

    let name = read_null_terminated_string(&data[ptr..]);
    if !name.eq_ignore_ascii_case("gr2ugx") {
        return Err(Error::InvalidGrannyChunk { actual: name });
    }

    Ok(())
}
