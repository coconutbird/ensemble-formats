//! TEA-based encryption/decryption for ERA archives
//!
//! ERA archives are encrypted using a modified TEA cipher in CTR mode with 64-byte blocks.
//! The key is derived from a password using SHA-1.

use sha1::{Digest, Sha1};

/// Default TEA initialization vector
pub const DEFAULT_TEA_IV: u64 = 0x15EF_0AF3_3424_8FE2;

/// TEA block size in bytes (64 bytes = 8 x u64)
pub const TEA_BLOCK_SIZE: usize = 64;

const TEA_BLOCK_SIZE_U64: u64 = 64;

fn split_u64(value: u64) -> (u32, u32) {
    let [low_0, low_1, low_2, low_3, high_0, high_1, high_2, high_3] = value.to_le_bytes();
    (
        u32::from_le_bytes([low_0, low_1, low_2, low_3]),
        u32::from_le_bytes([high_0, high_1, high_2, high_3]),
    )
}

fn starting_counter(start_offset: u64) -> crate::Result<u32> {
    if !start_offset.is_multiple_of(TEA_BLOCK_SIZE_U64) {
        return Err(crate::Error::InvalidChunkData(
            "TEA start offset is not block-aligned".into(),
        ));
    }
    u32::try_from(start_offset / TEA_BLOCK_SIZE_U64)
        .map_err(|_| crate::Error::SizeOverflow("TEA block counter"))
}

fn block_counter(start: u32, block_index: usize) -> crate::Result<u32> {
    let block_index =
        u32::try_from(block_index).map_err(|_| crate::Error::SizeOverflow("TEA block index"))?;
    start
        .checked_add(block_index)
        .ok_or(crate::Error::SizeOverflow("TEA block counter"))
}

/// The archive encryption password
pub const ARCHIVE_PASSWORD: &str = "3zDdptN*rV=qOkRbE*NAuWM6";

/// Three 64-bit keys derived from the password
#[derive(Debug, Clone, Copy)]
pub struct TeaKeys {
    pub k1: u64,
    pub k2: u64,
    pub k3: u64,
}

impl TeaKeys {
    /// Initialize keys from a password phrase
    #[must_use]
    pub fn from_password(password: &str) -> Self {
        // First SHA-1 hash
        let mut hasher = Sha1::new();
        hasher.update(0xa480_0c14_u32.to_be_bytes());
        hasher.update(password.as_bytes());
        hasher.update(0x5AF4_A9F1_u32.to_be_bytes());
        hasher.update(0xCA68_84EC_u32.to_be_bytes());
        let hash1 = hasher.finalize();

        // Second SHA-1 hash
        let mut hasher = Sha1::new();
        hasher.update(0xcb92_eaeb_u32.to_be_bytes());
        hasher.update(hash1);
        hasher.update(0x1d91_9bf8_u32.to_be_bytes());
        let hash2 = hasher.finalize();

        // Extract keys from hashes (big-endian DWORDs)
        let get_dword = |hash: &[u8], index: usize| -> u32 {
            let offset = index * 4;
            u32::from_be_bytes([
                hash[offset],
                hash[offset + 1],
                hash[offset + 2],
                hash[offset + 3],
            ])
        };

        let k1 = u64::from(get_dword(&hash2, 0)) | (u64::from(get_dword(&hash2, 1)) << 32);
        let k2 = u64::from(get_dword(&hash2, 2)) | (u64::from(get_dword(&hash2, 3)) << 32);
        let k3 = u64::from(get_dword(&hash2, 4)) | (u64::from(get_dword(&hash1, 0)) << 32);

        Self { k1, k2, k3 }
    }

    /// Get the default archive keys
    #[must_use]
    pub fn default_archive_keys() -> Self {
        Self::from_password(ARCHIVE_PASSWORD)
    }
}

/// 16-round TEA decipher on 4 parallel 64-bit values
fn tea_decipher_4(v0: u64, v1: u64, v2: u64, v3: u64, k0: u64, k1: u64) -> (u64, u64, u64, u64) {
    let (a, b) = split_u64(k0);
    let (c, d) = split_u64(k1);

    let (mut y0, mut z0) = split_u64(v0);
    let (mut y1, mut z1) = split_u64(v1);
    let (mut y2, mut z2) = split_u64(v2);
    let (mut y3, mut z3) = split_u64(v3);

    macro_rules! tea_round_4 {
        ($sum:expr) => {
            z0 = z0.wrapping_sub(
                (y0 << 4)
                    .wrapping_add(c ^ y0)
                    .wrapping_add($sum ^ (y0 >> 5))
                    .wrapping_add(d),
            );
            z1 = z1.wrapping_sub(
                (y1 << 4)
                    .wrapping_add(c ^ y1)
                    .wrapping_add($sum ^ (y1 >> 5))
                    .wrapping_add(d),
            );
            z2 = z2.wrapping_sub(
                (y2 << 4)
                    .wrapping_add(c ^ y2)
                    .wrapping_add($sum ^ (y2 >> 5))
                    .wrapping_add(d),
            );
            z3 = z3.wrapping_sub(
                (y3 << 4)
                    .wrapping_add(c ^ y3)
                    .wrapping_add($sum ^ (y3 >> 5))
                    .wrapping_add(d),
            );
            y0 = y0.wrapping_sub(
                (z0 << 4)
                    .wrapping_add(a ^ z0)
                    .wrapping_add($sum ^ (z0 >> 5))
                    .wrapping_add(b),
            );
            y1 = y1.wrapping_sub(
                (z1 << 4)
                    .wrapping_add(a ^ z1)
                    .wrapping_add($sum ^ (z1 >> 5))
                    .wrapping_add(b),
            );
            y2 = y2.wrapping_sub(
                (z2 << 4)
                    .wrapping_add(a ^ z2)
                    .wrapping_add($sum ^ (z2 >> 5))
                    .wrapping_add(b),
            );
            y3 = y3.wrapping_sub(
                (z3 << 4)
                    .wrapping_add(a ^ z3)
                    .wrapping_add($sum ^ (z3 >> 5))
                    .wrapping_add(b),
            );
        };
    }

    tea_round_4!(0xE377_9B90_u32);
    tea_round_4!(0x4540_21D7_u32);
    tea_round_4!(0xA708_A81E_u32);
    tea_round_4!(0x08D1_2E65_u32);
    tea_round_4!(0x6A99_B4AC_u32);
    tea_round_4!(0xCC62_3AF3_u32);
    tea_round_4!(0x2E2A_C13A_u32);
    tea_round_4!(0x8FF3_4781_u32);
    tea_round_4!(0xF1BB_CDC8_u32);
    tea_round_4!(0x5384_540F_u32);
    tea_round_4!(0xB54C_DA56_u32);
    tea_round_4!(0x1715_609D_u32);
    tea_round_4!(0x78DD_E6E4_u32);
    tea_round_4!(0xDAA6_6D2B_u32);
    tea_round_4!(0x3C6E_F372_u32);
    tea_round_4!(0x9E37_79B9_u32);

    (
        u64::from(y0) | (u64::from(z0) << 32),
        u64::from(y1) | (u64::from(z1) << 32),
        u64::from(y2) | (u64::from(z2) << 32),
        u64::from(y3) | (u64::from(z3) << 32),
    )
}

/// LFSR3 transformation
#[inline]
fn lfsr3(mut x: u32) -> u32 {
    x ^= x << 17;
    x ^= x >> 13;
    x ^= x << 5;
    x
}

/// Block contract operation - returns new values
#[inline]
fn block_contract(x: u64, y: u64, z: u64, w: u64) -> (u64, u64, u64, u64) {
    let w = w ^ x;
    let z = z ^ w;
    let y = y ^ z;
    let x = x ^ y;
    (x, y, z, w)
}

/// Decrypt a 64-byte block in CTR mode
pub fn tea_decrypt_block64(keys: &TeaKeys, src: &[u8; 64], dst: &mut [u8; 64], counter: u32) {
    let iv = DEFAULT_TEA_IV;

    // Read 8 u64 values from source (big-endian)
    let read_u64 = |offset: usize| -> u64 {
        u64::from_be_bytes([
            src[offset],
            src[offset + 1],
            src[offset + 2],
            src[offset + 3],
            src[offset + 4],
            src[offset + 5],
            src[offset + 6],
            src[offset + 7],
        ])
    };

    let v0 = read_u64(0);
    let v1 = read_u64(8);
    let v2 = read_u64(16);
    let v3 = read_u64(24);
    let mut v4 = read_u64(32);
    let mut v5 = read_u64(40);
    let mut v6 = read_u64(48);
    let mut v7 = read_u64(56);

    // Block contract on v[4..8] (3 times)
    (v4, v5, v6, v7) = block_contract(v4, v5, v6, v7);
    (v4, v5, v6, v7) = block_contract(v4, v5, v6, v7);
    (v4, v5, v6, v7) = block_contract(v4, v5, v6, v7);

    // First decipher: v[0..4] with k1, k2
    let (w0, w1, w2, w3) = tea_decipher_4(v0, v1, v2, v3, keys.k1, keys.k2);

    // Mix
    let w0 = w0.wrapping_sub(v7);
    let w1 = w1 ^ v6;
    let w2 = w2.wrapping_add(v5);
    let w3 = w3 ^ v4;

    // Block contract on v[4..8] (3 more times)
    (v4, v5, v6, v7) = block_contract(v4, v5, v6, v7);
    (v4, v5, v6, v7) = block_contract(v4, v5, v6, v7);
    (v4, v5, v6, v7) = block_contract(v4, v5, v6, v7);

    // Second decipher: v[4..8] with k2, k3
    let (mut w4, mut w5, mut w6, mut w7) = tea_decipher_4(v4, v5, v6, v7, keys.k2, keys.k3);

    // Mix
    w4 ^= w3;
    w5 = w5.wrapping_add(w2);
    w6 ^= w1;
    w7 = w7.wrapping_sub(w0);

    // Block contract on w[0..4] (3 times)
    let (mut w0, mut w1, mut w2, mut w3) = (w0, w1, w2, w3);
    (w0, w1, w2, w3) = block_contract(w0, w1, w2, w3);
    (w0, w1, w2, w3) = block_contract(w0, w1, w2, w3);
    (w0, w1, w2, w3) = block_contract(w0, w1, w2, w3);

    // Third decipher: w[0..4] with k2, k1
    let (mut out0, mut out1, mut out2, mut out3) = tea_decipher_4(w0, w1, w2, w3, keys.k2, keys.k1);

    // Apply counter-based XOR
    let mut ctr = counter.wrapping_add(split_u64(iv >> 10).0);
    if ctr == 0 {
        ctr = 1;
    }

    ctr = lfsr3(ctr);
    out0 ^= u64::from(ctr).wrapping_add(iv);
    ctr = lfsr3(ctr);
    out1 ^= u64::from(ctr).wrapping_sub(iv);
    ctr = lfsr3(ctr);
    out2 ^= u64::from(ctr).wrapping_add(iv);
    ctr = lfsr3(ctr);
    out3 ^= u64::from(ctr).wrapping_sub(iv);
    ctr = lfsr3(ctr);
    w4 ^= u64::from(ctr).wrapping_add(iv);
    ctr = lfsr3(ctr);
    w5 ^= u64::from(ctr).wrapping_sub(iv);
    ctr = lfsr3(ctr);
    w6 ^= u64::from(ctr).wrapping_add(iv);
    ctr = lfsr3(ctr);
    w7 ^= u64::from(ctr).wrapping_sub(iv);

    // Write output (big-endian)
    let outputs = [out0, out1, out2, out3, w4, w5, w6, w7];
    for (i, &val) in outputs.iter().enumerate() {
        let bytes = val.to_be_bytes();
        let offset = i * 8;
        dst[offset..offset + 8].copy_from_slice(&bytes);
    }
}

/// Decrypt data in-place.
///
/// # Errors
///
/// Returns an error if `data` or `start_offset` is not block-aligned, or if
/// the block counter exceeds the cipher's `u32` counter space.
pub fn tea_decrypt_data(keys: &TeaKeys, data: &mut [u8], start_offset: u64) -> crate::Result<()> {
    if !data.len().is_multiple_of(TEA_BLOCK_SIZE) {
        return Err(crate::Error::InvalidChunkData(
            "TEA data length is not block-aligned".into(),
        ));
    }
    let start_counter = starting_counter(start_offset)?;

    for (block_index, chunk) in data
        .as_chunks_mut::<TEA_BLOCK_SIZE>()
        .0
        .iter_mut()
        .enumerate()
    {
        let counter = block_counter(start_counter, block_index)?;
        let src = *chunk;
        let mut dst = [0u8; 64];
        tea_decrypt_block64(keys, &src, &mut dst, counter);
        chunk.copy_from_slice(&dst);
    }
    Ok(())
}

/// 16-round TEA encipher on 4 parallel 64-bit values (inverse of `tea_decipher_4`)
fn tea_encipher_4(v0: u64, v1: u64, v2: u64, v3: u64, k0: u64, k1: u64) -> (u64, u64, u64, u64) {
    let (a, b) = split_u64(k0);
    let (c, d) = split_u64(k1);

    let (mut y0, mut z0) = split_u64(v0);
    let (mut y1, mut z1) = split_u64(v1);
    let (mut y2, mut z2) = split_u64(v2);
    let (mut y3, mut z3) = split_u64(v3);

    macro_rules! tea_round_4_enc {
        ($sum:expr) => {
            y0 = y0.wrapping_add(
                (z0 << 4)
                    .wrapping_add(a ^ z0)
                    .wrapping_add($sum ^ (z0 >> 5))
                    .wrapping_add(b),
            );
            y1 = y1.wrapping_add(
                (z1 << 4)
                    .wrapping_add(a ^ z1)
                    .wrapping_add($sum ^ (z1 >> 5))
                    .wrapping_add(b),
            );
            y2 = y2.wrapping_add(
                (z2 << 4)
                    .wrapping_add(a ^ z2)
                    .wrapping_add($sum ^ (z2 >> 5))
                    .wrapping_add(b),
            );
            y3 = y3.wrapping_add(
                (z3 << 4)
                    .wrapping_add(a ^ z3)
                    .wrapping_add($sum ^ (z3 >> 5))
                    .wrapping_add(b),
            );
            z0 = z0.wrapping_add(
                (y0 << 4)
                    .wrapping_add(c ^ y0)
                    .wrapping_add($sum ^ (y0 >> 5))
                    .wrapping_add(d),
            );
            z1 = z1.wrapping_add(
                (y1 << 4)
                    .wrapping_add(c ^ y1)
                    .wrapping_add($sum ^ (y1 >> 5))
                    .wrapping_add(d),
            );
            z2 = z2.wrapping_add(
                (y2 << 4)
                    .wrapping_add(c ^ y2)
                    .wrapping_add($sum ^ (y2 >> 5))
                    .wrapping_add(d),
            );
            z3 = z3.wrapping_add(
                (y3 << 4)
                    .wrapping_add(c ^ y3)
                    .wrapping_add($sum ^ (y3 >> 5))
                    .wrapping_add(d),
            );
        };
    }

    // Encryption uses sums in reverse order
    tea_round_4_enc!(0x9E37_79B9_u32);
    tea_round_4_enc!(0x3C6E_F372_u32);
    tea_round_4_enc!(0xDAA6_6D2B_u32);
    tea_round_4_enc!(0x78DD_E6E4_u32);
    tea_round_4_enc!(0x1715_609D_u32);
    tea_round_4_enc!(0xB54C_DA56_u32);
    tea_round_4_enc!(0x5384_540F_u32);
    tea_round_4_enc!(0xF1BB_CDC8_u32);
    tea_round_4_enc!(0x8FF3_4781_u32);
    tea_round_4_enc!(0x2E2A_C13A_u32);
    tea_round_4_enc!(0xCC62_3AF3_u32);
    tea_round_4_enc!(0x6A99_B4AC_u32);
    tea_round_4_enc!(0x08D1_2E65_u32);
    tea_round_4_enc!(0xA708_A81E_u32);
    tea_round_4_enc!(0x4540_21D7_u32);
    tea_round_4_enc!(0xE377_9B90_u32);

    (
        u64::from(y0) | (u64::from(z0) << 32),
        u64::from(y1) | (u64::from(z1) << 32),
        u64::from(y2) | (u64::from(z2) << 32),
        u64::from(y3) | (u64::from(z3) << 32),
    )
}

/// Block expand operation - inverse of `block_contract`
#[inline]
fn block_expand(x: u64, y: u64, z: u64, w: u64) -> (u64, u64, u64, u64) {
    let x = x ^ y;
    let y = y ^ z;
    let z = z ^ w;
    let w = w ^ x;
    (x, y, z, w)
}

/// Encrypt a 64-byte block in CTR mode (inverse of `tea_decrypt_block64`)
pub fn tea_encrypt_block64(keys: &TeaKeys, src: &[u8; 64], dst: &mut [u8; 64], counter: u32) {
    let iv = DEFAULT_TEA_IV;

    // Read 8 u64 values from source (big-endian)
    let read_u64 = |offset: usize| -> u64 {
        u64::from_be_bytes([
            src[offset],
            src[offset + 1],
            src[offset + 2],
            src[offset + 3],
            src[offset + 4],
            src[offset + 5],
            src[offset + 6],
            src[offset + 7],
        ])
    };

    let mut in0 = read_u64(0);
    let mut in1 = read_u64(8);
    let mut in2 = read_u64(16);
    let mut in3 = read_u64(24);
    let mut in4 = read_u64(32);
    let mut in5 = read_u64(40);
    let mut in6 = read_u64(48);
    let mut in7 = read_u64(56);

    // Apply counter-based XOR (same as decrypt - XOR is its own inverse)
    let mut ctr = counter.wrapping_add(split_u64(iv >> 10).0);
    if ctr == 0 {
        ctr = 1;
    }

    ctr = lfsr3(ctr);
    in0 ^= u64::from(ctr).wrapping_add(iv);
    ctr = lfsr3(ctr);
    in1 ^= u64::from(ctr).wrapping_sub(iv);
    ctr = lfsr3(ctr);
    in2 ^= u64::from(ctr).wrapping_add(iv);
    ctr = lfsr3(ctr);
    in3 ^= u64::from(ctr).wrapping_sub(iv);
    ctr = lfsr3(ctr);
    in4 ^= u64::from(ctr).wrapping_add(iv);
    ctr = lfsr3(ctr);
    in5 ^= u64::from(ctr).wrapping_sub(iv);
    ctr = lfsr3(ctr);
    in6 ^= u64::from(ctr).wrapping_add(iv);
    ctr = lfsr3(ctr);
    in7 ^= u64::from(ctr).wrapping_sub(iv);

    // Third encipher (inverse of third decipher): in[0..4] with k2, k1
    let (w0, w1, w2, w3) = tea_encipher_4(in0, in1, in2, in3, keys.k2, keys.k1);

    // Block expand on w[0..4] (3 times) - inverse of block_contract
    let (mut w0, mut w1, mut w2, mut w3) = (w0, w1, w2, w3);
    (w0, w1, w2, w3) = block_expand(w0, w1, w2, w3);
    (w0, w1, w2, w3) = block_expand(w0, w1, w2, w3);
    (w0, w1, w2, w3) = block_expand(w0, w1, w2, w3);

    // Inverse mix for w[4..8]
    in7 = in7.wrapping_add(w0);
    in6 ^= w1;
    in5 = in5.wrapping_sub(w2);
    in4 ^= w3;

    // Second encipher: in[4..8] with k2, k3
    let (mut v4, mut v5, mut v6, mut v7) = tea_encipher_4(in4, in5, in6, in7, keys.k2, keys.k3);

    // Block expand on v[4..8] (3 times)
    (v4, v5, v6, v7) = block_expand(v4, v5, v6, v7);
    (v4, v5, v6, v7) = block_expand(v4, v5, v6, v7);
    (v4, v5, v6, v7) = block_expand(v4, v5, v6, v7);

    // Inverse mix
    let w0 = w0.wrapping_add(v7);
    let w1 = w1 ^ v6;
    let w2 = w2.wrapping_sub(v5);
    let w3 = w3 ^ v4;

    // Block expand on v[4..8] (3 more times)
    (v4, v5, v6, v7) = block_expand(v4, v5, v6, v7);
    (v4, v5, v6, v7) = block_expand(v4, v5, v6, v7);
    (v4, v5, v6, v7) = block_expand(v4, v5, v6, v7);

    // First encipher: w[0..4] with k1, k2
    let (out0, out1, out2, out3) = tea_encipher_4(w0, w1, w2, w3, keys.k1, keys.k2);

    // Write output (big-endian)
    let outputs = [out0, out1, out2, out3, v4, v5, v6, v7];
    for (i, &val) in outputs.iter().enumerate() {
        let bytes = val.to_be_bytes();
        let offset = i * 8;
        dst[offset..offset + 8].copy_from_slice(&bytes);
    }
}

/// Encrypt data in-place.
///
/// # Errors
///
/// Returns an error if `data` or `start_offset` is not block-aligned, or if
/// the block counter exceeds the cipher's `u32` counter space.
pub fn tea_encrypt_data(keys: &TeaKeys, data: &mut [u8], start_offset: u64) -> crate::Result<()> {
    if !data.len().is_multiple_of(TEA_BLOCK_SIZE) {
        return Err(crate::Error::InvalidChunkData(
            "TEA data length is not block-aligned".into(),
        ));
    }
    let start_counter = starting_counter(start_offset)?;

    for (block_index, chunk) in data
        .as_chunks_mut::<TEA_BLOCK_SIZE>()
        .0
        .iter_mut()
        .enumerate()
    {
        let counter = block_counter(start_counter, block_index)?;
        let src = *chunk;
        let mut dst = [0u8; 64];
        tea_encrypt_block64(keys, &src, &mut dst, counter);
        chunk.copy_from_slice(&dst);
    }
    Ok(())
}

/// Decrypt data in-place using parallel processing (for large buffers).
///
/// This is faster than `tea_decrypt_data` for large amounts of data by
/// utilizing multiple CPU cores. Each 64-byte block is independent in CTR mode.
///
/// # Errors
///
/// Returns an error if `data` or `start_offset` is not block-aligned, or if
/// the block counter exceeds the cipher's `u32` counter space.
#[cfg(feature = "rayon")]
pub fn tea_decrypt_data_parallel(
    keys: &TeaKeys,
    data: &mut [u8],
    start_offset: u64,
) -> crate::Result<()> {
    use rayon::prelude::*;

    if !data.len().is_multiple_of(TEA_BLOCK_SIZE) {
        return Err(crate::Error::InvalidChunkData(
            "TEA data length is not block-aligned".into(),
        ));
    }
    let start_counter = starting_counter(start_offset)?;

    data.par_chunks_mut(TEA_BLOCK_SIZE)
        .enumerate()
        .try_for_each(|(block_index, chunk)| -> crate::Result<()> {
            let counter = block_counter(start_counter, block_index)?;
            let mut src = [0u8; 64];
            src.copy_from_slice(chunk);
            let mut dst = [0u8; 64];
            tea_decrypt_block64(keys, &src, &mut dst, counter);
            chunk.copy_from_slice(&dst);
            Ok(())
        })
}

/// Encrypt data in-place using parallel processing (for large buffers).
///
/// This is faster than `tea_encrypt_data` for large amounts of data by
/// utilizing multiple CPU cores. Each 64-byte block is independent in CTR mode.
///
/// # Errors
///
/// Returns an error if `data` or `start_offset` is not block-aligned, or if
/// the block counter exceeds the cipher's `u32` counter space.
#[cfg(feature = "rayon")]
pub fn tea_encrypt_data_parallel(
    keys: &TeaKeys,
    data: &mut [u8],
    start_offset: u64,
) -> crate::Result<()> {
    use rayon::prelude::*;

    if !data.len().is_multiple_of(TEA_BLOCK_SIZE) {
        return Err(crate::Error::InvalidChunkData(
            "TEA data length is not block-aligned".into(),
        ));
    }
    let start_counter = starting_counter(start_offset)?;

    data.par_chunks_mut(TEA_BLOCK_SIZE)
        .enumerate()
        .try_for_each(|(block_index, chunk)| -> crate::Result<()> {
            let counter = block_counter(start_counter, block_index)?;
            let mut src = [0u8; 64];
            src.copy_from_slice(chunk);
            let mut dst = [0u8; 64];
            tea_encrypt_block64(keys, &src, &mut dst, counter);
            chunk.copy_from_slice(&dst);
            Ok(())
        })
}
