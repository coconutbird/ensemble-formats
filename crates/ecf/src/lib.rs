//! ECF (Ensemble Common Format) container — `no_std` / zero-copy.
//!
//! ECF is a big-endian chunk container used by Ensemble Studios (Halo Wars,
//! Age of Empires III) to wrap various asset types: XMB, UGX, XTD, XTT, DDX,
//! UAX, and ERA archives.
//!
//! ## On-disk layout
//!
//! ```text
//! ┌──────────────────────────────────────┐
//! │ ECF Header          (32 bytes, BE)   │
//! ├──────────────────────────────────────┤
//! │ Chunk Header 0      (24 bytes, BE)   │
//! │ [per-chunk extra data]               │
//! │ Chunk Header 1      …                │
//! │ …                                    │
//! ├──────────────────────────────────────┤
//! │ Chunk Data 0  (aligned, optionally   │
//! │                compressed)           │
//! │ Chunk Data 1  …                      │
//! │ …                                    │
//! └──────────────────────────────────────┘
//! ```
//!
//! Chunk data may be stored raw or compressed with `BDeflateStream` (EA's
//! custom wrapper around raw deflate). The [`Reader`] handles
//! decompression transparently.
//!
//! ## Reading
//!
//! ```ignore
//! let bytes = std::fs::read("model.ugx")?;
//! let ecf = ecf::Reader::new(&bytes)?;
//!
//! // Iterate chunks
//! for (i, chunk) in ecf.chunks().iter().enumerate() {
//!     println!("chunk {} id=0x{:X} size={}", i, chunk.id, chunk.size);
//! }
//!
//! // Get decompressed chunk data by index or ID
//! let data = ecf.chunk_data(0)?;
//! let data = ecf.chunk_data_by_id(0x700)?;
//! ```
//!
//! ## Writing
//!
//! ```ignore
//! let mut ecf = ecf::Writer::new(0xAAC93746);
//! ecf.add_chunk(0x700, cached_data);
//! ecf.add_chunk(0x701, index_buffer);
//! ecf.add_chunk_compressed(0x702, vertex_buffer)?;
//! let bytes: Vec<u8> = ecf.finalize()?;
//! ```

#![no_std]
extern crate alloc;

mod error;
pub use error::{Error, Result};

/// Minimal `no_std` IO traits and helpers.
pub mod io;

mod header;
pub use header::{EcfChunkHeader, EcfChunkHeaderRaw, EcfHeader, EcfHeaderRaw};

/// BDeflateStream compression/decompression.
pub mod deflate_stream;
pub use deflate_stream::{compress, decompress};

mod reader;
pub use reader::{ReadOptions, Reader};

mod writer;
pub use writer::Writer;

mod checksum;
pub use checksum::adler32;

/// ECF header magic number (`0xDABA7737`).
///
/// All ECF files begin with this 4-byte big-endian value.
pub const HEADER_MAGIC: u32 = 0xDABA_7737;

/// Byte-swapped header magic (`0x3777BADA`).
///
/// This is not accepted by the game-compatible reader. It is exposed so
/// recovery tools can recognize the common corruption when using permissive
/// ECF read options.
pub const HEADER_MAGIC_INVERTED: u32 = 0x3777_BADA;

/// Per-chunk resource flags stored in [`EcfChunkHeader::resource_flags`].
pub mod resource_flags {
    /// Bit 0 — memory region is contiguous.
    pub const CONTIGUOUS: u16 = 1 << 0;
    /// Bit 1 — memory region is write-combined.
    pub const WRITE_COMBINED: u16 = 1 << 1;
    /// Bit 2 — chunk data is compressed with `BDeflateStream`.
    pub const IS_DEFLATE_STREAM: u16 = 1 << 2;
    /// Bit 3 — chunk contains a resource tag.
    pub const IS_RESOURCE_TAG: u16 = 1 << 3;
}

/// Compression method for a chunk, derived from the low nibble of
/// [`EcfChunkHeader::flags`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionMethod {
    /// No compression — data is stored verbatim.
    Stored,
    /// Raw deflate (no zlib/gzip wrapper).
    DeflateRaw,
    /// `BDeflateStream` — EA's custom deflate wrapper with checksums.
    DeflateStream,
    /// Unrecognised compression nibble.
    Unknown(u8),
}

impl CompressionMethod {
    /// Decode the compression method from the low nibble of chunk flags.
    #[must_use]
    pub fn from_flags(flags: u8) -> Self {
        match flags & 0x0F {
            0 => Self::Stored,
            1 => Self::DeflateRaw,
            2 => Self::DeflateStream,
            n => Self::Unknown(n),
        }
    }
}

/// Round `value` up to the next multiple of `alignment` (must be a power of two).
#[must_use]
pub fn align_up(value: usize, alignment: usize) -> usize {
    (value + alignment - 1) & !(alignment - 1)
}

#[cfg(test)]
mod tests {
    extern crate alloc;
    use super::*;
    use alloc::vec;

    #[test]
    fn test_ecf_roundtrip_single_chunk() {
        let data = b"Hello, ECF World!".to_vec();
        let file_id = 0x1234_5678;
        let chunk_id = 0xDEAD_BEEF;

        let mut writer = Writer::new(file_id);
        writer.add_chunk(chunk_id, data.clone());
        let bytes = writer.finalize().expect("Failed to finalize");

        let reader = Reader::new(&bytes).expect("Failed to read");
        assert_eq!(reader.header().id, file_id);
        assert_eq!(reader.chunks().len(), 1);
        assert_eq!(reader.chunks()[0].id, chunk_id);
        assert_eq!(reader.chunk_data(0).unwrap(), data);
    }

    #[test]
    fn test_ecf_roundtrip_multiple_chunks() {
        let data1 = b"First chunk".to_vec();
        let data2 = b"Second chunk with more data".to_vec();
        let data3 = vec![0u8; 100];
        let file_id = 0xABCD_1234;

        let mut writer = Writer::new(file_id);
        writer.add_chunk(0x1111, data1.clone());
        writer.add_chunk(0x2222, data2.clone());
        writer.add_chunk(0x3333, data3.clone());
        let bytes = writer.finalize().expect("Failed to finalize");

        let reader = Reader::new(&bytes).expect("Failed to read");
        assert_eq!(reader.chunks().len(), 3);
        assert_eq!(reader.chunk_data(0).unwrap(), data1);
        assert_eq!(reader.chunk_data(1).unwrap(), data2);
        assert_eq!(reader.chunk_data(2).unwrap(), data3);
    }

    #[test]
    fn test_ecf_compressed_chunk_roundtrip() {
        let data =
            b"This is some data that will be compressed using BDeflateStream format!".to_vec();
        let file_id = 0x1111_1111;
        let chunk_id = 0x2222_2222;

        let mut writer = Writer::new(file_id);
        writer.add_chunk_compressed(chunk_id, &data).unwrap();
        let bytes = writer.finalize().expect("Failed to finalize");

        let reader = Reader::new(&bytes).expect("Failed to read");
        assert_eq!(reader.chunks().len(), 1);
        assert_eq!(reader.chunk_data(0).unwrap(), data);
    }

    #[test]
    fn explicit_options_can_accept_bad_magic() {
        let mut writer = Writer::new(0x1234_5678);
        writer.add_chunk(0x1111, vec![1, 2, 3]);
        let mut bytes = writer.finalize().unwrap();
        bytes[..4].copy_from_slice(&0xDEAD_BEEFu32.to_be_bytes());

        assert!(matches!(
            Reader::new(&bytes),
            Err(Error::InvalidMagic { .. })
        ));
        let reader = Reader::new_with_options(&bytes, ReadOptions::accepting_bad_magic()).unwrap();
        assert_eq!(reader.header().magic, 0xDEAD_BEEF);
    }

    #[test]
    fn strict_reader_rejects_byte_swapped_magic() {
        let mut writer = Writer::new(0x1234_5678);
        writer.add_chunk(0x1111, vec![1]);
        let mut bytes = writer.finalize().unwrap();
        bytes[..4].copy_from_slice(&HEADER_MAGIC_INVERTED.to_be_bytes());
        assert!(matches!(
            Reader::new(&bytes),
            Err(Error::InvalidMagic { .. })
        ));
    }

    #[test]
    fn reader_rejects_header_smaller_than_fixed_header() {
        let mut writer = Writer::new(0x1234_5678);
        writer.add_chunk(0x1111, vec![1]);
        let mut bytes = writer.finalize().unwrap();
        bytes[4..8].copy_from_slice(&16u32.to_be_bytes());
        assert!(matches!(
            Reader::new_unchecked(&bytes),
            Err(Error::InvalidHeaderSize {
                minimum: EcfHeader::SIZE,
                actual: 16
            })
        ));
    }

    #[test]
    fn checksum_bypass_still_rejects_out_of_range_chunks() {
        let mut writer = Writer::new(0x1234_5678);
        writer.add_chunk(0x1111, vec![1]);
        let mut bytes = writer.finalize().unwrap();
        bytes[40..44].copy_from_slice(&u32::MAX.to_be_bytes());

        assert!(matches!(
            Reader::new_unchecked(&bytes),
            Err(Error::UnexpectedEof)
        ));
    }

    #[test]
    fn metadata_writer_preserves_flags_and_compression() {
        let payload = vec![0x5A; 4096];
        let mut writer = Writer::new(0x1234_5678);
        writer.set_header_flags(0x1234);
        writer
            .add_chunk_with_metadata(
                0x1111,
                payload.clone(),
                5,
                0,
                resource_flags::CONTIGUOUS | resource_flags::IS_DEFLATE_STREAM,
            )
            .unwrap();
        let bytes = writer.finalize().unwrap();
        let reader = Reader::new(&bytes).unwrap();
        assert_eq!(reader.header().flags, 0x1234);
        assert_eq!(reader.chunks()[0].alignment_log2, 5);
        assert_eq!(
            reader.chunks()[0].resource_flags,
            resource_flags::CONTIGUOUS | resource_flags::IS_DEFLATE_STREAM
        );
        assert_eq!(reader.chunk_data(0).unwrap(), payload);
    }

    #[test]
    fn test_bdeflate_stream_roundtrip_le() {
        let original = b"Test data for BDeflateStream compression - little endian".to_vec();
        let compressed = compress(&original, false).unwrap();
        assert_eq!(decompress(&compressed).unwrap(), original);
    }

    #[test]
    fn test_bdeflate_stream_roundtrip_be() {
        let original = b"Test data for BDeflateStream compression - big endian".to_vec();
        let compressed = compress(&original, true).unwrap();
        assert_eq!(decompress(&compressed).unwrap(), original);
    }

    #[test]
    fn test_adler32_known_values() {
        assert_eq!(adler32(&[]), 1);
        let hello = b"Hello";
        let checksum = adler32(hello);
        assert_ne!(checksum, 0);
        assert_ne!(checksum, 1);
        assert_eq!(adler32(hello), adler32(hello));
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
