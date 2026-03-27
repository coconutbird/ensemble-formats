//! Convenience traits for reading little-endian and big-endian primitives.
//!
//! [`ReadLe`] and [`ReadBe`] are blanket-implemented for every [`Read`]
//! type, so importing the trait is all that's needed:
//!
//! ```
//! use nostdio::{SliceCursor, ReadLe};
//!
//! let data = 42.0_f32.to_le_bytes();
//! let mut cur = SliceCursor::new(&data);
//! assert_eq!(cur.read_f32_le().unwrap(), 42.0);
//! ```

use crate::traits::{IoError, Read};

/// Convenience methods for reading little-endian primitives.
///
/// Automatically available on any [`Read`] implementor.
///
/// # Example
///
/// ```
/// use nostdio::{SliceCursor, ReadLe};
///
/// let data = [0x01, 0x00, 0x00, 0x00, 0xFF];
/// let mut cur = SliceCursor::new(&data);
///
/// assert_eq!(cur.read_u32_le().unwrap(), 1);
/// assert_eq!(cur.read_u8_le().unwrap(), 0xFF);
/// ```
pub trait ReadLe: Read {
    /// Read a `u8` in little-endian byte order (identity, but named for consistency).
    fn read_u8_le(&mut self) -> Result<u8, IoError> {
        let mut buf = [0u8; 1];
        self.read_exact(&mut buf)?;
        Ok(buf[0])
    }

    /// Read an `i8` in little-endian byte order (identity, but named for consistency).
    fn read_i8_le(&mut self) -> Result<i8, IoError> {
        Ok(self.read_u8_le()? as i8)
    }

    /// Read a `u16` in little-endian byte order.
    fn read_u16_le(&mut self) -> Result<u16, IoError> {
        let mut buf = [0u8; 2];
        self.read_exact(&mut buf)?;
        Ok(u16::from_le_bytes(buf))
    }

    /// Read an `i16` in little-endian byte order.
    fn read_i16_le(&mut self) -> Result<i16, IoError> {
        let mut buf = [0u8; 2];
        self.read_exact(&mut buf)?;
        Ok(i16::from_le_bytes(buf))
    }

    /// Read a `u32` in little-endian byte order.
    fn read_u32_le(&mut self) -> Result<u32, IoError> {
        let mut buf = [0u8; 4];
        self.read_exact(&mut buf)?;
        Ok(u32::from_le_bytes(buf))
    }

    /// Read an `i32` in little-endian byte order.
    fn read_i32_le(&mut self) -> Result<i32, IoError> {
        let mut buf = [0u8; 4];
        self.read_exact(&mut buf)?;
        Ok(i32::from_le_bytes(buf))
    }

    /// Read a `u64` in little-endian byte order.
    fn read_u64_le(&mut self) -> Result<u64, IoError> {
        let mut buf = [0u8; 8];
        self.read_exact(&mut buf)?;
        Ok(u64::from_le_bytes(buf))
    }

    /// Read an `i64` in little-endian byte order.
    fn read_i64_le(&mut self) -> Result<i64, IoError> {
        let mut buf = [0u8; 8];
        self.read_exact(&mut buf)?;
        Ok(i64::from_le_bytes(buf))
    }

    /// Read an `f32` in little-endian byte order.
    fn read_f32_le(&mut self) -> Result<f32, IoError> {
        let mut buf = [0u8; 4];
        self.read_exact(&mut buf)?;
        Ok(f32::from_le_bytes(buf))
    }

    /// Read an `f64` in little-endian byte order.
    fn read_f64_le(&mut self) -> Result<f64, IoError> {
        let mut buf = [0u8; 8];
        self.read_exact(&mut buf)?;
        Ok(f64::from_le_bytes(buf))
    }
}

impl<T: Read + ?Sized> ReadLe for T {}

/// Convenience methods for reading big-endian primitives.
///
/// Automatically available on any [`Read`] implementor.
///
/// # Example
///
/// ```
/// use nostdio::{SliceCursor, ReadBe};
///
/// // 0x00000001 in big-endian
/// let data = [0x00, 0x00, 0x00, 0x01];
/// let mut cur = SliceCursor::new(&data);
///
/// assert_eq!(cur.read_u32_be().unwrap(), 1);
/// ```
pub trait ReadBe: Read {
    /// Read a `u8` in big-endian byte order (identity, but named for consistency).
    fn read_u8_be(&mut self) -> Result<u8, IoError> {
        let mut buf = [0u8; 1];
        self.read_exact(&mut buf)?;
        Ok(buf[0])
    }

    /// Read an `i8` in big-endian byte order (identity, but named for consistency).
    fn read_i8_be(&mut self) -> Result<i8, IoError> {
        Ok(self.read_u8_be()? as i8)
    }

    /// Read a `u16` in big-endian byte order.
    fn read_u16_be(&mut self) -> Result<u16, IoError> {
        let mut buf = [0u8; 2];
        self.read_exact(&mut buf)?;
        Ok(u16::from_be_bytes(buf))
    }

    /// Read an `i16` in big-endian byte order.
    fn read_i16_be(&mut self) -> Result<i16, IoError> {
        let mut buf = [0u8; 2];
        self.read_exact(&mut buf)?;
        Ok(i16::from_be_bytes(buf))
    }

    /// Read a `u32` in big-endian byte order.
    fn read_u32_be(&mut self) -> Result<u32, IoError> {
        let mut buf = [0u8; 4];
        self.read_exact(&mut buf)?;
        Ok(u32::from_be_bytes(buf))
    }

    /// Read an `i32` in big-endian byte order.
    fn read_i32_be(&mut self) -> Result<i32, IoError> {
        let mut buf = [0u8; 4];
        self.read_exact(&mut buf)?;
        Ok(i32::from_be_bytes(buf))
    }

    /// Read a `u64` in big-endian byte order.
    fn read_u64_be(&mut self) -> Result<u64, IoError> {
        let mut buf = [0u8; 8];
        self.read_exact(&mut buf)?;
        Ok(u64::from_be_bytes(buf))
    }

    /// Read an `i64` in big-endian byte order.
    fn read_i64_be(&mut self) -> Result<i64, IoError> {
        let mut buf = [0u8; 8];
        self.read_exact(&mut buf)?;
        Ok(i64::from_be_bytes(buf))
    }

    /// Read an `f32` in big-endian byte order.
    fn read_f32_be(&mut self) -> Result<f32, IoError> {
        let mut buf = [0u8; 4];
        self.read_exact(&mut buf)?;
        Ok(f32::from_be_bytes(buf))
    }

    /// Read an `f64` in big-endian byte order.
    fn read_f64_be(&mut self) -> Result<f64, IoError> {
        let mut buf = [0u8; 8];
        self.read_exact(&mut buf)?;
        Ok(f64::from_be_bytes(buf))
    }
}

impl<T: Read + ?Sized> ReadBe for T {}

/// Read a null-terminated UTF-8 string from a byte slice.
///
/// Returns the string up to (but not including) the first NUL byte,
/// or the entire slice if no NUL is found.  Invalid UTF-8 sequences
/// are replaced with the Unicode replacement character (U+FFFD).
///
/// # Examples
///
/// ```
/// use nostdio::read_null_terminated_string;
///
/// assert_eq!(read_null_terminated_string(b"hello\0world"), "hello");
/// assert_eq!(read_null_terminated_string(b"no null"), "no null");
/// assert_eq!(read_null_terminated_string(b"\0"), "");
/// ```
pub fn read_null_terminated_string(data: &[u8]) -> alloc::string::String {
    let end = data.iter().position(|&b| b == 0).unwrap_or(data.len());
    alloc::string::String::from_utf8_lossy(&data[..end]).into_owned()
}
