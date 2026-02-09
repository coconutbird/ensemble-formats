//! XTD data types.

/// XTD file header.
///
/// From TerrainIO.h:
/// ```cpp
/// struct XTDHeader {
///     int mVersion;
///     int mNumXVerts;
///     int mNumXChunks;
///     float mTileScale;
///     D3DXVECTOR3 worldMin;
///     D3DXVECTOR3 worldMax;
/// };
/// ```
#[derive(Debug, Clone, Default)]
pub struct XtdHeader {
    /// File version (should be 0x000C).
    pub version: i32,
    /// Number of vertices along X axis.
    pub num_x_verts: i32,
    /// Number of terrain chunks.
    pub num_x_chunks: i32,
    /// Tile scale factor.
    pub tile_scale: f32,
    /// World minimum bounds (x, y, z).
    pub world_min: [f32; 3],
    /// World maximum bounds (x, y, z).
    pub world_max: [f32; 3],
}

impl XtdHeader {
    /// Size of XTDHeader in bytes.
    pub const SIZE: usize = 40;
}

/// Terrain visual chunk header.
///
/// From TerrainIO.h:
/// ```cpp
/// struct XTDVisualChunkHeader {
///     int gridX;
///     int gridZ;
///     int maxVStride;
///     D3DXVECTOR3 mmin;
///     D3DXVECTOR3 mmax;
///     bool canCastShadows;
/// };
/// ```
#[derive(Debug, Clone, Default)]
pub struct XtdVisualChunk {
    /// Grid X position.
    pub grid_x: i32,
    /// Grid Z position.
    pub grid_z: i32,
    /// Maximum vertex stride.
    pub max_v_stride: i32,
    /// Chunk minimum bounds (x, y, z).
    pub min: [f32; 3],
    /// Chunk maximum bounds (x, y, z).
    pub max: [f32; 3],
    /// Whether this chunk can cast shadows.
    pub can_cast_shadows: bool,
}

impl XtdVisualChunk {
    /// Size of XTDVisualChunkHeader in bytes (3*4 + 6*4 + 1 = 37).
    pub const SIZE: usize = 37;
}

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

/// Complete XTD file data.
#[derive(Debug, Clone)]
pub struct XtdFile {
    /// ECF file ID (for round-trip).
    pub ecf_file_id: u32,
    /// ECF header flags (for round-trip).
    pub ecf_flags: u16,
    /// Chunk metadata in original order (for round-trip).
    pub chunk_order: Vec<ChunkMeta>,
    /// Main header.
    pub header: XtdHeader,
    /// Visual chunks (one per terrain tile).
    pub visual_chunks: Vec<XtdVisualChunk>,
    /// Atlas texture data (compressed DXT format).
    pub atlas_data: Vec<u8>,
    /// Tessellation data.
    pub tess_data: Vec<u8>,
    /// Lighting data.
    pub lighting_data: Vec<u8>,
    /// Ambient occlusion data.
    pub ao_data: Vec<u8>,
    /// Alpha/transparency data.
    pub alpha_data: Vec<u8>,
}

impl Default for XtdFile {
    fn default() -> Self {
        Self {
            ecf_file_id: 0x00077826,
            ecf_flags: 0,
            chunk_order: Vec::new(),
            header: XtdHeader::default(),
            visual_chunks: Vec::new(),
            atlas_data: Vec::new(),
            tess_data: Vec::new(),
            lighting_data: Vec::new(),
            ao_data: Vec::new(),
            alpha_data: Vec::new(),
        }
    }
}

