//! `validate` subcommand — parse all database XMBs and report success/failure.

use std::time::Instant;

use crate::assets::AssetSource;

struct DbFile {
    path: &'static str,
    label: &'static str,
    parse: fn(&[u8]) -> Result<String, String>,
}

fn parse_xmb(data: &[u8]) -> Result<xmb::Document, String> {
    xmb::Reader::read(data).map_err(|e| format!("XMB parse error: {e}"))
}

pub fn run(era_path: &str) {
    let start = Instant::now();
    let mut src = AssetSource::new();
    let count = src.add_era(era_path).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1);
    });

    println!("Opened {era_path} ({count} entries)\n");

    let db_files: &[DbFile] = &[
        DbFile {
            path: "data\\objects.xml.xmb",
            label: "objects",
            parse: |d| {
                let doc = parse_xmb(d)?;
                let r = database::objects::parse(&doc).map_err(|e| format!("{e}"))?;
                Ok(format!("{} proto objects", r.len()))
            },
        },
        DbFile {
            path: "data\\squads.xml.xmb",
            label: "squads",
            parse: |d| {
                let doc = parse_xmb(d)?;
                let r = database::squads::parse(&doc).map_err(|e| format!("{e}"))?;
                Ok(format!("{} squads", r.len()))
            },
        },
        DbFile {
            path: "data\\techs.xml.xmb",
            label: "techs",
            parse: |d| {
                let doc = parse_xmb(d)?;
                let r = database::techs::parse(&doc).map_err(|e| format!("{e}"))?;
                Ok(format!("{} techs", r.len()))
            },
        },
        DbFile {
            path: "data\\abilities.xml.xmb",
            label: "abilities",
            parse: |d| {
                let doc = parse_xmb(d)?;
                let r = database::abilities::parse(&doc).map_err(|e| format!("{e}"))?;
                Ok(format!("{} abilities", r.len()))
            },
        },
        DbFile {
            path: "data\\powers.xml.xmb",
            label: "powers",
            parse: |d| {
                let doc = parse_xmb(d)?;
                let r = database::powers::parse(&doc).map_err(|e| format!("{e}"))?;
                Ok(format!("{} powers", r.len()))
            },
        },
        DbFile {
            path: "data\\civs.xml.xmb",
            label: "civs",
            parse: |d| {
                let doc = parse_xmb(d)?;
                let r = database::civs::parse(&doc).map_err(|e| format!("{e}"))?;
                Ok(format!("{} civs", r.len()))
            },
        },
        DbFile {
            path: "data\\leaders.xml.xmb",
            label: "leaders",
            parse: |d| {
                let doc = parse_xmb(d)?;
                let r = database::leaders::parse(&doc).map_err(|e| format!("{e}"))?;
                Ok(format!("{} leaders", r.len()))
            },
        },
        DbFile {
            path: "data\\weapontypes.xml.xmb",
            label: "weapontypes",
            parse: |d| {
                let doc = parse_xmb(d)?;
                let r = database::weapontypes::parse(&doc).map_err(|e| format!("{e}"))?;
                Ok(format!("{} weapon types", r.len()))
            },
        },
        DbFile {
            path: "data\\damagetypes.xml.xmb",
            label: "damagetypes",
            parse: |d| {
                let doc = parse_xmb(d)?;
                let r = database::damagetypes::parse(&doc).map_err(|e| format!("{e}"))?;
                Ok(format!("{} damage types", r.len()))
            },
        },
        DbFile {
            path: "data\\gamedata.xml.xmb",
            label: "gamedata",
            parse: |d| {
                let doc = parse_xmb(d)?;
                let g = database::gamedata::parse(&doc).map_err(|e| format!("{e}"))?;
                Ok(format!(
                    "{} resources, {} pops",
                    g.resources.len(),
                    g.pops.len()
                ))
            },
        },
    ];

    let (mut passed, mut failed, mut missing) = (0usize, 0usize, 0usize);

    for db in db_files {
        let Some(raw) = src.read(db.path) else {
            println!("  SKIP  {:<14} not found in archive", db.label);
            missing += 1;
            continue;
        };
        match (db.parse)(&raw) {
            Ok(summary) => {
                println!("  OK    {:<14} {summary}", db.label);
                passed += 1;
            }
            Err(e) => {
                println!("  FAIL  {:<14} {e}", db.label);
                failed += 1;
            }
        }
    }

    let elapsed = start.elapsed();
    println!("\n--- Summary ---");
    println!(
        "{passed} passed, {failed} failed, {missing} missing ({:.1}s)",
        elapsed.as_secs_f64()
    );
    if failed > 0 {
        std::process::exit(1);
    }
}
