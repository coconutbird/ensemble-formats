//! UGX CLI - Command-line tool for UGX model files.

use clap::{Parser, Subcommand};
use ecf::Reader as EcfReader;
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Info { input, no_verify } => cmd_info(&input, no_verify)?,
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
                    eprintln!("Unknown version '{}', expected 'hw1' or 'hw2'", other);
                    std::process::exit(1);
                }
            };
            cmd_from_gltf(&input, &output, no_skeleton, ugx_version)?
        }
        Commands::Dump { input } => cmd_dump(&input)?,
        Commands::Diff {
            original,
            roundtrip,
        } => cmd_diff(&original, &roundtrip)?,
        Commands::Scan { dir } => cmd_scan(&dir)?,
    }

    Ok(())
}

fn cmd_info(input: &PathBuf, no_verify: bool) -> Result<(), Box<dyn std::error::Error>> {
    let data = fs::read(input)?;
    let geom = if no_verify {
        ugx::UgxGeom::from_bytes_unchecked(&data)?
    } else {
        UgxReader::read(&data)?
    };

    println!("UGX File: {}", input.display());
    println!();

    println!("Bounding Sphere:");
    println!(
        "  Center: [{:.3}, {:.3}, {:.3}]",
        geom.bounding_sphere.center[0],
        geom.bounding_sphere.center[1],
        geom.bounding_sphere.center[2]
    );
    println!("  Radius: {:.3}", geom.bounding_sphere.radius);
    println!();

    println!("Bounding Box:");
    println!(
        "  Min: [{:.3}, {:.3}, {:.3}]",
        geom.bounds.min[0], geom.bounds.min[1], geom.bounds.min[2]
    );
    println!(
        "  Max: [{:.3}, {:.3}, {:.3}]",
        geom.bounds.max[0], geom.bounds.max[1], geom.bounds.max[2]
    );
    println!();

    println!("Materials: {}", geom.materials.len());
    for (i, mat) in geom.materials.iter().enumerate() {
        match &mat.data {
            ugx::types::MaterialData::Hogan(hogan) => {
                println!("  [{}] (Hogan)", i);
                for (j, perm) in hogan.shader_permutations.iter().enumerate() {
                    println!(
                        "      Permutation[{}]: {} (hash=0x{:08X})",
                        j, perm.name, perm.hash
                    );
                }
                println!(
                    "      ufx_version={}, blend_mode={}",
                    hogan.ufx_version, hogan.blend_mode
                );
                println!(
                    "      skinned={}, terrain_blending={}, shadow_requires_consts={}",
                    hogan.skinned, hogan.terrain_blending, hogan.shadow_requires_consts
                );
                println!(
                    "      vs_cb={} ps_cb={} hs_cb={} ds_cb={} gs_cb={} bytes",
                    hogan.vs_cb_data.len(),
                    hogan.ps_cb_data.len(),
                    hogan.hs_cb_data.len(),
                    hogan.ds_cb_data.len(),
                    hogan.gs_cb_data.len(),
                );
                println!("      textures: {}", hogan.textures);
            }
            ugx::types::MaterialData::Legacy(legacy) => {
                println!("  [{}] {} (legacy v{})", i, mat.name, mat.material_version);
                println!(
                    "      blend_type={}, opacity={}, flags=0x{:X}",
                    legacy.blend_type, legacy.opacity, legacy.flags
                );
                println!(
                    "      spec_power={}, env_fresnel={}, env_fresnel_power={}",
                    legacy.spec_power, legacy.env_fresnel, legacy.env_fresnel_power
                );
                for (t, maps) in legacy.maps.iter().enumerate() {
                    if !maps.is_empty() {
                        let names: Vec<&str> = maps.iter().map(|m| m.name.as_str()).collect();
                        println!(
                            "      {}: {}",
                            ugx::types::material::MapType::ALL[t].name(),
                            names.join(", ")
                        );
                    }
                }
            }
        }
    }
    println!();

    println!("Bones: {}", geom.bones.len());
    for (i, bone) in geom.bones.iter().enumerate() {
        let parent = if bone.parent_index >= 0 {
            format!("parent={}", bone.parent_index)
        } else {
            "root".to_string()
        };
        println!("  [{}] {} ({})", i, bone.name, parent);
        // Print matrix for first 3 bones
        if i < 3 {
            let m = &bone.model_to_bone.rows;
            println!("      Matrix (model_to_bone):");
            println!(
                "        [{:8.4}, {:8.4}, {:8.4}, {:8.4}]",
                m[0][0], m[0][1], m[0][2], m[0][3]
            );
            println!(
                "        [{:8.4}, {:8.4}, {:8.4}, {:8.4}]",
                m[1][0], m[1][1], m[1][2], m[1][3]
            );
            println!(
                "        [{:8.4}, {:8.4}, {:8.4}, {:8.4}]",
                m[2][0], m[2][1], m[2][2], m[2][3]
            );
            println!(
                "        [{:8.4}, {:8.4}, {:8.4}, {:8.4}]",
                m[3][0], m[3][1], m[3][2], m[3][3]
            );
        }
    }
    println!();

    println!("Sections: {}", geom.sections.len());
    for (i, section) in geom.sections.iter().enumerate() {
        println!(
            "  [{}] Material={}, Verts={}, Tris={}, VB={}bytes, VertSize={}",
            i,
            section.material_index,
            section.num_verts,
            section.num_tris,
            section.vb_bytes,
            section.vert_size
        );
        println!(
            "       MaxBones={}, RigidBoneIdx={}, RigidOnly={}, GlobalBones={}",
            section.max_bones, section.rigid_bone_index, section.rigid_only, section.global_bones
        );
        if !section.bone_remap.is_empty() {
            println!(
                "       BoneRemap[{}]: {:?}",
                section.bone_remap.len(),
                section.bone_remap
            );
        } else {
            println!("       BoneRemap: (empty)");
        }
        if let Some(ref packer) = section.base_vert_packer {
            println!("       PackOrder: {}", packer.pack_order);
            println!("       PosType: {:?}", packer.pos_type);
            println!("       NormType: {:?}", packer.normal_type);
            println!("       TangentType: {:?}", packer.tangent_type);
            println!("       UV[0]Type: {:?}", packer.uv_types[0]);
            println!("       IndicesType: {:?}", packer.indices_type);
            println!("       WeightsType: {:?}", packer.weights_type);
        } else {
            println!("       (HW2 format — no UnivertPacker)");
        }

        // Print first 3 unpacked vertices for debugging
        if section.num_verts > 0
            && let Ok(verts) = geom.unpack_section_vertices(i)
        {
            println!("       First 3 vertices:");
            for (vi, v) in verts.iter().take(3).enumerate() {
                println!(
                    "         [{}] pos=[{:.3}, {:.3}, {:.3}] bones=[{},{},{},{}] weights=[{:.3},{:.3},{:.3},{:.3}]",
                    vi,
                    v.position[0],
                    v.position[1],
                    v.position[2],
                    v.bone_indices[0],
                    v.bone_indices[1],
                    v.bone_indices[2],
                    v.bone_indices[3],
                    v.bone_weights[0],
                    v.bone_weights[1],
                    v.bone_weights[2],
                    v.bone_weights[3]
                );
            }
        }
    }
    println!();

    println!("Summary:");
    println!("  Total Vertices: {}", geom.total_vertices());
    println!("  Total Triangles: {}", geom.total_triangles());
    println!("  Vertex Buffer: {} bytes", geom.vertex_buffer.len());
    println!("  Index Buffer: {} indices", geom.index_buffer.len());
    println!("  Rigid Only: {}", geom.rigid_only);
    println!("  All Sections Rigid: {}", geom.all_sections_rigid);
    println!("  All Sections Skinned: {}", geom.all_sections_skinned);

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
#[allow(clippy::type_complexity)]
fn parse_glb(data: &[u8]) -> Result<(String, Option<Vec<u8>>), Box<dyn std::error::Error>> {
    // GLB Header: magic (4) + version (4) + length (4) = 12 bytes
    if data.len() < 12 {
        return Err("GLB file too small".into());
    }

    let magic = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    if magic != 0x46546C67 {
        // "glTF" in little-endian
        return Err(format!("Invalid GLB magic: 0x{:08X}", magic).into());
    }

    let version = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    if version != 2 {
        return Err(format!("Unsupported GLB version: {}", version).into());
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
            0x4E4F534A => {
                // "JSON" in little-endian
                json_str = String::from_utf8(data[offset..offset + chunk_length].to_vec())?;
            }
            0x004E4942 => {
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

    Ok((json_str, bin_data))
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
        .map(|ext| ext.eq_ignore_ascii_case("glb"))
        .unwrap_or(false);

    let (json_str, buffer_data) = if is_glb {
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
                    if !uri.starts_with("data:") {
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
                    } else {
                        None // Base64 embedded, will be handled by import_from_gltf
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

        (json_str, buffer_data)
    };

    let options = GltfImportOptions {
        include_skeleton: !no_skeleton,
        include_materials: true,
        version,
    };

    let geom = import_from_gltf(&json_str, buffer_data.as_deref(), &options)?;

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
            print!("{:02X} ", byte);
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
            print!("{}", c);
        }
        println!("|");
    }
    if data.len() > max {
        println!("  ... ({} more bytes)", data.len() - max);
    }
}

#[allow(clippy::needless_range_loop)]
fn cmd_diff(orig_path: &PathBuf, rt_path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let chunk_name = |id: u64| -> &'static str {
        match id {
            0x700 => "CachedData (0x700)",
            0x701 => "IndexBuffer (0x701)",
            0x702 => "VertexBuffer (0x702)",
            0x703 => "Granny (0x703)",
            0x704 => "Material (0x704)",
            0x705 => "AABBTree (0x705)",
            _ => "Unknown",
        }
    };

    let orig_data = fs::read(orig_path)?;
    let rt_data = fs::read(rt_path)?;

    let orig_ecf = EcfReader::new(&orig_data)?;
    let rt_ecf = EcfReader::new(&rt_data)?;

    println!("=== UGX Binary Diff ===");
    println!("  A: {} ({} bytes)", orig_path.display(), orig_data.len());
    println!("  B: {} ({} bytes)", rt_path.display(), rt_data.len());
    println!();

    // Collect all chunk IDs
    let mut all_ids: Vec<u64> = Vec::new();
    for c in orig_ecf.chunks() {
        if !all_ids.contains(&c.id) {
            all_ids.push(c.id);
        }
    }
    for c in rt_ecf.chunks() {
        if !all_ids.contains(&c.id) {
            all_ids.push(c.id);
        }
    }
    all_ids.sort();

    let mut any_diff = false;

    for &id in &all_ids {
        let o = orig_ecf.chunk_data_by_id(id);
        let r = rt_ecf.chunk_data_by_id(id);

        match (o, r) {
            (Err(_), Ok(r_data)) => {
                println!(
                    "  {} : MISSING in A, {} bytes in B",
                    chunk_name(id),
                    r_data.len()
                );
                any_diff = true;
            }
            (Ok(o_data), Err(_)) => {
                println!(
                    "  {} : {} bytes in A, MISSING in B",
                    chunk_name(id),
                    o_data.len()
                );
                any_diff = true;
            }
            (Err(_), Err(_)) => {}
            (Ok(o_data), Ok(r_data)) => {
                if o_data == r_data {
                    println!("  {} : IDENTICAL ({} bytes)", chunk_name(id), o_data.len());
                } else {
                    any_diff = true;
                    let diff_count = o_data
                        .iter()
                        .zip(r_data.iter())
                        .filter(|(a, b)| a != b)
                        .count()
                        + o_data.len().abs_diff(r_data.len());
                    println!(
                        "  {} : DIFFER (A={} B={} bytes, {} bytes differ)",
                        chunk_name(id),
                        o_data.len(),
                        r_data.len(),
                        diff_count
                    );

                    // Show first few diffs with context
                    let min_len = o_data.len().min(r_data.len());
                    let mut shown = 0;
                    let mut i = 0;
                    while i < min_len && shown < 5 {
                        if o_data[i] != r_data[i] {
                            // Find the end of this diff region
                            let start = i;
                            while i < min_len && o_data[i] != r_data[i] {
                                i += 1;
                            }
                            let end = i;
                            let ctx_start = start.saturating_sub(4);
                            let ctx_end = (end + 4).min(min_len);
                            println!(
                                "    offset 0x{:04X}..0x{:04X} ({} bytes differ):",
                                start,
                                end,
                                end - start
                            );
                            print!("      A: ");
                            for j in ctx_start..ctx_end {
                                if j >= start && j < end {
                                    print!("\x1b[31m{:02X}\x1b[0m ", o_data[j]);
                                } else {
                                    print!("{:02X} ", o_data[j]);
                                }
                            }
                            println!();
                            print!("      B: ");
                            for j in ctx_start..ctx_end {
                                if j >= start && j < end {
                                    print!("\x1b[32m{:02X}\x1b[0m ", r_data[j]);
                                } else {
                                    print!("{:02X} ", r_data[j]);
                                }
                            }
                            println!();
                            shown += 1;
                        } else {
                            i += 1;
                        }
                    }
                    if o_data.len() != r_data.len() {
                        println!(
                            "    size diff: A has {} extra bytes",
                            o_data.len() as i64 - r_data.len() as i64
                        );
                    }
                }
            }
        }
    }

    if !any_diff {
        println!("\nAll chunks identical.");
    }

    // Also diff at the parsed geom level for sections/materials
    println!("\n=== Parsed Geom Diff ===");
    let orig_geom = UgxReader::read(&orig_data)?;
    let rt_geom = UgxReader::read(&rt_data)?;

    // Materials
    if orig_geom.materials.len() != rt_geom.materials.len() {
        println!(
            "  Materials: count differs ({} vs {})",
            orig_geom.materials.len(),
            rt_geom.materials.len()
        );
    }
    for i in 0..orig_geom.materials.len().min(rt_geom.materials.len()) {
        let om = &orig_geom.materials[i];
        let rm = &rt_geom.materials[i];
        let mut diffs = Vec::new();
        if om.name != rm.name {
            diffs.push(format!("name: {:?} vs {:?}", om.name, rm.name));
        }
        match (&om.data, &rm.data) {
            (ugx::types::MaterialData::Hogan(oh), ugx::types::MaterialData::Hogan(rh)) => {
                if oh.skinned != rh.skinned {
                    diffs.push(format!("skinned: {} vs {}", oh.skinned, rh.skinned));
                }
                if oh.textures != rh.textures {
                    diffs.push(format!("textures: {:?} vs {:?}", oh.textures, rh.textures));
                }
                if oh.blend_mode != rh.blend_mode {
                    diffs.push(format!(
                        "blend_mode: {} vs {}",
                        oh.blend_mode, rh.blend_mode
                    ));
                }
                if oh.shader_permutations.len() != rh.shader_permutations.len() {
                    diffs.push(format!(
                        "perm count: {} vs {}",
                        oh.shader_permutations.len(),
                        rh.shader_permutations.len()
                    ));
                }
                for j in 0..oh
                    .shader_permutations
                    .len()
                    .min(rh.shader_permutations.len())
                {
                    if oh.shader_permutations[j].name != rh.shader_permutations[j].name {
                        diffs.push(format!(
                            "perm[{}]: {} vs {}",
                            j, oh.shader_permutations[j].name, rh.shader_permutations[j].name
                        ));
                    }
                    if oh.shader_permutations[j].hash != rh.shader_permutations[j].hash {
                        diffs.push(format!(
                            "perm[{}] hash: 0x{:08X} vs 0x{:08X}",
                            j, oh.shader_permutations[j].hash, rh.shader_permutations[j].hash
                        ));
                    }
                }
                if oh.ps_cb_data != rh.ps_cb_data {
                    diffs.push(format!(
                        "ps_cb: {} vs {} bytes",
                        oh.ps_cb_data.len(),
                        rh.ps_cb_data.len()
                    ));
                }
                if oh.vs_cb_data != rh.vs_cb_data {
                    diffs.push(format!(
                        "vs_cb: {} vs {} bytes",
                        oh.vs_cb_data.len(),
                        rh.vs_cb_data.len()
                    ));
                }
            }
            (ugx::types::MaterialData::Legacy(_), ugx::types::MaterialData::Hogan(_)) => {
                diffs.push("type: Legacy vs Hogan".to_string());
            }
            (ugx::types::MaterialData::Hogan(_), ugx::types::MaterialData::Legacy(_)) => {
                diffs.push("type: Hogan vs Legacy".to_string());
            }
            _ => {}
        }
        if diffs.is_empty() {
            println!("  Material[{}]: identical", i);
        } else {
            println!("  Material[{}]: DIFFERS", i);
            for d in &diffs {
                println!("    {}", d);
            }
        }
    }

    // Sections
    if orig_geom.sections.len() != rt_geom.sections.len() {
        println!(
            "  Sections: count differs ({} vs {})",
            orig_geom.sections.len(),
            rt_geom.sections.len()
        );
    }
    for i in 0..orig_geom.sections.len().min(rt_geom.sections.len()) {
        let os = &orig_geom.sections[i];
        let rs = &rt_geom.sections[i];
        let mut diffs = Vec::new();
        if os.vert_size != rs.vert_size {
            diffs.push(format!("vert_size: {} vs {}", os.vert_size, rs.vert_size));
        }
        if os.num_verts != rs.num_verts {
            diffs.push(format!("num_verts: {} vs {}", os.num_verts, rs.num_verts));
        }
        if os.num_tris != rs.num_tris {
            diffs.push(format!("num_tris: {} vs {}", os.num_tris, rs.num_tris));
        }
        if os.material_index != rs.material_index {
            diffs.push(format!(
                "material: {} vs {}",
                os.material_index, rs.material_index
            ));
        }
        if os.rigid_only != rs.rigid_only {
            diffs.push(format!(
                "rigid_only: {} vs {}",
                os.rigid_only, rs.rigid_only
            ));
        }
        if os.global_bones != rs.global_bones {
            diffs.push(format!(
                "global_bones: {} vs {}",
                os.global_bones, rs.global_bones
            ));
        }
        if os.max_bones != rs.max_bones {
            diffs.push(format!("max_bones: {} vs {}", os.max_bones, rs.max_bones));
        }
        if os.rigid_bone_index != rs.rigid_bone_index {
            diffs.push(format!(
                "rigid_bone_index: {} vs {}",
                os.rigid_bone_index, rs.rigid_bone_index
            ));
        }
        if diffs.is_empty() {
            println!("  Section[{}]: identical", i);
        } else {
            println!("  Section[{}]: DIFFERS", i);
            for d in &diffs {
                println!("    {}", d);
            }
        }
    }

    // Normal length analysis on original file
    println!("\n=== Normal Length Analysis (original) ===");
    for i in 0..orig_geom.sections.len() {
        if let Ok(ov) = orig_geom.unpack_section_vertices(i) {
            let lengths: Vec<f32> = ov
                .iter()
                .map(|v| {
                    (v.normal[0] * v.normal[0]
                        + v.normal[1] * v.normal[1]
                        + v.normal[2] * v.normal[2])
                        .sqrt()
                })
                .collect();
            let min_len = lengths.iter().cloned().fold(f32::MAX, f32::min);
            let max_len = lengths.iter().cloned().fold(0.0f32, f32::max);
            let avg_len: f32 = lengths.iter().sum::<f32>() / lengths.len() as f32;
            let near_unit = lengths.iter().filter(|l| (1.0 - **l).abs() < 0.01).count();
            println!(
                "  Section[{}]: {} normals, len min={:.4} max={:.4} avg={:.4}, near_unit={}/{}",
                i,
                lengths.len(),
                min_len,
                max_len,
                avg_len,
                near_unit,
                lengths.len()
            );
            // Print first 5 normals
            for (vi, v) in ov.iter().take(5).enumerate() {
                let len = (v.normal[0] * v.normal[0]
                    + v.normal[1] * v.normal[1]
                    + v.normal[2] * v.normal[2])
                    .sqrt();
                println!(
                    "    [{}] normal=[{:.6}, {:.6}, {:.6}] len={:.6}",
                    vi, v.normal[0], v.normal[1], v.normal[2], len
                );
            }
        }
    }

    // Vertex comparison (first few verts per section)
    for i in 0..orig_geom.sections.len().min(rt_geom.sections.len()) {
        if let (Ok(ov), Ok(rv)) = (
            orig_geom.unpack_section_vertices(i),
            rt_geom.unpack_section_vertices(i),
        ) {
            let mut pos_diffs = 0;
            let mut norm_diffs = 0;
            let mut uv_diffs = 0;
            let max_pos_err: f32 = ov
                .iter()
                .zip(rv.iter())
                .map(|(a, b)| {
                    let dx = a.position[0] - b.position[0];
                    let dy = a.position[1] - b.position[1];
                    let dz = a.position[2] - b.position[2];
                    (dx * dx + dy * dy + dz * dz).sqrt()
                })
                .fold(0.0f32, f32::max);

            for (a, b) in ov.iter().zip(rv.iter()) {
                let pdist = ((a.position[0] - b.position[0]).powi(2)
                    + (a.position[1] - b.position[1]).powi(2)
                    + (a.position[2] - b.position[2]).powi(2))
                .sqrt();
                if pdist > 0.01 {
                    pos_diffs += 1;
                }
                let ndist = ((a.normal[0] - b.normal[0]).powi(2)
                    + (a.normal[1] - b.normal[1]).powi(2)
                    + (a.normal[2] - b.normal[2]).powi(2))
                .sqrt();
                if ndist > 0.05 {
                    norm_diffs += 1;
                }
                if a.num_texcoords > 0
                    && b.num_texcoords > 0
                    && ((a.texcoords[0][0] - b.texcoords[0][0]).abs() > 0.001
                        || (a.texcoords[0][1] - b.texcoords[0][1]).abs() > 0.001)
                {
                    uv_diffs += 1;
                }
            }
            if pos_diffs > 0 || norm_diffs > 0 || uv_diffs > 0 || ov.len() != rv.len() {
                println!(
                    "  Section[{}] vertices: {} vs {} verts, max_pos_err={:.4}, pos_diffs={}, norm_diffs={}, uv_diffs={}",
                    i,
                    ov.len(),
                    rv.len(),
                    max_pos_err,
                    pos_diffs,
                    norm_diffs,
                    uv_diffs
                );
            } else {
                println!(
                    "  Section[{}] vertices: identical ({} verts, max_pos_err={:.6})",
                    i,
                    ov.len(),
                    max_pos_err
                );
            }
        }
    }

    Ok(())
}

fn cmd_scan(dir: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    use std::collections::BTreeMap;

    // Walk the directory tree for .ugx files
    let mut ugx_paths: Vec<PathBuf> = Vec::new();
    collect_ugx_files(dir, &mut ugx_paths)?;
    ugx_paths.sort();
    println!("Found {} UGX files in {}", ugx_paths.len(), dir.display());
    if ugx_paths.is_empty() {
        return Ok(());
    }

    let mut hw1_count = 0u32;
    let mut hw2_count = 0u32;
    let mut parse_errors = 0u32;

    // HW2 section reserved fields: (reserved1, reserved2) -> Vec<(filename, section_idx)>
    let mut hw2_reserved: BTreeMap<(u32, u32), Vec<(String, usize)>> = BTreeMap::new();

    // Header padding: (pad_2byte, pad_4byte) -> Vec<filename>
    let mut hdr_padding: BTreeMap<(u16, u32), Vec<String>> = BTreeMap::new();

    // HW1 section trailing fields: (rigid_only, global_bones, padding) -> Vec<(filename, sec)>
    let mut hw1_trailing: BTreeMap<(i32, i32, i32), Vec<(String, usize)>> = BTreeMap::new();

    // HW2 section flags: (flags1, flags2) for non-boolean values
    let mut hw2_flags: BTreeMap<(i32, i32), Vec<(String, usize)>> = BTreeMap::new();

    for path in &ugx_paths {
        let data = match fs::read(path) {
            Ok(d) => d,
            Err(_) => {
                parse_errors += 1;
                continue;
            }
        };

        let ecf = match EcfReader::new(&data) {
            Ok(e) => e,
            Err(_) => {
                parse_errors += 1;
                continue;
            }
        };

        let cached = match ecf.chunk_data_by_id(0x700) {
            Ok(c) => c,
            Err(_) => {
                parse_errors += 1;
                continue;
            }
        };

        if cached.len() < 0x50 {
            parse_errors += 1;
            continue;
        }

        let sig = u32::from_le_bytes([cached[0], cached[1], cached[2], cached[3]]);
        let fname = path.strip_prefix(dir).unwrap_or(path).display().to_string();

        // Header padding at +0x38 (2 bytes) and +0x3C (4 bytes)
        let pad1 = u16::from_le_bytes([cached[0x38], cached[0x39]]);
        let pad2 = u32::from_le_bytes([cached[0x3C], cached[0x3D], cached[0x3E], cached[0x3F]]);
        hdr_padding
            .entry((pad1, pad2))
            .or_default()
            .push(fname.clone());

        // Sections packed array at +0x40
        let sec_count =
            u32::from_le_bytes([cached[0x40], cached[0x41], cached[0x42], cached[0x43]]) as usize;
        // skip 4 bytes padding at +0x44
        let sec_offset = u64::from_le_bytes([
            cached[0x48],
            cached[0x49],
            cached[0x4A],
            cached[0x4B],
            cached[0x4C],
            cached[0x4D],
            cached[0x4E],
            cached[0x4F],
        ]) as usize;

        match sig {
            0xC2340006 => {
                // HW2 — 72-byte sections
                hw2_count += 1;
                for s in 0..sec_count {
                    let base = sec_offset + s * 72;
                    if base + 72 > cached.len() {
                        break;
                    }
                    // flags at +0x28, +0x2C
                    let f1 = i32::from_le_bytes([
                        cached[base + 0x28],
                        cached[base + 0x29],
                        cached[base + 0x2A],
                        cached[base + 0x2B],
                    ]);
                    let f2 = i32::from_le_bytes([
                        cached[base + 0x2C],
                        cached[base + 0x2D],
                        cached[base + 0x2E],
                        cached[base + 0x2F],
                    ]);
                    if f1 != 0 && f1 != 1 || f2 != 0 && f2 != 1 {
                        hw2_flags
                            .entry((f1, f2))
                            .or_default()
                            .push((fname.clone(), s));
                    }

                    // reserved at +0x30, +0x34
                    let r1 = u32::from_le_bytes([
                        cached[base + 0x30],
                        cached[base + 0x31],
                        cached[base + 0x32],
                        cached[base + 0x33],
                    ]);
                    let r2 = u32::from_le_bytes([
                        cached[base + 0x34],
                        cached[base + 0x35],
                        cached[base + 0x36],
                        cached[base + 0x37],
                    ]);
                    hw2_reserved
                        .entry((r1, r2))
                        .or_default()
                        .push((fname.clone(), s));
                }
            }
            0xC2340004 => {
                // HW1 — 152-byte sections
                hw1_count += 1;
                for s in 0..sec_count {
                    let base = sec_offset + s * 152;
                    if base + 152 > cached.len() {
                        break;
                    }
                    let rigid = i32::from_le_bytes([
                        cached[base + 0x8C],
                        cached[base + 0x8D],
                        cached[base + 0x8E],
                        cached[base + 0x8F],
                    ]);
                    let global = i32::from_le_bytes([
                        cached[base + 0x90],
                        cached[base + 0x91],
                        cached[base + 0x92],
                        cached[base + 0x93],
                    ]);
                    let padding = i32::from_le_bytes([
                        cached[base + 0x94],
                        cached[base + 0x95],
                        cached[base + 0x96],
                        cached[base + 0x97],
                    ]);
                    if rigid != 0 && rigid != 1 || global != 0 && global != 1 || padding != 0 {
                        hw1_trailing
                            .entry((rigid, global, padding))
                            .or_default()
                            .push((fname.clone(), s));
                    }
                }
            }
            _ => {
                parse_errors += 1;
            }
        }
    }

    println!("\n=== Summary ===");
    println!(
        "HW1 (v4): {} files, HW2 (v6): {} files, errors: {}",
        hw1_count, hw2_count, parse_errors
    );

    // --- Header padding ---
    println!("\n=== GeomHeader Padding (+0x38 u16, +0x3C u32) ===");
    for ((p1, p2), files) in &hdr_padding {
        println!(
            "  pad1=0x{:04X} pad2=0x{:08X}: {} files",
            p1,
            p2,
            files.len()
        );
        if *p1 != 0 || *p2 != 0 {
            for f in files.iter().take(10) {
                println!("    {}", f);
            }
            if files.len() > 10 {
                println!("    ... and {} more", files.len() - 10);
            }
        }
    }

    // --- HW2 reserved fields ---
    println!("\n=== HW2 Section Reserved Fields (+0x30, +0x34) ===");
    for ((r1, r2), entries) in &hw2_reserved {
        let r1_f = f32::from_bits(*r1);
        println!(
            "  reserved1=0x{:08X} ({:e}), reserved2=0x{:08X}: {} sections",
            r1,
            r1_f,
            r2,
            entries.len()
        );
        if *r1 != 0x7F7FFFFF || *r2 != 0 {
            // Non-standard — show examples
            for (f, s) in entries.iter().take(10) {
                println!("    {}[sec{}]", f, s);
            }
            if entries.len() > 10 {
                println!("    ... and {} more", entries.len() - 10);
            }
        }
    }

    // --- HW2 non-boolean flags ---
    if !hw2_flags.is_empty() {
        println!("\n=== HW2 Section Flags (non-boolean values at +0x28, +0x2C) ===");
        for ((f1, f2), entries) in &hw2_flags {
            println!(
                "  flags1={} (0x{:08X}), flags2={} (0x{:08X}): {} sections",
                f1,
                *f1 as u32,
                f2,
                *f2 as u32,
                entries.len()
            );
            for (f, s) in entries.iter().take(10) {
                println!("    {}[sec{}]", f, s);
            }
            if entries.len() > 10 {
                println!("    ... and {} more", entries.len() - 10);
            }
        }
    }

    // --- HW1 non-standard trailing ---
    if !hw1_trailing.is_empty() {
        println!("\n=== HW1 Section Trailing (non-standard rigid/global/padding) ===");
        for ((r, g, p), entries) in &hw1_trailing {
            println!(
                "  rigid={} global={} padding=0x{:08X}: {} sections",
                r,
                g,
                *p as u32,
                entries.len()
            );
            for (f, s) in entries.iter().take(10) {
                println!("    {}[sec{}]", f, s);
            }
            if entries.len() > 10 {
                println!("    ... and {} more", entries.len() - 10);
            }
        }
    }

    Ok(())
}

fn collect_ugx_files(
    dir: &std::path::Path,
    out: &mut Vec<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
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
