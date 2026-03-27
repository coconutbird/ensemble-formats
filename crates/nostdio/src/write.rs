//! Convenience traits for writing little-endian and big-endian primitives.
//!
//! [`WriteLe`] and [`WriteBe`] are blanket-implemented for every [`Write`]
//! type, so importing the trait is all that's needed:
//!
//! ```
//! use nostdio::{MutCursor, WriteLe};
//!
//! let mut buf = Vec::new();
//! let mut cur = MutCursor::new(&mut buf);
//! cur.write_u32_le(1).unwrap();
//! assert_eq!(buf, [0x01, 0x00, 0x00, 0x00]);
//! ```

use crate::traits::{IoError, Write};

/// Convenience methods for writing little-endian primitives.
///
/// Automatically available on any [`Write`] implementor.
///
/// # Example
///
/// ```
/// use nostdio::{MutCursor, WriteLe};
///
/// let mut buf = Vec::new();
/// let mut cur = MutCursor::new(&mut buf);
///
/// cur.write_u8_le(0xFF).unwrap();
/// cur.write_u16_le(1000).unwrap();
/// cur.write_f32_le(3.14).unwrap();
///
/// assert_eq!(buf.len(), 1 + 2 + 4);
/// ```
pub trait WriteLe: Write {
    /// Write a `u8` in little-endian byte order (identity, but named for consistency).
    fn write_u8_le(&mut self, v: u8) -> Result<(), IoError> {
        self.write_all(&[v])
    }

    /// Write an `i8` in little-endian byte order (identity, but named for consistency).
    fn write_i8_le(&mut self, v: i8) -> Result<(), IoError> {
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
///
/// Automatically available on any [`Write`] implementor.
///
/// # Example
///
/// ```
/// use nostdio::{MutCursor, WriteBe};
///
/// let mut buf = Vec::new();
/// let mut cur = MutCursor::new(&mut buf);
///
/// cur.write_u32_be(1).unwrap();
/// assert_eq!(buf, [0x00, 0x00, 0x00, 0x01]);
/// ```
pub trait WriteBe: Write {
    /// Write a `u8` in big-endian byte order (identity, but named for consistency).
    fn write_u8_be(&mut self, v: u8) -> Result<(), IoError> {
        self.write_all(&[v])
    }

    /// Write an `i8` in big-endian byte order (identity, but named for consistency).
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
