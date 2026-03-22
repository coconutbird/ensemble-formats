//! Core node types for the BBinaryDataTree format.
//!
//! A BDT document is a tree of [`Node`]s. Each node has a name, an optional
//! text value, zero or more [`Attribute`]s, and zero or more child nodes.
//!
//! ```ignore
//! use bdt::{Node, Attribute, Variant};
//!
//! let mut root = Node::new("Config");
//! root.add_attribute(Attribute::with_string("version", "1.0"));
//! root.add_child(Node::with_text("Entry", "hello"));
//! assert_eq!(root.node_count(), 2);
//! ```

use alloc::string::String;
use alloc::vec::Vec;

use crate::variant::Variant;
use crate::{Endian, Result};

/// A named attribute on a [`Node`].
///
/// Attributes are key-value pairs where the key is a UTF-8 string and the
/// value is a [`Variant`]. In the original Ensemble format these correspond
/// to XML attributes.
#[derive(Debug, Clone, Default)]
pub struct Attribute {
    /// Attribute name (e.g. `"Name"`, `"Type"`).
    pub name: String,
    /// Attribute value, stored as a typed [`Variant`].
    pub value: Variant,
}

impl Attribute {
    /// Create an attribute with an arbitrary [`Variant`] value.
    pub fn new(name: impl Into<String>, value: Variant) -> Self {
        Self {
            name: name.into(),
            value,
        }
    }

    /// Convenience: create an attribute whose value is a [`Variant::String`].
    pub fn with_string(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: Variant::String(value.into()),
        }
    }

    /// Return the value formatted as a string (delegates to [`Variant::to_string_value`]).
    pub fn value_string(&self) -> String {
        self.value.to_string_value()
    }
}

/// A node in the binary data tree.
///
/// Mirrors an XML element: `name` is the tag name, `text` is the inner text
/// content, `attributes` are the XML attributes, and `children` are nested
/// elements.
#[derive(Debug, Clone, Default)]
pub struct Node {
    /// Element / tag name.
    pub name: String,
    /// Inner text content (often [`Variant::Null`] when absent).
    pub text: Variant,
    /// Key-value attributes on this node.
    pub attributes: Vec<Attribute>,
    /// Child nodes.
    pub children: Vec<Node>,
}

impl Node {
    /// Create an empty node with the given name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            text: Variant::Null,
            attributes: Vec::new(),
            children: Vec::new(),
        }
    }

    /// Create a node with a string text value.
    pub fn with_text(name: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            text: Variant::String(text.into()),
            attributes: Vec::new(),
            children: Vec::new(),
        }
    }

    /// Append an attribute to this node.
    pub fn add_attribute(&mut self, attr: Attribute) {
        self.attributes.push(attr);
    }

    /// Append a child node.
    pub fn add_child(&mut self, child: Node) {
        self.children.push(child);
    }

    /// Look up an attribute by name, returning `None` if not found.
    pub fn get_attribute(&self, name: &str) -> Option<&Attribute> {
        self.attributes.iter().find(|a| a.name == name)
    }

    /// Return the text value formatted as a string.
    pub fn text_string(&self) -> String {
        self.text.to_string_value()
    }

    /// Returns `true` if this node has any children.
    pub fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    /// Returns `true` if this node has any attributes.
    pub fn has_attributes(&self) -> bool {
        !self.attributes.is_empty()
    }

    /// Count this node plus all descendants recursively.
    pub fn node_count(&self) -> usize {
        1 + self.children.iter().map(|c| c.node_count()).sum::<usize>()
    }

    /// Parse a node tree from a byte slice with the given endianness.
    pub fn from_bytes(data: &[u8], endian: Endian) -> Result<Option<Self>> {
        crate::Reader::read(data, endian)
    }

    /// Serialize this node tree to bytes with the given endianness.
    pub fn to_bytes(&self, endian: Endian) -> Result<Vec<u8>> {
        crate::Writer::write(self, endian)
    }
}
