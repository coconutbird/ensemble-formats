//! Deduplicated string table with 64-bit pointer fixups.
//!
//! A writer records each string pointer as a fixup: a byte position in the
//! output buffer and a string value. [`StringTable::write`] then appends each
//! unique string one time and writes the offset of each string as a
//! little-endian `u64` at each fixup position.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Size in bytes of one patched string pointer.
const POINTER_SIZE: usize = core::mem::size_of::<u64>();

/// Error that [`StringTable::write`] returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum StringTableError {
    /// The end of a fixup range is larger than `usize::MAX`.
    #[error("string pointer fixup at {position} overflows the address range")]
    PositionOverflow {
        /// Byte position of the fixup.
        position: usize,
    },
    /// A fixup range is not fully in the output buffer.
    #[error("string pointer fixup at {position} is outside the buffer")]
    PositionOutOfRange {
        /// Byte position of the fixup.
        position: usize,
    },
    /// A string offset does not fit in a `u64`.
    #[error("string table offset {offset} does not fit in u64")]
    OffsetOverflow {
        /// Byte offset of the string in the output buffer.
        offset: usize,
    },
}

/// A deferred string pointer.
struct StringFixup {
    /// Byte position in the output buffer of the `u64` offset.
    position: usize,
    /// String that the pointer refers to.
    string: String,
}

/// Collects string fixups and writes a deduplicated string table.
///
/// The table contains each unique string one time, in the order of its
/// first fixup. Each string has a NUL terminator. The table has no padding.
#[derive(Default)]
pub struct StringTable {
    fixups: Vec<StringFixup>,
}

impl StringTable {
    /// Creates an empty string table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a fixup: write the offset of `string` at `position`.
    pub fn add(&mut self, position: usize, string: String) {
        self.fixups.push(StringFixup { position, string });
    }

    /// Appends the string table to `buf` and patches each fixup position.
    ///
    /// Each offset is the byte position of the string in `buf`. The writer
    /// writes each offset as a little-endian `u64`.
    ///
    /// # Errors
    ///
    /// Returns [`StringTableError`] if a fixup range is not in `buf` after
    /// the table is appended, or if a string offset does not fit in a `u64`.
    pub fn write(self, buf: &mut Vec<u8>) -> Result<(), StringTableError> {
        let mut offsets: BTreeMap<&str, usize> = BTreeMap::new();
        let mut fixup_offsets = Vec::with_capacity(self.fixups.len());

        for fixup in &self.fixups {
            let offset = *offsets.entry(fixup.string.as_str()).or_insert_with(|| {
                let offset = buf.len();
                buf.extend_from_slice(fixup.string.as_bytes());
                buf.push(0);
                offset
            });
            fixup_offsets.push(offset);
        }

        for (fixup, offset) in self.fixups.iter().zip(fixup_offsets) {
            let position = fixup.position;
            let end = position
                .checked_add(POINTER_SIZE)
                .ok_or(StringTableError::PositionOverflow { position })?;
            let destination = buf
                .get_mut(position..end)
                .ok_or(StringTableError::PositionOutOfRange { position })?;
            let pointer =
                u64::try_from(offset).map_err(|_| StringTableError::OffsetOverflow { offset })?;
            destination.copy_from_slice(&pointer.to_le_bytes());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use alloc::string::String;
    use alloc::vec;

    use super::{StringTable, StringTableError};

    #[test]
    fn writes_unique_strings_in_first_use_order_and_patches_pointers() {
        let mut buf = vec![0xAA; 4 * 8];
        let mut table = StringTable::new();
        table.add(0, String::from("bone"));
        table.add(8, String::from("mesh"));
        table.add(16, String::from("bone"));
        table.add(24, String::new());
        table.write(&mut buf).unwrap();

        let mut expected = vec![];
        expected.extend_from_slice(&32_u64.to_le_bytes());
        expected.extend_from_slice(&37_u64.to_le_bytes());
        expected.extend_from_slice(&32_u64.to_le_bytes());
        expected.extend_from_slice(&42_u64.to_le_bytes());
        expected.extend_from_slice(b"bone\0mesh\0\0");
        assert_eq!(buf, expected);
    }

    #[test]
    fn empty_table_leaves_buffer_unchanged() {
        let mut buf = vec![1, 2, 3];
        StringTable::new().write(&mut buf).unwrap();
        assert_eq!(buf, [1, 2, 3]);
    }

    #[test]
    fn rejects_fixup_outside_buffer() {
        let mut buf = vec![0; 4];
        let mut table = StringTable::new();
        table.add(0, String::from("a"));
        assert_eq!(
            table.write(&mut buf),
            Err(StringTableError::PositionOutOfRange { position: 0 })
        );
    }

    #[test]
    fn rejects_overflowing_fixup_position() {
        let mut buf = vec![];
        let mut table = StringTable::new();
        table.add(usize::MAX, String::from("a"));
        assert_eq!(
            table.write(&mut buf),
            Err(StringTableError::PositionOverflow {
                position: usize::MAX
            })
        );
    }
}
