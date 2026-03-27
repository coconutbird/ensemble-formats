//! Core I/O traits, error types, and helper functions.

#[cfg(feature = "std")]
pub use std::io::{Error as IoError, Read, Seek, SeekFrom, Write};

/// Seek origin (`no_std` replacement for [`std::io::SeekFrom`]).
#[cfg(not(feature = "std"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeekFrom {
    /// Absolute byte offset from the start.
    Start(u64),
    /// Signed offset relative to the current position.
    Current(i64),
    /// Signed offset relative to the end.
    End(i64),
}

/// I/O error (`no_std` replacement for [`std::io::Error`]).
#[cfg(not(feature = "std"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoError {
    /// The stream ended before enough bytes could be read/written.
    UnexpectedEof,
    /// A seek landed at an invalid (negative / overflow) position.
    InvalidSeek,
}

#[cfg(not(feature = "std"))]
impl core::fmt::Display for IoError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnexpectedEof => f.write_str("unexpected end of data"),
            Self::InvalidSeek => f.write_str("invalid seek position"),
        }
    }
}

#[cfg(not(feature = "std"))]
impl core::error::Error for IoError {}

/// Byte-oriented reader (`no_std` replacement for [`std::io::Read`]).
#[cfg(not(feature = "std"))]
pub trait Read {
    /// Pull bytes from this source into `buf`, returning how many were read.
    ///
    /// A return value of `0` signals end-of-stream.
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError>;

    /// Read exactly `buf.len()` bytes or fail with [`IoError::UnexpectedEof`].
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

/// Positional seeking (`no_std` replacement for [`std::io::Seek`]).
#[cfg(not(feature = "std"))]
pub trait Seek {
    /// Move the cursor to `pos`, returning the new absolute offset.
    fn seek(&mut self, pos: SeekFrom) -> Result<u64, IoError>;

    /// Return the current stream position (shorthand for `seek(Current(0))`).
    fn stream_position(&mut self) -> Result<u64, IoError> {
        self.seek(SeekFrom::Current(0))
    }
}

/// Byte-oriented writer (`no_std` replacement for [`std::io::Write`]).
#[cfg(not(feature = "std"))]
pub trait Write {
    /// Write bytes from `buf`, returning how many were accepted.
    fn write(&mut self, buf: &[u8]) -> Result<usize, IoError>;

    /// Flush any buffered data to the underlying sink.
    fn flush(&mut self) -> Result<(), IoError>;

    /// Write the entire buffer or fail with [`IoError::UnexpectedEof`].
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

// `std::io` provides blanket impls for `&mut T`; replicate them here.

#[cfg(not(feature = "std"))]
impl<T: Read + ?Sized> Read for &mut T {
    #[inline]
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
        (**self).read(buf)
    }
}

#[cfg(not(feature = "std"))]
impl<T: Seek + ?Sized> Seek for &mut T {
    #[inline]
    fn seek(&mut self, pos: SeekFrom) -> Result<u64, IoError> {
        (**self).seek(pos)
    }
}

/// Construct an "unexpected EOF" [`IoError`].
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

/// Construct an "invalid seek" [`IoError`].
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

/// Returns `true` when `err` represents an unexpected-EOF condition.
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
