//! ERA archive format.
//!
//! ERA files are ECF-based archives used by Halo Wars to store game assets.
//! Files are encrypted using TEA cipher and must be decrypted on read.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use ecf::{CompressionMethod, EcfChunkHeader, EcfHeader};

use crate::error::{Error, Result};

/// Archive header magic number.
pub const ARCHIVE_HEADER_MAGIC: u32 = 0x05ABDBD8;

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
    pub const SIZE: usize = 16;

    /// Create a new archive header with default values.
    pub fn new() -> Self {
        Self {
            archive_magic: ARCHIVE_HEADER_MAGIC,
            signature_size: 0,
            reserved: [0, 0],
        }
    }

    /// Parse archive header extension from a byte slice.
    pub fn from_bytes(buf: &[u8]) -> Result<Self> {
        if buf.len() < Self::SIZE {
            return Err(Error::UnexpectedEof);
        }

        let archive_magic = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]);
        if archive_magic != ARCHIVE_HEADER_MAGIC {
            return Err(Error::InvalidArchiveMagic {
                expected: ARCHIVE_HEADER_MAGIC,
                found: archive_magic,
            });
        }

        Ok(Self {
            archive_magic,
            signature_size: u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]),
            reserved: [
                u32::from_be_bytes([buf[8], buf[9], buf[10], buf[11]]),
                u32::from_be_bytes([buf[12], buf[13], buf[14], buf[15]]),
            ],
        })
    }

    /// Serialize archive header extension to bytes.
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
    pub const SIZE: usize = 32;

    /// Create a new chunk extra with given values.
    pub fn new(decomp_size: u32, name_offset: u32, comp_tiger128: [u8; 16]) -> Self {
        Self {
            date: 0,
            decomp_size,
            comp_tiger128,
            name_offset,
        }
    }

    /// Parse extra data from a byte slice.
    pub fn from_bytes(buf: &[u8]) -> Result<Self> {
        if buf.len() < Self::SIZE {
            return Err(Error::UnexpectedEof);
        }

        Ok(Self {
            date: u64::from_be_bytes([
                buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7],
            ]),
            decomp_size: u32::from_be_bytes([buf[8], buf[9], buf[10], buf[11]]),
            comp_tiger128: buf[12..28].try_into().unwrap(),
            name_offset: u32::from_be_bytes([0, buf[28], buf[29], buf[30]]),
        })
    }

    /// Serialize extra data to bytes.
    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut buf = [0u8; Self::SIZE];
        buf[0..8].copy_from_slice(&self.date.to_be_bytes());
        buf[8..12].copy_from_slice(&self.decomp_size.to_be_bytes());
        buf[12..28].copy_from_slice(&self.comp_tiger128);
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
    pub fn decompressed_size(&self) -> u32 {
        self.extra.decomp_size
    }

    /// Get the compressed size of this entry.
    pub fn compressed_size(&self) -> u32 {
        self.chunk.size
    }

    /// Decompress data for this entry.
    pub fn decompress(&self, compressed: &[u8]) -> Result<Vec<u8>> {
        match self.chunk.compression_method() {
            CompressionMethod::Stored => Ok(compressed.to_vec()),
            CompressionMethod::DeflateRaw => {
                let decompressed = miniz_oxide::inflate::decompress_to_vec(compressed)
                    .map_err(|e| Error::DecompressionError(format!("deflate raw: {:?}", e)))?;
                Ok(decompressed)
            }
            CompressionMethod::DeflateStream => ecf::decompress(compressed).map_err(Error::from),
            CompressionMethod::Unknown(n) => Err(Error::DecompressionError(format!(
                "unknown compression method: {}",
                n
            ))),
        }
    }
}

/// Resolve a filename from the filename table.
///
/// The filename table is a sequence of null-terminated strings. Each entry
/// has a `name_offset` that points to its filename within this table.
pub fn resolve_filename(table: &[u8], offset: u32) -> Option<String> {
    if table.is_empty() || offset as usize >= table.len() {
        return None;
    }
    let start = offset as usize;
    let end = table[start..]
        .iter()
        .position(|&b| b == 0)
        .map(|p| start + p)
        .unwrap_or(table.len());
    String::from_utf8(table[start..end].to_vec()).ok()
}

/// Parse chunk headers from a byte slice at the given offset.
///
/// Returns the parsed entries and the new offset after all headers.
fn parse_chunk_headers(
    data: &[u8],
    offset: usize,
    ecf_header: &EcfHeader,
) -> Result<(Vec<EraEntry>, usize)> {
    let mut entries = Vec::with_capacity(ecf_header.num_chunks as usize);
    let mut pos = offset;
    let stride = EcfChunkHeader::SIZE + ecf_header.chunk_extra_data_size as usize;

    for _ in 0..ecf_header.num_chunks {
        if pos + EcfChunkHeader::SIZE > data.len() {
            return Err(Error::UnexpectedEof);
        }
        let chunk = EcfChunkHeader::from_bytes(&data[pos..pos + EcfChunkHeader::SIZE])?;
        pos += EcfChunkHeader::SIZE;

        let extra = if ecf_header.chunk_extra_data_size >= EraChunkExtra::SIZE as u16 {
            if pos + EraChunkExtra::SIZE > data.len() {
                return Err(Error::UnexpectedEof);
            }
            let extra = EraChunkExtra::from_bytes(&data[pos..])?;
            pos += ecf_header.chunk_extra_data_size as usize;
            extra
        } else {
            pos += ecf_header.chunk_extra_data_size as usize;
            EraChunkExtra {
                date: 0,
                decomp_size: chunk.size,
                comp_tiger128: [0; 16],
                name_offset: 0,
            }
        };

        entries.push(EraEntry {
            chunk,
            extra,
            filename: None,
        });
    }

    // Verify we didn't skip past stride boundaries
    let expected_end = offset + stride * ecf_header.num_chunks as usize;
    Ok((entries, expected_end))
}

/// Compressed entry data: (compressed_bytes, decompressed_size, tiger128_hash).
pub type CompressedEntryData = (Vec<u8>, u32, [u8; 16]);

/// An ERA archive reader that operates on a byte slice.
pub struct Reader<'a> {
    data: &'a [u8],
    /// ECF header.
    pub ecf_header: EcfHeader,
    /// Archive header extension.
    pub archive_header: EraArchiveHeader,
    /// File entries.
    pub entries: Vec<EraEntry>,
}

impl<'a> Reader<'a> {
    /// Parse an ERA archive from a decrypted byte slice.
    pub fn new(data: &'a [u8]) -> Result<Self> {
        if data.len() < EcfHeader::SIZE + EraArchiveHeader::SIZE {
            return Err(Error::UnexpectedEof);
        }

        // Parse ECF header
        let ecf_header = EcfHeader::from_bytes(&data[..EcfHeader::SIZE])?;

        // Parse archive header extension
        let archive_header = EraArchiveHeader::from_bytes(
            &data[EcfHeader::SIZE..EcfHeader::SIZE + EraArchiveHeader::SIZE],
        )?;

        // Chunk headers start after the full header
        let chunk_start = ecf_header.header_size as usize;
        let (mut entries, _) = parse_chunk_headers(data, chunk_start, &ecf_header)?;

        // Read and decompress filename table (always at index 0)
        let filename_table = if !entries.is_empty() {
            let e = &entries[0];
            let start = e.chunk.offset as usize;
            let end = start + e.chunk.size as usize;
            if end > data.len() {
                return Err(Error::UnexpectedEof);
            }
            entries[0].decompress(&data[start..end])?
        } else {
            Vec::new()
        };

        // Resolve filenames (skip index 0 which is the filename table)
        for entry in entries.iter_mut().skip(1) {
            entry.filename = resolve_filename(&filename_table, entry.extra.name_offset);
        }

        Ok(Self {
            data,
            ecf_header,
            archive_header,
            entries,
        })
    }

    /// Get the number of entries in the archive.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if the archive is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Get an entry by index.
    pub fn entry(&self, index: usize) -> Option<&EraEntry> {
        self.entries.get(index)
    }

    /// Iterate over all entries.
    pub fn iter(&self) -> impl Iterator<Item = &EraEntry> {
        self.entries.iter()
    }

    /// Find an entry by filename.
    pub fn find_by_name(&self, name: &str) -> Option<usize> {
        let name_lower = name.to_lowercase().replace('/', "\\");
        self.entries.iter().position(|e| {
            e.filename
                .as_ref()
                .is_some_and(|f| f.to_lowercase() == name_lower)
        })
    }

    /// Read and decompress the data for an entry.
    pub fn read_entry(&self, index: usize) -> Result<Vec<u8>> {
        let entry = self
            .entries
            .get(index)
            .ok_or(Error::ChunkIndexOutOfBounds {
                index,
                count: self.entries.len(),
            })?;

        let start = entry.chunk.offset as usize;
        let end = start + entry.chunk.size as usize;
        if end > self.data.len() {
            return Err(Error::UnexpectedEof);
        }

        entry.decompress(&self.data[start..end])
    }

    /// Read compressed data for an entry WITHOUT decompressing.
    ///
    /// Returns: (compressed_data, decompressed_size, tiger128_hash).
    pub fn read_entry_compressed(&self, index: usize) -> Result<CompressedEntryData> {
        let entry = self
            .entries
            .get(index)
            .ok_or(Error::ChunkIndexOutOfBounds {
                index,
                count: self.entries.len(),
            })?;

        let start = entry.chunk.offset as usize;
        let end = start + entry.chunk.size as usize;
        if end > self.data.len() {
            return Err(Error::UnexpectedEof);
        }

        Ok((
            self.data[start..end].to_vec(),
            entry.extra.decomp_size,
            entry.extra.comp_tiger128,
        ))
    }

    /// Read multiple entries sequentially.
    pub fn read_entries(&self, indices: &[usize]) -> Result<Vec<Vec<u8>>> {
        indices.iter().map(|&idx| self.read_entry(idx)).collect()
    }

    /// Read compressed data for multiple entries sequentially.
    pub fn read_entries_compressed(&self, indices: &[usize]) -> Result<Vec<CompressedEntryData>> {
        indices
            .iter()
            .map(|&idx| self.read_entry_compressed(idx))
            .collect()
    }
}
