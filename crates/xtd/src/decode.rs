//! XTD terrain vertex decoding utilities.
//!
//! The atlas chunk contains packed vertex positions and normals.
//! This module provides functions to decode them for rendering.
//!
//! The DE (PC) version stores data as LittleEndian with PC R10G10B10A2 bit layout.
//! The original Xbox 360 data was BigEndian with tiled (swizzled) textures.

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;

use crate::{Error, Result, XtdFile};

/// Xbox 360 texture tile size for 32-bit formats (R11G11B10, etc.)
/// Kept for potential future use with original Xbox 360 data.
#[allow(dead_code)]
const TILE_SIZE: usize = 32;

/// Un-tile Xbox 360 texture data.
///
/// Xbox 360 textures are stored in a tiled format for GPU cache efficiency.
/// This function converts tiled data back to linear row-major order.
///
/// For 32-bit formats, tiles are 32x32 pixels.
///
/// Note: The DE (PC) version appears to already use linear layout,
/// so this function may not be needed. Kept for potential future use.
#[allow(dead_code)]
fn untile_texture(tiled: &[u32], width: usize, height: usize) -> Vec<u32> {
    let mut linear = vec![0u32; width * height];

    let tiles_x = width / TILE_SIZE;
    let tiles_y = height / TILE_SIZE;

    for tile_y in 0..tiles_y {
        for tile_x in 0..tiles_x {
            // Calculate base offset for this tile in the tiled data
            let tile_index = tile_y * tiles_x + tile_x;
            let tile_base = tile_index * TILE_SIZE * TILE_SIZE;

            // Un-tile each pixel within the tile
            for local_y in 0..TILE_SIZE {
                for local_x in 0..TILE_SIZE {
                    // Calculate the swizzled index within the tile
                    // Xbox 360 uses Morton code (Z-order curve) within tiles
                    let swizzled_idx = morton_index(local_x, local_y);
                    let tiled_idx = tile_base + swizzled_idx;

                    // Calculate linear destination
                    let global_x = tile_x * TILE_SIZE + local_x;
                    let global_y = tile_y * TILE_SIZE + local_y;
                    let linear_idx = global_y * width + global_x;

                    if tiled_idx < tiled.len() && linear_idx < linear.len() {
                        linear[linear_idx] = tiled[tiled_idx];
                    }
                }
            }
        }
    }

    linear
}

/// Calculate Morton code (Z-order curve) for 2D coordinates.
///
/// This interleaves the bits of x and y to create the swizzled index.
/// Used by `untile_texture` for Xbox 360 tiled data.
#[allow(dead_code)]
#[inline]
fn morton_index(x: usize, y: usize) -> usize {
    let mut result = 0;
    for i in 0..16 {
        result |= ((x >> i) & 1) << (2 * i);
        result |= ((y >> i) & 1) << (2 * i + 1);
    }
    result
}

/// Xbox 360 tile size for R8 (8-bit) format textures.
/// R8 uses 8x8 tiles (64 bytes per tile).
const R8_TILE_SIZE: usize = 8;

/// Un-tile Xbox 360 R8 texture data.
///
/// Xbox 360 R8 textures use 8x8 tiles with Morton (Z-order) swizzling within tiles.
/// This converts tiled data back to linear row-major order.
fn untile_r8_texture(tiled: &[u8], width: usize, height: usize) -> Vec<u8> {
    let mut linear = vec![0u8; width * height];

    let tiles_x = width.div_ceil(R8_TILE_SIZE);
    let tiles_y = height.div_ceil(R8_TILE_SIZE);

    for tile_y in 0..tiles_y {
        for tile_x in 0..tiles_x {
            // Calculate base offset for this tile in the tiled data
            let tile_index = tile_y * tiles_x + tile_x;
            let tile_base = tile_index * R8_TILE_SIZE * R8_TILE_SIZE;

            // Un-tile each pixel within the tile
            for local_y in 0..R8_TILE_SIZE {
                for local_x in 0..R8_TILE_SIZE {
                    // Calculate global position
                    let global_x = tile_x * R8_TILE_SIZE + local_x;
                    let global_y = tile_y * R8_TILE_SIZE + local_y;

                    // Skip if outside texture bounds
                    if global_x >= width || global_y >= height {
                        continue;
                    }

                    // Calculate the swizzled index within the tile using Morton code
                    let swizzled_idx = morton_index(local_x, local_y);
                    let tiled_idx = tile_base + swizzled_idx;

                    // Calculate linear destination
                    let linear_idx = global_y * width + global_x;

                    if tiled_idx < tiled.len() && linear_idx < linear.len() {
                        linear[linear_idx] = tiled[tiled_idx];
                    }
                }
            }
        }
    }

    linear
}

/// Atlas chunk header containing position encoding parameters.
///
/// From ExportXTD.cs:
/// ```cpp
/// // mid: Vec3 + padding (16 bytes)
/// // range: Vec3 + padding (16 bytes)
/// ```
#[derive(Debug, Clone, Default)]
pub struct AtlasHeader {
    /// Mid-point offset for position decoding (x, y, z).
    pub mid: [f32; 3],
    /// Range scale for position decoding (x, y, z).
    pub range: [f32; 3],
}

impl AtlasHeader {
    /// Size of atlas header in bytes (2 * Vec4 = 32 bytes).
    pub const SIZE: usize = 32;

    /// Parse atlas header from raw bytes.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        if data.len() < Self::SIZE {
            return Err(Error::InvalidChunkData(format!(
                "Atlas header too small: {} < {}",
                data.len(),
                Self::SIZE
            )));
        }

        let f = |off: usize| f32::from_be_bytes(data[off..off + 4].try_into().unwrap());
        Ok(Self {
            mid: [f(0), f(4), f(8)],
            // Skip padding at bytes 12-15
            range: [f(16), f(20), f(24)],
            // Skip padding at bytes 28-31
        })
    }
}

/// Unpack a 32-bit R10G10B10A2 packed position into a displacement vector.
///
/// DE/PC data format (DXGI_FORMAT_R10G10B10A2_UNORM):
/// - R: bits 0-9   (10 bits) → X displacement
/// - G: bits 10-19 (10 bits) → Y displacement (height)
/// - B: bits 20-29 (10 bits) → Z displacement
/// - A: bits 30-31 (2 bits)  → unused
///
/// Data is stored as LittleEndian in the DE PC files.
///
/// The unpacked value represents a displacement from the base grid position.
/// Formula: displacement = (bits / 1023) * range - mid
#[inline]
pub fn unpack_position(packed: u32, mid: &[f32; 3], range: &[f32; 3]) -> [f32; 3] {
    const BIT_MAX_10: f32 = 1023.0;

    // Extract 10-bit components - PC R10G10B10A2 format
    // R = bits 0-9 (X), G = bits 10-19 (Y), B = bits 20-29 (Z)
    let x_bits = (packed & 0x3FF) as f32;
    let y_bits = ((packed >> 10) & 0x3FF) as f32;
    let z_bits = ((packed >> 20) & 0x3FF) as f32;

    // Convert to normalized [0, 1] range, then apply range and offset
    [
        (x_bits / BIT_MAX_10) * range[0] - mid[0],
        (y_bits / BIT_MAX_10) * range[1] - mid[1],
        (z_bits / BIT_MAX_10) * range[2] - mid[2],
    ]
}

/// Unpack a 32-bit packed normal value.
///
/// Normals are packed as: ((norm + 1) * 0.5) * 1023
/// So unpacking is: (bits / 1023) * 2 - 1
#[inline]
pub fn unpack_normal(packed: u32) -> [f32; 3] {
    const BIT_MAX_10: f32 = 1023.0;

    // All three use 10-bit values (0-1023)
    let x_bits = ((packed >> 22) & 0x3FF) as f32;
    let y_bits = ((packed >> 11) & 0x3FF) as f32;
    let z_bits = (packed & 0x3FF) as f32;

    [
        (x_bits / BIT_MAX_10) * 2.0 - 1.0,
        (y_bits / BIT_MAX_10) * 2.0 - 1.0,
        (z_bits / BIT_MAX_10) * 2.0 - 1.0,
    ]
}

/// Decoded terrain vertex data.
#[derive(Debug, Clone)]
pub struct TerrainVertices {
    /// Atlas encoding header.
    pub header: AtlasHeader,
    /// Decoded vertex positions.
    pub positions: Vec<[f32; 3]>,
    /// Decoded vertex normals.
    pub normals: Vec<[f32; 3]>,
    /// Texture coordinates for terrain atlas (0-1 range).
    pub uvs: Vec<[f32; 2]>,
    /// Number of vertices per axis (square terrain).
    pub num_verts_per_axis: usize,
}

/// Raw packed terrain data for GPU tessellation.
///
/// Contains the packed position/normal data that can be uploaded as textures
/// and decoded in the vertex shader for GPU-based tessellation.
#[derive(Debug, Clone)]
pub struct RawTerrainData {
    /// Packed position data (R10G10B10A2 format, one u32 per vertex).
    pub packed_positions: Vec<u32>,
    /// Packed normal data (one u32 per vertex).
    pub packed_normals: Vec<u32>,
    /// Number of vertices per axis (e.g., 1025 for 1024x1024 terrain).
    pub num_verts_per_axis: u32,
    /// Atlas mid point for position decoding.
    pub mid: [f32; 3],
    /// Atlas range for position decoding.
    pub range: [f32; 3],
    /// Tile scale for world position calculation.
    pub tile_scale: f32,
    /// World min bounds.
    pub world_min: [f32; 3],
    /// World max bounds.
    pub world_max: [f32; 3],
}

impl XtdFile {
    /// Extract raw packed terrain data for GPU tessellation.
    ///
    /// This returns the packed position/normal data that can be uploaded
    /// as GPU textures and decoded in the vertex shader.
    pub fn extract_raw_data(&self) -> Result<RawTerrainData> {
        if self.atlas_data.is_empty() {
            return Err(Error::InvalidChunkData("Atlas chunk is empty".to_string()));
        }

        let header = AtlasHeader::from_bytes(&self.atlas_data)?;
        let width = self.header.num_x_verts as usize;
        let num_verts = width * width;

        // Expected size: header + positions + normals
        let expected_size = AtlasHeader::SIZE + num_verts * 4 + num_verts * 4;
        if self.atlas_data.len() < expected_size {
            return Err(Error::InvalidChunkData(format!(
                "Atlas data too small: {} < {} (expected {} verts)",
                self.atlas_data.len(),
                expected_size,
                num_verts
            )));
        }

        let pos_start = AtlasHeader::SIZE;
        let norm_start = pos_start + num_verts * 4;

        let mut packed_positions = Vec::with_capacity(num_verts);
        let mut packed_normals = Vec::with_capacity(num_verts);

        for i in 0..num_verts {
            let pos_offset = pos_start + i * 4;
            packed_positions.push(u32::from_le_bytes(
                self.atlas_data[pos_offset..pos_offset + 4]
                    .try_into()
                    .unwrap(),
            ));

            let norm_offset = norm_start + i * 4;
            packed_normals.push(u32::from_le_bytes(
                self.atlas_data[norm_offset..norm_offset + 4]
                    .try_into()
                    .unwrap(),
            ));
        }

        Ok(RawTerrainData {
            packed_positions,
            packed_normals,
            num_verts_per_axis: width as u32,
            mid: header.mid,
            range: header.range,
            tile_scale: self.header.tile_scale,
            world_min: self.header.world_min,
            world_max: self.header.world_max,
        })
    }

    /// Decode terrain vertices from the atlas chunk.
    ///
    /// Returns positions and normals as Vec<[f32; 3]>.
    pub fn decode_vertices(&self) -> Result<TerrainVertices> {
        if self.atlas_data.is_empty() {
            return Err(Error::InvalidChunkData("Atlas chunk is empty".to_string()));
        }

        let header = AtlasHeader::from_bytes(&self.atlas_data)?;

        let width = self.header.num_x_verts as usize;
        let num_verts = width * width;

        // Expected size: header + positions + normals
        let expected_size = AtlasHeader::SIZE + num_verts * 4 + num_verts * 4;
        if self.atlas_data.len() < expected_size {
            return Err(Error::InvalidChunkData(format!(
                "Atlas data too small: {} < {} (expected {} verts)",
                self.atlas_data.len(),
                expected_size,
                num_verts
            )));
        }

        let pos_start = AtlasHeader::SIZE;
        let norm_start = pos_start + num_verts * 4;

        // DE (PC) version uses LittleEndian byte order and linear (non-tiled) layout
        let mut packed_positions = Vec::with_capacity(num_verts);
        let mut packed_normals = Vec::with_capacity(num_verts);

        for i in 0..num_verts {
            let pos_offset = pos_start + i * 4;
            packed_positions.push(u32::from_le_bytes(
                self.atlas_data[pos_offset..pos_offset + 4]
                    .try_into()
                    .unwrap(),
            ));

            let norm_offset = norm_start + i * 4;
            packed_normals.push(u32::from_le_bytes(
                self.atlas_data[norm_offset..norm_offset + 4]
                    .try_into()
                    .unwrap(),
            ));
        }

        // Unpack positions, normals, and compute UVs
        // The packed data stores DISPLACEMENTS from the base grid position.
        // World position = base_grid_position + displacement
        let tile_scale = self.header.tile_scale;
        let mut positions = Vec::with_capacity(num_verts);
        let mut normals = Vec::with_capacity(num_verts);
        let mut uvs = Vec::with_capacity(num_verts);
        let width_f = (width - 1) as f32;

        for i in 0..num_verts {
            // Standard grid mapping: row-major order
            // i % width = column = X position
            // i / width = row = Z position
            let grid_x = (i % width) as f32;
            let grid_z = (i / width) as f32;

            // The packed data contains position data that needs to be combined with grid position.
            // X/Z: grid position provides the base, packed data adds displacement
            // Y: comes entirely from the packed data (height)
            let unpacked = unpack_position(packed_positions[i], &header.mid, &header.range);

            // Use grid position for X/Z base, unpacked Y for height
            // The unpacked X/Z may be small displacements (detail offsets)
            positions.push([
                grid_x * tile_scale + unpacked[0],
                unpacked[1],
                grid_z * tile_scale + unpacked[2],
            ]);

            normals.push(unpack_normal(packed_normals[i]));

            // UV coordinates: Z→U, X→V (matching the game's convention).
            //
            // The original Halo Wars shaders consistently use world Z for the U axis
            // and world X for the V axis (e.g. the roads shader samples `gPos.zx`).
            // All terrain textures — splat alpha, albedo, AO — are authored for this
            // convention, so we adopt it here at the source rather than compensating
            // with rotation/transpose hacks downstream.
            let u = grid_z / width_f;
            let v = grid_x / width_f;
            uvs.push([u, v]);
        }

        Ok(TerrainVertices {
            header,
            positions,
            normals,
            uvs,
            num_verts_per_axis: width,
        })
    }
}

impl TerrainVertices {
    /// Generate triangle indices for rendering the terrain as a triangle list.
    ///
    /// The terrain is a regular grid, so we generate 2 triangles per quad.
    /// Returns indices in counter-clockwise winding order.
    pub fn generate_indices(&self) -> Vec<u32> {
        let n = self.num_verts_per_axis;
        if n < 2 {
            return Vec::new();
        }

        let num_quads = (n - 1) * (n - 1);
        let mut indices = Vec::with_capacity(num_quads * 6);

        for z in 0..(n - 1) {
            for x in 0..(n - 1) {
                let top_left = (z * n + x) as u32;
                let top_right = top_left + 1;
                let bottom_left = ((z + 1) * n + x) as u32;
                let bottom_right = bottom_left + 1;

                // First triangle (top-left, bottom-left, top-right)
                indices.push(top_left);
                indices.push(bottom_left);
                indices.push(top_right);

                // Second triangle (top-right, bottom-left, bottom-right)
                indices.push(top_right);
                indices.push(bottom_left);
                indices.push(bottom_right);
            }
        }

        indices
    }

    /// Get terrain dimensions in world units.
    pub fn world_size(&self) -> [f32; 3] {
        self.header.range
    }

    /// Generate tessellated terrain mesh with CPU subdivision.
    ///
    /// This subdivides patches based on their tessellation levels to produce
    /// a higher-resolution mesh similar to what the retail game achieves with
    /// GPU tessellation shaders.
    ///
    /// Returns new positions, normals, uvs, and indices for the tessellated mesh.
    pub fn tessellate(&self, tess_data: &crate::TessellationData) -> TessellatedMesh {
        // Using BTreeMap instead of HashMap for no_std compatibility

        let n = self.num_verts_per_axis;

        // Calculate vertices per patch (terrain is n x n, patches are num_x_patches x num_z_patches)
        // So each patch spans (n-1)/num_patches + 1 vertices
        let verts_per_patch_x = (n - 1) / tess_data.num_x_patches as usize + 1;
        let verts_per_patch_z = (n - 1) / tess_data.num_z_patches as usize + 1;

        // For efficient lookups, index existing vertices
        // Key: grid (x, z) position, Value: index in positions array
        let mut vertex_map: BTreeMap<(usize, usize), usize> = BTreeMap::new();
        for z in 0..n {
            for x in 0..n {
                vertex_map.insert((x, z), z * n + x);
            }
        }

        // Output buffers - start with copies of existing data
        let mut positions = self.positions.clone();
        let mut normals = self.normals.clone();
        let mut uvs = self.uvs.clone();
        let mut indices = Vec::new();

        // Track newly created vertices with fractional grid positions
        // Key: (x * 10000 + frac_x, z * 10000 + frac_z), Value: index
        let mut new_vertex_map: BTreeMap<(u64, u64), usize> = BTreeMap::new();

        // Helper to encode fractional position as u64
        let encode_pos =
            |x: f32, z: f32| -> (u64, u64) { ((x * 10000.0) as u64, (z * 10000.0) as u64) };

        // Helper to get or create interpolated vertex
        let get_or_create_vertex = |positions: &mut Vec<[f32; 3]>,
                                    normals: &mut Vec<[f32; 3]>,
                                    uvs: &mut Vec<[f32; 2]>,
                                    new_vertex_map: &mut BTreeMap<(u64, u64), usize>,
                                    vertex_map: &BTreeMap<(usize, usize), usize>,
                                    x: f32,
                                    z: f32,
                                    n: usize|
         -> usize {
            // Check if this is an existing integer vertex
            let ix = x as usize;
            let iz = z as usize;
            let frac_x = x - ix as f32;
            let frac_z = z - iz as f32;

            if frac_x.abs() < 0.001 && frac_z.abs() < 0.001 && ix < n && iz < n {
                // Exact grid vertex
                return *vertex_map.get(&(ix, iz)).unwrap();
            }

            // Check if we've already created this vertex
            let key = encode_pos(x, z);
            if let Some(&idx) = new_vertex_map.get(&key) {
                return idx;
            }

            // Bilinear interpolation
            let x0 = ix.min(n - 2);
            let z0 = iz.min(n - 2);
            let x1 = (x0 + 1).min(n - 1);
            let z1 = (z0 + 1).min(n - 1);

            let fx = x - x0 as f32;
            let fz = z - z0 as f32;

            let i00 = *vertex_map.get(&(x0, z0)).unwrap();
            let i10 = *vertex_map.get(&(x1, z0)).unwrap();
            let i01 = *vertex_map.get(&(x0, z1)).unwrap();
            let i11 = *vertex_map.get(&(x1, z1)).unwrap();

            // Interpolate position
            let p00 = positions[i00];
            let p10 = positions[i10];
            let p01 = positions[i01];
            let p11 = positions[i11];

            let pos = [
                (1.0 - fx) * (1.0 - fz) * p00[0]
                    + fx * (1.0 - fz) * p10[0]
                    + (1.0 - fx) * fz * p01[0]
                    + fx * fz * p11[0],
                (1.0 - fx) * (1.0 - fz) * p00[1]
                    + fx * (1.0 - fz) * p10[1]
                    + (1.0 - fx) * fz * p01[1]
                    + fx * fz * p11[1],
                (1.0 - fx) * (1.0 - fz) * p00[2]
                    + fx * (1.0 - fz) * p10[2]
                    + (1.0 - fx) * fz * p01[2]
                    + fx * fz * p11[2],
            ];

            // Interpolate and renormalize normal
            let n00 = normals[i00];
            let n10 = normals[i10];
            let n01 = normals[i01];
            let n11 = normals[i11];

            let mut norm = [
                (1.0 - fx) * (1.0 - fz) * n00[0]
                    + fx * (1.0 - fz) * n10[0]
                    + (1.0 - fx) * fz * n01[0]
                    + fx * fz * n11[0],
                (1.0 - fx) * (1.0 - fz) * n00[1]
                    + fx * (1.0 - fz) * n10[1]
                    + (1.0 - fx) * fz * n01[1]
                    + fx * fz * n11[1],
                (1.0 - fx) * (1.0 - fz) * n00[2]
                    + fx * (1.0 - fz) * n10[2]
                    + (1.0 - fx) * fz * n01[2]
                    + fx * fz * n11[2],
            ];
            let len = (norm[0] * norm[0] + norm[1] * norm[1] + norm[2] * norm[2]).sqrt();
            if len > 0.001 {
                norm[0] /= len;
                norm[1] /= len;
                norm[2] /= len;
            }

            // Interpolate UV
            let uv00 = uvs[i00];
            let uv10 = uvs[i10];
            let uv01 = uvs[i01];
            let uv11 = uvs[i11];

            let uv = [
                (1.0 - fx) * (1.0 - fz) * uv00[0]
                    + fx * (1.0 - fz) * uv10[0]
                    + (1.0 - fx) * fz * uv01[0]
                    + fx * fz * uv11[0],
                (1.0 - fx) * (1.0 - fz) * uv00[1]
                    + fx * (1.0 - fz) * uv10[1]
                    + (1.0 - fx) * fz * uv01[1]
                    + fx * fz * uv11[1],
            ];

            let idx = positions.len();
            positions.push(pos);
            normals.push(norm);
            uvs.push(uv);
            new_vertex_map.insert(key, idx);
            idx
        };

        // Process each patch
        for patch_z in 0..tess_data.num_z_patches as usize {
            for patch_x in 0..tess_data.num_x_patches as usize {
                let patch_idx = patch_z * tess_data.num_x_patches as usize + patch_x;
                let tess_level = tess_data.patch_tess_levels[patch_idx];

                // Vertex range for this patch
                let base_x = patch_x * (verts_per_patch_x - 1);
                let base_z = patch_z * (verts_per_patch_z - 1);
                let end_x = (base_x + verts_per_patch_x - 1).min(n - 1);
                let end_z = (base_z + verts_per_patch_z - 1).min(n - 1);

                // Subdivision factor: 2^tess_level
                let subdiv = 1 << tess_level;

                // Generate triangles for this patch with subdivision
                for cell_z in base_z..end_z {
                    for cell_x in base_x..end_x {
                        // Subdivide this quad
                        let step = 1.0 / subdiv as f32;

                        for sub_z in 0..subdiv {
                            for sub_x in 0..subdiv {
                                let x0 = cell_x as f32 + sub_x as f32 * step;
                                let z0 = cell_z as f32 + sub_z as f32 * step;
                                let x1 = x0 + step;
                                let z1 = z0 + step;

                                let v00 = get_or_create_vertex(
                                    &mut positions,
                                    &mut normals,
                                    &mut uvs,
                                    &mut new_vertex_map,
                                    &vertex_map,
                                    x0,
                                    z0,
                                    n,
                                );
                                let v10 = get_or_create_vertex(
                                    &mut positions,
                                    &mut normals,
                                    &mut uvs,
                                    &mut new_vertex_map,
                                    &vertex_map,
                                    x1,
                                    z0,
                                    n,
                                );
                                let v01 = get_or_create_vertex(
                                    &mut positions,
                                    &mut normals,
                                    &mut uvs,
                                    &mut new_vertex_map,
                                    &vertex_map,
                                    x0,
                                    z1,
                                    n,
                                );
                                let v11 = get_or_create_vertex(
                                    &mut positions,
                                    &mut normals,
                                    &mut uvs,
                                    &mut new_vertex_map,
                                    &vertex_map,
                                    x1,
                                    z1,
                                    n,
                                );

                                // Two triangles per sub-quad
                                indices.push(v00 as u32);
                                indices.push(v01 as u32);
                                indices.push(v10 as u32);

                                indices.push(v10 as u32);
                                indices.push(v01 as u32);
                                indices.push(v11 as u32);
                            }
                        }
                    }
                }
            }
        }

        TessellatedMesh {
            positions,
            normals,
            uvs,
            indices,
        }
    }
}

/// Result of CPU tessellation.
#[derive(Debug, Clone)]
pub struct TessellatedMesh {
    /// Vertex positions (original + interpolated).
    pub positions: Vec<[f32; 3]>,
    /// Vertex normals (original + interpolated).
    pub normals: Vec<[f32; 3]>,
    /// Texture coordinates (original + interpolated).
    pub uvs: Vec<[f32; 2]>,
    /// Triangle indices.
    pub indices: Vec<u32>,
}

/// Decoded ambient occlusion data.
///
/// Based on IDA reverse engineering: AO is stored at half resolution
/// (512×1024 for a 1024×1024 terrain) and sampled with bilinear filtering
/// in the vertex shader via gVertSampler_ao_Texture.
#[derive(Debug, Clone)]
pub struct AmbientOcclusionData {
    /// AO values per texel (0-255, where 255 = fully lit, 0 = fully occluded).
    pub values: Vec<u8>,
    /// Texture width (half the terrain vertex count in X).
    pub width: usize,
    /// Texture height (same as terrain vertex count in Z).
    pub height: usize,
}

/// Decoded alpha (transparency) data.
///
/// Uses the same compression/format as AO data.
/// Sampled via gVertSampler_alpha_Texture in the vertex shader.
#[derive(Debug, Clone)]
pub struct AlphaData {
    /// Alpha values per texel (0-255, where 255 = fully opaque, 0 = fully transparent).
    pub values: Vec<u8>,
    /// Texture width (half the terrain vertex count in X).
    pub width: usize,
    /// Texture height (same as terrain vertex count in Z).
    pub height: usize,
}

impl XtdFile {
    /// Decode ambient occlusion data from the AO chunk.
    ///
    /// Based on IDA reverse engineering of the game's decompression (sub_1407E3440):
    /// - Input: 524,288 bytes (8 bytes per block × 65,536 blocks)
    /// - For each 8-byte input block:
    ///   - Read 4× 16-bit big-endian values
    ///   - Byte-swap each to little-endian
    ///   - Write 8 bytes of swapped data + 8 bytes of 0xFF padding (16 bytes total)
    /// - Game allocates (8 * chunk_size) >> 2 = 2× input size for output buffer
    ///
    /// The actual AO data is the first 8 bytes of each 16-byte decompressed block.
    /// Total actual data: 65,536 blocks × 8 bytes = 524,288 bytes = 512×1024 R8 texture
    ///
    /// The game samples this half-resolution texture with bilinear filtering and
    /// applies it in the vertex shader via gVertSampler_ao_Texture.
    ///
    /// Returns AO values at half resolution (512×1024 for a 1024×1024 terrain).
    pub fn decode_ao(&self) -> Result<AmbientOcclusionData> {
        if self.ao_data.is_empty() {
            return Err(Error::InvalidChunkData("AO chunk is empty".to_string()));
        }

        let num_verts_per_axis = self.header.num_x_verts as usize;

        // Decompress matching the game's algorithm exactly
        // Each 8-byte input block produces 8 bytes of actual AO data
        // (The game also writes 8 bytes of 0xFF padding which we skip)
        let num_blocks = self.ao_data.len() / 8;
        let mut decompressed = Vec::with_capacity(num_blocks * 8);

        for block_idx in 0..num_blocks {
            let in_offset = block_idx * 8;

            // Read 4× 16-bit values and byte-swap each (big-endian to little-endian)
            for word_idx in 0..4 {
                let offset = in_offset + word_idx * 2;
                if offset + 1 < self.ao_data.len() {
                    // Byte swap: read as [hi, lo], write as [lo, hi]
                    let hi = self.ao_data[offset];
                    let lo = self.ao_data[offset + 1];
                    decompressed.push(lo);
                    decompressed.push(hi);
                }
            }
            // Skip 0xFF padding - we don't write it
        }

        // The decompressed data is 8 bytes per block = 524,288 bytes total
        // This represents a half-resolution texture in R8 format:
        // - Full width (1024) × half height (512) = 524,288 texels
        //
        // The game uses bilinear sampling to interpolate this to full resolution
        // via gVertSampler_ao_Texture

        // Calculate dimensions: full width, half height
        let width = num_verts_per_axis;
        let height = num_verts_per_axis / 2;
        let expected_size = width * height;

        // Resize to expected size
        let mut tiled_data = decompressed;
        if tiled_data.len() < expected_size {
            tiled_data.resize(expected_size, 255);
        } else if tiled_data.len() > expected_size {
            tiled_data.truncate(expected_size);
        }

        // Xbox 360 R8 textures are stored in tiled format
        // For R8 format, tiles are typically 64 bytes arranged as 8x8 pixels
        // The data needs to be un-tiled to linear row-major order
        let values = untile_r8_texture(&tiled_data, width, height);

        Ok(AmbientOcclusionData {
            values,
            width,
            height,
        })
    }

    /// Decode alpha (transparency) data from the Alpha chunk.
    ///
    /// Uses the same decompression as AO data.
    /// Returns alpha values at half resolution.
    pub fn decode_alpha(&self) -> Result<AlphaData> {
        if self.alpha_data.is_empty() {
            return Err(Error::InvalidChunkData("Alpha chunk is empty".to_string()));
        }

        let num_verts_per_axis = self.header.num_x_verts as usize;

        // Full width, half height (same as AO)
        let width = num_verts_per_axis;
        let height = num_verts_per_axis / 2;
        let expected_size = width * height;

        // Check for "placeholder" alpha pattern: [255, 255, 0, 0, 0, 0, 0, 0] repeating
        // This indicates no terrain holes - return all-opaque texture
        // Blood Gulch and other maps without terrain holes use this pattern
        let is_placeholder = self.alpha_data.len() >= 8 && {
            let pattern = &[255u8, 255, 0, 0, 0, 0, 0, 0];
            self.alpha_data
                .chunks(8)
                .take(100)
                .all(|chunk| chunk == pattern)
        };

        if is_placeholder {
            return Ok(AlphaData {
                values: vec![255u8; expected_size],
                width,
                height,
            });
        }

        // Same decompression as AO
        let num_blocks = self.alpha_data.len() / 8;
        let mut decompressed = Vec::with_capacity(num_blocks * 8);

        for block_idx in 0..num_blocks {
            let in_offset = block_idx * 8;

            for word_idx in 0..4 {
                let offset = in_offset + word_idx * 2;
                if offset + 1 < self.alpha_data.len() {
                    let hi = self.alpha_data[offset];
                    let lo = self.alpha_data[offset + 1];
                    decompressed.push(lo);
                    decompressed.push(hi);
                }
            }
        }

        // Use decompressed bytes directly as R8 values
        let mut values = decompressed;
        if values.len() < expected_size {
            values.resize(expected_size, 255);
        } else if values.len() > expected_size {
            values.truncate(expected_size);
        }

        Ok(AlphaData {
            values,
            width,
            height,
        })
    }
}
