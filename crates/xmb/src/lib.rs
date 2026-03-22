//! XMB binary XML format library for Halo Wars (Ensemble Studios).
//!
//! XMB files are binary representations of XML used in Halo Wars and other
//! Ensemble Studios titles. An XMB file is an ECF container holding a single
//! packed BDT tree prefixed by a 4-byte signature.
//!
//! ```text
//! ┌─────────────────────────────────┐
//! │  ECF container (id = 0xE43ABC00)│
//! │  ┌───────────────────────────┐  │
//! │  │ Chunk 0xA9C96500          │  │
//! │  │  ┌─────────────────────┐  │  │
//! │  │  │ XMB sig (4 bytes)   │  │  │
//! │  │  │ BDT packed tree     │  │  │
//! │  │  └─────────────────────┘  │  │
//! │  └───────────────────────────┘  │
//! └─────────────────────────────────┘
//! ```
//!
//! # Example
//!
//! ```no_run
//! use xmb::{Reader, Document};
//!
//! let data = std::fs::read("example.xmb").unwrap();
//! let doc = Reader::read(&data).unwrap();
//!
//! // Convert to XML
//! let xml = doc.to_xml();
//! println!("{}", xml);
//! ```

#![no_std]
extern crate alloc;

mod document;
mod error;
mod reader;
mod writer;
mod xml;

pub use bdt::Variant;
pub use document::{Attribute, Document, Format, Node};
pub use error::{Error, Result};
pub use reader::Reader;
pub use writer::Writer;

/// XMB signature in the packed data header.
pub const SIGNATURE: u32 = 0x71439800;

/// ECF file ID for XMB containers.
pub const ECF_FILE_ID: u32 = 0xE43ABC00;

/// ECF chunk ID for the packed BDT data.
pub const PACKED_DATA_CHUNK_ID: u64 = 0xA9C96500;

/// ECF chunk ID for file info (unused in practice).
pub const FILE_INFO_CHUNK_ID: u64 = 0xA9C96501;

#[cfg(test)]
mod tests {
    extern crate alloc;
    use alloc::string::ToString;

    use super::*;

    #[test]
    fn test_roundtrip_pc_format() {
        let mut root = Node::new("root");
        root.add_child(Node::with_text("message", "Hello, XMB!"));
        let doc = Document::with_root(root);

        let bytes = Writer::write(&doc, Format::PC).expect("Failed to write");
        let read = Reader::read(&bytes).expect("Failed to read");

        assert!(read.root().is_some());
        let read_root = read.root().unwrap();
        assert_eq!(read_root.name, "root");
        assert_eq!(read_root.children.len(), 1);
        assert_eq!(read_root.children[0].name, "message");
        assert_eq!(read_root.children[0].text_string(), "Hello, XMB!");
    }

    #[test]
    fn test_roundtrip_xbox360_format() {
        let mut root = Node::new("root");
        root.add_child(Node::with_text("message", "Hello, Xbox!"));
        let doc = Document::with_root(root);

        let bytes = Writer::write(&doc, Format::Xbox360).expect("Failed to write");
        let read = Reader::read(&bytes).expect("Failed to read");

        assert!(read.root().is_some());
        let read_root = read.root().unwrap();
        assert_eq!(read_root.name, "root");
        assert_eq!(read_root.children.len(), 1);
        assert_eq!(read_root.children[0].name, "message");
        assert_eq!(read_root.children[0].text_string(), "Hello, Xbox!");
    }

    #[test]
    fn test_xml_to_xmb_to_xml() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<config>
    <setting name="volume" value="50"/>
    <setting name="enabled" value="true"/>
</config>"#;

        let doc = Document::from_xml(xml).expect("Failed to parse XML");

        let bytes = Writer::write(&doc, Format::PC).expect("Failed to write XMB");
        let read = Reader::read(&bytes).expect("Failed to read XMB");
        let result_xml = read.to_xml();

        assert!(result_xml.contains("<config>"));
        assert!(result_xml.contains("<setting"));
        assert!(result_xml.contains("name=\"volume\""));
        assert!(result_xml.contains("value=\"50\""));
    }

    #[test]
    fn test_with_attributes() {
        let mut root = Node::new("element");
        root.add_attribute(Attribute::with_string("id", "test123"));
        root.add_attribute(Attribute::new("count", Variant::UInt(42)));
        let doc = Document::with_root(root);

        let bytes = Writer::write(&doc, Format::PC).expect("Failed to write");
        let read = Reader::read(&bytes).expect("Failed to read");

        let read_root = read.root().unwrap();
        assert_eq!(read_root.attributes.len(), 2);
        assert_eq!(
            read_root.get_attribute("id").unwrap().value_string(),
            "test123"
        );
        assert_eq!(
            read_root.get_attribute("count").unwrap().value_string(),
            "42"
        );
    }

    #[test]
    fn test_nested_nodes() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<level1>
    <level2>
        <level3>
            <leaf>deep value</leaf>
        </level3>
    </level2>
</level1>"#;

        let doc = Document::from_xml(xml).expect("Failed to parse XML");

        let parsed_root = doc.root().unwrap();
        assert_eq!(parsed_root.name, "level1");
        assert_eq!(parsed_root.children.len(), 1);
        assert_eq!(parsed_root.children[0].name, "level2");
        assert_eq!(parsed_root.children[0].children.len(), 1);

        let bytes = Writer::write(&doc, Format::PC).expect("Failed to write");
        let read = Reader::read(&bytes).expect("Failed to read");

        let root = read.root().unwrap();
        assert_eq!(root.name, "level1");
        assert_eq!(root.children.len(), 1);
        assert_eq!(root.children[0].name, "level2");
        assert_eq!(root.children[0].children.len(), 1);
        assert_eq!(root.children[0].children[0].name, "level3");
        assert_eq!(root.children[0].children[0].children.len(), 1);
        assert_eq!(root.children[0].children[0].children[0].name, "leaf");
        assert_eq!(
            root.children[0].children[0].children[0].text_string(),
            "deep value"
        );
    }

    #[test]
    fn test_variant_types() {
        assert_eq!(Variant::Null.to_string_value(), "");
        assert_eq!(Variant::Bool(true).to_string_value(), "true");
        assert_eq!(Variant::Bool(false).to_string_value(), "false");
        assert_eq!(Variant::Int(-42).to_string_value(), "-42");
        assert_eq!(Variant::UInt(100).to_string_value(), "100");
        assert_eq!(
            Variant::String("test".to_string()).to_string_value(),
            "test"
        );
    }

    #[test]
    fn test_node_count() {
        let mut root = Node::new("root");
        root.add_child(Node::new("child1"));
        root.add_child(Node::new("child2"));
        let mut child3 = Node::new("child3");
        child3.add_child(Node::new("grandchild"));
        root.add_child(child3);

        assert_eq!(root.node_count(), 5);
    }

    #[test]
    fn test_uncompressed_roundtrip() {
        let mut root = Node::new("data");
        root.add_child(Node::with_text("item", "uncompressed test"));
        let doc = Document::with_root(root);

        let bytes = Writer::write_uncompressed(&doc, Format::PC).expect("Failed to write");
        let read = Reader::read(&bytes).expect("Failed to read");

        let read_root = read.root().unwrap();
        assert_eq!(read_root.name, "data");
        assert_eq!(read_root.children[0].text_string(), "uncompressed test");
    }
}
