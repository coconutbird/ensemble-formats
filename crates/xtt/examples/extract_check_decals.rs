use std::env;
use std::fs;
use xtt::Reader;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        println!("Usage: {} <xtt_file>", args[0]);
        return Ok(());
    }

    let xtt_path = &args[1];
    println!("Opening XTT: {}", xtt_path);

    let data = fs::read(xtt_path)?;
    let xtt = Reader::read(&data)?;

    println!("\nXTT Header:");
    println!("  num_active_decals: {}", xtt.header.num_active_decals);
    println!(
        "  num_active_decal_instances: {}",
        xtt.header.num_active_decal_instances
    );

    if !xtt.active_decals.is_empty() {
        println!("\nActive decals:");
        for (i, d) in xtt.active_decals.iter().enumerate() {
            println!("  [{}] {}", i, d.filename);
        }
    }

    // Check linkers for decal layers
    let chunks_with_decals: usize = xtt
        .linkers
        .iter()
        .filter(|l| l.num_decal_layers > 0)
        .count();
    println!(
        "\nChunks with decal layers: {} / {}",
        chunks_with_decals,
        xtt.linkers.len()
    );

    Ok(())
}
