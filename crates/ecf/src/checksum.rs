//! Adler-32 checksum used by ECF headers and BDeflateStream.

/// Compute the Adler-32 checksum of `data`.
///
/// Returns `1` for an empty slice (the Adler-32 identity value).
pub fn adler32(data: &[u8]) -> u32 {
    const MOD_ADLER: u32 = 65521;
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for byte in data {
        a = (a + *byte as u32) % MOD_ADLER;
        b = (b + a) % MOD_ADLER;
    }
    (b << 16) | a
}
