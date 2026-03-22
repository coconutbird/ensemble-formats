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
