//! Post-import processing for `UgxGeom`.
//!
//! After a glTF round-trip, several engine-computed fields are lost (AABB tree,
//! bone bounds, accessories, metadata flags). The submodules here recompute all
//! of them from the primary mesh/skeleton data so that re-exported UGX files are
//! byte-level functional equivalents of the originals.

mod aabb_tree;
mod accessories;
mod bounds;
mod metadata;

use crate::{Result, UgxGeom};

impl UgxGeom {
    /// Rebuild **all** derived data from the primary mesh/skeleton data.
    ///
    /// This is the one-stop call after a glTF import. It recomputes:
    /// - Global bounding volumes (`bounds`, `bounding_sphere`)
    /// - Per-bone bounding boxes (`bone_bounds`)
    /// - Metadata flags (`rigid_only`, `all_sections_rigid`, etc.)
    /// - Instanced index buffer (baked copies for `max_instances`)
    /// - AABB tree (spatial acceleration structure)
    /// - Accessories (must be built AFTER the tree — accessories are indexed
    ///   by tree node index at runtime)
    ///
    /// # Errors
    ///
    /// Returns an error if source geometry is malformed or any derived count,
    /// offset, or index cannot be represented by the UGX format.
    pub fn rebuild_derived_data(&mut self) -> Result<()> {
        self.rebuild_bounds()?;
        self.rebuild_bone_bounds()?;
        self.rebuild_metadata_flags()?;
        self.rebuild_instanced_index_buffer()?;
        // Tree must be built first — the per-node section mapping it produces
        // is consumed by rebuild_accessories to satisfy the engine invariant:
        //   accessories[node_index].object_indices == sections for that node.
        let node_section_map = self.rebuild_aabb_tree()?;
        self.rebuild_accessories(node_section_map)?;
        Ok(())
    }
}
