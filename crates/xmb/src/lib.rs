//! XMB binary XML format library for Halo Wars (Ensemble Studios).
//!
//! This crate provides reading and writing of XMB files, which are binary
//! representations of XML used in Halo Wars and other Ensemble Studios games.
//!
//! # Features
//!
//! - Read XMB files (PC and Xbox 360 formats)
//! - Write XMB files (PC and Xbox 360 formats)
//! - Convert between XMB and XML
//! - Automatic format detection
//!
//! # Example
//!
//! ```no_run
//! use xmb::{XmbReader, XmbData};
//!
//! let data = std::fs::read("example.xmb").unwrap();
//! let xmb = XmbReader::read(&data).unwrap();
//!
//! // Convert to XML
//! let xml = xmb.to_xml();
//! println!("{}", xml);
//! ```

mod error;
mod types;
mod variant;
mod xmb;

pub use error::{Error, Result};
pub use types::{Attribute, Node, XmbData, XmbFormat};
pub use variant::Variant;
pub use xmb::{XMB_ECF_FILE_ID, XMB_SIGNATURE, XMX_PACKED_DATA_CHUNK_ID, XmbReader, XmbWriter};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xmb_roundtrip_pc_format() {
        let mut root = Node::new("root");
        root.add_child(Node::with_text("message", "Hello, XMB!"));
        let xmb = XmbData::with_root(root);

        // Write
        let bytes = XmbWriter::write(&xmb, XmbFormat::PC).expect("Failed to write");

        // Read
        let read_xmb = XmbReader::read(&bytes).expect("Failed to read");

        assert!(read_xmb.root().is_some());
        let read_root = read_xmb.root().unwrap();
        assert_eq!(read_root.name, "root");
        assert_eq!(read_root.children.len(), 1);
        assert_eq!(read_root.children[0].name, "message");
        assert_eq!(read_root.children[0].text_string(), "Hello, XMB!");
    }

    #[test]
    fn test_xmb_roundtrip_xbox360_format() {
        let mut root = Node::new("root");
        root.add_child(Node::with_text("message", "Hello, Xbox!"));
        let xmb = XmbData::with_root(root);

        // Write
        let bytes = XmbWriter::write(&xmb, XmbFormat::Xbox360).expect("Failed to write");

        // Read
        let read_xmb = XmbReader::read(&bytes).expect("Failed to read");

        assert!(read_xmb.root().is_some());
        let read_root = read_xmb.root().unwrap();
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

        let xmb = XmbData::from_xml(xml).expect("Failed to parse XML");

        // Write to XMB
        let bytes = XmbWriter::write(&xmb, XmbFormat::PC).expect("Failed to write XMB");

        // Read back
        let read_xmb = XmbReader::read(&bytes).expect("Failed to read XMB");

        // Convert back to XML
        let result_xml = read_xmb.to_xml();

        // Verify structure preserved
        assert!(result_xml.contains("<config>"));
        assert!(result_xml.contains("<setting"));
        assert!(result_xml.contains("name=\"volume\""));
        assert!(result_xml.contains("value=\"50\""));
    }

    #[test]
    fn test_xmb_with_attributes() {
        let mut root = Node::new("element");
        root.add_attribute(Attribute::with_string("id", "test123"));
        root.add_attribute(Attribute::new("count", Variant::UInt(42)));
        let xmb = XmbData::with_root(root);

        // Write
        let bytes = XmbWriter::write(&xmb, XmbFormat::PC).expect("Failed to write");

        // Read
        let read_xmb = XmbReader::read(&bytes).expect("Failed to read");

        let read_root = read_xmb.root().unwrap();
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
    fn test_xmb_nested_nodes() {
        // Test deeply nested node structures
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<level1>
    <level2>
        <level3>
            <leaf>deep value</leaf>
        </level3>
    </level2>
</level1>"#;

        let xmb = XmbData::from_xml(xml).expect("Failed to parse XML");

        // Verify the XML parsed correctly first
        let parsed_root = xmb.root().unwrap();
        assert_eq!(parsed_root.name, "level1");
        assert_eq!(
            parsed_root.children.len(),
            1,
            "XML parsing: level1 should have 1 child"
        );
        assert_eq!(parsed_root.children[0].name, "level2");
        assert_eq!(
            parsed_root.children[0].children.len(),
            1,
            "XML parsing: level2 should have 1 child"
        );

        // Write
        let bytes = XmbWriter::write(&xmb, XmbFormat::PC).expect("Failed to write");

        // Read back
        let read_xmb = XmbReader::read(&bytes).expect("Failed to read");

        let root = read_xmb.root().unwrap();
        assert_eq!(root.name, "level1");
        assert_eq!(
            root.children.len(),
            1,
            "XMB roundtrip: level1 should have 1 child"
        );
        assert_eq!(root.children[0].name, "level2");
        assert_eq!(
            root.children[0].children.len(),
            1,
            "XMB roundtrip: level2 should have 1 child"
        );
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

        assert_eq!(root.node_count(), 5); // root + 3 children + 1 grandchild
    }

    #[test]
    fn test_xmb_uncompressed_roundtrip() {
        let mut root = Node::new("data");
        root.add_child(Node::with_text("item", "uncompressed test"));
        let xmb = XmbData::with_root(root);

        // Write uncompressed
        let bytes = XmbWriter::write_uncompressed(&xmb, XmbFormat::PC).expect("Failed to write");

        // Read
        let read_xmb = XmbReader::read(&bytes).expect("Failed to read");

        let read_root = read_xmb.root().unwrap();
        assert_eq!(read_root.name, "data");
        assert_eq!(read_root.children[0].text_string(), "uncompressed test");
    }
}
