//! XTD terrain vertex decoding utilities.
//!
//! The atlas chunk contains packed vertex positions and normals.
//! This module provides functions to decode them for rendering.
//!
//! The DE (PC) version stores data as `LittleEndian` with PC R10G10B10A2 bit layout.
//! The original Xbox 360 data was `BigEndian` with tiled (swizzled) textures.

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;

use nostdio::{Cursor, ReadBe, ReadLe};
use num_traits::ToPrimitive;

use crate::{Error, Result, XtdFile};

mod auxiliary_textures;
pub use auxiliary_textures::LightingData;

/// Calculate Morton code (Z-order curve) for 2D coordinates.
///
/// This interleaves the bits of x and y to create the swizzled index.
/// Used by the Xbox 360 R8 texture decoder.
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
    ///
    /// # Errors
    ///
    /// Returns an error if `data` is shorter than the 32-byte atlas header.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        if data.len() < Self::SIZE {
            return Err(Error::InvalidChunkData(format!(
                "Atlas header too small: {} < {}",
                data.len(),
                Self::SIZE
            )));
        }

        let mut cursor = Cursor::new(data);
        let mut values = [0.0; 8];
        for value in &mut values {
            *value = cursor.read_f32_be()?;
        }
        Ok(Self {
            mid: [values[0], values[1], values[2]],
            range: [values[4], values[5], values[6]],
        })
    }
}

fn ten_bit_component(value: u32) -> f32 {
    f32::from(u16::try_from(value & 0x3FF).unwrap_or_default())
}

/// Unpack a 32-bit R10G10B10A2 packed position into a displacement vector.
///
/// DE/PC data format (`DXGI_FORMAT_R10G10B10A2_UNORM`), consumed as `.zyx` by
/// the PC terrain shaders:
/// - R: bits 0-9   (10 bits) → Z displacement
/// - G: bits 10-19 (10 bits) → Y displacement (height)
/// - B: bits 20-29 (10 bits) → X displacement
/// - A: bits 30-31 (2 bits)  → unused
///
/// Data is stored as `LittleEndian` in the DE PC files.
///
/// The unpacked value represents a displacement from the base grid position.
/// Formula: `displacement = (sample.zyx - [0, 1/2048, 0]) * range - mid`.
/// The normalized Y bias is the exact `g_yOffset` default in the PC terrain
/// vertex and domain shaders.
#[inline]
#[must_use]
pub fn unpack_position(packed: u32, mid: &[f32; 3], range: &[f32; 3]) -> [f32; 3] {
    const BIT_MAX_10: f32 = 1023.0;
    const NORMALIZED_Y_OFFSET: f32 = 1.0 / 2048.0;

    // The texture's R/G/B components occupy low/middle/high bits. The shader's
    // `.zyx` swizzle makes the high component world X and the low component Z.
    let z_bits = ten_bit_component(packed);
    let y_bits = ten_bit_component(packed >> 10);
    let x_bits = ten_bit_component(packed >> 20);

    // Convert to normalized [0, 1] range, then apply range and offset
    [
        (x_bits / BIT_MAX_10) * range[0] - mid[0],
        (y_bits / BIT_MAX_10 - NORMALIZED_Y_OFFSET) * range[1] - mid[1],
        (z_bits / BIT_MAX_10) * range[2] - mid[2],
    ]
}

#[cfg(test)]
mod position_tests {
    use super::unpack_position;

    #[test]
    fn packed_position_matches_pc_shader_swizzle_and_y_bias() {
        let packed = (1023_u32 << 20) | (512_u32 << 10) | 1_u32;
        let decoded = unpack_position(packed, &[0.0; 3], &[10.0, 20.0, 30.0]);
        let expected_y = (512.0 / 1023.0 - 1.0 / 2048.0) * 20.0;

        assert_eq!(decoded[0].to_bits(), 10.0f32.to_bits());
        assert!((decoded[1] - expected_y).abs() < f32::EPSILON * 16.0);
        assert!((decoded[2] - 30.0 / 1023.0).abs() < f32::EPSILON * 16.0);
    }
}

/// Unpack a 32-bit packed normal value.
///
/// The PC basis texture uses `DXGI_FORMAT_R10G10B10A2_UNORM`, and the terrain
/// shaders consume it as `.zyx * 2 - 1`. Consequently the high ten bits are
/// world X, the middle ten bits are world Y, and the low ten bits are world Z.
#[inline]
#[must_use]
pub fn unpack_normal(packed: u32) -> [f32; 3] {
    const BIT_MAX_10: f32 = 1023.0;

    let x_bits = ten_bit_component(packed >> 20);
    let y_bits = ten_bit_component(packed >> 10);
    let z_bits = ten_bit_component(packed);

    [
        (x_bits / BIT_MAX_10) * 2.0 - 1.0,
        (y_bits / BIT_MAX_10) * 2.0 - 1.0,
        (z_bits / BIT_MAX_10) * 2.0 - 1.0,
    ]
}

#[cfg(test)]
mod normal_tests {
    use super::unpack_normal;

    #[test]
    fn packed_normal_matches_pc_shader_swizzle() {
        let packed = (1023_u32 << 20) | (512_u32 << 10);
        let decoded = unpack_normal(packed);

        assert_eq!(decoded[0].to_bits(), 1.0f32.to_bits());
        assert!((decoded[1] - (512.0 / 1023.0 * 2.0 - 1.0)).abs() < f32::EPSILON * 4.0);
        assert_eq!(decoded[2].to_bits(), (-1.0f32).to_bits());
    }
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

struct PackedAtlas {
    header: AtlasHeader,
    width: usize,
    positions: Vec<u32>,
    normals: Vec<u32>,
}

fn nonnegative_usize(value: i32, field: &'static str) -> Result<usize> {
    usize::try_from(value).map_err(|_| Error::InvalidChunkData(format!("Invalid {field}: {value}")))
}

fn parse_packed_atlas(file: &XtdFile) -> Result<PackedAtlas> {
    if file.atlas_data.is_empty() {
        return Err(Error::InvalidChunkData("Atlas chunk is empty".to_string()));
    }

    let header = AtlasHeader::from_bytes(&file.atlas_data)?;
    let width = nonnegative_usize(file.header.num_x_verts, "terrain vertex count")?;
    if width == 0 {
        return Err(Error::InvalidChunkData(
            "Terrain vertex count must be positive".to_string(),
        ));
    }
    let vertex_count = width
        .checked_mul(width)
        .ok_or(Error::SizeOverflow("terrain vertex count"))?;
    let payload_size = vertex_count
        .checked_mul(8)
        .ok_or(Error::SizeOverflow("terrain atlas payload"))?;
    let expected_size = AtlasHeader::SIZE
        .checked_add(payload_size)
        .ok_or(Error::SizeOverflow("terrain atlas"))?;
    if file.atlas_data.len() < expected_size {
        return Err(Error::InvalidChunkData(format!(
            "Atlas data too small: {} < {} (expected {} verts)",
            file.atlas_data.len(),
            expected_size,
            vertex_count
        )));
    }

    let payload = file
        .atlas_data
        .get(AtlasHeader::SIZE..expected_size)
        .ok_or(Error::UnexpectedEof)?;
    let mut cursor = Cursor::new(payload);
    let mut positions = Vec::with_capacity(vertex_count);
    for _ in 0..vertex_count {
        positions.push(cursor.read_u32_le()?);
    }
    let mut normals = Vec::with_capacity(vertex_count);
    for _ in 0..vertex_count {
        normals.push(cursor.read_u32_le()?);
    }

    Ok(PackedAtlas {
        header,
        width,
        positions,
        normals,
    })
}

impl XtdFile {
    /// Extract raw packed terrain data for GPU tessellation.
    ///
    /// This returns the packed position/normal data that can be uploaded
    /// as GPU textures and decoded in the vertex shader.
    ///
    /// # Errors
    ///
    /// Returns an error if the atlas is missing, truncated, has invalid
    /// dimensions, or describes an allocation that overflows.
    pub fn extract_raw_data(&self) -> Result<RawTerrainData> {
        let packed = parse_packed_atlas(self)?;

        Ok(RawTerrainData {
            packed_positions: packed.positions,
            packed_normals: packed.normals,
            num_verts_per_axis: u32::try_from(packed.width)
                .map_err(|_| Error::SizeOverflow("terrain width"))?,
            mid: packed.header.mid,
            range: packed.header.range,
            tile_scale: self.header.tile_scale,
            world_min: self.header.world_min,
            world_max: self.header.world_max,
        })
    }

    /// Decode terrain vertices from the atlas chunk.
    ///
    /// Returns positions and normals as Vec<[f32; 3]>.
    ///
    /// # Errors
    ///
    /// Returns an error if the atlas is missing, truncated, has invalid
    /// dimensions, or describes coordinates that cannot be represented.
    pub fn decode_vertices(&self) -> Result<TerrainVertices> {
        let packed = parse_packed_atlas(self)?;
        let width = packed.width;
        if width < 2 {
            return Err(Error::InvalidChunkData(
                "Terrain atlas must be at least 2x2".to_string(),
            ));
        }
        let num_verts = packed.positions.len();

        // Unpack positions, normals, and compute UVs
        // The packed data stores DISPLACEMENTS from the base grid position.
        // World position = base_grid_position + displacement
        let tile_scale = self.header.tile_scale;
        let mut positions = Vec::with_capacity(num_verts);
        let mut normals = Vec::with_capacity(num_verts);
        let mut uvs = Vec::with_capacity(num_verts);
        let width_f = (width - 1)
            .to_f32()
            .ok_or(Error::SizeOverflow("terrain width"))?;

        for world_z_index in 0..width {
            for world_x_index in 0..width {
                // The XTD source axes are diagonally mirrored relative to the
                // XTT material world: viewer (x, z) is source (z, x). The PC
                // byte stream stores source (x, z) at x * width + z, so this
                // conversion reads source (world_z, world_x).
                let source_index = world_z_index * width + world_x_index;
                let world_x = world_x_index
                    .to_f32()
                    .ok_or(Error::SizeOverflow("terrain X coordinate"))?;
                let world_z = world_z_index
                    .to_f32()
                    .ok_or(Error::SizeOverflow("terrain Z coordinate"))?;

                // The packed data contains position data that needs to be combined with grid position.
                // X/Z: grid position provides the base, packed data adds displacement
                // Y: comes entirely from the packed data (height)
                let unpacked = unpack_position(
                    packed.positions[source_index],
                    &packed.header.mid,
                    &packed.header.range,
                );

                // Mirror the complete position, including the packed X/Z
                // displacement. Moving only the height texel applies lateral
                // displacement in the wrong orientation and can fold terrain.
                positions.push([
                    world_x * tile_scale + unpacked[2],
                    unpacked[1],
                    world_z * tile_scale + unpacked[0],
                ]);

                let source_normal = unpack_normal(packed.normals[source_index]);
                normals.push([source_normal[2], source_normal[1], source_normal[0]]);

                // UV coordinates: Z→U, X→V (matching the game's convention).
                //
                // The original Halo Wars shaders consistently use world Z for the U axis
                // and world X for the V axis (e.g. the roads shader samples `gPos.zx`).
                // All terrain textures — splat alpha, albedo, AO — are authored for this
                // convention, so we adopt it here at the source rather than compensating
                // with rotation/transpose hacks downstream.
                let u = world_z / width_f;
                let v = world_x / width_f;
                uvs.push([u, v]);
            }
        }

        Ok(TerrainVertices {
            header: packed.header,
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
    ///
    /// # Errors
    ///
    /// Returns an error if the grid or one of its indices exceeds the supported
    /// in-memory or `u32` index range.
    pub fn generate_indices(&self) -> Result<Vec<u32>> {
        let n = self.num_verts_per_axis;
        if n < 2 {
            return Ok(Vec::new());
        }

        let num_quads = (n - 1)
            .checked_mul(n - 1)
            .ok_or(Error::SizeOverflow("terrain quad count"))?;
        let index_count = num_quads
            .checked_mul(6)
            .ok_or(Error::SizeOverflow("terrain index count"))?;
        let mut indices = Vec::with_capacity(index_count);

        for z in 0..(n - 1) {
            for x in 0..(n - 1) {
                let top_left_index = z
                    .checked_mul(n)
                    .and_then(|row| row.checked_add(x))
                    .ok_or(Error::SizeOverflow("terrain vertex index"))?;
                let bottom_left_index = (z + 1)
                    .checked_mul(n)
                    .and_then(|row| row.checked_add(x))
                    .ok_or(Error::SizeOverflow("terrain vertex index"))?;
                let top_left = u32::try_from(top_left_index)
                    .map_err(|_| Error::SizeOverflow("terrain vertex index"))?;
                let bottom_left = u32::try_from(bottom_left_index)
                    .map_err(|_| Error::SizeOverflow("terrain vertex index"))?;
                let top_right = top_left
                    .checked_add(1)
                    .ok_or(Error::SizeOverflow("terrain vertex index"))?;
                let bottom_right = bottom_left
                    .checked_add(1)
                    .ok_or(Error::SizeOverflow("terrain vertex index"))?;

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

        Ok(indices)
    }

    /// Get terrain dimensions in world units.
    #[must_use]
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
    ///
    /// # Errors
    ///
    /// Returns an error if dimensions or tessellation levels are invalid, a
    /// coordinate cannot be represented, or the generated mesh exceeds `u32`
    /// index limits.
    pub fn tessellate(&self, tess_data: &crate::TessellationData) -> Result<TessellatedMesh> {
        let n = self.num_verts_per_axis;
        if n < 2 {
            return Err(Error::InvalidChunkData(
                "Terrain grid must be at least 2x2".to_string(),
            ));
        }
        let x_patch_count = nonnegative_usize(tess_data.num_x_patches, "X patch count")?;
        let z_patch_count = nonnegative_usize(tess_data.num_z_patches, "Z patch count")?;
        if x_patch_count == 0 || z_patch_count == 0 {
            return Err(Error::InvalidChunkData(
                "Tessellation patch counts must be positive".to_string(),
            ));
        }

        let verts_per_patch_x = (n - 1) / x_patch_count + 1;
        let verts_per_patch_z = (n - 1) / z_patch_count + 1;
        let mut buffers = TessellationBuffers::new(self)?;
        let mut indices = Vec::new();

        for patch_z in 0..z_patch_count {
            for patch_x in 0..x_patch_count {
                let patch_index = patch_z
                    .checked_mul(x_patch_count)
                    .and_then(|row| row.checked_add(patch_x))
                    .ok_or(Error::SizeOverflow("tessellation patch index"))?;
                let tess_level =
                    *tess_data
                        .patch_tess_levels
                        .get(patch_index)
                        .ok_or_else(|| {
                            Error::InvalidChunkData("Missing patch tessellation level".to_string())
                        })?;
                let base_x = patch_x
                    .checked_mul(verts_per_patch_x - 1)
                    .ok_or(Error::SizeOverflow("patch X offset"))?;
                let base_z = patch_z
                    .checked_mul(verts_per_patch_z - 1)
                    .ok_or(Error::SizeOverflow("patch Z offset"))?;
                let end_x = base_x
                    .checked_add(verts_per_patch_x - 1)
                    .ok_or(Error::SizeOverflow("patch X extent"))?
                    .min(n - 1);
                let end_z = base_z
                    .checked_add(verts_per_patch_z - 1)
                    .ok_or(Error::SizeOverflow("patch Z extent"))?
                    .min(n - 1);
                let subdivisions = 1usize
                    .checked_shl(u32::from(tess_level))
                    .ok_or(Error::SizeOverflow("patch subdivision count"))?;
                let step = 1.0
                    / subdivisions
                        .to_f32()
                        .ok_or(Error::SizeOverflow("patch subdivision count"))?;

                for cell_z in base_z..end_z {
                    for cell_x in base_x..end_x {
                        let horizontal_cell = cell_x
                            .to_f32()
                            .ok_or(Error::SizeOverflow("terrain cell X"))?;
                        let vertical_cell = cell_z
                            .to_f32()
                            .ok_or(Error::SizeOverflow("terrain cell Z"))?;
                        for sub_z in 0..subdivisions {
                            for sub_x in 0..subdivisions {
                                let x0 = horizontal_cell
                                    + sub_x
                                        .to_f32()
                                        .ok_or(Error::SizeOverflow("terrain subdivision X"))?
                                        * step;
                                let z0 = vertical_cell
                                    + sub_z
                                        .to_f32()
                                        .ok_or(Error::SizeOverflow("terrain subdivision Z"))?
                                        * step;
                                let x1 = x0 + step;
                                let z1 = z0 + step;
                                let top_left = buffers.get_or_create(x0, z0, n)?;
                                let top_right = buffers.get_or_create(x1, z0, n)?;
                                let bottom_left = buffers.get_or_create(x0, z1, n)?;
                                let bottom_right = buffers.get_or_create(x1, z1, n)?;
                                push_quad_indices(
                                    &mut indices,
                                    [top_left, bottom_left, top_right, bottom_right],
                                )?;
                            }
                        }
                    }
                }
            }
        }

        Ok(TessellatedMesh {
            positions: buffers.positions,
            normals: buffers.normals,
            uvs: buffers.uvs,
            indices,
        })
    }
}

struct TessellationBuffers {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    grid_vertices: BTreeMap<(usize, usize), usize>,
    interpolated_vertices: BTreeMap<(u32, u32), usize>,
}

impl TessellationBuffers {
    fn new(vertices: &TerrainVertices) -> Result<Self> {
        let n = vertices.num_verts_per_axis;
        let expected_count = n
            .checked_mul(n)
            .ok_or(Error::SizeOverflow("terrain vertex count"))?;
        if vertices.positions.len() < expected_count
            || vertices.normals.len() < expected_count
            || vertices.uvs.len() < expected_count
        {
            return Err(Error::InvalidChunkData(
                "Terrain vertex arrays are incomplete".to_string(),
            ));
        }

        let mut grid_vertices = BTreeMap::new();
        for z in 0..n {
            for x in 0..n {
                let index = z
                    .checked_mul(n)
                    .and_then(|row| row.checked_add(x))
                    .ok_or(Error::SizeOverflow("terrain vertex index"))?;
                grid_vertices.insert((x, z), index);
            }
        }

        Ok(Self {
            positions: vertices.positions.clone(),
            normals: vertices.normals.clone(),
            uvs: vertices.uvs.clone(),
            grid_vertices,
            interpolated_vertices: BTreeMap::new(),
        })
    }

    fn get_or_create(&mut self, x: f32, z: f32, n: usize) -> Result<usize> {
        let grid_x = x
            .floor()
            .to_usize()
            .ok_or(Error::SizeOverflow("interpolated vertex X"))?;
        let grid_z = z
            .floor()
            .to_usize()
            .ok_or(Error::SizeOverflow("interpolated vertex Z"))?;
        let fraction_x = x - grid_x
            .to_f32()
            .ok_or(Error::SizeOverflow("interpolated vertex X"))?;
        let fraction_z = z - grid_z
            .to_f32()
            .ok_or(Error::SizeOverflow("interpolated vertex Z"))?;

        if fraction_x.abs() < 0.001 && fraction_z.abs() < 0.001 {
            return self.grid_index(grid_x, grid_z);
        }
        let key = (x.to_bits(), z.to_bits());
        if let Some(index) = self.interpolated_vertices.get(&key) {
            return Ok(*index);
        }

        let x0 = grid_x.min(n - 2);
        let z0 = grid_z.min(n - 2);
        let x1 = (x0 + 1).min(n - 1);
        let z1 = (z0 + 1).min(n - 1);
        let local_x = x - x0
            .to_f32()
            .ok_or(Error::SizeOverflow("interpolated vertex X"))?;
        let local_z = z - z0
            .to_f32()
            .ok_or(Error::SizeOverflow("interpolated vertex Z"))?;
        let corners = [
            self.grid_index(x0, z0)?,
            self.grid_index(x1, z0)?,
            self.grid_index(x0, z1)?,
            self.grid_index(x1, z1)?,
        ];
        let position = bilinear_vec3(corners.map(|index| self.positions[index]), local_x, local_z);
        let normal = normalize(bilinear_vec3(
            corners.map(|index| self.normals[index]),
            local_x,
            local_z,
        ));
        let uv = bilinear_vec2(corners.map(|index| self.uvs[index]), local_x, local_z);
        let index = self.positions.len();
        self.positions.push(position);
        self.normals.push(normal);
        self.uvs.push(uv);
        self.interpolated_vertices.insert(key, index);
        Ok(index)
    }

    fn grid_index(&self, x: usize, z: usize) -> Result<usize> {
        self.grid_vertices.get(&(x, z)).copied().ok_or_else(|| {
            Error::InvalidChunkData("Interpolated vertex lies outside the terrain grid".to_string())
        })
    }
}

fn bilinear_weights(x: f32, z: f32) -> [f32; 4] {
    [(1.0 - x) * (1.0 - z), x * (1.0 - z), (1.0 - x) * z, x * z]
}

fn bilinear_vec3(corners: [[f32; 3]; 4], x: f32, z: f32) -> [f32; 3] {
    let weights = bilinear_weights(x, z);
    core::array::from_fn(|axis| {
        corners
            .iter()
            .zip(weights)
            .map(|(corner, weight)| corner[axis] * weight)
            .sum()
    })
}

fn bilinear_vec2(corners: [[f32; 2]; 4], x: f32, z: f32) -> [f32; 2] {
    let weights = bilinear_weights(x, z);
    core::array::from_fn(|axis| {
        corners
            .iter()
            .zip(weights)
            .map(|(corner, weight)| corner[axis] * weight)
            .sum()
    })
}

fn normalize(mut vector: [f32; 3]) -> [f32; 3] {
    let length = (vector[0] * vector[0] + vector[1] * vector[1] + vector[2] * vector[2]).sqrt();
    if length > 0.001 {
        for component in &mut vector {
            *component /= length;
        }
    }
    vector
}

fn push_quad_indices(indices: &mut Vec<u32>, vertices: [usize; 4]) -> Result<()> {
    let [top_left, bottom_left, top_right, bottom_right] = vertices;
    let top_left =
        u32::try_from(top_left).map_err(|_| Error::SizeOverflow("tessellated vertex index"))?;
    let bottom_left =
        u32::try_from(bottom_left).map_err(|_| Error::SizeOverflow("tessellated vertex index"))?;
    let top_right =
        u32::try_from(top_right).map_err(|_| Error::SizeOverflow("tessellated vertex index"))?;
    let bottom_right =
        u32::try_from(bottom_right).map_err(|_| Error::SizeOverflow("tessellated vertex index"))?;
    indices.extend_from_slice(&[
        top_left,
        bottom_left,
        top_right,
        top_right,
        bottom_left,
        bottom_right,
    ]);
    Ok(())
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
/// in the vertex shader via `gVertSampler_ao_Texture`.
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
/// Sampled via `gVertSampler_alpha_Texture` in the vertex shader.
#[derive(Debug, Clone)]
pub struct AlphaData {
    /// Alpha values per texel (0-255, where 255 = fully opaque, 0 = fully transparent).
    pub values: Vec<u8>,
    /// Texture width (half the terrain vertex count in X).
    pub width: usize,
    /// Texture height (same as terrain vertex count in Z).
    pub height: usize,
}
