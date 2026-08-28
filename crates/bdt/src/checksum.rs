//! CRC checksums for `BPackedHeader` validation.
//!
//! The engine's `BDT_BPackedDocumentReader_init` validates two checksums:
//! - **Header checksum** (byte 2): CRC-16/CCITT over the 28-byte header with byte 2 zeroed
//! - **Data CRC** (bytes 4-7): CRC-32 (ISO 3309) over the data section

/// CRC-16/CCITT (XMODEM) lookup table.
///
/// Polynomial: 0x1021, init: 0xFFFF, final XOR: 0xFFFF.
/// Matches the engine's `word_141184C40` table.
const CRC16_TABLE: [u16; 256] = {
    let mut table = [0u16; 256];
    let mut i = 0u16;
    while i < 256 {
        let mut crc = i << 8;
        let mut j = 0;
        while j < 8 {
            if crc & 0x8000 != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
            j += 1;
        }
        table[i as usize] = crc;
        i += 1;
    }
    table
};

/// CRC-32 (ISO 3309 / ITU-T V.42) lookup table.
///
/// Polynomial: 0xEDB88320 (reflected), init: 0xFFFFFFFF, final XOR: 0xFFFFFFFF.
/// Matches the engine's `dword_1411845F0` table.
const CRC32_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut i = 0u32;
    while i < 256 {
        let mut crc = i;
        let mut j = 0;
        while j < 8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
            j += 1;
        }
        table[i as usize] = crc;
        i += 1;
    }
    table
};

/// Compute the CRC-16/CCITT header checksum.
///
/// The engine computes this over the 28-byte `BPackedHeader` with byte 2 zeroed,
/// using init=0xFFFF and final XOR ~crc (complement).
#[must_use]
pub fn crc16_ccitt(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &b in data {
        let idx = ((crc >> 8) ^ u16::from(b)).to_le_bytes()[0];
        crc = (crc << 8) ^ CRC16_TABLE[usize::from(idx)];
    }
    !crc
}

/// Compute the CRC-32 data checksum.
///
/// Standard CRC-32 with init=0xFFFFFFFF and final XOR ~crc.
#[must_use]
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in data {
        let idx = (crc ^ u32::from(b)).to_le_bytes()[0];
        crc = (crc >> 8) ^ CRC32_TABLE[usize::from(idx)];
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_known_values() {
        // CRC-32 of empty = 0x00000000 (since !0xFFFFFFFF = 0x00000000... no)
        // Actually CRC-32("") = 0x00000000 per the standard? No, it's 0.
        // Let's test with known value: CRC-32("123456789") = 0xCBF43926
        let data = b"123456789";
        assert_eq!(crc32(data), 0xCBF4_3926);
    }

    #[test]
    fn crc16_table_spot_check() {
        // Verify first few entries match engine's table
        assert_eq!(CRC16_TABLE[0], 0x0000);
        assert_eq!(CRC16_TABLE[1], 0x1021);
        assert_eq!(CRC16_TABLE[2], 0x2042);
        assert_eq!(CRC16_TABLE[3], 0x3063);
    }

    #[test]
    fn crc32_table_spot_check() {
        // Verify first few entries match engine's table
        assert_eq!(CRC32_TABLE[0], 0x0000_0000);
        assert_eq!(CRC32_TABLE[1], 0x7707_3096);
        assert_eq!(CRC32_TABLE[2], 0xEE0E_612C);
        assert_eq!(CRC32_TABLE[3], 0x9909_51BA);
    }
}
