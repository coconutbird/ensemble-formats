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
//! use bdt::{Reader, Node};
//!
//! // Read a packed document from raw bytes (e.g., from an ECF chunk)
//! let node = Reader::read_le(&data)?;
//! if let Some(root) = node {
//!     for child in &root.children {
//!         println!("{}: {:?}", child.name, child.get_attribute("Name"));
//!     }
//! }
//! ```

#![no_std]
extern crate alloc;

mod error;
pub use error::{Error, Result};

pub mod variant;
pub use variant::{OFFSET_FLAG, TYPE_MASK, UNSIGNED_FLAG, VEC_SIZE_MASK, VEC_SIZE_SHIFT};
pub use variant::{Variant, VariantType};
pub use variant::{
    pack_float24, pack_fract24, pack_int24, pack_uint24, unpack_float24, unpack_fract24,
    unpack_int24, unpack_uint24,
};

mod node;
pub use node::{Attribute, Node};

pub mod raw;

mod compact;
mod util;

mod reader;
pub use reader::Reader;

mod writer;
pub use writer::Writer;
