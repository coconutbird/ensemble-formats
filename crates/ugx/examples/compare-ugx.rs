//! Compares retail and roundtripped UGX files chunk by chunk.
fn hexdump(data: &[u8], max: usize) {
    for row in 0..max.min(data.len()).div_ceil(16) {
        let off = row * 16;
        if off >= data.len() || off >= max {
            break;
        }
        let end = (off + 16).min(data.len()).min(max);
        let hex: Vec<String> = data[off..end].iter().map(|b| format!("{b:02X}")).collect();
        println!("    {:04X}: {}", off, hex.join(" "));
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or("test_ugx_rebuild/art/game/collectable/skull_01/skull_01.ugx".into());
    let data = std::fs::read(&path)?;

    println!("=== ORIGINAL: {} ({} bytes) ===", path, data.len());
    let ecf_orig = ecf::Reader::new(&data)?;
    for (i, ch) in ecf_orig.chunks().iter().enumerate() {
        println!(
            "  [{i}] id=0x{:X} off=0x{:X} sz={} align={} res=0x{:X}",
            ch.id, ch.offset, ch.size, ch.alignment_log2, ch.resource_flags
        );
    }

    let geom = ugx::Reader::read(&data)?;
    println!(
        "\nGeom: {} sections, {} bones, {} granny_bones, {} materials",
        geom.sections.len(),
        geom.bones.len(),
        geom.granny_bones.len(),
        geom.materials.len()
    );

    let rt_bytes = ugx::Writer::write(&geom, ugx::UgxVersion::Hw1)?;

    println!("\n=== ROUNDTRIPPED ({} bytes) ===", rt_bytes.len());
    let ecf_rt = ecf::Reader::new(&rt_bytes)?;
    for (i, ch) in ecf_rt.chunks().iter().enumerate() {
        println!(
            "  [{i}] id=0x{:X} off=0x{:X} sz={} align={} res=0x{:X}",
            ch.id, ch.offset, ch.size, ch.alignment_log2, ch.resource_flags
        );
    }

    for chunk_id in [0x703u64, 0x700, 0x702, 0x701, 0x704, 0x705] {
        let orig = ecf_orig.chunk_data_by_id(chunk_id);
        let rt = ecf_rt.chunk_data_by_id(chunk_id);
        match (orig, rt) {
            (Ok(o), Ok(r)) => {
                println!(
                    "\n--- Chunk 0x{:X}: orig={} rt={} ---",
                    chunk_id,
                    o.len(),
                    r.len()
                );
                let min_len = o.len().min(r.len());
                let mut diffs = 0usize;
                let mut first_diff = None;
                for i in 0..min_len {
                    if o[i] != r[i] {
                        diffs += 1;
                        if first_diff.is_none() {
                            first_diff = Some(i);
                        }
                    }
                }
                if o.len() != r.len() {
                    diffs += o.len().abs_diff(r.len());
                }
                if diffs == 0 {
                    println!("  IDENTICAL!");
                } else {
                    println!("  {diffs} byte differences");
                    if let Some(d) = first_diff {
                        println!(
                            "  First diff at 0x{:X}: orig=0x{:02X} rt=0x{:02X}",
                            d, o[d], r[d]
                        );
                        let start = d.saturating_sub(16) & !0xF;
                        println!("  ORIGINAL around first diff (from 0x{start:X}):");
                        hexdump(&o[start..], 96);
                        println!("  ROUNDTRIPPED around first diff (from 0x{start:X}):");
                        hexdump(&r[start..], 96);
                    }
                }
            }
            (Ok(_), Err(_)) => println!("\n--- Chunk 0x{chunk_id:X}: MISSING in roundtrip ---"),
            (Err(_), Ok(_)) => println!("\n--- Chunk 0x{chunk_id:X}: NEW in roundtrip ---"),
            (Err(_), Err(_)) => {}
        }
    }
    Ok(())
}
