//! Face and mip subresource access for texture payloads.

use alloc::vec;
use alloc::vec::Vec;
use core::ops::Range;

use crate::format::DataFormat;
use crate::header::ResourceType;
use crate::reader::{DdxTexture, TextureSource};
use crate::{Error, Result};

/// Byte order of the faces and mip levels in a texture payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubresourceOrder {
    /// All mips of face 0, then all mips of face 1, and so on.
    ///
    /// DDS files use this order.
    FaceMajor,
    /// Mip 0 of each face, then mips 1 and up of face 0, then mips 1 and up
    /// of face 1, and so on.
    ///
    /// ECF-wrapped DDX files use this order.
    MipZeroFirst,
}

/// Shape of a texture payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubresourceLayout {
    /// Data format of each subresource.
    pub data_format: DataFormat,
    /// Width of mip 0 in pixels.
    pub width: u32,
    /// Height of mip 0 in pixels.
    pub height: u32,
    /// Number of faces: 6 for a cubemap, 1 for other textures.
    pub faces: u32,
    /// Number of mip levels of each face.
    pub mip_levels: u32,
    /// Byte order of the subresources in the payload.
    pub order: SubresourceOrder,
}

impl SubresourceLayout {
    /// Splits a payload into one byte slice for each face and mip level.
    ///
    /// The result uses face-major indexes: `face * mip_levels + mip`. The
    /// payload must contain all subresources and no other bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if the format has no fixed size, if a size overflows,
    /// if the payload is truncated, or if bytes follow the last subresource.
    pub fn split<'data>(&self, data: &'data [u8]) -> Result<Vec<&'data [u8]>> {
        Ok(self
            .ranges(data.len())?
            .into_iter()
            .map(|range| &data[range])
            .collect())
    }

    /// Returns the byte range of each face and mip level in a payload of
    /// `payload_len` bytes.
    ///
    /// The result uses face-major indexes: `face * mip_levels + mip`. The
    /// payload must contain all subresources and no other bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if the format has no fixed size, if a size overflows,
    /// if the payload is truncated, or if bytes follow the last subresource.
    pub fn ranges(&self, payload_len: usize) -> Result<Vec<Range<usize>>> {
        let faces =
            usize::try_from(self.faces).map_err(|_| Error::SizeOverflow("cube face count"))?;
        let mips =
            usize::try_from(self.mip_levels).map_err(|_| Error::SizeOverflow("mip level count"))?;
        let count = faces
            .checked_mul(mips)
            .ok_or(Error::SizeOverflow("subresource count"))?;
        let mut result = vec![0..0; count];
        let mut cursor = 0;
        // `chunks_mut` gives one non-empty slot group for each face.
        let face_size = mips.max(1);
        match self.order {
            SubresourceOrder::FaceMajor => {
                for face_slots in result.chunks_mut(face_size) {
                    for (slot, mip) in face_slots.iter_mut().zip(0..self.mip_levels) {
                        *slot = self.take(payload_len, &mut cursor, mip)?;
                    }
                }
            }
            SubresourceOrder::MipZeroFirst => {
                for face_slots in result.chunks_mut(face_size) {
                    if let Some(slot) = face_slots.first_mut() {
                        *slot = self.take(payload_len, &mut cursor, 0)?;
                    }
                }
                for face_slots in result.chunks_mut(face_size) {
                    let chain = face_slots.get_mut(1..).unwrap_or_default();
                    for (slot, mip) in chain.iter_mut().zip(1..self.mip_levels) {
                        *slot = self.take(payload_len, &mut cursor, mip)?;
                    }
                }
            }
        }
        if cursor != payload_len {
            return Err(Error::TrailingData(payload_len.saturating_sub(cursor)));
        }
        Ok(result)
    }

    /// Returns the byte size of one face at mip level `mip`.
    ///
    /// # Errors
    ///
    /// Returns an error if the format has no fixed size or the size overflows.
    pub fn subresource_size(&self, mip: u32) -> Result<usize> {
        let width = usize::try_from(mip_extent(self.width, mip))
            .map_err(|_| Error::SizeOverflow("mip width"))?;
        let height = usize::try_from(mip_extent(self.height, mip))
            .map_err(|_| Error::SizeOverflow("mip height"))?;
        let format = self.data_format;
        if format.is_dxtq() {
            return Err(Error::UnsupportedLayout(format));
        }
        let size = if format.is_dxt() {
            let block_size = usize::try_from(format.dxt_block_size())
                .map_err(|_| Error::SizeOverflow("block size"))?;
            width
                .div_ceil(4)
                .checked_mul(height.div_ceil(4))
                .and_then(|blocks| blocks.checked_mul(block_size))
        } else {
            let bits = usize::try_from(format.bits_per_pixel())
                .map_err(|_| Error::SizeOverflow("bits per pixel"))?;
            if bits == 0 || bits % 8 != 0 {
                return Err(Error::UnsupportedLayout(format));
            }
            width
                .checked_mul(height)
                .and_then(|pixels| pixels.checked_mul(bits / 8))
        };
        size.ok_or(Error::SizeOverflow("subresource size"))
    }

    fn take(&self, payload_len: usize, cursor: &mut usize, mip: u32) -> Result<Range<usize>> {
        let needed = self.subresource_size(mip)?;
        let start = *cursor;
        let end = start
            .checked_add(needed)
            .ok_or(Error::SizeOverflow("subresource offset"))?;
        if end > payload_len {
            return Err(Error::SubresourceTruncated {
                offset: start,
                needed,
                available: payload_len.saturating_sub(start),
            });
        }
        *cursor = end;
        Ok(start..end)
    }
}

/// Returns the width or height of mip level `mip`. The value is at least 1.
#[must_use]
pub fn mip_extent(size: u32, mip: u32) -> u32 {
    size.checked_shr(mip).unwrap_or(0).max(1)
}

impl DdxTexture {
    /// Returns the payload shape of this texture.
    ///
    /// A [`ResourceType::CubeMap`] texture has six faces. The order follows
    /// the source container: DDS files are face-major, ECF-wrapped DDX files
    /// store mip 0 of each face first.
    #[must_use]
    pub fn subresource_layout(&self) -> SubresourceLayout {
        SubresourceLayout {
            data_format: self.info.data_format,
            width: self.info.width,
            height: self.info.height,
            faces: if self.info.resource_type == ResourceType::CubeMap {
                6
            } else {
                1
            },
            mip_levels: self.info.num_mip_levels,
            order: match self.source {
                TextureSource::Dds(_) => SubresourceOrder::FaceMajor,
                TextureSource::Ddx(_) => SubresourceOrder::MipZeroFirst,
            },
        }
    }

    /// Splits the payload into one byte slice for each face and mip level.
    ///
    /// The result uses face-major indexes: `face * mip_levels + mip`.
    ///
    /// # Errors
    ///
    /// Returns an error if the format has no fixed size, if the payload is
    /// truncated, or if bytes follow the last subresource.
    pub fn subresources(&self) -> Result<Vec<&[u8]>> {
        self.subresource_layout().split(&self.data)
    }
}

#[cfg(test)]
mod tests {
    use std::vec;
    use std::vec::Vec;

    use super::{SubresourceLayout, SubresourceOrder};
    use crate::{
        D3DFMT_A16B16G16R16F, DDPF_FOURCC, DDS_HEADER_SIZE, DDS_PIXEL_FORMAT_SIZE,
        DDSCAPS2_CUBEMAP_ALL_FACES, DataFormat, DdxTexture, Error, ResourceType,
    };

    const RGBA16F_BYTES: usize = 8;

    fn push_u32(bytes: &mut Vec<u8>, value: u32) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }

    /// Builds a 2x2 `A16B16G16R16F` DDS cubemap with two mips.
    ///
    /// Each byte of face `n` mip 0 is `n`. Each byte of face `n` mip 1 is `n + 16`.
    fn two_by_two_cube() -> Vec<u8> {
        let mut bytes = Vec::with_capacity(128 + 240);
        bytes.extend_from_slice(b"DDS ");
        push_u32(&mut bytes, DDS_HEADER_SIZE);
        push_u32(&mut bytes, 0x0002_100F);
        push_u32(&mut bytes, 2);
        push_u32(&mut bytes, 2);
        push_u32(&mut bytes, 16);
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 2);
        for _ in 0..11 {
            push_u32(&mut bytes, 0);
        }
        push_u32(&mut bytes, DDS_PIXEL_FORMAT_SIZE);
        push_u32(&mut bytes, DDPF_FOURCC);
        push_u32(&mut bytes, D3DFMT_A16B16G16R16F);
        for _ in 0..5 {
            push_u32(&mut bytes, 0);
        }
        push_u32(&mut bytes, 0x0040_1008);
        push_u32(&mut bytes, DDSCAPS2_CUBEMAP_ALL_FACES);
        for _ in 0..3 {
            push_u32(&mut bytes, 0);
        }
        assert_eq!(bytes.len(), 128);
        for face in 0..6_u8 {
            bytes.extend(std::iter::repeat_n(face, 4 * RGBA16F_BYTES));
            bytes.extend(std::iter::repeat_n(face + 16, RGBA16F_BYTES));
        }
        bytes
    }

    #[test]
    fn dds_numeric_fourcc_cube_is_split_face_then_mip() {
        let texture = DdxTexture::from_bytes(&two_by_two_cube()).unwrap();
        assert_eq!(texture.info.data_format, DataFormat::A16B16G16R16F);
        assert_eq!(texture.info.resource_type, ResourceType::CubeMap);
        assert_eq!(texture.info.num_mip_levels, 2);
        assert_eq!(
            texture.subresource_layout().order,
            SubresourceOrder::FaceMajor
        );
        let subresources = texture.subresources().unwrap();
        assert_eq!(subresources.len(), 12);
        assert_eq!(subresources[0], [0; 32]);
        assert_eq!(subresources[1], [16; 8]);
        assert_eq!(subresources[10], [5; 32]);
        assert_eq!(subresources[11], [21; 8]);
    }

    #[test]
    fn mip_zero_first_payload_is_returned_face_major() {
        let layout = SubresourceLayout {
            data_format: DataFormat::A16B16G16R16F,
            width: 2,
            height: 2,
            faces: 6,
            mip_levels: 2,
            order: SubresourceOrder::MipZeroFirst,
        };
        let mut payload = Vec::new();
        for face in 0..6_u8 {
            payload.extend(std::iter::repeat_n(face, 4 * RGBA16F_BYTES));
        }
        for face in 0..6_u8 {
            payload.extend(std::iter::repeat_n(face + 16, RGBA16F_BYTES));
        }
        let subresources = layout.split(&payload).unwrap();
        assert_eq!(subresources.len(), 12);
        for face in 0..6_u8 {
            let index = usize::from(face) * 2;
            assert_eq!(subresources[index], vec![face; 32]);
            assert_eq!(subresources[index + 1], vec![face + 16; 8]);
        }
    }

    #[test]
    fn split_rejects_truncated_and_trailing_payloads() {
        let cube = two_by_two_cube();
        let mut texture = DdxTexture::from_bytes(&cube).unwrap();
        texture.data.push(0);
        assert!(matches!(
            texture.subresources(),
            Err(Error::TrailingData(1))
        ));
        texture.data.truncate(texture.data.len() - 2);
        assert!(matches!(
            texture.subresources(),
            Err(Error::SubresourceTruncated {
                offset: 232,
                needed: 8,
                available: 7,
            })
        ));
    }
}
