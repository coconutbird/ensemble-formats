//! Merkle tree signature scheme for ERA archives.
//!
//! ERA archives are signed using a custom Merkle one-time signature scheme.
//! The signature covers a SHA-1 hash of the archive headers (ECF magic, header
//! size, chunk count, chunk extra size, adler32, and all chunk headers).
//!
//! The verification process uses a KISS PRNG seeded from the header hash to
//! select leaf nodes, then walks up the Merkle tree comparing against a
//! 20-byte public key.
//!
//! # Signing
//!
//! To generate signatures, create a [`PrivateKey`] from random leaf secrets
//! and use [`sign`] to produce a signature blob that [`verify`] will accept.
//!
//! ```ignore
//! let private_key = era::crypto::merkle::PrivateKey::from_secrets(10, secrets);
//! let public_key = private_key.public_key();
//! let signature = era::crypto::merkle::sign(&private_key, &header_hash).unwrap();
//! assert!(era::crypto::merkle::verify(&public_key, &header_hash, &signature).unwrap());
//! ```

use alloc::vec;
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
        for index in 0..256usize {
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
            let lcg = 69069u32.wrapping_mul(prng.lcg).wrapping_add(1_234_567);
            prng.lcg = lcg;

            // Combine
            let combined = (mwc << 16).wrapping_add(mwc_w_val);
            prng.table[index] = xs.wrapping_add(lcg ^ combined);
        }

        prng
    }

    fn next(&mut self) -> u32 {
        self.index = self.index.wrapping_add(1);
        let idx = usize::from(self.index);

        // XorShift step
        let mut xs = self.xorshift;
        xs ^= xs << 17;
        xs ^= xs >> 13;
        xs ^= xs << 5;
        self.xorshift = xs;

        // LCG step
        let lcg = 69069u32.wrapping_mul(self.lcg).wrapping_add(1_234_567);
        self.lcg = lcg;

        // MWC-Z step
        let mwc = 36969u32
            .wrapping_mul(self.mwc_z & 0xFFFF)
            .wrapping_add(self.mwc_z >> 16);
        self.mwc_z = mwc;

        // Carry comparison (subtract-with-borrow)
        let carry = u32::from(self.swb_a < self.swb_b);
        self.carry = carry;

        // MWC-W step
        let mwc_w = 18000u32
            .wrapping_mul(self.mwc_w & 0xFFFF)
            .wrapping_add(self.mwc_w >> 16);
        self.mwc_w = mwc_w;

        // SWB table lookups
        let a_idx = usize::from(self.index.wrapping_add(34));
        let b_idx = usize::from(self.index.wrapping_add(19));

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
/// 2. Hash `header_size` as 4 big-endian bytes
/// 3. Hash `num_chunks` as 4 big-endian bytes (u16 zero-extended to u32)
/// 4. Hash `chunk_extra_data_size` as 4 big-endian bytes (u16 zero-extended to u32)
/// 5. Hash `file_size` as 4 big-endian bytes
/// 6. Hash all raw chunk header bytes
#[must_use]
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
    let nc = u32::from(num_chunks);
    for &b in &nc.to_be_bytes() {
        hasher.update([b]);
    }

    // 4. chunk_extra_data_size (zero-extended to u32, big-endian)
    let ces = u32::from(chunk_extra_size);
    for &b in &ces.to_be_bytes() {
        hasher.update([b]);
    }

    // 5. file_size (big-endian u32) — game reads ECF offset 12-15
    for &b in &file_size.to_be_bytes() {
        hasher.update([b]);
    }

    // 6. All chunk headers (raw bytes, already big-endian on disk)
    let chunk_stride = ecf::EcfChunkHeader::SIZE + usize::from(chunk_extra_size);
    hasher.update(&chunk_headers_raw[..chunk_stride * usize::from(num_chunks)]);

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
    let Some(end) = pos.checked_add(n) else {
        return Err(Error::SignatureTruncated);
    };
    let Some(slice) = data.get(*pos..end) else {
        return Err(Error::SignatureTruncated);
    };
    *pos = end;
    Ok(slice)
}

/// Read one hash node from a signature, returning `None` for clean truncation.
fn read_signature_node(data: &[u8], pos: &mut usize) -> Result<Option<[u8; SHA1_SIZE]>> {
    if data.len().saturating_sub(*pos) < SHA1_SIZE {
        return Ok(None);
    }

    let raw = read_bytes(data, pos, SHA1_SIZE)?;
    let mut node = [0u8; SHA1_SIZE];
    node.copy_from_slice(raw);
    Ok(Some(node))
}

/// Compute the extended hash: 20-byte SHA-1 hash + 1-byte popcount.
fn extend_hash(hash: &[u8; SHA1_SIZE]) -> [u8; 21] {
    let mut extended = [0u8; 21];
    extended[..SHA1_SIZE].copy_from_slice(hash);
    let popcount = hash
        .iter()
        .map(|byte| u8::try_from(byte.count_ones()).unwrap_or_default())
        .sum();
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
    hasher.update((sibling_index >> 1).to_be_bytes());
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
    let scaled = u64::from(half) * u64::from(prng.next());
    let offset = u32::try_from(scaled >> u32::BITS).unwrap_or_default();
    half + offset
}

/// Return the adjacent hash-bit index for a leaf sibling.
fn sibling_bit_index(bit_index: usize, sibling_before: bool) -> usize {
    if sibling_before {
        bit_index.checked_sub(1).unwrap_or(HASH_BITS - 1)
    } else if bit_index + 1 == HASH_BITS {
        0
    } else {
        bit_index + 1
    }
}

/// Advance to the next leaf, wrapping to the first leaf after the last node.
fn next_leaf_index(leaf_index: u32, half: u32) -> u32 {
    let next = leaf_index.wrapping_add(1);
    if next == half.wrapping_mul(2) {
        half
    } else {
        next
    }
}

/// Build the sequence of nodes traversed from a leaf toward the root.
fn path_to_root(mut node_index: u32) -> Vec<u32> {
    let mut path = Vec::new();
    while node_index > 1 {
        path.push(node_index);
        node_index >>= 1;
    }
    path
}

/// Return the sibling of a non-root node.
fn sibling_node_index(node_index: u32) -> u32 {
    if node_index & 1 == 0 {
        node_index + 1
    } else {
        node_index - 1
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

// ---------------------------------------------------------------------------
// Key generation and signing
// ---------------------------------------------------------------------------

/// Private key for the Merkle one-time signature scheme.
///
/// Contains the tree depth and all leaf secrets. The full Merkle tree
/// (internal nodes + public key) is derived deterministically from these.
#[derive(Clone)]
pub struct PrivateKey {
    depth: u8,
    /// Leaf secrets: `secrets[i]` corresponds to tree node `half + i`.
    secrets: Vec<[u8; SHA1_SIZE]>,
    /// Full tree: `tree[i]` is the hash for node index `i` (1-indexed).
    /// Index 0 is unused. Index 1 is the root (public key).
    tree: Vec<[u8; SHA1_SIZE]>,
}

impl PrivateKey {
    /// Build a private key from raw leaf secrets.
    ///
    /// - `depth`: tree depth (2..=32). The tree has `2^(depth-1)` leaves.
    /// - `secrets`: exactly `2^(depth-1)` random 20-byte values.
    ///
    /// # Panics
    ///
    /// Panics if `secrets.len() != 2^(depth-1)` or depth is out of range.
    #[must_use]
    pub fn from_secrets(depth: u8, secrets: Vec<[u8; SHA1_SIZE]>) -> Self {
        assert!((2..=32).contains(&depth), "depth must be 2..=32");
        let half = 1usize
            .checked_shl(u32::from(depth - 1))
            .expect("tree depth must fit the target pointer width");
        let tree_size = half
            .checked_mul(2)
            .expect("tree size must fit the target pointer width");
        assert_eq!(
            secrets.len(),
            half,
            "expected {half} secrets for depth {depth}"
        );

        // Allocate tree (1-indexed, so tree_size entries; index 0 unused)
        let mut tree = vec![[0u8; SHA1_SIZE]; tree_size];

        // Leaf nodes: tree[half + i] = SHA1(secrets[i])
        for (i, secret) in secrets.iter().enumerate() {
            tree[half + i] = sha1_hash_node(secret);
        }

        // Internal nodes: bottom-up
        // For each level from (half-1) down to 1:
        //   tree[parent] = compute_parent_hash(sibling_index, left, right)
        //
        // The sibling_index used in compute_parent_hash is the RIGHT child
        // when called from verify. Looking at verify:
        //   accumulated = compute_parent_hash(sibling, left, right)
        // where sibling is the sibling of node_idx. The first arg is always
        // the sibling index regardless of left/right position.
        //
        // For building the tree, parent = i, children = 2i (left) and 2i+1 (right).
        // In verify, compute_parent_hash's first arg is the sibling index.
        // When node_idx is left child (even), sibling = node_idx+1 (right child).
        // When node_idx is right child (odd), sibling = node_idx-1 (left child).
        // So the first arg alternates. But for a canonical tree build, we need
        // to match what verify expects.
        //
        // verify does: compute_parent_hash(sibling, left, right)
        // where left is the child with smaller index.
        // The sibling arg is always the sibling's index (not the current node).
        //
        // For parent i with children 2i and 2i+1:
        // If we're walking up from child 2i, sibling = 2i+1
        // If we're walking up from child 2i+1, sibling = 2i
        //
        // But the parent hash must be the same regardless of which child we
        // walk from. Let's check: compute_parent_hash(sibling, left, right)
        // uses sibling in the hash. So from child 2i: parent = f(2i+1, tree[2i], tree[2i+1])
        // From child 2i+1: parent = f(2i, tree[2i], tree[2i+1])
        // These are DIFFERENT because the sibling_index differs!
        //
        // That means the tree is NOT a standard Merkle tree — the parent hash
        // depends on which child you're walking from. This means we can't
        // pre-build a single canonical tree. Instead, the tree is implicitly
        // defined by the walk path.
        //
        // For signing, we need to emit the right values so that the verifier's
        // walk produces the correct root. We'll store leaf hashes and secrets,
        // and compute internal nodes on-the-fly during signing.

        Self {
            depth,
            secrets,
            tree,
        }
    }

    /// Get the public key (Merkle tree root hash).
    ///
    /// Note: because the parent hash depends on the sibling index used during
    /// the walk, the "public key" is the root that the verifier computes.
    /// We determine it by replaying the verifier's walk for the supplied hash.
    #[must_use]
    pub fn public_key(&self, header_hash: &[u8; SHA1_SIZE]) -> [u8; SHA1_SIZE] {
        // Sign, then extract the root the verifier would compute.
        // We do this by running the verify walk ourselves with known values.
        self.compute_root(header_hash)
    }

    /// Return a tree node while validating its platform-sized index.
    fn signing_tree_node(&self, node_index: u32) -> Result<[u8; SHA1_SIZE]> {
        let index = usize::try_from(node_index)
            .map_err(|_| Error::SizeOverflow("private-key node index"))?;
        self.tree
            .get(index)
            .copied()
            .ok_or(Error::SignatureVerifyFailed)
    }

    /// Return the secret corresponding to a leaf node.
    fn signing_leaf_secret(&self, node_index: u32, half: u32) -> Result<[u8; SHA1_SIZE]> {
        let local_index = node_index
            .checked_sub(half)
            .ok_or(Error::SignatureVerifyFailed)?;
        let index = usize::try_from(local_index)
            .map_err(|_| Error::SizeOverflow("private-key leaf index"))?;
        self.secrets
            .get(index)
            .copied()
            .ok_or(Error::SignatureVerifyFailed)
    }

    /// Compute the root hash that the verifier would produce for a given header hash.
    fn compute_root(&self, header_hash: &[u8; SHA1_SIZE]) -> [u8; SHA1_SIZE] {
        let half = 1u32 << (self.depth - 1);

        let mut prng = seed_prng_from_hash(header_hash);
        let start_leaf = pick_starting_leaf(&mut prng, half);

        // Replay the walk to compute what the verifier would get as root
        let mut cache: Vec<(u32, [u8; SHA1_SIZE])> = Vec::new();
        let mut leaf_index = start_leaf;
        let mut root = [0u8; SHA1_SIZE];

        for _ in 0..HASH_BITS {
            let current_hash = if let Some(h) = cache_lookup(&cache, leaf_index) {
                h
            } else {
                // The verifier would read from sig and resolve:
                // bit_set=true: resolved = raw (which is leaf hash)
                // bit_set=false: resolved = SHA1(raw) where raw is secret
                // Either way, resolved = leaf hash = tree[leaf_index]
                let resolved = self.tree[usize::try_from(leaf_index)
                    .expect("private-key node index must fit the target pointer width")];
                cache_insert(&mut cache, leaf_index, resolved);
                resolved
            };

            let path = path_to_root(leaf_index);

            let mut accumulated = current_hash;

            for &node_idx in &path {
                if node_idx == 1 {
                    break;
                }
                let sibling = sibling_node_index(node_idx);

                let sibling_hash = if let Some(h) = cache_lookup(&cache, sibling) {
                    h
                } else {
                    let resolved = if sibling < half {
                        // Internal node — compute on the fly
                        // This is what the verifier would read from sig
                        // We need to figure out what value the verifier
                        // would store. For internal nodes, verifier uses
                        // raw bytes directly. So we need the value that,
                        // when used in compute_parent_hash, gives the
                        // correct parent.
                        //
                        // But we don't have pre-built internal nodes
                        // because parent hashes depend on the walk path.
                        // We need to compute them bottom-up for this
                        // specific subtree.
                        self.compute_subtree_hash(sibling, half)
                    } else {
                        // Leaf sibling: resolved = tree[sibling]
                        self.tree[usize::try_from(sibling)
                            .expect("private-key node index must fit the target pointer width")]
                    };
                    cache_insert(&mut cache, sibling, resolved);
                    resolved
                };

                let (left, right) = if sibling < node_idx {
                    (&sibling_hash, &accumulated)
                } else {
                    (&accumulated, &sibling_hash)
                };
                accumulated = compute_parent_hash(sibling, left, right);
            }

            root = accumulated;

            leaf_index = next_leaf_index(leaf_index, half);
        }

        root
    }

    /// Recursively compute the hash for a subtree rooted at `node_index`.
    fn compute_subtree_hash(&self, node_index: u32, half: u32) -> [u8; SHA1_SIZE] {
        if node_index >= half {
            // Leaf node
            return self.tree[usize::try_from(node_index)
                .expect("private-key node index must fit the target pointer width")];
        }

        let left_child = node_index * 2;
        let right_child = node_index * 2 + 1;

        let left_hash = self.compute_subtree_hash(left_child, half);
        let right_hash = self.compute_subtree_hash(right_child, half);

        // The sibling arg in compute_parent_hash: when the verifier walks up
        // from a child, it uses the sibling's index. For a canonical subtree
        // computation, we need to pick which child is "the sibling."
        // Since the verifier always calls compute_parent_hash(sibling, left, right),
        // and sibling could be either child, we need to match the verifier's
        // perspective. For an internal node that the verifier reads from the
        // signature stream, the verifier just stores it as-is and uses it
        // when computing the parent above. So the value we emit for this
        // internal node IS the hash that the verifier will use as one side
        // of compute_parent_hash at the level above.
        //
        // The verifier calls compute_parent_hash(sibling_of_this_node, ...)
        // at the parent level, not at this level. So the value we store here
        // is just the subtree hash, and the sibling_index used will be
        // determined by the walk at the parent level.
        //
        // For computing the subtree hash itself, we need to pick a consistent
        // sibling_index. The right child is the sibling when walking from left.
        compute_parent_hash(right_child, &left_hash, &right_hash)
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
///
/// # Errors
///
/// Returns an error when the signature header is truncated or contains an
/// invalid magic value or tree depth.
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

    let half = 1u32 << (depth - 1);

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
        let current_hash = if let Some(h) = cache_lookup(&cache, leaf_index) {
            h
        } else {
            let Some(node) = read_signature_node(signature, &mut pos)? else {
                return Ok(false);
            };

            // If bit is NOT set, hash the raw bytes (public commitment)
            // If bit IS set, use raw bytes directly (secret preimage)
            let resolved = if bit_set { node } else { sha1_hash_node(&node) };

            cache_insert(&mut cache, leaf_index, resolved);
            resolved
        };

        // Build path from leaf to root and walk up
        let path = path_to_root(leaf_index);

        let mut accumulated = current_hash;

        for &node_idx in &path {
            if node_idx == 1 {
                break;
            }
            // Compute sibling index
            let sibling = sibling_node_index(node_idx);

            // Get or read sibling hash
            let sibling_hash = if let Some(h) = cache_lookup(&cache, sibling) {
                h
            } else {
                let Some(node) = read_signature_node(signature, &mut pos)? else {
                    return Ok(false);
                };

                // Determine if we hash this sibling's raw data
                let resolved = if sibling < half {
                    // Internal node: use raw bytes
                    node
                } else {
                    // Leaf node: check corresponding bit
                    let sibling_bit = sibling_bit_index(bit_index, sibling < leaf_index);
                    if get_bit(&extended, sibling_bit) {
                        node // bit set: use raw
                    } else {
                        sha1_hash_node(&node) // bit not set: hash
                    }
                };

                cache_insert(&mut cache, sibling, resolved);
                resolved
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
        leaf_index = next_leaf_index(leaf_index, half);
    }

    // 6. Read and validate end magic
    if signature.len().saturating_sub(pos) >= 4 {
        let end_magic = read_be_u32(signature, &mut pos)?;
        if end_magic == SIGNATURE_MAGIC {
            return Ok(true);
        }
    }

    // If we got through all 168 iterations matching, it's valid even without end magic
    Ok(true)
}

/// Write a big-endian u32 to a byte vector.
fn write_be_u32(buf: &mut Vec<u8>, val: u32) {
    buf.extend_from_slice(&val.to_be_bytes());
}

/// Generate a Merkle signature for the given header hash.
///
/// Replays the same PRNG-driven walk as [`verify`], emitting the node
/// values that the verifier expects to read from the signature stream.
///
/// - For leaf nodes: emits the secret (preimage) when the bit is 0,
///   or the leaf hash when the bit is 1.
/// - For internal sibling nodes: emits the precomputed subtree hash.
///
/// Returns the raw signature bytes (magic + depth + node hashes + end magic).
///
/// # Errors
///
/// Returns an error if the private key has an invalid depth or its stored tree
/// and leaf data are inconsistent with that depth.
pub fn sign(private_key: &PrivateKey, header_hash: &[u8; SHA1_SIZE]) -> Result<Vec<u8>> {
    let depth = private_key.depth;
    if !(2..=32).contains(&depth) {
        return Err(Error::InvalidTreeDepth { depth });
    }

    let half = 1u32 << (depth - 1);

    let extended = extend_hash(header_hash);

    let mut prng = seed_prng_from_hash(header_hash);
    let start_leaf = pick_starting_leaf(&mut prng, half);

    let mut sig = Vec::new();

    // 1. Write start magic
    write_be_u32(&mut sig, SIGNATURE_MAGIC);

    // 2. Write depth
    sig.push(depth);

    // 3. Walk and emit nodes (mirrors verify exactly)
    let mut cache: Vec<(u32, [u8; SHA1_SIZE])> = Vec::new();
    let mut leaf_index = start_leaf;

    for bit_index in 0..HASH_BITS {
        let bit_set = get_bit(&extended, bit_index);

        // Leaf node
        if cache_lookup(&cache, leaf_index).is_none() {
            // Emit the value the verifier will read
            let leaf_hash = private_key.signing_tree_node(leaf_index)?;
            if bit_set {
                // Verifier does: resolved = node (uses raw bytes as leaf hash)
                // So emit the leaf hash directly
                sig.extend_from_slice(&leaf_hash);
            } else {
                // Verifier does: resolved = SHA1(node)
                // So emit the secret preimage
                sig.extend_from_slice(&private_key.signing_leaf_secret(leaf_index, half)?);
            }
            // Cache the resolved value (always the leaf hash)
            cache_insert(&mut cache, leaf_index, leaf_hash);
        }

        let current_hash = cache_lookup(&cache, leaf_index).ok_or(Error::SignatureVerifyFailed)?;

        // Build path from leaf to root
        let path = path_to_root(leaf_index);

        let mut accumulated = current_hash;

        for &node_idx in &path {
            if node_idx == 1 {
                break;
            }
            let sibling = sibling_node_index(node_idx);

            if cache_lookup(&cache, sibling).is_none() {
                // Emit sibling value
                if sibling < half {
                    // Internal node: verifier uses raw bytes directly
                    let subtree_hash = private_key.compute_subtree_hash(sibling, half);
                    sig.extend_from_slice(&subtree_hash);
                    cache_insert(&mut cache, sibling, subtree_hash);
                } else {
                    // Leaf sibling: same bit-dependent logic as verify
                    let sibling_hash = private_key.signing_tree_node(sibling)?;
                    let sibling_bit = sibling_bit_index(bit_index, sibling < leaf_index);

                    if get_bit(&extended, sibling_bit) {
                        // Verifier: resolved = node (raw)
                        sig.extend_from_slice(&sibling_hash);
                    } else {
                        // Verifier: resolved = SHA1(node), so emit secret
                        sig.extend_from_slice(&private_key.signing_leaf_secret(sibling, half)?);
                    }
                    cache_insert(&mut cache, sibling, sibling_hash);
                }
            }

            let sibling_hash = cache_lookup(&cache, sibling).ok_or(Error::SignatureVerifyFailed)?;

            let (left, right) = if sibling < node_idx {
                (&sibling_hash, &accumulated)
            } else {
                (&accumulated, &sibling_hash)
            };
            accumulated = compute_parent_hash(sibling, left, right);
        }

        leaf_index = next_leaf_index(leaf_index, half);
    }

    // 4. Write end magic
    write_be_u32(&mut sig, SIGNATURE_MAGIC);

    Ok(sig)
}
