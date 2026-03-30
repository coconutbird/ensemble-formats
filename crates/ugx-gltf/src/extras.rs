//! Serializable material extras for glTF roundtrip.
//!
//! These structs map 1:1 to the JSON stored in glTF material `extras`.
//! Using `#[derive(Serialize, Deserialize)]` replaces ~200 lines of manual
//! `serde_json::Map::insert` / `obj.get(...)` calls in export and import.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Top-level material extras stored in glTF.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct MaterialExtrasJson {
    /// Material version from `@Ver` attribute (4 = HW1, 5 = HW2 legacy).
    #[serde(default = "default_mat_version")]
    pub ugx_material_version: u32,

    // --- Legacy fields (present when material is Legacy) ---
    #[serde(default)]
    pub ugx_flags: Option<u32>,
    #[serde(default)]
    pub ugx_blend_type: Option<u8>,
    #[serde(default)]
    pub ugx_spec_power: Option<f32>,
    #[serde(default)]
    pub ugx_spec_color: Option<[f32; 3]>,
    #[serde(default)]
    pub ugx_env_reflectivity: Option<f32>,
    #[serde(default)]
    pub ugx_env_sharpness: Option<f32>,
    #[serde(default)]
    pub ugx_env_fresnel: Option<f32>,
    #[serde(default)]
    pub ugx_env_fresnel_power: Option<f32>,
    #[serde(default)]
    pub ugx_accessory_index: Option<u32>,
    #[serde(default)]
    pub ugx_opacity: Option<f32>,

    /// UVW velocity per map type (only present if any non-zero).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ugx_uvw_velocity: Option<Vec<[f32; 3]>>,

    /// Texture maps keyed by map type name, each with name/channel/flags.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ugx_maps: Option<std::collections::BTreeMap<String, Vec<MapEntryJson>>>,

    // --- Hogan fields (present when material is Hogan) ---
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ugx_hogan: Option<HoganExtrasJson>,
}

/// A single texture map entry in extras.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct MapEntryJson {
    pub name: String,
    pub channel: i16,
    pub flags: u8,
}

/// HW2 Hogan material data stored in extras.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct HoganExtrasJson {
    #[serde(default)]
    pub shader_permutations: Vec<ShaderPermJson>,
    #[serde(default = "default_ufx_version")]
    pub ufx_version: u32,
    #[serde(default)]
    pub blend_mode: u32,
    #[serde(default)]
    pub shadow_requires_consts: bool,
    #[serde(default)]
    pub skinned: bool,
    #[serde(default)]
    pub terrain_blending: bool,

    /// Shader bitflags hex string (e.g. `"00080000a8000960"`).
    ///
    /// Determines which CB parameters are allocated and their packing order.
    /// Used to reconstruct the raw constant buffer layout on re-import.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shader_flags: Option<String>,

    /// Named vertex shader CB parameters (new format, preferred).
    ///
    /// Keys are parameter names derived from the shader flags, values are
    /// floats or float arrays (scalars stored as numbers, multi-component as arrays).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vs_cb: Option<BTreeMap<String, serde_json::Value>>,

    /// Named pixel shader CB parameters (new format, preferred).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ps_cb: Option<BTreeMap<String, serde_json::Value>>,

    /// Vertex shader runtime parameters as ordered float4 arrays (legacy fallback).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vs_params: Vec<[f32; 4]>,
    /// Pixel shader runtime parameters as ordered float4 arrays (legacy fallback).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ps_params: Vec<[f32; 4]>,
    /// Hull shader runtime parameters as ordered float4 arrays.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hs_params: Vec<[f32; 4]>,
    /// Domain shader runtime parameters as ordered float4 arrays.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ds_params: Vec<[f32; 4]>,
    /// Geometry shader runtime parameters as ordered float4 arrays.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gs_params: Vec<[f32; 4]>,
    #[serde(default)]
    pub textures: String,
}

/// Decode raw CB bytes into float4 parameter arrays.
///
/// Each 16-byte chunk becomes one `[f32; 4]` entry.
/// Trailing bytes that don't fill a complete float4 are ignored.
pub(crate) fn cb_bytes_to_params(data: &[u8]) -> Vec<[f32; 4]> {
    data.chunks_exact(16)
        .map(|chunk| {
            [
                f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]),
                f32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]),
                f32::from_le_bytes([chunk[8], chunk[9], chunk[10], chunk[11]]),
                f32::from_le_bytes([chunk[12], chunk[13], chunk[14], chunk[15]]),
            ]
        })
        .collect()
}

/// Encode float4 parameter arrays back into raw CB bytes.
pub(crate) fn params_to_cb_bytes(params: &[[f32; 4]]) -> Vec<u8> {
    let mut out = Vec::with_capacity(params.len() * 16);
    for p in params {
        out.extend_from_slice(&p[0].to_le_bytes());
        out.extend_from_slice(&p[1].to_le_bytes());
        out.extend_from_slice(&p[2].to_le_bytes());
        out.extend_from_slice(&p[3].to_le_bytes());
    }
    out
}

/// Convert raw CB bytes into a named parameter map using a predicted layout.
///
/// When the layout has entries, reads floats at each entry's predicted offset
/// and stores them as named JSON values.
///
/// When the layout is **empty** (unknown shader family), falls back to
/// positional naming: each float becomes `f0`, `f1`, `f2`, etc.
/// This ensures every Hogan family round-trips through glTF extras.
pub(crate) fn cb_bytes_to_named(
    data: &[u8],
    layout: &[crate::hogan_cb_layout::CbLayoutEntry],
) -> BTreeMap<String, serde_json::Value> {
    let floats = cb_bytes_to_floats(data);
    let mut map = BTreeMap::new();

    if layout.is_empty() {
        // Positional fallback: name each float by index.
        for (i, &v) in floats.iter().enumerate() {
            map.insert(format!("f{i}"), serde_json::Value::from(v));
        }
    } else {
        for entry in layout {
            let start = entry.offset as usize;
            let n = entry.components as usize;
            let vals: Vec<f32> = (0..n)
                .map(|i| floats.get(start + i).copied().unwrap_or(0.0))
                .collect();
            let value = if n == 1 {
                serde_json::Value::from(vals[0])
            } else {
                serde_json::Value::Array(vals.iter().map(|&f| serde_json::Value::from(f)).collect())
            };
            map.insert(entry.name.to_string(), value);
        }
    }
    map
}

/// Reconstruct raw CB bytes from a named parameter map and predicted layout.
///
/// When the layout has entries, allocates a float array based on register
/// count and fills named values at their predicted offsets.
///
/// When the layout is **empty** (positional fallback), reconstructs from
/// `f0`, `f1`, `f2`… keys in sorted order.
pub(crate) fn named_to_cb_bytes(
    map: &BTreeMap<String, serde_json::Value>,
    layout: &[crate::hogan_cb_layout::CbLayoutEntry],
    register_count: u32,
) -> Vec<u8> {
    if layout.is_empty() {
        // Positional fallback: collect f0, f1, f2… in numeric order.
        return positional_map_to_bytes(map);
    }

    let total_floats = register_count as usize * 4;
    let mut floats = vec![0.0f32; total_floats];

    for entry in layout {
        let start = entry.offset as usize;
        if let Some(value) = map.get(entry.name) {
            let values = json_value_to_floats(value, entry.components);
            for (i, &v) in values.iter().enumerate() {
                if start + i < floats.len() {
                    floats[start + i] = v;
                }
            }
        }
    }

    // If every float is zero the original blob was likely empty — preserve that.
    if floats.iter().all(|&f| f == 0.0) {
        return Vec::new();
    }

    floats_to_cb_bytes(&floats)
}

/// Reconstruct raw CB bytes from positional `f0`, `f1`, … keys.
///
/// Returns an empty vec if the map has no positional keys.
fn positional_map_to_bytes(map: &BTreeMap<String, serde_json::Value>) -> Vec<u8> {
    // Find the highest index to determine array size.
    let max_idx = map
        .keys()
        .filter_map(|k| k.strip_prefix('f').and_then(|s| s.parse::<usize>().ok()))
        .max();

    let Some(max_idx) = max_idx else {
        return Vec::new();
    };

    let mut floats = vec![0.0f32; max_idx + 1];
    for (key, value) in map {
        if let Some(idx) = key.strip_prefix('f').and_then(|s| s.parse::<usize>().ok())
            && idx < floats.len()
        {
            floats[idx] = json_value_to_floats(value, 1)[0];
        }
    }

    // If every float is zero, the original blob was likely empty.
    if floats.iter().all(|&f| f == 0.0) {
        return Vec::new();
    }

    floats_to_cb_bytes(&floats)
}

/// Decode raw CB bytes into a flat float slice.
fn cb_bytes_to_floats(data: &[u8]) -> Vec<f32> {
    data.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

/// Encode a flat float slice into raw CB bytes.
fn floats_to_cb_bytes(floats: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(floats.len() * 4);
    for &f in floats {
        out.extend_from_slice(&f.to_le_bytes());
    }
    out
}

/// Extract floats from a JSON value (number or array of numbers).
fn json_value_to_floats(value: &serde_json::Value, expected: u8) -> Vec<f32> {
    match value {
        serde_json::Value::Number(n) => {
            vec![n.as_f64().unwrap_or(0.0) as f32]
        }
        serde_json::Value::Array(arr) => arr
            .iter()
            .take(expected as usize)
            .map(|v| v.as_f64().unwrap_or(0.0) as f32)
            .collect(),
        _ => vec![0.0; expected as usize],
    }
}

/// Shader permutation entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ShaderPermJson {
    pub name: String,
    pub hash: u32,
}

/// Mesh-level extras stored in glTF mesh `extras`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct MeshExtrasJson {
    /// Triangle indices per bone name (from Granny bone bindings).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ugx_triangle_indices: Option<std::collections::BTreeMap<String, Vec<i32>>>,

    // The following fields are read by import but NOT written by the current
    // exporter (the import side infers them from vertex bone weights).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ugx_global_bones: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ugx_rigid_only: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ugx_rigid_bone_index: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ugx_granny_mesh_index: Option<usize>,

    /// LOD near transition distance (HW2 section +0x2C).
    /// Omitted when `0.0` (default for single-LOD or closest LOD).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub ugx_lod_near_distance: f32,
    /// LOD far transition distance (HW2 section +0x30).
    /// Omitted when `f32::MAX` (default = always visible).
    #[serde(default = "default_lod_far", skip_serializing_if = "is_f32_max")]
    pub ugx_lod_far_distance: f32,
    /// LOD vertical fade distance (HW2 section +0x34).
    /// Omitted when `0.0` (default = no atmospheric fade).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub ugx_lod_fade_distance: f32,
}

impl MeshExtrasJson {
    /// Returns `true` when no extras data is present.
    pub fn is_empty(&self) -> bool {
        self.ugx_triangle_indices.is_none()
            && self.ugx_global_bones.is_none()
            && self.ugx_rigid_only.is_none()
            && self.ugx_rigid_bone_index.is_none()
            && self.ugx_granny_mesh_index.is_none()
            && is_zero(&self.ugx_lod_near_distance)
            && is_f32_max(&self.ugx_lod_far_distance)
            && is_zero(&self.ugx_lod_fade_distance)
    }
}

/// Scene-level extras stored in glTF scene `extras`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SceneExtrasJson {
    pub ugx_max_instances: i16,
}

/// Serialize a value to a `Box<RawValue>` suitable for glTF `extras`.
///
/// Returns `None` only if serialization fails (which shouldn't happen for
/// well-formed types).
pub(crate) fn to_raw_value<T: Serialize>(val: &T) -> Option<Box<serde_json::value::RawValue>> {
    let json_str = serde_json::to_string(val).ok()?;
    serde_json::value::RawValue::from_string(json_str).ok()
}

fn default_mat_version() -> u32 {
    4
}

fn default_ufx_version() -> u32 {
    9
}

fn default_lod_far() -> f32 {
    f32::MAX
}

fn is_zero(v: &f32) -> bool {
    *v == 0.0
}

fn is_f32_max(v: &f32) -> bool {
    *v == f32::MAX
}
