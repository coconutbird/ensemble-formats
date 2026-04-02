#![allow(clippy::needless_range_loop)]
//! Direct UGX read → write round-trip test (no glTF conversion).
//!
//! Compares **decompressed** ECF chunk data between the original and
//! round-tripped files so compression/encryption differences don't
//! produce false positives.
//!
//! Usage: cargo run -p ugx --example direct_roundtrip -- <file.ugx>

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "launcher_01.ugx".to_string());

    let data = match std::fs::read(&path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Cannot read {path}: {e}");
            std::process::exit(1);
        }
    };
    eprintln!("Original file: {} bytes", data.len());

    let geom = ugx::UgxGeom::from_bytes_unchecked(&data).unwrap();
    eprintln!(
        "Read: {} bones, {} granny_bones, {} sections",
        geom.bones.len(),
        geom.granny_bones.len(),
        geom.sections.len()
    );

    for (i, b) in geom.granny_bones.iter().enumerate() {
        let lt = b
            .local_transform
            .as_ref()
            .map(|t| format!("flags=0x{:X}", t.flags))
            .unwrap_or_else(|| "NONE".into());
        eprintln!("  bone[{}] '{}': lt={} lod={}", i, b.name, lt, b.lod_error);
    }

    let version = if geom
        .sections
        .first()
        .is_some_and(|s| s.base_vert_packer.is_some())
    {
        ugx::UgxVersion::Hw1
    } else {
        ugx::UgxVersion::Hw2
    };
    eprintln!("Version: {:?}", version);

    let rt = ugx::Writer::write(&geom, version).unwrap();
    eprintln!("Written file: {} bytes", rt.len());

    // Show key header fields
    eprintln!("\n=== HEADER FIELDS ===");
    eprintln!("  max_instances: {}", geom.max_instances);
    eprintln!(
        "  instance_index_multiplier: {}",
        geom.instance_index_multiplier
    );
    eprintln!(
        "  index_buffer.len(): {} ({} indices)",
        geom.index_buffer.len() * 2,
        geom.index_buffer.len()
    );
    eprintln!("  accessories: {}", geom.accessories.len());
    eprintln!("  valid_accessories: {}", geom.valid_accessories.len());
    eprintln!("  bone_bounds: {}", geom.bone_bounds.len());
    eprintln!("  rigid_bone_index: {}", geom.rigid_bone_index);
    for (i, s) in geom.sections.iter().enumerate() {
        eprintln!(
            "  section[{i}]: ib_off={} num_tris={} vb_off={} num_verts={} max_bones={} rigid_bone={} rigid_only={} global_bones={}",
            s.ib_offset,
            s.num_tris,
            s.vb_offset,
            s.num_verts,
            s.max_bones,
            s.rigid_bone_index,
            s.rigid_only,
            s.global_bones
        );
    }

    // Compare decompressed chunk data
    let ecf_orig = ecf::Reader::new_unchecked(&data).unwrap();
    let ecf_rt = ecf::Reader::new_unchecked(&rt).unwrap();

    eprintln!("\n=== PER-CHUNK DECOMPRESSED COMPARISON ===");
    let chunk_ids: &[(u64, &str)] = &[
        (0x700, "CachedData"),
        (0x701, "IndexBuffer"),
        (0x702, "VertexBuffer"),
        (0x703, "Granny"),
        (0x704, "Materials"),
        (0x705, "AABBTree"),
    ];

    let mut any_diff = false;
    for &(id, label) in chunk_ids {
        let orig_chunk = ecf_orig.chunk_data_by_id(id);
        let rt_chunk = ecf_rt.chunk_data_by_id(id);

        match (orig_chunk, rt_chunk) {
            (Ok(orig), Ok(rt)) => {
                let min_len = orig.len().min(rt.len());
                let mut diffs = 0usize;
                let mut first_diffs = Vec::new();
                for i in 0..min_len {
                    if orig[i] != rt[i] {
                        diffs += 1;
                        if first_diffs.len() < 10 {
                            first_diffs.push((i, orig[i], rt[i]));
                        }
                    }
                }
                diffs += orig.len().abs_diff(rt.len());

                if diffs == 0 {
                    eprintln!("  0x{:03X} {}: MATCH ({} bytes)", id, label, orig.len());
                } else {
                    any_diff = true;
                    eprintln!(
                        "  0x{:03X} {}: {} diffs (orig={} rt={})",
                        id,
                        label,
                        diffs,
                        orig.len(),
                        rt.len()
                    );
                    for (off, o, r) in &first_diffs {
                        eprintln!("    @0x{:06X}: 0x{:02X} -> 0x{:02X}", off, o, r);
                    }
                }
            }
            (Err(_), Err(_)) => {
                eprintln!("  0x{:03X} {}: not present in either", id, label);
            }
            (Ok(orig), Err(_)) => {
                any_diff = true;
                eprintln!(
                    "  0x{:03X} {}: MISSING in rt (orig={} bytes)",
                    id,
                    label,
                    orig.len()
                );
            }
            (Err(_), Ok(rt)) => {
                any_diff = true;
                eprintln!("  0x{:03X} {}: NEW in rt ({} bytes)", id, label, rt.len());
            }
        }
    }

    if !any_diff {
        eprintln!("\nALL CHUNKS MATCH!");
    }

    // Detailed structural comparison
    eprintln!("\n=== ECF STRUCTURE ===");
    eprintln!("Original: {} chunks", ecf_orig.chunks().len());
    for (i, ch) in ecf_orig.chunks().iter().enumerate() {
        let dec = ecf_orig.chunk_data(i).map(|d| d.len()).unwrap_or(0);
        eprintln!(
            "  [{i}] id=0x{:03X} raw={} dec={} flags=0x{:02X} res=0x{:04X} align={}",
            ch.id, ch.size, dec, ch.flags, ch.resource_flags, ch.alignment_log2
        );
    }
    eprintln!("Roundtrip: {} chunks", ecf_rt.chunks().len());
    for (i, ch) in ecf_rt.chunks().iter().enumerate() {
        let dec = ecf_rt.chunk_data(i).map(|d| d.len()).unwrap_or(0);
        eprintln!(
            "  [{i}] id=0x{:03X} raw={} dec={} flags=0x{:02X} res=0x{:04X} align={}",
            ch.id, ch.size, dec, ch.flags, ch.resource_flags, ch.alignment_log2
        );
    }

    // Index buffer investigation
    if let (Ok(orig_ib), Ok(rt_ib)) = (
        ecf_orig.chunk_data_by_id(0x701),
        ecf_rt.chunk_data_by_id(0x701),
    ) {
        eprintln!("\n=== INDEX BUFFER DETAIL ===");
        eprintln!(
            "Orig: {} bytes ({} u16 indices)",
            orig_ib.len(),
            orig_ib.len() / 2
        );
        eprintln!(
            "RT:   {} bytes ({} u16 indices)",
            rt_ib.len(),
            rt_ib.len() / 2
        );
        // Show first few indices from each
        let show = 20.min(orig_ib.len() / 2);
        eprint!("Orig first {show}: ");
        for i in 0..show {
            let idx = u16::from_le_bytes([orig_ib[i * 2], orig_ib[i * 2 + 1]]);
            eprint!("{idx} ");
        }
        eprintln!();
        let show = 20.min(rt_ib.len() / 2);
        eprint!("RT   first {show}: ");
        for i in 0..show {
            let idx = u16::from_le_bytes([rt_ib[i * 2], rt_ib[i * 2 + 1]]);
            eprint!("{idx} ");
        }
        eprintln!();
    }

    // CachedData header investigation
    if let (Ok(orig_cd), Ok(rt_cd)) = (
        ecf_orig.chunk_data_by_id(0x700),
        ecf_rt.chunk_data_by_id(0x700),
    ) {
        eprintln!("\n=== CACHED DATA (0x700) DETAIL ===");
        eprintln!("Orig: {} bytes", orig_cd.len());
        eprintln!("RT:   {} bytes", rt_cd.len());

        // Parse packed array headers from both
        eprintln!("\n  --- Packed Array Headers (at +0x40) ---");
        let names = [
            "sections",
            "bones",
            "accessories",
            "validAcc",
            "boundsLo",
            "boundsHi",
        ];
        for (i, name) in names.iter().enumerate() {
            let base = 0x40 + i * 16;
            if base + 16 <= orig_cd.len() {
                let o_count = u32::from_le_bytes([
                    orig_cd[base],
                    orig_cd[base + 1],
                    orig_cd[base + 2],
                    orig_cd[base + 3],
                ]);
                let o_pad = u32::from_le_bytes([
                    orig_cd[base + 4],
                    orig_cd[base + 5],
                    orig_cd[base + 6],
                    orig_cd[base + 7],
                ]);
                let o_off = u64::from_le_bytes(orig_cd[base + 8..base + 16].try_into().unwrap());
                eprint!(
                    "  orig {:12}: count={:3} pad=0x{:08X} off=0x{:016X}",
                    name, o_count, o_pad, o_off
                );
            }
            if base + 16 <= rt_cd.len() {
                let r_count = u32::from_le_bytes([
                    rt_cd[base],
                    rt_cd[base + 1],
                    rt_cd[base + 2],
                    rt_cd[base + 3],
                ]);
                let r_pad = u32::from_le_bytes([
                    rt_cd[base + 4],
                    rt_cd[base + 5],
                    rt_cd[base + 6],
                    rt_cd[base + 7],
                ]);
                let r_off = u64::from_le_bytes(rt_cd[base + 8..base + 16].try_into().unwrap());
                eprint!(
                    "  |  rt: count={:3} pad=0x{:08X} off=0x{:016X}",
                    r_count, r_pad, r_off
                );
            }
            eprintln!();
        }

        // Full hex dump showing ALL diff rows
        eprintln!("\n  --- Full hex diff ---");
        let show = orig_cd.len().max(rt_cd.len());
        for row in (0..show).step_by(16) {
            let o_end = (row + 16).min(orig_cd.len());
            let r_end = (row + 16).min(rt_cd.len());
            let mut any_diff_row = false;
            let max_end = o_end.max(r_end);
            for i in row..max_end {
                let o = if i < orig_cd.len() { orig_cd[i] } else { 0 };
                let r = if i < rt_cd.len() { rt_cd[i] } else { 0 };
                if o != r || (i >= orig_cd.len()) != (i >= rt_cd.len()) {
                    any_diff_row = true;
                    break;
                }
            }
            if any_diff_row {
                eprint!("  @{:04X} orig: ", row);
                for i in row..o_end {
                    eprint!("{:02X} ", orig_cd[i]);
                }
                if o_end < row + 16 {
                    eprint!("(end)");
                }
                eprintln!();
                eprint!("  @{:04X} rt:   ", row);
                for i in row..r_end {
                    eprint!("{:02X} ", rt_cd[i]);
                }
                if r_end < row + 16 {
                    eprint!("(end)");
                }
                eprintln!();
            }
        }
    }

    // Granny chunk structural comparison
    if let (Ok(orig_gr), Ok(rt_gr)) = (
        ecf_orig.chunk_data_by_id(0x703),
        ecf_rt.chunk_data_by_id(0x703),
    ) {
        eprintln!("\n=== GRANNY CHUNK (0x703) DETAIL ===");
        eprintln!("Orig: {} bytes", orig_gr.len());
        eprintln!("RT:   {} bytes", rt_gr.len());
        // Dump first 384 bytes
        let show = 384.min(orig_gr.len()).min(rt_gr.len());
        for row in (0..show).step_by(16) {
            let end = (row + 16).min(show);
            let mut any_diff_row = false;
            for i in row..end {
                if orig_gr[i] != rt_gr[i] {
                    any_diff_row = true;
                    break;
                }
            }
            if any_diff_row {
                eprint!("  @{:04X} orig: ", row);
                for i in row..end {
                    eprint!("{:02X} ", orig_gr[i]);
                }
                eprintln!();
                eprint!("  @{:04X} rt:   ", row);
                for i in row..end {
                    eprint!("{:02X} ", rt_gr[i]);
                }
                eprintln!();
            }
        }
    }

    // Materials comparison
    if let (Ok(orig_mat), Ok(rt_mat)) = (
        ecf_orig.chunk_data_by_id(0x704),
        ecf_rt.chunk_data_by_id(0x704),
    ) {
        eprintln!("\n=== MATERIALS (0x704) DETAIL ===");
        eprintln!("Orig: {} bytes", orig_mat.len());
        eprintln!("RT:   {} bytes", rt_mat.len());

        // Dump original tree structure
        fn dump_tree(node: &bdt::Node, indent: usize) {
            let pad = " ".repeat(indent * 2);
            let text_str = if matches!(node.text, bdt::Variant::Null) {
                String::new()
            } else {
                format!(" text={:?}", node.text)
            };
            eprintln!(
                "{}<{}>{}  attrs={}",
                pad,
                node.name,
                text_str,
                node.attributes.len()
            );
            for a in &node.attributes {
                eprintln!("{}  @{}={:?}", pad, a.name, a.value);
            }
            for c in &node.children {
                dump_tree(c, indent + 1);
            }
        }

        eprintln!("\n--- Original material tree ---");
        if let Some(orig_tree) = bdt::Reader::read(&orig_mat, bdt::Endian::Little).unwrap() {
            dump_tree(&orig_tree, 0);
        }
        eprintln!("\n--- Round-tripped material tree ---");
        if let Some(rt_tree) = bdt::Reader::read(&rt_mat, bdt::Endian::Little).unwrap() {
            dump_tree(&rt_tree, 0);
        }

        // Just show first 64 bytes of diffs
        let show = 64.min(orig_mat.len()).min(rt_mat.len());
        for row in (0..show).step_by(16) {
            let end = (row + 16).min(show);
            let mut any_diff_row = false;
            for i in row..end {
                if orig_mat[i] != rt_mat[i] {
                    any_diff_row = true;
                    break;
                }
            }
            if any_diff_row {
                eprint!("  @{:04X} orig: ", row);
                for i in row..end {
                    eprint!("{:02X} ", orig_mat[i]);
                }
                eprintln!();
                eprint!("  @{:04X} rt:   ", row);
                for i in row..end {
                    eprint!("{:02X} ", rt_mat[i]);
                }
                eprintln!();
            }
        }
    }

    // ---- Re-read verification: parse RT output and compare structures ----
    eprintln!("\n=== RE-READ VERIFICATION ===");
    let rt_geom = ugx::Reader::read(&rt).unwrap();
    eprintln!(
        "RT re-read: {} bones, {} granny_bones, {} sections, {} meshes",
        rt_geom.bones.len(),
        rt_geom.granny_bones.len(),
        rt_geom.sections.len(),
        rt_geom.granny_meshes.len(),
    );
    let mut reread_ok = true;

    // Compare bone counts
    if geom.granny_bones.len() != rt_geom.granny_bones.len() {
        eprintln!(
            "  FAIL: bone count {} vs {}",
            geom.granny_bones.len(),
            rt_geom.granny_bones.len()
        );
        reread_ok = false;
    }

    // Compare bone names and transforms
    for (i, (orig, rt)) in geom
        .granny_bones
        .iter()
        .zip(rt_geom.granny_bones.iter())
        .enumerate()
    {
        if orig.name != rt.name {
            eprintln!("  FAIL: bone[{i}] name '{}' vs '{}'", orig.name, rt.name);
            reread_ok = false;
        }
        if orig.parent_index != rt.parent_index {
            eprintln!(
                "  FAIL: bone[{i}] parent {} vs {}",
                orig.parent_index, rt.parent_index
            );
            reread_ok = false;
        }
        // Compare local transform
        match (&orig.local_transform, &rt.local_transform) {
            (Some(a), Some(b)) => {
                if a.flags != b.flags {
                    eprintln!(
                        "  FAIL: bone[{i}] lt flags 0x{:X} vs 0x{:X}",
                        a.flags, b.flags
                    );
                    reread_ok = false;
                }
            }
            (None, None) => {}
            _ => {
                eprintln!("  FAIL: bone[{i}] transform presence mismatch");
                reread_ok = false;
            }
        }
        // Compare extended data presence
        let orig_has_ext = orig.extended_data.is_some();
        let rt_has_ext = rt.extended_data.is_some();
        if orig_has_ext != rt_has_ext {
            eprintln!("  FAIL: bone[{i}] extended_data: orig={orig_has_ext} rt={rt_has_ext}");
            reread_ok = false;
        }
    }

    // Compare mesh counts and bone bindings
    if geom.granny_meshes.len() != rt_geom.granny_meshes.len() {
        eprintln!(
            "  FAIL: mesh count {} vs {}",
            geom.granny_meshes.len(),
            rt_geom.granny_meshes.len()
        );
        reread_ok = false;
    }
    for (i, (orig, rt)) in geom
        .granny_meshes
        .iter()
        .zip(rt_geom.granny_meshes.iter())
        .enumerate()
    {
        if orig.name != rt.name {
            eprintln!("  FAIL: mesh[{i}] name '{}' vs '{}'", orig.name, rt.name);
            reread_ok = false;
        }
        if orig.bone_bindings.len() != rt.bone_bindings.len() {
            eprintln!(
                "  FAIL: mesh[{i}] bone_binding count {} vs {}",
                orig.bone_bindings.len(),
                rt.bone_bindings.len()
            );
            reread_ok = false;
        } else {
            for (j, (ob, rb)) in orig
                .bone_bindings
                .iter()
                .zip(rt.bone_bindings.iter())
                .enumerate()
            {
                if ob.bone_name != rb.bone_name {
                    eprintln!(
                        "  FAIL: mesh[{i}] bb[{j}] name '{}' vs '{}'",
                        ob.bone_name, rb.bone_name
                    );
                    reread_ok = false;
                }
                if (ob.obb_min != rb.obb_min) || (ob.obb_max != rb.obb_max) {
                    eprintln!("  FAIL: mesh[{i}] bb[{j}] OBB mismatch");
                    reread_ok = false;
                }
            }
        }
    }

    if reread_ok {
        eprintln!("  ✅ RE-READ VERIFICATION PASSED — all structures match");
    } else {
        eprintln!("  ❌ RE-READ VERIFICATION FAILED");
        std::process::exit(1);
    }
}
