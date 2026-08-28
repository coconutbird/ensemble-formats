//! Deserialize XMB binary XML files directly into Rust structs via serde.
//!
//! This is a thin wrapper around [`xmb::Reader`] and [`bdt_serde`] that
//! provides a single `from_bytes` entry point — the XMB equivalent of
//! `serde_json::from_str`.
//!
//! # Example
//!
//! ```ignore
//! use serde::Deserialize;
//!
//! #[derive(Deserialize)]
//! #[serde(deny_unknown_fields)]
//! struct Config {
//!     #[serde(rename = "@name")]
//!     name: String,
//!     value: Option<String>,
//! }
//!
//! let bytes = std::fs::read("config.xmb").unwrap();
//! let config: Config = xmb_serde::from_bytes(&bytes).unwrap();
//! ```

#![no_std]
extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

mod error;

pub use bdt_serde::NodeDeserializer;
pub use bdt_serde::Warning;
pub use error::Error;

/// Deserialize a `T` from raw XMB bytes (binary or XML text).
///
/// This is the XMB equivalent of `serde_json::from_str` — it parses the XMB
/// file and deserializes the root node into `T` in one call.
///
/// # Errors
///
/// Returns an error if `data` is not a valid XMB document, has no root node,
/// or its root cannot be deserialized as `T`.
pub fn from_bytes<'de, T: serde::Deserialize<'de>>(data: &[u8]) -> Result<T, Error> {
    let doc = xmb::Reader::read(data)?;
    let root = doc.root().ok_or(Error::EmptyDocument)?;
    bdt_serde::from_node(root).map_err(Error::Deserialize)
}

/// Deserialize a `T` from raw XMB bytes, collecting diagnostic warnings.
///
/// Returns `(value, warnings)` — the parse succeeds even when there are
/// extra fields or type mismatches; inspect the warnings to discover
/// schema differences in game data files.
///
/// # Errors
///
/// Returns an error if `data` is not a valid XMB document, has no root node,
/// or its root cannot be deserialized as `T`.
pub fn from_bytes_warned<'de, T: serde::Deserialize<'de>>(
    data: &[u8],
) -> Result<(T, alloc::vec::Vec<Warning>), Error> {
    let doc = xmb::Reader::read(data)?;
    let root = doc.root().ok_or(Error::EmptyDocument)?;
    bdt_serde::from_node_warned(root).map_err(Error::Deserialize)
}

/// Deserialize a `T` from an XML string.
///
/// This is the XMB equivalent of `serde_json::from_str` for raw XML text.
///
/// # Errors
///
/// Returns an error if `xml` is malformed, has no root node, or its root
/// cannot be deserialized as `T`.
pub fn from_str<'de, T: serde::Deserialize<'de>>(xml: &str) -> Result<T, Error> {
    let doc = xmb::Document::from_xml(xml)?;
    let root = doc.root().ok_or(Error::EmptyDocument)?;
    bdt_serde::from_node(root).map_err(Error::Deserialize)
}

/// Deserialize a `T` from an XML string, collecting diagnostic warnings.
///
/// Returns `(value, warnings)` — see [`from_bytes_warned`] for details.
///
/// # Errors
///
/// Returns an error if `xml` is malformed, has no root node, or its root
/// cannot be deserialized as `T`.
pub fn from_str_warned<'de, T: serde::Deserialize<'de>>(
    xml: &str,
) -> Result<(T, alloc::vec::Vec<Warning>), Error> {
    let doc = xmb::Document::from_xml(xml)?;
    let root = doc.root().ok_or(Error::EmptyDocument)?;
    bdt_serde::from_node_warned(root).map_err(Error::Deserialize)
}

/// Deserialize a `T` from a pre-parsed [`xmb::Document`].
///
/// Use this when you already have a parsed document and want to avoid
/// re-parsing the bytes.
///
/// # Errors
///
/// Returns an error if `doc` has no root node or its root cannot be
/// deserialized as `T`.
pub fn from_document<'de, T: serde::Deserialize<'de>>(doc: &xmb::Document) -> Result<T, Error> {
    let root = doc.root().ok_or(Error::EmptyDocument)?;
    bdt_serde::from_node(root).map_err(Error::Deserialize)
}

/// Deserialize a `T` from a pre-parsed [`xmb::Document`], collecting
/// diagnostic warnings.
///
/// Returns `(value, warnings)` — see [`from_bytes_warned`] for details.
///
/// # Errors
///
/// Returns an error if `doc` has no root node or its root cannot be
/// deserialized as `T`.
pub fn from_document_warned<'de, T: serde::Deserialize<'de>>(
    doc: &xmb::Document,
) -> Result<(T, alloc::vec::Vec<Warning>), Error> {
    let root = doc.root().ok_or(Error::EmptyDocument)?;
    bdt_serde::from_node_warned(root).map_err(Error::Deserialize)
}
