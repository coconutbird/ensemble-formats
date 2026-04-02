//! Bounding volume computation for imported geometry.

use ugx::{AABB, Sphere, UnpackedVertex};

/// Compute AABB and bounding sphere from vertices.
///
/// The bounding sphere is centered at the model origin `[0,0,0]` (root bone),
/// **not** the geometric centroid.  The engine uses the sphere center as the
/// model's anchor/pivot point, so shifting it to the mesh centroid would
/// offset the model in-game.
pub(crate) fn compute_bounds(vertices: &[UnpackedVertex]) -> (AABB, Sphere) {
    if vertices.is_empty() {
        return (AABB::default(), Sphere::default());
    }

    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];

    for v in vertices {
        for i in 0..3 {
            min[i] = min[i].min(v.position[i]);
            max[i] = max[i].max(v.position[i]);
        }
    }

    // Sphere centered at the model origin — radius is max distance from
    // origin to any vertex.
    let center = [0.0f32; 3];
    let mut max_dist_sq = 0.0f32;
    for v in vertices {
        let d = v.position[0] * v.position[0]
            + v.position[1] * v.position[1]
            + v.position[2] * v.position[2];
        max_dist_sq = max_dist_sq.max(d);
    }

    (
        AABB { min, max },
        Sphere {
            center,
            radius: max_dist_sq.sqrt(),
        },
    )
}
