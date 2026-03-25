//! HW1 game database CLI — validate database files and resolve the full asset
//! pipeline from ERA archives.

mod resolve;

use clap::{Parser, Subcommand};

use database_cli::assets::AssetSource;
use database_cli::load_game_dir;

#[derive(Parser)]
#[command(name = "database")]
#[command(about = "HW1 game database tool — validate and resolve game assets")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Validate all database XMB files parse correctly
    Validate {
        /// Path to the game directory containing ERA files
        #[arg(long)]
        game_dir: String,
        /// Additional ERA to layer on top (e.g. a scenario ERA)
        #[arg(long = "era")]
        era_paths: Vec<String>,
    },
    /// Resolve the full asset pipeline: objects → visuals → models/anims
    Resolve {
        /// Path to the game directory containing ERA files
        #[arg(long)]
        game_dir: Option<String>,
        /// Explicit ERA paths to load (in priority order, last = highest)
        #[arg(long = "era")]
        era_paths: Vec<String>,
        /// Print every resolved asset path (verbose)
        #[arg(short, long)]
        verbose: bool,
    },
}

/// Build an [`AssetSource`] from explicit ERA paths.
fn load_era_list(paths: &[String]) -> AssetSource {
    let mut src = AssetSource::new();
    for path in paths {
        match src.add_era(path) {
            Ok(n) => println!("  Loaded {path} ({n} entries)"),
            Err(e) => {
                eprintln!("Failed to load {path}: {e}");
                std::process::exit(1);
            }
        }
    }
    src
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::Validate {
            game_dir,
            era_paths,
        } => {
            println!("Loading ERAs from {game_dir}:");
            let mut src = load_game_dir(&game_dir);
            for path in &era_paths {
                match src.add_era(path) {
                    Ok(n) => println!("  Loaded {path} ({n} entries)"),
                    Err(e) => {
                        eprintln!("Failed to load {path}: {e}");
                        std::process::exit(1);
                    }
                }
            }
            println!();
            database_cli::validate::run(&mut src);
        }
        Commands::Resolve {
            game_dir,
            era_paths,
            verbose,
        } => {
            let mut src = if let Some(dir) = &game_dir {
                println!("Loading ERAs from {dir}:");
                load_game_dir(dir)
            } else if !era_paths.is_empty() {
                println!("Loading ERAs:");
                load_era_list(&era_paths)
            } else {
                eprintln!("Error: provide --game-dir or --era paths");
                std::process::exit(1);
            };
            println!();
            for (label, count) in src.summary() {
                println!("  {label:<24} {count} files");
            }
            println!();
            resolve::run(&mut src, verbose);
        }
    }
}
