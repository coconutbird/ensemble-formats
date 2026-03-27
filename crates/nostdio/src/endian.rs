//! Runtime-selectable byte order for reading and writing primitives.
//!
//! Use [`Endian`] when the byte order isn't known at compile time (e.g. a
//! file header declares whether the rest is LE or BE).
//!
//! This module provides:
//!
//! * **[`Endian`] enum** — `Little` / `Big`, `Copy + Eq`.
//! * **[`ReadEndian`] / [`WriteEndian`] traits** — blanket-implemented on
//!   every [`ReadLe`]+[`ReadBe`] / [`WriteLe`]+[`WriteBe`]; adds
//!   `read_u32(endian)` style methods.
//!
//! # Example
//!
//! ```
//! use nostdio::{SliceCursor, Endian, ReadEndian};
//!
//! let data = 1u32.to_be_bytes();
//! let mut cur = SliceCursor::new(&data);
//! assert_eq!(cur.read_u32(Endian::Big).unwrap(), 1);
//! ```

use crate::read::{ReadBe, ReadLe};
use crate::traits::IoError;
use crate::write::{WriteBe, WriteLe};

/// Runtime byte-order selector.
///
/// ```
/// use nostdio::Endian;
///
/// let e = Endian::Little;
/// assert_eq!(e, Endian::Little);
/// assert_ne!(e, Endian::Big);
/// ```
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Endian {
    /// Least-significant byte first (x86, ARM default).
    Little,
    /// Most-significant byte first (PowerPC, network order).
    Big,
}

/// Endian-dispatched reading — adds `read_<T>(endian)` methods to any
/// [`ReadLe`] + [`ReadBe`] implementor.
///
/// ```
/// use nostdio::{SliceCursor, Endian, ReadEndian};
///
/// let data = 42u32.to_be_bytes();
/// let mut cur = SliceCursor::new(&data);
/// assert_eq!(cur.read_u32(Endian::Big).unwrap(), 42);
/// ```
pub trait ReadEndian: ReadLe + ReadBe {
    /// Read a `u8` (endianness is irrelevant, parameter accepted for uniformity).
    fn read_u8(&mut self, _e: Endian) -> Result<u8, IoError> {
        self.read_u8_le()
    }

    /// Read an `i8` (endianness is irrelevant, parameter accepted for uniformity).
    fn read_i8(&mut self, _e: Endian) -> Result<i8, IoError> {
        self.read_i8_le()
    }

    /// Read a `u16` with the given byte order.
    fn read_u16(&mut self, e: Endian) -> Result<u16, IoError> {
        match e {
            Endian::Little => self.read_u16_le(),
            Endian::Big => self.read_u16_be(),
        }
    }

    /// Read an `i16` with the given byte order.
    fn read_i16(&mut self, e: Endian) -> Result<i16, IoError> {
        match e {
            Endian::Little => self.read_i16_le(),
            Endian::Big => self.read_i16_be(),
        }
    }

    /// Read a `u32` with the given byte order.
    fn read_u32(&mut self, e: Endian) -> Result<u32, IoError> {
        match e {
            Endian::Little => self.read_u32_le(),
            Endian::Big => self.read_u32_be(),
        }
    }

    /// Read an `i32` with the given byte order.
    fn read_i32(&mut self, e: Endian) -> Result<i32, IoError> {
        match e {
            Endian::Little => self.read_i32_le(),
            Endian::Big => self.read_i32_be(),
        }
    }

    /// Read a `u64` with the given byte order.
    fn read_u64(&mut self, e: Endian) -> Result<u64, IoError> {
        match e {
            Endian::Little => self.read_u64_le(),
            Endian::Big => self.read_u64_be(),
        }
    }

    /// Read an `i64` with the given byte order.
    fn read_i64(&mut self, e: Endian) -> Result<i64, IoError> {
        match e {
            Endian::Little => self.read_i64_le(),
            Endian::Big => self.read_i64_be(),
        }
    }

    /// Read an `f32` with the given byte order.
    fn read_f32(&mut self, e: Endian) -> Result<f32, IoError> {
        match e {
            Endian::Little => self.read_f32_le(),
            Endian::Big => self.read_f32_be(),
        }
    }

    /// Read an `f64` with the given byte order.
    fn read_f64(&mut self, e: Endian) -> Result<f64, IoError> {
        match e {
            Endian::Little => self.read_f64_le(),
            Endian::Big => self.read_f64_be(),
        }
    }
}

/// Endian-dispatched writing — adds `write_<T>(v, endian)` methods to any
/// [`WriteLe`] + [`WriteBe`] implementor.
///
/// ```
/// use nostdio::{MutCursor, Endian, WriteEndian};
///
/// let mut buf = Vec::new();
/// let mut cur = MutCursor::new(&mut buf);
/// cur.write_u32(1, Endian::Big).unwrap();
/// assert_eq!(buf, [0x00, 0x00, 0x00, 0x01]);
/// ```
pub trait WriteEndian: WriteLe + WriteBe {
    /// Write a `u8` (endianness is irrelevant, parameter accepted for uniformity).
    fn write_u8(&mut self, v: u8, _e: Endian) -> Result<(), IoError> {
        self.write_u8_le(v)
    }

    /// Write an `i8` (endianness is irrelevant, parameter accepted for uniformity).
    fn write_i8(&mut self, v: i8, _e: Endian) -> Result<(), IoError> {
        self.write_i8_le(v)
    }

    /// Write a `u16` with the given byte order.
    fn write_u16(&mut self, v: u16, e: Endian) -> Result<(), IoError> {
        match e {
            Endian::Little => self.write_u16_le(v),
            Endian::Big => self.write_u16_be(v),
        }
    }

    /// Write an `i16` with the given byte order.
    fn write_i16(&mut self, v: i16, e: Endian) -> Result<(), IoError> {
        match e {
            Endian::Little => self.write_i16_le(v),
            Endian::Big => self.write_i16_be(v),
        }
    }

    /// Write a `u32` with the given byte order.
    fn write_u32(&mut self, v: u32, e: Endian) -> Result<(), IoError> {
        match e {
            Endian::Little => self.write_u32_le(v),
            Endian::Big => self.write_u32_be(v),
        }
    }

    /// Write an `i32` with the given byte order.
    fn write_i32(&mut self, v: i32, e: Endian) -> Result<(), IoError> {
        match e {
            Endian::Little => self.write_i32_le(v),
            Endian::Big => self.write_i32_be(v),
        }
    }

    /// Write a `u64` with the given byte order.
    fn write_u64(&mut self, v: u64, e: Endian) -> Result<(), IoError> {
        match e {
            Endian::Little => self.write_u64_le(v),
            Endian::Big => self.write_u64_be(v),
        }
    }

    /// Write an `i64` with the given byte order.
    fn write_i64(&mut self, v: i64, e: Endian) -> Result<(), IoError> {
        match e {
            Endian::Little => self.write_i64_le(v),
            Endian::Big => self.write_i64_be(v),
        }
    }

    /// Write an `f32` with the given byte order.
    fn write_f32(&mut self, v: f32, e: Endian) -> Result<(), IoError> {
        match e {
            Endian::Little => self.write_f32_le(v),
            Endian::Big => self.write_f32_be(v),
        }
    }

    /// Write an `f64` with the given byte order.
    fn write_f64(&mut self, v: f64, e: Endian) -> Result<(), IoError> {
        match e {
            Endian::Little => self.write_f64_le(v),
            Endian::Big => self.write_f64_be(v),
        }
    }
}

impl<T: WriteLe + WriteBe + ?Sized> WriteEndian for T {}

impl<T: ReadLe + ReadBe + ?Sized> ReadEndian for T {}
