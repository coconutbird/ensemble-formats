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

/// Result of validating a single database file.
pub struct FileResult {
    pub label: String,
    pub outcome: FileOutcome,
}

/// Outcome for a single database file.
pub enum FileOutcome {
    /// Parsed successfully with a summary string and optional warnings.
    Ok {
        summary: String,
        warnings: Vec<Warning>,
    },
    /// Parse or type error.
    Failed(String),
    /// File not found in the asset source.
    Missing,
}

/// Aggregate result of a full validation run.
pub struct ValidateReport {
    pub files: Vec<FileResult>,
    pub elapsed: std::time::Duration,
}

impl ValidateReport {
    pub fn passed(&self) -> usize {
        self.files
            .iter()
            .filter(|f| matches!(f.outcome, FileOutcome::Ok { .. }))
            .count()
    }

    pub fn failed(&self) -> usize {
        self.files
            .iter()
            .filter(|f| matches!(f.outcome, FileOutcome::Failed(_)))
            .count()
    }

    pub fn missing(&self) -> usize {
        self.files
            .iter()
            .filter(|f| matches!(f.outcome, FileOutcome::Missing))
            .count()
    }

    pub fn total_warnings(&self) -> usize {
        self.files
            .iter()
            .map(|f| match &f.outcome {
                FileOutcome::Ok { warnings, .. } => warnings.len(),
                _ => 0,
            })
            .sum()
    }
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

fn db_files() -> Vec<DbFile> {
    vec![
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
                    parse_children_warned(&doc, "TechTree", "Tech")?;
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
    ]
}

/// Run validation and return structured results.
pub fn validate(src: &mut AssetSource) -> ValidateReport {
    let start = Instant::now();
    let mut files = Vec::new();

    for db in &db_files() {
        let outcome = match src.read(db.path) {
            Some(raw) => match (db.parse)(&raw) {
                Ok((summary, warnings)) => FileOutcome::Ok { summary, warnings },
                Err(e) => FileOutcome::Failed(e),
            },
            None => FileOutcome::Missing,
        };
        files.push(FileResult {
            label: db.label.to_string(),
            outcome,
        });
    }

    ValidateReport {
        files,
        elapsed: start.elapsed(),
    }
}

/// Run validation, print results to stdout, and exit on failure.
pub fn run(src: &mut AssetSource) {
    let report = validate(src);

    for f in &report.files {
        match &f.outcome {
            FileOutcome::Ok { summary, warnings } => {
                if warnings.is_empty() {
                    println!("  OK    {:<14} {summary}", f.label);
                } else {
                    println!(
                        "  OK    {:<14} {summary}  ({} warnings)",
                        f.label,
                        warnings.len()
                    );
                    for w in warnings {
                        println!("        ⚠ {w}");
                    }
                }
            }
            FileOutcome::Failed(e) => {
                println!("  FAIL  {:<14} {e}", f.label);
            }
            FileOutcome::Missing => {
                println!("  SKIP  {:<14} not found in archive", f.label);
            }
        }
    }

    let elapsed = report.elapsed;
    println!("\n--- Summary ---");
    println!(
        "{} passed, {} failed, {} missing, {} warnings ({:.1}s)",
        report.passed(),
        report.failed(),
        report.missing(),
        report.total_warnings(),
        elapsed.as_secs_f64()
    );

    if report.failed() > 0 {
        std::process::exit(1);
    }
}
