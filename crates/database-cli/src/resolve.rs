//! `resolve` subcommand — walk the full HW1 asset resolution pipeline.
//!
//! Pipeline: objects.xml → .vis.xmb → .ugx/.uax
//!           objects.xml → .tactics.xmb
//!           objects.xml → .physics.xmb → .blueprint.xmb → .shp.xmb
//!
//! Path resolution rules (from engine RE):
//!   Visual field  "foo\bar\baz.vis"     → ERA path "art\foo\bar\baz.vis.xmb"
//!   Asset file    "foo\bar\model"       → ERA path "art\foo\bar\model.ugx" (Model type)
//!                                       → ERA path "art\foo\bar\model.uax" (Anim type)
//!   Tactics field "unit_name"           → ERA path "data\tactics\unit_name.tactics.xmb"
//!   PhysicsInfo   "name"               → ERA path "physics\name.physics.xmb"
//!   Blueprint     "name"               → ERA path "physics\name.blueprint.xmb"
//!   Shape         "name"               → ERA path "physics\name.shp.xmb"

use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use database_cli::assets::AssetSource;

/// Collected asset manifest from the full resolution pipeline.
#[derive(Default)]
struct Manifest {
    objects_total: usize,
    objects_with_visual: usize,
    objects_with_tactics: usize,
    objects_with_physics: usize,
    visuals_resolved: usize,
    visuals_failed: Vec<String>,
    tactics_resolved: usize,
    tactics_failed: Vec<String>,
    physics_resolved: usize,
    physics_failed: Vec<String>,
    blueprints_resolved: usize,
    shapes_resolved: usize,
    model_refs: BTreeSet<String>,
    anim_refs: BTreeSet<String>,
    damage_model_refs: BTreeSet<String>,
    models_found: usize,
    models_missing: Vec<String>,
    anims_found: usize,
    anims_missing: Vec<String>,
    object_assets: BTreeMap<String, ObjectAssets>,
}

#[derive(Default)]
struct ObjectAssets {
    visual_path: Option<String>,
    tactics_path: Option<String>,
    physics_path: Option<String>,
    models: Vec<String>,
    anims: Vec<String>,
}

pub fn run(src: &mut AssetSource, verbose: bool) {
    let start = Instant::now();

    // 1. Load the object database
    let objects = {
        let doc = src
            .read_xmb("data\\objects.xml.xmb")
            .expect("Failed to load objects.xml.xmb");
        database::objects::parse(&doc).expect("Failed to parse objects.xml.xmb")
    };

    let mut m = Manifest {
        objects_total: objects.len(),
        ..Default::default()
    };
    println!("Loaded {} proto objects\n", objects.len());

    // 2. Walk each object and resolve references
    println!("Resolving asset pipeline...");
    for obj in &objects {
        let mut oa = ObjectAssets::default();

        // --- Visual chain: object → .vis.xmb → .ugx/.uax ---
        if let Some(vis_ref) = &obj.visual {
            m.objects_with_visual += 1;
            let vis_era = format!("art\\{}.xmb", vis_ref.replace('/', "\\"));
            oa.visual_path = Some(vis_era.clone());

            if let Some(vis_doc) = src.read_xmb(&vis_era) {
                match database::visual::parse(&vis_doc) {
                    Ok(vis) => {
                        m.visuals_resolved += 1;
                        collect_visual_assets(&vis, &mut m, &mut oa);
                    }
                    Err(e) => m.visuals_failed.push(format!("{vis_era}: {e}")),
                }
            } else {
                m.visuals_failed.push(vis_era);
            }
        }

        // --- Tactics chain ---
        if let Some(tac_ref) = &obj.tactics {
            m.objects_with_tactics += 1;
            let tac_era = format!("data\\tactics\\{}.xmb", tac_ref);
            oa.tactics_path = Some(tac_era.clone());

            if let Some(tac_doc) = src.read_xmb(&tac_era) {
                match database::tactics::parse(&tac_doc) {
                    Ok(_) => m.tactics_resolved += 1,
                    Err(e) => m.tactics_failed.push(format!("{tac_era}: {e}")),
                }
            } else {
                m.tactics_failed.push(tac_era);
            }
        }

        // --- Physics chain: .physics.xmb → .blueprint.xmb → .shp.xmb ---
        if let Some(phys_ref) = &obj.physics_info {
            m.objects_with_physics += 1;
            let phys_era = format!("physics\\{}.physics.xmb", phys_ref);
            oa.physics_path = Some(phys_era.clone());

            if let Some(phys_doc) = src.read_xmb(&phys_era) {
                match database::physics::parse_physics(&phys_doc) {
                    Ok(phys) => {
                        m.physics_resolved += 1;
                        if let Some(bp_ref) = &phys.blueprint {
                            let bp_era = format!("physics\\{}.blueprint.xmb", bp_ref);
                            if let Some(bp_doc) = src.read_xmb(&bp_era)
                                && let Ok(bp) = database::physics::parse_blueprint(&bp_doc)
                            {
                                m.blueprints_resolved += 1;
                                if let Some(shp_ref) = &bp.shape {
                                    let shp_era = format!("physics\\{}.shp.xmb", shp_ref);
                                    if src.read_xmb(&shp_era).is_some() {
                                        m.shapes_resolved += 1;
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => m.physics_failed.push(format!("{phys_era}: {e}")),
                }
            } else {
                m.physics_failed.push(phys_era);
            }
        }

        if verbose {
            m.object_assets.insert(obj.name.clone(), oa);
        }
    }

    // 3. Verify binary assets exist in loaded archives
    println!("\nVerifying binary assets...");
    for model_path in &m.model_refs {
        if src.exists(model_path) {
            m.models_found += 1;
        } else {
            m.models_missing.push(model_path.clone());
        }
    }
    for anim_path in &m.anim_refs {
        if src.exists(anim_path) {
            m.anims_found += 1;
        } else {
            m.anims_missing.push(anim_path.clone());
        }
    }

    // 4. Print verbose per-object breakdown
    if verbose {
        println!("\n=== Per-Object Asset Breakdown ===\n");
        for (name, oa) in &m.object_assets {
            if oa.visual_path.is_none() && oa.tactics_path.is_none() && oa.physics_path.is_none() {
                continue;
            }
            println!("{name}:");
            if let Some(v) = &oa.visual_path {
                println!("  visual:  {v}");
            }
            if let Some(t) = &oa.tactics_path {
                println!("  tactics: {t}");
            }
            if let Some(p) = &oa.physics_path {
                println!("  physics: {p}");
            }
            for model in &oa.models {
                println!("    model: {model}");
            }
            for anim in &oa.anims {
                println!("    anim:  {anim}");
            }
        }
    }

    // 5. Print summary
    let elapsed = start.elapsed();
    print_summary(&m, verbose, elapsed);
}

fn print_summary(m: &Manifest, verbose: bool, elapsed: std::time::Duration) {
    println!("\n=== Asset Resolution Summary ===\n");
    println!("Proto Objects:     {}", m.objects_total);
    println!("  with visual:     {}", m.objects_with_visual);
    println!("  with tactics:    {}", m.objects_with_tactics);
    println!("  with physics:    {}", m.objects_with_physics);
    println!();
    println!(
        "Visuals resolved:  {} / {} ({} failed)",
        m.visuals_resolved,
        m.objects_with_visual,
        m.visuals_failed.len()
    );
    println!(
        "Tactics resolved:  {} / {} ({} failed)",
        m.tactics_resolved,
        m.objects_with_tactics,
        m.tactics_failed.len()
    );
    println!(
        "Physics resolved:  {} / {} ({} failed)",
        m.physics_resolved,
        m.objects_with_physics,
        m.physics_failed.len()
    );
    println!("Blueprints:        {}", m.blueprints_resolved);
    println!("Shapes:            {}", m.shapes_resolved);
    println!();
    println!("Unique model refs: {} (.ugx)", m.model_refs.len());
    println!("Unique anim refs:  {} (.uax)", m.anim_refs.len());
    println!("Damage model refs: {}", m.damage_model_refs.len());
    println!();
    println!("Binary asset verification:");
    println!(
        "  Models found:    {} / {}",
        m.models_found,
        m.model_refs.len()
    );
    println!("  Models missing:  {}", m.models_missing.len());
    println!(
        "  Anims found:     {} / {}",
        m.anims_found,
        m.anim_refs.len()
    );
    println!("  Anims missing:   {}", m.anims_missing.len());
    if verbose && !m.models_missing.is_empty() {
        println!("\n  Missing models:");
        for p in &m.models_missing {
            println!("    {p}");
        }
    }
    if verbose && !m.anims_missing.is_empty() {
        println!("\n  Missing anims:");
        for p in &m.anims_missing {
            println!("    {p}");
        }
    }

    if !m.visuals_failed.is_empty() || !m.tactics_failed.is_empty() || !m.physics_failed.is_empty()
    {
        println!("\n=== Failures ===");
        for (label, list) in [
            ("Visual", &m.visuals_failed),
            ("Tactics", &m.tactics_failed),
            ("Physics", &m.physics_failed),
        ] {
            if !list.is_empty() {
                println!("\n{label} failures ({}):", list.len());
                for f in list {
                    println!("  {f}");
                }
            }
        }
    }

    println!("\nCompleted in {:.1}s", elapsed.as_secs_f64());
}

/// Collect all model/anim asset references from a parsed visual.
fn collect_visual_assets(vis: &database::visual::Visual, m: &mut Manifest, oa: &mut ObjectAssets) {
    for model in &vis.models {
        if let Some(comp) = &model.component {
            for asset in &comp.assets {
                register_asset(asset, m, oa);
            }
            if let Some(logic) = &comp.logic {
                for entry in &logic.entries {
                    if let Some(asset) = &entry.asset {
                        register_asset(asset, m, oa);
                    }
                }
            }
        }
        for anim in &model.anims {
            for asset in &anim.assets {
                register_asset(asset, m, oa);
            }
        }
    }
}

fn register_asset(asset: &database::visual::Asset, m: &mut Manifest, oa: &mut ObjectAssets) {
    if let Some(file) = &asset.file {
        let normalized = file.replace('/', "\\");
        match asset.asset_type.as_str() {
            "Model" => {
                let p = format!("art\\{normalized}.ugx");
                m.model_refs.insert(p.clone());
                oa.models.push(p);
            }
            "Anim" => {
                let p = format!("art\\{normalized}.uax");
                m.anim_refs.insert(p.clone());
                oa.anims.push(p);
            }
            _ => {}
        }
    }
    if let Some(dmg) = &asset.damage_file {
        let p = format!("art\\{}.ugx", dmg.replace('/', "\\"));
        m.damage_model_refs.insert(p);
    }
}
