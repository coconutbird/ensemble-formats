//! Fallback local transform derivation (used when bones lack stored transforms).

use alloc::vec::Vec;

use crate::constants::{GRANNY_HAS_ORIENTATION, GRANNY_HAS_POSITION, GRANNY_HAS_SCALE_SHEAR};
use crate::types::{Matrix4x4, UgxGeom};

pub(super) struct FallbackTransform {
    pub flags: u32,
    pub position: [f32; 3],
    pub orientation: [f32; 4],
    pub scale_shear: [[f32; 3]; 3],
}

/// Derive local transforms from inverse world matrices for bones that lack
/// stored transform data (e.g. after a glTF round-trip).
pub(super) fn compute_fallback_local_transforms(geom: &UgxGeom) -> Vec<FallbackTransform> {
    let bone_count = geom.granny_bones.len();
    let world_matrices: Vec<Matrix4x4> = geom
        .granny_bones
        .iter()
        .map(|bone| bone.inverse_world_matrix.inverse().unwrap_or_default())
        .collect();

    geom.granny_bones
        .iter()
        .enumerate()
        .map(|(i, bone)| {
            // DX row-vector: World = Local * ParentWorld
            // Therefore:    Local = World * ParentWorld^{-1} = World * ParentIWM
            let local_matrix =
                if bone.parent_index >= 0 && (bone.parent_index as usize) < bone_count {
                    let parent_idx = bone.parent_index as usize;
                    world_matrices[i].multiply(&geom.granny_bones[parent_idx].inverse_world_matrix)
                } else {
                    world_matrices[i].clone()
                };

            let position = local_matrix.translation();
            let m = &local_matrix.rows;
            let sx = (m[0][0] * m[0][0] + m[1][0] * m[1][0] + m[2][0] * m[2][0]).sqrt();
            let sy = (m[0][1] * m[0][1] + m[1][1] * m[1][1] + m[2][1] * m[2][1]).sqrt();
            let sz = (m[0][2] * m[0][2] + m[1][2] * m[1][2] + m[2][2] * m[2][2]).sqrt();

            let rot_matrix = if sx > 1e-7 && sy > 1e-7 && sz > 1e-7 {
                Matrix4x4 {
                    rows: [
                        [m[0][0] / sx, m[0][1] / sy, m[0][2] / sz, 0.0],
                        [m[1][0] / sx, m[1][1] / sy, m[1][2] / sz, 0.0],
                        [m[2][0] / sx, m[2][1] / sy, m[2][2] / sz, 0.0],
                        [0.0, 0.0, 0.0, 1.0],
                    ],
                }
            } else {
                Matrix4x4::identity()
            };

            let mut orientation = rot_matrix.to_quaternion();
            // Granny stores quaternions in conjugate form (negated xyz).
            orientation[0] = -orientation[0];
            orientation[1] = -orientation[1];
            orientation[2] = -orientation[2];
            // Canonical sign: ensure w >= 0.
            if orientation[3] < 0.0 {
                orientation[0] = -orientation[0];
                orientation[1] = -orientation[1];
                orientation[2] = -orientation[2];
                orientation[3] = -orientation[3];
            }
            let rt = rot_matrix.transpose();
            let scale_shear = [
                [
                    rt.rows[0][0] * m[0][0] + rt.rows[0][1] * m[1][0] + rt.rows[0][2] * m[2][0],
                    rt.rows[0][0] * m[0][1] + rt.rows[0][1] * m[1][1] + rt.rows[0][2] * m[2][1],
                    rt.rows[0][0] * m[0][2] + rt.rows[0][1] * m[1][2] + rt.rows[0][2] * m[2][2],
                ],
                [
                    rt.rows[1][0] * m[0][0] + rt.rows[1][1] * m[1][0] + rt.rows[1][2] * m[2][0],
                    rt.rows[1][0] * m[0][1] + rt.rows[1][1] * m[1][1] + rt.rows[1][2] * m[2][1],
                    rt.rows[1][0] * m[0][2] + rt.rows[1][1] * m[1][2] + rt.rows[1][2] * m[2][2],
                ],
                [
                    rt.rows[2][0] * m[0][0] + rt.rows[2][1] * m[1][0] + rt.rows[2][2] * m[2][0],
                    rt.rows[2][0] * m[0][1] + rt.rows[2][1] * m[1][1] + rt.rows[2][2] * m[2][1],
                    rt.rows[2][0] * m[0][2] + rt.rows[2][1] * m[1][2] + rt.rows[2][2] * m[2][2],
                ],
            ];

            const FLAG_EPS: f32 = 1e-4;

            let mut flags = 0u32;
            if position[0].abs() > FLAG_EPS
                || position[1].abs() > FLAG_EPS
                || position[2].abs() > FLAG_EPS
            {
                flags |= GRANNY_HAS_POSITION;
            }
            if (orientation[0].abs() > FLAG_EPS)
                || (orientation[1].abs() > FLAG_EPS)
                || (orientation[2].abs() > FLAG_EPS)
                || ((orientation[3] - 1.0).abs() > FLAG_EPS)
            {
                flags |= GRANNY_HAS_ORIENTATION;
            }
            let is_identity_scale = (scale_shear[0][0] - 1.0).abs() < FLAG_EPS
                && scale_shear[0][1].abs() < FLAG_EPS
                && scale_shear[0][2].abs() < FLAG_EPS
                && scale_shear[1][0].abs() < FLAG_EPS
                && (scale_shear[1][1] - 1.0).abs() < FLAG_EPS
                && scale_shear[1][2].abs() < FLAG_EPS
                && scale_shear[2][0].abs() < FLAG_EPS
                && scale_shear[2][1].abs() < FLAG_EPS
                && (scale_shear[2][2] - 1.0).abs() < FLAG_EPS;
            if !is_identity_scale {
                flags |= GRANNY_HAS_SCALE_SHEAR;
            }

            FallbackTransform {
                flags,
                position,
                orientation,
                scale_shear,
            }
        })
        .collect()
}
