//! ECF header structures.

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use std::io::{Read, Write};

use crate::{CompressionMethod, Error, Result, ECF_HEADER_MAGIC, ECF_INVERTED_HEADER_MAGIC};

/// ECF file header.
#[derive(Debug, Clone, Default)]
pub struct EcfHeader {
    /// Header magic number (should be ECF_HEADER_MAGIC).
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

    /// Read an ECF header from a reader.
    pub fn read<R: Read>(reader: &mut R) -> Result<Self> {
        let magic = reader.read_u32::<BigEndian>()?;
        if magic != ECF_HEADER_MAGIC && magic != ECF_INVERTED_HEADER_MAGIC {
            return Err(Error::InvalidMagic {
                expected: ECF_HEADER_MAGIC,
                found: magic,
            });
        }

        let header = Self {
            magic,
            header_size: reader.read_u32::<BigEndian>()?,
            adler32: reader.read_u32::<BigEndian>()?,
            file_size: reader.read_u32::<BigEndian>()?,
            num_chunks: reader.read_u16::<BigEndian>()?,
            flags: reader.read_u16::<BigEndian>()?,
            id: reader.read_u32::<BigEndian>()?,
            chunk_extra_data_size: reader.read_u16::<BigEndian>()?,
        };
        // Read padding bytes
        let _pad0 = reader.read_u16::<BigEndian>()?;
        let _pad1 = reader.read_u32::<BigEndian>()?;
        Ok(header)
    }

    /// Write an ECF header to a writer.
    pub fn write<W: Write>(&self, writer: &mut W) -> Result<()> {
        writer.write_u32::<BigEndian>(self.magic)?;
        writer.write_u32::<BigEndian>(self.header_size)?;
        writer.write_u32::<BigEndian>(self.adler32)?;
        writer.write_u32::<BigEndian>(self.file_size)?;
        writer.write_u16::<BigEndian>(self.num_chunks)?;
        writer.write_u16::<BigEndian>(self.flags)?;
        writer.write_u32::<BigEndian>(self.id)?;
        writer.write_u16::<BigEndian>(self.chunk_extra_data_size)?;
        writer.write_u16::<BigEndian>(0)?; // pad0
        writer.write_u32::<BigEndian>(0)?; // pad1
        Ok(())
    }
}

/// ECF chunk header.
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

    /// Read a chunk header from a reader.
    pub fn read<R: Read>(reader: &mut R) -> Result<Self> {
        Ok(Self {
            id: reader.read_u64::<BigEndian>()?,
            offset: reader.read_u32::<BigEndian>()?,
            size: reader.read_u32::<BigEndian>()?,
            adler32: reader.read_u32::<BigEndian>()?,
            flags: reader.read_u8()?,
            alignment_log2: reader.read_u8()?,
            resource_flags: reader.read_u16::<BigEndian>()?,
        })
    }

    /// Write a chunk header to a writer.
    pub fn write<W: Write>(&self, writer: &mut W) -> Result<()> {
        writer.write_u64::<BigEndian>(self.id)?;
        writer.write_u32::<BigEndian>(self.offset)?;
        writer.write_u32::<BigEndian>(self.size)?;
        writer.write_u32::<BigEndian>(self.adler32)?;
        writer.write_u8(self.flags)?;
        writer.write_u8(self.alignment_log2)?;
        writer.write_u16::<BigEndian>(self.resource_flags)?;
        Ok(())
    }

    /// Get the alignment in bytes.
    pub fn alignment(&self) -> usize {
        1 << self.alignment_log2
    }

    /// Get the compression method from flags.
    pub fn compression_method(&self) -> CompressionMethod {
        CompressionMethod::from_flags(self.flags)
    }
}
