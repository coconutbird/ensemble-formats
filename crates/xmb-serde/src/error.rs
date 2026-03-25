//! Error types for xmb-serde.

use core::fmt;

/// Errors that can occur when deserializing an XMB file.
#[derive(Debug)]
pub enum Error {
    /// XMB parsing failed (ECF, BDT, or XML layer).
    Xmb(xmb::Error),
    /// Serde deserialization of the node tree failed.
    Deserialize(bdt_serde::Error),
    /// The XMB document has no root node.
    EmptyDocument,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Xmb(e) => write!(f, "XMB parse error: {e}"),
            Error::Deserialize(e) => write!(f, "deserialize error: {e}"),
            Error::EmptyDocument => f.write_str("XMB document has no root node"),
        }
    }
}

impl From<xmb::Error> for Error {
    fn from(e: xmb::Error) -> Self {
        Error::Xmb(e)
    }
}

#[cfg(feature = "std")]
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Xmb(e) => Some(e),
            Error::Deserialize(e) => Some(e),
            Error::EmptyDocument => None,
        }
    }
}

#[cfg(not(feature = "std"))]
impl core::error::Error for Error {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Error::Xmb(e) => Some(e),
            Error::Deserialize(e) => Some(e),
            Error::EmptyDocument => None,
        }
    }
}
