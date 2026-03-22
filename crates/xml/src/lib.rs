//! Minimal `no_std` XML reader/writer for Ensemble Studios formats.
//!
//! - **Reading:** re-exports [`xmlparser`] for zero-alloc tokenized parsing.
//! - **Writing:** provides [`Writer`], a simple indented XML builder.
//! - **Escaping:** [`escape::escape_into`] and [`escape::unescape`].
//!
//! # Reading example
//!
//! ```
//! for event in xml::Reader::new("<root attr=\"1\"/>") {
//!     println!("{:?}", event.unwrap());
//! }
//! ```
//!
//! # Writing example
//!
//! ```
//! let mut w = xml::Writer::new();
//! w.declaration();
//! w.open("config");
//! w.close();
//! w.empty("setting");
//! w.end("config");
//! let output = w.finish();
//! assert!(output.contains("<config>"));
//! ```

#![no_std]
extern crate alloc;

pub mod escape;
pub mod reader;
mod writer;

pub use reader::Reader;
pub use writer::Writer;

// Re-export xmlparser types for direct access if needed.
pub use xmlparser;
