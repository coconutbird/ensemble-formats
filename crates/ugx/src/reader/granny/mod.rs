//! Granny chunk (0x703) parser.
//!
//! Parses granny bones (inverse world matrices), granny meshes
//! (bone bindings per mesh), and bone `ExtendedData` (Granny2 variant system)
//! from the Granny2-compatible serialized chunk.

mod bones;
mod extended_data;
mod meshes;

use alloc::string::String;

use crate::error::{Error, Result};
use nostdio::{Cursor, ReadLe, read_null_terminated_string};

pub(super) use bones::parse_granny_bones;
pub(super) use meshes::parse_granny_meshes;

/// Return the Granny data starting at an encoded offset.
fn data_tail<'a>(data: &'a [u8], offset: usize, context: &str) -> Result<&'a [u8]> {
    data.get(offset..).ok_or_else(|| Error::UnexpectedEof {
        context: String::from(context),
    })
}

/// Return an exact range from Granny data using checked offset arithmetic.
fn data_range<'a>(data: &'a [u8], offset: usize, length: usize, context: &str) -> Result<&'a [u8]> {
    let end = offset
        .checked_add(length)
        .ok_or(Error::SizeOverflow("Granny data range"))?;
    data.get(offset..end).ok_or_else(|| Error::UnexpectedEof {
        context: String::from(context),
    })
}

/// Convert an encoded pointer to a target-sized offset.
fn pointer_offset(pointer: u64, context: &'static str) -> Result<usize> {
    crate::checked_usize(pointer, context)
}

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

    let mut cursor = Cursor::new(&data[0x10..]);
    let pointer = cursor.read_u64_le()?;
    if pointer == 0 {
        return Err(Error::InvalidGrannyChunk {
            actual: String::from("<null or OOB pointer>"),
        });
    }
    let offset = pointer_offset(pointer, "Granny filename pointer")?;
    let name_data =
        data_tail(data, offset, "Granny filename").map_err(|_| Error::InvalidGrannyChunk {
            actual: String::from("<null or OOB pointer>"),
        })?;

    let name = read_null_terminated_string(name_data);
    if !name.eq_ignore_ascii_case("gr2ugx") {
        return Err(Error::InvalidGrannyChunk { actual: name });
    }

    Ok(())
}
