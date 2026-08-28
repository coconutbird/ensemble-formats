//! Raw UGX reserved-field scanning.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use ecf::Reader as EcfReader;

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
    hw2_reserved: BTreeMap<(u32, u32), Vec<FileSection>>,
    header_padding: BTreeMap<(u16, u32), Vec<String>>,
    hw1_trailing: BTreeMap<(i32, i32, i32), Vec<FileSection>>,
    hw2_flags: BTreeMap<(i32, i32), Vec<FileSection>>,
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
    let Ok(cached) = ecf.chunk_data_by_id(0x700) else {
        stats.parse_errors += 1;
        return;
    };
    if cached.len() < 0x50 {
        stats.parse_errors += 1;
        return;
    }

    let Some(signature) = read_u32_at(&cached, 0) else {
        stats.parse_errors += 1;
        return;
    };
    let Some(section_count) =
        read_u32_at(&cached, 0x40).and_then(|count| usize::try_from(count).ok())
    else {
        stats.parse_errors += 1;
        return;
    };
    let Some(section_offset) =
        read_u64_at(&cached, 0x48).and_then(|offset| usize::try_from(offset).ok())
    else {
        stats.parse_errors += 1;
        return;
    };
    let filename = path
        .strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string();

    let Some(padding_2) = read_u16_at(&cached, 0x38) else {
        stats.parse_errors += 1;
        return;
    };
    let Some(padding_4) = read_u32_at(&cached, 0x3C) else {
        stats.parse_errors += 1;
        return;
    };
    stats
        .header_padding
        .entry((padding_2, padding_4))
        .or_default()
        .push(filename.clone());

    match signature {
        0xC234_0006 => {
            stats.hw2_count += 1;
            scan_hw2_sections(&cached, section_count, section_offset, &filename, stats);
        }
        0xC234_0004 => {
            stats.hw1_count += 1;
            scan_hw1_sections(&cached, section_count, section_offset, &filename, stats);
        }
        _ => stats.parse_errors += 1,
    }
}

fn scan_hw2_sections(
    data: &[u8],
    section_count: usize,
    section_offset: usize,
    filename: &str,
    stats: &mut ScanStats,
) {
    for section_index in 0..section_count {
        let Some(section) = section_at(data, section_offset, section_index, 72) else {
            break;
        };
        let (Some(flag_1), Some(flag_2)) = (read_i32_at(section, 0x28), read_i32_at(section, 0x2C))
        else {
            break;
        };
        if flag_1 != 0 && flag_1 != 1 || flag_2 != 0 && flag_2 != 1 {
            stats
                .hw2_flags
                .entry((flag_1, flag_2))
                .or_default()
                .push((filename.to_owned(), section_index));
        }

        let (Some(reserved_1), Some(reserved_2)) =
            (read_u32_at(section, 0x30), read_u32_at(section, 0x34))
        else {
            break;
        };
        stats
            .hw2_reserved
            .entry((reserved_1, reserved_2))
            .or_default()
            .push((filename.to_owned(), section_index));
    }
}

fn scan_hw1_sections(
    data: &[u8],
    section_count: usize,
    section_offset: usize,
    filename: &str,
    stats: &mut ScanStats,
) {
    for section_index in 0..section_count {
        let Some(section) = section_at(data, section_offset, section_index, 152) else {
            break;
        };
        let (Some(rigid), Some(global), Some(padding)) = (
            read_i32_at(section, 0x8C),
            read_i32_at(section, 0x90),
            read_i32_at(section, 0x94),
        ) else {
            break;
        };
        if rigid != 0 && rigid != 1 || global != 0 && global != 1 || padding != 0 {
            stats
                .hw1_trailing
                .entry((rigid, global, padding))
                .or_default()
                .push((filename.to_owned(), section_index));
        }
    }
}

fn section_at(
    data: &[u8],
    array_offset: usize,
    index: usize,
    section_size: usize,
) -> Option<&[u8]> {
    let relative_offset = index.checked_mul(section_size)?;
    let start = array_offset.checked_add(relative_offset)?;
    let end = start.checked_add(section_size)?;
    data.get(start..end)
}

fn read_u16_at(data: &[u8], offset: usize) -> Option<u16> {
    let end = offset.checked_add(2)?;
    Some(u16::from_le_bytes(data.get(offset..end)?.try_into().ok()?))
}

fn read_u32_at(data: &[u8], offset: usize) -> Option<u32> {
    let end = offset.checked_add(4)?;
    Some(u32::from_le_bytes(data.get(offset..end)?.try_into().ok()?))
}

fn read_i32_at(data: &[u8], offset: usize) -> Option<i32> {
    let end = offset.checked_add(4)?;
    Some(i32::from_le_bytes(data.get(offset..end)?.try_into().ok()?))
}

fn read_u64_at(data: &[u8], offset: usize) -> Option<u64> {
    let end = offset.checked_add(8)?;
    Some(u64::from_le_bytes(data.get(offset..end)?.try_into().ok()?))
}

fn print_scan_report(stats: &ScanStats) {
    println!("\n=== Summary ===");
    println!(
        "HW1 (v4): {} files, HW2 (v6): {} files, errors: {}",
        stats.hw1_count, stats.hw2_count, stats.parse_errors
    );
    print_header_padding(&stats.header_padding);
    print_hw2_reserved(&stats.hw2_reserved);
    print_hw2_flags(&stats.hw2_flags);
    print_hw1_trailing(&stats.hw1_trailing);
}

fn print_header_padding(padding: &BTreeMap<(u16, u32), Vec<String>>) {
    println!("\n=== GeomHeader Padding (+0x38 u16, +0x3C u32) ===");
    for ((padding_2, padding_4), files) in padding {
        println!(
            "  pad1=0x{:04X} pad2=0x{:08X}: {} files",
            padding_2,
            padding_4,
            files.len()
        );
        if *padding_2 != 0 || *padding_4 != 0 {
            for filename in files.iter().take(10) {
                println!("    {filename}");
            }
            if files.len() > 10 {
                println!("    ... and {} more", files.len() - 10);
            }
        }
    }
}

fn print_hw2_reserved(reserved: &BTreeMap<(u32, u32), Vec<FileSection>>) {
    println!("\n=== HW2 Section Reserved Fields (+0x30, +0x34) ===");
    for ((reserved_1, reserved_2), entries) in reserved {
        let reserved_float = f32::from_bits(*reserved_1);
        println!(
            "  reserved1=0x{:08X} ({:e}), reserved2=0x{:08X}: {} sections",
            reserved_1,
            reserved_float,
            reserved_2,
            entries.len()
        );
        if *reserved_1 != 0x7F7F_FFFF || *reserved_2 != 0 {
            for (filename, section) in entries.iter().take(10) {
                println!("    {filename}[sec{section}]");
            }
            if entries.len() > 10 {
                println!("    ... and {} more", entries.len() - 10);
            }
        }
    }
}

fn print_hw2_flags(flags: &BTreeMap<(i32, i32), Vec<FileSection>>) {
    if !flags.is_empty() {
        println!("\n=== HW2 Section Flags (non-boolean values at +0x28, +0x2C) ===");
        for ((flag_1, flag_2), entries) in flags {
            println!(
                "  flags1={} (0x{:08X}), flags2={} (0x{:08X}): {} sections",
                flag_1,
                flag_1.cast_unsigned(),
                flag_2,
                flag_2.cast_unsigned(),
                entries.len()
            );
            for (filename, section) in entries.iter().take(10) {
                println!("    {filename}[sec{section}]");
            }
            if entries.len() > 10 {
                println!("    ... and {} more", entries.len() - 10);
            }
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
            for (filename, section) in entries.iter().take(10) {
                println!("    {filename}[sec{section}]");
            }
            if entries.len() > 10 {
                println!("    ... and {} more", entries.len() - 10);
            }
        }
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
