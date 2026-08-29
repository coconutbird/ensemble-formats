//! Model-space transforms applied while authoring glTF data for UGX.

use ugx::{Bone, Error, GrannyBone, Matrix4x4, Result, UnpackedVertex};

/// Validate the uniform model scale accepted by the command-line and Blender paths.
pub(super) fn validate_model_scale(scale: f32) -> Result<()> {
    if !scale.is_finite() || scale <= 0.0 {
        return Err(Error::UnsupportedFormat(
            "glTF model scale must be finite and greater than zero".into(),
        ));
    }
    Ok(())
}

/// Apply an authoring coordinate conversion to inverse bind matrices.
pub(super) fn transform_skeleton(
    bones: &mut [Bone],
    granny_bones: &mut [GrannyBone],
    scale: f32,
    mirror_x: bool,
) -> Result<()> {
    validate_model_scale(scale)?;
    if scale.to_bits() == 1.0_f32.to_bits() && !mirror_x {
        return Ok(());
    }
    let coordinate = coordinate_matrix(scale, mirror_x);
    let inverse = coordinate
        .inverse()
        .ok_or_else(|| Error::UnsupportedFormat("glTF authoring transform is singular".into()))?;
    for bone in bones {
        bone.model_to_bone = conjugate(&bone.model_to_bone, &coordinate, &inverse);
    }
    for bone in granny_bones {
        bone.inverse_world_matrix = conjugate(&bone.inverse_world_matrix, &coordinate, &inverse);
    }
    Ok(())
}

/// Apply the same authoring conversion to model-space mesh data.
pub(super) fn apply_model_transform(
    vertices: &mut [UnpackedVertex],
    indices: &mut [u16],
    scale: f32,
    mirror_x: bool,
) -> Result<()> {
    validate_model_scale(scale)?;
    if scale.to_bits() == 1.0_f32.to_bits() && !mirror_x {
        return Ok(());
    }
    apply_node_transform(vertices, indices, &coordinate_matrix(scale, mirror_x))
}

fn coordinate_matrix(scale: f32, mirror_x: bool) -> Matrix4x4 {
    Matrix4x4 {
        rows: [
            [if mirror_x { -scale } else { scale }, 0.0, 0.0, 0.0],
            [0.0, scale, 0.0, 0.0],
            [0.0, 0.0, scale, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    }
}

fn conjugate(matrix: &Matrix4x4, coordinate: &Matrix4x4, inverse: &Matrix4x4) -> Matrix4x4 {
    inverse.multiply(matrix).multiply(coordinate)
}

pub(super) fn apply_node_transform(
    vertices: &mut [UnpackedVertex],
    indices: &mut [u16],
    transform: &Matrix4x4,
) -> Result<()> {
    let determinant = linear_determinant(transform);
    if !determinant.is_finite() || determinant.abs() < 1.0e-10 {
        return Err(Error::UnsupportedFormat(
            "Mesh node transform is singular".into(),
        ));
    }
    let normal_transform = transform
        .inverse()
        .ok_or_else(|| Error::UnsupportedFormat("Mesh node transform is singular".into()))?
        .transpose();
    let handedness = determinant.signum();
    for vertex in vertices {
        vertex.position = transform_point(vertex.position, transform);
        let normal = normalized_direction(
            transform_direction(vertex.normal, &normal_transform),
            [0.0, 1.0, 0.0],
        );
        vertex.normal = normal;
        let source_tangent = [vertex.tangent[0], vertex.tangent[1], vertex.tangent[2]];
        if squared_length(source_tangent) > 1.0e-12 {
            let transformed = transform_direction(source_tangent, transform);
            let fallback = super::fallback_tangent(normal);
            let tangent =
                normalized_direction(transformed, [fallback[0], fallback[1], fallback[2]]);
            vertex.tangent = [
                tangent[0],
                tangent[1],
                tangent[2],
                vertex.tangent[3] * handedness,
            ];
        }
    }
    if determinant < 0.0 {
        for triangle in indices.as_chunks_mut::<3>().0 {
            triangle.swap(1, 2);
        }
    }
    Ok(())
}

fn linear_determinant(matrix: &Matrix4x4) -> f32 {
    let rows = &matrix.rows;
    rows[0][0] * (rows[1][1] * rows[2][2] - rows[1][2] * rows[2][1])
        - rows[0][1] * (rows[1][0] * rows[2][2] - rows[1][2] * rows[2][0])
        + rows[0][2] * (rows[1][0] * rows[2][1] - rows[1][1] * rows[2][0])
}

pub(super) fn normalized_direction(value: [f32; 3], fallback: [f32; 3]) -> [f32; 3] {
    let length_squared = squared_length(value);
    if !length_squared.is_finite() || length_squared < 1.0e-12 {
        fallback
    } else {
        let inverse_length = length_squared.sqrt().recip();
        value.map(|component| component * inverse_length)
    }
}

fn squared_length(value: [f32; 3]) -> f32 {
    value
        .iter()
        .map(|component| component * component)
        .sum::<f32>()
}

pub(super) fn transform_point(value: [f32; 3], matrix: &Matrix4x4) -> [f32; 3] {
    let rows = &matrix.rows;
    [
        value[0] * rows[0][0] + value[1] * rows[1][0] + value[2] * rows[2][0] + rows[3][0],
        value[0] * rows[0][1] + value[1] * rows[1][1] + value[2] * rows[2][1] + rows[3][1],
        value[0] * rows[0][2] + value[1] * rows[1][2] + value[2] * rows[2][2] + rows[3][2],
    ]
}

pub(super) fn transform_direction(value: [f32; 3], matrix: &Matrix4x4) -> [f32; 3] {
    let rows = &matrix.rows;
    [
        value[0] * rows[0][0] + value[1] * rows[1][0] + value[2] * rows[2][0],
        value[0] * rows[0][1] + value[1] * rows[1][1] + value[2] * rows[2][1],
        value[0] * rows[0][2] + value[1] * rows[1][2] + value[2] * rows[2][2],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_components_close<const N: usize>(actual: [f32; N], expected: [f32; N]) {
        for (actual, expected) in actual.into_iter().zip(expected) {
            assert!((actual - expected).abs() < 1.0e-6, "{actual} != {expected}");
        }
    }

    #[test]
    fn scale_and_mirror_conjugate_inverse_bind_matrices() {
        let source = Matrix4x4 {
            rows: [
                [0.0, 0.0, -1.0, 0.0],
                [-1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [2.0, 3.0, 4.0, 1.0],
            ],
        };
        let coordinate = coordinate_matrix(1.575, true);
        let transformed = conjugate(&source, &coordinate, &coordinate.inverse().unwrap());

        assert_components_close(transformed.rows[0], [0.0, 0.0, 1.0, 0.0]);
        assert_components_close(transformed.rows[1], [1.0, 0.0, 0.0, 0.0]);
        assert_components_close(transformed.rows[2], [0.0, 1.0, 0.0, 0.0]);
        assert!((transformed.rows[3][0] + 3.15).abs() < 1.0e-6);
        assert!((transformed.rows[3][1] - 4.725).abs() < 1.0e-6);
        assert!((transformed.rows[3][2] - 6.3).abs() < 1.0e-6);
    }

    #[test]
    fn scale_and_mirror_transform_vertices_and_winding() {
        let mut vertices = [UnpackedVertex {
            position: [1.0, 2.0, 3.0],
            normal: [1.0, 0.0, 0.0],
            ..UnpackedVertex::default()
        }];
        let mut indices = [0, 1, 2];

        apply_model_transform(&mut vertices, &mut indices, 2.0, true).unwrap();

        assert_components_close(vertices[0].position, [-2.0, 4.0, 6.0]);
        assert_components_close(vertices[0].normal, [-1.0, 0.0, 0.0]);
        assert_eq!(indices, [0, 2, 1]);
    }
}
