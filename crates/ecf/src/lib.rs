//! ECF (Ensemble Common Format) container handling.
//!
//! ECF is a container format used by Ensemble Studios to wrap various
//! file types including XMB. The format uses big-endian byte order.
//!
//! ## ECF Structure
//!
//! An ECF file consists of:
//! - ECF Header (32 bytes)
//! - Chunk Headers (24 bytes each + optional extra data)
//! - Chunk Data (optionally compressed)
//!
//! ## Compression
//!
//! Chunks can be compressed using BDeflateStream format, which wraps
//! standard deflate compression with checksums and metadata.

mod error;

pub use error::{Error, Result};

mod header;
pub use header::{EcfChunkHeader, EcfHeader};

mod deflate_stream;
pub use deflate_stream::{
    decompress_bdeflate_stream, compress_bdeflate_stream, 
    SIGNATURE, SIGNATURE_INVERTED, HEADER_SIZE, END_MAGIC,
};

mod reader;
pub use reader::EcfReader;

mod writer;
pub use writer::EcfWriter;

mod checksum;
pub use checksum::adler32;

// ============================================================================
// ECF Constants
// ============================================================================

/// ECF header magic number.
pub const ECF_HEADER_MAGIC: u32 = 0xDABA7737;
/// ECF inverted header magic (for little-endian detection).
pub const ECF_INVERTED_HEADER_MAGIC: u32 = 0x3777BADA;

// ============================================================================
// ECF Chunk Resource Flags
// ============================================================================

/// ECF chunk resource flags.
pub mod chunk_resource_flags {
    /// Bit 0: Memory is contiguous.
    pub const CONTIGUOUS: u16 = 1 << 0;

    /// Bit 1: Memory is write-combined.
    pub const WRITE_COMBINED: u16 = 1 << 1;

    /// Bit 2: Chunk data is compressed using BDeflateStream format.
    pub const IS_DEFLATE_STREAM: u16 = 1 << 2;

    /// Bit 3: Chunk contains a resource tag.
    pub const IS_RESOURCE_TAG: u16 = 1 << 3;
}

// ============================================================================
// Compression Method
// ============================================================================

/// Compression method for chunks (stored in flags field).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionMethod {
    /// Uncompressed data
    Stored,
    /// Raw deflate (no zlib header)
    DeflateRaw,
    /// BDeflateStream format (EA's custom wrapper around deflate)
    DeflateStream,
    /// Unknown compression method
    Unknown(u8),
}

impl CompressionMethod {
    /// Parse compression method from chunk flags.
    pub fn from_flags(flags: u8) -> Self {
        match flags & 0x0F {
            0 => CompressionMethod::Stored,
            1 => CompressionMethod::DeflateRaw,
            2 => CompressionMethod::DeflateStream,
            n => CompressionMethod::Unknown(n),
        }
    }
}

/// Align a value up to the given alignment.
pub fn align_up(value: usize, alignment: usize) -> usize {
    (value + alignment - 1) & !(alignment - 1)
}

