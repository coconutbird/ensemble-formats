//! Rebuild derived-data tests using real game files.

use std::path::{Path, PathBuf};

use test_utils::prelude::*;
use ugx::{AABB, UgxGeom, UgxVersion};

#[test]
fn rebuild_matches_original_derived_data() {
    let tested = candidate_paths()
        .iter()
        .filter(|path| verify_path(path))
        .count();

    if tested == 0 {
        eprintln!("No real UGX fixtures found; skipping the rebuild comparison");
    }
}

fn candidate_paths() -> Vec<PathBuf> {
    let mut paths = vec![
        PathBuf::from("../../foxcannon01/mesh_turret_0.ugx"),
        PathBuf::from("../../foxcannon01/mesh_barrel_0.ugx"),
        PathBuf::from("../../foxcannon01/mesh_foxcannon01.ugx"),
        PathBuf::from("../../test_ugx/art/covenant/air/banshee_01/banshee_damage_01.ugx"),
    ];
    paths.extend(find_files_by_ext(
        Path::new("../../test_ugx_rebuild"),
        "ugx",
    ));
    paths
}

fn verify_path(path: &Path) -> bool {
    let Ok(data) = std::fs::read(path) else {
        return false;
    };
    let Ok(original) = ugx::Reader::read(&data) else {
        return false;
    };

    let mut rebuilt = stripped_clone(&original);
    rebuilt
        .rebuild_derived_data()
        .expect("valid source geometry should rebuild");

    assert_bounds(path, &original, &rebuilt);
    assert_bone_bounds(path, &original, &rebuilt);
    assert_eq!(
        rebuilt.instance_index_multiplier,
        original.instance_index_multiplier,
        "{}: instance-index multiplier mismatch",
        path.display(),
    );
    assert_tree(path, &original, &rebuilt);
    assert_accessory_coverage(path, &rebuilt);
    assert_writable(&original, &rebuilt);
    true
}

fn stripped_clone(original: &UgxGeom) -> UgxGeom {
    let mut rebuilt = original.clone();
    rebuilt.bone_bounds = original.bones.iter().map(|_| AABB::default()).collect();
    rebuilt.accessories.clear();
    rebuilt.valid_accessories.clear();
    rebuilt.aabb_tree = None;
    rebuilt
}

fn assert_bounds(path: &Path, original: &UgxGeom, rebuilt: &UgxGeom) {
    for axis in 0..3 {
        let tolerance =
            (original.bounds.max[axis] - original.bounds.min[axis]).abs() * 0.005 + 0.01;
        assert!(
            (rebuilt.bounds.min[axis] - original.bounds.min[axis]).abs() < tolerance,
            "{}: bounds.min[{axis}] mismatch: rebuilt={} original={} tolerance={tolerance}",
            path.display(),
            rebuilt.bounds.min[axis],
            original.bounds.min[axis],
        );
        assert!(
            (rebuilt.bounds.max[axis] - original.bounds.max[axis]).abs() < tolerance,
            "{}: bounds.max[{axis}] mismatch: rebuilt={} original={} tolerance={tolerance}",
            path.display(),
            rebuilt.bounds.max[axis],
            original.bounds.max[axis],
        );
    }

    let original_radius = original.bounding_sphere.radius;
    let rebuilt_radius = rebuilt.bounding_sphere.radius;
    let percentage_error = if original_radius.abs() > f32::EPSILON {
        ((rebuilt_radius - original_radius) / original_radius * 100.0).abs()
    } else {
        0.0
    };
    assert!(
        percentage_error < 1.0,
        "{}: sphere radius mismatch: rebuilt={rebuilt_radius} original={original_radius} ({percentage_error:.1}% difference)",
        path.display(),
    );
}

fn assert_bone_bounds(path: &Path, original: &UgxGeom, rebuilt: &UgxGeom) {
    assert_eq!(
        rebuilt.bone_bounds.len(),
        original.bone_bounds.len(),
        "{}: bone-bounds count mismatch",
        path.display(),
    );
    for (bone_index, (rebuilt_bounds, original_bounds)) in rebuilt
        .bone_bounds
        .iter()
        .zip(&original.bone_bounds)
        .enumerate()
    {
        if original_bounds.min[0] > original_bounds.max[0] {
            continue;
        }
        for axis in 0..3 {
            let extent = (original_bounds.max[axis] - original_bounds.min[axis]).abs();
            let tolerance = extent * 0.05 + 7.0;
            let min_error = (rebuilt_bounds.min[axis] - original_bounds.min[axis]).abs();
            let max_error = (rebuilt_bounds.max[axis] - original_bounds.max[axis]).abs();
            assert!(
                min_error <= tolerance && max_error <= tolerance,
                "{}: bone_bounds[{bone_index}] axis {axis}: errors [{min_error:.4}, {max_error:.4}], tolerance {tolerance:.4}",
                path.display(),
            );
        }
    }
}

fn assert_tree(path: &Path, original: &UgxGeom, rebuilt: &UgxGeom) {
    if original.aabb_tree.is_none() {
        return;
    }
    let tree = rebuilt.aabb_tree.as_ref().unwrap_or_else(|| {
        panic!(
            "{}: rebuilt geometry should retain an AABB tree",
            path.display()
        )
    });
    assert!(
        tree.nodes.iter().all(|node| node.obj_indices.is_empty()),
        "{}: rebuilt AABB nodes must use accessories, not object indices",
        path.display(),
    );
}

fn assert_accessory_coverage(path: &Path, rebuilt: &UgxGeom) {
    if let Some(tree) = &rebuilt.aabb_tree {
        assert_eq!(
            rebuilt.accessories.len(),
            tree.nodes.len(),
            "{}: accessory and tree-node counts differ",
            path.display(),
        );
    }
    if rebuilt.accessories.is_empty() {
        return;
    }

    let mut covered = vec![false; rebuilt.sections.len()];
    for section_index in rebuilt
        .accessories
        .iter()
        .flat_map(|accessory| &accessory.object_indices)
        .filter_map(|index| usize::try_from(*index).ok())
    {
        if let Some(value) = covered.get_mut(section_index) {
            *value = true;
        }
    }
    for (section_index, is_covered) in covered.into_iter().enumerate() {
        assert!(
            is_covered,
            "{}: section {section_index} is not reachable through an accessory",
            path.display(),
        );
    }
}

fn assert_writable(original: &UgxGeom, rebuilt: &UgxGeom) {
    let bytes = ugx::Writer::write(rebuilt, UgxVersion::Hw2)
        .expect("rebuilt geometry should serialize as HW2");
    let reread = ugx::Reader::read(&bytes).expect("rebuilt geometry should parse after writing");
    assert_eq!(reread.sections.len(), original.sections.len());
}
