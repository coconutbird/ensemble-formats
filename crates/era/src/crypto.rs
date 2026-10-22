//! Cryptographic primitives for ERA archives.
//!
//! - [`tea`]: TEA cipher (key derivation, block encrypt/decrypt, CTR mode)
//! - [`decrypt`] / [`encrypt`]: Streaming TEA reader/writer wrappers
//! - [`merkle`]: Merkle tree signature verification

pub mod decrypt;
pub mod encrypt;
pub mod merkle;
pub mod tea;
