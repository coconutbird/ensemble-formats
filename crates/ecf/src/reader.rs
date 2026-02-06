//! ECF file reader.

use std::io::{Read, Seek, SeekFrom};

use crate::{
    chunk_resource_flags, decompress_bdeflate_stream, EcfChunkHeader, EcfHeader, Error, Result,
};

/// ECF file reader.
pub struct EcfReader<R: Read + Seek> {
    reader: R,
    header: EcfHeader,
    chunks: Vec<EcfChunkHeader>,
}

impl<R: Read + Seek> EcfReader<R> {
    /// Create a new ECF reader from a Read + Seek source.
    pub fn new(mut reader: R) -> Result<Self> {
        let header = EcfHeader::read(&mut reader)?;

        // Skip extra header data
        let extra_size = header.header_size as usize - EcfHeader::SIZE;
        if extra_size > 0 {
            reader.seek(SeekFrom::Current(extra_size as i64))?;
        }

        // Read chunk headers
        let mut chunks = Vec::with_capacity(header.num_chunks as usize);
        for _ in 0..header.num_chunks {
            let chunk = EcfChunkHeader::read(&mut reader)?;
            // Skip extra chunk data
            if header.chunk_extra_data_size > 0 {
                reader.seek(SeekFrom::Current(header.chunk_extra_data_size as i64))?;
            }
            chunks.push(chunk);
        }

        Ok(Self {
            reader,
            header,
            chunks,
        })
    }

    /// Get the ECF header.
    pub fn header(&self) -> &EcfHeader {
        &self.header
    }

    /// Get the chunk headers.
    pub fn chunks(&self) -> &[EcfChunkHeader] {
        &self.chunks
    }

    /// Find a chunk by ID.
    pub fn find_chunk(&self, id: u64) -> Option<&EcfChunkHeader> {
        self.chunks.iter().find(|c| c.id == id)
    }

    /// Read chunk data by index.
    ///
    /// If the chunk is compressed (deflate stream flag set), the data will be
    /// automatically decompressed before being returned.
    pub fn read_chunk_data(&mut self, index: usize) -> Result<Vec<u8>> {
        if index >= self.chunks.len() {
            return Err(Error::ChunkNotFound(index as u64));
        }
        let chunk = &self.chunks[index];
        self.reader.seek(SeekFrom::Start(chunk.offset as u64))?;
        let mut data = vec![0u8; chunk.size as usize];
        self.reader.read_exact(&mut data)?;

        // Check if the chunk data is deflate compressed
        if (chunk.resource_flags & chunk_resource_flags::IS_DEFLATE_STREAM) != 0 {
            decompress_bdeflate_stream(&data)
        } else {
            Ok(data)
        }
    }

    /// Read chunk data by ID.
    pub fn read_chunk_data_by_id(&mut self, id: u64) -> Result<Vec<u8>> {
        let index = self
            .chunks
            .iter()
            .position(|c| c.id == id)
            .ok_or(Error::ChunkNotFound(id))?;
        self.read_chunk_data(index)
    }
}

