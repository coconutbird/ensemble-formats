//! Buffer resolution and accessor reading for glTF import.

use base64::{Engine, engine::general_purpose::STANDARD};
use num_traits::ToPrimitive;

use ugx::{Error, Result};

/// Resolve the binary buffer data from either the provided bytes or embedded base64.
pub(crate) fn resolve_buffer(
    root: &gltf_json::Root,
    external_data: Option<&[u8]>,
) -> Result<Vec<u8>> {
    if let Some(data) = external_data {
        return Ok(data.to_vec());
    }

    // Try to decode from the first buffer's URI (base64 embedded)
    if let Some(buffer) = root.buffers.first()
        && let Some(ref uri) = buffer.uri
        && let Some(base64_data) = uri.strip_prefix("data:application/octet-stream;base64,")
    {
        let decoded = STANDARD
            .decode(base64_data)
            .map_err(|e| Error::UnsupportedFormat(format!("Invalid base64 buffer: {e}")))?;
        return Ok(decoded);
    }

    // No buffer data available — might be a mesh with no buffer
    Ok(Vec::new())
}

/// Read accessor data as f32 values from the buffer.
pub(crate) fn read_accessor_f32(
    accessor: &gltf_json::Accessor,
    root: &gltf_json::Root,
    buffer_bytes: &[u8],
) -> Result<Vec<f32>> {
    let view_idx = accessor
        .buffer_view
        .ok_or_else(|| Error::UnsupportedFormat("Accessor missing buffer_view".into()))?;
    let view = &root.buffer_views[view_idx.value()];

    let accessor_offset = accessor
        .byte_offset
        .map_or(Ok(0), |offset| checked_usize(offset.0, "accessor offset"))?;
    let view_offset = view.byte_offset.map_or(Ok(0), |offset| {
        checked_usize(offset.0, "buffer-view offset")
    })?;
    let byte_offset = accessor_offset
        .checked_add(view_offset)
        .ok_or(Error::SizeOverflow("combined accessor offset"))?;
    let stride = view.byte_stride.map(|s| s.0);
    let count = checked_usize(accessor.count.0, "accessor element count")?;

    let components: usize = match accessor.type_ {
        gltf_json::validation::Checked::Valid(gltf_json::accessor::Type::Scalar) => 1,
        gltf_json::validation::Checked::Valid(gltf_json::accessor::Type::Vec2) => 2,
        gltf_json::validation::Checked::Valid(gltf_json::accessor::Type::Vec3) => 3,
        gltf_json::validation::Checked::Valid(
            gltf_json::accessor::Type::Vec4 | gltf_json::accessor::Type::Mat2,
        ) => 4,
        gltf_json::validation::Checked::Valid(gltf_json::accessor::Type::Mat3) => 9,
        gltf_json::validation::Checked::Valid(gltf_json::accessor::Type::Mat4) => 16,
        gltf_json::validation::Checked::Invalid => {
            return Err(Error::UnsupportedFormat("Unsupported accessor type".into()));
        }
    };

    let component_type = match accessor.component_type {
        gltf_json::validation::Checked::Valid(gltf_json::accessor::GenericComponentType(ct)) => ct,
        gltf_json::validation::Checked::Invalid => {
            return Err(Error::UnsupportedFormat("Invalid component type".into()));
        }
    };
    let component_size = match component_type {
        gltf_json::accessor::ComponentType::U8 | gltf_json::accessor::ComponentType::I8 => 1,
        gltf_json::accessor::ComponentType::U16 | gltf_json::accessor::ComponentType::I16 => 2,
        gltf_json::accessor::ComponentType::F32 | gltf_json::accessor::ComponentType::U32 => 4,
    };

    let element_size = components
        .checked_mul(component_size)
        .ok_or(Error::SizeOverflow("accessor element size"))?;
    let actual_stride = stride.unwrap_or(element_size);
    let capacity = count
        .checked_mul(components)
        .ok_or(Error::SizeOverflow("accessor output length"))?;
    let mut result = Vec::with_capacity(capacity);
    for element_index in 0..count {
        let element_offset = element_index
            .checked_mul(actual_stride)
            .and_then(|offset| byte_offset.checked_add(offset))
            .ok_or(Error::SizeOverflow("accessor element offset"))?;
        for component_index in 0..components {
            let offset = component_index
                .checked_mul(component_size)
                .and_then(|offset| element_offset.checked_add(offset))
                .ok_or(Error::SizeOverflow("accessor component offset"))?;
            result.push(read_component(component_type, buffer_bytes, offset)?);
        }
    }

    Ok(result)
}

fn checked_usize(value: u64, context: &'static str) -> Result<usize> {
    usize::try_from(value).map_err(|_| Error::SizeOverflow(context))
}

fn read_component(
    component_type: gltf_json::accessor::ComponentType,
    data: &[u8],
    offset: usize,
) -> Result<f32> {
    let eof = || Error::UnsupportedFormat("Accessor reads past end of buffer".into());
    match component_type {
        gltf_json::accessor::ComponentType::F32 => {
            Ok(f32::from_le_bytes(read_array(data, offset)?))
        }
        gltf_json::accessor::ComponentType::U8 => Ok(f32::from(*data.get(offset).ok_or_else(eof)?)),
        gltf_json::accessor::ComponentType::U16 => {
            Ok(f32::from(u16::from_le_bytes(read_array(data, offset)?)))
        }
        gltf_json::accessor::ComponentType::I8 => {
            Ok(f32::from(data.get(offset).ok_or_else(eof)?.cast_signed()))
        }
        gltf_json::accessor::ComponentType::I16 => {
            Ok(f32::from(i16::from_le_bytes(read_array(data, offset)?)))
        }
        gltf_json::accessor::ComponentType::U32 => u32::from_le_bytes(read_array(data, offset)?)
            .to_f32()
            .ok_or(Error::SizeOverflow("u32 accessor component")),
    }
}

fn read_array<const N: usize>(data: &[u8], offset: usize) -> Result<[u8; N]> {
    let end = offset
        .checked_add(N)
        .ok_or(Error::SizeOverflow("accessor byte range"))?;
    data.get(offset..end)
        .ok_or_else(|| Error::UnsupportedFormat("Accessor reads past end of buffer".into()))?
        .try_into()
        .map_err(|_| Error::UnsupportedFormat("Invalid accessor component size".into()))
}
