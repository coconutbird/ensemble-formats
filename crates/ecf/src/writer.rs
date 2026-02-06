//! ECF file writer.

use std::io::{Seek, Write};

use crate::{
    adler32, align_up, chunk_resource_flags, compress_bdeflate_stream, EcfChunkHeader, EcfHeader,
    Result, ECF_HEADER_MAGIC,
};

/// Default alignment for chunks (16-byte, log2 = 4).
pub const DEFAULT_ALIGNMENT_LOG2: u8 = 4;

/// ECF file writer.
pub struct EcfWriter<W: Write + Seek> {
    writer: W,
    header: EcfHeader,
    chunks: Vec<(EcfChunkHeader, Vec<u8>)>,
    /// Default alignment for new chunks (log2 value).
    default_alignment_log2: u8,
}

impl<W: Write + Seek> EcfWriter<W> {
    /// Create a new ECF writer with default 16-byte alignment.
    pub fn new(writer: W, file_id: u32) -> Self {
        Self::with_alignment(writer, file_id, DEFAULT_ALIGNMENT_LOG2)
    }

    /// Create a new ECF writer with specified alignment.
    pub fn with_alignment(writer: W, file_id: u32, alignment_log2: u8) -> Self {
        Self {
            writer,
            header: EcfHeader {
                magic: ECF_HEADER_MAGIC,
                header_size: EcfHeader::SIZE as u32,
                adler32: 0,
                file_size: 0,
                num_chunks: 0,
                flags: 0,
                id: file_id,
                chunk_extra_data_size: 0,
            },
            chunks: Vec::new(),
            default_alignment_log2: alignment_log2,
        }
    }

    /// Add a chunk to the ECF file.
    pub fn add_chunk(&mut self, id: u64, data: Vec<u8>) {
        self.add_chunk_with_alignment(id, data, self.default_alignment_log2)
    }

    /// Add a chunk with specific alignment.
    pub fn add_chunk_with_alignment(&mut self, id: u64, data: Vec<u8>, alignment_log2: u8) {
        let chunk = EcfChunkHeader {
            id,
            offset: 0, // Will be calculated on finalize
            size: data.len() as u32,
            adler32: adler32(&data),
            flags: 0,
            alignment_log2,
            resource_flags: 0,
        };
        self.chunks.push((chunk, data));
    }

    /// Add a compressed chunk (little-endian, for PC).
    pub fn add_chunk_compressed(&mut self, id: u64, data: Vec<u8>) -> Result<()> {
        self.add_chunk_compressed_with_options(id, data, false, self.default_alignment_log2)
    }

    /// Add a compressed chunk (big-endian, for Xbox 360).
    pub fn add_chunk_compressed_be(&mut self, id: u64, data: Vec<u8>) -> Result<()> {
        self.add_chunk_compressed_with_options(id, data, true, self.default_alignment_log2)
    }

    /// Add a compressed chunk with specified endianness and alignment.
    pub fn add_chunk_compressed_with_options(
        &mut self,
        id: u64,
        data: Vec<u8>,
        big_endian: bool,
        alignment_log2: u8,
    ) -> Result<()> {
        let wrapped_data = compress_bdeflate_stream(&data, big_endian)?;

        let chunk = EcfChunkHeader {
            id,
            offset: 0,
            size: wrapped_data.len() as u32,
            adler32: adler32(&wrapped_data),
            flags: 0,
            alignment_log2,
            resource_flags: chunk_resource_flags::IS_DEFLATE_STREAM,
        };
        self.chunks.push((chunk, wrapped_data));
        Ok(())
    }

    /// Finalize and write the ECF file.
    pub fn finalize(mut self) -> Result<()> {
        let num_chunks = self.chunks.len() as u16;
        self.header.num_chunks = num_chunks;

        // Calculate data offset (after header and chunk headers)
        let headers_size = EcfHeader::SIZE + (EcfChunkHeader::SIZE * self.chunks.len());

        // Use the first chunk's alignment for the initial data offset, or default to 16-byte
        let initial_alignment = self
            .chunks
            .first()
            .map(|(c, _)| c.alignment())
            .unwrap_or(16);
        let mut data_offset = align_up(headers_size, initial_alignment);

        // Update chunk offsets
        for (chunk, data) in &mut self.chunks {
            chunk.offset = data_offset as u32;
            data_offset = align_up(data_offset + data.len(), chunk.alignment());
        }

        self.header.file_size = data_offset as u32;

        // Build the header and chunk headers to compute adler32
        // The adler32 covers bytes 12-31 of the header (after adler32 field) plus all chunk headers
        let mut checksum_data = Vec::new();

        // Header bytes after adler32: file_size(4) + num_chunks(2) + flags(2) + id(4) + chunk_extra_data_size(2) + pad(6) = 20 bytes
        use byteorder::{BigEndian, WriteBytesExt};
        checksum_data.write_u32::<BigEndian>(self.header.file_size)?;
        checksum_data.write_u16::<BigEndian>(self.header.num_chunks)?;
        checksum_data.write_u16::<BigEndian>(self.header.flags)?;
        checksum_data.write_u32::<BigEndian>(self.header.id)?;
        checksum_data.write_u16::<BigEndian>(self.header.chunk_extra_data_size)?;
        checksum_data.write_u16::<BigEndian>(0)?; // pad0
        checksum_data.write_u32::<BigEndian>(0)?; // pad1

        // Add chunk headers
        for (chunk, _) in &self.chunks {
            checksum_data.write_u64::<BigEndian>(chunk.id)?;
            checksum_data.write_u32::<BigEndian>(chunk.offset)?;
            checksum_data.write_u32::<BigEndian>(chunk.size)?;
            checksum_data.write_u32::<BigEndian>(chunk.adler32)?;
            checksum_data.write_u8(chunk.flags)?;
            checksum_data.write_u8(chunk.alignment_log2)?;
            checksum_data.write_u16::<BigEndian>(chunk.resource_flags)?;
        }

        self.header.adler32 = adler32(&checksum_data);

        // Write header
        self.header.write(&mut self.writer)?;

        // Write chunk headers
        for (chunk, _) in &self.chunks {
            chunk.write(&mut self.writer)?;
        }

        // Write chunk data with alignment padding
        for (chunk, data) in &self.chunks {
            let current_pos = self.writer.stream_position()? as usize;
            let padding = chunk.offset as usize - current_pos;
            if padding > 0 {
                self.writer.write_all(&vec![0u8; padding])?;
            }
            self.writer.write_all(&data)?;
        }

        Ok(())
    }
}

