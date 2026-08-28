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

/// Validate the Granny chunk (0x703) using the requirements enforced by the game.
///
/// The engine rejects the chunk unless `FromFileName` is `"gr2ugx"`, the
/// model count is exactly one, and both the model array and its single model
/// pointer are non-null.
pub(super) fn validate_granny_chunk(data: &[u8]) -> Result<()> {
    if data.len() < 0x70 {
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

    let mut model_cursor = Cursor::new(&data[0x60..]);
    let model_count = model_cursor.read_u32_le()?;
    if model_count != 1 {
        return Err(Error::InvalidGrannyModelCount {
            actual: model_count,
        });
    }

    let model_array_pointer = model_cursor.read_u64_le()?;
    if model_array_pointer == 0 {
        return Err(Error::InvalidGrannyPointer {
            context: "model-array",
        });
    }
    let models_offset = pointer_offset(model_array_pointer, "Granny model pointer array")?;
    let model_pointer_data = data_range(data, models_offset, 8, "Granny model pointer array")
        .map_err(|_| Error::InvalidGrannyPointer {
            context: "model-array",
        })?;
    let mut pointer_cursor = Cursor::new(model_pointer_data);
    let model_record_pointer = pointer_cursor.read_u64_le()?;
    if model_record_pointer == 0 {
        return Err(Error::InvalidGrannyPointer { context: "model" });
    }
    let model_offset = pointer_offset(model_record_pointer, "Granny model")?;
    data_range(data, model_offset, 0x60, "Granny model")
        .map_err(|_| Error::InvalidGrannyPointer { context: "model" })?;

    Ok(())
}
