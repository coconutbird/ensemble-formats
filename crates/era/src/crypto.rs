//! TEA-based encryption/decryption for ERA archives
//!
//! ERA archives are encrypted using a modified TEA cipher in CTR mode with 64-byte blocks.
//! The key is derived from a password using SHA-1.

use sha1::{Digest, Sha1};

/// Default TEA initialization vector
pub const DEFAULT_TEA_IV: u64 = 0x15EF0AF334248FE2;

/// TEA block size in bytes (64 bytes = 8 x u64)
pub const TEA_BLOCK_SIZE: usize = 64;

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
    pub fn from_password(password: &str) -> Self {
        // First SHA-1 hash
        let mut hasher = Sha1::new();
        hasher.update(0xa4800c14_u32.to_be_bytes());
        hasher.update(password.as_bytes());
        hasher.update(0x5AF4A9F1_u32.to_be_bytes());
        hasher.update(0xCA6884EC_u32.to_be_bytes());
        let hash1 = hasher.finalize();

        // Second SHA-1 hash
        let mut hasher = Sha1::new();
        hasher.update(0xcb92eaeb_u32.to_be_bytes());
        hasher.update(hash1);
        hasher.update(0x1d919bf8_u32.to_be_bytes());
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

        let k1 = (get_dword(&hash2, 0) as u64) | ((get_dword(&hash2, 1) as u64) << 32);
        let k2 = (get_dword(&hash2, 2) as u64) | ((get_dword(&hash2, 3) as u64) << 32);
        let k3 = (get_dword(&hash2, 4) as u64) | ((get_dword(&hash1, 0) as u64) << 32);

        Self { k1, k2, k3 }
    }

    /// Get the default archive keys
    pub fn default_archive_keys() -> Self {
        Self::from_password(ARCHIVE_PASSWORD)
    }
}

/// 16-round TEA decipher on 4 parallel 64-bit values
fn tea_decipher_4(v0: u64, v1: u64, v2: u64, v3: u64, k0: u64, k1: u64) -> (u64, u64, u64, u64) {
    let a = k0 as u32;
    let b = (k0 >> 32) as u32;
    let c = k1 as u32;
    let d = (k1 >> 32) as u32;

    let mut y0 = v0 as u32;
    let mut z0 = (v0 >> 32) as u32;
    let mut y1 = v1 as u32;
    let mut z1 = (v1 >> 32) as u32;
    let mut y2 = v2 as u32;
    let mut z2 = (v2 >> 32) as u32;
    let mut y3 = v3 as u32;
    let mut z3 = (v3 >> 32) as u32;

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

    tea_round_4!(0xE3779B90_u32);
    tea_round_4!(0x454021D7_u32);
    tea_round_4!(0xA708A81E_u32);
    tea_round_4!(0x08D12E65_u32);
    tea_round_4!(0x6A99B4AC_u32);
    tea_round_4!(0xCC623AF3_u32);
    tea_round_4!(0x2E2AC13A_u32);
    tea_round_4!(0x8FF34781_u32);
    tea_round_4!(0xF1BBCDC8_u32);
    tea_round_4!(0x5384540F_u32);
    tea_round_4!(0xB54CDA56_u32);
    tea_round_4!(0x1715609D_u32);
    tea_round_4!(0x78DDE6E4_u32);
    tea_round_4!(0xDAA66D2B_u32);
    tea_round_4!(0x3C6EF372_u32);
    tea_round_4!(0x9E3779B9_u32);

    (
        (y0 as u64) | ((z0 as u64) << 32),
        (y1 as u64) | ((z1 as u64) << 32),
        (y2 as u64) | ((z2 as u64) << 32),
        (y3 as u64) | ((z3 as u64) << 32),
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
    let mut ctr = counter.wrapping_add((iv >> 10) as u32);
    if ctr == 0 {
        ctr = 1;
    }

    ctr = lfsr3(ctr);
    out0 ^= (ctr as u64).wrapping_add(iv);
    ctr = lfsr3(ctr);
    out1 ^= (ctr as u64).wrapping_sub(iv);
    ctr = lfsr3(ctr);
    out2 ^= (ctr as u64).wrapping_add(iv);
    ctr = lfsr3(ctr);
    out3 ^= (ctr as u64).wrapping_sub(iv);
    ctr = lfsr3(ctr);
    w4 ^= (ctr as u64).wrapping_add(iv);
    ctr = lfsr3(ctr);
    w5 ^= (ctr as u64).wrapping_sub(iv);
    ctr = lfsr3(ctr);
    w6 ^= (ctr as u64).wrapping_add(iv);
    ctr = lfsr3(ctr);
    w7 ^= (ctr as u64).wrapping_sub(iv);

    // Write output (big-endian)
    let outputs = [out0, out1, out2, out3, w4, w5, w6, w7];
    for (i, &val) in outputs.iter().enumerate() {
        let bytes = val.to_be_bytes();
        let offset = i * 8;
        dst[offset..offset + 8].copy_from_slice(&bytes);
    }
}

/// Decrypt data in-place (must be multiple of 64 bytes)
pub fn tea_decrypt_data(keys: &TeaKeys, data: &mut [u8], start_offset: u64) {
    assert!(data.len().is_multiple_of(TEA_BLOCK_SIZE));
    assert!(start_offset.is_multiple_of(TEA_BLOCK_SIZE as u64));

    let num_blocks = data.len() / TEA_BLOCK_SIZE;
    let start_counter = (start_offset / TEA_BLOCK_SIZE as u64) as u32;

    for i in 0..num_blocks {
        let offset = i * TEA_BLOCK_SIZE;
        let counter = start_counter + i as u32;

        let mut src = [0u8; 64];
        src.copy_from_slice(&data[offset..offset + 64]);

        let mut dst = [0u8; 64];
        tea_decrypt_block64(keys, &src, &mut dst, counter);

        data[offset..offset + 64].copy_from_slice(&dst);
    }
}

/// 16-round TEA encipher on 4 parallel 64-bit values (inverse of tea_decipher_4)
fn tea_encipher_4(v0: u64, v1: u64, v2: u64, v3: u64, k0: u64, k1: u64) -> (u64, u64, u64, u64) {
    let a = k0 as u32;
    let b = (k0 >> 32) as u32;
    let c = k1 as u32;
    let d = (k1 >> 32) as u32;

    let mut y0 = v0 as u32;
    let mut z0 = (v0 >> 32) as u32;
    let mut y1 = v1 as u32;
    let mut z1 = (v1 >> 32) as u32;
    let mut y2 = v2 as u32;
    let mut z2 = (v2 >> 32) as u32;
    let mut y3 = v3 as u32;
    let mut z3 = (v3 >> 32) as u32;

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
    tea_round_4_enc!(0x9E3779B9_u32);
    tea_round_4_enc!(0x3C6EF372_u32);
    tea_round_4_enc!(0xDAA66D2B_u32);
    tea_round_4_enc!(0x78DDE6E4_u32);
    tea_round_4_enc!(0x1715609D_u32);
    tea_round_4_enc!(0xB54CDA56_u32);
    tea_round_4_enc!(0x5384540F_u32);
    tea_round_4_enc!(0xF1BBCDC8_u32);
    tea_round_4_enc!(0x8FF34781_u32);
    tea_round_4_enc!(0x2E2AC13A_u32);
    tea_round_4_enc!(0xCC623AF3_u32);
    tea_round_4_enc!(0x6A99B4AC_u32);
    tea_round_4_enc!(0x08D12E65_u32);
    tea_round_4_enc!(0xA708A81E_u32);
    tea_round_4_enc!(0x454021D7_u32);
    tea_round_4_enc!(0xE3779B90_u32);

    (
        (y0 as u64) | ((z0 as u64) << 32),
        (y1 as u64) | ((z1 as u64) << 32),
        (y2 as u64) | ((z2 as u64) << 32),
        (y3 as u64) | ((z3 as u64) << 32),
    )
}

/// Block expand operation - inverse of block_contract
#[inline]
fn block_expand(x: u64, y: u64, z: u64, w: u64) -> (u64, u64, u64, u64) {
    let x = x ^ y;
    let y = y ^ z;
    let z = z ^ w;
    let w = w ^ x;
    (x, y, z, w)
}

/// Encrypt a 64-byte block in CTR mode (inverse of tea_decrypt_block64)
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
    let mut ctr = counter.wrapping_add((iv >> 10) as u32);
    if ctr == 0 {
        ctr = 1;
    }

    ctr = lfsr3(ctr);
    in0 ^= (ctr as u64).wrapping_add(iv);
    ctr = lfsr3(ctr);
    in1 ^= (ctr as u64).wrapping_sub(iv);
    ctr = lfsr3(ctr);
    in2 ^= (ctr as u64).wrapping_add(iv);
    ctr = lfsr3(ctr);
    in3 ^= (ctr as u64).wrapping_sub(iv);
    ctr = lfsr3(ctr);
    in4 ^= (ctr as u64).wrapping_add(iv);
    ctr = lfsr3(ctr);
    in5 ^= (ctr as u64).wrapping_sub(iv);
    ctr = lfsr3(ctr);
    in6 ^= (ctr as u64).wrapping_add(iv);
    ctr = lfsr3(ctr);
    in7 ^= (ctr as u64).wrapping_sub(iv);

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

/// Encrypt data in-place (must be multiple of 64 bytes)
pub fn tea_encrypt_data(keys: &TeaKeys, data: &mut [u8], start_offset: u64) {
    assert!(data.len().is_multiple_of(TEA_BLOCK_SIZE));
    assert!(start_offset.is_multiple_of(TEA_BLOCK_SIZE as u64));

    let num_blocks = data.len() / TEA_BLOCK_SIZE;
    let start_counter = (start_offset / TEA_BLOCK_SIZE as u64) as u32;

    for i in 0..num_blocks {
        let offset = i * TEA_BLOCK_SIZE;
        let counter = start_counter + i as u32;

        let mut src = [0u8; 64];
        src.copy_from_slice(&data[offset..offset + 64]);

        let mut dst = [0u8; 64];
        tea_encrypt_block64(keys, &src, &mut dst, counter);

        data[offset..offset + 64].copy_from_slice(&dst);
    }
}
