extern crate std;

use super::*;
use alloc::vec;
use alloc::vec::Vec;

#[test]
fn height_grid_uses_retail_cache_blocks() {
    let header = header(ByteOrder::Little, 9, 16, 2.0);
    let mut heights = vec![0_u8; 16 * 16 * 2];
    write_height(&mut heights, ByteOrder::Little, 16, 3, 5, 42.5);
    write_height(&mut heights, ByteOrder::Little, 16, 8, 8, -7.25);

    let terrain = Terrain::from_chunks(&header, &heights, None).unwrap();

    assert_eq!(terrain.byte_order, ByteOrder::Little);
    assert_eq!(terrain.data_axis, 8);
    assert_eq!(terrain.height_axis, 9);
    assert_eq!(terrain.cache_axis, 16);
    assert_eq!(terrain.height_tile_scale.to_bits(), 2.0_f32.to_bits());
    assert_eq!(terrain.heights.len(), 16 * 16);
    assert_eq!(terrain.height_index(3, 5), Some(43));
    assert_eq!(terrain.height_index(8, 8), Some(192));
    assert_eq!(terrain.height_sample(3, 5), Some(42.5));
    assert_eq!(terrain.height_sample(8, 8), Some(-7.25));
    assert_eq!(terrain.height_sample(5, 3), Some(0.0));
    assert_eq!(terrain.height_index(16, 0), None);
    assert_eq!(terrain.height_index(0, 16), None);
}

#[test]
fn big_endian_and_little_endian_files_decode_to_the_same_values() {
    let mut decoded = Vec::new();
    for byte_order in [ByteOrder::Big, ByteOrder::Little] {
        let mut header = header(byte_order, 8, 8, 2.0);
        write_f32(&mut header[8..12], byte_order, 1.5);
        let mut heights = vec![0_u8; 8 * 8 * 2];
        write_height(&mut heights, byte_order, 8, 7, 2, 6.5);
        write_height(&mut heights, byte_order, 8, 3, 4, 11.5);
        let terrain = Terrain::from_chunks(&header, &heights, Some(&[3; 64])).unwrap();
        assert_eq!(terrain.byte_order, byte_order);
        assert_eq!(terrain.height_sample(7, 2), Some(6.5));
        decoded.push(terrain);
    }

    let [big, little] = <[Terrain; 2]>::try_from(decoded).unwrap();
    assert_eq!(
        big.data_tile_scale.to_bits(),
        little.data_tile_scale.to_bits()
    );
    assert_eq!(
        big.heights
            .iter()
            .map(|height| height.to_bits())
            .collect::<Vec<_>>(),
        little
            .heights
            .iter()
            .map(|height| height.to_bits())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        Terrain {
            byte_order: ByteOrder::Little,
            ..big
        },
        little
    );
}

#[test]
fn tile_types_are_optional_and_row_major() {
    let mut header = header(ByteOrder::Little, 8, 8, 1.0);
    write_i32(&mut header[4..8], ByteOrder::Little, 4);
    let heights = vec![0_u8; 8 * 8 * 2];
    let mut tile_types = (0_u8..16).collect::<Vec<_>>();
    tile_types.extend([99, 99]);

    let with_types = Terrain::from_chunks(&header, &heights, Some(&tile_types)).unwrap();
    let without_types = Terrain::from_chunks(&header, &heights, None).unwrap();

    assert_eq!(with_types.tile_types.as_deref(), Some(&tile_types[..16]));
    assert_eq!(with_types.tile_type(2, 1), Some(6));
    assert_eq!(with_types.tile_type(3, 3), Some(15));
    assert_eq!(with_types.tile_type(4, 0), None);
    assert_eq!(without_types.tile_types, None);
    assert_eq!(without_types.tile_type(0, 0), None);
}

#[test]
fn read_finds_the_chunks_in_the_container() {
    let header = header(ByteOrder::Big, 8, 8, 1.0);
    let mut heights = vec![0_u8; 8 * 8 * 2];
    write_height(&mut heights, ByteOrder::Big, 8, 1, 1, 2.0);

    let terrain = Terrain::read(&container(&[
        (CHUNK_HEADER, header.clone()),
        (CHUNK_HEIGHTS, heights.clone()),
        (CHUNK_TILE_TYPES, vec![7; 64]),
    ]))
    .unwrap();
    assert_eq!(terrain.height_sample(1, 1), Some(2.0));
    assert_eq!(terrain.tile_type(7, 7), Some(7));

    let terrain = Terrain::read(&container(&[
        (CHUNK_HEADER, header.clone()),
        (CHUNK_HEIGHTS, heights.clone()),
    ]))
    .unwrap();
    assert_eq!(terrain.tile_types, None);

    assert!(matches!(
        Terrain::read(&container(&[(CHUNK_HEIGHTS, heights)])),
        Err(Error::MissingChunk(CHUNK_HEADER))
    ));
    assert!(matches!(
        Terrain::read(&container(&[(CHUNK_HEADER, header)])),
        Err(Error::MissingChunk(CHUNK_HEIGHTS))
    ));
    assert!(matches!(Terrain::read(&[0; 8]), Err(Error::Container(_))));
}

#[test]
fn malformed_headers_are_rejected() {
    let heights = vec![0_u8; 8 * 8 * 2];
    let valid = header(ByteOrder::Little, 8, 8, 1.0);

    assert!(matches!(
        Terrain::from_chunks(&valid[..31], &heights, None),
        Err(Error::TruncatedHeader {
            expected: 32,
            actual: 31
        })
    ));

    let mut version = valid.clone();
    write_i32(&mut version[0..4], ByteOrder::Little, 5);
    assert!(matches!(
        Terrain::from_chunks(&version, &heights, None),
        Err(Error::UnsupportedVersion([5, 0, 0, 0]))
    ));

    for (offset, field) in [
        (4, "data tile axis"),
        (16, "height axis"),
        (20, "cache axis"),
    ] {
        let mut zero_axis = valid.clone();
        write_i32(&mut zero_axis[offset..offset + 4], ByteOrder::Little, 0);
        assert!(
            matches!(
                Terrain::from_chunks(&zero_axis, &heights, None),
                Err(Error::InvalidDimensions(message)) if message == field
            ),
            "{field}"
        );
    }

    for (height_axis, cache_axis) in [(9, 8), (8, 12)] {
        let dimensions = header(ByteOrder::Little, height_axis, cache_axis, 1.0);
        assert!(matches!(
            Terrain::from_chunks(&dimensions, &vec![0; 16 * 16 * 2], None),
            Err(Error::InvalidDimensions(
                "cache axis must contain the height grid and be divisible by eight"
            ))
        ));
    }

    for scale in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        let height_scale = header(ByteOrder::Little, 8, 8, scale);
        assert!(matches!(
            Terrain::from_chunks(&height_scale, &heights, None),
            Err(Error::InvalidDimensions(
                "height tile scale must be finite and positive"
            ))
        ));
        let mut data_scale = valid.clone();
        write_f32(&mut data_scale[8..12], ByteOrder::Little, scale);
        assert!(matches!(
            Terrain::from_chunks(&data_scale, &heights, None),
            Err(Error::InvalidDimensions(
                "data tile scale must be finite and positive"
            ))
        ));
    }
}

#[test]
fn malformed_grids_are_rejected() {
    let header = header(ByteOrder::Little, 8, 8, 1.0);
    let heights = vec![0_u8; 8 * 8 * 2];

    assert!(matches!(
        Terrain::from_chunks(&header, &heights[..127], None),
        Err(Error::TruncatedHeights {
            expected: 128,
            actual: 127
        })
    ));

    let mut infinite = heights.clone();
    infinite[2..4].copy_from_slice(&half::f16::INFINITY.to_bits().to_le_bytes());
    assert!(matches!(
        Terrain::from_chunks(&header, &infinite, None),
        Err(Error::InvalidDimensions("height samples must be finite"))
    ));

    assert!(matches!(
        Terrain::from_chunks(&header, &heights, Some(&[0; 63])),
        Err(Error::TruncatedTileTypes {
            expected: 64,
            actual: 63
        })
    ));
}

fn header(byte_order: ByteOrder, height_axis: i32, cache_axis: i32, scale: f32) -> Vec<u8> {
    let mut bytes = vec![0_u8; HEADER_SIZE];
    write_i32(&mut bytes[0..4], byte_order, XSD_VERSION);
    write_i32(&mut bytes[4..8], byte_order, 8);
    write_f32(&mut bytes[8..12], byte_order, 1.0);
    write_f32(&mut bytes[12..16], byte_order, 1.0);
    write_i32(&mut bytes[16..20], byte_order, height_axis);
    write_i32(&mut bytes[20..24], byte_order, cache_axis);
    write_f32(&mut bytes[24..28], byte_order, scale);
    write_i32(&mut bytes[28..32], byte_order, 1);
    bytes
}

fn write_height(
    bytes: &mut [u8],
    byte_order: ByteOrder,
    cache_axis: usize,
    x: usize,
    z: usize,
    height: f32,
) {
    let blocks_per_axis = cache_axis / HEIGHT_BLOCK_AXIS;
    let index = (z / HEIGHT_BLOCK_AXIS) * blocks_per_axis * HEIGHT_BLOCK_SIZE
        + (x / HEIGHT_BLOCK_AXIS) * HEIGHT_BLOCK_SIZE
        + (z % HEIGHT_BLOCK_AXIS) * HEIGHT_BLOCK_AXIS
        + x % HEIGHT_BLOCK_AXIS;
    let bits = half::f16::from_f32(height).to_bits();
    let encoded = match byte_order {
        ByteOrder::Big => bits.to_be_bytes(),
        ByteOrder::Little => bits.to_le_bytes(),
    };
    bytes[index * 2..index * 2 + 2].copy_from_slice(&encoded);
}

fn write_i32(bytes: &mut [u8], byte_order: ByteOrder, value: i32) {
    bytes.copy_from_slice(&match byte_order {
        ByteOrder::Big => value.to_be_bytes(),
        ByteOrder::Little => value.to_le_bytes(),
    });
}

fn write_f32(bytes: &mut [u8], byte_order: ByteOrder, value: f32) {
    bytes.copy_from_slice(&match byte_order {
        ByteOrder::Big => value.to_be_bytes(),
        ByteOrder::Little => value.to_le_bytes(),
    });
}

fn container(chunks: &[(u64, Vec<u8>)]) -> Vec<u8> {
    let mut writer = ecf::Writer::new(0);
    for (id, data) in chunks {
        writer.add_chunk(*id, data.clone());
    }
    writer.finalize().unwrap()
}
