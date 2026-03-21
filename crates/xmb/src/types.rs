//! Core XMB data types.

pub use bdt::{Attribute, Node};

use crate::error::{Error, Result};
use bdt::Variant;
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};
use quick_xml::{Reader, Writer};
use std::io::{BufRead, Cursor, Write};

/// XMB format variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum XmbFormat {
    /// Xbox 360 format (big-endian, 28-byte nodes).
    Xbox360,
    /// PC format (little-endian, 48-byte nodes).
    #[default]
    PC,
}

impl XmbFormat {
    pub fn is_xbox360(&self) -> bool {
        matches!(self, XmbFormat::Xbox360)
    }

    pub fn is_pc(&self) -> bool {
        matches!(self, XmbFormat::PC)
    }
}

/// XMB document data.
#[derive(Debug, Clone, Default)]
pub struct XmbData {
    pub root: Option<Node>,
    pub format: XmbFormat,
    pub source_file: Option<String>,
}

impl XmbData {
    pub fn new() -> Self {
        Self {
            root: None,
            format: XmbFormat::PC,
            source_file: None,
        }
    }

    pub fn with_root(root: Node) -> Self {
        Self {
            root: Some(root),
            format: XmbFormat::PC,
            source_file: None,
        }
    }

    pub fn format(&self) -> XmbFormat {
        self.format
    }

    pub fn set_format(&mut self, format: XmbFormat) {
        self.format = format;
    }

    pub fn is_xbox360(&self) -> bool {
        self.format.is_xbox360()
    }

    pub fn is_pc(&self) -> bool {
        self.format.is_pc()
    }

    pub fn set_root(&mut self, root: Node) {
        self.root = Some(root);
    }

    pub fn root(&self) -> Option<&Node> {
        self.root.as_ref()
    }

    pub fn root_mut(&mut self) -> Option<&mut Node> {
        self.root.as_mut()
    }

    pub fn to_xml(&self) -> String {
        let mut buffer = Cursor::new(Vec::new());
        self.write_xml_to(&mut buffer)
            .expect("Failed to write XML to buffer");
        String::from_utf8(buffer.into_inner()).expect("Invalid UTF-8 in XML output")
    }

    pub fn write_xml_to<W: Write>(&self, writer: &mut W) -> Result<()> {
        let mut xml_writer = Writer::new_with_indent(writer, b' ', 4);

        xml_writer
            .write_event(Event::Decl(BytesDecl::new("1.0", Some("utf-8"), None)))
            .map_err(|e| Error::Io(std::io::Error::other(e)))?;

        xml_writer.get_mut().write_all(b"\n").map_err(Error::Io)?;

        if let Some(root) = &self.root {
            write_node_xml(root, &mut xml_writer)?;
        }

        Ok(())
    }

    pub fn from_xml(xml: &str) -> Result<Self> {
        Self::from_xml_reader(xml.as_bytes())
    }

    pub fn from_xml_reader<R: BufRead>(reader: R) -> Result<Self> {
        let mut xml_reader = Reader::from_reader(reader);

        let mut buf = Vec::new();
        let mut root: Option<Node> = None;
        let mut stack: Vec<Node> = Vec::new();

        loop {
            match xml_reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) => {
                    let node = parse_start_element(e)?;
                    stack.push(node);
                }
                Ok(Event::Empty(ref e)) => {
                    let node = parse_start_element(e)?;
                    if let Some(parent) = stack.last_mut() {
                        parent.children.push(node);
                    } else {
                        root = Some(node);
                    }
                }
                Ok(Event::End(_)) => {
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
                Ok(Event::Text(ref e)) => {
                    let text = e
                        .unescape()
                        .map_err(|err| Error::InvalidString(err.to_string()))?;

                    if let Some(node) = stack.last_mut() {
                        match &node.text {
                            Variant::Null => {
                                if !text.trim().is_empty() {
                                    node.text = Variant::String(text.into_owned());
                                }
                            }
                            Variant::String(existing) => {
                                node.text = Variant::String(format!("{}{}", existing, text));
                            }
                            _ => {
                                let existing = node.text_string();
                                node.text = Variant::String(format!("{}{}", existing, text));
                            }
                        }
                    }
                }
                Ok(Event::CData(ref e)) => {
                    let text = String::from_utf8_lossy(e.as_ref()).to_string();
                    if !text.is_empty()
                        && let Some(node) = stack.last_mut()
                    {
                        node.text = Variant::String(text);
                    }
                }
                Ok(Event::Eof) => break,
                Ok(_) => {}
                Err(e) => return Err(Error::InvalidString(format!("XML parse error: {}", e))),
            }
            buf.clear();
        }

        Ok(XmbData {
            root,
            format: XmbFormat::PC,
            source_file: None,
        })
    }
}

// ============================================================================
// XML helpers
// ============================================================================

fn write_node_xml<W: Write>(node: &Node, writer: &mut Writer<W>) -> Result<()> {
    let has_text = !matches!(node.text, Variant::Null);
    let has_children = !node.children.is_empty();

    let mut elem = BytesStart::new(&node.name);
    for attr in &node.attributes {
        elem.push_attribute((attr.name.as_str(), attr.value.to_string_value().as_str()));
    }

    if !has_text && !has_children {
        writer
            .write_event(Event::Empty(elem))
            .map_err(|e| Error::Io(std::io::Error::other(e)))?;
    } else {
        writer
            .write_event(Event::Start(elem.borrow()))
            .map_err(|e| Error::Io(std::io::Error::other(e)))?;

        if has_text {
            let text = node.text.to_string_value();
            writer
                .write_event(Event::Text(BytesText::new(&text)))
                .map_err(|e| Error::Io(std::io::Error::other(e)))?;
        }

        for child in &node.children {
            write_node_xml(child, writer)?;
        }

        writer
            .write_event(Event::End(BytesEnd::new(&node.name)))
            .map_err(|e| Error::Io(std::io::Error::other(e)))?;
    }

    Ok(())
}

fn parse_start_element(e: &BytesStart) -> Result<Node> {
    let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
    let mut node = Node::new(name);

    for attr_result in e.attributes() {
        let attr =
            attr_result.map_err(|e| Error::InvalidString(format!("Attribute error: {}", e)))?;
        let attr_name = String::from_utf8_lossy(attr.key.as_ref()).to_string();
        let attr_value = attr
            .unescape_value()
            .map_err(|e| Error::InvalidString(format!("Attribute value error: {}", e)))?
            .to_string();
        node.attributes.push(Attribute {
            name: attr_name,
            value: parse_text_value(&attr_value),
        });
    }

    Ok(node)
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
            let floats: std::result::Result<Vec<f32>, _> =
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
            return Variant::UInt(v as u32);
        } else {
            return Variant::Int(v);
        }
    }

    if let Ok(v) = s.parse::<f32>() {
        if let Ok(d) = s.parse::<f64>() {
            let f32_back = v as f64;
            if (d - f32_back).abs() > 1e-6 {
                return Variant::Double(d);
            }
        }
        return Variant::Float(v);
    }

    if s.is_ascii() {
        Variant::String(s.to_string())
    } else {
        Variant::UString(s.to_string())
    }
}
