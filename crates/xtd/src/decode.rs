//! XTD terrain vertex decoding utilities.
//!
//! The atlas chunk contains packed vertex positions and normals.
//! This module provides functions to decode them for rendering.
//!
//! Note: Xbox 360 data is tiled (swizzled) for GPU cache efficiency.
//! This module includes un-tiling support for the DE (PC) version.

use crate::{Error, Result, XtdFile};
use byteorder::{BigEndian, ByteOrder};

/// Xbox 360 texture tile size for 32-bit formats (R11G11B10, etc.)
const TILE_SIZE: usize = 32;

/// Un-tile Xbox 360 texture data.
///
/// Xbox 360 textures are stored in a tiled format for GPU cache efficiency.
/// This function converts tiled data back to linear row-major order.
///
/// For 32-bit formats, tiles are 32x32 pixels.
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
#[inline]
fn morton_index(x: usize, y: usize) -> usize {
    let mut result = 0;
    for i in 0..16 {
        result |= ((x >> i) & 1) << (2 * i);
        result |= ((y >> i) & 1) << (2 * i + 1);
    }
    result
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

        Ok(Self {
            mid: [
                BigEndian::read_f32(&data[0..4]),
                BigEndian::read_f32(&data[4..8]),
                BigEndian::read_f32(&data[8..12]),
            ],
            // Skip padding at bytes 12-15
            range: [
                BigEndian::read_f32(&data[16..20]),
                BigEndian::read_f32(&data[20..24]),
                BigEndian::read_f32(&data[24..28]),
            ],
            // Skip padding at bytes 28-31
        })
    }
}

/// Unpack a 32-bit packed position value.
///
/// Bit layout (from ExportXTD.cs):
/// - X: bits 22-31 (10 bits, max 1023)
/// - Y: bits 11-21 (10-bit value stored in 11-bit field)
/// - Z: bits 0-10  (10-bit value stored in 11-bit field)
///
/// All values are packed using bitMax10 (1023) as the scale.
#[inline]
pub fn unpack_position(packed: u32, mid: &[f32; 3], range: &[f32; 3]) -> [f32; 3] {
    const BIT_MAX_10: f32 = 1023.0;

    // All three use 10-bit values (0-1023), even though Y and Z fields are 11 bits wide
    let x_bits = ((packed >> 22) & 0x3FF) as f32;
    let y_bits = ((packed >> 11) & 0x3FF) as f32;
    let z_bits = (packed & 0x3FF) as f32;

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
    /// Number of vertices per axis (square terrain).
    pub num_verts_per_axis: usize,
}

impl XtdFile {
    /// Decode terrain vertices from the atlas chunk.
    ///
    /// Returns positions and normals as Vec<[f32; 3]>.
    pub fn decode_vertices(&self) -> Result<TerrainVertices> {
        if self.atlas_data.is_empty() {
            return Err(Error::InvalidChunkData(
                "Atlas chunk is empty".to_string(),
            ));
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

        // Read packed data as u32 values (BigEndian for Xbox 360 format)
        let mut packed_positions = Vec::with_capacity(num_verts);
        let mut packed_normals = Vec::with_capacity(num_verts);

        for i in 0..num_verts {
            let pos_offset = pos_start + i * 4;
            packed_positions.push(BigEndian::read_u32(&self.atlas_data[pos_offset..pos_offset + 4]));

            let norm_offset = norm_start + i * 4;
            packed_normals.push(BigEndian::read_u32(&self.atlas_data[norm_offset..norm_offset + 4]));
        }

        // Note: The DE (PC) version uses linear layout, not Xbox 360 tiled format.
        // If you need to support the original Xbox 360 format, use untile_texture().
        let linear_positions = packed_positions;
        let linear_normals = packed_normals;

        // Unpack positions and normals from the un-tiled linear data
        // The packed data stores DISPLACEMENTS from the base grid position.
        // World position = base_grid_position + displacement
        let tile_scale = self.header.tile_scale;
        let mut positions = Vec::with_capacity(num_verts);
        let mut normals = Vec::with_capacity(num_verts);

        for i in 0..num_verts {
            // Note: The texture data is stored transposed - the shader accesses with
            // tex2Dlod(..., float4(uv.y, uv.x, 0, 0)) which swaps the coordinates.
            // This means z varies fast (columns) and x varies slow (rows).
            let grid_z = (i % width) as f32;
            let grid_x = (i / width) as f32;

            let displacement = unpack_position(linear_positions[i], &header.mid, &header.range);

            // Add base grid position to displacement to get world position
            positions.push([
                grid_x * tile_scale + displacement[0],
                displacement[1], // Y is just the height (displacement)
                grid_z * tile_scale + displacement[2],
            ]);

            normals.push(unpack_normal(linear_normals[i]));
        }

        Ok(TerrainVertices {
            header,
            positions,
            normals,
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
}

