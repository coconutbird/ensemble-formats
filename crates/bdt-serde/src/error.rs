//! Error type for the bdt-serde deserializer.

use alloc::string::{String, ToString};
use core::fmt;

/// Errors that can occur during deserialization of a [`bdt::Node`] tree.
#[derive(Debug, Clone)]
pub struct Error {
    msg: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.msg)
    }
}

impl Error {
    /// Create a new error with the given message.
    pub fn new(msg: impl Into<String>) -> Self {
        Self { msg: msg.into() }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for Error {}

#[cfg(not(feature = "std"))]
impl core::error::Error for Error {}

impl serde::de::Error for Error {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        Self {
            msg: msg.to_string(),
        }
    }
}

impl serde::ser::Error for Error {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        Self {
            msg: msg.to_string(),
        }
    }
}
