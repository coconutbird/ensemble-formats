//! Dump UGX file structure for debugging.

use ecf::EcfReader;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: dump_ugx <file.ugx>");
        std::process::exit(1);
    }

    let data = std::fs::read(&args[1])?;
    let ecf = EcfReader::new(&data)?;

    println!("=== ECF Header ===");
    let header = ecf.header();
    println!("  File ID: 0x{:08X}", header.id);
    println!("  Num chunks: {}", header.num_chunks);
    println!("  Flags: 0x{:04X}", header.flags);

    println!("\n=== Chunks ===");
    for (i, chunk) in ecf.chunks().iter().enumerate() {
        println!(
            "Chunk {}: ID=0x{:08X} offset=0x{:X} size=0x{:X} flags=0x{:02X} resource_flags=0x{:04X}",
            i, chunk.id, chunk.offset, chunk.size, chunk.flags, chunk.resource_flags
        );
    }

    // Read and dump cached data chunk
    let cached_data = ecf.chunk_data_by_id(0x700)?;
    println!(
        "\n=== Cached Data (0x700) - {} bytes ===",
        cached_data.len()
    );
    hexdump(&cached_data, 256);

    // Read IB chunk
    let ib_data = ecf.chunk_data_by_id(0x701)?;
    println!("\n=== Index Buffer (0x701) - {} bytes ===", ib_data.len());
    println!("  {} indices", ib_data.len() / 2);
    hexdump(&ib_data, 64);

    // Read VB chunk
    let vb_data = ecf.chunk_data_by_id(0x702)?;
    println!("\n=== Vertex Buffer (0x702) - {} bytes ===", vb_data.len());
    hexdump(&vb_data, 64);

    // Read material chunk if it exists
    if let Ok(mat_data) = ecf.chunk_data_by_id(0x704) {
        println!("\n=== Materials (0x704) - {} bytes ===", mat_data.len());
        hexdump(&mat_data, 256);
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
