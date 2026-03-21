//! ERA archive format
//!
//! ERA files are ECF-based archives used by Halo Wars to store game assets.
//! Files are encrypted using TEA cipher and must be decrypted on read.

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom, Write};
use std::path::Path;

use ecf::{CompressionMethod, EcfChunkHeader, EcfHeader};

use crate::crypto::TeaKeys;
use crate::decrypt_reader::DecryptReader;
use crate::error::{Error, Result};

/// Archive header magic number
pub const ARCHIVE_HEADER_MAGIC: u32 = 0x05ABDBD8;

/// ERA archive header extension (16 bytes after ECF header)
#[derive(Debug, Clone)]
pub struct EraArchiveHeader {
    /// Archive-specific magic (0x05ABDBD8)
    pub archive_magic: u32,
    /// Size of digital signature
    pub signature_size: u32,
    /// Reserved fields
    pub reserved: [u32; 2],
}

impl EraArchiveHeader {
    /// Size of the archive header extension
    pub const SIZE: usize = 16;

    /// Create a new archive header with default values
    pub fn new() -> Self {
        Self {
            archive_magic: ARCHIVE_HEADER_MAGIC,
            signature_size: 0,
            reserved: [0, 0],
        }
    }

    /// Read archive header extension from reader
    pub fn read<R: Read>(reader: &mut R) -> Result<Self> {
        let mut buf = [0u8; Self::SIZE];
        reader.read_exact(&mut buf)?;

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

    /// Write archive header extension to writer
    pub fn write<W: Write>(&self, writer: &mut W) -> Result<()> {
        let mut buf = [0u8; Self::SIZE];
        buf[0..4].copy_from_slice(&self.archive_magic.to_be_bytes());
        buf[4..8].copy_from_slice(&self.signature_size.to_be_bytes());
        buf[8..12].copy_from_slice(&self.reserved[0].to_be_bytes());
        buf[12..16].copy_from_slice(&self.reserved[1].to_be_bytes());
        writer.write_all(&buf)?;
        Ok(())
    }
}

impl Default for EraArchiveHeader {
    fn default() -> Self {
        Self::new()
    }
}

/// ERA chunk header extra data (32 bytes total with base header)
#[derive(Debug, Clone)]
pub struct EraChunkExtra {
    /// File modification date
    pub date: u64,
    /// Decompressed size
    pub decomp_size: u32,
    /// Tiger128 hash of compressed data
    pub comp_tiger128: [u8; 16],
    /// Offset into filename table (3 bytes, big-endian)
    pub name_offset: u32,
}

impl EraChunkExtra {
    /// Size of the extra data (8 + 4 + 16 + 3 + 1 = 32 bytes)
    pub const SIZE: usize = 32;

    /// Create a new chunk extra with given values
    pub fn new(decomp_size: u32, name_offset: u32, comp_tiger128: [u8; 16]) -> Self {
        Self {
            date: 0,
            decomp_size,
            comp_tiger128,
            name_offset,
        }
    }

    /// Read extra data from reader
    pub fn read<R: Read>(reader: &mut R) -> Result<Self> {
        let mut buf = [0u8; Self::SIZE];
        reader.read_exact(&mut buf)?;

        Ok(Self {
            date: u64::from_be_bytes([
                buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7],
            ]),
            decomp_size: u32::from_be_bytes([buf[8], buf[9], buf[10], buf[11]]),
            comp_tiger128: buf[12..28].try_into().unwrap(),
            // 3-byte big-endian offset at bytes 28, 29, 30
            name_offset: u32::from_be_bytes([0, buf[28], buf[29], buf[30]]),
        })
    }

    /// Write extra data to writer
    pub fn write<W: Write>(&self, writer: &mut W) -> Result<()> {
        let mut buf = [0u8; Self::SIZE];
        buf[0..8].copy_from_slice(&self.date.to_be_bytes());
        buf[8..12].copy_from_slice(&self.decomp_size.to_be_bytes());
        buf[12..28].copy_from_slice(&self.comp_tiger128);
        // 3-byte big-endian offset at bytes 28, 29, 30 (byte 31 is padding)
        let offset_bytes = self.name_offset.to_be_bytes();
        buf[28] = offset_bytes[1];
        buf[29] = offset_bytes[2];
        buf[30] = offset_bytes[3];
        buf[31] = 0; // padding
        writer.write_all(&buf)?;
        Ok(())
    }
}

/// A file entry in an ERA archive
#[derive(Debug, Clone)]
pub struct EraEntry {
    /// Base chunk header
    pub chunk: EcfChunkHeader,
    /// Extra archive-specific data
    pub extra: EraChunkExtra,
    /// Filename (if resolved)
    pub filename: Option<String>,
}

impl EraEntry {
    /// Get the decompressed size of this entry
    pub fn decompressed_size(&self) -> u32 {
        self.extra.decomp_size
    }

    /// Get the compressed size of this entry
    pub fn compressed_size(&self) -> u32 {
        self.chunk.size
    }

    /// Decompress data for this entry
    ///
    /// Takes compressed bytes and decompresses them according to the entry's
    /// compression method. This is the shared decompression logic used by
    /// both `EraArchive` and `MmapEraArchive`.
    pub fn decompress(&self, compressed: &[u8]) -> Result<Vec<u8>> {
        match self.chunk.compression_method() {
            CompressionMethod::Stored => Ok(compressed.to_vec()),
            CompressionMethod::DeflateRaw => {
                use flate2::read::DeflateDecoder;
                let mut decoder = DeflateDecoder::new(compressed);
                let mut decompressed = vec![0u8; self.extra.decomp_size as usize];
                decoder
                    .read_exact(&mut decompressed)
                    .map_err(|e| Error::DecompressionError(format!("deflate raw: {}", e)))?;
                Ok(decompressed)
            }
            CompressionMethod::DeflateStream => {
                ecf::decompress_bdeflate_stream(compressed).map_err(Error::from)
            }
            CompressionMethod::Unknown(n) => Err(Error::DecompressionError(format!(
                "unknown compression method: {}",
                n
            ))),
        }
    }
}

/// Resolve a filename from the filename table
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

/// Parse chunk headers from a reader
///
/// This is shared logic used by both `EraArchive` and `MmapEraArchive` to
/// parse the chunk headers after the main ECF and ERA headers.
pub fn parse_chunk_headers<R: Read + Seek>(
    reader: &mut R,
    ecf_header: &EcfHeader,
) -> Result<Vec<EraEntry>> {
    let mut entries = Vec::with_capacity(ecf_header.num_chunks as usize);

    for _ in 0..ecf_header.num_chunks {
        let chunk = EcfChunkHeader::read(reader)?;

        let extra = if ecf_header.chunk_extra_data_size >= EraChunkExtra::SIZE as u16 {
            let extra = EraChunkExtra::read(reader)?;
            let remaining = ecf_header.chunk_extra_data_size as i64 - EraChunkExtra::SIZE as i64;
            if remaining > 0 {
                reader.seek(SeekFrom::Current(remaining))?;
            }
            extra
        } else {
            if ecf_header.chunk_extra_data_size > 0 {
                reader.seek(SeekFrom::Current(ecf_header.chunk_extra_data_size as i64))?;
            }
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

    Ok(entries)
}

/// An ERA archive reader
pub struct EraArchive<R> {
    reader: R,
    /// ECF header
    pub ecf_header: EcfHeader,
    /// Archive header extension
    pub archive_header: EraArchiveHeader,
    /// File entries
    pub entries: Vec<EraEntry>,
    /// Filename table (raw bytes)
    #[allow(dead_code)]
    filename_table: Vec<u8>,
}

impl EraArchive<DecryptReader<BufReader<File>>> {
    /// Open an ERA archive from a file path
    ///
    /// The file is automatically decrypted using the default archive password.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let keys = TeaKeys::default_archive_keys();
        let decrypt_reader = DecryptReader::new(reader, keys);
        Self::new(decrypt_reader)
    }
}

impl<R: Read + Seek> EraArchive<R> {
    /// Create a new ERA archive reader
    pub fn new(mut reader: R) -> Result<Self> {
        // Read ECF header
        let ecf_header = EcfHeader::read(&mut reader)?;

        // Read archive header extension
        let archive_header = EraArchiveHeader::read(&mut reader)?;

        // Skip signature data (seek to where chunk headers start)
        let signature_skip =
            ecf_header.header_size as i64 - EcfHeader::SIZE as i64 - EraArchiveHeader::SIZE as i64;
        if signature_skip > 0 {
            reader.seek(SeekFrom::Current(signature_skip))?;
        }

        // Parse chunk headers using shared function
        let mut entries = parse_chunk_headers(&mut reader, &ecf_header)?;

        // Read and decompress filename table (always at index 0)
        let filename_table = if !entries.is_empty() {
            Self::read_filename_table(&mut reader, &entries[0])?
        } else {
            Vec::new()
        };

        // Resolve filenames for all entries (skip index 0 which is the filename table)
        for (i, entry) in entries.iter_mut().enumerate() {
            if i > 0 {
                entry.filename = resolve_filename(&filename_table, entry.extra.name_offset);
            }
        }

        Ok(Self {
            reader,
            ecf_header,
            archive_header,
            entries,
            filename_table,
        })
    }

    fn read_filename_table(reader: &mut R, entry: &EraEntry) -> Result<Vec<u8>> {
        reader.seek(SeekFrom::Start(entry.chunk.offset as u64))?;
        let mut compressed = vec![0u8; entry.chunk.size as usize];
        reader.read_exact(&mut compressed)?;
        entry.decompress(&compressed)
    }

    /// Get the number of entries in the archive
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if the archive is empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Get an entry by index
    pub fn entry(&self, index: usize) -> Option<&EraEntry> {
        self.entries.get(index)
    }

    /// Iterate over all entries
    pub fn iter(&self) -> impl Iterator<Item = &EraEntry> {
        self.entries.iter()
    }

    /// Find an entry by filename
    pub fn find_by_name(&self, name: &str) -> Option<usize> {
        let name_lower = name.to_lowercase().replace('/', "\\");
        self.entries.iter().position(|e| {
            e.filename
                .as_ref()
                .is_some_and(|f| f.to_lowercase() == name_lower)
        })
    }

    /// Read and decompress the data for an entry
    pub fn read_entry(&mut self, index: usize) -> Result<Vec<u8>> {
        let entry = self
            .entries
            .get(index)
            .ok_or(Error::ChunkIndexOutOfBounds {
                index,
                count: self.entries.len(),
            })?
            .clone();

        self.reader
            .seek(SeekFrom::Start(entry.chunk.offset as u64))?;
        let mut compressed = vec![0u8; entry.chunk.size as usize];
        self.reader.read_exact(&mut compressed)?;

        entry.decompress(&compressed)
    }

    /// Read compressed data for an entry WITHOUT decompressing
    ///
    /// This is useful for copying files between archives without the overhead
    /// of decompression and recompression. Returns the compressed bytes along
    /// with metadata needed to write to another archive.
    ///
    /// Returns: (compressed_data, decompressed_size, tiger128_hash)
    pub fn read_entry_compressed(&mut self, index: usize) -> Result<crate::CompressedEntryData> {
        let entry = self
            .entries
            .get(index)
            .ok_or(Error::ChunkIndexOutOfBounds {
                index,
                count: self.entries.len(),
            })?
            .clone();

        self.reader
            .seek(SeekFrom::Start(entry.chunk.offset as u64))?;
        let mut compressed = vec![0u8; entry.chunk.size as usize];
        self.reader.read_exact(&mut compressed)?;

        Ok((
            compressed,
            entry.extra.decomp_size,
            entry.extra.comp_tiger128,
        ))
    }

    /// Read multiple entries sequentially
    ///
    /// Note: For parallel reading, use `MmapEraArchive` which supports
    /// `read_entries_parallel()` for concurrent decompression.
    pub fn read_entries(&mut self, indices: &[usize]) -> Result<Vec<Vec<u8>>> {
        indices.iter().map(|&idx| self.read_entry(idx)).collect()
    }

    /// Read compressed data for multiple entries sequentially
    ///
    /// Note: For parallel reading, use `MmapEraArchive` which supports
    /// `read_entries_compressed_parallel()` for concurrent access.
    pub fn read_entries_compressed(
        &mut self,
        indices: &[usize],
    ) -> Result<Vec<crate::CompressedEntryData>> {
        indices
            .iter()
            .map(|&idx| self.read_entry_compressed(idx))
            .collect()
    }
}
