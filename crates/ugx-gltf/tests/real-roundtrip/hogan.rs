use std::path::Path;

use test_utils::prelude::*;
use ugx::{HoganMaterialData, MaterialData, UgxVersion};
use ugx_gltf::{GltfExportOptions, GltfImportOptions, export_to_gltf, import_from_gltf};

#[derive(Default)]
struct HoganStats {
    files: usize,
    named_files: usize,
    exact_materials: usize,
    mismatches: Vec<String>,
}

#[test]
fn hogan_named_constant_buffers_roundtrip() {
    let Some(game_dir) = load_game_dir("HW2_GAME_DIR") else {
        return;
    };
    let files = find_files_by_ext(&game_dir, "ugx");
    assert!(
        !files.is_empty(),
        "No UGX files found under {}",
        game_dir.display()
    );

    let mut stats = HoganStats::default();
    for path in files.iter().take(100) {
        if let Err(error) = inspect_file(path, &mut stats) {
            stats.mismatches.push(error);
        }
    }

    assert!(stats.files > 0, "No Hogan materials found in HW2 files");
    assert!(
        stats.named_files > 0,
        "No exported Hogan material contained named constant-buffer parameters"
    );
    assert!(
        stats.mismatches.is_empty(),
        "Hogan constant-buffer roundtrip failures:\n{}",
        stats.mismatches.join("\n")
    );
    eprintln!(
        "Hogan roundtrip: {} files, {} with named parameters, {} exact materials",
        stats.files, stats.named_files, stats.exact_materials
    );
}

fn inspect_file(path: &Path, stats: &mut HoganStats) -> Result<(), String> {
    let Ok(data) = std::fs::read(path) else {
        return Ok(());
    };
    let Ok(source) = ugx::Reader::read(&data) else {
        return Ok(());
    };
    if !source.materials.iter().any(ugx::Material::is_hogan) {
        return Ok(());
    }
    stats.files += 1;

    let export = export_to_gltf(
        &source,
        &GltfExportOptions {
            embed_buffers: false,
            include_materials: true,
            include_skeleton: true,
        },
    )
    .map_err(|error| format!("{}: export: {error}", path.display()))?;
    if export.json.contains("\"shader_flags\"") && export.json.contains("\"ps_cb\"") {
        stats.named_files += 1;
    }
    let result = import_from_gltf(
        &export.json,
        export.buffer.as_deref(),
        &GltfImportOptions {
            version: UgxVersion::Hw2,
            include_skeleton: true,
            include_materials: true,
            ..GltfImportOptions::default()
        },
    )
    .map_err(|error| format!("{}: import: {error}", path.display()))?;
    compare_hogan_materials(path, &source.materials, &result.materials, stats)
}

fn compare_hogan_materials(
    path: &Path,
    source: &[ugx::Material],
    result: &[ugx::Material],
    stats: &mut HoganStats,
) -> Result<(), String> {
    for (material_index, source_material) in source.iter().enumerate() {
        let MaterialData::Hogan(source_data) = &source_material.data else {
            continue;
        };
        let Some(result_material) = result.get(material_index) else {
            return Err(format!(
                "{}: Hogan material {material_index} was lost",
                path.display()
            ));
        };
        let MaterialData::Hogan(result_data) = &result_material.data else {
            return Err(format!(
                "{}: material {material_index} changed from Hogan format",
                path.display()
            ));
        };
        compare_constant_buffers(path, material_index, source_data, result_data)?;
        stats.exact_materials += 1;
    }
    Ok(())
}

fn compare_constant_buffers(
    path: &Path,
    material_index: usize,
    source: &HoganMaterialData,
    result: &HoganMaterialData,
) -> Result<(), String> {
    if source.ps_cb_data == result.ps_cb_data && source.vs_cb_data == result.vs_cb_data {
        Ok(())
    } else {
        Err(format!(
            "{}: material {material_index} constant buffers changed (PS {}->{}, VS {}->{})",
            path.display(),
            source.ps_cb_data.len(),
            result.ps_cb_data.len(),
            source.vs_cb_data.len(),
            result.vs_cb_data.len(),
        ))
    }
}
