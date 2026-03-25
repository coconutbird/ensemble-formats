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

    // Also check squads
    let raw = src
        .read("data\\squads.xml.xmb")
        .expect("squads.xml.xmb not found");
    let doc = xmb::Reader::read(&raw).expect("XMB parse failed");
    let root = doc.root().expect("no root");

    for (i, child) in root
        .children
        .iter()
        .filter(|c| c.name == "Squad")
        .enumerate()
    {
        let name_attr = child
            .get_attribute("name")
            .map(|a| a.value_string())
            .unwrap_or_default();
        let result: Result<(database::Squad, Vec<bdt_serde::Warning>), _> =
            bdt_serde::from_node_warned(child);
        match result {
            Ok(_) => {}
            Err(e) => {
                eprintln!("FAIL Squad[{i}] name={name_attr}: {e}");
                for attr in &child.attributes {
                    eprintln!("  @{} = {:?}", attr.name, attr.value);
                }
                for ch in &child.children {
                    eprintln!("  <{}> text={:?}", ch.name, ch.text);
                }
                return;
            }
        }
    }
    eprintln!("All squads parsed OK");
}
