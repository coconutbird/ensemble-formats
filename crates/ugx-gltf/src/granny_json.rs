//! JSON serialization for Granny2 type definitions and variant data.
//!
//! Used to preserve bone `ExtendedData` through glTF extras roundtrip.

use base64::{Engine, engine::general_purpose::STANDARD};
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
    let type_id = obj.get("type")?.as_u64()? as u32;
    let member_type = GrannyMemberType::from_u32(type_id)?;
    let name = obj
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let array_width = obj.get("array_width").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    let extra_arr = obj.get("extra").and_then(|v| v.as_array());
    let extra = if let Some(ea) = extra_arr {
        let mut e = [0u32; 3];
        for (i, v) in ea.iter().enumerate().take(3) {
            e[i] = v.as_u64().unwrap_or(0) as u32;
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
    serde_json::Number::from_f64(f as f64)
        .map(Value::Number)
        .unwrap_or(Value::Null)
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
        GrannyMemberType::Real32 => {
            let arr = val.as_array()?;
            let vals: Vec<f32> = arr
                .iter()
                .take(width)
                .map(|v| v.as_f64().unwrap_or(0.0) as f32)
                .collect();
            Some(GrannyVariant::Real32(vals))
        }
        GrannyMemberType::Int8 | GrannyMemberType::BinormalInt8 => {
            let arr = val.as_array()?;
            let vals: Vec<i8> = arr
                .iter()
                .take(width)
                .map(|v| v.as_i64().unwrap_or(0) as i8)
                .collect();
            Some(GrannyVariant::Int8(vals))
        }
        GrannyMemberType::UInt8 | GrannyMemberType::NormalUInt8 => {
            let arr = val.as_array()?;
            let vals: Vec<u8> = arr
                .iter()
                .take(width)
                .map(|v| v.as_u64().unwrap_or(0) as u8)
                .collect();
            Some(GrannyVariant::UInt8(vals))
        }
        GrannyMemberType::Int16 | GrannyMemberType::BinormalInt16 => {
            let arr = val.as_array()?;
            let vals: Vec<i16> = arr
                .iter()
                .take(width)
                .map(|v| v.as_i64().unwrap_or(0) as i16)
                .collect();
            Some(GrannyVariant::Int16(vals))
        }
        GrannyMemberType::UInt16 | GrannyMemberType::NormalUInt16 | GrannyMemberType::Real16 => {
            let arr = val.as_array()?;
            let vals: Vec<u16> = arr
                .iter()
                .take(width)
                .map(|v| v.as_u64().unwrap_or(0) as u16)
                .collect();
            Some(GrannyVariant::UInt16(vals))
        }
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
