//! `validate` subcommand — parse all database XMBs and report success/failure.
//!
//! Uses `bdt_serde::from_node_warned` to collect diagnostic warnings about
//! extra fields and type mismatches without aborting the parse.

use std::time::Instant;

use bdt_serde::Warning;

use crate::assets::AssetSource;

type ParseResult = Result<(String, Vec<Warning>), String>;

struct DbFile {
    path: &'static str,
    label: &'static str,
    parse: fn(&[u8]) -> ParseResult,
}

fn parse_xmb(data: &[u8]) -> Result<xmb::Document, String> {
    xmb::Reader::read(data).map_err(|e| format!("XMB parse error: {e}"))
}

/// Deserialize each child element of `root` that matches `child_name`,
/// collecting warnings across all children.
fn parse_children_warned<'de, T: serde::Deserialize<'de>>(
    doc: &xmb::Document,
    root_name: &str,
    child_name: &str,
) -> Result<(Vec<T>, Vec<Warning>), String> {
    let root = doc
        .root()
        .ok_or_else(|| "missing root element".to_string())?;
    if root.name != root_name {
        return Err(format!(
            "unexpected root: expected '{root_name}', got '{}'",
            root.name
        ));
    }
    let mut items = Vec::new();
    let mut all_warnings = Vec::new();
    for child in root.children.iter().filter(|c| c.name == child_name) {
        let (item, warnings) = bdt_serde::from_node_warned(child).map_err(|e| format!("{e}"))?;
        all_warnings.extend(warnings);
        items.push(item);
    }
    Ok((items, all_warnings))
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
                let (r, w): (Vec<database::ProtoObject>, _) =
                    parse_children_warned(&doc, "Objects", "Object")?;
                Ok((format!("{} proto objects", r.len()), w))
            },
        },
        DbFile {
            path: "data\\squads.xml.xmb",
            label: "squads",
            parse: |d| {
                let doc = parse_xmb(d)?;
                let (r, w): (Vec<database::Squad>, _) =
                    parse_children_warned(&doc, "Squads", "Squad")?;
                Ok((format!("{} squads", r.len()), w))
            },
        },
        DbFile {
            path: "data\\techs.xml.xmb",
            label: "techs",
            parse: |d| {
                let doc = parse_xmb(d)?;
                let (r, w): (Vec<database::Tech>, _) =
                    parse_children_warned(&doc, "Techs", "Tech")?;
                Ok((format!("{} techs", r.len()), w))
            },
        },
        DbFile {
            path: "data\\abilities.xml.xmb",
            label: "abilities",
            parse: |d| {
                let doc = parse_xmb(d)?;
                let (r, w): (Vec<database::Ability>, _) =
                    parse_children_warned(&doc, "Abilities", "Ability")?;
                Ok((format!("{} abilities", r.len()), w))
            },
        },
        DbFile {
            path: "data\\powers.xml.xmb",
            label: "powers",
            parse: |d| {
                let doc = parse_xmb(d)?;
                let (r, w): (Vec<database::Power>, _) =
                    parse_children_warned(&doc, "Powers", "Power")?;
                Ok((format!("{} powers", r.len()), w))
            },
        },
        DbFile {
            path: "data\\civs.xml.xmb",
            label: "civs",
            parse: |d| {
                let doc = parse_xmb(d)?;
                let (r, w): (Vec<database::Civ>, _) = parse_children_warned(&doc, "Civs", "Civ")?;
                Ok((format!("{} civs", r.len()), w))
            },
        },
        DbFile {
            path: "data\\leaders.xml.xmb",
            label: "leaders",
            parse: |d| {
                let doc = parse_xmb(d)?;
                let (r, w): (Vec<database::Leader>, _) =
                    parse_children_warned(&doc, "Leaders", "Leader")?;
                Ok((format!("{} leaders", r.len()), w))
            },
        },
        DbFile {
            path: "data\\weapontypes.xml.xmb",
            label: "weapontypes",
            parse: |d| {
                let doc = parse_xmb(d)?;
                let (r, w): (Vec<database::WeaponType>, _) =
                    parse_children_warned(&doc, "WeaponTypes", "WeaponType")?;
                Ok((format!("{} weapon types", r.len()), w))
            },
        },
        DbFile {
            path: "data\\damagetypes.xml.xmb",
            label: "damagetypes",
            parse: |d| {
                let doc = parse_xmb(d)?;
                let (r, w): (Vec<database::DamageType>, _) =
                    parse_children_warned(&doc, "DamageTypes", "DamageType")?;
                Ok((format!("{} damage types", r.len()), w))
            },
        },
        DbFile {
            path: "data\\gamedata.xml.xmb",
            label: "gamedata",
            parse: |d| {
                let doc = parse_xmb(d)?;
                let root = doc.root().ok_or_else(|| "missing root".to_string())?;
                let (g, w): (database::GameData, _) =
                    bdt_serde::from_node_warned(root).map_err(|e| format!("{e}"))?;
                Ok((
                    format!(
                        "{} resources, {} pops",
                        g.resources.as_ref().map_or(0, |r| r.entries.len()),
                        g.pops.as_ref().map_or(0, |p| p.entries.len())
                    ),
                    w,
                ))
            },
        },
    ];

    let (mut passed, mut failed, mut missing) = (0usize, 0usize, 0usize);
    let mut total_warnings = 0usize;

    for db in db_files {
        let Some(raw) = src.read(db.path) else {
            println!("  SKIP  {:<14} not found in archive", db.label);
            missing += 1;
            continue;
        };
        match (db.parse)(&raw) {
            Ok((summary, warnings)) => {
                if warnings.is_empty() {
                    println!("  OK    {:<14} {summary}", db.label);
                } else {
                    println!(
                        "  OK    {:<14} {summary}  ({} warnings)",
                        db.label,
                        warnings.len()
                    );
                    for w in &warnings {
                        println!("        ⚠ {w}");
                    }
                    total_warnings += warnings.len();
                }
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
        "{passed} passed, {failed} failed, {missing} missing, {total_warnings} warnings ({:.1}s)",
        elapsed.as_secs_f64()
    );
    if failed > 0 {
        std::process::exit(1);
    }
}
