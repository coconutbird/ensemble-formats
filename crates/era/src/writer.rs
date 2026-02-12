//! ERA archive writer

use std::fs::OpenOptions;
use std::io::{Seek, Write};
use std::path::Path;

use byteorder::{BigEndian, WriteBytesExt};
use flate2::write::DeflateEncoder;
use flate2::Compression;
use tiger::{Digest, Tiger};

use crate::crypto::TeaKeys;
use crate::encrypt_writer::EncryptWriter;
use crate::era::{EraArchiveHeader, EraChunkExtra};
use crate::error::Result;

/// ERA file ID constant
const ERA_FILE_ID: u32 = 0x0076C900;

/// ERA chunk ID (used for both filename table and file entries)
const ERA_CHUNK_ID: u64 = 0x8DAFB100;

/// A file to be added to an ERA archive
struct PendingFile {
    /// Filename (relative path with backslashes)
    filename: String,
    /// Uncompressed file data
    data: Vec<u8>,
}

/// Compressed data with its Tiger128 hash
struct CompressedData {
    /// Compressed bytes
    data: Vec<u8>,
    /// Tiger128 hash of compressed data
    tiger128: [u8; 16],
}

/// ERA archive writer
pub struct EraWriter {
    files: Vec<PendingFile>,
}

impl EraWriter {
    /// Create a new ERA writer
    pub fn new() -> Self {
        Self { files: Vec::new() }
    }

    /// Add a file to the archive
    pub fn add_file(&mut self, filename: impl Into<String>, data: Vec<u8>) {
        let filename = filename.into().replace('/', "\\");
        self.files.push(PendingFile { filename, data });
    }

    /// Write the archive to a file
    pub fn write_to_file<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        // Open with read+write since EncryptWriter may need to read back blocks
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)?;
        let keys = TeaKeys::default_archive_keys();
        let encrypt_writer = EncryptWriter::new(file, keys);
        self.write(encrypt_writer)
    }

    /// Write the archive to a writer (should be an EncryptWriter for proper encryption)
    pub fn write<W: Write + Seek>(&self, mut writer: W) -> Result<()> {
        // Build filename table
        let mut filename_table = Vec::new();
        let mut name_offsets = Vec::new();
        for file in &self.files {
            name_offsets.push(filename_table.len() as u32);
            filename_table.extend_from_slice(file.filename.as_bytes());
            filename_table.push(0); // null terminator
        }

        // Compress filename table
        let compressed_names = compress_data(&filename_table)?;

        // Compress all file data
        let compressed_files: Vec<_> = self
            .files
            .iter()
            .map(|f| compress_data(&f.data))
            .collect::<Result<_>>()?;

        // Calculate layout
        let num_chunks = 1 + self.files.len(); // filename table + files
        let total_header_size = 32 + 16; // EcfHeader + EraArchiveHeader
        let chunk_header_size = 24 + 32; // EcfChunkHeader + EraChunkExtra
        let headers_size = total_header_size + chunk_header_size * num_chunks;

        let mut data_offset = align16(headers_size);
        let mut chunks = Vec::with_capacity(num_chunks);

        // Filename table chunk (index 0)
        chunks.push(ChunkData {
            id: ERA_CHUNK_ID,
            offset: data_offset as u32,
            size: compressed_names.data.len() as u32,
            decomp_size: filename_table.len() as u32,
            name_offset: 0,
            comp_tiger128: compressed_names.tiger128,
        });
        data_offset = align16(data_offset + compressed_names.data.len());

        // File chunks
        for (i, (file, compressed)) in self.files.iter().zip(&compressed_files).enumerate() {
            chunks.push(ChunkData {
                id: ERA_CHUNK_ID,
                offset: data_offset as u32,
                size: compressed.data.len() as u32,
                decomp_size: file.data.len() as u32,
                name_offset: name_offsets[i],
                comp_tiger128: compressed.tiger128,
            });
            data_offset = align16(data_offset + compressed.data.len());
        }

        // Build ECF header
        let ecf_header = EcfHeaderData {
            header_size: total_header_size as u32,
            file_size: data_offset as u32,
            num_chunks: num_chunks as u16,
            id: ERA_FILE_ID,
            chunk_extra_data_size: 32,
        };

        // Compute adler32 over header fields and chunk headers
        let adler32 = compute_header_adler32(&ecf_header, &chunks);

        // Write ECF header
        write_ecf_header(&mut writer, &ecf_header, adler32)?;

        // Write ERA archive header
        EraArchiveHeader::new().write(&mut writer)?;

        // Write chunk headers
        for chunk in &chunks {
            write_chunk_header(&mut writer, chunk)?;
        }

        // Write chunk data with padding
        write_chunk_data(&mut writer, &chunks, &compressed_names, &compressed_files)?;

        writer.flush()?;
        Ok(())
    }
}

impl Default for EraWriter {
    fn default() -> Self {
        Self::new()
    }
}

/// Internal ECF header data
struct EcfHeaderData {
    header_size: u32,
    file_size: u32,
    num_chunks: u16,
    id: u32,
    chunk_extra_data_size: u16,
}

/// Internal chunk data
struct ChunkData {
    id: u64,
    offset: u32,
    size: u32,
    decomp_size: u32,
    name_offset: u32,
    comp_tiger128: [u8; 16],
}

/// Align to 16-byte boundary
fn align16(n: usize) -> usize {
    (n + 15) & !15
}

/// Compress data and compute its Tiger128 hash
fn compress_data(data: &[u8]) -> Result<CompressedData> {
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data)?;
    let compressed = encoder.finish()?;

    // Tiger128 = first 16 bytes of Tiger-192
    let hash = Tiger::digest(&compressed);
    let mut tiger128 = [0u8; 16];
    tiger128.copy_from_slice(&hash[..16]);

    Ok(CompressedData {
        data: compressed,
        tiger128,
    })
}

/// Compute adler32 over header fields and chunk headers
fn compute_header_adler32(header: &EcfHeaderData, chunks: &[ChunkData]) -> u32 {
    let mut data = Vec::new();

    // Header bytes after adler32 field
    data.extend_from_slice(&header.file_size.to_be_bytes());
    data.extend_from_slice(&header.num_chunks.to_be_bytes());
    data.extend_from_slice(&0u16.to_be_bytes()); // flags
    data.extend_from_slice(&header.id.to_be_bytes());
    data.extend_from_slice(&header.chunk_extra_data_size.to_be_bytes());
    data.extend_from_slice(&0u16.to_be_bytes()); // pad0
    data.extend_from_slice(&0u32.to_be_bytes()); // pad1

    // Chunk headers (base headers only, without extra data)
    for chunk in chunks {
        data.extend_from_slice(&chunk.id.to_be_bytes());
        data.extend_from_slice(&chunk.offset.to_be_bytes());
        data.extend_from_slice(&chunk.size.to_be_bytes());
        data.extend_from_slice(&0u32.to_be_bytes()); // adler32
        data.push(0); // flags
        data.push(4); // alignment_log2 = 4 (16 bytes)
        data.extend_from_slice(&0x0001u16.to_be_bytes()); // resource_flags: deflate raw
    }

    ecf::adler32(&data)
}

/// Write ECF header
fn write_ecf_header<W: Write>(writer: &mut W, header: &EcfHeaderData, adler32: u32) -> Result<()> {
    writer.write_u32::<BigEndian>(0xDABA7737)?; // ECF magic
    writer.write_u32::<BigEndian>(header.header_size)?;
    writer.write_u32::<BigEndian>(adler32)?;
    writer.write_u32::<BigEndian>(header.file_size)?;
    writer.write_u16::<BigEndian>(header.num_chunks)?;
    writer.write_u16::<BigEndian>(0)?; // flags
    writer.write_u32::<BigEndian>(header.id)?;
    writer.write_u16::<BigEndian>(header.chunk_extra_data_size)?;
    writer.write_u16::<BigEndian>(0)?; // pad0
    writer.write_u32::<BigEndian>(0)?; // pad1
    Ok(())
}

/// Write a chunk header with extra data
fn write_chunk_header<W: Write>(writer: &mut W, chunk: &ChunkData) -> Result<()> {
    // EcfChunkHeader (24 bytes)
    writer.write_u64::<BigEndian>(chunk.id)?;
    writer.write_u32::<BigEndian>(chunk.offset)?;
    writer.write_u32::<BigEndian>(chunk.size)?;
    writer.write_u32::<BigEndian>(0)?; // adler32
    writer.write_u8(1)?; // flags: 1 = DeflateRaw compression
    writer.write_u8(4)?; // alignment_log2 = 4 (16 bytes)
    writer.write_u16::<BigEndian>(0x0000)?; // resource_flags

    // EraChunkExtra (32 bytes)
    let extra = EraChunkExtra::new(chunk.decomp_size, chunk.name_offset, chunk.comp_tiger128);
    extra.write(writer)?;

    Ok(())
}

/// Write chunk data with padding
fn write_chunk_data<W: Write + Seek>(
    writer: &mut W,
    chunks: &[ChunkData],
    filename_table: &CompressedData,
    files: &[CompressedData],
) -> Result<()> {
    // Write filename table (first chunk)
    write_padded(writer, chunks[0].offset as usize, &filename_table.data)?;

    // Write file data
    for (chunk, file) in chunks[1..].iter().zip(files) {
        write_padded(writer, chunk.offset as usize, &file.data)?;
    }

    Ok(())
}

/// Write data at offset, padding with zeros if needed
fn write_padded<W: Write + Seek>(writer: &mut W, offset: usize, data: &[u8]) -> Result<()> {
    let current = writer.stream_position()? as usize;
    if offset > current {
        writer.write_all(&vec![0u8; offset - current])?;
    }
    writer.write_all(data)?;
    Ok(())
}
