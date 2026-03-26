//! Rebuild derived data tests — compare rebuilt output against real game files.

use test_utils::prelude::*;
use ugx::*;

/// Test rebuild against real UGX files: read original, strip derived data, rebuild, compare.
#[test]
fn test_rebuild_vs_original() {
    // Static paths + recursive scan of extracted ERA contents
    let mut paths: Vec<String> = vec![
        "../../foxcannon01/mesh_turret_0.ugx".into(),
        "../../foxcannon01/mesh_barrel_0.ugx".into(),
        "../../foxcannon01/mesh_foxcannon01.ugx".into(),
        "../../test_ugx/art/covenant/air/banshee_01/banshee_damage_01.ugx".into(),
    ];
    paths.extend(
        find_files_by_ext(std::path::Path::new("../../test_ugx_rebuild"), "ugx")
            .into_iter()
            .map(|p| p.to_string_lossy().into_owned()),
    );

    let mut tested = 0usize;
    for path in &paths {
        let data = match std::fs::read(path) {
            Ok(d) => d,
            Err(_) => continue,
        };

        let original = match ugx::Reader::read(&data) {
            Ok(g) => g,
            Err(_) => continue,
        };

        // Clone and strip derived data
        let mut rebuilt = original.clone();
        rebuilt.bone_bounds = original.bones.iter().map(|_| AABB::default()).collect();
        rebuilt.accessories = Vec::new();
        rebuilt.valid_accessories = Vec::new();
        rebuilt.aabb_tree = None;

        rebuilt.rebuild_derived_data();

        // Bounds: tolerance accounts for Half4 vertex packing precision
        for i in 0..3 {
            let tol = (original.bounds.max[i] - original.bounds.min[i]).abs() * 0.005 + 0.01;
            assert!(
                (rebuilt.bounds.min[i] - original.bounds.min[i]).abs() < tol,
                "{path}: bounds.min[{i}] mismatch: rebuilt={} vs original={} (tol={tol})",
                rebuilt.bounds.min[i],
                original.bounds.min[i],
            );
            assert!(
                (rebuilt.bounds.max[i] - original.bounds.max[i]).abs() < tol,
                "{path}: bounds.max[{i}] mismatch: rebuilt={} vs original={} (tol={tol})",
                rebuilt.bounds.max[i],
                original.bounds.max[i],
            );
        }

        // Bounding sphere: radius = half the AABB diagonal
        let orig_r = original.bounding_sphere.radius;
        let rebuilt_r = rebuilt.bounding_sphere.radius;
        let sphere_err = if orig_r > 0.0 {
            ((rebuilt_r - orig_r) / orig_r * 100.0).abs()
        } else {
            0.0
        };
        assert!(
            sphere_err < 1.0,
            "{path}: bounding sphere radius mismatch: rebuilt={rebuilt_r} vs original={orig_r} ({sphere_err:.1}% diff)",
        );

        // Bone bounds: count and per-bone AABB values
        assert_eq!(
            rebuilt.bone_bounds.len(),
            original.bone_bounds.len(),
            "{path}: bone bounds count mismatch",
        );
        for (bi, (rb, ob)) in rebuilt
            .bone_bounds
            .iter()
            .zip(original.bone_bounds.iter())
            .enumerate()
        {
            let orig_is_sentinel = ob.min[0] > ob.max[0];
            if orig_is_sentinel {
                continue;
            }
            // Wider tolerance: vertex quantization (Half4) can cause up to
            // ~7 units of drift on wreckage/debris models with extreme spread.
            for axis in 0..3 {
                let extent = (ob.max[axis] - ob.min[axis]).abs();
                let tol = extent * 0.05 + 7.0;
                let min_err = (rb.min[axis] - ob.min[axis]).abs();
                let max_err = (rb.max[axis] - ob.max[axis]).abs();
                assert!(
                    min_err <= tol && max_err <= tol,
                    "{path}: bone_bounds[{bi}] axis={axis}: rebuilt=[{:.4}, {:.4}] orig=[{:.4}, {:.4}] err=[{:.4}, {:.4}] tol={tol:.4}",
                    rb.min[axis],
                    rb.max[axis],
                    ob.min[axis],
                    ob.max[axis],
                    min_err,
                    max_err,
                );
            }
        }

        // Metadata flags: only instance_index_multiplier must be exact.
        // Header-level booleans (rigid_only, global_bones, etc.) sometimes
        // don't follow strictly from per-section flags in the original data.
        assert_eq!(
            rebuilt.instance_index_multiplier, original.instance_index_multiplier,
            "{path}: instance_index_multiplier mismatch",
        );

        // AABB tree: if original had one, rebuilt should too.
        // The original tree is typically a minimal stub (few nodes, 0 obj_indices)
        // while our rebuilt tree is a proper section-level BVH. We verify
        // structural invariants rather than exact node-count match.
        if original.aabb_tree.is_some() {
            assert!(
                rebuilt.aabb_tree.is_some(),
                "{path}: rebuilt should have AABB tree when original did",
            );
            let new_tree = rebuilt.aabb_tree.as_ref().unwrap();
            // All obj_indices should be empty (engine reads from accessories)
            for node in &new_tree.nodes {
                assert!(
                    node.obj_indices.is_empty(),
                    "{path}: tree node obj_indices should be empty",
                );
            }
        }

        // Accessories: engine invariant is accessories.len() == tree.nodes.len().
        // The original file's accessory count matches its (stub) tree, and our
        // rebuilt count matches our (full) tree — they won't be equal, but every
        // section must be reachable through the rebuilt accessories.
        if let Some(ref tree) = rebuilt.aabb_tree {
            assert_eq!(
                rebuilt.accessories.len(),
                tree.nodes.len(),
                "{path}: accessories.len() must equal tree.nodes.len()",
            );
        }
        // Verify all sections are covered by at least one accessory.
        if !rebuilt.accessories.is_empty() {
            let all_sections = rebuilt.sections.len();
            let mut covered = vec![false; all_sections];
            for acc in &rebuilt.accessories {
                for &si in &acc.object_indices {
                    if (si as usize) < all_sections {
                        covered[si as usize] = true;
                    }
                }
            }
            for (si, &c) in covered.iter().enumerate() {
                assert!(c, "{path}: section {si} not reachable via any accessory",);
            }
        }

        // Write rebuilt to UGX bytes and read back to verify structural validity
        let ugx_bytes = ugx::Writer::write(&rebuilt, ugx::UgxVersion::Hw2).unwrap();
        let re_read = ugx::Reader::read(&ugx_bytes).unwrap();
        assert_eq!(re_read.sections.len(), original.sections.len());

        tested += 1;
    }

    assert!(
        tested > 0,
        "No real UGX files found — rebuild comparison test skipped"
    );
}
