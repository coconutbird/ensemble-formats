//! Metadata flag and index buffer recomputation for `UgxGeom`.
//!
//! Rebuilds engine metadata flags (`rigid_only`, `all_sections_rigid`, etc.)
//! and the instanced index buffer from section data.

use alloc::vec::Vec;

use crate::UgxGeom;

impl UgxGeom {
    /// Recompute metadata flags from section data.
    ///
    /// Updates: `rigid_only`, `rigid_bone_index`, `all_sections_rigid`,
    /// `all_sections_skinned`, `global_bones`, `instance_index_multiplier`,
    /// `max_instances`, `large_geom_bone_index`.
    pub fn rebuild_metadata_flags(&mut self) {
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
        let same_rigid_bone = all_rigid
            && !self.sections.is_empty()
            && self
                .sections
                .iter()
                .all(|s| s.rigid_bone_index == self.sections[0].rigid_bone_index);
        self.rigid_only = same_rigid_bone;
        // all_sections_rigid uses the broader "effectively rigid" test
        self.all_sections_rigid = all_rigid;
        self.global_bones = any_global || all_global_indices;

        // all_sections_skinned: true only when every section is skinned AND
        // no section uses global_bones (matching original engine logic).
        self.all_sections_skinned = !any_global && !all_rigid && all_skinned;

        // rigid_bone_index: if a single rigid bone is used across all rigid
        // sections, use it; otherwise 0.
        if all_rigid && !self.sections.is_empty() {
            let first_rigid = self.sections[0].rigid_bone_index;
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
            .map(|s| s.num_verts as u32)
            .max()
            .unwrap_or(1);
        self.instance_index_multiplier = max_verts.next_power_of_two() as i16;

        // max_instances is set by artist tooling, not derivable from mesh data.
        // Preserve the existing value if already set (e.g. from glTF extras);
        // only default to 1 when it hasn't been initialised yet (0 or negative).
        if self.max_instances <= 0 {
            self.max_instances = 1;
        }
        // large_geom_bone_index defaults to i16::MAX (no large geom)
        self.large_geom_bone_index = i16::MAX;
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
    pub fn rebuild_instanced_index_buffer(&mut self) {
        let max_inst = self.max_instances as u32;
        let multiplier = self.instance_index_multiplier as u32;

        if max_inst <= 1 || multiplier == 0 {
            return;
        }

        // Compute the total base index count from sections.
        let base_index_count: usize = self
            .sections
            .iter()
            .map(|s| (s.num_tris as usize) * 3)
            .sum();

        // If the buffer already has the expected instanced size, skip.
        let expected_instanced = base_index_count * max_inst as usize;
        if self.index_buffer.len() == expected_instanced {
            return;
        }

        // If the buffer doesn't match base size either, skip to avoid corruption.
        if self.index_buffer.len() != base_index_count {
            return;
        }

        // Build the instanced buffer: for each instance i (0..max_instances),
        // copy every section's indices with vertex indices offset by
        // i * instance_index_multiplier.
        let mut instanced = Vec::with_capacity(expected_instanced);
        for inst in 0..max_inst {
            let vertex_offset = (inst * multiplier) as u16;
            for section in &self.sections {
                let sec_start = section.ib_offset as usize;
                let sec_count = (section.num_tris as usize) * 3;
                let sec_end = sec_start + sec_count;
                if sec_end > self.index_buffer.len() {
                    continue;
                }
                for &idx in &self.index_buffer[sec_start..sec_end] {
                    instanced.push(idx.wrapping_add(vertex_offset));
                }
            }
        }

        // Update section ib_offsets for the new sequential layout.
        let mut offset = 0i32;
        for section in &mut self.sections {
            section.ib_offset = offset;
            offset += section.num_tris * 3;
        }

        self.index_buffer = instanced;
    }
}
