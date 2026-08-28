//! ECF header structures.
//!
//! Each ECF file begins with a 32-byte file header ([`Header`]) followed
//! by one or more 24-byte chunk headers ([`ChunkHeader`]). Both are
//! big-endian on disk.
//!
//! Parsing uses the zero-copy overlay types [`EcfHeaderRaw`] /
//! [`EcfChunkHeaderRaw`] (via `zerocopy`) and then converts into the
//! friendlier native-endian [`Header`] / [`ChunkHeader`].
//!
//! ## File header layout (32 bytes)
//!
//! | Offset | Size | Field                  |
//! |--------|------|------------------------|
//! | 0      | 4    | magic (`0xDABA7737`)   |
//! | 4      | 4    | `header_size`            |
//! | 8      | 4    | adler32                |
//! | 12     | 4    | `file_size`              |
//! | 16     | 2    | `num_chunks`             |
//! | 18     | 2    | flags                  |
//! | 20     | 4    | id (user file-type ID) |
//! | 24     | 2    | `chunk_extra_data_size`  |
//! | 26     | 6    | padding                |
//!
//! ## Chunk header layout (24 bytes)
//!
//! | Offset | Size | Field          |
//! |--------|------|----------------|
//! | 0      | 8    | id             |
//! | 8      | 4    | offset         |
//! | 12     | 4    | size           |
//! | 16     | 4    | adler32        |
//! | 20     | 1    | flags          |
//! | 21     | 1    | alignment_log2 |
//! | 22     | 2    | resource_flags |

use zerocopy::{FromBytes, Immutable, KnownLayout, Ref};

use crate::{CompressionMethod, Error, HEADER_MAGIC, Result};

/// Raw on-disk ECF file header (32 bytes, big-endian).
#[derive(FromBytes, KnownLayout, Immutable, Debug)]
#[repr(C)]
pub struct EcfHeaderRaw {
    pub magic: [u8; 4],
    pub header_size: [u8; 4],
    pub adler32: [u8; 4],
    pub file_size: [u8; 4],
    pub num_chunks: [u8; 2],
    pub flags: [u8; 2],
    pub id: [u8; 4],
    pub chunk_extra_data_size: [u8; 2],
    pub pad0: [u8; 2],
    pub pad1: [u8; 4],
}

/// Raw on-disk ECF chunk header (24 bytes, big-endian).
#[derive(FromBytes, KnownLayout, Immutable, Debug)]
#[repr(C)]
pub struct EcfChunkHeaderRaw {
    pub id: [u8; 8],
    pub offset: [u8; 4],
    pub size: [u8; 4],
    pub adler32: [u8; 4],
    pub flags: u8,
    pub alignment_log2: u8,
    pub resource_flags: [u8; 2],
}

/// Parsed ECF file header.
#[derive(Debug, Clone, Default)]
pub struct EcfHeader {
    /// Header magic number (should be [`HEADER_MAGIC`](crate::HEADER_MAGIC)).
    pub magic: u32,
    /// Total header size including extra data.
    pub header_size: u32,
    /// Adler-32 checksum.
    pub adler32: u32,
    /// Total file size including header.
    pub file_size: u32,
    /// Number of chunks in the file.
    pub num_chunks: u16,
    /// Header flags.
    pub flags: u16,
    /// User-provided file ID.
    pub id: u32,
    /// Size of extra data per chunk header.
    pub chunk_extra_data_size: u16,
}

impl EcfHeader {
    /// The size of the ECF header in bytes (including padding).
    pub const SIZE: usize = 32;

    /// Parse an ECF header from a byte slice (zero-copy).
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnexpectedEof`] if `data` is shorter than the header,
    /// or [`Error::InvalidMagic`] if its signature is not recognized.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        Self::from_bytes_with_magic_validation(data, true)
    }

    pub(crate) fn from_bytes_with_magic_validation(
        data: &[u8],
        validate_magic: bool,
    ) -> Result<Self> {
        let (raw, _): (Ref<_, EcfHeaderRaw>, _) =
            Ref::from_prefix(data).map_err(|_| Error::UnexpectedEof)?;

        let magic = u32::from_be_bytes(raw.magic);
        if validate_magic && magic != HEADER_MAGIC {
            return Err(Error::InvalidMagic {
                expected: HEADER_MAGIC,
                found: magic,
            });
        }

        Ok(Self {
            magic,
            header_size: u32::from_be_bytes(raw.header_size),
            adler32: u32::from_be_bytes(raw.adler32),
            file_size: u32::from_be_bytes(raw.file_size),
            num_chunks: u16::from_be_bytes(raw.num_chunks),
            flags: u16::from_be_bytes(raw.flags),
            id: u32::from_be_bytes(raw.id),
            chunk_extra_data_size: u16::from_be_bytes(raw.chunk_extra_data_size),
        })
    }

    /// Serialize this header to a 32-byte big-endian buffer.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; 32] {
        let mut buf = [0u8; 32];
        buf[0..4].copy_from_slice(&self.magic.to_be_bytes());
        buf[4..8].copy_from_slice(&self.header_size.to_be_bytes());
        buf[8..12].copy_from_slice(&self.adler32.to_be_bytes());
        buf[12..16].copy_from_slice(&self.file_size.to_be_bytes());
        buf[16..18].copy_from_slice(&self.num_chunks.to_be_bytes());
        buf[18..20].copy_from_slice(&self.flags.to_be_bytes());
        buf[20..24].copy_from_slice(&self.id.to_be_bytes());
        buf[24..26].copy_from_slice(&self.chunk_extra_data_size.to_be_bytes());
        // bytes 26..32 are padding (already zero)
        buf
    }
}

/// Parsed ECF chunk header.
#[derive(Debug, Clone, Default)]
pub struct EcfChunkHeader {
    /// Chunk ID.
    pub id: u64,
    /// Offset to chunk data from start of file.
    pub offset: u32,
    /// Size of chunk data.
    pub size: u32,
    /// Adler-32 checksum of chunk data.
    pub adler32: u32,
    /// Chunk flags.
    pub flags: u8,
    /// Alignment as log2 (e.g., 2 = 4-byte alignment).
    pub alignment_log2: u8,
    /// Resource flags.
    pub resource_flags: u16,
}

impl EcfChunkHeader {
    /// The size of the ECF chunk header in bytes (without extra data).
    pub const SIZE: usize = 24;

    /// Parse a chunk header from a byte slice (zero-copy).
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnexpectedEof`] if `data` is shorter than a complete
    /// chunk header.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let (raw, _): (Ref<_, EcfChunkHeaderRaw>, _) =
            Ref::from_prefix(data).map_err(|_| Error::UnexpectedEof)?;

        Ok(Self {
            id: u64::from_be_bytes(raw.id),
            offset: u32::from_be_bytes(raw.offset),
            size: u32::from_be_bytes(raw.size),
            adler32: u32::from_be_bytes(raw.adler32),
            flags: raw.flags,
            alignment_log2: raw.alignment_log2,
            resource_flags: u16::from_be_bytes(raw.resource_flags),
        })
    }

    /// Serialize this chunk header to a 24-byte big-endian buffer.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; 24] {
        let mut buf = [0u8; 24];
        buf[0..8].copy_from_slice(&self.id.to_be_bytes());
        buf[8..12].copy_from_slice(&self.offset.to_be_bytes());
        buf[12..16].copy_from_slice(&self.size.to_be_bytes());
        buf[16..20].copy_from_slice(&self.adler32.to_be_bytes());
        buf[20] = self.flags;
        buf[21] = self.alignment_log2;
        buf[22..24].copy_from_slice(&self.resource_flags.to_be_bytes());
        buf
    }

    /// Get the alignment in bytes.
    #[must_use]
    pub fn alignment(&self) -> usize {
        1 << self.alignment_log2
    }

    /// Get the compression method from flags.
    #[must_use]
    pub fn compression_method(&self) -> CompressionMethod {
        CompressionMethod::from_flags(self.flags)
    }
}
