//! Raw UGX header, section-flag, and LOD-field scanning.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use ecf::Reader as EcfReader;
use ugx::UgxVersion;
use ugx::inspect::{CACHED_DATA_CHUNK_ID, CachedDataView, SectionArray};

pub(crate) fn cmd_scan(dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut ugx_paths: Vec<PathBuf> = Vec::new();
    collect_ugx_files(dir, &mut ugx_paths)?;
    ugx_paths.sort();
    println!("Found {} UGX files in {}", ugx_paths.len(), dir.display());
    if ugx_paths.is_empty() {
        return Ok(());
    }

    let mut stats = ScanStats::default();
    for path in &ugx_paths {
        scan_file(path, dir, &mut stats);
    }
    print_scan_report(&stats);

    Ok(())
}

type FileSection = (String, usize);

#[derive(Default)]
struct ScanStats {
    hw1_count: u32,
    hw2_count: u32,
    parse_errors: u32,
    header_flags: BTreeMap<(u8, u8, u8, u8), Vec<String>>,
    header_padding: BTreeMap<(u16, u32), Vec<String>>,
    hw1_trailing: BTreeMap<(i32, i32, i32), Vec<FileSection>>,
    hw2_non_boolean_rigid: BTreeMap<i32, Vec<FileSection>>,
    hw2_lod_distances: BTreeMap<(u32, u32, u32), Vec<FileSection>>,
}

fn scan_file(path: &Path, root: &Path, stats: &mut ScanStats) {
    let Ok(data) = fs::read(path) else {
        stats.parse_errors += 1;
        return;
    };
    let Ok(ecf) = EcfReader::new(&data) else {
        stats.parse_errors += 1;
        return;
    };
    let Ok(cached) = ecf.chunk_data_by_id(CACHED_DATA_CHUNK_ID) else {
        stats.parse_errors += 1;
        return;
    };
    let Some(view) = CachedDataView::new(&cached) else {
        stats.parse_errors += 1;
        return;
    };
    let Some(sections) = view.section_array() else {
        stats.parse_errors += 1;
        return;
    };
    let filename = path
        .strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string();

    let flags = view.header_flags();
    let padding = view.header_padding();
    stats
        .header_flags
        .entry((
            flags.all_sections_rigid,
            flags.global_bones,
            flags.all_sections_skinned,
            flags.rigid_only,
        ))
        .or_default()
        .push(filename.clone());
    stats
        .header_padding
        .entry((padding.padding, padding.padding2))
        .or_default()
        .push(filename.clone());

    match view.version() {
        Some(UgxVersion::Hw2) => {
            stats.hw2_count += 1;
            scan_hw2_sections(&view, sections, &filename, stats);
        }
        Some(UgxVersion::Hw1) => {
            stats.hw1_count += 1;
            scan_hw1_sections(&view, sections, &filename, stats);
        }
        None => stats.parse_errors += 1,
    }
}

fn scan_hw2_sections(
    view: &CachedDataView<'_>,
    sections: SectionArray,
    filename: &str,
    stats: &mut ScanStats,
) {
    for (section_index, tail) in view.hw2_section_tails(sections).enumerate() {
        if tail.rigid_only != 0 && tail.rigid_only != 1 {
            stats
                .hw2_non_boolean_rigid
                .entry(tail.rigid_only)
                .or_default()
                .push((filename.to_owned(), section_index));
        }
        stats
            .hw2_lod_distances
            .entry((
                tail.lod_near_distance_bits,
                tail.lod_far_distance_bits,
                tail.lod_fade_distance_bits,
            ))
            .or_default()
            .push((filename.to_owned(), section_index));
    }
}

fn scan_hw1_sections(
    view: &CachedDataView<'_>,
    sections: SectionArray,
    filename: &str,
    stats: &mut ScanStats,
) {
    for (section_index, flags) in view.hw1_section_flags(sections).enumerate() {
        let (rigid, global, padding) = (flags.rigid_only, flags.global_bones, flags.padding);
        if rigid != 0 && rigid != 1 || global != 0 && global != 1 || padding != 0 {
            stats
                .hw1_trailing
                .entry((rigid, global, padding))
                .or_default()
                .push((filename.to_owned(), section_index));
        }
    }
}

fn print_scan_report(stats: &ScanStats) {
    println!("\n=== Summary ===");
    println!(
        "HW1 (v4): {} files, HW2 (v6): {} files, errors: {}",
        stats.hw1_count, stats.hw2_count, stats.parse_errors
    );
    print_header_flags(&stats.header_flags);
    print_header_padding(&stats.header_padding);
    print_hw2_lod_distances(&stats.hw2_lod_distances);
    print_hw2_non_boolean_rigid(&stats.hw2_non_boolean_rigid);
    print_hw1_trailing(&stats.hw1_trailing);
}

fn print_header_flags(flags: &BTreeMap<(u8, u8, u8, u8), Vec<String>>) {
    println!("\n=== GeomHeader Flags (+0x36 through +0x39) ===");
    for ((all_rigid, global_bones, all_skinned, rigid_only), files) in flags {
        println!(
            "  all_rigid={all_rigid} global_bones={global_bones} \
             all_skinned={all_skinned} rigid_only={rigid_only}: {} files",
            files.len()
        );
        if [*all_rigid, *global_bones, *all_skinned, *rigid_only]
            .iter()
            .any(|value| *value > 1)
        {
            print_file_examples(files);
        }
    }
}

fn print_header_padding(padding: &BTreeMap<(u16, u32), Vec<String>>) {
    println!("\n=== GeomHeader Padding (+0x3A u16, +0x3C u32) ===");
    for ((padding_2, padding_4), files) in padding {
        println!(
            "  pad1=0x{:04X} pad2=0x{:08X}: {} files",
            padding_2,
            padding_4,
            files.len()
        );
        if *padding_2 != 0 || *padding_4 != 0 {
            print_file_examples(files);
        }
    }
}

fn print_hw2_lod_distances(lod: &BTreeMap<(u32, u32, u32), Vec<FileSection>>) {
    println!("\n=== HW2 Section LOD Distances (+0x2C, +0x30, +0x34) ===");
    for ((near, far, fade), entries) in lod {
        println!(
            "  near={:e} (0x{near:08X}), far={:e} (0x{far:08X}), \
             fade={:e} (0x{fade:08X}): {} sections",
            f32::from_bits(*near),
            f32::from_bits(*far),
            f32::from_bits(*fade),
            entries.len()
        );
    }
}

fn print_hw2_non_boolean_rigid(flags: &BTreeMap<i32, Vec<FileSection>>) {
    if !flags.is_empty() {
        println!("\n=== HW2 Section rigid_only (non-boolean values at +0x28) ===");
        for (rigid_only, entries) in flags {
            println!(
                "  rigid_only={} (0x{:08X}): {} sections",
                rigid_only,
                rigid_only.cast_unsigned(),
                entries.len()
            );
            print_section_examples(entries);
        }
    }
}

fn print_hw1_trailing(trailing: &BTreeMap<(i32, i32, i32), Vec<FileSection>>) {
    if !trailing.is_empty() {
        println!("\n=== HW1 Section Trailing (non-standard rigid/global/padding) ===");
        for ((rigid, global, padding), entries) in trailing {
            println!(
                "  rigid={} global={} padding=0x{:08X}: {} sections",
                rigid,
                global,
                padding.cast_unsigned(),
                entries.len()
            );
            print_section_examples(entries);
        }
    }
}

fn print_file_examples(files: &[String]) {
    for filename in files.iter().take(10) {
        println!("    {filename}");
    }
    if files.len() > 10 {
        println!("    ... and {} more", files.len() - 10);
    }
}

fn print_section_examples(entries: &[FileSection]) {
    for (filename, section) in entries.iter().take(10) {
        println!("    {filename}[sec{section}]");
    }
    if entries.len() > 10 {
        println!("    ... and {} more", entries.len() - 10);
    }
}

fn collect_ugx_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_ugx_files(&path, out)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("ugx") {
            out.push(path);
        }
    }
    Ok(())
}
