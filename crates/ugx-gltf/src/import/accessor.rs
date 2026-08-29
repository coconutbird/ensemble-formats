//! Buffer resolution and accessor reading for glTF import.

use base64::{Engine, engine::general_purpose::STANDARD};
use num_traits::ToPrimitive;

use ugx::{Error, Result};

/// Resolve the binary buffer data from either the provided bytes or embedded base64.
pub(crate) fn resolve_buffer(
    root: &gltf_json::Root,
    external_data: Option<&[u8]>,
) -> Result<Vec<u8>> {
    if root.buffers.len() > 1 {
        return Err(Error::UnsupportedFormat(
            "UGX import supports one glTF buffer".into(),
        ));
    }
    if let Some(data) = external_data {
        validate_buffer_length(root, data)?;
        return Ok(data.to_vec());
    }

    // Try to decode the first buffer's base64 data URI.
    if let Some(buffer) = root.buffers.first()
        && let Some(ref uri) = buffer.uri
        && let Some((_, base64_data)) = uri.split_once(";base64,")
        && uri.starts_with("data:")
    {
        let decoded = STANDARD
            .decode(base64_data)
            .map_err(|e| Error::UnsupportedFormat(format!("Invalid base64 buffer: {e}")))?;
        validate_buffer_length(root, &decoded)?;
        return Ok(decoded);
    }

    // No buffer data available — might be a mesh with no buffer
    Ok(Vec::new())
}

fn validate_buffer_length(root: &gltf_json::Root, data: &[u8]) -> Result<()> {
    let Some(buffer) = root.buffers.first() else {
        return Ok(());
    };
    let declared = checked_usize(buffer.byte_length.0, "buffer byte length")?;
    if data.len() < declared {
        return Err(Error::UnsupportedFormat(format!(
            "glTF buffer has {} bytes; expected at least {declared}",
            data.len()
        )));
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct AccessorLayout {
    byte_offset: usize,
    count: usize,
    components: usize,
    component_size: usize,
    stride: usize,
    component_type: gltf_json::accessor::ComponentType,
    normalized: bool,
}

/// Read accessor data as f32 values from the buffer.
pub(crate) fn read_accessor_f32(
    accessor: &gltf_json::Accessor,
    root: &gltf_json::Root,
    buffer_bytes: &[u8],
) -> Result<Vec<f32>> {
    let layout = accessor_layout(accessor, root, buffer_bytes)?;
    let capacity = layout
        .count
        .checked_mul(layout.components)
        .ok_or(Error::SizeOverflow("accessor output length"))?;
    let mut result = Vec::with_capacity(capacity);
    for element_index in 0..layout.count {
        let element_offset = element_index
            .checked_mul(layout.stride)
            .and_then(|offset| layout.byte_offset.checked_add(offset))
            .ok_or(Error::SizeOverflow("accessor element offset"))?;
        for component_index in 0..layout.components {
            let offset = component_index
                .checked_mul(layout.component_size)
                .and_then(|offset| element_offset.checked_add(offset))
                .ok_or(Error::SizeOverflow("accessor component offset"))?;
            result.push(read_component(
                layout.component_type,
                layout.normalized,
                buffer_bytes,
                offset,
            )?);
        }
    }
    Ok(result)
}

fn accessor_layout(
    accessor: &gltf_json::Accessor,
    root: &gltf_json::Root,
    buffer_bytes: &[u8],
) -> Result<AccessorLayout> {
    if accessor.sparse.is_some() {
        return Err(Error::UnsupportedFormat(
            "Sparse glTF accessors are not supported".into(),
        ));
    }
    let (byte_offset, accessor_offset, view_length, declared_stride) =
        accessor_view(accessor, root, buffer_bytes)?;
    let count = checked_usize(accessor.count.0, "accessor element count")?;
    let components = accessor_component_count(accessor)?;
    let component_type = accessor_component_type(accessor)?;
    let component_size = match component_type {
        gltf_json::accessor::ComponentType::U8 | gltf_json::accessor::ComponentType::I8 => 1,
        gltf_json::accessor::ComponentType::U16 | gltf_json::accessor::ComponentType::I16 => 2,
        gltf_json::accessor::ComponentType::F32 | gltf_json::accessor::ComponentType::U32 => 4,
    };
    let element_size = components
        .checked_mul(component_size)
        .ok_or(Error::SizeOverflow("accessor element size"))?;
    let stride = declared_stride.unwrap_or(element_size);
    let layout = AccessorLayout {
        byte_offset,
        count,
        components,
        component_size,
        stride,
        component_type,
        normalized: accessor.normalized,
    };
    validate_accessor_layout(
        accessor,
        &layout,
        accessor_offset,
        view_length,
        element_size,
    )?;
    Ok(layout)
}

fn accessor_view(
    accessor: &gltf_json::Accessor,
    root: &gltf_json::Root,
    buffer_bytes: &[u8],
) -> Result<(usize, usize, usize, Option<usize>)> {
    let view_idx = accessor
        .buffer_view
        .ok_or_else(|| Error::UnsupportedFormat("Accessor missing buffer_view".into()))?;
    let view = root.buffer_views.get(view_idx.value()).ok_or_else(|| {
        Error::UnsupportedFormat("Accessor buffer-view index is out of bounds".into())
    })?;
    if view.buffer.value() != 0 || root.buffers.len() != 1 {
        return Err(Error::UnsupportedFormat(
            "UGX import supports one glTF buffer".into(),
        ));
    }

    let accessor_offset = accessor
        .byte_offset
        .map_or(Ok(0), |offset| checked_usize(offset.0, "accessor offset"))?;
    let view_offset = view.byte_offset.map_or(Ok(0), |offset| {
        checked_usize(offset.0, "buffer-view offset")
    })?;
    let view_length = checked_usize(view.byte_length.0, "buffer-view length")?;
    let view_end = view_offset
        .checked_add(view_length)
        .ok_or(Error::SizeOverflow("buffer-view range"))?;
    if view_end > buffer_bytes.len() {
        return Err(Error::UnsupportedFormat(
            "Buffer view extends past the end of its buffer".into(),
        ));
    }
    let byte_offset = accessor_offset
        .checked_add(view_offset)
        .ok_or(Error::SizeOverflow("combined accessor offset"))?;
    Ok((
        byte_offset,
        accessor_offset,
        view_length,
        view.byte_stride.map(|stride| stride.0),
    ))
}

fn accessor_component_count(accessor: &gltf_json::Accessor) -> Result<usize> {
    Ok(match accessor.type_ {
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
    })
}

fn accessor_component_type(
    accessor: &gltf_json::Accessor,
) -> Result<gltf_json::accessor::ComponentType> {
    Ok(match accessor.component_type {
        gltf_json::validation::Checked::Valid(gltf_json::accessor::GenericComponentType(ct)) => ct,
        gltf_json::validation::Checked::Invalid => {
            return Err(Error::UnsupportedFormat("Invalid component type".into()));
        }
    })
}

fn validate_accessor_layout(
    accessor: &gltf_json::Accessor,
    layout: &AccessorLayout,
    accessor_offset: usize,
    view_length: usize,
    element_size: usize,
) -> Result<()> {
    if layout.stride < element_size || !layout.stride.is_multiple_of(layout.component_size) {
        return Err(Error::UnsupportedFormat(format!(
            "Accessor stride {} is invalid for a {element_size}-byte element",
            layout.stride
        )));
    }
    if !layout.byte_offset.is_multiple_of(layout.component_size) {
        return Err(Error::UnsupportedFormat(
            "Accessor offset is not aligned to its component size".into(),
        ));
    }
    let occupied_bytes = if layout.count == 0 {
        0
    } else {
        (layout.count - 1)
            .checked_mul(layout.stride)
            .and_then(|value| value.checked_add(element_size))
            .ok_or(Error::SizeOverflow("accessor byte range"))?
    };
    let accessor_end = accessor_offset
        .checked_add(occupied_bytes)
        .ok_or(Error::SizeOverflow("accessor buffer-view range"))?;
    if accessor_end > view_length {
        return Err(Error::UnsupportedFormat(
            "Accessor extends past the end of its buffer view".into(),
        ));
    }
    if accessor.normalized
        && matches!(
            layout.component_type,
            gltf_json::accessor::ComponentType::F32 | gltf_json::accessor::ComponentType::U32
        )
    {
        return Err(Error::UnsupportedFormat(
            "Accessor component type cannot be normalized".into(),
        ));
    }
    Ok(())
}

fn checked_usize(value: u64, context: &'static str) -> Result<usize> {
    usize::try_from(value).map_err(|_| Error::SizeOverflow(context))
}

fn read_component(
    component_type: gltf_json::accessor::ComponentType,
    normalized: bool,
    data: &[u8],
    offset: usize,
) -> Result<f32> {
    let eof = || Error::UnsupportedFormat("Accessor reads past end of buffer".into());
    match component_type {
        gltf_json::accessor::ComponentType::F32 => {
            Ok(f32::from_le_bytes(read_array(data, offset)?))
        }
        gltf_json::accessor::ComponentType::U8 => {
            let value = f32::from(*data.get(offset).ok_or_else(eof)?);
            Ok(if normalized { value / 255.0 } else { value })
        }
        gltf_json::accessor::ComponentType::U16 => {
            let value = f32::from(u16::from_le_bytes(read_array(data, offset)?));
            Ok(if normalized { value / 65_535.0 } else { value })
        }
        gltf_json::accessor::ComponentType::I8 => {
            let value = f32::from(data.get(offset).ok_or_else(eof)?.cast_signed());
            Ok(if normalized {
                (value / 127.0).max(-1.0)
            } else {
                value
            })
        }
        gltf_json::accessor::ComponentType::I16 => {
            let value = f32::from(i16::from_le_bytes(read_array(data, offset)?));
            Ok(if normalized {
                (value / 32_767.0).max(-1.0)
            } else {
                value
            })
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

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_float_bits(actual: f32, expected: f32) {
        assert_eq!(actual.to_bits(), expected.to_bits());
    }

    #[test]
    fn normalizes_integer_components() {
        assert_float_bits(
            read_component(gltf_json::accessor::ComponentType::U8, true, &[255], 0).unwrap(),
            1.0,
        );
        assert_float_bits(
            read_component(gltf_json::accessor::ComponentType::I8, true, &[128], 0).unwrap(),
            -1.0,
        );
        assert_float_bits(
            read_component(
                gltf_json::accessor::ComponentType::U16,
                true,
                &u16::MAX.to_le_bytes(),
                0,
            )
            .unwrap(),
            1.0,
        );
        assert_float_bits(
            read_component(
                gltf_json::accessor::ComponentType::I16,
                true,
                &i16::MIN.to_le_bytes(),
                0,
            )
            .unwrap(),
            -1.0,
        );
    }
}
