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
use alloc::string::String;
use alloc::vec::Vec;

use bdt::Node;
use bdt::variant::Variant;
use serde::de::{self, DeserializeSeed, IntoDeserializer, Visitor};

use crate::Error;

// ---------------------------------------------------------------------------
// Top-level Deserializer
// ---------------------------------------------------------------------------

/// A serde [`Deserializer`] backed by a [`bdt::Node`] reference.
pub struct NodeDeserializer<'a> {
    node: &'a Node,
}

impl<'a> NodeDeserializer<'a> {
    pub fn new(node: &'a Node) -> Self {
        Self { node }
    }
}

impl<'a, 'de> de::Deserializer<'de> for NodeDeserializer<'a> {
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
        visitor.visit_map(NodeMapAccess::new(self.node, fields))
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
        visitor.visit_f64(v as f64)
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
        visitor.visit_u32(v as u32)
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
        })
    }

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_map(NodeMapAccess::new(self.node, &[]))
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
    Child(&'a Node),
    Children(Vec<&'a Node>),
}

struct NodeMapAccess<'a> {
    entries: Vec<(String, MapEntry<'a>)>,
    index: usize,
}

impl<'a> NodeMapAccess<'a> {
    fn new(node: &'a Node, _expected_fields: &'static [&'static str]) -> Self {
        let mut entries: Vec<(String, MapEntry<'a>)> = Vec::new();

        // Attributes → @name
        for attr in &node.attributes {
            entries.push((format!("@{}", attr.name), MapEntry::Attribute(attr)));
        }

        // Text content → $text
        let text_str = node.text_string();
        if !text_str.is_empty() {
            entries.push((String::from("$text"), MapEntry::Text(node)));
        }

        // Children — group by name
        let mut seen: Vec<(&str, Vec<&Node>)> = Vec::new();
        for child in &node.children {
            if let Some(e) = seen.iter_mut().find(|(n, _)| *n == child.name.as_str()) {
                e.1.push(child);
            } else {
                seen.push((child.name.as_str(), alloc::vec![child]));
            }
        }
        for (name, nodes) in seen {
            let key = String::from(name);
            if nodes.len() == 1 {
                entries.push((key, MapEntry::Child(nodes[0])));
            } else {
                entries.push((key, MapEntry::Children(nodes)));
            }
        }

        Self { entries, index: 0 }
    }
}

impl<'a, 'de> de::MapAccess<'de> for NodeMapAccess<'a> {
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
        let (_, ref entry) = self.entries[self.index];
        self.index += 1;
        match entry {
            MapEntry::Attribute(attr) => seed.deserialize(VariantDeserializer(&attr.value)),
            MapEntry::Text(node) => seed.deserialize(VariantDeserializer(&node.text)),
            MapEntry::Child(child) => seed.deserialize(NodeDeserializer::new(child)),
            MapEntry::Children(children) => seed.deserialize(ChildrenSeqDeserializer(children)),
        }
    }
}

// ---------------------------------------------------------------------------
// SeqAccess implementations
// ---------------------------------------------------------------------------

/// Iterates over owned `Vec<Node>` children.
struct ChildSeqAccess<'a> {
    children: &'a [Node],
    index: usize,
}

impl<'a, 'de> de::SeqAccess<'de> for ChildSeqAccess<'a> {
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
        seed.deserialize(NodeDeserializer::new(child)).map(Some)
    }
}

/// Wraps `&[&Node]` as a seq Deserializer (for grouped children).
struct ChildrenSeqDeserializer<'a>(&'a Vec<&'a Node>);

impl<'a, 'de> de::Deserializer<'de> for ChildrenSeqDeserializer<'a> {
    type Error = Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.deserialize_seq(visitor)
    }

    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_seq(RefSeqAccess {
            nodes: self.0,
            index: 0,
        })
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64
        char str string bytes byte_buf option unit unit_struct
        newtype_struct tuple tuple_struct map struct enum identifier
        ignored_any
    }
}

struct RefSeqAccess<'a> {
    nodes: &'a [&'a Node],
    index: usize,
}

impl<'a, 'de> de::SeqAccess<'de> for RefSeqAccess<'a> {
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
        seed.deserialize(NodeDeserializer::new(node)).map(Some)
    }
}

// ---------------------------------------------------------------------------
// VariantDeserializer — deserializes a bdt::Variant as a primitive
// ---------------------------------------------------------------------------

/// Deserializes a [`bdt::variant::Variant`] value into serde primitives.
struct VariantDeserializer<'a>(&'a Variant);

impl<'a, 'de> de::Deserializer<'de> for VariantDeserializer<'a> {
    type Error = Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        match self.0 {
            Variant::Null => visitor.visit_unit(),
            Variant::Float(v) => visitor.visit_f32(*v),
            Variant::Double(v) => visitor.visit_f64(*v),
            Variant::Int(v) => visitor.visit_i32(*v),
            Variant::UInt(v) => visitor.visit_u32(*v),
            Variant::Bool(v) => visitor.visit_bool(*v),
            Variant::String(s) => visitor.visit_string(s.clone()),
            Variant::UString(s) => visitor.visit_string(s.clone()),
            Variant::FloatVec(_) => visitor.visit_string(self.0.to_string_value()),
            Variant::Fract24(v) => visitor.visit_f32(*v),
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
        visitor.visit_f64(v as f64)
    }

    fn deserialize_i32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        let v = self.0.as_int().ok_or_else(|| Error::new("expected i32"))?;
        visitor.visit_i32(v)
    }

    fn deserialize_u32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        let v = self.0.as_int().ok_or_else(|| Error::new("expected u32"))?;
        visitor.visit_u32(v as u32)
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
