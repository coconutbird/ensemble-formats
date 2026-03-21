use era::EraArchive;
use std::env;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    let era_path = args
        .get(1)
        .map(|s| s.as_str())
        .unwrap_or("/Users/dev/Documents/steamcmd/halo_wars/root.era");

    println!("Opening ERA: {}", era_path);
    let archive = EraArchive::open(era_path)?;

    println!("\nAll files containing 'terrain', 'shader', or '.bin':\n");

    for (i, entry) in archive.iter().enumerate() {
        if let Some(name) = &entry.filename {
            let lower = name.to_lowercase();
            if lower.contains("terrain") || lower.contains("shader") || lower.ends_with(".bin") {
                println!("[{:4}] {}", i, name);
            }
        }
    }

    Ok(())
}
