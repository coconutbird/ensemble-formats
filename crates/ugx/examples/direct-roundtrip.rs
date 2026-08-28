//! Exercises a direct UGX read → write roundtrip without glTF conversion.
//!
//! Compares decompressed ECF chunk data between the original and
//! round-tripped files so compression and encryption differences do not
//! produce false positives.
//!
//! Usage: cargo run -p ugx --example `direct-roundtrip` -- <file.ugx>

const CHUNKS: &[(u64, &str)] = &[
    (0x700, "CachedData"),
    (0x701, "IndexBuffer"),
    (0x702, "VertexBuffer"),
    (0x703, "Granny"),
    (0x704, "Materials"),
    (0x705, "AABBTree"),
];

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "launcher_01.ugx".to_string());
    let data = std::fs::read(&path).unwrap_or_else(|error| {
        eprintln!("Cannot read {path}: {error}");
        std::process::exit(1);
    });
    eprintln!("Original file: {} bytes", data.len());

    let geom = ugx::UgxGeom::from_bytes_unchecked(&data).unwrap();
    print_geometry_summary(&geom);
    let version = detect_version(&geom);
    eprintln!("Version: {version:?}");

    let roundtrip = ugx::Writer::write(&geom, version).unwrap();
    eprintln!("Written file: {} bytes", roundtrip.len());

    if !compare_chunks(&data, &roundtrip) {
        eprintln!("\nALL CHUNKS MATCH!");
    }
    print_ecf_structure(&data, &roundtrip);
    compare_index_buffers(&data, &roundtrip);
    compare_cached_data(&data, &roundtrip);
    compare_granny_data(&data, &roundtrip);
    compare_materials(&data, &roundtrip);

    if !verify_reread(&geom, &roundtrip) {
        std::process::exit(1);
    }
}

fn detect_version(geom: &ugx::UgxGeom) -> ugx::UgxVersion {
    if geom
        .sections
        .first()
        .is_some_and(|section| section.base_vert_packer.is_some())
    {
        ugx::UgxVersion::Hw1
    } else {
        ugx::UgxVersion::Hw2
    }
}

fn print_geometry_summary(geom: &ugx::UgxGeom) {
    eprintln!(
        "Read: {} bones, {} granny_bones, {} sections",
        geom.bones.len(),
        geom.granny_bones.len(),
        geom.sections.len()
    );
    for (index, bone) in geom.granny_bones.iter().enumerate() {
        let transform = bone.local_transform.as_ref().map_or_else(
            || "NONE".to_string(),
            |value| format!("flags=0x{:X}", value.flags),
        );
        eprintln!(
            "  bone[{index}] '{}': lt={transform} lod={}",
            bone.name, bone.lod_error
        );
    }

    eprintln!("\n=== HEADER FIELDS ===");
    eprintln!("  max_instances: {}", geom.max_instances);
    eprintln!(
        "  instance_index_multiplier: {}",
        geom.instance_index_multiplier
    );
    eprintln!("  index_buffer: {} indices", geom.index_buffer.len());
    eprintln!("  accessories: {}", geom.accessories.len());
    eprintln!("  valid_accessories: {}", geom.valid_accessories.len());
    eprintln!("  bone_bounds: {}", geom.bone_bounds.len());
    eprintln!("  rigid_bone_index: {}", geom.rigid_bone_index);
    for (index, section) in geom.sections.iter().enumerate() {
        eprintln!(
            "  section[{index}]: ib_off={} num_tris={} vb_off={} num_verts={} max_bones={} rigid_bone={} rigid_only={} global_bones={}",
            section.ib_offset,
            section.num_tris,
            section.vb_offset,
            section.num_verts,
            section.max_bones,
            section.rigid_bone_index,
            section.rigid_only,
            section.global_bones
        );
    }
}

fn compare_chunks(original_file: &[u8], roundtrip_file: &[u8]) -> bool {
    let original = ecf::Reader::new_unchecked(original_file).unwrap();
    let roundtrip = ecf::Reader::new_unchecked(roundtrip_file).unwrap();
    eprintln!("\n=== PER-CHUNK DECOMPRESSED COMPARISON ===");
    let mut any_difference = false;

    for &(id, label) in CHUNKS {
        match (
            original.chunk_data_by_id(id),
            roundtrip.chunk_data_by_id(id),
        ) {
            (Ok(source), Ok(rebuilt)) => {
                let differences: Vec<_> = source
                    .iter()
                    .zip(rebuilt.iter())
                    .enumerate()
                    .filter_map(|(offset, (&left, &right))| {
                        (left != right).then_some((offset, left, right))
                    })
                    .collect();
                let difference_count = differences.len() + source.len().abs_diff(rebuilt.len());
                if difference_count == 0 {
                    eprintln!("  0x{id:03X} {label}: MATCH ({} bytes)", source.len());
                } else {
                    any_difference = true;
                    eprintln!(
                        "  0x{id:03X} {label}: {difference_count} diffs (orig={} rt={})",
                        source.len(),
                        rebuilt.len()
                    );
                    for &(offset, left, right) in differences.iter().take(10) {
                        eprintln!("    @0x{offset:06X}: 0x{left:02X} -> 0x{right:02X}");
                    }
                }
            }
            (Err(_), Err(_)) => eprintln!("  0x{id:03X} {label}: not present in either"),
            (Ok(source), Err(_)) => {
                any_difference = true;
                eprintln!(
                    "  0x{id:03X} {label}: MISSING in rt (orig={} bytes)",
                    source.len()
                );
            }
            (Err(_), Ok(rebuilt)) => {
                any_difference = true;
                eprintln!("  0x{id:03X} {label}: NEW in rt ({} bytes)", rebuilt.len());
            }
        }
    }
    any_difference
}

fn print_ecf_structure(original_file: &[u8], roundtrip_file: &[u8]) {
    eprintln!("\n=== ECF STRUCTURE ===");
    print_chunks("Original", original_file);
    print_chunks("Roundtrip", roundtrip_file);
}

fn print_chunks(label: &str, data: &[u8]) {
    let reader = ecf::Reader::new_unchecked(data).unwrap();
    eprintln!("{label}: {} chunks", reader.chunks().len());
    for (index, chunk) in reader.chunks().iter().enumerate() {
        let decompressed_size = reader.chunk_data(index).map_or(0, |bytes| bytes.len());
        eprintln!(
            "  [{index}] id=0x{:03X} raw={} dec={decompressed_size} flags=0x{:02X} res=0x{:04X} align={}",
            chunk.id, chunk.size, chunk.flags, chunk.resource_flags, chunk.alignment_log2
        );
    }
}

fn compare_index_buffers(original_file: &[u8], roundtrip_file: &[u8]) {
    let original = ecf::Reader::new_unchecked(original_file).unwrap();
    let roundtrip = ecf::Reader::new_unchecked(roundtrip_file).unwrap();
    let (Ok(source), Ok(rebuilt)) = (
        original.chunk_data_by_id(0x701),
        roundtrip.chunk_data_by_id(0x701),
    ) else {
        return;
    };

    eprintln!("\n=== INDEX BUFFER DETAIL ===");
    print_indices("Orig", &source);
    print_indices("RT", &rebuilt);
}

fn print_indices(label: &str, data: &[u8]) {
    eprintln!(
        "{label}: {} bytes ({} u16 indices)",
        data.len(),
        data.len() / 2
    );
    eprint!("{label} first {}: ", 20.min(data.len() / 2));
    for bytes in data.as_chunks::<2>().0.iter().take(20) {
        eprint!("{} ", u16::from_le_bytes([bytes[0], bytes[1]]));
    }
    eprintln!();
}

fn compare_cached_data(original_file: &[u8], roundtrip_file: &[u8]) {
    let original = ecf::Reader::new_unchecked(original_file).unwrap();
    let roundtrip = ecf::Reader::new_unchecked(roundtrip_file).unwrap();
    let (Ok(source), Ok(rebuilt)) = (
        original.chunk_data_by_id(0x700),
        roundtrip.chunk_data_by_id(0x700),
    ) else {
        return;
    };

    eprintln!("\n=== CACHED DATA (0x700) DETAIL ===");
    eprintln!("Orig: {} bytes", source.len());
    eprintln!("RT:   {} bytes", rebuilt.len());
    print_packed_array_headers(&source, &rebuilt);
    print_bone_name_layout(&source);
    eprintln!("\n  --- Full hex diff ---");
    print_hex_differences(&source, &rebuilt, source.len().max(rebuilt.len()));
}

fn print_packed_array_headers(original: &[u8], roundtrip: &[u8]) {
    const NAMES: [&str; 6] = [
        "sections",
        "bones",
        "accessories",
        "validAcc",
        "boundsLo",
        "boundsHi",
    ];
    eprintln!("\n  --- Packed Array Headers (at +0x40) ---");
    for (index, name) in NAMES.iter().enumerate() {
        let base = 0x40 + index * 16;
        if let (Some(count), Some(padding), Some(offset)) = (
            read_u32(original, base),
            read_u32(original, base + 4),
            read_u64(original, base + 8),
        ) {
            eprint!("  orig {name:12}: count={count:3} pad=0x{padding:08X} off=0x{offset:016X}");
        }
        if let (Some(count), Some(padding), Some(offset)) = (
            read_u32(roundtrip, base),
            read_u32(roundtrip, base + 4),
            read_u64(roundtrip, base + 8),
        ) {
            eprint!("  |  rt: count={count:3} pad=0x{padding:08X} off=0x{offset:016X}");
        }
        eprintln!();
    }
}

fn print_bone_name_layout(data: &[u8]) {
    let Some(bone_count) = read_u32(data, 0x50).and_then(|value| usize::try_from(value).ok())
    else {
        return;
    };
    let Some(bone_offset) = read_offset(data, 0x58) else {
        return;
    };
    let Some(records) = data.get(bone_offset..) else {
        return;
    };

    eprintln!("\n  --- Bone name layout (orig): {bone_count} bones at 0x{bone_offset:X} ---");
    let mut previous_name_offset = None;
    for (index, record) in records
        .as_chunks::<80>()
        .0
        .iter()
        .take(bone_count)
        .enumerate()
    {
        let Some(name_offset) = read_offset(record, 0) else {
            break;
        };
        let name = read_c_string(data, name_offset).unwrap_or_else(|| "???".to_string());
        let slot = previous_name_offset
            .and_then(|previous| name_offset.checked_sub(previous))
            .map_or_else(String::new, |size| format!("prev_slot={size}"));
        eprintln!(
            "    bone[{index:2}] name_off=0x{name_offset:04X} len={:2} '{name}' {slot}",
            name.len() + 1
        );
        previous_name_offset = Some(name_offset);
    }
    print_accessory_gap(data, previous_name_offset);
}

fn print_accessory_gap(data: &[u8], last_name_offset: Option<usize>) {
    let Some(last_name_offset) = last_name_offset else {
        return;
    };
    let Some(accessory_offset) = read_offset(data, 0x68) else {
        return;
    };
    let Some(name_tail) = data.get(last_name_offset..) else {
        return;
    };
    let last_name_end = name_tail
        .iter()
        .position(|&byte| byte == 0)
        .and_then(|length| last_name_offset.checked_add(length + 1))
        .unwrap_or(last_name_offset);
    let gap = accessory_offset.saturating_sub(last_name_end);
    eprintln!(
        "    last_name_end=0x{last_name_end:04X} acc_start=0x{accessory_offset:04X} gap={gap}"
    );
}

fn read_u32(data: &[u8], offset: usize) -> Option<u32> {
    let end = offset.checked_add(4)?;
    Some(u32::from_le_bytes(data.get(offset..end)?.try_into().ok()?))
}

fn read_u64(data: &[u8], offset: usize) -> Option<u64> {
    let end = offset.checked_add(8)?;
    Some(u64::from_le_bytes(data.get(offset..end)?.try_into().ok()?))
}

fn read_offset(data: &[u8], offset: usize) -> Option<usize> {
    usize::try_from(read_u64(data, offset)?).ok()
}

fn read_c_string(data: &[u8], offset: usize) -> Option<String> {
    let tail = data.get(offset..)?;
    let length = tail.iter().position(|&byte| byte == 0)?;
    Some(String::from_utf8_lossy(&tail[..length]).into_owned())
}

fn print_hex_differences(original: &[u8], roundtrip: &[u8], limit: usize) {
    for row in (0..limit).step_by(16) {
        let end = row.saturating_add(16).min(limit);
        let source = original.get(row..end.min(original.len())).unwrap_or(&[]);
        let rebuilt = roundtrip.get(row..end.min(roundtrip.len())).unwrap_or(&[]);
        if source != rebuilt {
            print_hex_row(row, "orig", source);
            print_hex_row(row, "rt:  ", rebuilt);
        }
    }
}

fn print_hex_row(offset: usize, label: &str, data: &[u8]) {
    eprint!("  @{offset:04X} {label}: ");
    for byte in data {
        eprint!("{byte:02X} ");
    }
    if data.len() < 16 {
        eprint!("(end)");
    }
    eprintln!();
}

fn compare_granny_data(original_file: &[u8], roundtrip_file: &[u8]) {
    let original = ecf::Reader::new_unchecked(original_file).unwrap();
    let roundtrip = ecf::Reader::new_unchecked(roundtrip_file).unwrap();
    let (Ok(source), Ok(rebuilt)) = (
        original.chunk_data_by_id(0x703),
        roundtrip.chunk_data_by_id(0x703),
    ) else {
        return;
    };
    eprintln!("\n=== GRANNY CHUNK (0x703) DETAIL ===");
    eprintln!("Orig: {} bytes", source.len());
    eprintln!("RT:   {} bytes", rebuilt.len());
    let limit = 384.min(source.len()).min(rebuilt.len());
    print_hex_differences(&source, &rebuilt, limit);
}

fn compare_materials(original_file: &[u8], roundtrip_file: &[u8]) {
    let original = ecf::Reader::new_unchecked(original_file).unwrap();
    let roundtrip = ecf::Reader::new_unchecked(roundtrip_file).unwrap();
    let (Ok(source), Ok(rebuilt)) = (
        original.chunk_data_by_id(0x704),
        roundtrip.chunk_data_by_id(0x704),
    ) else {
        return;
    };
    eprintln!("\n=== MATERIALS (0x704) DETAIL ===");
    eprintln!("Orig: {} bytes", source.len());
    eprintln!("RT:   {} bytes", rebuilt.len());
    eprintln!("\n--- Original material tree ---");
    if let Some(tree) = bdt::Reader::read(&source, bdt::Endian::Little).unwrap() {
        dump_tree(&tree, 0);
    }
    eprintln!("\n--- Round-tripped material tree ---");
    if let Some(tree) = bdt::Reader::read(&rebuilt, bdt::Endian::Little).unwrap() {
        dump_tree(&tree, 0);
    }
    print_hex_differences(&source, &rebuilt, 64.min(source.len()).min(rebuilt.len()));
}

fn dump_tree(node: &bdt::Node, indent: usize) {
    let padding = " ".repeat(indent * 2);
    let text = if matches!(node.text, bdt::Variant::Null) {
        String::new()
    } else {
        format!(" text={:?}", node.text)
    };
    eprintln!(
        "{}<{}>{}  attrs={}",
        padding,
        node.name,
        text,
        node.attributes.len()
    );
    for attribute in &node.attributes {
        eprintln!("{}  @{}={:?}", padding, attribute.name, attribute.value);
    }
    for child in &node.children {
        dump_tree(child, indent + 1);
    }
}

fn verify_reread(original: &ugx::UgxGeom, roundtrip_file: &[u8]) -> bool {
    eprintln!("\n=== RE-READ VERIFICATION ===");
    let rebuilt = ugx::Reader::read(roundtrip_file).unwrap();
    eprintln!(
        "RT re-read: {} bones, {} granny_bones, {} sections, {} meshes",
        rebuilt.bones.len(),
        rebuilt.granny_bones.len(),
        rebuilt.sections.len(),
        rebuilt.granny_meshes.len(),
    );
    let bones_match = verify_bones(&original.granny_bones, &rebuilt.granny_bones);
    let meshes_match = verify_meshes(&original.granny_meshes, &rebuilt.granny_meshes);
    if bones_match && meshes_match {
        eprintln!("  RE-READ VERIFICATION PASSED — all structures match");
        true
    } else {
        eprintln!("  RE-READ VERIFICATION FAILED");
        false
    }
}

fn verify_bones(original: &[ugx::GrannyBone], rebuilt: &[ugx::GrannyBone]) -> bool {
    let mut matches = true;
    if original.len() != rebuilt.len() {
        eprintln!("  FAIL: bone count {} vs {}", original.len(), rebuilt.len());
        matches = false;
    }
    for (index, (source, reread)) in original.iter().zip(rebuilt).enumerate() {
        if source.name != reread.name {
            eprintln!(
                "  FAIL: bone[{index}] name '{}' vs '{}'",
                source.name, reread.name
            );
            matches = false;
        }
        if source.parent_index != reread.parent_index {
            eprintln!(
                "  FAIL: bone[{index}] parent {} vs {}",
                source.parent_index, reread.parent_index
            );
            matches = false;
        }
        match (&source.local_transform, &reread.local_transform) {
            (Some(left), Some(right)) if left.flags != right.flags => {
                eprintln!(
                    "  FAIL: bone[{index}] lt flags 0x{:X} vs 0x{:X}",
                    left.flags, right.flags
                );
                matches = false;
            }
            (Some(_), None) | (None, Some(_)) => {
                eprintln!("  FAIL: bone[{index}] transform presence mismatch");
                matches = false;
            }
            _ => {}
        }
        if source.extended_data.is_some() != reread.extended_data.is_some() {
            eprintln!(
                "  FAIL: bone[{index}] extended_data: orig={} rt={}",
                source.extended_data.is_some(),
                reread.extended_data.is_some()
            );
            matches = false;
        }
    }
    matches
}

fn verify_meshes(original: &[ugx::GrannyMesh], rebuilt: &[ugx::GrannyMesh]) -> bool {
    let mut matches = true;
    if original.len() != rebuilt.len() {
        eprintln!("  FAIL: mesh count {} vs {}", original.len(), rebuilt.len());
        matches = false;
    }
    for (mesh_index, (source, reread)) in original.iter().zip(rebuilt).enumerate() {
        if source.name != reread.name {
            eprintln!(
                "  FAIL: mesh[{mesh_index}] name '{}' vs '{}'",
                source.name, reread.name
            );
            matches = false;
        }
        if source.bone_bindings.len() != reread.bone_bindings.len() {
            eprintln!(
                "  FAIL: mesh[{mesh_index}] bone_binding count {} vs {}",
                source.bone_bindings.len(),
                reread.bone_bindings.len()
            );
            matches = false;
            continue;
        }
        for (binding_index, (left, right)) in source
            .bone_bindings
            .iter()
            .zip(&reread.bone_bindings)
            .enumerate()
        {
            if left.bone_name != right.bone_name {
                eprintln!(
                    "  FAIL: mesh[{mesh_index}] bb[{binding_index}] name '{}' vs '{}'",
                    left.bone_name, right.bone_name
                );
                matches = false;
            }
            if !vec3_nearly_equal(&left.obb_min, &right.obb_min)
                || !vec3_nearly_equal(&left.obb_max, &right.obb_max)
            {
                eprintln!("  FAIL: mesh[{mesh_index}] bb[{binding_index}] OBB mismatch");
                matches = false;
            }
        }
    }
    matches
}

fn vec3_nearly_equal(left: &[f32; 3], right: &[f32; 3]) -> bool {
    left.iter()
        .zip(right)
        .all(|(left_value, right_value)| (left_value - right_value).abs() <= f32::EPSILON)
}
