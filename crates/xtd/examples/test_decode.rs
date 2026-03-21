//! Test XTD vertex decoding with a real file.

use byteorder::{BigEndian, ByteOrder};
use std::env;
use xtd::XtdReader;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    let path = args
        .get(1)
        .map(|s| s.as_str())
        .unwrap_or("test_extract/scenario/skirmish/design/blood_gulch/blood_gulch.xtd");

    println!("Loading XTD file: {}", path);
    let data = std::fs::read(path)?;
    println!("File size: {} bytes", data.len());

    // Debug: print first 64 bytes of atlas data in hex
    let _atlas_offset = 0x8888; // Approximate - we'll get exact from the file

    let file = XtdReader::read(&data)?;
    println!("\n=== XTD Header ===");
    println!("  Version: 0x{:04X}", file.header.version);
    println!("  NumXVerts: {}", file.header.num_x_verts);
    println!("  NumXChunks: {}", file.header.num_x_chunks);
    println!("  TileScale: {}", file.header.tile_scale);
    println!("  WorldMin: {:?}", file.header.world_min);
    println!("  WorldMax: {:?}", file.header.world_max);
    println!("\n  Visual chunks: {}", file.visual_chunks.len());
    println!("  Atlas data: {} bytes", file.atlas_data.len());

    println!("\n=== Decoding Vertices ===");
    let vertices = file.decode_vertices()?;

    println!("  Atlas Header:");
    println!("    Mid: {:?}", vertices.header.mid);
    println!("    Range: {:?}", vertices.header.range);

    // Debug: print first few raw packed values
    println!("\n  Raw atlas data (first 64 bytes after header):");
    let atlas_data = &file.atlas_data;
    print!("    Header bytes: ");
    for byte in atlas_data.iter().take(32) {
        print!("{:02x} ", byte);
    }
    println!();

    // Print first few packed positions (as u32 in hex and decimal)
    println!("\n  First 8 packed positions (at offset 32):");
    for i in 0..8 {
        let offset = 32 + i * 4;
        if offset + 4 <= atlas_data.len() {
            let packed_be = BigEndian::read_u32(&atlas_data[offset..offset + 4]);
            let x = (packed_be >> 22) & 0x3FF;
            let y = (packed_be >> 11) & 0x3FF;
            let z = packed_be & 0x3FF;
            println!(
                "    [{}] 0x{:08x} -> X={:4} Y={:4} Z={:4}",
                i, packed_be, x, y, z
            );
        }
    }

    // Check if first 1024 positions might represent row 0
    println!("\n  Checking if data is row-major (first 8 vs positions 1024-1031):");
    for i in 0..4 {
        let offset0 = 32 + i * 4;
        let offset1 = 32 + (i + 1024) * 4;
        if offset1 + 4 <= atlas_data.len() {
            let p0 = BigEndian::read_u32(&atlas_data[offset0..offset0 + 4]);
            let p1 = BigEndian::read_u32(&atlas_data[offset1..offset1 + 4]);
            println!(
                "    Row 0 col {}: 0x{:08x}   Row 1 col {}: 0x{:08x}",
                i, p0, i, p1
            );
        }
    }
    println!(
        "\n  Terrain grid: {}x{}",
        vertices.num_verts_per_axis, vertices.num_verts_per_axis
    );
    println!("  Total vertices: {}", vertices.positions.len());

    // Print first few vertices
    println!("\n  First 10 vertices:");
    for i in 0..10.min(vertices.positions.len()) {
        let pos = vertices.positions[i];
        let norm = vertices.normals[i];
        let norm_len = (norm[0] * norm[0] + norm[1] * norm[1] + norm[2] * norm[2]).sqrt();
        println!(
            "    [{:5}] pos=({:8.2}, {:8.2}, {:8.2}) norm=({:6.3}, {:6.3}, {:6.3}) |n|={:.3}",
            i, pos[0], pos[1], pos[2], norm[0], norm[1], norm[2], norm_len
        );
    }

    // Print vertices at specific grid positions to check layout
    let n = vertices.num_verts_per_axis;
    println!("\n  Corner vertices (expecting X~gridX, Z~gridZ):");
    let corners = [(0, 0), (0, n - 1), (n - 1, 0), (n - 1, n - 1), (512, 512)];
    for (x, z) in corners {
        let i = z * n + x;
        let pos = vertices.positions[i];
        let norm = vertices.normals[i];
        let norm_len = (norm[0] * norm[0] + norm[1] * norm[1] + norm[2] * norm[2]).sqrt();
        println!(
            "    Grid ({:4},{:4}) -> pos=({:8.2}, {:8.2}, {:8.2}) expected~({:4},{:4}) |n|={:.3}",
            x, z, pos[0], pos[1], pos[2], x, z, norm_len
        );
    }

    // Position bounds
    let mut min_pos = [f32::MAX; 3];
    let mut max_pos = [f32::MIN; 3];
    for pos in &vertices.positions {
        for j in 0..3 {
            min_pos[j] = min_pos[j].min(pos[j]);
            max_pos[j] = max_pos[j].max(pos[j]);
        }
    }
    println!("\n  Position bounds:");
    println!(
        "    Min: ({:.2}, {:.2}, {:.2})",
        min_pos[0], min_pos[1], min_pos[2]
    );
    println!(
        "    Max: ({:.2}, {:.2}, {:.2})",
        max_pos[0], max_pos[1], max_pos[2]
    );

    // Generate indices
    let indices = vertices.generate_indices();
    println!(
        "\n  Generated {} indices ({} triangles)",
        indices.len(),
        indices.len() / 3
    );

    // Validate normals
    let mut bad_normals = 0;
    for (i, norm) in vertices.normals.iter().enumerate() {
        let len = (norm[0] * norm[0] + norm[1] * norm[1] + norm[2] * norm[2]).sqrt();
        if (len - 1.0).abs() > 0.15 {
            bad_normals += 1;
            if bad_normals <= 5 {
                println!(
                    "  WARNING: Normal {} not normalized: {:?} (len={})",
                    i, norm, len
                );
            }
        }
    }
    if bad_normals > 5 {
        println!("  ... and {} more bad normals", bad_normals - 5);
    }

    println!("\n=== Summary ===");
    println!("  ✓ XTD file parsed successfully");
    println!("  ✓ {} vertices decoded", vertices.positions.len());
    println!("  ✓ {} indices generated", indices.len());
    if bad_normals == 0 {
        println!("  ✓ All normals are normalized");
    } else {
        println!(
            "  ⚠ {}/{} normals have unusual length",
            bad_normals,
            vertices.normals.len()
        );
    }

    Ok(())
}
