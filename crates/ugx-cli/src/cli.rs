//! UGX CLI implementation.

use clap::{Parser, Subcommand};
use ecf::Reader as EcfReader;
use std::cmp::Ordering;
use std::fs;
use std::path::PathBuf;
use ugx::{Reader as UgxReader, UgxVersion, Writer as UgxWriter};
use ugx_gltf::{
    GltfExportOptions, GltfImportOptions, export_to_gltf_with_buffer_name, import_from_gltf,
};

#[derive(Parser)]
#[command(name = "ugx")]
#[command(about = "UGX (Unit Graphics) model file tool for Halo Wars")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Show information about a UGX file
    Info {
        /// Input UGX file
        #[arg(short, long)]
        input: PathBuf,
        /// Skip ECF checksum validation
        #[arg(long)]
        no_verify: bool,
    },
    /// Convert UGX to glTF format
    ToGltf {
        /// Input UGX file
        #[arg(short, long)]
        input: PathBuf,
        /// Output glTF file
        #[arg(short, long)]
        output: PathBuf,
        /// Create separate .bin file instead of embedding data
        #[arg(long)]
        external_buffer: bool,
        /// Exclude skeleton/bones from the export
        #[arg(long)]
        no_skeleton: bool,
        /// Skip ECF checksum validation
        #[arg(long)]
        no_verify: bool,
    },
    /// Convert glTF/GLB to UGX format
    FromGltf {
        /// Input glTF or GLB file
        #[arg(short, long)]
        input: PathBuf,
        /// Output UGX file
        #[arg(short, long)]
        output: PathBuf,
        /// Exclude skeleton/bones from the import
        #[arg(long)]
        no_skeleton: bool,
        /// Target game version: "hw1" for Halo Wars DE (v4) or "hw2" for Halo Wars 2 (v6)
        #[arg(long, default_value = "hw2")]
        version: String,
    },
    /// Dump ECF structure for debugging
    Dump {
        /// Input UGX file
        #[arg(short, long)]
        input: PathBuf,
    },
    /// Binary diff two UGX files chunk-by-chunk
    Diff {
        /// First (original) UGX file
        #[arg(short = 'a', long)]
        original: PathBuf,
        /// Second (round-tripped) UGX file
        #[arg(short = 'b', long)]
        roundtrip: PathBuf,
    },
    /// Scan a directory of UGX files and report raw reserved/padding field values
    Scan {
        /// Directory containing UGX files (searched recursively)
        #[arg(short, long)]
        dir: PathBuf,
    },
}

pub(crate) fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Info { input, no_verify } => crate::info::cmd_info(&input, no_verify)?,
        Commands::ToGltf {
            input,
            output,
            external_buffer,
            no_skeleton,
            no_verify,
        } => cmd_to_gltf(&input, &output, external_buffer, no_skeleton, no_verify)?,
        Commands::FromGltf {
            input,
            output,
            no_skeleton,
            version,
        } => {
            let ugx_version = match version.to_lowercase().as_str() {
                "hw1" | "de" | "v4" => UgxVersion::Hw1,
                "hw2" | "v6" => UgxVersion::Hw2,
                other => {
                    eprintln!("Unknown version '{other}', expected 'hw1' or 'hw2'");
                    std::process::exit(1);
                }
            };
            cmd_from_gltf(&input, &output, no_skeleton, ugx_version)?;
        }
        Commands::Dump { input } => cmd_dump(&input)?,
        Commands::Diff {
            original,
            roundtrip,
        } => cmd_diff(&original, &roundtrip)?,
        Commands::Scan { dir } => crate::scan::cmd_scan(&dir)?,
    }

    Ok(())
}

fn cmd_to_gltf(
    input: &PathBuf,
    output: &PathBuf,
    external_buffer: bool,
    no_skeleton: bool,
    no_verify: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let data = fs::read(input)?;
    let geom = if no_verify {
        ugx::UgxGeom::from_bytes_unchecked(&data)?
    } else {
        UgxReader::read(&data)?
    };

    let options = GltfExportOptions {
        embed_buffers: !external_buffer,
        include_materials: true,
        include_skeleton: !no_skeleton,
    };

    // Derive buffer filename from output path for external buffer mode
    let bin_name = output
        .with_extension("bin")
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let export = export_to_gltf_with_buffer_name(&geom, &options, &bin_name)?;

    // Write the glTF JSON
    fs::write(output, &export.json)?;
    println!("Wrote {}", output.display());

    // Write external buffer if present
    if let Some(buffer_data) = export.buffer {
        let bin_path = output.with_extension("bin");
        fs::write(&bin_path, &buffer_data)?;
        println!("Wrote {}", bin_path.display());
    }

    println!();
    println!(
        "Exported {} sections, {} materials, {} bones",
        geom.sections.len(),
        geom.materials.len(),
        geom.bones.len()
    );
    println!(
        "Total: {} vertices, {} triangles",
        geom.total_vertices(),
        geom.total_triangles()
    );

    Ok(())
}

/// Parse a GLB file and return the JSON string and binary buffer.
struct GltfSource {
    json: String,
    buffer: Option<Vec<u8>>,
}

fn parse_glb(data: &[u8]) -> Result<GltfSource, Box<dyn std::error::Error>> {
    // GLB Header: magic (4) + version (4) + length (4) = 12 bytes
    if data.len() < 12 {
        return Err("GLB file too small".into());
    }

    let magic = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    if magic != 0x4654_6C67 {
        // "glTF" in little-endian
        return Err(format!("Invalid GLB magic: 0x{magic:08X}").into());
    }

    let version = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    if version != 2 {
        return Err(format!("Unsupported GLB version: {version}").into());
    }

    let _total_length = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);

    // Parse chunks
    let mut offset = 12usize;
    let mut json_str = String::new();
    let mut bin_data: Option<Vec<u8>> = None;

    while offset + 8 <= data.len() {
        let chunk_length = u32::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]) as usize;
        let chunk_type = u32::from_le_bytes([
            data[offset + 4],
            data[offset + 5],
            data[offset + 6],
            data[offset + 7],
        ]);
        offset += 8;

        if offset + chunk_length > data.len() {
            return Err("GLB chunk extends beyond file".into());
        }

        match chunk_type {
            0x4E4F_534A => {
                // "JSON" in little-endian
                json_str = String::from_utf8(data[offset..offset + chunk_length].to_vec())?;
            }
            0x004E_4942 => {
                // "BIN\0" in little-endian
                bin_data = Some(data[offset..offset + chunk_length].to_vec());
            }
            _ => {
                // Unknown chunk type, skip
            }
        }

        offset += chunk_length;
    }

    if json_str.is_empty() {
        return Err("GLB file missing JSON chunk".into());
    }

    Ok(GltfSource {
        json: json_str,
        buffer: bin_data,
    })
}

fn cmd_from_gltf(
    input: &PathBuf,
    output: &PathBuf,
    no_skeleton: bool,
    version: UgxVersion,
) -> Result<(), Box<dyn std::error::Error>> {
    // Check if input is GLB or glTF based on extension
    let is_glb = input
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("glb"));

    let source = if is_glb {
        // Parse GLB container
        let data = fs::read(input)?;
        parse_glb(&data)?
    } else {
        // Read glTF JSON
        let json_str = fs::read_to_string(input)?;

        // Parse JSON to find the buffer URI
        let root: serde_json::Value = serde_json::from_str(&json_str)?;
        let buffer_data = if let Some(buffers) = root.get("buffers").and_then(|b| b.as_array()) {
            if let Some(first_buffer) = buffers.first() {
                if let Some(uri) = first_buffer.get("uri").and_then(|u| u.as_str()) {
                    // Check if it's a file path (not base64 embedded)
                    if uri.starts_with("data:") {
                        None // Base64 embedded, will be handled by import_from_gltf
                    } else {
                        // Resolve relative to the input file's directory
                        let base_dir = input.parent().unwrap_or(std::path::Path::new("."));
                        let bin_path = base_dir.join(uri);
                        if bin_path.exists() {
                            Some(fs::read(&bin_path)?)
                        } else {
                            return Err(format!(
                                "External buffer file not found: {} (expected at {})",
                                uri,
                                bin_path.display()
                            )
                            .into());
                        }
                    }
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        GltfSource {
            json: json_str,
            buffer: buffer_data,
        }
    };

    let options = GltfImportOptions {
        include_skeleton: !no_skeleton,
        include_materials: true,
        version,
    };

    let geom = import_from_gltf(&source.json, source.buffer.as_deref(), &options)?;

    // Write UGX
    let ugx_data = UgxWriter::write(&geom, version)?;
    fs::write(output, &ugx_data)?;

    println!("Wrote {}", output.display());
    println!();
    println!(
        "Imported {} sections, {} materials, {} bones",
        geom.sections.len(),
        geom.materials.len(),
        geom.bones.len()
    );
    println!(
        "Total: {} vertices, {} triangles",
        geom.total_vertices(),
        geom.total_triangles()
    );

    Ok(())
}

fn cmd_dump(input: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let data = std::fs::read(input)?;
    let ecf = EcfReader::new(&data)?;

    println!("=== ECF Header ===");
    let header = ecf.header();
    println!("  File ID: 0x{:08X}", header.id);
    println!("  Num chunks: {}", header.num_chunks);
    println!("  Flags: 0x{:04X}", header.flags);

    println!("\n=== Chunks ===");
    for (i, chunk) in ecf.chunks().iter().enumerate() {
        println!(
            "Chunk {}: ID=0x{:08X} offset=0x{:X} size=0x{:X} flags=0x{:02X}",
            i, chunk.id, chunk.offset, chunk.size, chunk.flags
        );
    }

    // Read and dump cached data chunk (0x700)
    if let Ok(cached_data) = ecf.chunk_data_by_id(0x700) {
        println!(
            "\n=== Cached Data (0x700) - {} bytes ===",
            cached_data.len()
        );
        hexdump(&cached_data, 768); // Dump more to see section data
    }

    // Read IB chunk (0x701)
    if let Ok(ib_data) = ecf.chunk_data_by_id(0x701) {
        println!("\n=== Index Buffer (0x701) - {} bytes ===", ib_data.len());
        println!("  {} indices", ib_data.len() / 2);
        hexdump(&ib_data, 64);
    }

    // Read VB chunk (0x702)
    if let Ok(vb_data) = ecf.chunk_data_by_id(0x702) {
        println!("\n=== Vertex Buffer (0x702) - {} bytes ===", vb_data.len());
        hexdump(&vb_data, 64);
    }

    // Read material chunk (0x704)
    if let Ok(mat_data) = ecf.chunk_data_by_id(0x704) {
        println!("\n=== Materials (0x704) - {} bytes ===", mat_data.len());
        hexdump(&mat_data, 512);
    }

    Ok(())
}

fn hexdump(data: &[u8], max: usize) {
    let show = data.len().min(max);
    for (i, chunk) in data[..show].chunks(16).enumerate() {
        print!("  {:04X}: ", i * 16);
        for byte in chunk {
            print!("{byte:02X} ");
        }
        // Pad
        for _ in chunk.len()..16 {
            print!("   ");
        }
        print!(" |");
        for byte in chunk {
            let c = if *byte >= 0x20 && *byte < 0x7F {
                *byte as char
            } else {
                '.'
            };
            print!("{c}");
        }
        println!("|");
    }
    if data.len() > max {
        println!("  ... ({} more bytes)", data.len() - max);
    }
}

fn cmd_diff(orig_path: &PathBuf, rt_path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let orig_data = fs::read(orig_path)?;
    let rt_data = fs::read(rt_path)?;
    let orig_ecf = EcfReader::new(&orig_data)?;
    let rt_ecf = EcfReader::new(&rt_data)?;

    println!("=== UGX Binary Diff ===");
    println!("  A: {} ({} bytes)", orig_path.display(), orig_data.len());
    println!("  B: {} ({} bytes)", rt_path.display(), rt_data.len());
    println!();

    print_binary_diff(&orig_ecf, &rt_ecf);

    println!("\n=== Parsed Geom Diff ===");
    let orig_geom = UgxReader::read(&orig_data)?;
    let rt_geom = UgxReader::read(&rt_data)?;
    print_material_diff(&orig_geom, &rt_geom);
    print_section_diff(&orig_geom, &rt_geom);
    print_normal_analysis(&orig_geom);
    print_vertex_diff(&orig_geom, &rt_geom);

    Ok(())
}

fn chunk_name(id: u64) -> &'static str {
    match id {
        0x700 => "CachedData (0x700)",
        0x701 => "IndexBuffer (0x701)",
        0x702 => "VertexBuffer (0x702)",
        0x703 => "Granny (0x703)",
        0x704 => "Material (0x704)",
        0x705 => "AABBTree (0x705)",
        _ => "Unknown",
    }
}

fn print_binary_diff(original: &EcfReader<'_>, roundtrip: &EcfReader<'_>) {
    let mut chunk_ids: Vec<u64> = original
        .chunks()
        .iter()
        .chain(roundtrip.chunks())
        .map(|chunk| chunk.id)
        .collect();
    chunk_ids.sort_unstable();
    chunk_ids.dedup();

    let mut any_diff = false;
    for id in chunk_ids {
        let original_data = original.chunk_data_by_id(id).ok();
        let roundtrip_data = roundtrip.chunk_data_by_id(id).ok();
        any_diff |= print_chunk_diff(id, original_data.as_deref(), roundtrip_data.as_deref());
    }

    if !any_diff {
        println!("\nAll chunks identical.");
    }
}

fn print_chunk_diff(id: u64, original: Option<&[u8]>, roundtrip: Option<&[u8]>) -> bool {
    match (original, roundtrip) {
        (None, Some(data)) => {
            println!(
                "  {} : MISSING in A, {} bytes in B",
                chunk_name(id),
                data.len()
            );
            true
        }
        (Some(data), None) => {
            println!(
                "  {} : {} bytes in A, MISSING in B",
                chunk_name(id),
                data.len()
            );
            true
        }
        (None, None) => false,
        (Some(original_data), Some(roundtrip_data)) if original_data == roundtrip_data => {
            println!(
                "  {} : IDENTICAL ({} bytes)",
                chunk_name(id),
                original_data.len()
            );
            false
        }
        (Some(original_data), Some(roundtrip_data)) => {
            print_differing_chunk(id, original_data, roundtrip_data);
            true
        }
    }
}

fn print_differing_chunk(id: u64, original: &[u8], roundtrip: &[u8]) {
    let diff_count = original
        .iter()
        .zip(roundtrip)
        .filter(|(left, right)| left != right)
        .count()
        + original.len().abs_diff(roundtrip.len());
    println!(
        "  {} : DIFFER (A={} B={} bytes, {} bytes differ)",
        chunk_name(id),
        original.len(),
        roundtrip.len(),
        diff_count
    );
    print_byte_differences(original, roundtrip);

    match original.len().cmp(&roundtrip.len()) {
        Ordering::Greater => println!(
            "    size diff: A has {} extra bytes",
            original.len() - roundtrip.len()
        ),
        Ordering::Less => println!(
            "    size diff: B has {} extra bytes",
            roundtrip.len() - original.len()
        ),
        Ordering::Equal => {}
    }
}

fn print_byte_differences(original: &[u8], roundtrip: &[u8]) {
    let common_len = original.len().min(roundtrip.len());
    let mut shown = 0;
    let mut index = 0;
    while index < common_len && shown < 5 {
        if original[index] == roundtrip[index] {
            index += 1;
            continue;
        }

        let start = index;
        while index < common_len && original[index] != roundtrip[index] {
            index += 1;
        }
        let end = index;
        let context_start = start.saturating_sub(4);
        let context_end = end.saturating_add(4).min(common_len);
        println!(
            "    offset 0x{start:04X}..0x{end:04X} ({} bytes differ):",
            end - start
        );
        print_diff_context("A", original, context_start, context_end, start..end, 31);
        print_diff_context("B", roundtrip, context_start, context_end, start..end, 32);
        shown += 1;
    }
}

fn print_diff_context(
    label: &str,
    data: &[u8],
    context_start: usize,
    context_end: usize,
    changed: std::ops::Range<usize>,
    color: u8,
) {
    print!("      {label}: ");
    for (offset, byte) in data[context_start..context_end].iter().enumerate() {
        let index = context_start + offset;
        if changed.contains(&index) {
            print!("\x1b[{color}m{byte:02X}\x1b[0m ");
        } else {
            print!("{byte:02X} ");
        }
    }
    println!();
}

fn print_material_diff(original: &ugx::UgxGeom, roundtrip: &ugx::UgxGeom) {
    if original.materials.len() != roundtrip.materials.len() {
        println!(
            "  Materials: count differs ({} vs {})",
            original.materials.len(),
            roundtrip.materials.len()
        );
    }

    for (index, (original_material, roundtrip_material)) in original
        .materials
        .iter()
        .zip(&roundtrip.materials)
        .enumerate()
    {
        let diffs = material_differences(original_material, roundtrip_material);
        if diffs.is_empty() {
            println!("  Material[{index}]: identical");
        } else {
            println!("  Material[{index}]: DIFFERS");
            for difference in diffs {
                println!("    {difference}");
            }
        }
    }
}

fn material_differences(original: &ugx::Material, roundtrip: &ugx::Material) -> Vec<String> {
    let mut differences = Vec::new();
    if original.name != roundtrip.name {
        differences.push(format!("name: {:?} vs {:?}", original.name, roundtrip.name));
    }

    match (&original.data, &roundtrip.data) {
        (ugx::MaterialData::Hogan(left), ugx::MaterialData::Hogan(right)) => {
            if left.skinned != right.skinned {
                differences.push(format!("skinned: {} vs {}", left.skinned, right.skinned));
            }
            if left.textures != right.textures {
                differences.push(format!(
                    "textures: {:?} vs {:?}",
                    left.textures, right.textures
                ));
            }
            if left.blend_mode != right.blend_mode {
                differences.push(format!(
                    "blend_mode: {} vs {}",
                    left.blend_mode, right.blend_mode
                ));
            }
            if left.shader_permutations.len() != right.shader_permutations.len() {
                differences.push(format!(
                    "perm count: {} vs {}",
                    left.shader_permutations.len(),
                    right.shader_permutations.len()
                ));
            }
            append_permutation_differences(&mut differences, left, right);
            if left.ps_cb_data != right.ps_cb_data {
                differences.push(format!(
                    "ps_cb: {} vs {} bytes",
                    left.ps_cb_data.len(),
                    right.ps_cb_data.len()
                ));
            }
            if left.vs_cb_data != right.vs_cb_data {
                differences.push(format!(
                    "vs_cb: {} vs {} bytes",
                    left.vs_cb_data.len(),
                    right.vs_cb_data.len()
                ));
            }
        }
        (ugx::MaterialData::Legacy(_), ugx::MaterialData::Hogan(_)) => {
            differences.push("type: Legacy vs Hogan".to_string());
        }
        (ugx::MaterialData::Hogan(_), ugx::MaterialData::Legacy(_)) => {
            differences.push("type: Hogan vs Legacy".to_string());
        }
        (ugx::MaterialData::Legacy(_), ugx::MaterialData::Legacy(_)) => {}
    }
    differences
}

fn append_permutation_differences(
    differences: &mut Vec<String>,
    original: &ugx::HoganMaterialData,
    roundtrip: &ugx::HoganMaterialData,
) {
    for (index, (left, right)) in original
        .shader_permutations
        .iter()
        .zip(&roundtrip.shader_permutations)
        .enumerate()
    {
        if left.name != right.name {
            differences.push(format!("perm[{index}]: {} vs {}", left.name, right.name));
        }
        if left.hash != right.hash {
            differences.push(format!(
                "perm[{index}] hash: 0x{:08X} vs 0x{:08X}",
                left.hash, right.hash
            ));
        }
    }
}

fn print_section_diff(original: &ugx::UgxGeom, roundtrip: &ugx::UgxGeom) {
    if original.sections.len() != roundtrip.sections.len() {
        println!(
            "  Sections: count differs ({} vs {})",
            original.sections.len(),
            roundtrip.sections.len()
        );
    }

    for (index, (original_section, roundtrip_section)) in original
        .sections
        .iter()
        .zip(&roundtrip.sections)
        .enumerate()
    {
        let diffs = section_differences(original_section, roundtrip_section);
        if diffs.is_empty() {
            println!("  Section[{index}]: identical");
        } else {
            println!("  Section[{index}]: DIFFERS");
            for difference in diffs {
                println!("    {difference}");
            }
        }
    }
}

fn section_differences(original: &ugx::Section, roundtrip: &ugx::Section) -> Vec<String> {
    let mut differences = Vec::new();
    if original.vert_size != roundtrip.vert_size {
        differences.push(format!(
            "vert_size: {} vs {}",
            original.vert_size, roundtrip.vert_size
        ));
    }
    if original.num_verts != roundtrip.num_verts {
        differences.push(format!(
            "num_verts: {} vs {}",
            original.num_verts, roundtrip.num_verts
        ));
    }
    if original.num_tris != roundtrip.num_tris {
        differences.push(format!(
            "num_tris: {} vs {}",
            original.num_tris, roundtrip.num_tris
        ));
    }
    if original.material_index != roundtrip.material_index {
        differences.push(format!(
            "material: {} vs {}",
            original.material_index, roundtrip.material_index
        ));
    }
    if original.rigid_only != roundtrip.rigid_only {
        differences.push(format!(
            "rigid_only: {} vs {}",
            original.rigid_only, roundtrip.rigid_only
        ));
    }
    if original.global_bones != roundtrip.global_bones {
        differences.push(format!(
            "global_bones: {} vs {}",
            original.global_bones, roundtrip.global_bones
        ));
    }
    if original.max_bones != roundtrip.max_bones {
        differences.push(format!(
            "max_bones: {} vs {}",
            original.max_bones, roundtrip.max_bones
        ));
    }
    if original.rigid_bone_index != roundtrip.rigid_bone_index {
        differences.push(format!(
            "rigid_bone_index: {} vs {}",
            original.rigid_bone_index, roundtrip.rigid_bone_index
        ));
    }
    differences
}

fn print_normal_analysis(original: &ugx::UgxGeom) {
    println!("\n=== Normal Length Analysis (original) ===");
    for (index, _) in original.sections.iter().enumerate() {
        if let Ok(vertices) = original.unpack_section_vertices(index) {
            let lengths: Vec<f32> = vertices
                .iter()
                .map(|vertex| vector_length(vertex.normal))
                .collect();
            let min_len = lengths.iter().copied().fold(f32::MAX, f32::min);
            let max_len = lengths.iter().copied().fold(0.0f32, f32::max);
            let sample_count = lengths.iter().fold(0.0f32, |count, _| count + 1.0);
            let avg_len = lengths.iter().sum::<f32>() / sample_count;
            let near_unit = lengths
                .iter()
                .filter(|length| (1.0 - **length).abs() < 0.01)
                .count();
            println!(
                "  Section[{}]: {} normals, len min={:.4} max={:.4} avg={:.4}, near_unit={}/{}",
                index,
                lengths.len(),
                min_len,
                max_len,
                avg_len,
                near_unit,
                lengths.len()
            );
            for (vertex_index, vertex) in vertices.iter().take(5).enumerate() {
                let length = vector_length(vertex.normal);
                println!(
                    "    [{}] normal=[{:.6}, {:.6}, {:.6}] len={:.6}",
                    vertex_index, vertex.normal[0], vertex.normal[1], vertex.normal[2], length
                );
            }
        }
    }
}

fn vector_length(vector: [f32; 3]) -> f32 {
    (vector[0] * vector[0] + vector[1] * vector[1] + vector[2] * vector[2]).sqrt()
}

fn print_vertex_diff(original: &ugx::UgxGeom, roundtrip: &ugx::UgxGeom) {
    for (index, _) in original
        .sections
        .iter()
        .zip(&roundtrip.sections)
        .enumerate()
    {
        if let (Ok(original_vertices), Ok(roundtrip_vertices)) = (
            original.unpack_section_vertices(index),
            roundtrip.unpack_section_vertices(index),
        ) {
            let stats = compare_vertices(&original_vertices, &roundtrip_vertices);
            if stats.position_diffs > 0
                || stats.normal_diffs > 0
                || stats.texcoord_diffs > 0
                || original_vertices.len() != roundtrip_vertices.len()
            {
                println!(
                    "  Section[{}] vertices: {} vs {} verts, max_pos_err={:.4}, pos_diffs={}, norm_diffs={}, uv_diffs={}",
                    index,
                    original_vertices.len(),
                    roundtrip_vertices.len(),
                    stats.max_position_error,
                    stats.position_diffs,
                    stats.normal_diffs,
                    stats.texcoord_diffs
                );
            } else {
                println!(
                    "  Section[{}] vertices: identical ({} verts, max_pos_err={:.6})",
                    index,
                    original_vertices.len(),
                    stats.max_position_error
                );
            }
        }
    }
}

#[derive(Default)]
struct VertexDiffStats {
    position_diffs: usize,
    normal_diffs: usize,
    texcoord_diffs: usize,
    max_position_error: f32,
}

fn compare_vertices(
    original: &[ugx::UnpackedVertex],
    roundtrip: &[ugx::UnpackedVertex],
) -> VertexDiffStats {
    let mut stats = VertexDiffStats::default();
    for (left, right) in original.iter().zip(roundtrip) {
        let position_delta = [
            left.position[0] - right.position[0],
            left.position[1] - right.position[1],
            left.position[2] - right.position[2],
        ];
        let position_distance = vector_length(position_delta);
        stats.max_position_error = stats.max_position_error.max(position_distance);
        if position_distance > 0.01 {
            stats.position_diffs += 1;
        }

        let normal_delta = [
            left.normal[0] - right.normal[0],
            left.normal[1] - right.normal[1],
            left.normal[2] - right.normal[2],
        ];
        if vector_length(normal_delta) > 0.05 {
            stats.normal_diffs += 1;
        }
        if left.num_texcoords > 0
            && right.num_texcoords > 0
            && ((left.texcoords[0][0] - right.texcoords[0][0]).abs() > 0.001
                || (left.texcoords[0][1] - right.texcoords[0][1]).abs() > 0.001)
        {
            stats.texcoord_diffs += 1;
        }
    }
    stats
}
