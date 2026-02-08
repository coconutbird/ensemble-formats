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
    compress_bdeflate_stream, decompress_bdeflate_stream, END_MAGIC, HEADER_SIZE, SIGNATURE,
    SIGNATURE_INVERTED,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_ecf_roundtrip_single_chunk() {
        let data = b"Hello, ECF World!".to_vec();
        let file_id = 0x12345678;
        let chunk_id = 0xDEADBEEF;

        // Write
        let mut buffer = Cursor::new(Vec::new());
        let mut writer = EcfWriter::new(&mut buffer, file_id);
        writer.add_chunk(chunk_id, data.clone());
        writer.finalize().expect("Failed to finalize");

        // Read
        buffer.set_position(0);
        let mut reader = EcfReader::new(&mut buffer).expect("Failed to read");

        assert_eq!(reader.header().id, file_id);
        assert_eq!(reader.chunks().len(), 1);
        assert_eq!(reader.chunks()[0].id, chunk_id);

        let read_data = reader.read_chunk_data(0).expect("Failed to read chunk");
        assert_eq!(read_data, data);
    }

    #[test]
    fn test_ecf_roundtrip_multiple_chunks() {
        let data1 = b"First chunk".to_vec();
        let data2 = b"Second chunk with more data".to_vec();
        let data3 = vec![0u8; 100]; // Binary data
        let file_id = 0xABCD1234;

        // Write
        let mut buffer = Cursor::new(Vec::new());
        let mut writer = EcfWriter::new(&mut buffer, file_id);
        writer.add_chunk(0x1111, data1.clone());
        writer.add_chunk(0x2222, data2.clone());
        writer.add_chunk(0x3333, data3.clone());
        writer.finalize().expect("Failed to finalize");

        // Read
        buffer.set_position(0);
        let mut reader = EcfReader::new(&mut buffer).expect("Failed to read");

        assert_eq!(reader.chunks().len(), 3);
        assert_eq!(reader.read_chunk_data(0).unwrap(), data1);
        assert_eq!(reader.read_chunk_data(1).unwrap(), data2);
        assert_eq!(reader.read_chunk_data(2).unwrap(), data3);
    }

    #[test]
    fn test_ecf_compressed_chunk_roundtrip() {
        let data =
            b"This is some data that will be compressed using BDeflateStream format!".to_vec();
        let file_id = 0x11111111;
        let chunk_id = 0x22222222;

        // Write with compression
        let mut buffer = Cursor::new(Vec::new());
        let mut writer = EcfWriter::new(&mut buffer, file_id);
        writer
            .add_chunk_compressed(chunk_id, data.clone())
            .expect("Failed to add compressed chunk");
        writer.finalize().expect("Failed to finalize");

        // Read (should auto-decompress)
        buffer.set_position(0);
        let mut reader = EcfReader::new(&mut buffer).expect("Failed to read");

        assert_eq!(reader.chunks().len(), 1);
        let read_data = reader.read_chunk_data(0).expect("Failed to read chunk");
        assert_eq!(read_data, data);
    }

    #[test]
    fn test_bdeflate_stream_roundtrip_le() {
        let original = b"Test data for BDeflateStream compression - little endian".to_vec();

        let compressed = compress_bdeflate_stream(&original, false).expect("Failed to compress");
        let decompressed = decompress_bdeflate_stream(&compressed).expect("Failed to decompress");

        assert_eq!(decompressed, original);
    }

    #[test]
    fn test_bdeflate_stream_roundtrip_be() {
        let original = b"Test data for BDeflateStream compression - big endian".to_vec();

        let compressed = compress_bdeflate_stream(&original, true).expect("Failed to compress");
        let decompressed = decompress_bdeflate_stream(&compressed).expect("Failed to decompress");

        assert_eq!(decompressed, original);
    }

    #[test]
    fn test_adler32_known_values() {
        // Empty data
        assert_eq!(adler32(&[]), 1);

        // "Hello" - known adler32 value
        let hello = b"Hello";
        let checksum = adler32(hello);
        assert_ne!(checksum, 0);
        assert_ne!(checksum, 1);

        // Same data should produce same checksum
        assert_eq!(adler32(hello), adler32(hello));

        // Different data should produce different checksum
        assert_ne!(adler32(b"Hello"), adler32(b"World"));
    }

    #[test]
    fn test_align_up() {
        assert_eq!(align_up(0, 16), 0);
        assert_eq!(align_up(1, 16), 16);
        assert_eq!(align_up(15, 16), 16);
        assert_eq!(align_up(16, 16), 16);
        assert_eq!(align_up(17, 16), 32);
        assert_eq!(align_up(100, 4), 100);
        assert_eq!(align_up(101, 4), 104);
    }

    #[test]
    fn test_compression_method_from_flags() {
        assert_eq!(CompressionMethod::from_flags(0), CompressionMethod::Stored);
        assert_eq!(
            CompressionMethod::from_flags(1),
            CompressionMethod::DeflateRaw
        );
        assert_eq!(
            CompressionMethod::from_flags(2),
            CompressionMethod::DeflateStream
        );
        assert_eq!(
            CompressionMethod::from_flags(5),
            CompressionMethod::Unknown(5)
        );
    }

    #[test]
    fn test_ecf_header_size() {
        assert_eq!(EcfHeader::SIZE, 32);
    }

    #[test]
    fn test_ecf_chunk_header_size() {
        assert_eq!(EcfChunkHeader::SIZE, 24);
    }
}
