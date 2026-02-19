//! Buffer pool for reducing allocations during ERA operations
//!
//! This module provides reusable byte buffers to avoid repeated allocations
//! when reading and writing many files in an archive.

use std::cell::RefCell;
use std::collections::VecDeque;

/// Thread-local buffer pool for reusable byte vectors
///
/// Buffers are returned to the pool when dropped via `PooledBuffer`.
/// This significantly reduces allocation overhead when processing many files.
pub struct BufferPool {
    /// Available buffers, sorted by capacity (smallest first)
    buffers: RefCell<VecDeque<Vec<u8>>>,
    /// Maximum number of buffers to keep in the pool
    max_buffers: usize,
}

impl BufferPool {
    /// Create a new buffer pool
    pub fn new(max_buffers: usize) -> Self {
        Self {
            buffers: RefCell::new(VecDeque::with_capacity(max_buffers)),
            max_buffers,
        }
    }

    /// Get a buffer with at least the specified capacity
    ///
    /// The buffer is cleared but may have excess capacity from previous use.
    pub fn get(&self, min_capacity: usize) -> PooledBuffer<'_> {
        let mut buffers = self.buffers.borrow_mut();

        // Find a buffer with sufficient capacity
        let buffer = buffers
            .iter()
            .position(|b| b.capacity() >= min_capacity)
            .map(|i| buffers.remove(i).unwrap())
            .unwrap_or_else(|| Vec::with_capacity(min_capacity));

        PooledBuffer {
            buffer: Some(buffer),
            pool: self,
        }
    }

    /// Get a buffer initialized to a specific size with zeros
    pub fn get_zeroed(&self, size: usize) -> PooledBuffer<'_> {
        let mut pooled = self.get(size);
        pooled.resize(size, 0);
        pooled
    }

    /// Return a buffer to the pool
    fn return_buffer(&self, mut buffer: Vec<u8>) {
        buffer.clear();

        let mut buffers = self.buffers.borrow_mut();
        if buffers.len() < self.max_buffers {
            // Insert sorted by capacity (smallest first)
            let pos = buffers
                .iter()
                .position(|b| b.capacity() > buffer.capacity())
                .unwrap_or(buffers.len());
            buffers.insert(pos, buffer);
        }
        // If pool is full, buffer is dropped
    }

    /// Get the number of buffers currently in the pool
    pub fn available(&self) -> usize {
        self.buffers.borrow().len()
    }
}

impl Default for BufferPool {
    fn default() -> Self {
        Self::new(32)
    }
}

/// A buffer borrowed from a `BufferPool`
///
/// When dropped, the buffer is automatically returned to the pool.
pub struct PooledBuffer<'a> {
    buffer: Option<Vec<u8>>,
    pool: &'a BufferPool,
}

impl<'a> PooledBuffer<'a> {
    /// Take ownership of the buffer, removing it from pool management
    pub fn take(mut self) -> Vec<u8> {
        self.buffer.take().unwrap()
    }
}

impl<'a> std::ops::Deref for PooledBuffer<'a> {
    type Target = Vec<u8>;

    fn deref(&self) -> &Self::Target {
        self.buffer.as_ref().unwrap()
    }
}

impl<'a> std::ops::DerefMut for PooledBuffer<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.buffer.as_mut().unwrap()
    }
}

impl<'a> Drop for PooledBuffer<'a> {
    fn drop(&mut self) {
        if let Some(buffer) = self.buffer.take() {
            self.pool.return_buffer(buffer);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_buffer_pool_reuse() {
        let pool = BufferPool::new(4);

        // Get a buffer and fill it
        {
            let mut buf = pool.get(1024);
            buf.extend_from_slice(&[1, 2, 3, 4]);
            assert!(buf.capacity() >= 1024);
        } // Buffer returned to pool

        assert_eq!(pool.available(), 1);

        // Get another buffer - should reuse
        {
            let buf = pool.get(512);
            assert!(buf.capacity() >= 1024); // Got the same buffer
            assert!(buf.is_empty()); // But it's cleared
        }
    }

    #[test]
    fn test_buffer_pool_take() {
        let pool = BufferPool::new(4);

        let mut buf = pool.get(100);
        buf.extend_from_slice(b"hello");

        let owned = buf.take();
        assert_eq!(&owned, b"hello");
        assert_eq!(pool.available(), 0); // Not returned to pool
    }
}
