//! IO traits and helpers.
//!
//! When the **`std`** feature is enabled the public items [`Read`], [`Seek`],
//! [`SeekFrom`] and [`IoError`] are re-exports of their `std::io`
//! counterparts, so any type that already implements `std::io::Read` or
//! `std::io::Seek` works directly.
//!
//! When building in **`no_std`** mode, minimal replacement traits and types
//! are defined here instead.
//!
//! [`SliceCursor`] wraps a `&[u8]` with a position tracker, serving as a
//! `no_std` replacement for `std::io::Cursor<&[u8]>`.

#[cfg(feature = "std")]
extern crate std;

#[cfg(feature = "std")]
pub use std::io::{Error as IoError, Read, Seek, SeekFrom, Write};

/// Seek position (no_std replacement for `std::io::SeekFrom`).
#[cfg(not(feature = "std"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeekFrom {
    /// Seek to an absolute position.
    Start(u64),
    /// Seek relative to the current position.
    Current(i64),
    /// Seek relative to the end of the stream.
    End(i64),
}

/// IO error (no_std replacement for `std::io::Error`).
#[cfg(not(feature = "std"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoError {
    /// Reached end of data unexpectedly.
    UnexpectedEof,
    /// Invalid seek position (negative or overflow).
    InvalidSeek,
}

#[cfg(not(feature = "std"))]
impl core::fmt::Display for IoError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnexpectedEof => write!(f, "unexpected end of data"),
            Self::InvalidSeek => write!(f, "invalid seek position"),
        }
    }
}

#[cfg(not(feature = "std"))]
impl core::error::Error for IoError {}

/// Minimal read trait (no_std replacement for `std::io::Read`).
#[cfg(not(feature = "std"))]
pub trait Read {
    /// Pull bytes from this source into `buf`.
    ///
    /// Returns the number of bytes read (0 means EOF).
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError>;

    /// Read exactly `buf.len()` bytes, or return an error.
    fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), IoError> {
        let mut filled = 0;
        while filled < buf.len() {
            match self.read(&mut buf[filled..])? {
                0 => return Err(IoError::UnexpectedEof),
                n => filled += n,
            }
        }
        Ok(())
    }
}

/// Minimal seek trait (no_std replacement for `std::io::Seek`).
#[cfg(not(feature = "std"))]
pub trait Seek {
    /// Seek to a position in the stream.
    fn seek(&mut self, pos: SeekFrom) -> Result<u64, IoError>;

    /// Return the current stream position.
    fn stream_position(&mut self) -> Result<u64, IoError> {
        self.seek(SeekFrom::Current(0))
    }
}

/// Minimal write trait (no_std replacement for `std::io::Write`).
#[cfg(not(feature = "std"))]
pub trait Write {
    /// Write a buffer into this writer, returning how many bytes were written.
    fn write(&mut self, buf: &[u8]) -> Result<usize, IoError>;

    /// Flush this output stream.
    fn flush(&mut self) -> Result<(), IoError>;

    /// Write all bytes from `buf`, returning an error if not all could be written.
    fn write_all(&mut self, mut buf: &[u8]) -> Result<(), IoError> {
        while !buf.is_empty() {
            match self.write(buf)? {
                0 => return Err(IoError::UnexpectedEof),
                n => buf = &buf[n..],
            }
        }
        Ok(())
    }
}

// Blanket impls for `&mut T` — `std::io` provides these automatically,
// but in `no_std` we need them explicitly.
#[cfg(not(feature = "std"))]
impl<T: Read + ?Sized> Read for &mut T {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
        (**self).read(buf)
    }
}

#[cfg(not(feature = "std"))]
impl<T: Seek + ?Sized> Seek for &mut T {
    fn seek(&mut self, pos: SeekFrom) -> Result<u64, IoError> {
        (**self).seek(pos)
    }
}

#[cfg(not(feature = "std"))]
impl<T: Write + ?Sized> Write for &mut T {
    fn write(&mut self, buf: &[u8]) -> Result<usize, IoError> {
        (**self).write(buf)
    }
    fn flush(&mut self) -> Result<(), IoError> {
        (**self).flush()
    }
}

/// Create an "unexpected EOF" error.
#[inline]
pub fn unexpected_eof() -> IoError {
    #[cfg(feature = "std")]
    {
        std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "unexpected eof")
    }
    #[cfg(not(feature = "std"))]
    {
        IoError::UnexpectedEof
    }
}

/// Create an "invalid seek" error.
#[inline]
pub fn invalid_seek() -> IoError {
    #[cfg(feature = "std")]
    {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid seek")
    }
    #[cfg(not(feature = "std"))]
    {
        IoError::InvalidSeek
    }
}

/// Check whether an IO error is an "unexpected EOF".
#[inline]
pub fn is_unexpected_eof(err: &IoError) -> bool {
    #[cfg(feature = "std")]
    {
        err.kind() == std::io::ErrorKind::UnexpectedEof
    }
    #[cfg(not(feature = "std"))]
    {
        matches!(err, IoError::UnexpectedEof)
    }
}

/// A cursor over a byte slice — `no_std` replacement for
/// `std::io::Cursor<&[u8]>`.
pub struct SliceCursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> SliceCursor<'a> {
    /// Create a new cursor at position 0.
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    /// Current byte offset.
    pub fn position(&self) -> usize {
        self.pos
    }

    /// Bytes remaining after the current position.
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

    /// Get a reference to the full underlying slice.
    pub fn get_ref(&self) -> &[u8] {
        self.data
    }
}

impl Read for SliceCursor<'_> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
        let available = self.data.len() - self.pos;
        let to_read = buf.len().min(available);
        buf[..to_read].copy_from_slice(&self.data[self.pos..self.pos + to_read]);
        self.pos += to_read;
        Ok(to_read)
    }
}

impl Seek for SliceCursor<'_> {
    fn seek(&mut self, pos: SeekFrom) -> Result<u64, IoError> {
        let new_pos = match pos {
            SeekFrom::Start(n) => n as usize,
            SeekFrom::Current(n) => if n >= 0 {
                self.pos.checked_add(n as usize)
            } else {
                self.pos.checked_sub((-n) as usize)
            }
            .ok_or(invalid_seek())?,
            SeekFrom::End(n) => if n >= 0 {
                self.data.len().checked_add(n as usize)
            } else {
                self.data.len().checked_sub((-n) as usize)
            }
            .ok_or(invalid_seek())?,
        };
        if new_pos > self.data.len() {
            return Err(invalid_seek());
        }
        self.pos = new_pos;
        Ok(new_pos as u64)
    }
}

/// A seekable, growable cursor over a `&mut Vec<u8>` — `no_std` replacement
/// for `std::io::Cursor<&mut Vec<u8>>`.
///
/// Writes at the current position overwrite existing bytes. Writes past the
/// end extend the vector with zeroes as needed.
pub struct MutCursor<'a> {
    buf: &'a mut alloc::vec::Vec<u8>,
    pos: usize,
}

impl<'a> MutCursor<'a> {
    /// Create a new cursor at position 0.
    pub fn new(buf: &'a mut alloc::vec::Vec<u8>) -> Self {
        Self { buf, pos: 0 }
    }

    /// Current byte offset.
    pub fn position(&self) -> usize {
        self.pos
    }

    /// Current stream position as `u64` (mirrors `std::io::Cursor::stream_position`).
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
        let new_pos = match pos {
            SeekFrom::Start(n) => n as usize,
            SeekFrom::Current(n) => if n >= 0 {
                self.pos.checked_add(n as usize)
            } else {
                self.pos.checked_sub((-n) as usize)
            }
            .ok_or(invalid_seek())?,
            SeekFrom::End(n) => if n >= 0 {
                self.buf.len().checked_add(n as usize)
            } else {
                self.buf.len().checked_sub((-n) as usize)
            }
            .ok_or(invalid_seek())?,
        };
        self.pos = new_pos;
        Ok(new_pos as u64)
    }
}

/// Extension trait for writing little-endian primitives.
///
/// Provided for any type implementing [`Write`], replacing the need for
/// the `byteorder` crate.
pub trait WriteLe: Write {
    /// Write a `u8`.
    fn write_u8(&mut self, v: u8) -> Result<(), IoError> {
        self.write_all(&[v])
    }
    /// Write a little-endian `u32`.
    fn write_u32_le(&mut self, v: u32) -> Result<(), IoError> {
        self.write_all(&v.to_le_bytes())
    }
    /// Write a little-endian `i32`.
    fn write_i32_le(&mut self, v: i32) -> Result<(), IoError> {
        self.write_all(&v.to_le_bytes())
    }
    /// Write a little-endian `u64`.
    fn write_u64_le(&mut self, v: u64) -> Result<(), IoError> {
        self.write_all(&v.to_le_bytes())
    }
    /// Write a little-endian `f32`.
    fn write_f32_le(&mut self, v: f32) -> Result<(), IoError> {
        self.write_all(&v.to_le_bytes())
    }
}

impl<T: Write + ?Sized> WriteLe for T {}
