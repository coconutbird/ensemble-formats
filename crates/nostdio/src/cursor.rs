//! Read-only and read/write cursors over byte slices.
//!
//! # Examples
//!
//! Reading sequentially with [`SliceCursor`]:
//!
//! ```
//! use nostdio::{SliceCursor, ReadLe, Seek, SeekFrom};
//!
//! let data = [0x0A, 0x00, 0x00, 0x00, 0x14, 0x00, 0x00, 0x00];
//! let mut cur = SliceCursor::new(&data);
//!
//! assert_eq!(cur.read_u32_le().unwrap(), 10);
//! assert_eq!(cur.position(), 4);
//!
//! // Seek back and re-read
//! cur.seek(SeekFrom::Start(0)).unwrap();
//! assert_eq!(cur.read_u32_le().unwrap(), 10);
//! ```
//!
//! Writing with [`MutCursor`]:
//!
//! ```
//! use nostdio::{MutCursor, WriteLe};
//!
//! let mut buf = Vec::new();
//! let mut cur = MutCursor::new(&mut buf);
//!
//! cur.write_u32_le(42).unwrap();
//! cur.write_u16_le(7).unwrap();
//!
//! assert_eq!(buf, [42, 0, 0, 0, 7, 0]);
//! ```

use alloc::vec::Vec;

use crate::traits::{IoError, Read, Seek, SeekFrom, Write, invalid_seek};

/// Read-only cursor over `&[u8]` (`no_std` replacement for
/// `std::io::Cursor<&[u8]>`).
///
/// Wraps a byte slice and tracks a position that advances on each
/// [`Read`] call.  Implements [`Seek`] for random access.
///
/// # Example
///
/// ```
/// use nostdio::{SliceCursor, Read};
///
/// let data = b"hello";
/// let mut cur = SliceCursor::new(data);
///
/// let mut buf = [0u8; 5];
/// cur.read_exact(&mut buf).unwrap();
/// assert_eq!(&buf, b"hello");
/// assert_eq!(cur.remaining().len(), 0);
/// ```
pub struct SliceCursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> SliceCursor<'a> {
    /// Wrap `data` with the cursor starting at offset 0.
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    /// Current byte offset within the slice.
    pub fn position(&self) -> usize {
        self.pos
    }

    /// Sub-slice from the current position to the end.
    pub fn remaining(&self) -> &[u8] {
        &self.data[self.pos..]
    }

    /// Total length of the underlying slice.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Whether the underlying slice is empty.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Borrow the full underlying slice.
    pub fn get_ref(&self) -> &[u8] {
        self.data
    }
}

impl Read for SliceCursor<'_> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
        let n = buf.len().min(self.data.len() - self.pos);
        buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

impl Seek for SliceCursor<'_> {
    fn seek(&mut self, pos: SeekFrom) -> Result<u64, IoError> {
        let new = resolve_seek(self.pos, self.data.len(), pos)?;
        if new > self.data.len() {
            return Err(invalid_seek());
        }
        self.pos = new;
        Ok(new as u64)
    }
}

/// Read/write cursor over `&mut Vec<u8>` (`no_std` replacement for
/// `std::io::Cursor<&mut Vec<u8>>`).
///
/// Writes past the current length zero-extend the vector automatically.
///
/// # Example
///
/// ```
/// use nostdio::{MutCursor, Write, Seek, SeekFrom};
///
/// let mut buf = Vec::new();
/// let mut cur = MutCursor::new(&mut buf);
///
/// cur.write_all(b"hello").unwrap();
/// assert_eq!(cur.position(), 5);
///
/// // Seek back and overwrite
/// cur.seek(SeekFrom::Start(0)).unwrap();
/// cur.write_all(b"HE").unwrap();
///
/// assert_eq!(&buf, b"HEllo");
/// ```
pub struct MutCursor<'a> {
    buf: &'a mut Vec<u8>,
    pos: usize,
}

impl<'a> MutCursor<'a> {
    /// Wrap `buf` with the cursor starting at offset 0.
    pub fn new(buf: &'a mut Vec<u8>) -> Self {
        Self { buf, pos: 0 }
    }

    /// Current byte offset within the buffer.
    pub fn position(&self) -> usize {
        self.pos
    }

    /// Current byte offset as `u64`.
    pub fn stream_position(&self) -> Result<u64, IoError> {
        Ok(self.pos as u64)
    }
}

impl Write for MutCursor<'_> {
    fn write(&mut self, buf: &[u8]) -> Result<usize, IoError> {
        let end = self.pos + buf.len();
        if end > self.buf.len() {
            self.buf.resize(end, 0);
        }
        self.buf[self.pos..end].copy_from_slice(buf);
        self.pos = end;
        Ok(buf.len())
    }

    fn flush(&mut self) -> Result<(), IoError> {
        Ok(())
    }
}

impl Seek for MutCursor<'_> {
    fn seek(&mut self, pos: SeekFrom) -> Result<u64, IoError> {
        self.pos = resolve_seek(self.pos, self.buf.len(), pos)?;
        Ok(self.pos as u64)
    }
}

/// Shared seek-position arithmetic for both cursor types.
fn resolve_seek(cur: usize, len: usize, pos: SeekFrom) -> Result<usize, IoError> {
    match pos {
        SeekFrom::Start(n) => Ok(n as usize),
        SeekFrom::Current(n) => if n >= 0 {
            cur.checked_add(n as usize)
        } else {
            cur.checked_sub((-n) as usize)
        }
        .ok_or(invalid_seek()),
        SeekFrom::End(n) => if n >= 0 {
            len.checked_add(n as usize)
        } else {
            len.checked_sub((-n) as usize)
        }
        .ok_or(invalid_seek()),
    }
}
