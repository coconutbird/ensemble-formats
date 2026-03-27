use super::*;
use std::{eprint, eprintln};

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
        let data = match read_test_file(path) {
            Some(d) => d,
            None => continue,
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
        let data = match read_test_file(path) {
            Some(d) => d,
            None => {
                eprintln!("HW2 test file not found, skipping");
                continue;
            }
        };

        let ecf = ecf::Reader::new(&data).unwrap();

        eprintln!("\n=== {} ===", path.rsplit('/').next().unwrap());
        eprintln!("ECF file ID: 0x{:08X}", ecf.header().id);
        eprintln!("ECF chunks: {}", ecf.chunks().len());
        for (i, chunk) in ecf.chunks().iter().enumerate() {
            eprintln!("  chunk[{}]: id=0x{:X}, size={}", i, chunk.id, chunk.size);
        }

        let cached = ecf.chunk_data_by_id(0x700).unwrap();
        eprintln!("Chunk 0x700 size: {} bytes", cached.len());

        let sig = u32::from_le_bytes([cached[0], cached[1], cached[2], cached[3]]);
        eprintln!("Signature: 0x{:08X}", sig);

        // Dump first 160 bytes
        for (i, byte) in cached.iter().enumerate().take(160) {
            if i % 16 == 0 {
                eprint!("\n  {:04x}: ", i);
            }
            eprint!("{:02x} ", byte);
        }
        eprintln!();

        // After 64-byte header, read packed arrays
        eprintln!("\n--- Packed Arrays (after 64-byte header) ---");
        for arr_idx in 0..8 {
            let base = 64 + arr_idx * 16;
            if base + 16 > cached.len() {
                break;
            }
            let count = u32::from_le_bytes([
                cached[base],
                cached[base + 1],
                cached[base + 2],
                cached[base + 3],
            ]);
            let offset = u64::from_le_bytes([
                cached[base + 8],
                cached[base + 9],
                cached[base + 10],
                cached[base + 11],
                cached[base + 12],
                cached[base + 13],
                cached[base + 14],
                cached[base + 15],
            ]);
            eprintln!(
                "  Array[{}]: count={}, offset=0x{:X}",
                arr_idx, count, offset
            );

            if count > 1 && (offset as usize) < cached.len() {
                let next_base = 64 + (arr_idx + 1) * 16;
                if next_base + 16 <= cached.len() {
                    let next_offset = u64::from_le_bytes([
                        cached[next_base + 8],
                        cached[next_base + 9],
                        cached[next_base + 10],
                        cached[next_base + 11],
                        cached[next_base + 12],
                        cached[next_base + 13],
                        cached[next_base + 14],
                        cached[next_base + 15],
                    ]);
                    if next_offset > offset && next_offset != EMPTY_OFFSET_SENTINEL {
                        let span = next_offset - offset;
                        eprintln!(
                            "    -> span to next: {} bytes, per-element: {}",
                            span,
                            span / count as u64
                        );
                    }
                }
            }
        }

        // Check other chunks
        for chunk_id in [0x701u64, 0x702, 0x703, 0x704, 0x705] {
            match ecf.chunk_data_by_id(chunk_id) {
                Ok(d) => {
                    eprintln!("\nChunk 0x{:03X}: {} bytes", chunk_id, d.len());
                    if chunk_id == 0x705 {
                        let ver = u32::from_le_bytes([d[0], d[1], d[2], d[3]]);
                        eprintln!("  AABB tree version: 0x{:08X}", ver);
                        let nc = u32::from_le_bytes([d[4], d[5], d[6], d[7]]);
                        eprintln!("  AABB tree node count: {}", nc);
                    }
                }
                Err(_) => eprintln!("\nChunk 0x{:03X}: NOT FOUND", chunk_id),
            }
        }

        // Dump section data (Array[0]) for the childmesh file
        if cached.len() > 0xA0 {
            let arr0_count =
                u32::from_le_bytes([cached[64], cached[65], cached[66], cached[67]]) as usize;
            let arr0_offset = u64::from_le_bytes([
                cached[72], cached[73], cached[74], cached[75], cached[76], cached[77], cached[78],
                cached[79],
            ]) as usize;

            eprintln!(
                "\n--- Section data (Array[0]: count={}, offset=0x{:X}) ---",
                arr0_count, arr0_offset
            );
            for sec_idx in 0..arr0_count {
                let sec_start = arr0_offset + sec_idx * 72;
                eprintln!("  Section[{}] at 0x{:X}:", sec_idx, sec_start);
                for row in 0..5 {
                    let row_start = sec_start + row * 16;
                    if row_start + 16 <= cached.len() {
                        eprint!("    {:04x}: ", row_start);
                        for b in 0..16 {
                            if row_start + b < cached.len() {
                                eprint!("{:02x} ", cached[row_start + b]);
                            }
                        }
                        eprintln!();
                    }
                }
                // Remaining 8 bytes
                let rem_start = sec_start + 64;
                if rem_start + 8 <= cached.len() {
                    eprint!("    {:04x}: ", rem_start);
                    for b in 0..8 {
                        eprint!("{:02x} ", cached[rem_start + b]);
                    }
                    eprintln!();
                }

                // Parse known fields (assuming same first 40 bytes as DE)
                if sec_start + 40 <= cached.len() {
                    let mat_idx = i32::from_le_bytes([
                        cached[sec_start],
                        cached[sec_start + 1],
                        cached[sec_start + 2],
                        cached[sec_start + 3],
                    ]);
                    let acc_idx = i32::from_le_bytes([
                        cached[sec_start + 4],
                        cached[sec_start + 5],
                        cached[sec_start + 6],
                        cached[sec_start + 7],
                    ]);
                    let max_bones = i32::from_le_bytes([
                        cached[sec_start + 8],
                        cached[sec_start + 9],
                        cached[sec_start + 10],
                        cached[sec_start + 11],
                    ]);
                    let rigid_bone = i32::from_le_bytes([
                        cached[sec_start + 12],
                        cached[sec_start + 13],
                        cached[sec_start + 14],
                        cached[sec_start + 15],
                    ]);
                    let ib_ofs = i32::from_le_bytes([
                        cached[sec_start + 16],
                        cached[sec_start + 17],
                        cached[sec_start + 18],
                        cached[sec_start + 19],
                    ]);
                    let num_tris = i32::from_le_bytes([
                        cached[sec_start + 20],
                        cached[sec_start + 21],
                        cached[sec_start + 22],
                        cached[sec_start + 23],
                    ]);
                    let vb_ofs = i32::from_le_bytes([
                        cached[sec_start + 24],
                        cached[sec_start + 25],
                        cached[sec_start + 26],
                        cached[sec_start + 27],
                    ]);
                    let vb_bytes = i32::from_le_bytes([
                        cached[sec_start + 28],
                        cached[sec_start + 29],
                        cached[sec_start + 30],
                        cached[sec_start + 31],
                    ]);
                    let vert_size = i32::from_le_bytes([
                        cached[sec_start + 32],
                        cached[sec_start + 33],
                        cached[sec_start + 34],
                        cached[sec_start + 35],
                    ]);
                    let num_verts = i32::from_le_bytes([
                        cached[sec_start + 36],
                        cached[sec_start + 37],
                        cached[sec_start + 38],
                        cached[sec_start + 39],
                    ]);
                    eprintln!(
                        "    mat={} acc={} maxBones={} rigidBone={}",
                        mat_idx, acc_idx, max_bones, rigid_bone
                    );
                    eprintln!(
                        "    ibOfs={} numTris={} vbOfs={} vbBytes={}",
                        ib_ofs, num_tris, vb_ofs, vb_bytes
                    );
                    eprintln!("    vertSize={} numVerts={}", vert_size, num_verts);
                }
            }
        }

        // Now try the actual parser
        eprintln!("\n--- Attempting UGX parse ---");
        match UgxGeom::from_bytes(&data) {
            Ok(geom) => {
                eprintln!("SUCCESS!");
                eprintln!("  Sections: {}", geom.sections.len());
                eprintln!("  Materials: {}", geom.materials.len());
                eprintln!("  Bones: {}", geom.bones.len());
            }
            Err(e) => {
                eprintln!("FAILED: {:?}", e);
            }
        }
    }
}
