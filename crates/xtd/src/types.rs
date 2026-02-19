//! XTD data types.

/// Tessellation data for terrain patches.
///
/// From the binary analysis:
/// - Each patch has a max tessellation level (1 byte)
/// - Each patch has a bounding box (32 bytes = 2x XMFLOAT4 for min/max)
#[derive(Debug, Clone, Default)]
pub struct TessellationData {
    /// Number of patches along X axis.
    pub num_x_patches: i32,
    /// Number of patches along Z axis.
    pub num_z_patches: i32,
    /// Maximum tessellation level (computed from all patches).
    pub max_tess_level: u8,
    /// Per-patch maximum tessellation levels (num_x_patches * num_z_patches).
    pub patch_tess_levels: Vec<u8>,
    /// Per-patch bounding boxes (min/max as [f32; 4] each).
    pub patch_bounding_boxes: Vec<PatchBoundingBox>,
}

impl TessellationData {
    /// Get the total number of patches.
    pub fn num_patches(&self) -> usize {
        (self.num_x_patches * self.num_z_patches) as usize
    }

    /// Get the tessellation level for a specific patch.
    pub fn get_patch_tess_level(&self, x: i32, z: i32) -> Option<u8> {
        if x < 0 || x >= self.num_x_patches || z < 0 || z >= self.num_z_patches {
            return None;
        }
        let index = (z * self.num_x_patches + x) as usize;
        self.patch_tess_levels.get(index).copied()
    }

    /// Get the bounding box for a specific patch.
    pub fn get_patch_bbox(&self, x: i32, z: i32) -> Option<&PatchBoundingBox> {
        if x < 0 || x >= self.num_x_patches || z < 0 || z >= self.num_z_patches {
            return None;
        }
        let index = (z * self.num_x_patches + x) as usize;
        self.patch_bounding_boxes.get(index)
    }
}

/// Bounding box for a tessellation patch.
///
/// 32 bytes total: min (16 bytes) + max (16 bytes)
/// Each is an XMFLOAT4 (x, y, z, w where w is typically 1.0)
#[derive(Debug, Clone, Default)]
pub struct PatchBoundingBox {
    /// Minimum corner of the bounding box (x, y, z, w).
    pub min: [f32; 4],
    /// Maximum corner of the bounding box (x, y, z, w).
    pub max: [f32; 4],
}

impl PatchBoundingBox {
    /// Size of a patch bounding box in bytes.
    pub const SIZE: usize = 32;
}

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

impl XtdFile {
    /// Decode tessellation data from the raw tess_data chunk.
    ///
    /// Returns None if there is no tessellation data.
    pub fn decode_tessellation(&self) -> Option<TessellationData> {
        use byteorder::{BigEndian, ReadBytesExt};
        use std::io::Cursor;

        if self.tess_data.is_empty() {
            return None;
        }

        let mut cursor = Cursor::new(&self.tess_data);

        // Read patch counts
        let num_x_patches = cursor.read_i32::<BigEndian>().ok()?;
        let num_z_patches = cursor.read_i32::<BigEndian>().ok()?;

        let num_patches = (num_x_patches * num_z_patches) as usize;
        if num_patches == 0 {
            return None;
        }

        // Read per-patch tessellation levels (1 byte each)
        let mut patch_tess_levels = vec![0u8; num_patches];
        std::io::Read::read_exact(&mut cursor, &mut patch_tess_levels).ok()?;

        // Calculate max tessellation level
        let max_tess_level = *patch_tess_levels.iter().max().unwrap_or(&0);

        // Read per-patch bounding boxes (32 bytes each)
        let mut patch_bounding_boxes = Vec::with_capacity(num_patches);
        for _ in 0..num_patches {
            let bbox = PatchBoundingBox {
                min: [
                    cursor.read_f32::<BigEndian>().ok()?,
                    cursor.read_f32::<BigEndian>().ok()?,
                    cursor.read_f32::<BigEndian>().ok()?,
                    cursor.read_f32::<BigEndian>().ok()?,
                ],
                max: [
                    cursor.read_f32::<BigEndian>().ok()?,
                    cursor.read_f32::<BigEndian>().ok()?,
                    cursor.read_f32::<BigEndian>().ok()?,
                    cursor.read_f32::<BigEndian>().ok()?,
                ],
            };
            patch_bounding_boxes.push(bbox);
        }

        Some(TessellationData {
            num_x_patches,
            num_z_patches,
            max_tess_level,
            patch_tess_levels,
            patch_bounding_boxes,
        })
    }
}
