//! XML ↔ XMB conversion.
//!
//! Provides [`to_xml`] and [`from_xml`] conversions between [`Document`] and
//! UTF-8 XML text. Uses the workspace [`xml`] crate for tokenized reading,
//! writing, and entity escaping.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use bdt::Variant;
use xml::escape::escape_into;
use xml::reader::Event;

use crate::document::{Attribute, Document, Format, Node};
use crate::error::{Error, Result};

impl Document {
    /// Serialize this document to a pretty-printed XML string.
    ///
    /// Uses a direct recursive tree-walker instead of the streaming
    /// [`xml::Writer`] so formatting decisions (newlines, indentation) are
    /// driven by each node's structure rather than by buffered state.
    #[must_use]
    pub fn to_xml(&self) -> String {
        let mut buf = String::from("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n");
        if let Some(root) = &self.root {
            serialize_node(root, &mut buf, 0);
        }
        buf
    }

    /// Parse an XML string into a [`Document`].
    ///
    /// # Errors
    ///
    /// Returns an error if the XML tokenizer encounters malformed input.
    pub fn from_xml(input: &str) -> Result<Self> {
        let mut root: Option<Node> = None;
        let mut stack: Vec<Node> = Vec::new();
        let mut current_node: Option<Node> = None;
        let reader = xml::Reader::new(input);

        for result in reader {
            let event = result.map_err(|e| Error::Xml(format!("{e}")))?;

            match event {
                Event::ElementStart { name } => {
                    if let Some(node) = current_node.take() {
                        stack.push(node);
                    }
                    current_node = Some(Node::new(&name));
                }
                Event::Attribute { name, value } => {
                    if let Some(ref mut node) = current_node {
                        node.attributes.push(Attribute {
                            name,
                            value: parse_text_value(&value),
                        });
                    }
                }
                Event::ElementOpen => {
                    if let Some(node) = current_node.take() {
                        stack.push(node);
                    }
                }
                Event::ElementClose { .. } => {
                    if let Some(mut node) = stack.pop() {
                        if let Variant::String(ref s) = node.text {
                            node.text = parse_text_value(s);
                        }
                        if let Some(parent) = stack.last_mut() {
                            parent.children.push(node);
                        } else {
                            root = Some(node);
                        }
                    }
                }
                Event::ElementEmpty => {
                    if let Some(node) = current_node.take() {
                        if let Some(parent) = stack.last_mut() {
                            parent.children.push(node);
                        } else {
                            root = Some(node);
                        }
                    }
                }
                Event::Text(text) => {
                    if let Some(node) = stack.last_mut() {
                        match &node.text {
                            Variant::Null => {
                                if !text.trim().is_empty() {
                                    node.text = Variant::String(text);
                                }
                            }
                            Variant::String(existing) => {
                                node.text = Variant::String(format!("{existing}{text}"));
                            }
                            _ => {
                                let existing = node.text_string();
                                node.text = Variant::String(format!("{existing}{text}"));
                            }
                        }
                    }
                }
                Event::Cdata(s) => {
                    if !s.is_empty()
                        && let Some(node) = stack.last_mut()
                    {
                        node.text = Variant::String(s);
                    }
                }
            }
        }

        Ok(Document {
            root,
            format: Format::PC,
            source_file: None,
        })
    }
}

/// Recursively serialize a node into `buf` with `depth`-level indentation.
///
/// Three cases:
/// - **Empty** (`<tag/>`) — no text, no children.
/// - **Text-only** (`<tag>text</tag>`) — inline, single line.
/// - **Children** — opening tag on its own line, children indented, closing
///   tag on its own line.
fn serialize_node(node: &Node, buf: &mut String, depth: usize) {
    let indent = "    ".repeat(depth);

    // Opening tag + attributes
    buf.push_str(&indent);
    buf.push('<');
    buf.push_str(&node.name);
    for attr in &node.attributes {
        buf.push(' ');
        buf.push_str(&attr.name);
        buf.push_str("=\"");
        escape_into(buf, &attr.value.to_string_value());
        buf.push('"');
    }

    let has_text = !matches!(node.text, Variant::Null);
    let has_children = !node.children.is_empty();

    if !has_text && !has_children {
        // <tag/>
        buf.push_str("/>\n");
    } else if has_children {
        // <tag>\n  children \n</tag>
        buf.push_str(">\n");
        for child in &node.children {
            serialize_node(child, buf, depth + 1);
        }
        buf.push_str(&indent);
        buf.push_str("</");
        buf.push_str(&node.name);
        buf.push_str(">\n");
    } else {
        // <tag>text</tag>  (inline, single line)
        buf.push('>');
        escape_into(buf, &node.text.to_string_value());
        buf.push_str("</");
        buf.push_str(&node.name);
        buf.push_str(">\n");
    }
}

fn parse_text_value(s: &str) -> Variant {
    if s.is_empty() {
        return Variant::Null;
    }

    if s.eq_ignore_ascii_case("true") {
        return Variant::Bool(true);
    }
    if s.eq_ignore_ascii_case("false") {
        return Variant::Bool(false);
    }

    if s.contains(',') {
        let parts: Vec<&str> = s.split(',').collect();
        if parts.len() >= 2 && parts.len() <= 4 {
            let floats: core::result::Result<Vec<f32>, _> =
                parts.iter().map(|p| p.trim().parse::<f32>()).collect();
            if let Ok(vec) = floats {
                return Variant::FloatVec(vec);
            }
        }
    }

    if let Some(hex_str) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X"))
        && let Ok(v) = u32::from_str_radix(hex_str, 16)
    {
        return Variant::UInt(v);
    }

    if let Ok(v) = s.parse::<i32>() {
        if v >= 0 {
            return Variant::UInt(v.cast_unsigned());
        }
        return Variant::Int(v);
    }

    if let Ok(v) = s.parse::<f32>() {
        if let Ok(d) = s.parse::<f64>() {
            let f32_back = f64::from(v);
            if (d - f32_back).abs() > 1e-6 {
                return Variant::Double(d);
            }
        }
        return Variant::Float(v);
    }

    if s.is_ascii() {
        Variant::String(String::from(s))
    } else {
        Variant::UString(String::from(s))
    }
}
