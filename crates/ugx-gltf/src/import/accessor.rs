//! Buffer resolution and accessor reading for glTF import.

use base64::{Engine, engine::general_purpose::STANDARD};

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
            .map_err(|e| Error::UnsupportedFormat(format!("Invalid base64 buffer: {}", e)))?;
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

    let byte_offset = accessor.byte_offset.map(|o| o.0 as usize).unwrap_or(0)
        + view.byte_offset.map(|o| o.0 as usize).unwrap_or(0);
    let stride = view.byte_stride.map(|s| s.0);
    let count = accessor.count.0 as usize;

    let components = match accessor.type_ {
        gltf_json::validation::Checked::Valid(gltf_json::accessor::Type::Scalar) => 1,
        gltf_json::validation::Checked::Valid(gltf_json::accessor::Type::Vec2) => 2,
        gltf_json::validation::Checked::Valid(gltf_json::accessor::Type::Vec3) => 3,
        gltf_json::validation::Checked::Valid(gltf_json::accessor::Type::Vec4) => 4,
        gltf_json::validation::Checked::Valid(gltf_json::accessor::Type::Mat4) => 16,
        _ => return Err(Error::UnsupportedFormat("Unsupported accessor type".into())),
    };

    let component_size = match accessor.component_type {
        gltf_json::validation::Checked::Valid(gltf_json::accessor::GenericComponentType(ct)) => {
            match ct {
                gltf_json::accessor::ComponentType::F32 => 4,
                gltf_json::accessor::ComponentType::U8 => 1,
                gltf_json::accessor::ComponentType::U16 => 2,
                gltf_json::accessor::ComponentType::I8 => 1,
                gltf_json::accessor::ComponentType::I16 => 2,
                gltf_json::accessor::ComponentType::U32 => 4,
            }
        }
        _ => return Err(Error::UnsupportedFormat("Invalid component type".into())),
    };

    let element_size = components * component_size;
    let actual_stride = stride.unwrap_or(element_size);

    let eof = || Error::UnsupportedFormat("Accessor reads past end of buffer".into());

    let mut result = Vec::with_capacity(count * components);
    for i in 0..count {
        let elem_offset = byte_offset + i * actual_stride;
        for c in 0..components {
            let offset = elem_offset + c * component_size;
            let value = match accessor.component_type {
                gltf_json::validation::Checked::Valid(
                    gltf_json::accessor::GenericComponentType(ct),
                ) => match ct {
                    gltf_json::accessor::ComponentType::F32 => {
                        let b: [u8; 4] = buffer_bytes
                            .get(offset..offset + 4)
                            .ok_or_else(eof)?
                            .try_into()
                            .map_err(|_| eof())?;
                        f32::from_le_bytes(b)
                    }
                    gltf_json::accessor::ComponentType::U8 => {
                        *buffer_bytes.get(offset).ok_or_else(eof)? as f32
                    }
                    gltf_json::accessor::ComponentType::U16 => {
                        let b: [u8; 2] = buffer_bytes
                            .get(offset..offset + 2)
                            .ok_or_else(eof)?
                            .try_into()
                            .map_err(|_| eof())?;
                        u16::from_le_bytes(b) as f32
                    }
                    gltf_json::accessor::ComponentType::I8 => {
                        *buffer_bytes.get(offset).ok_or_else(eof)? as i8 as f32
                    }
                    gltf_json::accessor::ComponentType::I16 => {
                        let b: [u8; 2] = buffer_bytes
                            .get(offset..offset + 2)
                            .ok_or_else(eof)?
                            .try_into()
                            .map_err(|_| eof())?;
                        i16::from_le_bytes(b) as f32
                    }
                    gltf_json::accessor::ComponentType::U32 => {
                        let b: [u8; 4] = buffer_bytes
                            .get(offset..offset + 4)
                            .ok_or_else(eof)?
                            .try_into()
                            .map_err(|_| eof())?;
                        u32::from_le_bytes(b) as f32
                    }
                },
                _ => 0.0,
            };
            result.push(value);
        }
    }

    Ok(result)
}
