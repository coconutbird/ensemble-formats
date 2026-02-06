//! ERA archive format for Halo Wars Definitive Edition
//!
//! ERA files are ECF-based archives containing compressed game assets.
//! Files are encrypted using TEA cipher in CTR mode with 64-byte blocks.
//!
//! # Reading ERA archives
//!
//! ```no_run
//! use era::EraArchive;
//!
//! let mut archive = EraArchive::open("root.era").unwrap();
//! for entry in archive.iter() {
//!     println!("{}", entry.filename.as_deref().unwrap_or("<unnamed>"));
//! }
//! ```
//!
//! # Writing ERA archives
//!
//! ```no_run
//! use era::EraWriter;
//!
//! let mut writer = EraWriter::new();
//! writer.add_file("data/test.txt", b"Hello, world!".to_vec());
//! writer.write_to_file("output.era").unwrap();
//! ```

pub mod crypto;
mod decrypt_reader;
mod encrypt_writer;
mod era;
mod error;
mod writer;

pub use crypto::{TeaKeys, ARCHIVE_PASSWORD, TEA_BLOCK_SIZE};
pub use decrypt_reader::DecryptReader;
pub use encrypt_writer::EncryptWriter;
pub use era::*;
pub use error::*;
pub use writer::EraWriter;

// Re-export ECF types that are used in the public API
pub use ecf::{CompressionMethod, EcfChunkHeader, EcfHeader};

