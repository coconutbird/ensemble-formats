//! ERA archive header structures and constants.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use ecf::{CompressionMethod, EcfChunkHeader};
use zerocopy::{FromBytes, Immutable, KnownLayout, Ref};

use crate::error::{Error, Result};

/// Archive header magic number.
pub const ARCHIVE_HEADER_MAGIC: u32 = 0x05AB_DBD8;

/// Raw on-disk ERA archive header extension (16 bytes, big-endian).
#[derive(FromBytes, KnownLayout, Immutable, Debug)]
#[repr(C)]
struct EraArchiveHeaderRaw {
    archive_magic: [u8; 4],
    signature_size: [u8; 4],
    reserved: [[u8; 4]; 2],
}

/// ERA archive header extension (16 bytes after ECF header).
#[derive(Debug, Clone)]
pub struct EraArchiveHeader {
    /// Archive-specific magic (0x05ABDBD8).
    pub archive_magic: u32,
    /// Size of digital signature.
    pub signature_size: u32,
    /// Reserved fields.
    pub reserved: [u32; 2],
}

impl EraArchiveHeader {
    /// Size of the archive header extension in bytes.
    pub const SIZE: usize = size_of::<EraArchiveHeaderRaw>();

    /// Create a new archive header with default values.
    #[must_use]
    pub fn new() -> Self {
        Self {
            archive_magic: ARCHIVE_HEADER_MAGIC,
            signature_size: 0,
            reserved: [0, 0],
        }
    }

    /// Parse archive header extension from a byte slice.
    ///
    /// # Errors
    ///
    /// Returns an error if the header is truncated or has an invalid archive
    /// magic value.
    pub fn from_bytes(buf: &[u8]) -> Result<Self> {
        let (raw, _): (Ref<_, EraArchiveHeaderRaw>, _) =
            Ref::from_prefix(buf).map_err(|_| Error::UnexpectedEof)?;

        let archive_magic = u32::from_be_bytes(raw.archive_magic);
        if archive_magic != ARCHIVE_HEADER_MAGIC {
            return Err(Error::InvalidArchiveMagic {
                expected: ARCHIVE_HEADER_MAGIC,
                found: archive_magic,
            });
        }

        Ok(Self {
            archive_magic,
            signature_size: u32::from_be_bytes(raw.signature_size),
            reserved: [
                u32::from_be_bytes(raw.reserved[0]),
                u32::from_be_bytes(raw.reserved[1]),
            ],
        })
    }

    /// Serialize archive header extension to bytes.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut buf = [0u8; Self::SIZE];
        buf[0..4].copy_from_slice(&self.archive_magic.to_be_bytes());
        buf[4..8].copy_from_slice(&self.signature_size.to_be_bytes());
        buf[8..12].copy_from_slice(&self.reserved[0].to_be_bytes());
        buf[12..16].copy_from_slice(&self.reserved[1].to_be_bytes());
        buf
    }
}

impl Default for EraArchiveHeader {
    fn default() -> Self {
        Self::new()
    }
}

/// Raw on-disk ERA chunk extra data (32 bytes, big-endian).
#[derive(FromBytes, KnownLayout, Immutable, Debug)]
#[repr(C)]
struct EraChunkExtraRaw {
    date: [u8; 8],
    decomp_size: [u8; 4],
    comp_tiger128: [u8; 16],
    name_offset: [u8; 3],
    _padding: u8,
}

fn swap_tiger_word_byte_order(mut hash: [u8; 16]) -> [u8; 16] {
    let (first_word, second_word) = hash.split_at_mut(8);
    first_word.reverse();
    second_word.reverse();
    hash
}

/// ERA chunk header extra data (32 bytes total with base header).
#[derive(Debug, Clone)]
pub struct EraChunkExtra {
    /// File modification date.
    pub date: u64,
    /// Decompressed size.
    pub decomp_size: u32,
    /// Tiger128 hash of compressed data.
    pub comp_tiger128: [u8; 16],
    /// Offset into filename table (3 bytes, big-endian).
    pub name_offset: u32,
}

impl EraChunkExtra {
    /// Size of the extra data (8 + 4 + 16 + 3 + 1 = 32 bytes).
    pub const SIZE: usize = size_of::<EraChunkExtraRaw>();

    /// Create a new chunk extra with given values.
    #[must_use]
    pub fn new(decomp_size: u32, name_offset: u32, comp_tiger128: [u8; 16]) -> Self {
        Self {
            date: 0,
            decomp_size,
            comp_tiger128,
            name_offset,
        }
    }

    /// Parse extra data from a byte slice.
    ///
    /// # Errors
    ///
    /// Returns an error if the chunk-extra record is truncated.
    pub fn from_bytes(buf: &[u8]) -> Result<Self> {
        let (raw, _): (Ref<_, EraChunkExtraRaw>, _) =
            Ref::from_prefix(buf).map_err(|_| Error::UnexpectedEof)?;

        // Tiger128 is stored on disk with BE 64-bit words; convert to native LE
        // so consumers can compare directly against Tiger::digest output.
        let tiger = swap_tiger_word_byte_order(raw.comp_tiger128);

        Ok(Self {
            date: u64::from_be_bytes(raw.date),
            decomp_size: u32::from_be_bytes(raw.decomp_size),
            comp_tiger128: tiger,
            name_offset: u32::from_be_bytes([
                0,
                raw.name_offset[0],
                raw.name_offset[1],
                raw.name_offset[2],
            ]),
        })
    }

    /// Serialize extra data to bytes.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut buf = [0u8; Self::SIZE];
        buf[0..8].copy_from_slice(&self.date.to_be_bytes());
        buf[8..12].copy_from_slice(&self.decomp_size.to_be_bytes());
        // Convert native LE Tiger128 back to BE words for on-disk storage
        let on_disk_hash = swap_tiger_word_byte_order(self.comp_tiger128);
        buf[12..28].copy_from_slice(&on_disk_hash);
        let offset_bytes = self.name_offset.to_be_bytes();
        buf[28] = offset_bytes[1];
        buf[29] = offset_bytes[2];
        buf[30] = offset_bytes[3];
        buf[31] = 0; // padding
        buf
    }
}

/// A file entry in an ERA archive.
#[derive(Debug, Clone)]
pub struct EraEntry {
    /// Base chunk header.
    pub chunk: EcfChunkHeader,
    /// Extra archive-specific data.
    pub extra: EraChunkExtra,
    /// Filename (if resolved).
    pub filename: Option<String>,
}

impl EraEntry {
    /// Get the decompressed size of this entry.
    #[must_use]
    pub fn decompressed_size(&self) -> u32 {
        self.extra.decomp_size
    }

    /// Get the compressed size of this entry.
    #[must_use]
    pub fn compressed_size(&self) -> u32 {
        self.chunk.size
    }

    /// Decompress data for this entry.
    ///
    /// # Errors
    ///
    /// Returns an error if the compression method is unknown or decompression
    /// fails.
    pub fn decompress(&self, compressed: &[u8]) -> Result<Vec<u8>> {
        match self.chunk.compression_method() {
            CompressionMethod::Stored => Ok(compressed.to_vec()),
            CompressionMethod::DeflateRaw => {
                let decompressed = miniz_oxide::inflate::decompress_to_vec(compressed)
                    .map_err(|e| Error::DecompressionError(format!("deflate raw: {e:?}")))?;
                Ok(decompressed)
            }
            CompressionMethod::DeflateStream => ecf::decompress(compressed).map_err(Error::from),
            CompressionMethod::Unknown(n) => Err(Error::DecompressionError(format!(
                "unknown compression method: {n}"
            ))),
        }
    }
}

/// Resolve a filename from the filename table.
///
/// The filename table is a sequence of null-terminated strings. Each entry
/// has a `name_offset` that points to its filename within this table.
#[must_use]
pub fn resolve_filename(table: &[u8], offset: u32) -> Option<String> {
    let start = usize::try_from(offset).ok()?;
    let remaining = table.get(start..)?;
    let end = remaining
        .iter()
        .position(|&b| b == 0)
        .map_or(table.len(), |p| start + p);
    String::from_utf8(table[start..end].to_vec()).ok()
}
