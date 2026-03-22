//! Decrypting reader wrapper for encrypted ERA files.

extern crate std;

use std::io::{Read, Seek, SeekFrom};

use crate::crypto::{TEA_BLOCK_SIZE, TeaKeys, tea_decrypt_block64};

/// A reader that decrypts TEA-encrypted data on the fly
pub struct DecryptReader<R> {
    inner: R,
    keys: TeaKeys,
    /// Current position in the decrypted stream
    position: u64,
    /// Buffered decrypted block
    buffer: [u8; TEA_BLOCK_SIZE],
    /// File offset of the start of the buffered block (aligned to TEA_BLOCK_SIZE)
    buffer_offset: u64,
    /// Whether the buffer is valid
    buffer_valid: bool,
}

impl<R: Read + Seek> DecryptReader<R> {
    /// Create a new decrypting reader
    pub fn new(inner: R, keys: TeaKeys) -> Self {
        Self {
            inner,
            keys,
            position: 0,
            buffer: [0; TEA_BLOCK_SIZE],
            buffer_offset: u64::MAX,
            buffer_valid: false,
        }
    }

    /// Read and decrypt a block at the given aligned offset
    fn read_block(&mut self, block_offset: u64) -> std::io::Result<()> {
        if self.buffer_valid && self.buffer_offset == block_offset {
            return Ok(());
        }

        self.inner.seek(SeekFrom::Start(block_offset))?;

        let mut encrypted = [0u8; TEA_BLOCK_SIZE];
        self.inner.read_exact(&mut encrypted)?;

        let counter = (block_offset / TEA_BLOCK_SIZE as u64) as u32;
        tea_decrypt_block64(&self.keys, &encrypted, &mut self.buffer, counter);

        self.buffer_offset = block_offset;
        self.buffer_valid = true;

        Ok(())
    }

    /// Get the underlying reader
    pub fn into_inner(self) -> R {
        self.inner
    }
}

impl<R: Read + Seek> Read for DecryptReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }

        let mut total_read = 0;

        while total_read < buf.len() {
            // Calculate which block we need
            let block_offset = (self.position / TEA_BLOCK_SIZE as u64) * TEA_BLOCK_SIZE as u64;
            let offset_in_block = (self.position % TEA_BLOCK_SIZE as u64) as usize;

            // Read and decrypt the block
            match self.read_block(block_offset) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    break;
                }
                Err(e) => return Err(e),
            }

            // Copy from buffer to output
            let bytes_available = TEA_BLOCK_SIZE - offset_in_block;
            let bytes_to_copy = (buf.len() - total_read).min(bytes_available);

            buf[total_read..total_read + bytes_to_copy]
                .copy_from_slice(&self.buffer[offset_in_block..offset_in_block + bytes_to_copy]);

            self.position += bytes_to_copy as u64;
            total_read += bytes_to_copy;
        }

        Ok(total_read)
    }
}

impl<R: Read + Seek> Seek for DecryptReader<R> {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        let new_pos = match pos {
            SeekFrom::Start(offset) => offset,
            SeekFrom::Current(offset) => if offset >= 0 {
                self.position.checked_add(offset as u64)
            } else {
                self.position.checked_sub((-offset) as u64)
            }
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "seek out of bounds")
            })?,
            SeekFrom::End(offset) => {
                // Get file size
                let end = self.inner.seek(SeekFrom::End(0))?;
                if offset >= 0 {
                    end.checked_add(offset as u64)
                } else {
                    end.checked_sub((-offset) as u64)
                }
                .ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "seek out of bounds")
                })?
            }
        };

        self.position = new_pos;
        Ok(new_pos)
    }
}
