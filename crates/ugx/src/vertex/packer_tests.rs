use super::*;
use alloc::string::ToString;

#[test]
fn test_vertex_size_calculation() {
    let packer = UnivertPacker {
        pack_order: "PNT0".to_string(),
        pos_type: VertexElementType::Float3,
        normal_type: VertexElementType::Float3,
        uv_types: [VertexElementType::Float2; MAX_UV],
        ..Default::default()
    };

    // Position (12) + Normal (12) + UV (8) = 32
    assert_eq!(packer.vertex_size(), 32);
}

#[test]
fn test_vertex_size_with_skin() {
    let packer = UnivertPacker {
        pack_order: "PNT0S".to_string(),
        pos_type: VertexElementType::Float3,
        normal_type: VertexElementType::Float3,
        uv_types: [VertexElementType::Float2; MAX_UV],
        indices_type: VertexElementType::UByte4,
        weights_type: VertexElementType::UByte4N,
        ..Default::default()
    };

    // Position (12) + Normal (12) + UV (8) + Indices (4) + Weights (4) = 40
    assert_eq!(packer.vertex_size(), 40);
}

#[test]
fn test_pack_unpack_vertex_roundtrip() {
    let packer = UnivertPacker {
        pack_order: "PNT0".to_string(),
        pos_type: VertexElementType::Float3,
        normal_type: VertexElementType::Float3,
        uv_types: [VertexElementType::Float2; MAX_UV],
        ..Default::default()
    };

    let original = UnpackedVertex {
        position: [1.0, 2.0, 3.0],
        normal: [0.0, 1.0, 0.0],
        texcoords: {
            let mut tc = [[0.0; 2]; MAX_UV];
            tc[0] = [0.5, 0.75];
            tc
        },
        num_texcoords: 1,
        ..Default::default()
    };

    let mut buf = Vec::new();
    packer.pack_vertex(&mut buf, &original);
    assert_eq!(buf.len(), packer.vertex_size());

    let mut pos = 0;
    let unpacked = packer.unpack_vertex(&buf, &mut pos).unwrap();

    assert_eq!(unpacked.position, original.position);
    assert_eq!(unpacked.normal, original.normal);
    assert_eq!(unpacked.texcoords[0], original.texcoords[0]);
}

#[test]
fn test_pack_unpack_vertex_with_skin_roundtrip() {
    let packer = UnivertPacker {
        pack_order: "PNT0S".to_string(),
        pos_type: VertexElementType::Float3,
        normal_type: VertexElementType::Float3,
        uv_types: [VertexElementType::Float2; MAX_UV],
        indices_type: VertexElementType::UByte4,
        weights_type: VertexElementType::Float4,
        ..Default::default()
    };

    let original = UnpackedVertex {
        position: [-1.0, 5.0, 0.0],
        normal: [1.0, 0.0, 0.0],
        texcoords: {
            let mut tc = [[0.0; 2]; MAX_UV];
            tc[0] = [0.25, 0.5];
            tc
        },
        num_texcoords: 1,
        bone_indices: [3, 1, 0, 0],
        bone_weights: [0.7, 0.3, 0.0, 0.0],
        ..Default::default()
    };

    let mut buf = Vec::new();
    packer.pack_vertex(&mut buf, &original);
    assert_eq!(buf.len(), packer.vertex_size());

    let mut pos = 0;
    let unpacked = packer.unpack_vertex(&buf, &mut pos).unwrap();

    assert_eq!(unpacked.position, original.position);
    assert_eq!(unpacked.normal, original.normal);
    assert_eq!(unpacked.texcoords[0], original.texcoords[0]);
    assert_eq!(unpacked.bone_indices, original.bone_indices);
    assert_eq!(unpacked.bone_weights, original.bone_weights);
}
