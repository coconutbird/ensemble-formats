use std::env;
use std::fs;
use xtt::XttReader;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    let xtt_path = args
        .get(1)
        .unwrap_or(&"test_extract/scenario/skirmish/design/blood_gulch/blood_gulch.xtt".to_string())
        .clone();

    println!("Opening XTT: {}", xtt_path);

    let data = fs::read(&xtt_path)?;
    let xtt = XttReader::read(&data)?;

    println!("\nRoad data size: {} bytes", xtt.road_data.len());

    println!("\n=== FOLIAGE DATA ===");
    println!("Foliage sets: {}", xtt.foliage.sets.len());
    for (i, set) in xtt.foliage.sets.iter().enumerate() {
        println!("  [{}] {}", i, set.filename);
    }

    println!("\nFoliage QN chunks: {}", xtt.foliage.qn_chunks.len());

    // Print first few QN chunks
    for (i, qn) in xtt.foliage.qn_chunks.iter().take(5).enumerate() {
        println!("\n  QN Chunk [{}]:", i);
        println!("    Parent index: {}", qn.qn_parent_index);
        println!("    Num sets: {}", qn.num_sets);
        println!("    Set indices: {:?}", qn.set_indices);
        println!("    Poly counts: {:?}", qn.set_poly_counts);
        for (j, buf) in qn.index_buffers.iter().enumerate() {
            println!("    Index buffer [{}]: {} bytes", j, buf.len());
        }
    }

    Ok(())
}
