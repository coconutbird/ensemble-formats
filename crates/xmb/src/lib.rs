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
//! use std::fs::File;
//!
//! let file = File::open("example.xmb").unwrap();
//! let xmb = XmbReader::read(file).unwrap();
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
pub use xmb::{XmbReader, XmbWriter, XMB_ECF_FILE_ID, XMB_SIGNATURE, XMX_PACKED_DATA_CHUNK_ID};
