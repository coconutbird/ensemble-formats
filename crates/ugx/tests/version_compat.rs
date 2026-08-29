//! Version-specific UGX regression and optional retail-corpus coverage.

use std::path::{Path, PathBuf};

use test_utils::prelude::*;
use ugx::{ReadOptions, Reader, UgxGeom, UgxVersion, Writer};

fn workspace_file(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

fn validate_layout(label: &str, geom: &UgxGeom, version: UgxVersion) -> Result<(), String> {
    for (section_index, section) in geom.sections.iter().enumerate() {
        let has_packer = section.base_vert_packer.is_some();
        if has_packer != version.has_embedded_packer() {
            return Err(format!(
                "{label}: section {section_index} packer presence {has_packer} does not match {version:?}"
            ));
        }

        let indices = geom
            .get_section_indices(section_index)
            .map_err(|error| format!("{label}: section {section_index} indices: {error}"))?;
        let expected_indices = usize::try_from(section.num_tris)
            .ok()
            .and_then(|triangles| triangles.checked_mul(3))
            .ok_or_else(|| format!("{label}: section {section_index} triangle count overflow"))?;
        if indices.len() != expected_indices {
            return Err(format!(
                "{label}: section {section_index} has {} indices, expected {expected_indices}",
                indices.len()
            ));
        }

        let vertices = geom
            .unpack_section_vertices(section_index)
            .map_err(|error| format!("{label}: section {section_index} vertices: {error}"))?;
        let expected_vertices = usize::try_from(section.num_verts)
            .map_err(|_| format!("{label}: section {section_index} has negative vertex count"))?;
        if vertices.len() != expected_vertices {
            return Err(format!(
                "{label}: section {section_index} has {} vertices, expected {expected_vertices}",
                vertices.len()
            ));
        }
    }
    Ok(())
}

fn assert_roundtrip_contract(source_path: &Path, source_options: ReadOptions, version: UgxVersion) {
    let source = std::fs::read(source_path).unwrap();
    assert_eq!(
        ugx::detect_version_with_options(&source, source_options).unwrap(),
        version
    );
    let original = Reader::read_with_options(&source, source_options).unwrap();
    validate_layout(&source_path.display().to_string(), &original, version).unwrap();

    let written = Writer::write(&original, version).unwrap();
    assert_eq!(ugx::detect_version(&written).unwrap(), version);
    let reread = Reader::read(&written).unwrap();
    validate_layout("written fixture", &reread, version).unwrap();

    assert_eq!(original.sections.len(), reread.sections.len());
    assert_eq!(original.materials.len(), reread.materials.len());
    assert_eq!(original.bones.len(), reread.bones.len());
    assert_eq!(original.index_buffer, reread.index_buffer);
    assert_eq!(original.vertex_buffer, reread.vertex_buffer);
    for (left, right) in original.sections.iter().zip(&reread.sections) {
        assert_eq!(left.num_verts, right.num_verts);
        assert_eq!(left.num_tris, right.num_tris);
        assert_eq!(left.vert_size, right.vert_size);
        assert_eq!(
            left.base_vert_packer
                .as_ref()
                .map(|packer| packer.pack_order.as_str()),
            right
                .base_vert_packer
                .as_ref()
                .map(|packer| packer.pack_order.as_str())
        );
    }
}

#[test]
fn checked_in_v4_and_v6_roundtrip_side_by_side() {
    let v4 = workspace_file("launcher_01.ugx");
    let v6 = workspace_file("input/mesh_magnum_01.ugx");

    assert_roundtrip_contract(&v4, ReadOptions::unchecked_checksums(), UgxVersion::Hw1);
    assert_roundtrip_contract(&v6, ReadOptions::strict(), UgxVersion::Hw2);

    let v4_geom = Reader::read_with_options(
        &std::fs::read(v4).unwrap(),
        ReadOptions::unchecked_checksums(),
    )
    .unwrap();
    assert!(v4_geom.materials.iter().all(ugx::Material::is_legacy));

    let v6_geom = Reader::read(&std::fs::read(v6).unwrap()).unwrap();
    assert!(v6_geom.materials.iter().any(ugx::Material::is_hogan));
}

#[test]
fn retail_hw2_loose_corpus_parses_as_v6() {
    let Some(game_dir) = load_game_dir("HW2_GAME_DIR") else {
        return;
    };
    let files = find_files_by_ext(&game_dir, "ugx");
    assert!(
        !files.is_empty(),
        "No UGX files found under {}",
        game_dir.display()
    );

    let mut sections = 0usize;
    let mut failures = Vec::new();
    for path in &files {
        let label = path.display().to_string();
        let data = match std::fs::read(path) {
            Ok(data) => data,
            Err(error) => {
                failures.push(format!("{label}: read: {error}"));
                continue;
            }
        };
        match Reader::read(&data) {
            Ok(geom) => {
                sections += geom.sections.len();
                if let Err(error) = validate_layout(&label, &geom, UgxVersion::Hw2) {
                    failures.push(error);
                }
            }
            Err(error) => failures.push(format!("{label}: parse: {error}")),
        }
    }

    eprintln!(
        "HW2 UGX corpus: {} v6 files, {sections} sections",
        files.len()
    );
    assert!(
        failures.is_empty(),
        "HW2 UGX failures ({}):\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn retail_hw1_era_corpus_parses_as_v4() {
    let Some(game_dir) = load_game_dir("HW1_GAME_DIR") else {
        return;
    };
    let archives = find_files_flat(&game_dir, "era");
    assert!(
        !archives.is_empty(),
        "No ERA archives found in {}",
        game_dir.display()
    );

    let mut files = 0usize;
    let mut sections = 0usize;
    let mut failures = Vec::new();
    for archive_path in &archives {
        let mut archive = match open_era(archive_path) {
            Ok(archive) => archive,
            Err(error) => {
                failures.push(format!("{}: open: {error}", archive_path.display()));
                continue;
            }
        };
        for (entry_index, filename) in find_entries_in_era(&archive, ".ugx") {
            files += 1;
            let label = format!("{}:{filename}", archive_path.display());
            let data = match archive.read_entry(entry_index) {
                Ok(data) => data,
                Err(error) => {
                    failures.push(format!("{label}: extract: {error}"));
                    continue;
                }
            };
            match Reader::read(&data) {
                Ok(geom) => {
                    sections += geom.sections.len();
                    if let Err(error) = validate_layout(&label, &geom, UgxVersion::Hw1) {
                        failures.push(error);
                    }
                }
                Err(error) => failures.push(format!("{label}: parse: {error}")),
            }
        }
    }

    eprintln!("HW1 UGX corpus: {files} v4 files, {sections} sections");
    assert!(files > 0, "No HW1 UGX entries were tested");
    assert!(
        failures.is_empty(),
        "HW1 UGX failures ({}):\n{}",
        failures.len(),
        failures.join("\n")
    );
}
