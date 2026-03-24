//! ERA archive format for Halo Wars Definitive Edition.
//!
//! ERA files are ECF-based archives containing compressed game assets.
//! Files are encrypted using TEA cipher in CTR mode with 64-byte blocks.
//!
//! # Reading ERA archives
//!
//! ```ignore
//! let mut reader = era::Reader::from_bytes(decrypted_bytes).unwrap();
//! for entry in reader.iter() {
//!     println!("{}", entry.filename.as_deref().unwrap_or("<unnamed>"));
//! }
//! ```
//!
//! # Writing ERA archives
//!
//! ```ignore
//! let mut writer = era::Writer::new();
//! writer.add_file("data/test.txt", b"Hello, world!".to_vec());
//! let archive_bytes = writer.finalize().unwrap();
//! ```

#![no_std]
extern crate alloc;

pub mod buffer_pool;
pub mod crypto;
mod error;
mod header;
mod reader;
mod writer;

pub use buffer_pool::{BufferPool, PooledBuffer};
pub use crypto::tea::{ARCHIVE_PASSWORD, TEA_BLOCK_SIZE, TeaKeys};
#[cfg(feature = "rayon")]
pub use crypto::tea::{tea_decrypt_data_parallel, tea_encrypt_data_parallel};
pub use error::*;
pub use header::*;
pub use reader::*;
pub use writer::{CompressedData, Writer, compress_file_data};
