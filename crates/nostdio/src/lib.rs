//! `no_std`-compatible I/O traits and helpers.
//!
//! # Feature flags
//!
//! | Feature | Effect |
//! |---------|--------|
//! | `std`   | Re-exports [`Read`], [`Write`], [`Seek`], [`SeekFrom`] and [`IoError`] from `std::io`. |
//!
//! Without `std`, minimal replacement traits are provided so that crates
//! can be compiled for bare-metal targets.
//!
//! # Cursors
//!
//! * [`SliceCursor`] — read-only cursor over `&[u8]` (replaces
//!   `std::io::Cursor<&[u8]>`).
//! * [`MutCursor`] — read/write cursor over `&mut Vec<u8>` (replaces
//!   `std::io::Cursor<&mut Vec<u8>>`).
//!
//! # Progress reporting
//!
//! The [`Progress`] trait provides a standard way for long-running
//! operations to report progress and support cancellation.  A blanket
//! impl lets closures `FnMut(u64, u64) -> bool` be used directly, and
//! [`NoProgress`] is a zero-cost no-op implementation.

#![no_std]

extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

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

/// Read-only cursor over `&[u8]` (`no_std` replacement for
/// `std::io::Cursor<&[u8]>`).
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
pub struct MutCursor<'a> {
    buf: &'a mut alloc::vec::Vec<u8>,
    pos: usize,
}

impl<'a> MutCursor<'a> {
    /// Wrap `buf` with the cursor starting at offset 0.
    pub fn new(buf: &'a mut alloc::vec::Vec<u8>) -> Self {
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

/// Progress reporting and cancellation for long-running I/O operations.
///
/// Return `true` from [`report`](Progress::report) to continue, or
/// `false` to request cancellation.
///
/// # Using a closure
///
/// Any `FnMut(u64, u64) -> bool` implements `Progress` automatically:
///
/// ```ignore
/// writer.finalize_with_progress(&mut |done, total| {
///     println!("{done}/{total}");
///     true // keep going
/// })?;
/// ```
///
/// # Opting out
///
/// Pass [`NoProgress`] when progress tracking is not needed — it compiles
/// to a no-op.
pub trait Progress {
    /// Called after each unit of work.
    ///
    /// * `bytes_done`  — cumulative bytes processed so far.
    /// * `total_bytes` — expected total (may be 0 if unknown).
    ///
    /// Return `true` to continue, `false` to cancel.
    fn report(&mut self, bytes_done: u64, total_bytes: u64) -> bool;
}

impl<F: FnMut(u64, u64) -> bool> Progress for F {
    #[inline]
    fn report(&mut self, bytes_done: u64, total_bytes: u64) -> bool {
        self(bytes_done, total_bytes)
    }
}

/// No-op [`Progress`] implementation that never cancels.
pub struct NoProgress;

impl Progress for NoProgress {
    #[inline]
    fn report(&mut self, _: u64, _: u64) -> bool {
        true
    }
}

/// Convenience methods for writing little-endian primitives.
pub trait WriteLe: Write {
    /// Write a `u8`.
    fn write_u8(&mut self, v: u8) -> Result<(), IoError> {
        self.write_all(&[v])
    }

    /// Write an `i8`.
    fn write_i8(&mut self, v: i8) -> Result<(), IoError> {
        self.write_all(&v.to_le_bytes())
    }

    /// Write a `u16` in little-endian byte order.
    fn write_u16_le(&mut self, v: u16) -> Result<(), IoError> {
        self.write_all(&v.to_le_bytes())
    }

    /// Write an `i16` in little-endian byte order.
    fn write_i16_le(&mut self, v: i16) -> Result<(), IoError> {
        self.write_all(&v.to_le_bytes())
    }

    /// Write a `u32` in little-endian byte order.
    fn write_u32_le(&mut self, v: u32) -> Result<(), IoError> {
        self.write_all(&v.to_le_bytes())
    }

    /// Write an `i32` in little-endian byte order.
    fn write_i32_le(&mut self, v: i32) -> Result<(), IoError> {
        self.write_all(&v.to_le_bytes())
    }

    /// Write a `u64` in little-endian byte order.
    fn write_u64_le(&mut self, v: u64) -> Result<(), IoError> {
        self.write_all(&v.to_le_bytes())
    }

    /// Write an `i64` in little-endian byte order.
    fn write_i64_le(&mut self, v: i64) -> Result<(), IoError> {
        self.write_all(&v.to_le_bytes())
    }

    /// Write an `f32` in little-endian byte order.
    fn write_f32_le(&mut self, v: f32) -> Result<(), IoError> {
        self.write_all(&v.to_le_bytes())
    }

    /// Write an `f64` in little-endian byte order.
    fn write_f64_le(&mut self, v: f64) -> Result<(), IoError> {
        self.write_all(&v.to_le_bytes())
    }
}

impl<T: Write + ?Sized> WriteLe for T {}

/// Convenience methods for writing big-endian primitives.
pub trait WriteBe: Write {
    /// Write a `u8`.
    fn write_u8_be(&mut self, v: u8) -> Result<(), IoError> {
        self.write_all(&[v])
    }

    /// Write an `i8`.
    fn write_i8_be(&mut self, v: i8) -> Result<(), IoError> {
        self.write_all(&v.to_be_bytes())
    }

    /// Write a `u16` in big-endian byte order.
    fn write_u16_be(&mut self, v: u16) -> Result<(), IoError> {
        self.write_all(&v.to_be_bytes())
    }

    /// Write an `i16` in big-endian byte order.
    fn write_i16_be(&mut self, v: i16) -> Result<(), IoError> {
        self.write_all(&v.to_be_bytes())
    }

    /// Write a `u32` in big-endian byte order.
    fn write_u32_be(&mut self, v: u32) -> Result<(), IoError> {
        self.write_all(&v.to_be_bytes())
    }

    /// Write an `i32` in big-endian byte order.
    fn write_i32_be(&mut self, v: i32) -> Result<(), IoError> {
        self.write_all(&v.to_be_bytes())
    }

    /// Write a `u64` in big-endian byte order.
    fn write_u64_be(&mut self, v: u64) -> Result<(), IoError> {
        self.write_all(&v.to_be_bytes())
    }

    /// Write an `i64` in big-endian byte order.
    fn write_i64_be(&mut self, v: i64) -> Result<(), IoError> {
        self.write_all(&v.to_be_bytes())
    }

    /// Write an `f32` in big-endian byte order.
    fn write_f32_be(&mut self, v: f32) -> Result<(), IoError> {
        self.write_all(&v.to_be_bytes())
    }

    /// Write an `f64` in big-endian byte order.
    fn write_f64_be(&mut self, v: f64) -> Result<(), IoError> {
        self.write_all(&v.to_be_bytes())
    }
}

impl<T: Write + ?Sized> WriteBe for T {}
