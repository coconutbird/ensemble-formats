//! Shared string table builder for UAX writer.
//!
//! Collects (position, string) fixup pairs, builds a deduplicated string
//! table, then patches the 8-byte little-endian offsets back into the buffer.

use alloc::string::String;
use alloc::vec::Vec;

/// A deferred string offset that will be patched once the string table is built.
struct StringFixup {
    /// Byte position in the output buffer where a u64 LE offset should be written.
    position: usize,
    /// The string value to be placed in the string table.
    string: String,
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
    /// Strings are null-terminated and tightly packed (no alignment padding).
    pub fn write(self, buf: &mut Vec<u8>) {
        let mut offsets: Vec<(String, usize)> = Vec::new();

        for fixup in &self.fixups {
            if offsets.iter().all(|(s, _)| s != &fixup.string) {
                let offset = buf.len();
                offsets.push((fixup.string.clone(), offset));
                buf.extend_from_slice(fixup.string.as_bytes());
                buf.push(0);
            }
        }

        for fixup in &self.fixups {
            let offset = offsets.iter().find(|(s, _)| s == &fixup.string).unwrap().1 as u64;
            buf[fixup.position..fixup.position + 8].copy_from_slice(&offset.to_le_bytes());
        }
    }
}
