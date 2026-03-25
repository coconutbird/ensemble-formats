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

pub use de::NodeDeserializer;
pub use error::Error;

/// Deserialize a `T` from a [`bdt::Node`] reference.
pub fn from_node<'de, T: serde::Deserialize<'de>>(node: &bdt::Node) -> Result<T, Error> {
    T::deserialize(NodeDeserializer::new(node))
}
