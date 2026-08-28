use super::*;

fn assert_float_array_bits_eq<const N: usize>(actual: &[f32; N], expected: &[f32; N]) {
    for (actual, expected) in actual.iter().zip(expected) {
        assert_eq!(actual.to_bits(), expected.to_bits());
    }
}

fn assert_float_bits_eq(actual: f32, expected: f32, message: &str) {
    assert_eq!(actual.to_bits(), expected.to_bits(), "{message}");
}

#[test]
fn test_float3_unpack() {
    let data: [u8; 12] = [
        0x00, 0x00, 0x80, 0x3F, // 1.0f
        0x00, 0x00, 0x00, 0x40, // 2.0f
        0x00, 0x00, 0x40, 0x40, // 3.0f
    ];
    let mut pos = 0;
    let result = VertexElementType::Float3.unpack(&data, &mut pos).unwrap();
    assert_float_array_bits_eq(&result, &[1.0, 2.0, 3.0, 1.0]);
}

#[test]
fn test_ubyte4n_unpack() {
    let data: [u8; 4] = [255, 128, 0, 255];
    let mut pos = 0;
    let result = VertexElementType::UByte4N.unpack(&data, &mut pos).unwrap();
    assert!((result[0] - 1.0).abs() < 0.01);
    assert!((result[1] - 0.5).abs() < 0.01);
    assert!((result[2] - 0.0).abs() < 0.01);
    assert!((result[3] - 1.0).abs() < 0.01);
}

#[test]
fn test_element_sizes() {
    assert_eq!(VertexElementType::Float3.size(), 12);
    assert_eq!(VertexElementType::Float4.size(), 16);
    assert_eq!(VertexElementType::HalfFloat2.size(), 4);
    assert_eq!(VertexElementType::Dec3N.size(), 4);
    assert_eq!(VertexElementType::UByte4.size(), 4);
}

#[test]
fn test_unpack_as_indices_ubyte4() {
    let data: [u8; 4] = [5, 10, 200, 0];
    let mut pos = 0;
    let result = VertexElementType::UByte4
        .unpack_as_indices(&data, &mut pos)
        .unwrap();
    assert_eq!(result, [5, 10, 200, 0]);
}

#[test]
fn test_unpack_as_indices_ubyte4n_not_normalized() {
    let data: [u8; 4] = [255, 128, 0, 255];
    let mut pos = 0;
    let result = VertexElementType::UByte4N
        .unpack_as_indices(&data, &mut pos)
        .unwrap();
    assert_eq!(result, [255, 128, 0, 255]);
}

#[test]
fn test_unpack_as_indices_short4_positive() {
    let mut data = Vec::new();
    data.extend_from_slice(&300i16.to_le_bytes());
    data.extend_from_slice(&1i16.to_le_bytes());
    data.extend_from_slice(&0i16.to_le_bytes());
    data.extend_from_slice(&0i16.to_le_bytes());
    let mut pos = 0;
    let result = VertexElementType::Short4
        .unpack_as_indices(&data, &mut pos)
        .unwrap();
    assert_eq!(result, [300, 1, 0, 0]);
}

#[test]
fn test_unpack_as_indices_short4_negative_clamped() {
    let mut data = Vec::new();
    data.extend_from_slice(&(-1i16).to_le_bytes());
    data.extend_from_slice(&5i16.to_le_bytes());
    data.extend_from_slice(&(-100i16).to_le_bytes());
    data.extend_from_slice(&0i16.to_le_bytes());
    let mut pos = 0;
    let result = VertexElementType::Short4
        .unpack_as_indices(&data, &mut pos)
        .unwrap();
    assert_eq!(result, [0, 5, 0, 0]);
}

#[test]
fn test_unpack_as_indices_ushort4n_not_normalized() {
    let mut data = Vec::new();
    data.extend_from_slice(&500u16.to_le_bytes());
    data.extend_from_slice(&65535u16.to_le_bytes());
    data.extend_from_slice(&0u16.to_le_bytes());
    data.extend_from_slice(&1u16.to_le_bytes());
    let mut pos = 0;
    let result = VertexElementType::UShort4N
        .unpack_as_indices(&data, &mut pos)
        .unwrap();
    assert_eq!(result, [500, 65535, 0, 1]);
}

// ---- Pack/unpack round-trip tests ----

fn roundtrip_pack_unpack(ty: VertexElementType, value: [f32; 4]) -> [f32; 4] {
    let mut buf = Vec::new();
    ty.pack(&mut buf, value);
    assert_eq!(buf.len(), ty.size(), "packed size mismatch for {ty:?}");
    let mut pos = 0;
    ty.unpack(&buf, &mut pos).unwrap()
}

#[test]
fn test_float3_roundtrip() {
    let v = [1.0, -2.5, 3.125, 1.0];
    let r = roundtrip_pack_unpack(VertexElementType::Float3, v);
    assert_float_array_bits_eq(&r, &v);
}

#[test]
fn test_float4_roundtrip() {
    let v = [1.0, -2.5, 3.125, 0.5];
    let r = roundtrip_pack_unpack(VertexElementType::Float4, v);
    assert_float_array_bits_eq(&r, &v);
}

#[test]
fn test_ubyte4_roundtrip() {
    let v = [5.0, 10.0, 200.0, 0.0];
    let r = roundtrip_pack_unpack(VertexElementType::UByte4, v);
    assert_float_array_bits_eq(&r, &v);
}

#[test]
fn test_ubyte4n_roundtrip() {
    let v = [1.0, 0.5, 0.0, 1.0];
    let r = roundtrip_pack_unpack(VertexElementType::UByte4N, v);
    assert!((r[0] - 1.0).abs() < 0.01);
    assert!((r[1] - 0.5).abs() < 0.01);
    assert!((r[2] - 0.0).abs() < 0.01);
    assert!((r[3] - 1.0).abs() < 0.01);
}

#[test]
fn test_halffloat2_roundtrip() {
    let v = [1.0, -0.5, 0.0, 1.0];
    let r = roundtrip_pack_unpack(VertexElementType::HalfFloat2, v);
    assert!((r[0] - 1.0).abs() < 0.001);
    assert!((r[1] - (-0.5)).abs() < 0.001);
}

#[test]
fn test_dec3n_roundtrip() {
    let v = [0.5, -0.5, 1.0, 1.0];
    let r = roundtrip_pack_unpack(VertexElementType::Dec3N, v);
    assert!((r[0] - 0.5).abs() < 0.01, "x: expected ~0.5, got {}", r[0]);
    assert!(
        (r[1] - (-0.5)).abs() < 0.01,
        "y: expected ~-0.5, got {}",
        r[1]
    );
    assert!((r[2] - 1.0).abs() < 0.01, "z: expected ~1.0, got {}", r[2]);
    assert_float_bits_eq(r[3], 1.0, "w: positive handedness should roundtrip");
}

#[test]
fn test_dec3n_negative_handedness() {
    let v = [0.5, -0.5, 1.0, -1.0];
    let r = roundtrip_pack_unpack(VertexElementType::Dec3N, v);
    assert!((r[0] - 0.5).abs() < 0.01, "x: expected ~0.5, got {}", r[0]);
    assert_float_bits_eq(r[3], -1.0, "w: negative handedness should roundtrip");
}

#[test]
fn test_dec3n_preserves_minus_512() {
    // Manually pack with -512 in X component (raw 0x200 in bits 0-9)
    let raw: u32 = 0x200; // X = -512, Y = 0, Z = 0, W = 0b00
    let data = raw.to_le_bytes();
    let mut pos = 0;
    let r = VertexElementType::Dec3N.unpack(&data, &mut pos).unwrap();
    // -512/511 = -1.00196 — preserved so unpack→repack is byte-identical
    let expected = -512.0f32 / 511.0;
    assert_float_bits_eq(r[0], expected, "x: -512/511 should be preserved");
    assert_float_bits_eq(r[3], 1.0, "w: bits 30-31 = 0b00 → +1.0");
}

#[test]
fn test_pack_as_indices_ubyte4_roundtrip() {
    let indices = [5u16, 10, 200, 0];
    let mut buf = Vec::new();
    VertexElementType::UByte4.pack_as_indices(&mut buf, indices);
    let mut pos = 0;
    let result = VertexElementType::UByte4
        .unpack_as_indices(&buf, &mut pos)
        .unwrap();
    assert_eq!(result, indices);
}

#[test]
fn test_pack_as_indices_short4_roundtrip() {
    let indices = [300u16, 1, 0, 0];
    let mut buf = Vec::new();
    VertexElementType::Short4.pack_as_indices(&mut buf, indices);
    let mut pos = 0;
    let result = VertexElementType::Short4
        .unpack_as_indices(&buf, &mut pos)
        .unwrap();
    assert_eq!(result, indices);
}
