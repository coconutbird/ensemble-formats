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
    /// Local texture filename (e.g., "arctic/snowdrift_01").
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
            ecf_file_id: 0x00077826,
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
