//! ERA archive writer

use std::fs::OpenOptions;
use std::io::{Seek, Write};
use std::path::Path;

use byteorder::{BigEndian, WriteBytesExt};
use flate2::write::DeflateEncoder;
use flate2::Compression;

use crate::crypto::TeaKeys;
use crate::encrypt_writer::EncryptWriter;
use crate::era::{EraArchiveHeader, EraChunkExtra};
use crate::error::Result;

/// ERA file ID constant
const ERA_FILE_ID: u32 = 0x0076C900;

/// ERA filename chunk ID (chunk 0)
const ERA_FILENAME_CHUNK_ID: u64 = 0x8DAFB100;

/// Default chunk ID for file entries
const ERA_FILE_CHUNK_ID: u64 = 0x8DAFB100;

/// A file to be added to an ERA archive
struct PendingFile {
    /// Filename (relative path with backslashes)
    filename: String,
    /// Uncompressed file data
    data: Vec<u8>,
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
        let compressed_names = compress_deflate(&filename_table)?;

        // Compress all file data
        let mut compressed_files = Vec::new();
        for file in &self.files {
            let compressed = compress_deflate(&file.data)?;
            compressed_files.push(compressed);
        }

        // Calculate layout
        let num_chunks = 1 + self.files.len(); // filename table + files
        let ecf_header_size = 32; // EcfHeader::SIZE
        let era_header_size = 16; // EraArchiveHeader::SIZE
        let chunk_header_size = 24; // EcfChunkHeader::SIZE
        let chunk_extra_size = 32; // EraChunkExtra::SIZE
        let total_header_size = ecf_header_size + era_header_size;
        let headers_size = total_header_size + (chunk_header_size + chunk_extra_size) * num_chunks;

        // 16-byte alignment
        let alignment = 16usize;
        let align_up = |n: usize| (n + alignment - 1) & !(alignment - 1);

        let mut data_offset = align_up(headers_size);

        // Build chunk info
        let mut chunk_offsets = Vec::new();
        let mut chunk_sizes = Vec::new();

        // Filename table chunk
        chunk_offsets.push(data_offset);
        chunk_sizes.push(compressed_names.len());
        data_offset = align_up(data_offset + compressed_names.len());

        // File chunks
        for compressed in &compressed_files {
            chunk_offsets.push(data_offset);
            chunk_sizes.push(compressed.len());
            data_offset = align_up(data_offset + compressed.len());
        }

        let file_size = data_offset;

        // Build ECF header data
        let ecf_header = EcfHeaderData {
            header_size: total_header_size as u32,
            file_size: file_size as u32,
            num_chunks: num_chunks as u16,
            id: ERA_FILE_ID,
            chunk_extra_data_size: chunk_extra_size as u16,
        };

        // Build chunk headers and extras
        let mut chunks = Vec::new();

        // Filename table chunk (index 0)
        chunks.push(ChunkData {
            id: ERA_FILENAME_CHUNK_ID,
            offset: chunk_offsets[0] as u32,
            size: chunk_sizes[0] as u32,
            decomp_size: filename_table.len() as u32,
            name_offset: 0, // filename table has no name
        });

        // File chunks
        for (i, file) in self.files.iter().enumerate() {
            chunks.push(ChunkData {
                id: ERA_FILE_CHUNK_ID,
                offset: chunk_offsets[i + 1] as u32,
                size: chunk_sizes[i + 1] as u32,
                decomp_size: file.data.len() as u32,
                name_offset: name_offsets[i],
            });
        }

        // Compute adler32 over header fields and chunk headers
        let adler32 = compute_header_adler32(&ecf_header, &chunks);

        // Write ECF header
        write_ecf_header(&mut writer, &ecf_header, adler32)?;

        // Write ERA archive header
        let archive_header = EraArchiveHeader::new();
        archive_header.write(&mut writer)?;

        // Write chunk headers with extra data
        for chunk in &chunks {
            write_chunk_header(&mut writer, chunk)?;
        }

        // Write chunk data with padding
        write_chunk_data(
            &mut writer,
            &chunk_offsets,
            &compressed_names,
            &compressed_files,
        )?;

        // Flush to ensure all data is written (important for EncryptWriter)
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
}

/// Compress data using raw deflate
fn compress_deflate(data: &[u8]) -> Result<Vec<u8>> {
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data)?;
    Ok(encoder.finish()?)
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
    let extra = EraChunkExtra::new(chunk.decomp_size, chunk.name_offset);
    extra.write(writer)?;

    Ok(())
}

/// Write chunk data with padding
fn write_chunk_data<W: Write + Seek>(
    writer: &mut W,
    offsets: &[usize],
    filename_table: &[u8],
    files: &[Vec<u8>],
) -> Result<()> {
    // Write filename table
    let current_pos = writer.stream_position()? as usize;
    if offsets[0] > current_pos {
        writer.write_all(&vec![0u8; offsets[0] - current_pos])?;
    }
    writer.write_all(filename_table)?;

    // Write file data
    for (i, data) in files.iter().enumerate() {
        let current_pos = writer.stream_position()? as usize;
        if offsets[i + 1] > current_pos {
            writer.write_all(&vec![0u8; offsets[i + 1] - current_pos])?;
        }
        writer.write_all(data)?;
    }

    Ok(())
}
