//! Shared string table builder for UGX writer chunks.
//!
//! Both the cached data (0x700) and granny (0x703) chunks use the same
//! pattern: collect (position, string) fixup pairs, build a deduplicated
//! string table, then patch the 8-byte little-endian offsets back into the
//! buffer.

use alloc::string::String;
use alloc::vec::Vec;

use crate::{Error, Result};

/// A deferred string offset that will be patched once the string table is built.
pub(crate) struct StringFixup {
    /// Byte position in the output buffer where a u64 LE offset should be written.
    pub position: usize,
    /// The string value to be placed in the string table.
    pub string: String,
}

/// Accumulates string fixups and builds a deduplicated string table.
pub(crate) struct StringTable {
    fixups: Vec<StringFixup>,
}

impl StringTable {
    /// Create an empty string table builder.
    pub fn new() -> Self {
        Self { fixups: Vec::new() }
    }

    /// Register a fixup: at `position` in the buffer, write the offset of `string`.
    pub fn add(&mut self, position: usize, string: String) {
        self.fixups.push(StringFixup { position, string });
    }

    /// Append the deduplicated string table to `buf` and patch all fixup positions
    /// with the final u64 LE offsets.
    ///
    /// Strings are null-terminated and no alignment padding is applied.
    ///
    /// # Errors
    ///
    /// Returns an error if a string offset or fixup range cannot be represented.
    pub fn write(self, buf: &mut Vec<u8>) -> Result<()> {
        self.write_inner(buf, 1)
    }

    fn write_inner(self, buf: &mut Vec<u8>, align: usize) -> Result<()> {
        let mut offsets: Vec<(String, usize)> = Vec::new();

        for fixup in &self.fixups {
            if offsets.iter().all(|(s, _)| s != &fixup.string) {
                let offset = buf.len();
                offsets.push((fixup.string.clone(), offset));
                buf.extend_from_slice(fixup.string.as_bytes());
                buf.push(0);
                if align > 1 {
                    while !buf.len().is_multiple_of(align) {
                        buf.push(0);
                    }
                }
            }
        }

        for fixup in &self.fixups {
            let offset = offsets
                .iter()
                .find(|(string, _)| string == &fixup.string)
                .map(|(_, offset)| *offset)
                .ok_or_else(|| Error::UnsupportedFormat("missing string-table entry".into()))?;
            let end = fixup
                .position
                .checked_add(core::mem::size_of::<u64>())
                .ok_or(Error::SizeOverflow("string-table fixup"))?;
            let destination =
                buf.get_mut(fixup.position..end)
                    .ok_or_else(|| Error::UnexpectedEof {
                        context: "string-table fixup".into(),
                    })?;
            destination.copy_from_slice(
                &u64::try_from(offset)
                    .map_err(|_| Error::SizeOverflow("string-table offset"))?
                    .to_le_bytes(),
            );
        }
        Ok(())
    }
}
