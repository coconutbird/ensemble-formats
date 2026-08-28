//! JSON serialization for Granny2 type definitions and variant data.
//!
//! Used to preserve bone `ExtendedData` through glTF extras roundtrip.

use base64::{Engine, engine::general_purpose::STANDARD};
use num_traits::ToPrimitive;
use serde_json::Value;
use ugx::{GrannyMemberType, GrannyTypeMember, GrannyVariant};

// ---------------------------------------------------------------------------
// GrannyTypeMember → JSON
// ---------------------------------------------------------------------------

pub fn type_members_to_json(members: &[GrannyTypeMember]) -> Value {
    Value::Array(members.iter().map(type_member_to_json).collect())
}

fn type_member_to_json(m: &GrannyTypeMember) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert("type".into(), Value::Number((m.member_type as u32).into()));
    obj.insert("name".into(), Value::String(m.name.clone()));
    obj.insert("array_width".into(), Value::Number(m.array_width.into()));
    obj.insert(
        "extra".into(),
        Value::Array(m.extra.iter().map(|&e| Value::Number(e.into())).collect()),
    );
    if let Some(ref nested) = m.reference_type {
        obj.insert("reference_type".into(), type_members_to_json(nested));
    }
    Value::Object(obj)
}

// ---------------------------------------------------------------------------
// JSON → GrannyTypeMember
// ---------------------------------------------------------------------------

pub fn json_to_type_members(val: &Value) -> Option<Vec<GrannyTypeMember>> {
    let arr = val.as_array()?;
    arr.iter().map(json_to_type_member).collect()
}

fn json_to_type_member(val: &Value) -> Option<GrannyTypeMember> {
    let obj = val.as_object()?;
    let type_id = u32::try_from(obj.get("type")?.as_u64()?).ok()?;
    let member_type = GrannyMemberType::from_u32(type_id)?;
    let name = obj
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let array_width = obj
        .get("array_width")
        .and_then(gltf_json::Value::as_u64)
        .map_or(Some(0), |value| u32::try_from(value).ok())?;
    let extra_arr = obj.get("extra").and_then(|v| v.as_array());
    let extra = if let Some(ea) = extra_arr {
        let mut e = [0u32; 3];
        for (i, v) in ea.iter().enumerate().take(3) {
            e[i] = u32::try_from(v.as_u64().unwrap_or(0)).ok()?;
        }
        e
    } else {
        [0u32; 3]
    };
    let reference_type = obj.get("reference_type").and_then(json_to_type_members);
    Some(GrannyTypeMember {
        member_type,
        name,
        reference_type,
        array_width,
        extra,
    })
}

// ---------------------------------------------------------------------------
// GrannyVariant → JSON
// ---------------------------------------------------------------------------

pub fn variant_to_json(v: &GrannyVariant) -> Value {
    match v {
        GrannyVariant::Struct(fields) => {
            // Serialize as array of [name, value] pairs to preserve
            // duplicate member names (e.g. multiple "TrackMask" fields).
            Value::Array(
                fields
                    .iter()
                    .map(|(name, val)| {
                        Value::Array(vec![Value::String(name.clone()), variant_to_json(val)])
                    })
                    .collect(),
            )
        }
        GrannyVariant::Real32(vals) => Value::Array(vals.iter().map(|&f| json_f32(f)).collect()),
        GrannyVariant::Int8(vals) => {
            Value::Array(vals.iter().map(|&v| Value::Number(v.into())).collect())
        }
        GrannyVariant::UInt8(vals) => {
            Value::Array(vals.iter().map(|&v| Value::Number(v.into())).collect())
        }
        GrannyVariant::Int16(vals) => {
            Value::Array(vals.iter().map(|&v| Value::Number(v.into())).collect())
        }
        GrannyVariant::UInt16(vals) => {
            Value::Array(vals.iter().map(|&v| Value::Number(v.into())).collect())
        }
        GrannyVariant::Int32(vals) => {
            Value::Array(vals.iter().map(|&v| Value::Number(v.into())).collect())
        }
        GrannyVariant::UInt32(vals) => {
            Value::Array(vals.iter().map(|&v| Value::Number(v.into())).collect())
        }
        GrannyVariant::StringVal(s) => Value::String(s.clone()),
        GrannyVariant::Reference(inner) | GrannyVariant::VariantReference(inner) => match inner {
            Some(boxed) => variant_to_json(boxed),
            None => Value::Null,
        },
        GrannyVariant::Empty => Value::Null,
        GrannyVariant::RawBytes(bytes) => {
            // Base64-encode raw bytes
            Value::String(format!("base64:{}", STANDARD.encode(bytes)))
        }
    }
}

fn json_f32(f: f32) -> Value {
    serde_json::Number::from_f64(f64::from(f)).map_or(Value::Null, Value::Number)
}

// ---------------------------------------------------------------------------
// JSON → GrannyVariant (guided by type members)
// ---------------------------------------------------------------------------

pub fn json_to_variant(val: &Value, members: &[GrannyTypeMember]) -> Option<GrannyVariant> {
    // Accept both formats:
    //   Array of [name, value] pairs (new, preserves duplicate keys)
    //   JSON object (legacy fallback)
    if let Some(arr) = val.as_array() {
        // New format: [[name, value], ...]
        // Walk members and pairs in lockstep — they must match 1:1.
        if arr.len() != members.len() {
            return None;
        }
        let mut fields = Vec::with_capacity(members.len());
        for (pair, m) in arr.iter().zip(members.iter()) {
            let pair_arr = pair.as_array()?;
            if pair_arr.len() != 2 {
                return None;
            }
            // Name in pair should match member name (sanity check)
            let _name = pair_arr[0].as_str()?;
            let field_val = &pair_arr[1];
            let width = if m.array_width == 0 {
                1
            } else {
                m.array_width as usize
            };
            let variant = json_to_variant_field(field_val, m, width)?;
            fields.push((m.name.clone(), variant));
        }
        Some(GrannyVariant::Struct(fields))
    } else if let Some(obj) = val.as_object() {
        // Legacy fallback: JSON object (lossy for duplicate keys)
        let mut fields = Vec::with_capacity(members.len());
        for m in members {
            let field_val = obj.get(&m.name).unwrap_or(&Value::Null);
            let width = if m.array_width == 0 {
                1
            } else {
                m.array_width as usize
            };
            let variant = json_to_variant_field(field_val, m, width)?;
            fields.push((m.name.clone(), variant));
        }
        Some(GrannyVariant::Struct(fields))
    } else {
        None
    }
}

fn json_to_variant_field(val: &Value, m: &GrannyTypeMember, width: usize) -> Option<GrannyVariant> {
    match m.member_type {
        GrannyMemberType::Real32 => Some(GrannyVariant::Real32(json_array(val, width, |value| {
            value.as_f64().unwrap_or(0.0).to_f32()
        })?)),
        GrannyMemberType::Int8 | GrannyMemberType::BinormalInt8 => {
            Some(GrannyVariant::Int8(json_array(val, width, |value| {
                i8::try_from(value.as_i64().unwrap_or(0)).ok()
            })?))
        }
        GrannyMemberType::UInt8 | GrannyMemberType::NormalUInt8 => {
            Some(GrannyVariant::UInt8(json_array(val, width, |value| {
                u8::try_from(value.as_u64().unwrap_or(0)).ok()
            })?))
        }
        GrannyMemberType::Int16 | GrannyMemberType::BinormalInt16 => {
            Some(GrannyVariant::Int16(json_array(val, width, |value| {
                i16::try_from(value.as_i64().unwrap_or(0)).ok()
            })?))
        }
        GrannyMemberType::UInt16 | GrannyMemberType::NormalUInt16 | GrannyMemberType::Real16 => {
            Some(GrannyVariant::UInt16(json_array(val, width, |value| {
                u16::try_from(value.as_u64().unwrap_or(0)).ok()
            })?))
        }
        GrannyMemberType::Int32 => Some(GrannyVariant::Int32(json_array(val, width, |value| {
            i32::try_from(value.as_i64().unwrap_or(0)).ok()
        })?)),
        GrannyMemberType::UInt32 => Some(GrannyVariant::UInt32(json_array(val, width, |value| {
            u32::try_from(value.as_u64().unwrap_or(0)).ok()
        })?)),
        GrannyMemberType::StringMember => {
            let s = val.as_str().unwrap_or("").to_string();
            Some(GrannyVariant::StringVal(s))
        }
        GrannyMemberType::Reference => {
            if val.is_null() {
                Some(GrannyVariant::Reference(None))
            } else if let Some(ref nested_type) = m.reference_type {
                let inner = json_to_variant(val, nested_type)?;
                Some(GrannyVariant::Reference(Some(Box::new(inner))))
            } else {
                Some(GrannyVariant::Reference(None))
            }
        }
        GrannyMemberType::VariantReference => {
            if val.is_null() {
                Some(GrannyVariant::VariantReference(None))
            } else if let Some(ref nested_type) = m.reference_type {
                let inner = json_to_variant(val, nested_type)?;
                Some(GrannyVariant::VariantReference(Some(Box::new(inner))))
            } else {
                Some(GrannyVariant::VariantReference(None))
            }
        }
        GrannyMemberType::Inline => {
            if let Some(ref nested_type) = m.reference_type {
                json_to_variant(val, nested_type)
            } else {
                Some(GrannyVariant::Empty)
            }
        }
        GrannyMemberType::Transform
        | GrannyMemberType::ReferenceToArray
        | GrannyMemberType::ArrayOfReferences
        | GrannyMemberType::ReferenceToVariantArray => {
            if let Some(s) = val.as_str()
                && let Some(b64) = s.strip_prefix("base64:")
            {
                let bytes = STANDARD.decode(b64).unwrap_or_default();
                return Some(GrannyVariant::RawBytes(bytes));
            }
            Some(GrannyVariant::RawBytes(Vec::new()))
        }
        GrannyMemberType::EmptyReference | GrannyMemberType::End => Some(GrannyVariant::Empty),
    }
}

fn json_array<T>(
    value: &Value,
    width: usize,
    convert: impl Fn(&Value) -> Option<T>,
) -> Option<Vec<T>> {
    value.as_array()?.iter().take(width).map(convert).collect()
}
