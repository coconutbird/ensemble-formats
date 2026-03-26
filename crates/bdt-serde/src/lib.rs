//! Serde support for [`bdt::Node`] trees.
//!
//! Provides a [`Deserializer`](de::NodeDeserializer) that maps BDT node trees
//! to Rust structs via `#[derive(Deserialize)]`, using the same conventions as
//! XML serde crates:
//!
//! - `@attr` → XML attribute
//! - `$text` → node text content
//! - plain name → child element
//!
//! Use `#[serde(deny_unknown_fields)]` to detect unmapped fields in game data.
//!
//! # Example
//!
//! ```ignore
//! use serde::Deserialize;
//!
//! #[derive(Deserialize)]
//! #[serde(deny_unknown_fields)]
//! struct DamageType {
//!     #[serde(rename = "$text")]
//!     name: String,
//!     #[serde(rename = "@AttackRating")]
//!     attack_rating: Option<bool>,
//! }
//!
//! let dt: DamageType = bdt_serde::from_node(&node)?;
//! ```

#![no_std]
extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

mod de;
mod error;
mod ser;
mod warn;

pub use de::NodeDeserializer;
pub use error::Error;
pub use ser::NodeSerializer;
pub use warn::Warning;

/// Serialize a `T` into a [`bdt::Node`] with the given element name.
///
/// This is the inverse of [`from_node`].
pub fn to_node<T: serde::Serialize>(name: &str, value: &T) -> Result<bdt::Node, Error> {
    ser::to_node(name, value)
}

/// Deserialize a `T` from a [`bdt::Node`] reference.
pub fn from_node<'de, T: serde::Deserialize<'de>>(node: &bdt::Node) -> Result<T, Error> {
    T::deserialize(de::NodeDeserializer::new(node, None))
}

/// Deserialize a `T` from a [`bdt::Node`] reference, collecting warnings
/// about unmapped fields instead of ignoring them silently.
///
/// Returns `(value, warnings)` — the parse succeeds even if there are
/// unmapped fields; the warnings tell you which XML attributes/elements
/// were not consumed by the target struct.
pub fn from_node_warned<'de, T: serde::Deserialize<'de>>(
    node: &bdt::Node,
) -> Result<(T, alloc::vec::Vec<Warning>), Error> {
    let diag = warn::Diagnostics::new();
    let value = T::deserialize(de::NodeDeserializer::new(node, Some(&diag)))?;
    Ok((value, diag.into_warnings()))
}
