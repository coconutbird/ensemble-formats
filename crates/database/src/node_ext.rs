//! Extension helpers for navigating [`bdt::Node`] trees.

use alloc::string::String;
use alloc::vec::Vec;
use bdt::Node;

/// Extension trait for convenient typed access to [`Node`] children and attributes.
pub trait NodeExt {
    /// Find the first child with the given tag name.
    fn child(&self, name: &str) -> Option<&Node>;

    /// Find all children with the given tag name.
    fn children_named(&self, name: &str) -> Vec<&Node>;

    /// Get the text content of the first child with the given tag name.
    fn child_text(&self, name: &str) -> Option<String>;

    /// Get the text content as f32.
    fn child_f32(&self, name: &str) -> Option<f32>;

    /// Get the text content as i32.
    fn child_i32(&self, name: &str) -> Option<i32>;

    /// Get the text content as u32.
    fn child_u32(&self, name: &str) -> Option<u32>;

    /// Get the text content as bool.
    fn child_bool(&self, name: &str) -> Option<bool>;

    /// Get an attribute value as a string.
    fn attr_str(&self, name: &str) -> Option<String>;

    /// Get an attribute value as f32.
    fn attr_f32(&self, name: &str) -> Option<f32>;

    /// Get an attribute value as i32.
    fn attr_i32(&self, name: &str) -> Option<i32>;

    /// Get an attribute value as bool.
    fn attr_bool(&self, name: &str) -> Option<bool>;
}

impl NodeExt for Node {
    fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }

    fn children_named(&self, name: &str) -> Vec<&Node> {
        self.children.iter().filter(|c| c.name == name).collect()
    }

    fn child_text(&self, name: &str) -> Option<String> {
        let c = self.child(name)?;
        let s = c.text_string();
        if s.is_empty() { None } else { Some(s) }
    }

    fn child_f32(&self, name: &str) -> Option<f32> {
        let c = self.child(name)?;
        c.text.as_float()
    }

    fn child_i32(&self, name: &str) -> Option<i32> {
        let c = self.child(name)?;
        c.text.as_int()
    }

    fn child_u32(&self, name: &str) -> Option<u32> {
        self.child_i32(name).map(|v| v as u32)
    }

    fn child_bool(&self, name: &str) -> Option<bool> {
        let c = self.child(name)?;
        c.text.as_bool()
    }

    fn attr_str(&self, name: &str) -> Option<String> {
        let a = self.get_attribute(name)?;
        let s = a.value_string();
        if s.is_empty() { None } else { Some(s) }
    }

    fn attr_f32(&self, name: &str) -> Option<f32> {
        self.get_attribute(name)?.value.as_float()
    }

    fn attr_i32(&self, name: &str) -> Option<i32> {
        self.get_attribute(name)?.value.as_int()
    }

    fn attr_bool(&self, name: &str) -> Option<bool> {
        self.get_attribute(name)?.value.as_bool()
    }
}

/// Helper: get root node from document, or return error.
pub fn root_node(doc: &xmb::Document) -> crate::Result<&Node> {
    doc.root().ok_or(crate::Error::MissingRoot)
}

/// Helper: validate root element name.
pub fn expect_root<'a>(doc: &'a xmb::Document, expected: &str) -> crate::Result<&'a Node> {
    let root = root_node(doc)?;
    if root.name != expected {
        return Err(crate::Error::UnexpectedRoot {
            expected: String::from(expected),
            actual: root.name.clone(),
        });
    }
    Ok(root)
}
