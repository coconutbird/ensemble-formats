//! XTT data types.
//!
//! Parsing uses the zero-copy overlay types [`XttHeaderRaw`] /
//! [`XttLinkerHeaderRaw`] / [`AlbedoHeaderRaw`] (via `zerocopy`) and then
//! converts into the friendlier native-endian structs.

use alloc::string::String;
use alloc::vec::Vec;
use zerocopy::{FromBytes, Immutable, KnownLayout};

// ============================================================================
// Zero-copy overlay structs
// ============================================================================

/// Raw on-disk XTT file header (16 bytes, big-endian).
#[derive(FromBytes, KnownLayout, Immutable, Debug)]
#[repr(C)]
pub struct XttHeaderRaw {
    pub version: [u8; 4],
    pub num_active_textures: [u8; 4],
    pub num_active_decals: [u8; 4],
    pub num_active_decal_instances: [u8; 4],
}

/// Raw on-disk XTT linker fixed header (36 bytes, big-endian).
#[derive(FromBytes, KnownLayout, Immutable, Debug)]
#[repr(C)]
pub struct XttLinkerHeaderRaw {
    pub grid_x: [u8; 4],
    pub grid_z: [u8; 4],
    pub spec_pass_needed: [u8; 4],
    pub self_pass_needed: [u8; 4],
    pub env_mask_pass_needed: [u8; 4],
    pub alpha_pass_needed: [u8; 4],
    pub is_fully_opaque: [u8; 4],
    pub num_splat_layers: [u8; 4],
    pub num_decal_layers: [u8; 4],
}

/// Raw on-disk albedo atlas header (16 bytes, big-endian).
#[derive(FromBytes, KnownLayout, Immutable, Debug)]
#[repr(C)]
pub struct AlbedoHeaderRaw {
    pub out_mem_size: [u8; 4],
    pub width: [u8; 4],
    pub height: [u8; 4],
    pub num_mips: [u8; 4],
}

// ============================================================================
// Parsed types
// ============================================================================

/// Metadata about a chunk for ECF reconstruction.
#[derive(Debug, Clone)]
pub struct ChunkMeta {
    /// Chunk ID.
    pub id: u64,
    /// Alignment as log2.
    pub alignment_log2: u8,
    /// Chunk flags.
    pub flags: u8,
    /// Resource flags.
    pub resource_flags: u16,
}

/// Active texture information.
///
/// From TerrainTexturing.h:
/// ```cpp
/// class BTerrainActiveTextureInfo {
///     BFixedString256 mFilename;
///     int mUScale;
///     int mVScale;
///     int mBlendOp;
/// };
/// ```
#[derive(Debug, Clone)]
pub struct ActiveTextureInfo {
    /// Local texture filename (e.g., "`arctic/snowdrift_01`").
    pub filename: String,
    /// U texture coordinate scale.
    pub u_scale: i32,
    /// V texture coordinate scale.
    pub v_scale: i32,
    /// Blend operation.
    pub blend_op: i32,
}

impl ActiveTextureInfo {
    /// Size of one active texture entry in bytes.
    /// 256 bytes for filename + 3 * 4 bytes for scales and blend op.
    pub const SIZE: usize = 256 + 4 + 4 + 4;
}

/// Decal texture information.
#[derive(Debug, Clone)]
pub struct ActiveDecalInfo {
    /// Local decal texture filename.
    pub filename: String,
}

impl ActiveDecalInfo {
    /// Size of one decal entry in bytes (just 256 bytes for filename).
    pub const SIZE: usize = 256;
}

/// Decal instance information.
#[derive(Debug, Clone)]
pub struct ActiveDecalInstance {
    /// Index into active decals array.
    pub active_decal_index: i32,
    /// Rotation angle.
    pub rotation: f32,
    /// Tile center X.
    pub tile_center_x: f32,
    /// Tile center Y.
    pub tile_center_y: f32,
    /// U scale.
    pub u_scale: f32,
    /// V scale.
    pub v_scale: f32,
}

impl ActiveDecalInstance {
    /// Size of one decal instance in bytes.
    pub const SIZE: usize = 4 + 5 * 4;
}

/// XTT file header.
///
/// From TerrainIO.h:
/// ```cpp
/// struct XTTHeader {
///     int mVersion;
///     int mNumActiveTextures;
///     int mNumActiveDecals;
///     int mNumActiveDecalInstances;
/// };
/// ```
#[derive(Debug, Clone, Default)]
pub struct XttHeader {
    /// File version (should be 0x0004).
    pub version: i32,
    /// Number of active textures.
    pub num_active_textures: i32,
    /// Number of active decals.
    pub num_active_decals: i32,
    /// Number of active decal instances.
    pub num_active_decal_instances: i32,
}

impl XttHeader {
    /// Size of `XTTHeader` in bytes.
    pub const SIZE: usize = 16;
}

/// Terrain atlas linker (per-chunk texture info).
///
/// From TerrainIO.h:
/// ```cpp
/// struct XTTLinker {
///     int gridX;
///     int gridZ;
///     int specPassNeeded;
///     int selfPassNeeded;
///     int envMaskPassNeeded;
///     int alphaPassNeeded;
///     int isFullyOpaque;
///     int numSplatLayers;
///     int numDecalLayers;
/// };
/// ```
#[derive(Debug, Clone, Default)]
pub struct XttLinker {
    /// Grid X position.
    pub grid_x: i32,
    /// Grid Z position.
    pub grid_z: i32,
    /// Whether specular pass is needed.
    pub spec_pass_needed: i32,
    /// Whether self-illumination pass is needed (used as bool).
    pub self_pass_needed: i32,
    /// Whether environment mask pass is needed (used as bool).
    pub env_mask_pass_needed: i32,
    /// Whether alpha pass is needed.
    pub alpha_pass_needed: i32,
    /// Whether chunk is fully opaque.
    pub is_fully_opaque: i32,
    /// Number of splat layers.
    pub num_splat_layers: i32,
    /// Number of decal layers.
    pub num_decal_layers: i32,
    /// Parsed splat layer active texture indices.
    pub splat_layer_ids: Vec<i32>,
    /// Raw splat alpha texture data (packed A4R4G4B4, 64x64, tile-swapped).
    pub splat_alpha_data: Vec<u8>,
    /// Parsed decal layer instance indices.
    pub decal_layer_ids: Vec<i32>,
    /// Raw decal alpha texture data (packed A4R4G4B4, 64x64, tile-swapped).
    pub decal_alpha_data: Vec<u8>,
}

impl XttLinker {
    /// Fixed header size (9 * 4 = 36 bytes).
    pub const HEADER_SIZE: usize = 36;
    /// Alpha texture width.
    pub const ALPHA_TEXTURE_WIDTH: usize = 64;
    /// Alpha texture height.
    pub const ALPHA_TEXTURE_HEIGHT: usize = 64;
    /// Bits per pixel for A4R4G4B4 format.
    pub const ALPHA_BPP: usize = 16;
}

/// Foliage set information.
///
/// From TerrainFoliage.cpp:
/// The foliage header contains the number of sets and filenames for each set.
/// The filename is expected as "foliage\\foliageset" format pointing to:
/// - art/{filename}_df.ddx (diffuse/albedo)
/// - art/{filename}_nm.ddx (normal)
/// - art/{filename}_sp.ddx (specular)
/// - art/{filename}_op.ddx (opacity)
/// - art/{filename}.xml (positions/normals for blade geometry)
#[derive(Debug, Clone)]
pub struct FoliageSetInfo {
    /// Filename path (e.g., "foliage\\foliageset").
    pub filename: String,
}

/// Foliage quad-node chunk data.
///
/// From TerrainFoliage.h:
/// ```cpp
/// class BTerrainFoliageQNChunk {
///    uint mQNParentIndex;
///    uint mNumSets;
///    int *mSetIndexes;
///    int *mSetPolyCount;
///    LPDIRECT3DINDEXBUFFER9 *mSetIBs;
///    void *mpPhysicalMemoryPointer;
/// };
/// ```
#[derive(Debug, Clone)]
pub struct FoliageQNChunk {
    /// Parent quad-node index.
    pub qn_parent_index: u32,
    /// Number of foliage sets used in this chunk.
    pub num_sets: u32,
    /// Indices into the foliage sets array.
    pub set_indices: Vec<i32>,
    /// Polygon count for each set (for `DrawIndexedPrimitive`).
    pub set_poly_counts: Vec<i32>,
    /// Raw index buffer data for each set.
    pub index_buffers: Vec<Vec<u8>>,
}

impl FoliageQNChunk {
    /// Decode raw index buffer bytes into the packed `u32` vertex IDs consumed
    /// by the PC foliage shader.
    ///
    /// Each big-endian word stores the blade geometry type in its upper 16 bits
    /// and `local_blade_index * 10 + vertex_in_blade` in its lower 16 bits.
    /// `0x0000_FFFF` is the triangle-strip restart value.
    /// Returns `None` if `set` is out of range.
    #[must_use]
    pub fn decode_indices(&self, set: usize) -> Option<Vec<u32>> {
        let buf = self.index_buffers.get(set)?;
        let count = buf.len() / 4;
        let mut indices = Vec::with_capacity(count);
        let (words, _) = buf.as_chunks::<4>();
        for &bytes in words {
            indices.push(u32::from_be_bytes(bytes));
        }
        Some(indices)
    }
}

#[cfg(test)]
mod foliage_index_tests {
    use super::FoliageQNChunk;
    use alloc::vec;

    #[test]
    fn decodes_pc_foliage_indices_as_big_endian_words() {
        let chunk = FoliageQNChunk {
            qn_parent_index: 0,
            num_sets: 1,
            set_indices: vec![0],
            set_poly_counts: vec![1],
            index_buffers: vec![vec![0x00, 0x03, 0x88, 0xEA, 0x00, 0x00, 0xFF, 0xFF]],
        };

        assert_eq!(
            chunk.decode_indices(0),
            Some(vec![0x0003_88EA, 0x0000_FFFF])
        );
    }
}

/// Foliage data.
#[derive(Debug, Clone, Default)]
pub struct XttFoliage {
    /// Foliage sets defined in the header.
    pub sets: Vec<FoliageSetInfo>,
    /// Foliage QN chunks (per-quad-node foliage data).
    pub qn_chunks: Vec<FoliageQNChunk>,
}

/// Complete XTT file data.
#[derive(Debug, Clone)]
pub struct XttFile {
    /// ECF file ID (for round-trip).
    pub ecf_file_id: u32,
    /// ECF header flags (for round-trip).
    pub ecf_flags: u16,
    /// Chunk metadata in original order (for round-trip).
    pub chunk_order: Vec<ChunkMeta>,
    /// Main header.
    pub header: XttHeader,
    /// Unparsed bytes after the typed texture, decal, and instance tables.
    ///
    /// The writer rebuilds those typed tables and appends this tail verbatim.
    pub header_extra: Vec<u8>,
    /// Parsed active texture definitions.
    pub active_textures: Vec<ActiveTextureInfo>,
    /// Parsed active decal definitions.
    pub active_decals: Vec<ActiveDecalInfo>,
    /// Parsed decal instances.
    pub decal_instances: Vec<ActiveDecalInstance>,
    /// Atlas linker chunks (one per terrain tile).
    pub linkers: Vec<XttLinker>,
    /// Albedo atlas texture data.
    pub albedo_data: Vec<u8>,
    /// Road data (optional).
    pub road_data: Vec<u8>,
    /// Foliage data.
    pub foliage: XttFoliage,
}

impl Default for XttFile {
    fn default() -> Self {
        Self {
            ecf_file_id: 0x0007_7826,
            ecf_flags: 0,
            chunk_order: Vec::new(),
            header: XttHeader::default(),
            header_extra: Vec::new(),
            active_textures: Vec::new(),
            active_decals: Vec::new(),
            decal_instances: Vec::new(),
            linkers: Vec::new(),
            albedo_data: Vec::new(),
            road_data: Vec::new(),
            foliage: XttFoliage::default(),
        }
    }
}

impl XttFile {
    /// Resolve a splat layer ID from a linker to its [`ActiveTextureInfo`].
    ///
    /// `layer_id` is an entry from [`XttLinker::splat_layer_ids`] which indexes
    /// into [`XttFile::active_textures`].
    #[must_use]
    pub fn resolve_splat_texture(&self, layer_id: i32) -> Option<&ActiveTextureInfo> {
        self.active_textures.get(usize::try_from(layer_id).ok()?)
    }

    /// Resolve a decal layer ID from a linker to its [`ActiveDecalInstance`].
    ///
    /// `layer_id` is an entry from [`XttLinker::decal_layer_ids`] which indexes
    /// into [`XttFile::decal_instances`].
    #[must_use]
    pub fn resolve_decal_instance(&self, layer_id: i32) -> Option<&ActiveDecalInstance> {
        self.decal_instances.get(usize::try_from(layer_id).ok()?)
    }

    /// Resolve a decal layer ID all the way to its [`ActiveDecalInfo`] (texture filename).
    ///
    /// Follows the chain: `decal_layer_ids[i]` → `decal_instances[idx]` →
    /// `active_decals[active_decal_index]`.
    #[must_use]
    pub fn resolve_decal_info(&self, layer_id: i32) -> Option<&ActiveDecalInfo> {
        let instance = self.resolve_decal_instance(layer_id)?;
        self.active_decals
            .get(usize::try_from(instance.active_decal_index).ok()?)
    }
}

// ============================================================================
// Road Data Types
// ============================================================================

/// A single road vertex with position and UV.
#[derive(Clone, Debug)]
pub struct RoadVertex {
    /// World position (X, Y, Z).
    pub position: [f32; 3],
    /// Texture UV coordinates.
    pub uv: [f32; 2],
}

/// Road triangles assigned to a specific quad-node (terrain chunk).
#[derive(Clone, Debug)]
pub struct RoadQNChunk {
    /// Owner quad-node index (qnX * numQNs + qnZ).
    pub qn_index: i32,
    /// Triangle vertices (every 3 vertices = 1 triangle).
    pub vertices: Vec<RoadVertex>,
}

/// Decoded road data from XTT chunk 0x8888.
#[derive(Clone, Debug)]
pub struct RoadData {
    /// Road texture name (e.g., "roads\\`road_01`").
    pub texture_name: String,
    /// Per-chunk road geometry.
    pub qn_chunks: Vec<RoadQNChunk>,
}
