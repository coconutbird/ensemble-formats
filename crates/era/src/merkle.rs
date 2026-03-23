//! Merkle tree signature verification for ERA archives.
//!
//! ERA archives are signed using a custom Merkle one-time signature scheme.
//! The signature covers a SHA-1 hash of the archive headers (ECF magic, header
//! size, chunk count, chunk extra size, adler32, and all chunk headers).
//!
//! The verification process uses a KISS PRNG seeded from the header hash to
//! select leaf nodes, then walks up the Merkle tree comparing against a
//! 20-byte public key.

use alloc::vec::Vec;
use sha1::{Digest, Sha1};

use crate::error::{Error, Result};

/// Signature block magic number (big-endian).
pub const SIGNATURE_MAGIC: u32 = 0xAAC9_4350;

/// Number of hash bits verified per signature (21 bytes × 8 = 168).
const HASH_BITS: usize = 168;

/// SHA-1 digest size in bytes.
const SHA1_SIZE: usize = 20;

// ---------------------------------------------------------------------------
// KISS PRNG – Keep It Simple Stupid
// ---------------------------------------------------------------------------
// Combines Multiply-With-Carry, XorShift, and Linear Congruential generators
// plus a Subtract-With-Borrow lag table.

/// KISS PRNG state (matches the game's `BKISSRandom` struct).
struct KissPrng {
    mwc_z: u32,        // offset 0
    mwc_w: u32,        // offset 4
    xorshift: u32,     // offset 8
    lcg: u32,          // offset 12
    _swb_x: u32,       // offset 16 (carry) — kept for struct layout parity
    _swb_y: u32,       // offset 20 — kept for struct layout parity
    table: [u32; 256], // offset 24..1047
    index: u8,         // offset 1064
    swb_a: u32,        // offset 1048
    swb_b: u32,        // offset 1052
    carry: u32,        // offset 1056
}

impl KissPrng {
    fn seed(s1: u32, s2: u32, s3: u32, s4: u32, s5: u32, s6: u32, _s7: u32) -> Self {
        let mut prng = Self {
            mwc_z: s1,
            mwc_w: s2,
            xorshift: if s3 == 0 { 1 } else { s3 },
            lcg: s4,
            _swb_x: s5,
            _swb_y: s6,
            table: [0u32; 256],
            index: 0,
            swb_a: 0,
            swb_b: 0,
            carry: 0,
        };

        // Fill table with 256 values generated from the sub-generators
        for i in 0..256u32 {
            // MWC-Z step
            let mwc = 36969u32
                .wrapping_mul(prng.mwc_z & 0xFFFF)
                .wrapping_add(prng.mwc_z >> 16);
            prng.mwc_z = mwc;

            // XorShift step
            let mut xs = prng.xorshift;
            xs ^= xs << 17;
            xs ^= xs >> 13;
            xs ^= xs << 5;
            prng.xorshift = xs;

            // MWC-W step
            let mwc_w_val = 18000u32
                .wrapping_mul(prng.mwc_w & 0xFFFF)
                .wrapping_add(prng.mwc_w >> 16);
            prng.mwc_w = mwc_w_val;

            // LCG step
            let lcg = 69069u32.wrapping_mul(prng.lcg).wrapping_add(1234567);
            prng.lcg = lcg;

            // Combine
            let combined = (mwc << 16).wrapping_add(mwc_w_val);
            prng.table[i as usize] = xs.wrapping_add(lcg ^ combined);
        }

        prng
    }

    fn next(&mut self) -> u32 {
        self.index = self.index.wrapping_add(1);
        let idx = self.index as usize;

        // XorShift step
        let mut xs = self.xorshift;
        xs ^= xs << 17;
        xs ^= xs >> 13;
        xs ^= xs << 5;
        self.xorshift = xs;

        // LCG step
        let lcg = 69069u32.wrapping_mul(self.lcg).wrapping_add(1234567);
        self.lcg = lcg;

        // MWC-Z step
        let mwc = 36969u32
            .wrapping_mul(self.mwc_z & 0xFFFF)
            .wrapping_add(self.mwc_z >> 16);
        self.mwc_z = mwc;

        // Carry comparison (subtract-with-borrow)
        let carry = if self.swb_a < self.swb_b { 1u32 } else { 0u32 };
        self.carry = carry;

        // MWC-W step
        let mwc_w = 18000u32
            .wrapping_mul(self.mwc_w & 0xFFFF)
            .wrapping_add(self.mwc_w >> 16);
        self.mwc_w = mwc_w;

        // SWB table lookups
        let a_idx = (idx as u8).wrapping_add(34) as usize;
        let b_idx = (idx as u8).wrapping_add(19) as usize;

        self.swb_a = self.table[a_idx];
        let b_val = self.table[b_idx].wrapping_add(carry);
        self.swb_b = b_val;
        let swb = self.swb_a.wrapping_sub(b_val);
        self.table[idx] = swb;

        // Final combine
        let combined = (mwc << 16).wrapping_add(mwc_w);
        swb.wrapping_add(xs).wrapping_add(lcg ^ combined)
    }
}

// ---------------------------------------------------------------------------
// Header hash computation
// ---------------------------------------------------------------------------

/// ECF magic used in the header hash (0xA7F95F9C in big-endian byte order).
const ECF_HASH_MAGIC: [u8; 4] = [0xA7, 0xF9, 0x5F, 0x9C];

/// Compute the SHA-1 hash over the ERA archive headers.
///
/// This replicates the hashing done by `ERA_LoadArchiveHeaders` in the game:
/// 1. Hash 4 magic bytes (0xA7, 0xF9, 0x5F, 0x9C)
/// 2. Hash header_size as 4 big-endian bytes
/// 3. Hash num_chunks as 4 big-endian bytes (u16 zero-extended to u32)
/// 4. Hash chunk_extra_data_size as 4 big-endian bytes (u16 zero-extended to u32)
/// 5. Hash file_size as 4 big-endian bytes
/// 6. Hash all raw chunk header bytes
pub fn compute_header_hash(
    header_size: u32,
    num_chunks: u16,
    chunk_extra_size: u16,
    file_size: u32,
    chunk_headers_raw: &[u8],
) -> [u8; SHA1_SIZE] {
    let mut hasher = Sha1::new();

    // 1. Magic bytes (fed one at a time in the game)
    for &b in &ECF_HASH_MAGIC {
        hasher.update([b]);
    }

    // 2. header_size (big-endian u32)
    for &b in &header_size.to_be_bytes() {
        hasher.update([b]);
    }

    // 3. num_chunks (zero-extended to u32, big-endian)
    let nc = num_chunks as u32;
    for &b in &nc.to_be_bytes() {
        hasher.update([b]);
    }

    // 4. chunk_extra_data_size (zero-extended to u32, big-endian)
    let ces = chunk_extra_size as u32;
    for &b in &ces.to_be_bytes() {
        hasher.update([b]);
    }

    // 5. file_size (big-endian u32) — game reads ECF offset 12-15
    for &b in &file_size.to_be_bytes() {
        hasher.update([b]);
    }

    // 6. All chunk headers (raw bytes, already big-endian on disk)
    let chunk_stride = ecf::EcfChunkHeader::SIZE + chunk_extra_size as usize;
    hasher.update(&chunk_headers_raw[..chunk_stride * num_chunks as usize]);

    let result = hasher.finalize();
    let mut hash = [0u8; SHA1_SIZE];
    hash.copy_from_slice(&result);
    hash
}

// ---------------------------------------------------------------------------
// Merkle signature verification
// ---------------------------------------------------------------------------

/// Read a big-endian u32 from a byte slice, advancing the cursor.
fn read_be_u32(data: &[u8], pos: &mut usize) -> Result<u32> {
    if *pos + 4 > data.len() {
        return Err(Error::SignatureTruncated);
    }
    let val = u32::from_be_bytes([data[*pos], data[*pos + 1], data[*pos + 2], data[*pos + 3]]);
    *pos += 4;
    Ok(val)
}

/// Read exactly `n` bytes from a byte slice, advancing the cursor.
fn read_bytes<'a>(data: &'a [u8], pos: &mut usize, n: usize) -> Result<&'a [u8]> {
    if *pos + n > data.len() {
        return Err(Error::SignatureTruncated);
    }
    let slice = &data[*pos..*pos + n];
    *pos += n;
    Ok(slice)
}

/// Compute the extended hash: 20-byte SHA-1 hash + 1-byte popcount.
fn extend_hash(hash: &[u8; SHA1_SIZE]) -> [u8; 21] {
    let mut extended = [0u8; 21];
    extended[..SHA1_SIZE].copy_from_slice(hash);
    let popcount: u8 = hash.iter().map(|b| b.count_ones() as u8).sum();
    extended[SHA1_SIZE] = popcount;
    extended
}

/// Get a specific bit from the extended hash.
fn get_bit(extended: &[u8; 21], bit_index: usize) -> bool {
    let byte_index = bit_index >> 3;
    let bit_mask = 1u8 << (bit_index & 7);
    (extended[byte_index] & bit_mask) != 0
}

/// Compute SHA-1 of a 20-byte input (used for leaf node hashing).
fn sha1_hash_node(input: &[u8; SHA1_SIZE]) -> [u8; SHA1_SIZE] {
    let mut hasher = Sha1::new();
    hasher.update(input);
    let result = hasher.finalize();
    let mut out = [0u8; SHA1_SIZE];
    out.copy_from_slice(&result);
    out
}

/// Compute the parent node hash from two child hashes.
///
/// `parent_hash = SHA1(0x01 || sibling_index_encoded || left_hash || right_hash)`
fn compute_parent_hash(
    sibling_index: u32,
    left: &[u8; SHA1_SIZE],
    right: &[u8; SHA1_SIZE],
) -> [u8; SHA1_SIZE] {
    let mut hasher = Sha1::new();
    hasher.update([0x01]);
    hasher.update([(sibling_index >> 25) as u8]);
    hasher.update([(sibling_index >> 17) as u8]);
    hasher.update([(sibling_index >> 9) as u8]);
    hasher.update([(sibling_index >> 1) as u8]);
    hasher.update(left.as_slice());
    hasher.update(right.as_slice());
    let result = hasher.finalize();
    let mut out = [0u8; SHA1_SIZE];
    out.copy_from_slice(&result);
    out
}

/// Seed the KISS PRNG from a SHA-1 hash (as done by the game).
fn seed_prng_from_hash(hash: &[u8; SHA1_SIZE]) -> KissPrng {
    // Interpret hash as 5 big-endian u32s
    let w0 = u32::from_be_bytes([hash[0], hash[1], hash[2], hash[3]]);
    let w1 = u32::from_be_bytes([hash[4], hash[5], hash[6], hash[7]]);
    let w2 = u32::from_be_bytes([hash[8], hash[9], hash[10], hash[11]]);
    let w3 = u32::from_be_bytes([hash[12], hash[13], hash[14], hash[15]]);
    let w4 = u32::from_be_bytes([hash[16], hash[17], hash[18], hash[19]]);

    let s7 = (w0 ^ w1 ^ w2 ^ w3 ^ w4).wrapping_add(1);

    KissPrng::seed(w0, w1, w2, w3, w4, s7, 0)
}

/// Pick the starting leaf index using PRNG.
fn pick_starting_leaf(prng: &mut KissPrng, half: u32) -> u32 {
    let inv = 1.0 / (u32::MAX as f64 + 1.0);
    let size = half as f64;
    loop {
        let raw = prng.next();
        let f = raw as f64 * inv;
        if (0.0..1.0).contains(&f) {
            let idx = (size * f) as i32;
            if idx >= 0 && (idx as u32) < half {
                return idx as u32 + half;
            }
        }
    }
}

/// Look up a node in cache, or return None.
fn cache_lookup(cache: &[(u32, [u8; SHA1_SIZE])], index: u32) -> Option<[u8; SHA1_SIZE]> {
    cache.iter().find(|(k, _)| *k == index).map(|(_, v)| *v)
}

/// Insert a node into the cache.
fn cache_insert(cache: &mut Vec<(u32, [u8; SHA1_SIZE])>, index: u32, hash: [u8; SHA1_SIZE]) {
    if let Some(entry) = cache.iter_mut().find(|(k, _)| *k == index) {
        entry.1 = hash;
    } else {
        cache.push((index, hash));
    }
}

/// Verify a Merkle signature against a public key and header hash.
///
/// - `public_key`: 20-byte public key (Merkle tree root hash)
/// - `header_hash`: 20-byte SHA-1 of the ERA headers
/// - `signature`: raw signature bytes (including magic, depth, node hashes, end magic)
///
/// Returns `Ok(true)` if the signature is valid, `Ok(false)` if verification
/// fails cleanly, or `Err` on parse errors.
pub fn verify(
    public_key: &[u8; SHA1_SIZE],
    header_hash: &[u8; SHA1_SIZE],
    signature: &[u8],
) -> Result<bool> {
    let mut pos = 0usize;

    // 1. Read and validate start magic
    let magic = read_be_u32(signature, &mut pos)?;
    if magic != SIGNATURE_MAGIC {
        return Err(Error::InvalidSignatureMagic {
            expected: SIGNATURE_MAGIC,
            found: magic,
        });
    }

    // 2. Read tree depth
    if pos >= signature.len() {
        return Err(Error::SignatureTruncated);
    }
    let depth = signature[pos];
    pos += 1;

    if !(2..=32).contains(&depth) {
        return Err(Error::InvalidTreeDepth { depth });
    }

    let tree_size = 1u32 << depth;
    let half = tree_size >> 1;

    // 3. Compute extended hash (20 bytes + popcount byte = 21 bytes = 168 bits)
    let extended = extend_hash(header_hash);

    // 4. Seed PRNG and pick starting leaf
    let mut prng = seed_prng_from_hash(header_hash);
    let start_leaf = pick_starting_leaf(&mut prng, half);

    // 5. Node cache: maps node_index -> 20-byte hash
    let mut cache: Vec<(u32, [u8; SHA1_SIZE])> = Vec::new();

    let mut leaf_index = start_leaf;

    for bit_index in 0..HASH_BITS {
        let bit_set = get_bit(&extended, bit_index);

        // Get or read the current leaf node hash
        let current_hash = match cache_lookup(&cache, leaf_index) {
            Some(h) => h,
            None => {
                // Check stream has data
                if pos + SHA1_SIZE > signature.len() {
                    return Ok(false);
                }
                let raw = read_bytes(signature, &mut pos, SHA1_SIZE)?;
                let mut node = [0u8; SHA1_SIZE];
                node.copy_from_slice(raw);

                // If bit is NOT set, hash the raw bytes (public commitment)
                // If bit IS set, use raw bytes directly (secret preimage)
                let resolved = if bit_set { node } else { sha1_hash_node(&node) };

                cache_insert(&mut cache, leaf_index, resolved);
                resolved
            }
        };

        // Build path from leaf to root and walk up
        let mut path = Vec::new();
        let mut idx = leaf_index;
        while idx > 1 {
            path.push(idx);
            idx >>= 1;
        }

        let mut accumulated = current_hash;

        for &node_idx in &path {
            if node_idx == 1 {
                break;
            }
            // Compute sibling index
            let sibling = if node_idx & 1 == 0 {
                node_idx + 1
            } else {
                node_idx - 1
            };

            // Get or read sibling hash
            let sibling_hash = match cache_lookup(&cache, sibling) {
                Some(h) => h,
                None => {
                    if pos + SHA1_SIZE > signature.len() {
                        return Ok(false);
                    }
                    let raw = read_bytes(signature, &mut pos, SHA1_SIZE)?;
                    let mut node = [0u8; SHA1_SIZE];
                    node.copy_from_slice(raw);

                    // Determine if we hash this sibling's raw data
                    let resolved = if sibling < half {
                        // Internal node: use raw bytes
                        node
                    } else {
                        // Leaf node: check corresponding bit
                        let mut sib_bit = if sibling < leaf_index {
                            bit_index as i32 - 1
                        } else {
                            bit_index as i32 + 1
                        };
                        // Wrap into [0, HASH_BITS)
                        if sib_bit < 0 {
                            sib_bit += HASH_BITS as i32;
                        } else if sib_bit >= HASH_BITS as i32 {
                            sib_bit -= HASH_BITS as i32;
                        }
                        if get_bit(&extended, sib_bit as usize) {
                            node // bit set: use raw
                        } else {
                            sha1_hash_node(&node) // bit not set: hash
                        }
                    };

                    cache_insert(&mut cache, sibling, resolved);
                    resolved
                }
            };

            // Compute parent hash: left child has smaller index
            let (left, right) = if sibling < node_idx {
                (&sibling_hash, &accumulated)
            } else {
                (&accumulated, &sibling_hash)
            };
            accumulated = compute_parent_hash(sibling, left, right);
        }

        // Compare accumulated root with public key
        if accumulated != *public_key {
            return Ok(false);
        }

        // Advance to next leaf (wrapping)
        leaf_index += 1;
        if leaf_index == tree_size {
            leaf_index = half;
        }
    }

    // 6. Read and validate end magic
    if pos + 4 <= signature.len() {
        let end_magic = read_be_u32(signature, &mut pos)?;
        if end_magic == SIGNATURE_MAGIC && pos <= signature.len() {
            return Ok(true);
        }
    }

    // If we got through all 168 iterations matching, it's valid even without end magic
    Ok(true)
}
