//! Metadata flag and index buffer recomputation for `UgxGeom`.
//!
//! Rebuilds engine metadata flags (`rigid_only`, `all_sections_rigid`, etc.)
//! and the instanced index buffer from section data.

use alloc::vec::Vec;

use crate::{Error, Result, UgxGeom};

impl UgxGeom {
    /// Recompute metadata flags from section data.
    ///
    /// Updates: `rigid_only`, `rigid_bone_index`, `all_sections_rigid`,
    /// `all_sections_skinned`, `global_bones`, `instance_index_multiplier`,
    /// `max_instances`, `large_geom_bone_index`.
    ///
    /// # Errors
    ///
    /// Returns an error if a derived vertex count cannot be represented by the
    /// UGX metadata fields.
    pub fn rebuild_metadata_flags(&mut self) -> Result<()> {
        // A section is "effectively rigid" if:
        // - its rigid_only flag is true, OR
        // - it uses global_bones with max_bones == 1 (single-bone global binding)
        let section_is_rigid = |s: &crate::types::Section| -> bool {
            s.rigid_only || (s.global_bones && s.max_bones <= 1)
        };

        let all_rigid = self.sections.iter().all(&section_is_rigid);
        let all_skinned = self.sections.iter().all(|s| !section_is_rigid(s));
        let any_global = self.sections.iter().any(|s| s.global_bones);
        // Geom-level global_bones: true when any section uses global_bones OR
        // when all sections use global bone indices (empty bone_remap), meaning
        // vertex bone indices refer directly to the skeleton rather than
        // section-local remapped indices.
        let all_global_indices =
            !self.sections.is_empty() && self.sections.iter().all(|s| s.bone_remap.is_empty());

        // rigid_only: true when all sections are effectively rigid AND all bound
        // to the same bone (single rigid_bone_index across all sections).
        let same_rigid_bone = self.sections.first().is_some_and(|first_section| {
            all_rigid
                && self
                    .sections
                    .iter()
                    .all(|section| section.rigid_bone_index == first_section.rigid_bone_index)
        });
        self.rigid_only = same_rigid_bone;
        // all_sections_rigid uses the broader "effectively rigid" test
        self.flags.all_sections_rigid = all_rigid;
        self.flags.global_bones = any_global || all_global_indices;

        // all_sections_skinned: true only when every section is skinned AND
        // no section uses global_bones (matching original engine logic).
        self.flags.all_sections_skinned = !any_global && !all_rigid && all_skinned;

        // rigid_bone_index: if a single rigid bone is used across all rigid
        // sections, use it; otherwise 0.
        if all_rigid && let Some(first_section) = self.sections.first() {
            let first_rigid = first_section.rigid_bone_index;
            if self
                .sections
                .iter()
                .all(|s| s.rigid_bone_index == first_rigid)
            {
                self.rigid_bone_index = first_rigid;
            } else {
                self.rigid_bone_index = 0;
            }
        } else {
            self.rigid_bone_index = 0;
        }

        // instance_index_multiplier: next power of two of max vertex count
        let max_verts = self
            .sections
            .iter()
            .map(|section| u32::try_from(section.num_verts).unwrap_or_default())
            .max()
            .unwrap_or(1);
        let multiplier = max_verts
            .checked_next_power_of_two()
            .ok_or(Error::SizeOverflow("instance index multiplier"))?;
        self.instance_index_multiplier = encode_instance_index_multiplier(multiplier)?;

        // max_instances is set by artist tooling, not derivable from mesh data.
        // Preserve the existing value if already set (e.g. from glTF extras);
        // only default to 1 when it hasn't been initialised yet (0 or negative).
        if self.max_instances <= 0 {
            self.max_instances = 1;
        }
        // large_geom_bone_index defaults to i16::MAX (no large geom)
        self.large_geom_bone_index = i16::MAX;
        Ok(())
    }

    /// Rebuild the index buffer with instanced copies baked in.
    ///
    /// The engine uploads the 0x701 chunk to the GPU as-is (via
    /// `BUGXGeomData::loadIndexBuffer`). When `max_instances > 1`, the
    /// original tooling bakes `max_instances` copies of each section's
    /// indices into the buffer, each copy offset by
    /// `instance_index * instance_index_multiplier` vertices. The engine
    /// reads `max_instances` from the header for render scaling but never
    /// splits or replicates the buffer at runtime.
    ///
    /// This method must be called AFTER `rebuild_metadata_flags` (which
    /// computes `max_instances` and `instance_index_multiplier`).
    ///
    /// If `max_instances <= 1`, the buffer is left unchanged.
    ///
    /// # Errors
    ///
    /// Returns an error if section counts, offsets, or instanced vertex indices
    /// cannot be represented by their UGX integer fields.
    pub fn rebuild_instanced_index_buffer(&mut self) -> Result<()> {
        let max_inst = u32::try_from(self.max_instances).unwrap_or_default();
        let multiplier = u32::from(decode_instance_index_multiplier(
            self.instance_index_multiplier,
        ));

        if max_inst <= 1 || multiplier == 0 {
            return Ok(());
        }

        // Compute the total base index count from sections.
        let base_index_count = self.sections.iter().try_fold(0usize, |total, section| {
            let triangle_count =
                crate::checked_usize_i32(section.num_tris, "section triangle count")?;
            let index_count = triangle_count
                .checked_mul(3)
                .ok_or(Error::SizeOverflow("section index count"))?;
            total
                .checked_add(index_count)
                .ok_or(Error::SizeOverflow("index-buffer size"))
        })?;

        // If the buffer already has the expected instanced size, skip.
        let expected_instanced = base_index_count
            .checked_mul(crate::checked_usize(u64::from(max_inst), "instance count")?)
            .ok_or(Error::SizeOverflow("instanced index-buffer size"))?;
        if self.index_buffer.len() == expected_instanced {
            return Ok(());
        }

        // If the buffer doesn't match base size either, skip to avoid corruption.
        if self.index_buffer.len() != base_index_count {
            return Ok(());
        }

        // Build the instanced buffer: for each instance i (0..max_instances),
        // copy every section's indices with vertex indices offset by
        // i * instance_index_multiplier.
        let mut instanced = Vec::with_capacity(expected_instanced);
        for inst in 0..max_inst {
            let vertex_offset = inst
                .checked_mul(multiplier)
                .and_then(|offset| u16::try_from(offset).ok())
                .ok_or(Error::SizeOverflow("instanced vertex offset"))?;
            for section in &self.sections {
                let sec_start =
                    crate::checked_usize_i32(section.ib_offset, "section index offset")?;
                let sec_count = crate::checked_usize_i32(section.num_tris, "triangle count")?
                    .checked_mul(3)
                    .ok_or(Error::SizeOverflow("section index count"))?;
                let sec_end = sec_start
                    .checked_add(sec_count)
                    .ok_or(Error::SizeOverflow("section index range"))?;
                let section_indices =
                    self.index_buffer.get(sec_start..sec_end).ok_or_else(|| {
                        Error::UnexpectedEof {
                            context: "section index buffer".into(),
                        }
                    })?;
                for &idx in section_indices {
                    instanced.push(idx.wrapping_add(vertex_offset));
                }
            }
        }

        // Update section ib_offsets for the new sequential layout.
        let mut offset = 0i32;
        for section in &mut self.sections {
            section.ib_offset = offset;
            let section_indices = section
                .num_tris
                .checked_mul(3)
                .ok_or(Error::SizeOverflow("section index count"))?;
            offset = offset
                .checked_add(section_indices)
                .ok_or(Error::SizeOverflow("section index offset"))?;
        }

        self.index_buffer = instanced;
        Ok(())
    }
}

/// Encode the unsigned on-disk multiplier while preserving the public signed
/// field used by the existing API. Retail files use `0x8000` for 32,768.
fn encode_instance_index_multiplier(multiplier: u32) -> Result<i16> {
    let raw =
        u16::try_from(multiplier).map_err(|_| Error::SizeOverflow("instance index multiplier"))?;
    Ok(i16::from_le_bytes(raw.to_le_bytes()))
}

fn decode_instance_index_multiplier(multiplier: i16) -> u16 {
    u16::from_le_bytes(multiplier.to_le_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_multiplier_preserves_unsigned_16_bit_values() {
        let encoded = encode_instance_index_multiplier(32_768).unwrap();
        assert_eq!(encoded, i16::MIN);
        assert_eq!(decode_instance_index_multiplier(encoded), 32_768);
        assert!(encode_instance_index_multiplier(65_536).is_err());
    }
}
