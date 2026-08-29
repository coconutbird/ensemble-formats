//! Checked glTF and GLB file loading for the command-line converter.

use std::path::{Path, PathBuf};

const GLB_HEADER_LENGTH: usize = 12;
const GLB_CHUNK_HEADER_LENGTH: usize = 8;
const GLB_MAGIC: u32 = 0x4654_6C67;
const GLB_VERSION: u32 = 2;
const JSON_CHUNK: u32 = 0x4E4F_534A;
const BIN_CHUNK: u32 = 0x004E_4942;

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("cannot read glTF data: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid glTF JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("GLB header is truncated")]
    TruncatedHeader,
    #[error("invalid GLB magic 0x{0:08X}")]
    InvalidMagic(u32),
    #[error("unsupported GLB version {0}")]
    UnsupportedVersion(u32),
    #[error("GLB length {declared} does not match file length {actual}")]
    LengthMismatch { declared: usize, actual: usize },
    #[error("GLB chunk header at offset {0} is truncated")]
    TruncatedChunkHeader(usize),
    #[error("GLB chunk at offset {offset} extends beyond the file")]
    ChunkOutOfBounds { offset: usize },
    #[error("GLB chunk at offset {offset} has unaligned length {length}")]
    UnalignedChunkLength { offset: usize, length: usize },
    #[error("the first GLB chunk is not JSON")]
    JsonChunkNotFirst,
    #[error("GLB contains more than one JSON chunk")]
    DuplicateJsonChunk,
    #[error("GLB contains more than one binary chunk")]
    DuplicateBinChunk,
    #[error("GLB is missing its JSON chunk")]
    MissingJsonChunk,
    #[error("GLB JSON chunk is not valid UTF-8: {0}")]
    InvalidUtf8(#[from] std::string::FromUtf8Error),
    #[error("external buffer '{uri}' was not found at {path}")]
    MissingExternalBuffer { uri: String, path: PathBuf },
    #[error("{0} cannot be represented on this platform")]
    SizeOverflow(&'static str),
}

pub(crate) struct GltfSource {
    pub json: String,
    pub buffer: Option<Vec<u8>>,
}

pub(crate) fn read(path: &Path) -> Result<GltfSource, Error> {
    if path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("glb"))
    {
        return parse_glb(&std::fs::read(path)?);
    }
    read_gltf(path)
}

fn read_gltf(path: &Path) -> Result<GltfSource, Error> {
    let json = std::fs::read_to_string(path)?;
    let root: serde_json::Value = serde_json::from_str(&json)?;
    let uri = root
        .get("buffers")
        .and_then(serde_json::Value::as_array)
        .and_then(|buffers| buffers.first())
        .and_then(|buffer| buffer.get("uri"))
        .and_then(serde_json::Value::as_str);
    let buffer = match uri {
        Some(uri) if !uri.starts_with("data:") => {
            let base_dir = path.parent().unwrap_or_else(|| Path::new("."));
            let buffer_path = base_dir.join(uri);
            if !buffer_path.is_file() {
                return Err(Error::MissingExternalBuffer {
                    uri: uri.to_string(),
                    path: buffer_path,
                });
            }
            Some(std::fs::read(buffer_path)?)
        }
        _ => None,
    };
    Ok(GltfSource { json, buffer })
}

fn parse_glb(data: &[u8]) -> Result<GltfSource, Error> {
    if data.len() < GLB_HEADER_LENGTH {
        return Err(Error::TruncatedHeader);
    }
    let magic = read_u32(data, 0, "GLB magic")?;
    if magic != GLB_MAGIC {
        return Err(Error::InvalidMagic(magic));
    }
    let version = read_u32(data, 4, "GLB version")?;
    if version != GLB_VERSION {
        return Err(Error::UnsupportedVersion(version));
    }
    let declared = usize::try_from(read_u32(data, 8, "GLB length")?)
        .map_err(|_| Error::SizeOverflow("GLB length"))?;
    if declared != data.len() {
        return Err(Error::LengthMismatch {
            declared,
            actual: data.len(),
        });
    }

    let mut offset = GLB_HEADER_LENGTH;
    let mut json = None;
    let mut buffer = None;
    while offset < data.len() {
        let header_end = offset
            .checked_add(GLB_CHUNK_HEADER_LENGTH)
            .ok_or(Error::SizeOverflow("GLB chunk header range"))?;
        if header_end > data.len() {
            return Err(Error::TruncatedChunkHeader(offset));
        }
        let chunk_length = usize::try_from(read_u32(data, offset, "GLB chunk length")?)
            .map_err(|_| Error::SizeOverflow("GLB chunk length"))?;
        let chunk_type_offset = offset
            .checked_add(4)
            .ok_or(Error::SizeOverflow("GLB chunk type offset"))?;
        let chunk_type = read_u32(data, chunk_type_offset, "GLB chunk type")?;
        if !chunk_length.is_multiple_of(4) {
            return Err(Error::UnalignedChunkLength {
                offset,
                length: chunk_length,
            });
        }
        if offset == GLB_HEADER_LENGTH && chunk_type != JSON_CHUNK {
            return Err(Error::JsonChunkNotFirst);
        }
        let chunk_end = header_end
            .checked_add(chunk_length)
            .ok_or(Error::SizeOverflow("GLB chunk range"))?;
        let payload = data
            .get(header_end..chunk_end)
            .ok_or(Error::ChunkOutOfBounds { offset })?;
        match chunk_type {
            JSON_CHUNK if json.is_some() => return Err(Error::DuplicateJsonChunk),
            JSON_CHUNK => json = Some(String::from_utf8(payload.to_vec())?),
            BIN_CHUNK if buffer.is_some() => return Err(Error::DuplicateBinChunk),
            BIN_CHUNK => buffer = Some(payload.to_vec()),
            _ => {}
        }
        offset = chunk_end;
    }
    Ok(GltfSource {
        json: json.ok_or(Error::MissingJsonChunk)?,
        buffer,
    })
}

fn read_u32(data: &[u8], offset: usize, context: &'static str) -> Result<u32, Error> {
    let end = offset.checked_add(4).ok_or(Error::SizeOverflow(context))?;
    let bytes: [u8; 4] = data
        .get(offset..end)
        .ok_or(Error::SizeOverflow(context))?
        .try_into()
        .map_err(|_| Error::SizeOverflow(context))?;
    Ok(u32::from_le_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glb(json: &[u8], binary: Option<&[u8]>) -> Vec<u8> {
        let mut chunks = Vec::new();
        chunks.extend_from_slice(&(u32::try_from(json.len()).unwrap()).to_le_bytes());
        chunks.extend_from_slice(&JSON_CHUNK.to_le_bytes());
        chunks.extend_from_slice(json);
        if let Some(binary) = binary {
            chunks.extend_from_slice(&(u32::try_from(binary.len()).unwrap()).to_le_bytes());
            chunks.extend_from_slice(&BIN_CHUNK.to_le_bytes());
            chunks.extend_from_slice(binary);
        }
        let length = u32::try_from(GLB_HEADER_LENGTH + chunks.len()).unwrap();
        let mut result = Vec::new();
        result.extend_from_slice(&GLB_MAGIC.to_le_bytes());
        result.extend_from_slice(&GLB_VERSION.to_le_bytes());
        result.extend_from_slice(&length.to_le_bytes());
        result.extend(chunks);
        result
    }

    #[test]
    fn parses_json_and_binary_chunks() {
        let source = parse_glb(&glb(
            br#"{"asset":{"version":"2.0"}} "#,
            Some(&[1, 2, 3, 0]),
        ))
        .unwrap();

        assert!(source.json.contains("asset"));
        assert_eq!(source.buffer, Some(vec![1, 2, 3, 0]));
    }

    #[test]
    fn rejects_declared_length_mismatch() {
        let mut data = glb(br"{}  ", None);
        data[8..12].copy_from_slice(&12u32.to_le_bytes());

        assert!(matches!(
            parse_glb(&data),
            Err(Error::LengthMismatch { .. })
        ));
    }

    #[test]
    fn rejects_out_of_bounds_chunk() {
        let mut data = glb(br"{}  ", None);
        data[12..16].copy_from_slice(&(u32::MAX - 3).to_le_bytes());

        assert!(matches!(
            parse_glb(&data),
            Err(Error::ChunkOutOfBounds { .. })
        ));
    }

    #[test]
    fn rejects_unaligned_chunk_length() {
        let mut data = glb(br"{}  ", None);
        data[12..16].copy_from_slice(&3u32.to_le_bytes());

        assert!(matches!(
            parse_glb(&data),
            Err(Error::UnalignedChunkLength { .. })
        ));
    }

    #[test]
    fn requires_json_to_be_the_first_chunk() {
        let mut data = glb(br"{}  ", None);
        data[16..20].copy_from_slice(&BIN_CHUNK.to_le_bytes());

        assert!(matches!(parse_glb(&data), Err(Error::JsonChunkNotFirst)));
    }
}
