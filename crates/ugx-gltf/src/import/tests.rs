use super::*;

fn assert_direction_near(actual: [f32; 3], expected: [f32; 3]) {
    for (actual, expected) in actual.into_iter().zip(expected) {
        assert!((actual - expected).abs() < 1.0e-5, "{actual} != {expected}");
    }
}

#[test]
fn rigid_restore_uses_inverse_transpose_and_normalizes_directions() {
    let world = Matrix4x4 {
        rows: [
            [2.0, 0.0, 0.0, 0.0],
            [0.0, 4.0, 0.0, 0.0],
            [0.0, 0.0, 8.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    };
    let inverse_transpose = world.inverse().unwrap().transpose();
    let expected_normal = normalized_direction([0.2, 0.3, 0.4], [0.0, 1.0, 0.0]);
    let expected_tangent = normalized_direction([0.4, 0.3, 0.2], [1.0, 0.0, 0.0]);
    let local_normal = normalized_direction(
        transform_direction(expected_normal, &world.transpose()),
        [0.0, 1.0, 0.0],
    );
    let local_tangent = normalized_direction(
        transform_direction(expected_tangent, &world.inverse().unwrap()),
        [1.0, 0.0, 0.0],
    );
    let source = UnpackedVertex {
        normal: local_normal,
        tangent: [local_tangent[0], local_tangent[1], local_tangent[2], -1.0],
        ..UnpackedVertex::default()
    };

    let restored = restore_rigid_vertex(&source, Some(&world), Some(&inverse_transpose));

    assert_direction_near(restored.normal, expected_normal);
    assert_direction_near(
        [
            restored.tangent[0],
            restored.tangent[1],
            restored.tangent[2],
        ],
        expected_tangent,
    );
    assert_eq!(restored.tangent[3].to_bits(), (-1.0f32).to_bits());
}
