//! XMB document model.
//!
//! The [`Document`] type holds a parsed XMB tree in memory. It wraps a
//! [`bdt::Node`] root with format metadata ([`Format`]) and an optional
//! source filename.
//!
//! Construct a document from scratch with [`Document::with_root`], or parse
//! one from bytes via [`crate::Reader::read`] / from XML via
//! [`Document::from_xml`].

use alloc::string::String;
use alloc::vec::Vec;

use crate::Result;

pub use bdt::{Attribute, Node};

/// XMB format variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Format {
    /// Xbox 360 format (big-endian, 28-byte nodes).
    Xbox360,
    /// PC / Definitive Edition format (little-endian, 48-byte nodes).
    #[default]
    PC,
}

impl Format {
    /// Returns `true` if this is the Xbox 360 (big-endian) format.
    #[must_use]
    pub fn is_xbox360(&self) -> bool {
        matches!(self, Format::Xbox360)
    }

    /// Returns `true` if this is the PC (little-endian) format.
    #[must_use]
    pub fn is_pc(&self) -> bool {
        matches!(self, Format::PC)
    }
}

/// An XMB document — a tree of [`Node`]s with format metadata.
#[derive(Debug, Clone, Default)]
pub struct Document {
    /// The root node of the document tree, or `None` for an empty document.
    pub root: Option<Node>,
    /// The binary format this document was read from (or should be written as).
    pub format: Format,
    /// Optional source filename for diagnostics.
    pub source_file: Option<String>,
}

impl Document {
    /// Create an empty document (PC format, no root).
    #[must_use]
    pub fn new() -> Self {
        Self {
            root: None,
            format: Format::PC,
            source_file: None,
        }
    }

    /// Create a document with the given root node (PC format).
    #[must_use]
    pub fn with_root(root: Node) -> Self {
        Self {
            root: Some(root),
            format: Format::PC,
            source_file: None,
        }
    }

    /// Returns the binary format of this document.
    #[must_use]
    pub fn format(&self) -> Format {
        self.format
    }

    /// Set the binary format.
    pub fn set_format(&mut self, format: Format) {
        self.format = format;
    }

    /// Returns `true` if this document uses the Xbox 360 format.
    #[must_use]
    pub fn is_xbox360(&self) -> bool {
        self.format.is_xbox360()
    }

    /// Returns `true` if this document uses the PC format.
    #[must_use]
    pub fn is_pc(&self) -> bool {
        self.format.is_pc()
    }

    /// Set the root node.
    pub fn set_root(&mut self, root: Node) {
        self.root = Some(root);
    }

    /// Returns a reference to the root node, if present.
    #[must_use]
    pub fn root(&self) -> Option<&Node> {
        self.root.as_ref()
    }

    /// Returns a mutable reference to the root node, if present.
    pub fn root_mut(&mut self) -> Option<&mut Node> {
        self.root.as_mut()
    }

    /// Parse a document from a byte slice, auto-detecting XML vs binary.
    ///
    /// # Errors
    ///
    /// Returns an error if the XML or binary XMB input is malformed,
    /// truncated, or fails checksum validation.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        crate::Reader::read(data)
    }

    /// Serialize this document to bytes in its native format (compressed).
    ///
    /// # Errors
    ///
    /// Returns an error if the document exceeds an on-disk size limit or its
    /// BDT tree cannot be serialized.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        crate::Writer::write_native(self)
    }

    /// Serialize this document to bytes with explicit format.
    ///
    /// # Errors
    ///
    /// Returns an error if the document exceeds an on-disk size limit or its
    /// BDT tree cannot be serialized.
    pub fn to_bytes_with_format(&self, format: Format) -> Result<Vec<u8>> {
        crate::Writer::write(self, format)
    }
}
