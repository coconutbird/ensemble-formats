//! Matrix and quaternion math for skeletal transforms.
//!
//! Contains `Matrix4x4` (row-major 4×4) and `QForm` (quaternion + translation)
//! used for bone transforms in UGX geometry data.

use nostdio::{Cursor, ReadLe};

use crate::error::Result;

/// Quaternion + translation transform (used in non-packed format).
#[derive(Debug, Clone, Default)]
pub struct QForm {
    /// Quaternion [x, y, z, w].
    pub rotation: [f32; 4],
    /// Translation [x, y, z].
    pub translation: [f32; 3],
}

/// 4×4 transformation matrix (used in packed format).
/// Row-major order: row\[0\] = \[m00, m01, m02, m03\], etc.
#[derive(Debug, Clone)]
pub struct Matrix4x4 {
    /// Matrix rows.
    pub rows: [[f32; 4]; 4],
}

impl Default for Matrix4x4 {
    fn default() -> Self {
        Self {
            rows: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }
}

impl Matrix4x4 {
    /// Get translation from the matrix (row 3, columns 0-2).
    #[must_use]
    pub fn translation(&self) -> [f32; 3] {
        [self.rows[3][0], self.rows[3][1], self.rows[3][2]]
    }

    /// Convert to glTF column-major format (16-element array).
    /// glTF expects: [m00, m10, m20, m30, m01, m11, m21, m31, m02, m12, m22, m32, m03, m13, m23, m33]
    #[must_use]
    pub fn to_gltf_column_major(&self) -> [f32; 16] {
        let m = &self.rows;
        [
            m[0][0], m[1][0], m[2][0], m[3][0], // column 0
            m[0][1], m[1][1], m[2][1], m[3][1], // column 1
            m[0][2], m[1][2], m[2][2], m[3][2], // column 2
            m[0][3], m[1][3], m[2][3], m[3][3], // column 3
        ]
    }

    /// Create an identity matrix.
    #[must_use]
    pub fn identity() -> Self {
        Self {
            rows: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    /// Invert this 4x4 matrix. Returns None if the matrix is singular.
    #[must_use]
    pub fn inverse(&self) -> Option<Self> {
        let m = &self.rows;

        let c00 = m[1][1] * (m[2][2] * m[3][3] - m[2][3] * m[3][2])
            - m[1][2] * (m[2][1] * m[3][3] - m[2][3] * m[3][1])
            + m[1][3] * (m[2][1] * m[3][2] - m[2][2] * m[3][1]);

        let c01 = -(m[1][0] * (m[2][2] * m[3][3] - m[2][3] * m[3][2])
            - m[1][2] * (m[2][0] * m[3][3] - m[2][3] * m[3][0])
            + m[1][3] * (m[2][0] * m[3][2] - m[2][2] * m[3][0]));

        let c02 = m[1][0] * (m[2][1] * m[3][3] - m[2][3] * m[3][1])
            - m[1][1] * (m[2][0] * m[3][3] - m[2][3] * m[3][0])
            + m[1][3] * (m[2][0] * m[3][1] - m[2][1] * m[3][0]);

        let c03 = -(m[1][0] * (m[2][1] * m[3][2] - m[2][2] * m[3][1])
            - m[1][1] * (m[2][0] * m[3][2] - m[2][2] * m[3][0])
            + m[1][2] * (m[2][0] * m[3][1] - m[2][1] * m[3][0]));

        let det = m[0][0] * c00 + m[0][1] * c01 + m[0][2] * c02 + m[0][3] * c03;

        if det.abs() < 1e-10 {
            return None;
        }

        let inv_det = 1.0 / det;

        let c10 = -(m[0][1] * (m[2][2] * m[3][3] - m[2][3] * m[3][2])
            - m[0][2] * (m[2][1] * m[3][3] - m[2][3] * m[3][1])
            + m[0][3] * (m[2][1] * m[3][2] - m[2][2] * m[3][1]));

        let c11 = m[0][0] * (m[2][2] * m[3][3] - m[2][3] * m[3][2])
            - m[0][2] * (m[2][0] * m[3][3] - m[2][3] * m[3][0])
            + m[0][3] * (m[2][0] * m[3][2] - m[2][2] * m[3][0]);

        let c12 = -(m[0][0] * (m[2][1] * m[3][3] - m[2][3] * m[3][1])
            - m[0][1] * (m[2][0] * m[3][3] - m[2][3] * m[3][0])
            + m[0][3] * (m[2][0] * m[3][1] - m[2][1] * m[3][0]));

        let c13 = m[0][0] * (m[2][1] * m[3][2] - m[2][2] * m[3][1])
            - m[0][1] * (m[2][0] * m[3][2] - m[2][2] * m[3][0])
            + m[0][2] * (m[2][0] * m[3][1] - m[2][1] * m[3][0]);

        let c20 = m[0][1] * (m[1][2] * m[3][3] - m[1][3] * m[3][2])
            - m[0][2] * (m[1][1] * m[3][3] - m[1][3] * m[3][1])
            + m[0][3] * (m[1][1] * m[3][2] - m[1][2] * m[3][1]);

        let c21 = -(m[0][0] * (m[1][2] * m[3][3] - m[1][3] * m[3][2])
            - m[0][2] * (m[1][0] * m[3][3] - m[1][3] * m[3][0])
            + m[0][3] * (m[1][0] * m[3][2] - m[1][2] * m[3][0]));

        let c22 = m[0][0] * (m[1][1] * m[3][3] - m[1][3] * m[3][1])
            - m[0][1] * (m[1][0] * m[3][3] - m[1][3] * m[3][0])
            + m[0][3] * (m[1][0] * m[3][1] - m[1][1] * m[3][0]);

        let c23 = -(m[0][0] * (m[1][1] * m[3][2] - m[1][2] * m[3][1])
            - m[0][1] * (m[1][0] * m[3][2] - m[1][2] * m[3][0])
            + m[0][2] * (m[1][0] * m[3][1] - m[1][1] * m[3][0]));

        let c30 = -(m[0][1] * (m[1][2] * m[2][3] - m[1][3] * m[2][2])
            - m[0][2] * (m[1][1] * m[2][3] - m[1][3] * m[2][1])
            + m[0][3] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]));

        let c31 = m[0][0] * (m[1][2] * m[2][3] - m[1][3] * m[2][2])
            - m[0][2] * (m[1][0] * m[2][3] - m[1][3] * m[2][0])
            + m[0][3] * (m[1][0] * m[2][2] - m[1][2] * m[2][0]);

        let c32 = -(m[0][0] * (m[1][1] * m[2][3] - m[1][3] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][3] - m[1][3] * m[2][0])
            + m[0][3] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]));

        let c33 = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);

        Some(Self {
            rows: [
                [c00 * inv_det, c10 * inv_det, c20 * inv_det, c30 * inv_det],
                [c01 * inv_det, c11 * inv_det, c21 * inv_det, c31 * inv_det],
                [c02 * inv_det, c12 * inv_det, c22 * inv_det, c32 * inv_det],
                [c03 * inv_det, c13 * inv_det, c23 * inv_det, c33 * inv_det],
            ],
        })
    }

    /// Multiply two matrices: self * other
    #[must_use]
    pub fn multiply(&self, other: &Self) -> Self {
        let left_rows = &self.rows;
        let right_rows = &other.rows;
        let mut result = [[0.0f32; 4]; 4];

        for row_index in 0..4 {
            for column_index in 0..4 {
                result[row_index][column_index] = left_rows[row_index][0]
                    * right_rows[0][column_index]
                    + left_rows[row_index][1] * right_rows[1][column_index]
                    + left_rows[row_index][2] * right_rows[2][column_index]
                    + left_rows[row_index][3] * right_rows[3][column_index];
            }
        }

        Self { rows: result }
    }

    /// Transpose the matrix (swap rows and columns).
    #[must_use]
    pub fn transpose(&self) -> Self {
        let m = &self.rows;
        Self {
            rows: [
                [m[0][0], m[1][0], m[2][0], m[3][0]],
                [m[0][1], m[1][1], m[2][1], m[3][1]],
                [m[0][2], m[1][2], m[2][2], m[3][2]],
                [m[0][3], m[1][3], m[2][3], m[3][3]],
            ],
        }
    }

    /// Extract rotation as a quaternion [x, y, z, w] from the 3x3 rotation part.
    /// Uses the Shepperd method for numerical stability.
    #[must_use]
    pub fn to_quaternion(&self) -> [f32; 4] {
        let matrix = &self.rows;
        let m00 = matrix[0][0];
        let m11 = matrix[1][1];
        let m22 = matrix[2][2];
        let trace = m00 + m11 + m22;

        let (quaternion_x, quaternion_y, quaternion_z, quaternion_w) = if trace > 0.0 {
            let scale = (trace + 1.0).sqrt() * 2.0;
            let quaternion_w = 0.25 * scale;
            let quaternion_x = (matrix[2][1] - matrix[1][2]) / scale;
            let quaternion_y = (matrix[0][2] - matrix[2][0]) / scale;
            let quaternion_z = (matrix[1][0] - matrix[0][1]) / scale;
            (quaternion_x, quaternion_y, quaternion_z, quaternion_w)
        } else if m00 > m11 && m00 > m22 {
            let scale = (1.0 + m00 - m11 - m22).sqrt() * 2.0;
            let quaternion_w = (matrix[2][1] - matrix[1][2]) / scale;
            let quaternion_x = 0.25 * scale;
            let quaternion_y = (matrix[0][1] + matrix[1][0]) / scale;
            let quaternion_z = (matrix[0][2] + matrix[2][0]) / scale;
            (quaternion_x, quaternion_y, quaternion_z, quaternion_w)
        } else if m11 > m22 {
            let scale = (1.0 + m11 - m00 - m22).sqrt() * 2.0;
            let quaternion_w = (matrix[0][2] - matrix[2][0]) / scale;
            let quaternion_x = (matrix[0][1] + matrix[1][0]) / scale;
            let quaternion_y = 0.25 * scale;
            let quaternion_z = (matrix[1][2] + matrix[2][1]) / scale;
            (quaternion_x, quaternion_y, quaternion_z, quaternion_w)
        } else {
            let scale = (1.0 + m22 - m00 - m11).sqrt() * 2.0;
            let quaternion_w = (matrix[1][0] - matrix[0][1]) / scale;
            let quaternion_x = (matrix[0][2] + matrix[2][0]) / scale;
            let quaternion_y = (matrix[1][2] + matrix[2][1]) / scale;
            let quaternion_z = 0.25 * scale;
            (quaternion_x, quaternion_y, quaternion_z, quaternion_w)
        };

        let length = (quaternion_x * quaternion_x
            + quaternion_y * quaternion_y
            + quaternion_z * quaternion_z
            + quaternion_w * quaternion_w)
            .sqrt();
        if length > 1e-10 {
            [
                quaternion_x / length,
                quaternion_y / length,
                quaternion_z / length,
                quaternion_w / length,
            ]
        } else {
            [0.0, 0.0, 0.0, 1.0]
        }
    }
}

impl Matrix4x4 {
    /// Read a 4×4 row-major matrix from 64 bytes of little-endian `f32` data.
    ///
    /// # Errors
    ///
    /// Returns an error if the input is truncated or the cursor position
    /// cannot be represented on the target platform.
    pub fn read(data: &[u8], pos: &mut usize) -> Result<Self> {
        let remaining = data
            .get(*pos..)
            .ok_or_else(|| crate::Error::UnexpectedEof {
                context: "matrix".into(),
            })?;
        let mut cur = Cursor::new(remaining);
        let mut rows = [[0.0f32; 4]; 4];
        for row in &mut rows {
            for col in row {
                *col = cur.read_f32_le()?;
            }
        }
        crate::advance_position(pos, cur.position(), "binary cursor position")?;
        Ok(Self { rows })
    }
}

impl QForm {
    /// Read a quaternion + translation from 28 bytes of little-endian `f32` data.
    ///
    /// # Errors
    ///
    /// Returns an error if the input is truncated or the cursor position
    /// cannot be represented on the target platform.
    pub fn read(data: &[u8], pos: &mut usize) -> Result<Self> {
        let remaining = data
            .get(*pos..)
            .ok_or_else(|| crate::Error::UnexpectedEof {
                context: "quaternion transform".into(),
            })?;
        let mut cur = Cursor::new(remaining);
        let rotation = [
            cur.read_f32_le()?,
            cur.read_f32_le()?,
            cur.read_f32_le()?,
            cur.read_f32_le()?,
        ];
        let translation = [cur.read_f32_le()?, cur.read_f32_le()?, cur.read_f32_le()?];
        crate::advance_position(pos, cur.position(), "binary cursor position")?;
        Ok(Self {
            rotation,
            translation,
        })
    }
}
