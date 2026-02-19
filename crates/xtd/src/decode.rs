//! XTD terrain vertex decoding utilities.
//!
//! The atlas chunk contains packed vertex positions and normals.
//! This module provides functions to decode them for rendering.
//!
//! The DE (PC) version stores data as LittleEndian with PC R10G10B10A2 bit layout.
//! The original Xbox 360 data was BigEndian with tiled (swizzled) textures.

use crate::{Error, Result, XtdFile};
use byteorder::{BigEndian, ByteOrder, LittleEndian};

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

        // Debug: print header values
        eprintln!("Atlas Header:");
        eprintln!("  mid:   [{:.2}, {:.2}, {:.2}]", header.mid[0], header.mid[1], header.mid[2]);
        eprintln!("  range: [{:.2}, {:.2}, {:.2}]", header.range[0], header.range[1], header.range[2]);

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
            packed_positions.push(LittleEndian::read_u32(&self.atlas_data[pos_offset..pos_offset + 4]));

            let norm_offset = norm_start + i * 4;
            packed_normals.push(LittleEndian::read_u32(&self.atlas_data[norm_offset..norm_offset + 4]));
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

            // Debug: print first few positions with raw packed values
            if i < 5 || i == num_verts / 2 {
                let packed = packed_positions[i];
                // PC layout
                let pc_x = packed & 0x3FF;
                let pc_y = (packed >> 10) & 0x3FF;
                let pc_z = (packed >> 20) & 0x3FF;
                // Xbox 360 layout
                let xbox_x = (packed >> 22) & 0x3FF;
                let xbox_y = (packed >> 11) & 0x3FF;
                let xbox_z = packed & 0x3FF;
                eprintln!("  [{}] packed=0x{:08X}", i, packed);
                eprintln!("       PC:   x={}, y={}, z={}", pc_x, pc_y, pc_z);
                eprintln!("       Xbox: x={}, y={}, z={}", xbox_x, xbox_y, xbox_z);
                eprintln!("       unpacked: [{:.2}, {:.2}, {:.2}]", unpacked[0], unpacked[1], unpacked[2]);
            }

            // Use grid position for X/Z base, unpacked Y for height
            // The unpacked X/Z may be small displacements (detail offsets)
            positions.push([
                grid_x * tile_scale + unpacked[0],
                unpacked[1],
                grid_z * tile_scale + unpacked[2],
            ]);

            normals.push(unpack_normal(packed_normals[i]));

            // UV coordinates: based on grid position (0-1 range over the terrain)
            // The terrain data uses X-major ordering (row = i / width = Z, col = i % width = X)
            // We need to flip to match texture orientation
            let u = grid_x / width_f;
            let v = grid_z / width_f;
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
}

