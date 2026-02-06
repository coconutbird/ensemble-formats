//! ERA archive format parser for Halo Wars Definitive Edition
//!
//! ERA files are ECF-based archives containing compressed game assets.
//! Files are encrypted using TEA cipher in CTR mode with 64-byte blocks.

pub mod crypto;
mod decrypt_reader;
mod era;
mod error;

pub use crypto::{TeaKeys, ARCHIVE_PASSWORD, TEA_BLOCK_SIZE};
pub use decrypt_reader::DecryptReader;
pub use era::*;
pub use error::*;

// Re-export ECF types that are used in the public API
pub use ecf::{CompressionMethod, EcfChunkHeader, EcfHeader};

