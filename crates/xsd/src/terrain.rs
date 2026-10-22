//! Decoding of the XSD simulation terrain chunks.

use alloc::vec::Vec;
use half::f16;
use zerocopy::byteorder::{self, BigEndian, F32, I32, LittleEndian, U16};
use zerocopy::{FromBytes, Immutable, KnownLayout, Unaligned};

use crate::{
    CHUNK_HEADER, CHUNK_HEIGHTS, CHUNK_TILE_TYPES, Error, HEADER_SIZE, HEIGHT_BLOCK_AXIS,
    HEIGHT_BLOCK_SIZE, Result, XSD_VERSION,
};

/// Byte order of the numeric values in an XSD file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ByteOrder {
    /// Big-endian values, as in the Xbox 360 retail files.
    Big,
    /// Little-endian values.
    Little,
}

/// Raw on-disk XSD header (32 bytes).
///
/// [`Terrain::from_chunks`] reads the version before it selects the byte order.
/// The reader does not use the values at offsets 12 and 28.
#[derive(FromBytes, KnownLayout, Immutable, Unaligned)]
#[repr(C)]
struct RawHeader<O: byteorder::ByteOrder> {
    _version: I32<O>,
    data_axis: I32<O>,
    data_tile_scale: F32<O>,
    _unknown_12: [u8; 4],
    height_axis: I32<O>,
    cache_axis: I32<O>,
    height_tile_scale: F32<O>,
    _unknown_28: [u8; 4],
}

/// Simulation terrain decoded from an XSD file.
///
/// [`Terrain::read`] and [`Terrain::from_chunks`] make sure that:
///
/// - all axes are positive,
/// - `cache_axis` is at least `height_axis` and divisible by [`HEIGHT_BLOCK_AXIS`],
/// - both tile scales are finite and positive,
/// - `heights` has `cache_axis * cache_axis` finite samples,
/// - `tile_types`, if present, has `data_axis * data_axis` values.
#[derive(Debug, Clone, PartialEq)]
pub struct Terrain {
    /// Byte order of the source file.
    pub byte_order: ByteOrder,
    /// Number of data tiles on each axis of the tile-type grid.
    pub data_axis: usize,
    /// World size of one data tile.
    pub data_tile_scale: f32,
    /// Number of height samples on each axis that the terrain uses.
    pub height_axis: usize,
    /// Number of height samples on each axis of the cache-aligned grid.
    pub cache_axis: usize,
    /// World distance between two adjacent height samples.
    pub height_tile_scale: f32,
    /// Height samples in file order.
    ///
    /// The file stores the grid in blocks of [`HEIGHT_BLOCK_AXIS`] by
    /// [`HEIGHT_BLOCK_AXIS`] samples. Use [`Terrain::height_index`] to find a
    /// sample.
    pub heights: Vec<f32>,
    /// Row-major tile types, if the file has the tile-type chunk.
    pub tile_types: Option<Vec<u8>>,
}

impl Terrain {
    /// Decode the terrain from a complete XSD file.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Container`] if the ECF container is not valid,
    /// [`Error::MissingChunk`] if the header or height chunk is absent, or an
    /// error from [`Terrain::from_chunks`].
    pub fn read(bytes: &[u8]) -> Result<Self> {
        let container = ecf::Reader::new(bytes)?;
        let header = container
            .chunk_data_by_id(CHUNK_HEADER)
            .map_err(|error| missing_chunk(error, CHUNK_HEADER))?;
        let heights = container
            .chunk_data_by_id(CHUNK_HEIGHTS)
            .map_err(|error| missing_chunk(error, CHUNK_HEIGHTS))?;
        let tile_types = match container.chunk_data_by_id(CHUNK_TILE_TYPES) {
            Ok(tile_types) => Some(tile_types),
            Err(ecf::Error::ChunkNotFound(_)) => None,
            Err(error) => return Err(Error::Container(error)),
        };
        Self::from_chunks(&header, &heights, tile_types.as_deref())
    }

    /// Decode the terrain from the data of the XSD chunks.
    ///
    /// The version field selects the byte order. The reader tries big-endian
    /// first.
    ///
    /// # Errors
    ///
    /// Returns an error if the header is truncated or has an unsupported
    /// version, if a dimension or scale is not valid, if a height sample is not
    /// finite, or if the height or tile-type data is truncated.
    pub fn from_chunks(header: &[u8], heights: &[u8], tile_types: Option<&[u8]>) -> Result<Self> {
        let truncated = Error::TruncatedHeader {
            expected: HEADER_SIZE,
            actual: header.len(),
        };
        if header.len() < HEADER_SIZE {
            return Err(truncated);
        }
        let version = *header.first_chunk::<4>().ok_or(truncated)?;
        if i32::from_be_bytes(version) == XSD_VERSION {
            decode::<BigEndian>(ByteOrder::Big, header, heights, tile_types)
        } else if i32::from_le_bytes(version) == XSD_VERSION {
            decode::<LittleEndian>(ByteOrder::Little, header, heights, tile_types)
        } else {
            Err(Error::UnsupportedVersion(version))
        }
    }

    /// Return the index in [`Terrain::heights`] of the sample at grid point
    /// (`x`, `z`).
    ///
    /// Returns `None` if a coordinate is outside the cache-aligned grid.
    #[must_use]
    pub fn height_index(&self, x: usize, z: usize) -> Option<usize> {
        if x >= self.cache_axis || z >= self.cache_axis {
            return None;
        }
        let blocks_per_axis = self.cache_axis / HEIGHT_BLOCK_AXIS;
        let block_x = (x / HEIGHT_BLOCK_AXIS).checked_mul(HEIGHT_BLOCK_SIZE)?;
        let block_z = (z / HEIGHT_BLOCK_AXIS)
            .checked_mul(blocks_per_axis)?
            .checked_mul(HEIGHT_BLOCK_SIZE)?;
        let local_x = x % HEIGHT_BLOCK_AXIS;
        let local_z = (z % HEIGHT_BLOCK_AXIS) * HEIGHT_BLOCK_AXIS;
        block_z
            .checked_add(block_x)?
            .checked_add(local_z)?
            .checked_add(local_x)
    }

    /// Return the height sample at grid point (`x`, `z`).
    ///
    /// Returns `None` if a coordinate is outside the cache-aligned grid.
    #[must_use]
    pub fn height_sample(&self, x: usize, z: usize) -> Option<f32> {
        self.heights.get(self.height_index(x, z)?).copied()
    }

    /// Return the tile type at data tile (`x`, `z`).
    ///
    /// Returns `None` if the file has no tile types or if a coordinate is
    /// outside the data grid.
    #[must_use]
    pub fn tile_type(&self, x: usize, z: usize) -> Option<u8> {
        if x >= self.data_axis || z >= self.data_axis {
            return None;
        }
        let index = z.checked_mul(self.data_axis)?.checked_add(x)?;
        self.tile_types.as_ref()?.get(index).copied()
    }
}

fn decode<O: byteorder::ByteOrder>(
    byte_order: ByteOrder,
    header: &[u8],
    height_bytes: &[u8],
    tile_type_bytes: Option<&[u8]>,
) -> Result<Terrain> {
    let (raw, _) = RawHeader::<O>::ref_from_prefix(header).map_err(|_| Error::TruncatedHeader {
        expected: HEADER_SIZE,
        actual: header.len(),
    })?;
    let data_axis = positive_usize(raw.data_axis.get(), "data tile axis")?;
    let data_tile_scale = raw.data_tile_scale.get();
    let height_axis = positive_usize(raw.height_axis.get(), "height axis")?;
    let cache_axis = positive_usize(raw.cache_axis.get(), "cache axis")?;
    let height_tile_scale = raw.height_tile_scale.get();
    if cache_axis < height_axis || !cache_axis.is_multiple_of(HEIGHT_BLOCK_AXIS) {
        return Err(Error::InvalidDimensions(
            "cache axis must contain the height grid and be divisible by eight",
        ));
    }
    if !height_tile_scale.is_finite() || height_tile_scale <= 0.0 {
        return Err(Error::InvalidDimensions(
            "height tile scale must be finite and positive",
        ));
    }
    if !data_tile_scale.is_finite() || data_tile_scale <= 0.0 {
        return Err(Error::InvalidDimensions(
            "data tile scale must be finite and positive",
        ));
    }
    let heights = decode_heights::<O>(cache_axis, height_bytes)?;
    let tile_types = tile_type_bytes
        .map(|bytes| decode_tile_types(data_axis, bytes))
        .transpose()?;
    Ok(Terrain {
        byte_order,
        data_axis,
        data_tile_scale,
        height_axis,
        cache_axis,
        height_tile_scale,
        heights,
        tile_types,
    })
}

fn decode_heights<O: byteorder::ByteOrder>(cache_axis: usize, bytes: &[u8]) -> Result<Vec<f32>> {
    let sample_count = cache_axis
        .checked_mul(cache_axis)
        .ok_or(Error::InvalidDimensions(
            "cache-aligned height grid overflows memory",
        ))?;
    let expected = sample_count
        .checked_mul(size_of::<u16>())
        .ok_or(Error::InvalidDimensions(
            "height byte count overflows memory",
        ))?;
    let truncated = Error::TruncatedHeights {
        expected,
        actual: bytes.len(),
    };
    if bytes.len() < expected {
        return Err(truncated);
    }
    let (samples, _) =
        <[U16<O>]>::ref_from_prefix_with_elems(bytes, sample_count).map_err(|_| truncated)?;
    let heights: Vec<f32> = samples
        .iter()
        .map(|sample| f16::from_bits(sample.get()).to_f32())
        .collect();
    if heights.iter().any(|height| !height.is_finite()) {
        return Err(Error::InvalidDimensions("height samples must be finite"));
    }
    Ok(heights)
}

fn decode_tile_types(data_axis: usize, bytes: &[u8]) -> Result<Vec<u8>> {
    let expected = data_axis
        .checked_mul(data_axis)
        .ok_or(Error::InvalidDimensions("tile-type grid overflows memory"))?;
    let data = bytes.get(..expected).ok_or(Error::TruncatedTileTypes {
        expected,
        actual: bytes.len(),
    })?;
    Ok(data.to_vec())
}

fn missing_chunk(error: ecf::Error, chunk_id: u64) -> Error {
    if matches!(error, ecf::Error::ChunkNotFound(_)) {
        Error::MissingChunk(chunk_id)
    } else {
        Error::Container(error)
    }
}

fn positive_usize(value: i32, field: &'static str) -> Result<usize> {
    usize::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or(Error::InvalidDimensions(field))
}

#[cfg(test)]
mod tests;
