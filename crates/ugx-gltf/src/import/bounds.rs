//! Bounding volume computation for imported geometry.

use ugx::{AABB, Sphere, UnpackedVertex};

/// Compute AABB and bounding sphere from vertices.
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

    let center = [
        f32::midpoint(min[0], max[0]),
        f32::midpoint(min[1], max[1]),
        f32::midpoint(min[2], max[2]),
    ];

    let mut max_dist_sq = 0.0f32;
    for v in vertices {
        let dx = v.position[0] - center[0];
        let dy = v.position[1] - center[1];
        let dz = v.position[2] - center[2];
        max_dist_sq = max_dist_sq.max(dx * dx + dy * dy + dz * dz);
    }

    (
        AABB { min, max },
        Sphere {
            center,
            radius: max_dist_sq.sqrt(),
        },
    )
}
