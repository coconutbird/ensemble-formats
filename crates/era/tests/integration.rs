use era::crypto::tea::{tea_decrypt_block64, tea_encrypt_block64};
use era::{Error, Reader, TeaKeys, Writer, compress_file_data};

#[test]
fn roundtrip_single_file() {
    let mut writer = Writer::new();
    writer.add_file("test/hello.txt", b"Hello, World!".to_vec());

    let data = writer.finalize().expect("Failed to write");
    let mut archive = Reader::from_bytes(&data).expect("Failed to read");

    assert_eq!(archive.len(), 2); // filename chunk + 1 file
    let entry = archive.entry(1).unwrap();
    assert_eq!(entry.filename.as_deref(), Some("test\\hello.txt"));

    let content = archive.read_entry(1).expect("Failed to read entry");
    assert_eq!(content, b"Hello, World!");
}

#[test]
fn roundtrip_multiple_files() {
    let mut writer = Writer::new();
    writer.add_file("test/hello.txt", b"Hello, World!".to_vec());
    writer.add_file("data/numbers.bin", vec![1, 2, 3, 4, 5, 6, 7, 8]);
    writer.add_file("empty.txt", vec![]);

    let data = writer.finalize().expect("Failed to write");
    let mut archive = Reader::from_bytes(&data).expect("Failed to read");

    assert_eq!(archive.len(), 4); // filename chunk + 3 files

    let content1 = archive.read_entry(1).expect("Failed to read entry 1");
    assert_eq!(content1, b"Hello, World!");

    let content2 = archive.read_entry(2).expect("Failed to read entry 2");
    assert_eq!(content2, vec![1, 2, 3, 4, 5, 6, 7, 8]);

    let content3 = archive.read_entry(3).expect("Failed to read entry 3");
    assert_eq!(content3, vec![]);
}

#[test]
fn roundtrip_identical_bytes() {
    let mut writer = Writer::new();
    writer.add_file("test/hello.txt", b"Hello, World!".to_vec());
    writer.add_file("data/numbers.bin", vec![1, 2, 3, 4, 5, 6, 7, 8]);

    let data1 = writer.finalize().expect("Failed to write");

    let mut archive = Reader::from_bytes(&data1).expect("Failed to read");
    let hashes1: Vec<_> = archive.iter().map(|e| e.extra.comp_tiger128).collect();

    let mut writer2 = Writer::new();
    for i in 1..archive.len() {
        let entry = archive.entry(i).unwrap();
        let filename = entry.filename.as_ref().unwrap().clone();
        let content = archive.read_entry(i).expect("Failed to read entry");
        writer2.add_file(filename, content);
    }

    let data2 = writer2.finalize().expect("Failed to write second time");

    let archive2 = Reader::from_bytes(&data2).expect("Failed to read second");
    let hashes2: Vec<_> = archive2.iter().map(|e| e.extra.comp_tiger128).collect();

    assert_eq!(data1, data2, "Round-trip should produce identical bytes");
    assert_eq!(
        hashes1, hashes2,
        "Tiger128 hashes should match after roundtrip"
    );
}

#[test]
fn large_file() {
    let large_data: Vec<u8> = (0..1000)
        .map(|i| u8::try_from(i % 256).expect("value is reduced modulo 256"))
        .collect();

    let mut writer = Writer::new();
    writer.add_file("large.bin", large_data.clone());

    let data = writer.finalize().expect("Failed to write");
    let mut archive = Reader::from_bytes(&data).expect("Failed to read");

    let content = archive.read_entry(1).expect("Failed to read entry");
    assert_eq!(content, large_data);
}

#[test]
fn tea_encrypt_decrypt_roundtrip() {
    let keys = TeaKeys::default_archive_keys();
    let original: [u8; 64] =
        core::array::from_fn(|i| u8::try_from(i).expect("array index is below 64"));
    let mut encrypted = [0u8; 64];
    let mut decrypted = [0u8; 64];

    tea_encrypt_block64(&keys, &original, &mut encrypted, 0);
    tea_decrypt_block64(&keys, &encrypted, &mut decrypted, 0);

    assert_eq!(original, decrypted);
}

#[cfg(feature = "rayon")]
#[test]
fn parallel_encryption_roundtrip() {
    use era::crypto::tea::{
        tea_decrypt_data_parallel, tea_encrypt_data, tea_encrypt_data_parallel,
    };

    let keys = TeaKeys::default_archive_keys();
    let original: Vec<u8> = (0..640)
        .map(|i| u8::try_from(i % 256).expect("value is reduced modulo 256"))
        .collect();

    let mut data1 = original.clone();
    tea_encrypt_data_parallel(&keys, &mut data1, 0).expect("parallel encryption failed");
    tea_decrypt_data_parallel(&keys, &mut data1, 0).expect("parallel decryption failed");
    assert_eq!(original, data1);

    let mut data2 = original.clone();
    tea_encrypt_data(&keys, &mut data2, 0).expect("encryption failed");
    tea_decrypt_data_parallel(&keys, &mut data2, 0).expect("parallel decryption failed");
    assert_eq!(original, data2);
}

#[test]
fn precompressed_file() {
    let mut writer = Writer::new();
    writer.add_file("regular.txt", b"Regular file".to_vec());

    let compressed = compress_file_data(b"Pre-compressed data").expect("compression failed");
    writer.add_compressed_file(
        "precomp.txt",
        compressed.data.clone(),
        compressed.decompressed_size,
        compressed.tiger128,
    );

    let data = writer.finalize().expect("Failed to write");
    let mut archive = Reader::from_bytes(&data).expect("Failed to read");

    assert_eq!(archive.len(), 3);

    let content1 = archive.read_entry(1).expect("Failed to read entry 1");
    assert_eq!(content1, b"Regular file");

    let content2 = archive.read_entry(2).expect("Failed to read entry 2");
    assert_eq!(content2, b"Pre-compressed data");
}

#[test]
fn read_entry_compressed() {
    let mut writer = Writer::new();
    writer.add_file("test.txt", b"Test content for compression".to_vec());

    let data = writer.finalize().expect("Failed to write");
    let mut archive = Reader::from_bytes(&data).expect("Failed to read");

    let (compressed, decomp_size, tiger128) =
        archive.read_entry_compressed(1).expect("Failed to read");

    assert_eq!(decomp_size, 28);
    assert!(!compressed.is_empty());
    assert_ne!(tiger128, [0u8; 16]);

    let decompressed = archive.read_entry(1).expect("Failed to read");
    assert_eq!(decompressed, b"Test content for compression");
}

#[test]
fn write_with_progress() {
    let mut writer = Writer::new();
    writer.add_file("test/file1.txt", b"Hello, World!".to_vec());
    writer.add_file("test/file2.txt", b"Second file content".to_vec());
    writer.add_file("test/file3.txt", b"Third file with more data here".to_vec());

    let mut progress_calls = Vec::new();
    let data = writer
        .finalize_with_progress(&mut |written, total| {
            progress_calls.push((written, total));
            true
        })
        .expect("Failed to write");

    assert_eq!(progress_calls.len(), 4);

    for i in 1..progress_calls.len() {
        assert!(
            progress_calls[i].0 > progress_calls[i - 1].0,
            "Progress should increase"
        );
    }

    let (last_written, last_total) = progress_calls.last().unwrap();
    assert_eq!(last_written, last_total);

    let mut archive = Reader::from_bytes(&data).expect("Failed to read");
    assert_eq!(archive.len(), 4);
    let content = archive.read_entry(1).expect("Failed to read entry");
    assert_eq!(content, b"Hello, World!");
}

#[test]
fn write_with_progress_cancellation() {
    let mut writer = Writer::new();
    writer.add_file("test/file1.txt", b"Hello, World!".to_vec());
    writer.add_file("test/file2.txt", b"Second file content".to_vec());

    let mut call_count = 0;
    let result = writer.finalize_with_progress(&mut |_written, _total| {
        call_count += 1;
        call_count < 2
    });

    assert!(matches!(result, Err(Error::Cancelled)));
}

#[test]
fn write_to_matches_finalize() {
    let mut writer = Writer::new();
    writer.add_file("test/hello.txt", b"Hello, World!".to_vec());
    writer.add_file("data/numbers.bin", vec![1, 2, 3, 4, 5, 6, 7, 8]);

    let finalized = writer.finalize().expect("finalize failed");

    let mut streamed = Vec::new();
    writer.write_to(&mut streamed).expect("write_to failed");

    assert_eq!(
        finalized, streamed,
        "write_to must produce identical bytes to finalize"
    );

    let mut archive = Reader::from_bytes(&streamed).expect("Failed to read streamed");
    assert_eq!(archive.len(), 3);
    assert_eq!(archive.read_entry(1).unwrap(), b"Hello, World!");
    assert_eq!(archive.read_entry(2).unwrap(), vec![1, 2, 3, 4, 5, 6, 7, 8]);
}

#[test]
fn write_to_with_precompressed() {
    let mut writer = Writer::new();
    writer.add_file("regular.txt", b"Regular file".to_vec());

    let compressed = compress_file_data(b"Pre-compressed data").expect("compression failed");
    writer.add_compressed_file(
        "precomp.txt",
        compressed.data.clone(),
        compressed.decompressed_size,
        compressed.tiger128,
    );

    let finalized = writer.finalize().expect("finalize failed");
    let mut streamed = Vec::new();
    writer.write_to(&mut streamed).expect("write_to failed");

    assert_eq!(finalized, streamed);
}

#[test]
fn merkle_sign_verify_roundtrip() {
    use era::crypto::merkle::{PrivateKey, sign, verify};
    use sha1::{Digest, Sha1};

    let depth: u8 = 10;
    let num_leaves = 1u32 << (depth - 1); // 512

    // Generate deterministic "random" secrets from a seed
    let mut secrets = Vec::with_capacity(num_leaves as usize);
    for i in 0..num_leaves {
        let mut hasher = Sha1::new();
        hasher.update(b"test-secret-seed");
        hasher.update(i.to_le_bytes());
        let result = hasher.finalize();
        let mut secret = [0u8; 20];
        secret.copy_from_slice(&result);
        secrets.push(secret);
    }

    let private_key = PrivateKey::from_secrets(depth, secrets);

    // Create a fake header hash
    let mut hasher = Sha1::new();
    hasher.update(b"test-header-data");
    let result = hasher.finalize();
    let mut header_hash = [0u8; 20];
    header_hash.copy_from_slice(&result);

    // Derive public key for this header hash
    let public_key = private_key.public_key(&header_hash);

    // Sign
    let signature = sign(&private_key, &header_hash).expect("signing failed");

    // Verify
    let valid = verify(&public_key, &header_hash, &signature).expect("verify failed");
    assert!(valid, "signature should verify against derived public key");
}

#[test]
fn merkle_sign_verify_different_depths() {
    use era::crypto::merkle::{PrivateKey, sign, verify};
    use sha1::{Digest, Sha1};

    for depth in [2u8, 5, 8, 10, 12] {
        let num_leaves = 1u32 << (depth - 1);

        let mut secrets = Vec::with_capacity(num_leaves as usize);
        for i in 0..num_leaves {
            let mut hasher = Sha1::new();
            hasher.update(b"depth-test-seed");
            hasher.update(depth.to_le_bytes());
            hasher.update(i.to_le_bytes());
            let result = hasher.finalize();
            let mut secret = [0u8; 20];
            secret.copy_from_slice(&result);
            secrets.push(secret);
        }

        let private_key = PrivateKey::from_secrets(depth, secrets);

        let mut hasher = Sha1::new();
        hasher.update(b"header-for-depth-test");
        hasher.update(depth.to_le_bytes());
        let result = hasher.finalize();
        let mut header_hash = [0u8; 20];
        header_hash.copy_from_slice(&result);

        let public_key = private_key.public_key(&header_hash);
        let signature = sign(&private_key, &header_hash).expect("signing failed");
        let valid = verify(&public_key, &header_hash, &signature).expect("verify failed");
        assert!(valid, "depth {depth} should verify");
    }
}

#[test]
fn merkle_wrong_key_rejects() {
    use era::crypto::merkle::{PrivateKey, sign, verify};
    use sha1::{Digest, Sha1};

    let depth: u8 = 8;
    let num_leaves = 1u32 << (depth - 1);

    let mut secrets = Vec::with_capacity(num_leaves as usize);
    for i in 0..num_leaves {
        let mut hasher = Sha1::new();
        hasher.update(b"reject-test-seed");
        hasher.update(i.to_le_bytes());
        let result = hasher.finalize();
        let mut secret = [0u8; 20];
        secret.copy_from_slice(&result);
        secrets.push(secret);
    }

    let private_key = PrivateKey::from_secrets(depth, secrets);

    let mut hasher = Sha1::new();
    hasher.update(b"reject-header");
    let result = hasher.finalize();
    let mut header_hash = [0u8; 20];
    header_hash.copy_from_slice(&result);

    let signature = sign(&private_key, &header_hash).expect("signing failed");

    // Use a wrong public key
    let wrong_key = [0xFFu8; 20];
    let valid = verify(&wrong_key, &header_hash, &signature).expect("verify failed");
    assert!(!valid, "wrong public key should reject");
}

#[test]
fn signed_archive_roundtrip() {
    use era::crypto::merkle::PrivateKey;
    use sha1::{Digest, Sha1};

    let depth: u8 = 10;
    let num_leaves = 1u32 << (depth - 1);

    // Generate deterministic secrets
    let mut secrets = Vec::with_capacity(num_leaves as usize);
    for i in 0..num_leaves {
        let mut hasher = Sha1::new();
        hasher.update(b"signed-archive-test");
        hasher.update(i.to_le_bytes());
        let result = hasher.finalize();
        let mut secret = [0u8; 20];
        secret.copy_from_slice(&result);
        secrets.push(secret);
    }

    let private_key = PrivateKey::from_secrets(depth, secrets);

    // Build a signed archive
    let mut writer = era::Writer::new();
    writer.set_signing_key(private_key.clone());
    writer.add_file("test\\hello.txt", b"Hello, signed world!".to_vec());
    writer.add_file("test\\data.bin", vec![0xAB; 1024]);

    let archive_bytes = writer.finalize().expect("finalize failed");

    // Read it back
    let cursor = std::io::Cursor::new(&archive_bytes);
    let mut reader = era::Reader::new(cursor).expect("reader failed");

    // Verify it has a signature
    assert!(reader.has_signature(), "archive should have a signature");

    // Compute public key from the header hash in the written archive
    let header_hash = reader.header_hash();
    let public_key = private_key.public_key(&header_hash);

    // Verify signature
    let valid = reader
        .verify_signature_with_key(&public_key)
        .expect("verify failed");
    assert!(valid, "signature should verify against derived public key");

    // Verify data integrity
    assert_eq!(reader.len(), 3); // filename table + 2 files
    let data0 = reader.read_entry(1).expect("read entry 1");
    assert_eq!(data0, b"Hello, signed world!");
    let data1 = reader.read_entry(2).expect("read entry 2");
    assert_eq!(data1, vec![0xAB; 1024]);
}

#[test]
fn signed_archive_write_to_roundtrip() {
    use era::crypto::merkle::PrivateKey;
    use sha1::{Digest, Sha1};

    let depth: u8 = 8;
    let num_leaves = 1u32 << (depth - 1);

    let mut secrets = Vec::with_capacity(num_leaves as usize);
    for i in 0..num_leaves {
        let mut hasher = Sha1::new();
        hasher.update(b"write-to-signed-test");
        hasher.update(i.to_le_bytes());
        let result = hasher.finalize();
        let mut secret = [0u8; 20];
        secret.copy_from_slice(&result);
        secrets.push(secret);
    }

    let private_key = PrivateKey::from_secrets(depth, secrets);

    let mut writer = era::Writer::new();
    writer.set_signing_key(private_key.clone());
    writer.add_file("data\\file.bin", vec![42u8; 512]);

    // Use write_to (streaming) path
    let mut buf = Vec::new();
    let cursor = std::io::Cursor::new(&mut buf);
    writer.write_to(cursor).expect("write_to failed");

    // Also get finalize output and compare
    let finalized = writer.finalize().expect("finalize failed");
    assert_eq!(
        buf, finalized,
        "write_to and finalize should produce identical output"
    );

    // Verify signature
    let reader = era::Reader::new(std::io::Cursor::new(&buf)).expect("reader failed");
    assert!(reader.has_signature());
    let header_hash = reader.header_hash();
    let public_key = private_key.public_key(&header_hash);
    assert!(
        reader
            .verify_signature_with_key(&public_key)
            .expect("verify failed")
    );
}
