//! BBinaryDataTree (BDT) packed document format.
//!
//! This crate implements the packed binary tree format used by Ensemble Studios games
//! (Halo Wars, Age of Empires III). The format stores hierarchical node data with
//! typed attributes using a compact variant encoding.
//!
//! Used by:
//! - XMB (binary XML) files
//! - UGX material chunks (chunk 0x704)
//! - Other Ensemble data formats
//!
//! ## Example
//!
//! ```ignore
//! use bdt::{PackedReader, Node};
//!
//! // Read a packed document from raw bytes (e.g., from an ECF chunk)
//! let node = PackedReader::read_le(&data)?;
//! if let Some(root) = node {
//!     for child in &root.children {
//!         println!("{}: {:?}", child.name, child.get_attribute("Name"));
//!     }
//! }
//! ```

mod error;
pub use error::{Error, Result};

pub mod variant;
pub use variant::{
    pack_float24, pack_fract24, pack_int24, pack_uint24, unpack_float24, unpack_fract24,
    unpack_int24, unpack_uint24,
};
pub use variant::{Variant, VariantType};
pub use variant::{OFFSET_FLAG, TYPE_MASK, UNSIGNED_FLAG, VEC_SIZE_MASK, VEC_SIZE_SHIFT};

mod types;
pub use types::{Attribute, Node};

mod reader;
pub use reader::PackedReader;

mod writer;
pub use writer::PackedWriter;
