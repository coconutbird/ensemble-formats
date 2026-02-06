//! ERA archive format for Halo Wars Definitive Edition
//!
//! ERA files are ECF-based archives containing compressed game assets.
//! Files are encrypted using TEA cipher in CTR mode with 64-byte blocks.
//!
//! # Reading ERA archives
//!
//! ```no_run
//! use era::EraArchive;
//!
//! let mut archive = EraArchive::open("root.era").unwrap();
//! for entry in archive.iter() {
//!     println!("{}", entry.filename.as_deref().unwrap_or("<unnamed>"));
//! }
//! ```
//!
//! # Writing ERA archives
//!
//! ```no_run
//! use era::EraWriter;
//!
//! let mut writer = EraWriter::new();
//! writer.add_file("data/test.txt", b"Hello, world!".to_vec());
//! writer.write_to_file("output.era").unwrap();
//! ```

pub mod crypto;
mod decrypt_reader;
mod encrypt_writer;
mod era;
mod error;
mod writer;

pub use crypto::{TeaKeys, ARCHIVE_PASSWORD, TEA_BLOCK_SIZE};
pub use decrypt_reader::DecryptReader;
pub use encrypt_writer::EncryptWriter;
pub use era::*;
pub use error::*;
pub use writer::EraWriter;

// Re-export ECF types that are used in the public API
pub use ecf::{CompressionMethod, EcfChunkHeader, EcfHeader};

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_era_roundtrip_single_file() {
        let mut writer = EraWriter::new();
        writer.add_file("test/hello.txt", b"Hello, World!".to_vec());

        let mut buffer = Cursor::new(Vec::new());
        let keys = TeaKeys::default_archive_keys();
        let encrypt_writer = EncryptWriter::new(&mut buffer, keys);
        writer.write(encrypt_writer).expect("Failed to write");

        let data = buffer.into_inner();
        let cursor = Cursor::new(data);
        let decrypt_reader = DecryptReader::new(cursor, keys);
        let mut archive = EraArchive::new(decrypt_reader).expect("Failed to read");

        assert_eq!(archive.len(), 2); // filename chunk + 1 file
        let entry = archive.entry(1).unwrap();
        assert_eq!(entry.filename.as_deref(), Some("test\\hello.txt"));

        let content = archive.read_entry(1).expect("Failed to read entry");
        assert_eq!(content, b"Hello, World!");
    }

    #[test]
    fn test_era_roundtrip_multiple_files() {
        let mut writer = EraWriter::new();
        writer.add_file("test/hello.txt", b"Hello, World!".to_vec());
        writer.add_file("data/numbers.bin", vec![1, 2, 3, 4, 5, 6, 7, 8]);
        writer.add_file("empty.txt", vec![]);

        let mut buffer = Cursor::new(Vec::new());
        let keys = TeaKeys::default_archive_keys();
        let encrypt_writer = EncryptWriter::new(&mut buffer, keys);
        writer.write(encrypt_writer).expect("Failed to write");

        let data = buffer.into_inner();
        let cursor = Cursor::new(data);
        let decrypt_reader = DecryptReader::new(cursor, keys);
        let mut archive = EraArchive::new(decrypt_reader).expect("Failed to read");

        assert_eq!(archive.len(), 4); // filename chunk + 3 files

        let content1 = archive.read_entry(1).expect("Failed to read entry 1");
        assert_eq!(content1, b"Hello, World!");

        let content2 = archive.read_entry(2).expect("Failed to read entry 2");
        assert_eq!(content2, vec![1, 2, 3, 4, 5, 6, 7, 8]);

        let content3 = archive.read_entry(3).expect("Failed to read entry 3");
        assert_eq!(content3, vec![]);
    }

    #[test]
    fn test_era_roundtrip_identical_bytes() {
        let mut writer = EraWriter::new();
        writer.add_file("test/hello.txt", b"Hello, World!".to_vec());
        writer.add_file("data/numbers.bin", vec![1, 2, 3, 4, 5, 6, 7, 8]);

        // First write
        let mut buffer1 = Cursor::new(Vec::new());
        let keys = TeaKeys::default_archive_keys();
        let encrypt_writer = EncryptWriter::new(&mut buffer1, keys);
        writer.write(encrypt_writer).expect("Failed to write");
        let data1 = buffer1.into_inner();

        // Read back and create new writer
        let cursor = Cursor::new(data1.clone());
        let decrypt_reader = DecryptReader::new(cursor, keys);
        let archive = EraArchive::new(decrypt_reader).expect("Failed to read");

        let mut writer2 = EraWriter::new();
        for i in 1..archive.len() {
            let entry = archive.entry(i).unwrap();
            let filename = entry.filename.as_ref().unwrap();
            let cursor2 = Cursor::new(data1.clone());
            let decrypt_reader2 = DecryptReader::new(cursor2, keys);
            let mut archive2 = EraArchive::new(decrypt_reader2).expect("Failed to read");
            let content = archive2.read_entry(i).expect("Failed to read entry");
            writer2.add_file(filename, content);
        }

        // Second write
        let mut buffer2 = Cursor::new(Vec::new());
        let encrypt_writer2 = EncryptWriter::new(&mut buffer2, keys);
        writer2.write(encrypt_writer2).expect("Failed to write second time");
        let data2 = buffer2.into_inner();

        assert_eq!(data1, data2, "Round-trip should produce identical bytes");
    }

    #[test]
    fn test_era_large_file() {
        // Test with a file larger than TEA block size (64 bytes)
        let large_data: Vec<u8> = (0..1000).map(|i| (i % 256) as u8).collect();

        let mut writer = EraWriter::new();
        writer.add_file("large.bin", large_data.clone());

        let mut buffer = Cursor::new(Vec::new());
        let keys = TeaKeys::default_archive_keys();
        let encrypt_writer = EncryptWriter::new(&mut buffer, keys);
        writer.write(encrypt_writer).expect("Failed to write");

        let data = buffer.into_inner();
        let cursor = Cursor::new(data);
        let decrypt_reader = DecryptReader::new(cursor, keys);
        let mut archive = EraArchive::new(decrypt_reader).expect("Failed to read");

        let content = archive.read_entry(1).expect("Failed to read entry");
        assert_eq!(content, large_data);
    }

    #[test]
    fn test_tea_encrypt_decrypt_roundtrip() {
        use crate::crypto::{tea_encrypt_block64, tea_decrypt_block64};

        let keys = TeaKeys::default_archive_keys();
        let original: [u8; 64] = std::array::from_fn(|i| i as u8);
        let mut encrypted = [0u8; 64];
        let mut decrypted = [0u8; 64];

        tea_encrypt_block64(&keys, &original, &mut encrypted, 0);
        tea_decrypt_block64(&keys, &encrypted, &mut decrypted, 0);

        assert_eq!(original, decrypted);
    }
}
