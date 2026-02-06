//! ECF file writer.

use std::io::{Seek, Write};

use crate::{
    adler32, align_up, chunk_resource_flags, compress_bdeflate_stream, EcfChunkHeader, EcfHeader,
    Result, ECF_HEADER_MAGIC,
};

/// ECF file writer.
pub struct EcfWriter<W: Write + Seek> {
    writer: W,
    header: EcfHeader,
    chunks: Vec<(EcfChunkHeader, Vec<u8>)>,
}

impl<W: Write + Seek> EcfWriter<W> {
    /// Create a new ECF writer.
    pub fn new(writer: W, file_id: u32) -> Self {
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
        }
    }

    /// Add a chunk to the ECF file.
    pub fn add_chunk(&mut self, id: u64, data: Vec<u8>) {
        let chunk = EcfChunkHeader {
            id,
            offset: 0, // Will be calculated on finalize
            size: data.len() as u32,
            adler32: adler32(&data),
            flags: 0,
            alignment_log2: 2, // 4-byte alignment
            resource_flags: 0,
        };
        self.chunks.push((chunk, data));
    }

    /// Add a compressed chunk (little-endian, for PC).
    pub fn add_chunk_compressed(&mut self, id: u64, data: Vec<u8>) -> Result<()> {
        self.add_chunk_compressed_with_endian(id, data, false)
    }

    /// Add a compressed chunk (big-endian, for Xbox 360).
    pub fn add_chunk_compressed_be(&mut self, id: u64, data: Vec<u8>) -> Result<()> {
        self.add_chunk_compressed_with_endian(id, data, true)
    }

    /// Add a compressed chunk with specified endianness.
    pub fn add_chunk_compressed_with_endian(
        &mut self,
        id: u64,
        data: Vec<u8>,
        big_endian: bool,
    ) -> Result<()> {
        let wrapped_data = compress_bdeflate_stream(&data, big_endian)?;

        let chunk = EcfChunkHeader {
            id,
            offset: 0,
            size: wrapped_data.len() as u32,
            adler32: adler32(&wrapped_data),
            flags: 0,
            alignment_log2: 2,
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
        let mut data_offset = align_up(headers_size, 4);

        // Update chunk offsets
        for (chunk, data) in &mut self.chunks {
            chunk.offset = data_offset as u32;
            data_offset = align_up(data_offset + data.len(), chunk.alignment());
        }

        self.header.file_size = data_offset as u32;
        self.header.adler32 = 0; // TODO: Calculate proper checksum

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

