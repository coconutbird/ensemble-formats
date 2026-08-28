//! Serde [`Deserializer`] for [`bdt::Node`] trees.
//!
//! # Field mapping
//!
//! | Serde field name | Maps to                        |
//! |------------------|--------------------------------|
//! | `@attr`          | XML attribute named `attr`     |
//! | `$text`          | Node's inner text content      |
//! | anything else    | Child element with that name   |

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use bdt::Node;
use bdt::variant::Variant;
use serde::de::{self, DeserializeSeed, IntoDeserializer, Visitor};

use crate::Error;
use crate::warn::Diagnostics;

// ---------------------------------------------------------------------------
// Top-level Deserializer
// ---------------------------------------------------------------------------

/// A serde [`serde::de::Deserializer`] backed by a [`bdt::Node`] reference.
pub struct NodeDeserializer<'a> {
    node: &'a Node,
    diag: Option<&'a Diagnostics>,
}

impl<'a> NodeDeserializer<'a> {
    #[must_use]
    pub fn new(node: &'a Node, diag: Option<&'a Diagnostics>) -> Self {
        Self { node, diag }
    }
}

impl<'de> de::Deserializer<'de> for NodeDeserializer<'_> {
    type Error = Error;

    fn deserialize_struct<V>(
        self,
        _name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_map(NodeMapAccess::new(self.node, fields, self.diag))
    }

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_some(self)
    }

    fn deserialize_string<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_string(self.node.text_string())
    }

    fn deserialize_str<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_string(self.node.text_string())
    }

    fn deserialize_f32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let v = self
            .node
            .text
            .as_float()
            .ok_or_else(|| Error::new("expected f32"))?;
        visitor.visit_f32(v)
    }

    fn deserialize_f64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let v = self
            .node
            .text
            .as_float()
            .ok_or_else(|| Error::new("expected f64"))?;
        visitor.visit_f64(f64::from(v))
    }

    fn deserialize_i32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let v = self
            .node
            .text
            .as_int()
            .ok_or_else(|| Error::new("expected i32"))?;
        visitor.visit_i32(v)
    }

    fn deserialize_u32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let v = self
            .node
            .text
            .as_int()
            .ok_or_else(|| Error::new("expected u32"))?;
        let value = u32::try_from(v).map_err(|_| Error::new("expected non-negative u32"))?;
        visitor.visit_u32(value)
    }

    fn deserialize_bool<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let v = self.node.text.as_bool().unwrap_or(true);
        visitor.visit_bool(v)
    }

    fn deserialize_seq<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_seq(ChildSeqAccess {
            children: &self.node.children,
            index: 0,
            diag: self.diag,
        })
    }

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_map(NodeMapAccess::new(self.node, &[], self.diag))
    }

    fn deserialize_ignored_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_unit()
    }

    serde::forward_to_deserialize_any! {
        i8 i16 i64 i128 u8 u16 u64 u128
        char bytes byte_buf
        unit unit_struct newtype_struct tuple tuple_struct
        map enum identifier
    }
}

// ---------------------------------------------------------------------------
// NodeMapAccess — struct field iteration
// ---------------------------------------------------------------------------

enum MapEntry<'a> {
    Attribute(&'a bdt::Attribute),
    Text(&'a Node),
    Children(Vec<&'a Node>),
}

struct NodeMapAccess<'a> {
    entries: Vec<(String, MapEntry<'a>)>,
    index: usize,
    element_name: &'a str,
    diag: Option<&'a Diagnostics>,
}

/// Find the canonical expected field name for `xml_key` by case-insensitive
/// comparison. Returns the expected name if found, otherwise the original key.
fn canonicalize(xml_key: &str, expected: &[&str]) -> String {
    for &e in expected {
        if e.eq_ignore_ascii_case(xml_key) {
            return String::from(e);
        }
    }
    String::from(xml_key)
}

impl<'a> NodeMapAccess<'a> {
    fn new(
        node: &'a Node,
        expected_fields: &'static [&'static str],
        diag: Option<&'a Diagnostics>,
    ) -> Self {
        let mut entries: Vec<(String, MapEntry<'a>)> = Vec::new();

        // Attributes → @name (case-insensitive match against expected fields)
        for attr in &node.attributes {
            let xml_key = format!("@{}", attr.name);
            let key = canonicalize(&xml_key, expected_fields);
            entries.push((key, MapEntry::Attribute(attr)));
        }

        // Text content → $text
        let text_str = node.text_string();
        if !text_str.is_empty() {
            entries.push((String::from("$text"), MapEntry::Text(node)));
        }

        // Children — group by canonical name (case-insensitive)
        let mut seen: Vec<(String, Vec<&Node>)> = Vec::new();
        for child in &node.children {
            let key = canonicalize(child.name.as_str(), expected_fields);
            if let Some(e) = seen.iter_mut().find(|(n, _)| *n == key) {
                e.1.push(child);
            } else {
                seen.push((key, alloc::vec![child]));
            }
        }

        for (name, nodes) in seen {
            entries.push((name, MapEntry::Children(nodes)));
        }

        Self {
            entries,
            index: 0,
            element_name: node.name.as_str(),
            diag,
        }
    }
}

impl<'de> de::MapAccess<'de> for NodeMapAccess<'_> {
    type Error = Error;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, Self::Error> {
        if self.index >= self.entries.len() {
            return Ok(None);
        }
        let key = self.entries[self.index].0.clone();
        seed.deserialize(key.into_deserializer()).map(Some)
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(
        &mut self,
        seed: V,
    ) -> Result<V::Value, Self::Error> {
        let (ref key, ref entry) = self.entries[self.index];
        let field_name = key.clone();
        let diag = self.diag;
        let element_name = self.element_name;
        self.index += 1;

        // Create a diagnostics-aware deserializer that records the field
        // name if serde calls `deserialize_ignored_any` on the value.
        let de = DiagValueDeserializer {
            field_name: field_name.clone(),
            element_name,
            entry,
            diag,
        };
        seed.deserialize(de).map_err(|e| {
            // Enrich the error with field context if not already present.
            let msg = e.to_string();
            if msg.contains("for field `") {
                e
            } else {
                Error::new(format!("{msg} (field `{field_name}` in <{element_name}>)"))
            }
        })
    }
}

/// A thin wrapper that intercepts `deserialize_ignored_any` to record
/// unmapped fields, then delegates everything else to the real value
/// deserializer.
struct DiagValueDeserializer<'a> {
    field_name: String,
    element_name: &'a str,
    entry: &'a MapEntry<'a>,
    diag: Option<&'a Diagnostics>,
}

impl<'de> de::Deserializer<'de> for DiagValueDeserializer<'_> {
    type Error = Error;

    fn deserialize_ignored_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        // This is called when serde doesn't have a struct field for this key.
        if let Some(diag) = self.diag {
            diag.record_extra(self.field_name, String::from(self.element_name));
        }
        visitor.visit_unit()
    }

    // All other methods delegate to the real value deserializer.
    // Typed methods use `try_typed` to record type-mismatch warnings on failure.
    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.dispatch().deserialize_any(visitor)
    }
    fn deserialize_bool<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.try_typed("bool", |d| d.deserialize_bool(visitor))
    }
    fn deserialize_i32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.try_typed("i32", |d| d.deserialize_i32(visitor))
    }
    fn deserialize_u32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.try_typed("u32", |d| d.deserialize_u32(visitor))
    }
    fn deserialize_f32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.try_typed("f32", |d| d.deserialize_f32(visitor))
    }
    fn deserialize_f64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.try_typed("f64", |d| d.deserialize_f64(visitor))
    }
    fn deserialize_str<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.dispatch().deserialize_str(visitor)
    }
    fn deserialize_string<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.dispatch().deserialize_string(visitor)
    }
    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.dispatch().deserialize_option(visitor)
    }
    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.dispatch().deserialize_seq(visitor)
    }
    fn deserialize_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.dispatch().deserialize_struct(name, fields, visitor)
    }

    serde::forward_to_deserialize_any! {
        i8 i16 i64 i128 u8 u16 u64 u128
        char bytes byte_buf unit unit_struct
        newtype_struct tuple tuple_struct map enum identifier
    }
}

impl<'a> DiagValueDeserializer<'a> {
    /// Dispatch to the real value deserializer based on the entry type.
    fn dispatch(&self) -> ChildrenOrVariantDeserializer<'a> {
        match self.entry {
            MapEntry::Attribute(attr) => {
                ChildrenOrVariantDeserializer::Variant(VariantDeserializer(&attr.value))
            }
            MapEntry::Text(node) => {
                ChildrenOrVariantDeserializer::Variant(VariantDeserializer(&node.text))
            }
            MapEntry::Children(children) => {
                ChildrenOrVariantDeserializer::Children(ChildrenDeserializer(children, self.diag))
            }
        }
    }

    /// Try a typed deserialization; on failure, record a type-mismatch warning
    /// (if diagnostics are enabled) and propagate the error.
    fn try_typed<V, F>(self, expected: &str, f: F) -> Result<V, Error>
    where
        F: FnOnce(ChildrenOrVariantDeserializer<'a>) -> Result<V, Error>,
    {
        // Capture diagnostic context before destructuring self.
        let actual_desc = match self.entry {
            MapEntry::Attribute(attr) => format!("{:?}", attr.value),
            MapEntry::Text(node) => format!("{:?}", node.text),
            MapEntry::Children(children) => format!("{} child element(s)", children.len()),
        };
        let diag = self.diag;
        let field_name = self.field_name;
        let element_name = String::from(self.element_name);
        let dispatched = match self.entry {
            MapEntry::Attribute(attr) => {
                ChildrenOrVariantDeserializer::Variant(VariantDeserializer(&attr.value))
            }
            MapEntry::Text(node) => {
                ChildrenOrVariantDeserializer::Variant(VariantDeserializer(&node.text))
            }
            MapEntry::Children(children) => {
                ChildrenOrVariantDeserializer::Children(ChildrenDeserializer(children, diag))
            }
        };
        let result = f(dispatched);
        if result.is_err() {
            if let Some(diag) = diag {
                diag.record_type_mismatch(
                    field_name.clone(),
                    element_name.clone(),
                    String::from(expected),
                    actual_desc.clone(),
                );
            }
            // Return an enriched error that includes field context.
            return Err(Error::new(format!(
                "expected {expected} for field `{field_name}` in <{element_name}>, got {actual_desc}"
            )));
        }
        result
    }
}

/// Helper enum so `DiagValueDeserializer::dispatch` can return either type.
enum ChildrenOrVariantDeserializer<'a> {
    Variant(VariantDeserializer<'a>),
    Children(ChildrenDeserializer<'a>),
}

impl<'de> de::Deserializer<'de> for ChildrenOrVariantDeserializer<'_> {
    type Error = Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        match self {
            Self::Variant(d) => de::Deserializer::deserialize_any(d, visitor),
            Self::Children(d) => de::Deserializer::deserialize_any(d, visitor),
        }
    }

    fn deserialize_bool<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        match self {
            Self::Variant(d) => de::Deserializer::deserialize_bool(d, visitor),
            Self::Children(d) => de::Deserializer::deserialize_bool(d, visitor),
        }
    }

    fn deserialize_i32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        match self {
            Self::Variant(d) => de::Deserializer::deserialize_i32(d, visitor),
            Self::Children(d) => de::Deserializer::deserialize_i32(d, visitor),
        }
    }

    fn deserialize_u32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        match self {
            Self::Variant(d) => de::Deserializer::deserialize_u32(d, visitor),
            Self::Children(d) => de::Deserializer::deserialize_u32(d, visitor),
        }
    }

    fn deserialize_f32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        match self {
            Self::Variant(d) => de::Deserializer::deserialize_f32(d, visitor),
            Self::Children(d) => de::Deserializer::deserialize_f32(d, visitor),
        }
    }

    fn deserialize_f64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        match self {
            Self::Variant(d) => de::Deserializer::deserialize_f64(d, visitor),
            Self::Children(d) => de::Deserializer::deserialize_f64(d, visitor),
        }
    }

    fn deserialize_str<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        match self {
            Self::Variant(d) => de::Deserializer::deserialize_str(d, visitor),
            Self::Children(d) => de::Deserializer::deserialize_str(d, visitor),
        }
    }

    fn deserialize_string<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        match self {
            Self::Variant(d) => de::Deserializer::deserialize_string(d, visitor),
            Self::Children(d) => de::Deserializer::deserialize_string(d, visitor),
        }
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        match self {
            Self::Variant(d) => de::Deserializer::deserialize_option(d, visitor),
            Self::Children(d) => de::Deserializer::deserialize_option(d, visitor),
        }
    }

    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        match self {
            Self::Variant(d) => de::Deserializer::deserialize_seq(d, visitor),
            Self::Children(d) => de::Deserializer::deserialize_seq(d, visitor),
        }
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        match self {
            Self::Variant(d) => de::Deserializer::deserialize_struct(d, name, fields, visitor),
            Self::Children(d) => de::Deserializer::deserialize_struct(d, name, fields, visitor),
        }
    }

    fn deserialize_ignored_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        match self {
            Self::Variant(d) => de::Deserializer::deserialize_ignored_any(d, visitor),
            Self::Children(d) => de::Deserializer::deserialize_ignored_any(d, visitor),
        }
    }

    serde::forward_to_deserialize_any! {
        i8 i16 i64 i128 u8 u16 u64 u128
        char bytes byte_buf unit unit_struct
        newtype_struct tuple tuple_struct map enum identifier
    }
}

// ---------------------------------------------------------------------------
// SeqAccess implementations
// ---------------------------------------------------------------------------

/// Iterates over owned `Vec<Node>` children.
struct ChildSeqAccess<'a> {
    children: &'a [Node],
    index: usize,
    diag: Option<&'a Diagnostics>,
}

impl<'de> de::SeqAccess<'de> for ChildSeqAccess<'_> {
    type Error = Error;

    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, Self::Error> {
        if self.index >= self.children.len() {
            return Ok(None);
        }
        let child = &self.children[self.index];
        self.index += 1;
        seed.deserialize(NodeDeserializer::new(child, self.diag))
            .map(Some)
    }
}

/// Wraps `&[&Node]` — dispatches to either single-element or sequence
/// depending on what serde requests.
///
/// - `deserialize_seq` → iterates **all** children (for `Vec<T>` fields)
/// - everything else → delegates to the **first** child as a `NodeDeserializer`
struct ChildrenDeserializer<'a>(&'a Vec<&'a Node>, Option<&'a Diagnostics>);

impl<'de> de::Deserializer<'de> for ChildrenDeserializer<'_> {
    type Error = Error;

    // Default: treat as a single element (first child).
    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        NodeDeserializer::new(self.0[0], self.1).deserialize_any(visitor)
    }

    // Vec<T> path: iterate all children.
    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_seq(RefSeqAccess {
            nodes: self.0,
            index: 0,
            diag: self.1,
        })
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        // If the single child element has Null text and no children/attributes
        // of its own, treat it as None (the element is present but empty).
        if self.0.len() == 1
            && matches!(self.0[0].text, Variant::Null)
            && self.0[0].children.is_empty()
            && self.0[0].attributes.is_empty()
        {
            return visitor.visit_none();
        }
        visitor.visit_some(self)
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        NodeDeserializer::new(self.0[0], self.1).deserialize_struct(name, fields, visitor)
    }

    fn deserialize_string<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        NodeDeserializer::new(self.0[0], self.1).deserialize_string(visitor)
    }

    fn deserialize_str<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        NodeDeserializer::new(self.0[0], self.1).deserialize_str(visitor)
    }

    fn deserialize_f32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        NodeDeserializer::new(self.0[0], self.1).deserialize_f32(visitor)
    }

    fn deserialize_f64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        NodeDeserializer::new(self.0[0], self.1).deserialize_f64(visitor)
    }

    fn deserialize_i32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        NodeDeserializer::new(self.0[0], self.1).deserialize_i32(visitor)
    }

    fn deserialize_u32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        NodeDeserializer::new(self.0[0], self.1).deserialize_u32(visitor)
    }

    fn deserialize_bool<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        NodeDeserializer::new(self.0[0], self.1).deserialize_bool(visitor)
    }

    fn deserialize_ignored_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_unit()
    }

    serde::forward_to_deserialize_any! {
        i8 i16 i64 i128 u8 u16 u64 u128
        char bytes byte_buf unit unit_struct
        newtype_struct tuple tuple_struct map enum identifier
    }
}

struct RefSeqAccess<'a> {
    nodes: &'a [&'a Node],
    index: usize,
    diag: Option<&'a Diagnostics>,
}

impl<'de> de::SeqAccess<'de> for RefSeqAccess<'_> {
    type Error = Error;

    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, Self::Error> {
        if self.index >= self.nodes.len() {
            return Ok(None);
        }
        let node = self.nodes[self.index];
        self.index += 1;
        seed.deserialize(NodeDeserializer::new(node, self.diag))
            .map(Some)
    }
}

// ---------------------------------------------------------------------------
// VariantDeserializer — deserializes a bdt::Variant as a primitive
// ---------------------------------------------------------------------------

/// Deserializes a [`bdt::variant::Variant`] value into serde primitives.
struct VariantDeserializer<'a>(&'a Variant);

impl<'de> de::Deserializer<'de> for VariantDeserializer<'_> {
    type Error = Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        match self.0 {
            Variant::Null => visitor.visit_unit(),
            Variant::Float(v) | Variant::Fract24(v) => visitor.visit_f32(*v),
            Variant::Double(v) => visitor.visit_f64(*v),
            Variant::Int(v) => visitor.visit_i32(*v),
            Variant::UInt(v) => visitor.visit_u32(*v),
            Variant::Bool(v) => visitor.visit_bool(*v),
            Variant::String(s) | Variant::UString(s) => visitor.visit_string(s.clone()),
            Variant::FloatVec(_) => visitor.visit_string(self.0.to_string_value()),
        }
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        match self.0 {
            Variant::Null => visitor.visit_none(),
            _ => visitor.visit_some(self),
        }
    }

    fn deserialize_string<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_string(self.0.to_string_value())
    }

    fn deserialize_str<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_string(self.0.to_string_value())
    }

    fn deserialize_f32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        let v = self
            .0
            .as_float()
            .ok_or_else(|| Error::new("expected f32"))?;
        visitor.visit_f32(v)
    }

    fn deserialize_f64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        let v = self
            .0
            .as_float()
            .ok_or_else(|| Error::new("expected f64"))?;
        visitor.visit_f64(f64::from(v))
    }

    fn deserialize_i32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        let v = self.0.as_int().ok_or_else(|| Error::new("expected i32"))?;
        visitor.visit_i32(v)
    }

    fn deserialize_u32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        let v = self.0.as_int().ok_or_else(|| Error::new("expected u32"))?;
        let value = u32::try_from(v).map_err(|_| Error::new("expected non-negative u32"))?;
        visitor.visit_u32(value)
    }

    fn deserialize_bool<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        let v = self
            .0
            .as_bool()
            .ok_or_else(|| Error::new("expected bool"))?;
        visitor.visit_bool(v)
    }

    fn deserialize_ignored_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_unit()
    }

    serde::forward_to_deserialize_any! {
        i8 i16 i64 i128 u8 u16 u64 u128
        char bytes byte_buf seq tuple tuple_struct
        map struct unit unit_struct newtype_struct enum identifier
    }
}
