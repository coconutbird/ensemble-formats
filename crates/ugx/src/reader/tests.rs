use super::*;
use crate::constants::EMPTY_OFFSET_SENTINEL;
use std::{eprint, eprintln, format};

/// Helper: read a test file, skipping if not present on disk.
fn read_test_file(path: &str) -> Option<Vec<u8>> {
    std::fs::read(path).ok()
}

#[test]
fn read_full_ugx_pipeline() {
    // Exercise the full parse pipeline on every available test file.
    let paths = [
        "../../foxcannon01/mesh_turret_0.ugx",
        "../../foxcannon01/mesh_barrel_0.ugx",
        "../../foxcannon01/mesh_chassis_front_0.ugx",
        "../../foxcannon01/mesh_foxcannon01.ugx",
        "../../test_ugx/art/covenant/air/banshee_01/banshee_damage_01.ugx",
        "../../test_ugx/art/covenant/air/banshee_01/upgrade_01.ugx",
    ];

    let mut parsed = 0usize;
    for path in paths {
        let Some(data) = read_test_file(path) else {
            continue;
        };

        let geom = Reader::read(&data).expect(path);

        // Basic structural invariants
        assert!(!geom.sections.is_empty(), "{path}: no sections");
        assert!(!geom.index_buffer.is_empty(), "{path}: empty IB");
        assert!(!geom.vertex_buffer.is_empty(), "{path}: empty VB");
        assert!(geom.bounding_sphere.radius > 0.0, "{path}: zero radius");

        for (i, sec) in geom.sections.iter().enumerate() {
            assert!(sec.num_verts > 0, "{path} sec[{i}]: zero verts");
            assert!(sec.num_tris > 0, "{path} sec[{i}]: zero tris");
            assert!(sec.vert_size > 0, "{path} sec[{i}]: zero vert_size");
            if let Some(ref packer) = sec.base_vert_packer {
                assert!(
                    !packer.pack_order.is_empty(),
                    "{path} sec[{i}]: empty pack_order"
                );
            }
        }

        parsed += 1;
    }

    if parsed == 0 {
        eprintln!("No test UGX files found on disk — skipping");
    }
}

#[test]
fn inspect_hw2_ugx() {
    let paths = [
        "/Users/dev/gamedepot/wstore/DUMP/data/maps/rostermode/evenflow_desert/evenflow_desert_water_01/mesh_water.ugx",
        "/Users/dev/gamedepot/wstore/DUMP/data/maps/rostermode/evenflow_desert/evenflow_desert_water_01/childmesh_child_asset003.ugx",
    ];

    for path in paths {
        let Some(data) = read_test_file(path) else {
            eprintln!("HW2 test file not found, skipping");
            continue;
        };
        inspect_hw2_file(path, &data);
    }
}

fn inspect_hw2_file(path: &str, data: &[u8]) {
    let ecf = ecf::Reader::new(data).unwrap();
    eprintln!("\n=== {} ===", path.rsplit('/').next().unwrap());
    eprintln!("ECF file ID: 0x{:08X}", ecf.header().id);
    eprintln!("ECF chunks: {}", ecf.chunks().len());
    for (index, chunk) in ecf.chunks().iter().enumerate() {
        eprintln!("  chunk[{index}]: id=0x{:X}, size={}", chunk.id, chunk.size);
    }

    let cached = ecf.chunk_data_by_id(0x700).unwrap();
    eprintln!("Chunk 0x700 size: {} bytes", cached.len());
    eprintln!("Signature: 0x{:08X}", read_u32(&cached, 0).unwrap_or(0));
    print_hex_bytes(&cached, 0, 160);
    inspect_packed_arrays(&cached);
    inspect_related_chunks(&ecf);
    inspect_sections(&cached);

    eprintln!("\n--- Attempting UGX parse ---");
    match UgxGeom::from_bytes(data) {
        Ok(geom) => eprintln!(
            "SUCCESS!\n  Sections: {}\n  Materials: {}\n  Bones: {}",
            geom.sections.len(),
            geom.materials.len(),
            geom.bones.len()
        ),
        Err(error) => eprintln!("FAILED: {error:?}"),
    }
}

fn inspect_packed_arrays(cached: &[u8]) {
    eprintln!("\n--- Packed Arrays (after 64-byte header) ---");
    for array_index in 0..8 {
        let base = 64 + array_index * 16;
        let (Some(count), Some(offset)) = (read_u32(cached, base), read_u64(cached, base + 8))
        else {
            break;
        };
        eprintln!("  Array[{array_index}]: count={count}, offset=0x{offset:X}");
        let Some(absolute_offset) = usize::try_from(offset).ok() else {
            continue;
        };
        if count <= 1 || absolute_offset >= cached.len() {
            continue;
        }
        let next_base = base + 16;
        let Some(next_offset) = read_u64(cached, next_base + 8) else {
            continue;
        };
        if next_offset > offset && next_offset != EMPTY_OFFSET_SENTINEL {
            let span = next_offset - offset;
            eprintln!(
                "    -> span to next: {span} bytes, per-element: {}",
                span / u64::from(count)
            );
        }
    }
}

fn inspect_related_chunks(ecf: &ecf::Reader<'_>) {
    for chunk_id in [0x701u64, 0x702, 0x703, 0x704, 0x705] {
        match ecf.chunk_data_by_id(chunk_id) {
            Ok(data) => {
                eprintln!("\nChunk 0x{chunk_id:03X}: {} bytes", data.len());
                if chunk_id == 0x705 {
                    eprintln!(
                        "  AABB tree version: 0x{:08X}",
                        read_u32(&data, 0).unwrap_or(0)
                    );
                    eprintln!(
                        "  AABB tree node count: {}",
                        read_u32(&data, 4).unwrap_or(0)
                    );
                }
            }
            Err(_) => eprintln!("\nChunk 0x{chunk_id:03X}: NOT FOUND"),
        }
    }
}

fn inspect_sections(cached: &[u8]) {
    let Some(section_count) = read_u32(cached, 64).and_then(|value| usize::try_from(value).ok())
    else {
        return;
    };
    let Some(section_offset) = read_u64(cached, 72).and_then(|value| usize::try_from(value).ok())
    else {
        return;
    };
    let Some(section_data) = cached.get(section_offset..) else {
        return;
    };
    eprintln!(
        "\n--- Section data (Array[0]: count={section_count}, offset=0x{section_offset:X}) ---"
    );
    for (index, section) in section_data
        .as_chunks::<72>()
        .0
        .iter()
        .take(section_count)
        .enumerate()
    {
        let offset = section_offset.saturating_add(index.saturating_mul(72));
        eprintln!("  Section[{index}] at 0x{offset:X}:");
        print_hex_bytes(section, offset, section.len());
        print_section_fields(section);
    }
}

fn print_section_fields(section: &[u8; 72]) {
    let fields: Vec<_> = (0..10)
        .filter_map(|index| read_i32(section, index * 4))
        .collect();
    let [
        material,
        accessory,
        max_bones,
        rigid_bone,
        ib_offset,
        triangle_count,
        vb_offset,
        vb_bytes,
        vertex_size,
        vertex_count,
    ] = fields.as_slice()
    else {
        return;
    };
    eprintln!("    mat={material} acc={accessory} maxBones={max_bones} rigidBone={rigid_bone}");
    eprintln!(
        "    ibOfs={ib_offset} numTris={triangle_count} vbOfs={vb_offset} vbBytes={vb_bytes}"
    );
    eprintln!("    vertSize={vertex_size} numVerts={vertex_count}");
}

fn print_hex_bytes(data: &[u8], base_offset: usize, limit: usize) {
    for (row, bytes) in data[..data.len().min(limit)].chunks(16).enumerate() {
        let offset = base_offset.saturating_add(row.saturating_mul(16));
        eprint!("\n  {offset:04x}: ");
        for byte in bytes {
            eprint!("{byte:02x} ");
        }
    }
    eprintln!();
}

fn read_u32(data: &[u8], offset: usize) -> Option<u32> {
    let end = offset.checked_add(4)?;
    Some(u32::from_le_bytes(data.get(offset..end)?.try_into().ok()?))
}

fn read_i32(data: &[u8], offset: usize) -> Option<i32> {
    let end = offset.checked_add(4)?;
    Some(i32::from_le_bytes(data.get(offset..end)?.try_into().ok()?))
}

fn read_u64(data: &[u8], offset: usize) -> Option<u64> {
    let end = offset.checked_add(8)?;
    Some(u64::from_le_bytes(data.get(offset..end)?.try_into().ok()?))
}

#[test]
fn binary_diff_hw2_roundtrip() {
    let paths = [
        "/Users/dev/gamedepot/wstore/DUMP/data/maps/rostermode/evenflow_desert/evenflow_desert_water_01/mesh_water.ugx",
        "/Users/dev/gamedepot/wstore/DUMP/data/maps/rostermode/evenflow_desert/evenflow_desert_water_01/childmesh_child_asset003.ugx",
    ];

    for path in paths {
        let Some(original) = read_test_file(path) else {
            eprintln!("Skipping {path}");
            continue;
        };

        let geom = UgxGeom::from_bytes(&original).unwrap();
        let round_tripped = geom.to_bytes().unwrap();

        let fname = path.rsplit('/').next().unwrap();
        eprintln!("\n=== {fname} ===");
        eprintln!("Original:      {} bytes", original.len());
        eprintln!("Round-tripped: {} bytes", round_tripped.len());

        if original == round_tripped {
            eprintln!("BYTE-EXACT MATCH!");
            continue;
        }

        // Compare chunk-by-chunk via ECF
        let orig_ecf = ecf::Reader::new(&original).unwrap();
        let rt_ecf = ecf::Reader::new(&round_tripped).unwrap();

        eprintln!(
            "Original ECF:  {} chunks, ID=0x{:08X}",
            orig_ecf.chunks().len(),
            orig_ecf.header().id
        );
        eprintln!(
            "RoundTrip ECF: {} chunks, ID=0x{:08X}",
            rt_ecf.chunks().len(),
            rt_ecf.header().id
        );

        // Compare chunk ordering
        eprintln!("\nChunk order:");
        eprintln!(
            "  Original:    {:?}",
            orig_ecf
                .chunks()
                .iter()
                .map(|c| format!("0x{:03X}", c.id))
                .collect::<Vec<_>>()
        );
        eprintln!(
            "  RoundTrip:   {:?}",
            rt_ecf
                .chunks()
                .iter()
                .map(|c| format!("0x{:03X}", c.id))
                .collect::<Vec<_>>()
        );

        for chunk in orig_ecf.chunks() {
            let orig_data = orig_ecf.chunk_data_by_id(chunk.id).unwrap();
            match rt_ecf.chunk_data_by_id(chunk.id) {
                Ok(rt_data) => {
                    if orig_data == rt_data {
                        eprintln!(
                            "Chunk 0x{:03X}: MATCH ({} bytes)",
                            chunk.id,
                            orig_data.len()
                        );
                    } else {
                        eprintln!(
                            "Chunk 0x{:03X}: DIFFERENT (orig={}, rt={})",
                            chunk.id,
                            orig_data.len(),
                            rt_data.len()
                        );
                        let mut chunk_diffs = 0;
                        for (index, (&original_byte, &roundtrip_byte)) in
                            orig_data.iter().zip(rt_data.iter()).enumerate()
                        {
                            if original_byte != roundtrip_byte {
                                chunk_diffs += 1;
                                if chunk_diffs <= 20 {
                                    eprintln!(
                                        "  diff at chunk+0x{index:04X}: orig=0x{original_byte:02X} vs rt=0x{roundtrip_byte:02X}"
                                    );
                                }
                            }
                        }
                        if orig_data.len() != rt_data.len() {
                            eprintln!("  SIZE DIFF: orig={} rt={}", orig_data.len(), rt_data.len());
                        }
                        eprintln!("  Total chunk byte diffs: {chunk_diffs}");
                    }
                }
                Err(_) => {
                    eprintln!("Chunk 0x{:03X}: MISSING in round-trip", chunk.id);
                }
            }
        }

        for chunk in rt_ecf.chunks() {
            if orig_ecf.chunk_data_by_id(chunk.id).is_err() {
                eprintln!(
                    "Chunk 0x{:03X}: EXTRA in round-trip ({} bytes)",
                    chunk.id, chunk.size
                );
            }
        }
    }
}

#[test]
fn diag_material_chunk_diff() {
    let path = "/Users/dev/gamedepot/wstore/DUMP/data/maps/rostermode/evenflow_desert/evenflow_desert_water_01/mesh_water.ugx";
    let Some(original) = read_test_file(path) else {
        eprintln!("Skipping material diag");
        return;
    };

    let orig_ecf = ecf::Reader::new(&original).unwrap();
    let orig_mat = orig_ecf.chunk_data_by_id(0x704).unwrap();
    eprintln!("=== Original 0x704: {} bytes ===", orig_mat.len());
    print_hex_bytes(&orig_mat, 0, 80);

    // Parse the original material data with BDT reader and dump the tree
    match bdt::Reader::read(&orig_mat, bdt::Endian::Little) {
        Ok(Some(root)) => {
            eprintln!("\n=== Original BDT tree ===");
            dump_bdt(&root, 0);
        }
        Ok(None) => eprintln!("Original BDT: empty"),
        Err(e) => eprintln!("Original BDT error: {e:?}"),
    }

    // Now round-trip
    let geom = UgxGeom::from_bytes(&original).unwrap();
    let round_tripped = geom.to_bytes().unwrap();
    let rt_ecf = ecf::Reader::new(&round_tripped).unwrap();
    let rt_mat = rt_ecf.chunk_data_by_id(0x704).unwrap();
    eprintln!("\n=== Round-tripped 0x704: {} bytes ===", rt_mat.len());
    print_hex_bytes(&rt_mat, 0, 80);

    match bdt::Reader::read(&rt_mat, bdt::Endian::Little) {
        Ok(Some(root)) => {
            eprintln!("\n=== Round-tripped BDT tree ===");
            dump_bdt(&root, 0);
        }
        Ok(None) => eprintln!("RT BDT: empty"),
        Err(e) => eprintln!("RT BDT error: {e:?}"),
    }
}

fn dump_bdt(node: &bdt::Node, depth: usize) {
    let indent = "  ".repeat(depth);
    let text = match &node.text {
        bdt::Variant::Null => std::string::String::new(),
        v => format!(" text={v:?}"),
    };
    eprint!("{indent}<{}", node.name);
    for attribute in &node.attributes {
        eprint!(" @{}={:?}", attribute.name, attribute.value);
    }
    eprintln!("{text}>");
    for child in &node.children {
        dump_bdt(child, depth + 1);
    }
}
