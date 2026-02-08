//! Core BBinaryDataTree node types.

use crate::variant::Variant;

/// An attribute on a tree node.
#[derive(Debug, Clone, Default)]
pub struct Attribute {
    pub name: String,
    pub value: Variant,
}

impl Attribute {
    pub fn new(name: impl Into<String>, value: Variant) -> Self {
        Self {
            name: name.into(),
            value,
        }
    }

    pub fn with_string(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: Variant::String(value.into()),
        }
    }

    pub fn value_string(&self) -> String {
        self.value.to_string_value()
    }
}

/// A node in the binary data tree.
#[derive(Debug, Clone, Default)]
pub struct Node {
    pub name: String,
    pub text: Variant,
    pub attributes: Vec<Attribute>,
    pub children: Vec<Node>,
}

impl Node {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            text: Variant::Null,
            attributes: Vec::new(),
            children: Vec::new(),
        }
    }

    pub fn with_text(name: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            text: Variant::String(text.into()),
            attributes: Vec::new(),
            children: Vec::new(),
        }
    }

    pub fn add_attribute(&mut self, attr: Attribute) {
        self.attributes.push(attr);
    }

    pub fn add_child(&mut self, child: Node) {
        self.children.push(child);
    }

    pub fn get_attribute(&self, name: &str) -> Option<&Attribute> {
        self.attributes.iter().find(|a| a.name == name)
    }

    pub fn text_string(&self) -> String {
        self.text.to_string_value()
    }

    pub fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    pub fn has_attributes(&self) -> bool {
        !self.attributes.is_empty()
    }

    pub fn node_count(&self) -> usize {
        1 + self.children.iter().map(|c| c.node_count()).sum::<usize>()
    }
}
