//! Encrypting writer wrapper for ERA files.

use ecf::io::{IoError, Read, Seek, SeekFrom, Write, invalid_seek, is_unexpected_eof};

use crate::crypto::{TEA_BLOCK_SIZE, TeaKeys, tea_decrypt_block64, tea_encrypt_block64};

/// A writer that encrypts data using TEA cipher before writing
pub struct EncryptWriter<W> {
    inner: W,
    keys: TeaKeys,
    /// Current position in the plaintext stream
    position: u64,
    /// Buffered plaintext block (waiting to be encrypted)
    buffer: [u8; TEA_BLOCK_SIZE],
    /// Number of valid bytes in buffer
    buffer_len: usize,
    /// File offset of the start of the buffered block
    buffer_offset: u64,
    /// Whether this block has been written to disk before
    block_written: bool,
}

impl<W: Write + Seek + Read> EncryptWriter<W> {
    /// Create a new encrypting writer
    pub fn new(inner: W, keys: TeaKeys) -> Self {
        Self {
            inner,
            keys,
            position: 0,
            buffer: [0; TEA_BLOCK_SIZE],
            buffer_len: 0,
            buffer_offset: 0,
            block_written: false,
        }
    }

    /// Flush the current buffer, encrypting and writing it
    fn flush_buffer(&mut self) -> Result<(), IoError> {
        if self.buffer_len == 0 {
            return Ok(());
        }

        // Pad with zeros if not a full block
        for i in self.buffer_len..TEA_BLOCK_SIZE {
            self.buffer[i] = 0;
        }

        // Encrypt the block
        let counter = (self.buffer_offset / TEA_BLOCK_SIZE as u64) as u32;
        let mut encrypted = [0u8; TEA_BLOCK_SIZE];
        tea_encrypt_block64(&self.keys, &self.buffer, &mut encrypted, counter);

        // Seek to the block position and write
        self.inner.seek(SeekFrom::Start(self.buffer_offset))?;
        self.inner.write_all(&encrypted)?;

        self.buffer_len = 0;
        self.block_written = true;
        Ok(())
    }

    /// Read back and decrypt a previously written block
    fn read_block(&mut self, block_offset: u64) -> Result<bool, IoError> {
        self.inner.seek(SeekFrom::Start(block_offset))?;
        let mut encrypted = [0u8; TEA_BLOCK_SIZE];
        match self.inner.read_exact(&mut encrypted) {
            Ok(()) => {
                let counter = (block_offset / TEA_BLOCK_SIZE as u64) as u32;
                tea_decrypt_block64(&self.keys, &encrypted, &mut self.buffer, counter);
                Ok(true)
            }
            Err(e) if is_unexpected_eof(&e) => {
                // Block doesn't exist yet, initialize to zeros
                self.buffer = [0; TEA_BLOCK_SIZE];
                Ok(false)
            }
            Err(e) => Err(e),
        }
    }

    /// Finish writing and return the inner writer
    pub fn finish(mut self) -> Result<W, IoError> {
        self.flush_buffer()?;
        Ok(self.inner)
    }

    /// Get current position
    pub fn position(&self) -> u64 {
        self.position
    }
}

impl<W: Write + Seek + Read> Write for EncryptWriter<W> {
    fn write(&mut self, buf: &[u8]) -> Result<usize, IoError> {
        if buf.is_empty() {
            return Ok(0);
        }

        let mut total_written = 0;

        while total_written < buf.len() {
            // Calculate block alignment
            let block_offset = (self.position / TEA_BLOCK_SIZE as u64) * TEA_BLOCK_SIZE as u64;
            let offset_in_block = (self.position % TEA_BLOCK_SIZE as u64) as usize;

            // If we're starting a new block, flush the old one
            if self.buffer_len > 0 && block_offset != self.buffer_offset {
                self.flush_buffer()?;
            }

            // Start a new block if needed
            if self.buffer_len == 0 {
                self.buffer_offset = block_offset;

                // If we've written data before and are not at the start of the block,
                // we need to read back the existing block data
                if self.block_written && offset_in_block > 0 {
                    self.read_block(block_offset)?;
                    self.buffer_len = offset_in_block;
                } else {
                    self.buffer = [0; TEA_BLOCK_SIZE];
                }
            }

            // Copy data into buffer
            let bytes_available = TEA_BLOCK_SIZE - offset_in_block;
            let bytes_to_copy = (buf.len() - total_written).min(bytes_available);

            self.buffer[offset_in_block..offset_in_block + bytes_to_copy]
                .copy_from_slice(&buf[total_written..total_written + bytes_to_copy]);

            self.buffer_len = self.buffer_len.max(offset_in_block + bytes_to_copy);
            self.position += bytes_to_copy as u64;
            total_written += bytes_to_copy;

            // Flush if block is complete
            if self.buffer_len == TEA_BLOCK_SIZE {
                self.flush_buffer()?;
            }
        }

        Ok(total_written)
    }

    fn flush(&mut self) -> Result<(), IoError> {
        self.flush_buffer()?;
        self.inner.flush()
    }
}

impl<W: Write + Seek + Read> Seek for EncryptWriter<W> {
    fn seek(&mut self, pos: SeekFrom) -> Result<u64, IoError> {
        // Flush before seeking
        self.flush_buffer()?;

        let new_pos = match pos {
            SeekFrom::Start(offset) => offset,
            SeekFrom::Current(offset) => if offset >= 0 {
                self.position.checked_add(offset as u64)
            } else {
                self.position.checked_sub((-offset) as u64)
            }
            .ok_or_else(invalid_seek)?,
            SeekFrom::End(_) => {
                return Err(invalid_seek());
            }
        };

        self.position = new_pos;
        Ok(new_pos)
    }
}
