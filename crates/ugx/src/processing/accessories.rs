//! Accessory recomputation for `UgxGeom`.
//!
//! Rebuilds accessories from AABB tree node-section mappings or via
//! legacy group-by-`accessory_index` fallback.

use alloc::vec;
use alloc::vec::Vec;

use crate::types::Accessory;
use crate::{Result, UgxGeom};

impl UgxGeom {
    /// Rebuild accessories from the AABB tree's per-node section mapping.
    ///
    /// `node_section_map[i]` contains the section indices that belong to
    /// tree node `i`. The engine indexes into the accessories array by
    /// tree node index at runtime (`accessories[node_index]`), so we must
    /// produce exactly `tree.nodes.len()` entries.
    ///
    /// For each node:
    /// - Internal nodes (empty section list): `object_indices = []`
    /// - Leaf nodes: `object_indices = [section indices]`
    /// - `first_bone` / `num_bones`: computed from the bones referenced by
    ///   the node's sections (union of all bone influences).
    ///
    /// If no tree was built (e.g. HW2, or no sections), falls back to
    /// the legacy group-by-`accessory_index` strategy.
    ///
    /// `valid_accessories` contains the indices of accessories that have
    /// non-empty `object_indices` (leaf nodes with actual section geometry).
    /// The engine reads this as a flat i32 array via
    /// `BPackedArray_Simple__unpack`.
    ///
    /// # Errors
    ///
    /// Returns an error if section vertex data is malformed or a section or
    /// bone index cannot be represented by the UGX format.
    pub fn rebuild_accessories(&mut self, node_section_map: Vec<Vec<i32>>) -> Result<()> {
        let bone_count = self.bones.len();

        // If no tree was built, fall back to legacy accessory grouping.
        if node_section_map.is_empty() {
            return self.rebuild_accessories_legacy();
        }

        // Build one accessory per tree node.
        self.accessories = node_section_map
            .into_iter()
            .map(|sec_indices| {
                let (first_bone, num_bones) =
                    self.compute_bone_range_for_sections(&sec_indices, bone_count)?;
                Ok(Accessory {
                    first_bone,
                    num_bones,
                    object_indices: sec_indices,
                })
            })
            .collect::<Result<Vec<_>>>()?;

        // valid_accessories = accessories with non-empty object_indices (leaf nodes).
        self.valid_accessories = self
            .accessories
            .iter()
            .enumerate()
            .filter(|(_, accessory)| !accessory.object_indices.is_empty())
            .map(|(index, _)| crate::checked_i32(index, "valid-accessory index"))
            .collect::<Result<Vec<_>>>()?;
        Ok(())
    }

    /// Legacy accessory rebuild: group sections by `accessory_index`.
    ///
    /// Used when no AABB tree is present (e.g. HW2 files).
    fn rebuild_accessories_legacy(&mut self) -> Result<()> {
        let bone_count = self.bones.len();
        if bone_count == 0 || self.sections.is_empty() {
            self.accessories = Vec::new();
            self.valid_accessories = Vec::new();
            return Ok(());
        }

        let num_groups = self
            .sections
            .iter()
            .filter_map(|section| usize::try_from(section.accessory_index).ok())
            .max()
            .unwrap_or_default()
            .checked_add(1)
            .ok_or(crate::Error::SizeOverflow("accessory group count"))?;

        let mut group_sections: Vec<Vec<i32>> = vec![Vec::new(); num_groups];
        for (section_index, section) in self.sections.iter().enumerate() {
            if let Ok(group_index) = usize::try_from(section.accessory_index)
                && let Some(group) = group_sections.get_mut(group_index)
            {
                group.push(crate::checked_i32(section_index, "section index")?);
            }
        }

        self.accessories = (0..num_groups)
            .map(|group_index| {
                let (first_bone, num_bones) =
                    self.compute_bone_range_for_sections(&group_sections[group_index], bone_count)?;
                Ok(Accessory {
                    first_bone,
                    num_bones,
                    object_indices: group_sections[group_index].clone(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        self.valid_accessories = self
            .accessories
            .iter()
            .enumerate()
            .filter(|(_, accessory)| !accessory.object_indices.is_empty())
            .map(|(index, _)| crate::checked_i32(index, "valid-accessory index"))
            .collect::<Result<Vec<_>>>()?;
        Ok(())
    }

    /// Compute the bone range (`first_bone`, `num_bones`) for a set of sections.
    ///
    /// Scans vertex bone influences across all given sections and returns
    /// the contiguous range [`first_bone`, `first_bone` + `num_bones`) that
    /// covers all referenced bones.
    fn compute_bone_range_for_sections(
        &self,
        sec_indices: &[i32],
        bone_count: usize,
    ) -> Result<(i32, i32)> {
        if bone_count == 0 || sec_indices.is_empty() {
            return Ok((0, crate::checked_i32(bone_count, "bone count")?));
        }

        let mut bmin = usize::MAX;
        let mut bmax = 0usize;

        for &encoded_section_index in sec_indices {
            let Ok(section_index) = usize::try_from(encoded_section_index) else {
                continue;
            };
            let Some(section) = self.sections.get(section_index) else {
                continue;
            };
            let rigid_bone = usize::try_from(section.rigid_bone_index).unwrap_or(usize::MAX);
            let has_skin = section
                .vertex_packer()
                .map_or(!section.rigid_only && section.vert_size >= 28, |p| {
                    p.pack_order.contains('S')
                });
            let use_rigid = (!has_skin || section.rigid_only) && rigid_bone < bone_count;

            if use_rigid {
                bmin = bmin.min(rigid_bone);
                bmax = bmax.max(rigid_bone);
            } else {
                let verts = self.unpack_section_vertices(section_index)?;
                let bone_remap = &section.bone_remap;
                for v in &verts {
                    for j in 0..4 {
                        if v.bone_weights[j] > 0.0 {
                            let raw_idx = usize::from(v.bone_indices[j]);
                            let global_idx = if bone_remap.is_empty() {
                                // Already 0-based global.
                                raw_idx
                            } else {
                                // Section-local 0-based → remap to global 0-based.
                                if raw_idx < bone_remap.len() {
                                    usize::from(bone_remap[raw_idx])
                                } else {
                                    continue;
                                }
                            };
                            if global_idx < bone_count {
                                bmin = bmin.min(global_idx);
                                bmax = bmax.max(global_idx);
                            }
                        }
                    }
                }
            }
        }

        if bmin <= bmax {
            Ok((
                crate::checked_i32(bmin, "first bone index")?,
                crate::checked_i32(bmax - bmin + 1, "bone range")?,
            ))
        } else {
            Ok((0, crate::checked_i32(bone_count, "bone count")?))
        }
    }
}
