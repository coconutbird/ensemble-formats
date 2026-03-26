//! Integration tests for HW1 database validation.
//!
//! Requires `HW1_GAME_DIR` to be set in the workspace `.env` file
//! (or as an environment variable) pointing to a Halo Wars DE installation.
//!
//! These tests are skipped when the variable is not set.

use database_cli::assets::AssetSource;
use database_cli::validate::{FileOutcome, ValidateReport};

/// Load the `.env` from the workspace root and return `HW1_GAME_DIR` if set.
fn hw1_game_dir() -> Option<String> {
    // cargo test runs from the crate dir; .env is at workspace root (../../)
    let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("could not find workspace root");
    let env_path = workspace_root.join(".env");
    let _ = dotenvy::from_path(&env_path);
    std::env::var("HW1_GAME_DIR").ok()
}

/// Build an [`AssetSource`] from the HW1 game directory using the engine's
/// ERA load order.
fn load_hw1(dir: &str) -> AssetSource {
    database_cli::load_game_dir(dir)
}

fn print_report(report: &ValidateReport) {
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
                }
            }
            FileOutcome::Failed(e) => println!("  FAIL  {:<14} {e}", f.label),
            FileOutcome::Missing => println!("  SKIP  {:<14} not found", f.label),
        }
    }
    println!(
        "\n  {} passed, {} failed, {} missing, {} warnings ({:.1}s)",
        report.passed(),
        report.failed(),
        report.missing(),
        report.total_warnings(),
        report.elapsed.as_secs_f64()
    );
}

#[test]
fn validate_base_game() {
    let Some(dir) = hw1_game_dir() else {
        eprintln!("SKIP: HW1_GAME_DIR not set");
        return;
    };

    let mut src = load_hw1(&dir);
    let report = database_cli::validate::validate(&mut src);

    print_report(&report);

    // Every file should be found (none missing)
    assert_eq!(report.missing(), 0, "some database files were not found");

    // We expect some known failures for now (objects/squads i32, techs root name)
    // but the majority should pass
    assert!(
        report.passed() >= 7,
        "expected at least 7 files to pass, got {}",
        report.passed()
    );
}

#[test]
fn validate_with_scenario_era() {
    let Some(dir) = hw1_game_dir() else {
        eprintln!("SKIP: HW1_GAME_DIR not set");
        return;
    };

    let mut src = load_hw1(&dir);

    // Layer on PHXscn01.era if it exists
    let scenario_path = format!("{dir}/PHXscn01.era");
    if !std::path::Path::new(&scenario_path).exists() {
        eprintln!("SKIP: PHXscn01.era not found at {scenario_path}");
        return;
    }
    src.add_era(&scenario_path)
        .expect("failed to load PHXscn01.era");

    let report = database_cli::validate::validate(&mut src);

    print_report(&report);

    assert_eq!(report.missing(), 0, "some database files were not found");
    assert!(
        report.passed() >= 7,
        "expected at least 7 files to pass, got {}",
        report.passed()
    );
}

#[test]
fn validate_with_dlc() {
    let Some(dir) = hw1_game_dir() else {
        eprintln!("SKIP: HW1_GAME_DIR not set");
        return;
    };

    // load_game_dir already includes DLC ERAs in the correct load order.
    let mut src = load_hw1(&dir);
    let report = database_cli::validate::validate(&mut src);

    print_report(&report);

    assert_eq!(report.missing(), 0, "some database files were not found");
    assert!(
        report.passed() >= 7,
        "expected at least 7 files to pass, got {}",
        report.passed()
    );
}

#[test]
fn debug_objects_i32_failure() {
    let Some(dir) = hw1_game_dir() else {
        eprintln!("SKIP: HW1_GAME_DIR not set");
        return;
    };

    let mut src = load_hw1(&dir);
    let raw = src
        .read("data\\objects.xml.xmb")
        .expect("objects.xml.xmb not found");
    let doc = xmb::Reader::read(&raw).expect("XMB parse failed");
    let root = doc.root().expect("no root");

    for (i, child) in root
        .children
        .iter()
        .filter(|c| c.name == "Object")
        .enumerate()
    {
        let name_attr = child
            .get_attribute("name")
            .map(|a| a.value_string())
            .unwrap_or_default();
        let result: Result<(database::ProtoObject, Vec<bdt_serde::Warning>), _> =
            bdt_serde::from_node_warned(child);
        match result {
            Ok((_, warnings)) => {
                for w in &warnings {
                    if format!("{w}").contains("i32") {
                        eprintln!("WARNING Object[{i}] name={name_attr}: {w}");
                    }
                }
            }
            Err(e) => {
                eprintln!("FAIL Object[{i}] name={name_attr}: {e}");
                for attr in &child.attributes {
                    eprintln!("  @{} = {:?}", attr.name, attr.value);
                }
                // Show all children
                for ch in &child.children {
                    eprintln!(
                        "  <{}> text={:?} attrs={:?}",
                        ch.name,
                        ch.text,
                        ch.attributes
                            .iter()
                            .map(|a| format!("@{}={:?}", a.name, a.value))
                            .collect::<Vec<_>>()
                    );
                }
                return;
            }
        }
    }
    eprintln!("All objects parsed OK");

    // Collect unique extra field warnings from objects
    let raw2 = src
        .read("data\\objects.xml.xmb")
        .expect("objects.xml.xmb not found");
    let doc2 = xmb::Reader::read(&raw2).expect("XMB parse failed");
    let root2 = doc2.root().expect("no root");
    let mut extra_fields: std::collections::BTreeMap<String, usize> =
        std::collections::BTreeMap::new();
    for child in root2.children.iter().filter(|c| c.name == "Object") {
        let result: Result<(database::ProtoObject, Vec<bdt_serde::Warning>), _> =
            bdt_serde::from_node_warned(child);
        if let Ok((_, warnings)) = result {
            for w in &warnings {
                if let bdt_serde::Warning::ExtraField { field, .. } = w {
                    *extra_fields.entry(field.clone()).or_insert(0) += 1;
                }
            }
        }
    }
    eprintln!("\n=== Extra fields in objects.xml.xmb (unique field name : count) ===");
    for (field, count) in &extra_fields {
        eprintln!("  {field:<40} {count}x");
    }
    eprintln!("  ({} unique extra fields)", extra_fields.len());

    // Helper to collect extra fields from any file
    fn collect_extras<T: serde::de::DeserializeOwned + Default>(
        src: &mut AssetSource,
        path: &str,
        _root_name: &str,
        child_name: &str,
        label: &str,
    ) {
        let raw = src.read(path).unwrap_or_else(|| panic!("{path} not found"));
        let doc = xmb::Reader::read(&raw).expect("XMB parse failed");
        let root = doc.root().expect("no root");
        let mut extra_fields: std::collections::BTreeMap<String, usize> =
            std::collections::BTreeMap::new();
        let mut total_warnings = 0usize;
        for child in root.children.iter().filter(|c| c.name == child_name) {
            let result: Result<(T, Vec<bdt_serde::Warning>), _> =
                bdt_serde::from_node_warned(child);
            if let Ok((_, warnings)) = result {
                total_warnings += warnings.len();
                for w in &warnings {
                    if let bdt_serde::Warning::ExtraField { field, element } = w {
                        let key = format!("{field} in <{element}>");
                        *extra_fields.entry(key).or_insert(0) += 1;
                    }
                }
            }
        }
        if extra_fields.is_empty() && total_warnings == 0 {
            return;
        }
        eprintln!("\n=== Extra fields in {label} ({total_warnings} total warnings) ===");
        for (field, count) in &extra_fields {
            eprintln!("  {field:<40} {count}x");
        }
        eprintln!("  ({} unique extra fields)", extra_fields.len());
    }

    collect_extras::<database::Squad>(
        &mut src,
        "data\\squads.xml.xmb",
        "Squads",
        "Squad",
        "squads.xml",
    );
    collect_extras::<database::Tech>(
        &mut src,
        "data\\techs.xml.xmb",
        "TechTree",
        "Tech",
        "techs.xml",
    );
    collect_extras::<database::Ability>(
        &mut src,
        "data\\abilities.xml.xmb",
        "Abilities",
        "Ability",
        "abilities.xml",
    );
    collect_extras::<database::Power>(
        &mut src,
        "data\\powers.xml.xmb",
        "Powers",
        "Power",
        "powers.xml",
    );
    collect_extras::<database::Civ>(&mut src, "data\\civs.xml.xmb", "Civs", "Civ", "civs.xml");
    collect_extras::<database::Leader>(
        &mut src,
        "data\\leaders.xml.xmb",
        "Leaders",
        "Leader",
        "leaders.xml",
    );
    collect_extras::<database::WeaponType>(
        &mut src,
        "data\\weapontypes.xml.xmb",
        "WeaponTypes",
        "WeaponType",
        "weapontypes.xml",
    );
    collect_extras::<database::DamageType>(
        &mut src,
        "data\\damagetypes.xml.xmb",
        "DamageTypes",
        "DamageType",
        "damagetypes.xml",
    );
}

#[test]
fn list_xmbs_in_extra_eras() {
    let Some(dir) = hw1_game_dir() else {
        eprintln!("SKIP: HW1_GAME_DIR not set");
        return;
    };

    let core: std::collections::HashSet<&str> = [
        "root.era",
        "root_update.era",
        "locale.era",
        "locale_update.era",
        "scenarioshared.era",
        "dlc01.era",
        "dlc02.era",
    ]
    .into_iter()
    .collect();

    let mut eras: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if name.ends_with(".era") && !core.contains(name.as_str()) {
                Some(name)
            } else {
                None
            }
        })
        .collect();
    eras.sort();

    for era_name in &eras {
        let path = format!("{dir}/{era_name}");
        let mut src = AssetSource::new();
        src.add_era(&path).unwrap();

        let xmbs: Vec<&str> = src
            .files_per_archive()
            .into_iter()
            .flat_map(|(_, files)| files)
            .filter(|f| f.ends_with(".xmb"))
            .collect();

        if !xmbs.is_empty() {
            eprintln!("\n=== {era_name} ({} xmbs) ===", xmbs.len());
            for x in &xmbs {
                eprintln!("  {x}");
            }
        }
    }
}
