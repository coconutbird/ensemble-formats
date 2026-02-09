//! XTT data types.

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
    /// Size of XTTHeader in bytes.
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
    /// Number of splat layers (aligned to multiple of 4).
    pub num_splat_layers: i32,
    /// Number of decal layers (aligned to multiple of 4).
    pub num_decal_layers: i32,
    /// Raw splat layer data.
    pub splat_data: Vec<u8>,
    /// Raw decal layer data.
    pub decal_data: Vec<u8>,
}

impl XttLinker {
    /// Fixed header size (9 * 4 = 36 bytes).
    pub const HEADER_SIZE: usize = 36;
}

/// Foliage data.
#[derive(Debug, Clone, Default)]
pub struct XttFoliage {
    /// Foliage header data.
    pub header_data: Vec<u8>,
    /// Foliage quantization chunks.
    pub qn_chunks: Vec<Vec<u8>>,
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
    /// Header chunk raw data (includes texture info beyond base header).
    pub header_extra: Vec<u8>,
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
            ecf_file_id: 0x00077826,
            ecf_flags: 0,
            chunk_order: Vec::new(),
            header: XttHeader::default(),
            header_extra: Vec::new(),
            linkers: Vec::new(),
            albedo_data: Vec::new(),
            road_data: Vec::new(),
            foliage: XttFoliage::default(),
        }
    }
}
