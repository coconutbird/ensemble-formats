//! Serde [`Serializer`] for [`bdt::Node`] trees.
//!
//! Produces a [`bdt::Node`] tree from `#[derive(Serialize)]` structs using
//! the same conventions as the deserializer:
//!
//! | Serde field name | Maps to                        |
//! |------------------|--------------------------------|
//! | `@attr`          | XML attribute named `attr`     |
//! | `$text`          | Node's inner text content      |
//! | anything else    | Child element with that name   |

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use bdt::variant::Variant;
use bdt::{Attribute, Node};
use serde::ser::{self, Serialize};

use crate::Error;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Serialize a `T` into a [`bdt::Node`] with the given element name.
///
/// This is the inverse of [`crate::from_node`].
///
/// # Example
///
/// ```ignore
/// use serde::Serialize;
///
/// #[derive(Serialize)]
/// struct DamageType {
///     #[serde(rename = "$text")]
///     name: String,
///     #[serde(rename = "@AttackRating")]
///     attack_rating: Option<bool>,
/// }
///
/// let dt = DamageType { name: "Melee".into(), attack_rating: Some(true) };
/// let node = bdt_serde::to_node("DamageType", &dt).unwrap();
/// ```
pub fn to_node<T: Serialize>(name: &str, value: &T) -> Result<Node, Error> {
    value.serialize(NodeSerializer {
        name: String::from(name),
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Serialize a struct field and add it to the given node.
fn add_field_to_node<T: Serialize + ?Sized>(
    node: &mut Node,
    key: &str,
    value: &T,
) -> Result<(), Error> {
    if let Some(attr_name) = key.strip_prefix('@') {
        let variant = value.serialize(ToVariant)?;
        // Skip Null variants (None for Option fields)
        if !matches!(variant, Variant::Null) {
            node.add_attribute(Attribute::new(attr_name, variant));
        }
    } else if key == "$text" {
        node.text = value.serialize(ToVariant)?;
    } else {
        let children = value.serialize(ChildrenSerializer {
            name: String::from(key),
        })?;
        for child in children {
            node.add_child(child);
        }
    }
    Ok(())
}

/// Create a leaf node with a [`Variant`] text value.
fn leaf_node(name: String, text: Variant) -> Node {
    let mut node = Node::new(name);
    node.text = text;
    node
}

// ---------------------------------------------------------------------------
// ToVariant — serializes a value as a bdt::Variant
// ---------------------------------------------------------------------------

struct ToVariant;

impl ser::Serializer for ToVariant {
    type Ok = Variant;
    type Error = Error;
    type SerializeSeq = ser::Impossible<Variant, Error>;
    type SerializeTuple = ser::Impossible<Variant, Error>;
    type SerializeTupleStruct = ser::Impossible<Variant, Error>;
    type SerializeTupleVariant = ser::Impossible<Variant, Error>;
    type SerializeMap = ser::Impossible<Variant, Error>;
    type SerializeStruct = ser::Impossible<Variant, Error>;
    type SerializeStructVariant = ser::Impossible<Variant, Error>;

    fn serialize_bool(self, v: bool) -> Result<Variant, Error> {
        Ok(Variant::Bool(v))
    }
    fn serialize_i8(self, v: i8) -> Result<Variant, Error> {
        Ok(Variant::Int(i32::from(v)))
    }
    fn serialize_i16(self, v: i16) -> Result<Variant, Error> {
        Ok(Variant::Int(i32::from(v)))
    }
    fn serialize_i32(self, v: i32) -> Result<Variant, Error> {
        Ok(Variant::Int(v))
    }
    fn serialize_i64(self, v: i64) -> Result<Variant, Error> {
        let value = i32::try_from(v).map_err(|_| Error::new("i64 is outside BDT i32 range"))?;
        Ok(Variant::Int(value))
    }
    fn serialize_u8(self, v: u8) -> Result<Variant, Error> {
        Ok(Variant::UInt(u32::from(v)))
    }
    fn serialize_u16(self, v: u16) -> Result<Variant, Error> {
        Ok(Variant::UInt(u32::from(v)))
    }
    fn serialize_u32(self, v: u32) -> Result<Variant, Error> {
        Ok(Variant::UInt(v))
    }
    fn serialize_u64(self, v: u64) -> Result<Variant, Error> {
        let value = u32::try_from(v).map_err(|_| Error::new("u64 is outside BDT u32 range"))?;
        Ok(Variant::UInt(value))
    }
    fn serialize_f32(self, v: f32) -> Result<Variant, Error> {
        Ok(Variant::Float(v))
    }
    fn serialize_f64(self, v: f64) -> Result<Variant, Error> {
        Ok(Variant::Double(v))
    }
    fn serialize_char(self, v: char) -> Result<Variant, Error> {
        let mut buf = [0u8; 4];
        Ok(Variant::String(String::from(v.encode_utf8(&mut buf))))
    }
    fn serialize_str(self, v: &str) -> Result<Variant, Error> {
        Ok(Variant::String(String::from(v)))
    }
    fn serialize_bytes(self, _v: &[u8]) -> Result<Variant, Error> {
        Err(ser::Error::custom("cannot serialize bytes as Variant"))
    }
    fn serialize_none(self) -> Result<Variant, Error> {
        Ok(Variant::Null)
    }
    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<Variant, Error> {
        value.serialize(self)
    }
    fn serialize_unit(self) -> Result<Variant, Error> {
        Ok(Variant::Null)
    }
    fn serialize_unit_struct(self, _name: &'static str) -> Result<Variant, Error> {
        Ok(Variant::Null)
    }
    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _idx: u32,
        variant: &'static str,
    ) -> Result<Variant, Error> {
        Ok(Variant::String(String::from(variant)))
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<Variant, Error> {
        value.serialize(self)
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        _idx: u32,
        _variant: &'static str,
        _value: &T,
    ) -> Result<Variant, Error> {
        Err(ser::Error::custom(
            "cannot serialize newtype variant as Variant",
        ))
    }
    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq, Error> {
        Err(ser::Error::custom("cannot serialize sequence as Variant"))
    }
    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple, Error> {
        Err(ser::Error::custom("cannot serialize tuple as Variant"))
    }
    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleStruct, Error> {
        Err(ser::Error::custom(
            "cannot serialize tuple struct as Variant",
        ))
    }
    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _idx: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant, Error> {
        Err(ser::Error::custom(
            "cannot serialize tuple variant as Variant",
        ))
    }
    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap, Error> {
        Err(ser::Error::custom("cannot serialize map as Variant"))
    }
    fn serialize_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStruct, Error> {
        Err(ser::Error::custom("cannot serialize struct as Variant"))
    }
    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _idx: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant, Error> {
        Err(ser::Error::custom(
            "cannot serialize struct variant as Variant",
        ))
    }
}

// ---------------------------------------------------------------------------
// NodeSerializer — serializes a struct into a single bdt::Node
// ---------------------------------------------------------------------------

/// Serializer that produces a single [`Node`] with the given element name.
///
/// Only `serialize_struct` is meaningful here — the struct fields drive
/// attribute/text/child population via the internal `NodeStructSerializer`.
pub struct NodeSerializer {
    pub(crate) name: String,
}

impl ser::Serializer for NodeSerializer {
    type Ok = Node;
    type Error = Error;
    type SerializeSeq = ser::Impossible<Node, Error>;
    type SerializeTuple = ser::Impossible<Node, Error>;
    type SerializeTupleStruct = ser::Impossible<Node, Error>;
    type SerializeTupleVariant = ser::Impossible<Node, Error>;
    type SerializeMap = ser::Impossible<Node, Error>;
    type SerializeStruct = NodeStructSerializer;
    type SerializeStructVariant = ser::Impossible<Node, Error>;

    fn serialize_bool(self, v: bool) -> Result<Node, Error> {
        Ok(leaf_node(self.name, Variant::Bool(v)))
    }
    fn serialize_i8(self, v: i8) -> Result<Node, Error> {
        Ok(leaf_node(self.name, Variant::Int(i32::from(v))))
    }
    fn serialize_i16(self, v: i16) -> Result<Node, Error> {
        Ok(leaf_node(self.name, Variant::Int(i32::from(v))))
    }
    fn serialize_i32(self, v: i32) -> Result<Node, Error> {
        Ok(leaf_node(self.name, Variant::Int(v)))
    }
    fn serialize_i64(self, v: i64) -> Result<Node, Error> {
        let value = i32::try_from(v).map_err(|_| Error::new("i64 is outside BDT i32 range"))?;
        Ok(leaf_node(self.name, Variant::Int(value)))
    }
    fn serialize_u8(self, v: u8) -> Result<Node, Error> {
        Ok(leaf_node(self.name, Variant::UInt(u32::from(v))))
    }
    fn serialize_u16(self, v: u16) -> Result<Node, Error> {
        Ok(leaf_node(self.name, Variant::UInt(u32::from(v))))
    }
    fn serialize_u32(self, v: u32) -> Result<Node, Error> {
        Ok(leaf_node(self.name, Variant::UInt(v)))
    }
    fn serialize_u64(self, v: u64) -> Result<Node, Error> {
        let value = u32::try_from(v).map_err(|_| Error::new("u64 is outside BDT u32 range"))?;
        Ok(leaf_node(self.name, Variant::UInt(value)))
    }
    fn serialize_f32(self, v: f32) -> Result<Node, Error> {
        Ok(leaf_node(self.name, Variant::Float(v)))
    }
    fn serialize_f64(self, v: f64) -> Result<Node, Error> {
        Ok(leaf_node(self.name, Variant::Double(v)))
    }
    fn serialize_char(self, v: char) -> Result<Node, Error> {
        let mut buf = [0u8; 4];
        Ok(leaf_node(
            self.name,
            Variant::String(String::from(v.encode_utf8(&mut buf))),
        ))
    }
    fn serialize_str(self, v: &str) -> Result<Node, Error> {
        Ok(leaf_node(self.name, Variant::String(String::from(v))))
    }
    fn serialize_bytes(self, _v: &[u8]) -> Result<Node, Error> {
        Err(ser::Error::custom("cannot serialize bytes as Node"))
    }
    fn serialize_none(self) -> Result<Node, Error> {
        Ok(Node::new(self.name))
    }
    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<Node, Error> {
        value.serialize(self)
    }
    fn serialize_unit(self) -> Result<Node, Error> {
        Ok(Node::new(self.name))
    }
    fn serialize_unit_struct(self, _name: &'static str) -> Result<Node, Error> {
        Ok(Node::new(self.name))
    }
    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _idx: u32,
        variant: &'static str,
    ) -> Result<Node, Error> {
        Ok(leaf_node(self.name, Variant::String(String::from(variant))))
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<Node, Error> {
        value.serialize(self)
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        _idx: u32,
        _variant: &'static str,
        _value: &T,
    ) -> Result<Node, Error> {
        Err(ser::Error::custom(
            "cannot serialize newtype variant as Node",
        ))
    }
    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq, Error> {
        Err(ser::Error::custom("use ChildrenSerializer for sequences"))
    }
    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple, Error> {
        Err(ser::Error::custom("cannot serialize tuple as Node"))
    }
    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleStruct, Error> {
        Err(ser::Error::custom("cannot serialize tuple struct as Node"))
    }
    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _idx: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant, Error> {
        Err(ser::Error::custom("cannot serialize tuple variant as Node"))
    }
    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap, Error> {
        Err(ser::Error::custom("cannot serialize map as Node"))
    }
    fn serialize_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStruct, Error> {
        Ok(NodeStructSerializer {
            node: Node::new(self.name),
        })
    }
    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _idx: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant, Error> {
        Err(ser::Error::custom(
            "cannot serialize struct variant as Node",
        ))
    }
}

// ---------------------------------------------------------------------------
// NodeStructSerializer — serialize_struct state for NodeSerializer
// ---------------------------------------------------------------------------

/// Accumulates fields into a [`Node`] during struct serialization.
pub struct NodeStructSerializer {
    node: Node,
}

impl ser::SerializeStruct for NodeStructSerializer {
    type Ok = Node;
    type Error = Error;

    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        add_field_to_node(&mut self.node, key, value)
    }

    fn end(self) -> Result<Node, Error> {
        Ok(self.node)
    }
}

// ---------------------------------------------------------------------------
// ChildrenSerializer — serializes a value into Vec<Node>
// ---------------------------------------------------------------------------

/// Serializer that produces a `Vec<Node>` from a value.
///
/// - A **sequence** (e.g. `Vec<T>`) produces one `Node` per element, all
///   sharing the same element name.
/// - A **struct** produces a single `Node` (wrapped in a one-element vec).
/// - A **primitive** produces a single leaf node.
/// - `None` / unit produces an empty vec (the child is omitted).
struct ChildrenSerializer {
    name: String,
}

impl ser::Serializer for ChildrenSerializer {
    type Ok = Vec<Node>;
    type Error = Error;
    type SerializeSeq = ChildrenSeqSerializer;
    type SerializeTuple = ser::Impossible<Vec<Node>, Error>;
    type SerializeTupleStruct = ser::Impossible<Vec<Node>, Error>;
    type SerializeTupleVariant = ser::Impossible<Vec<Node>, Error>;
    type SerializeMap = ser::Impossible<Vec<Node>, Error>;
    type SerializeStruct = ChildrenStructSerializer;
    type SerializeStructVariant = ser::Impossible<Vec<Node>, Error>;

    // Primitives → single leaf node in a vec
    fn serialize_bool(self, v: bool) -> Result<Vec<Node>, Error> {
        Ok(vec![leaf_node(self.name, Variant::Bool(v))])
    }
    fn serialize_i8(self, v: i8) -> Result<Vec<Node>, Error> {
        Ok(vec![leaf_node(self.name, Variant::Int(i32::from(v)))])
    }
    fn serialize_i16(self, v: i16) -> Result<Vec<Node>, Error> {
        Ok(vec![leaf_node(self.name, Variant::Int(i32::from(v)))])
    }
    fn serialize_i32(self, v: i32) -> Result<Vec<Node>, Error> {
        Ok(vec![leaf_node(self.name, Variant::Int(v))])
    }
    fn serialize_i64(self, v: i64) -> Result<Vec<Node>, Error> {
        let value = i32::try_from(v).map_err(|_| Error::new("i64 is outside BDT i32 range"))?;
        Ok(vec![leaf_node(self.name, Variant::Int(value))])
    }
    fn serialize_u8(self, v: u8) -> Result<Vec<Node>, Error> {
        Ok(vec![leaf_node(self.name, Variant::UInt(u32::from(v)))])
    }
    fn serialize_u16(self, v: u16) -> Result<Vec<Node>, Error> {
        Ok(vec![leaf_node(self.name, Variant::UInt(u32::from(v)))])
    }
    fn serialize_u32(self, v: u32) -> Result<Vec<Node>, Error> {
        Ok(vec![leaf_node(self.name, Variant::UInt(v))])
    }
    fn serialize_u64(self, v: u64) -> Result<Vec<Node>, Error> {
        let value = u32::try_from(v).map_err(|_| Error::new("u64 is outside BDT u32 range"))?;
        Ok(vec![leaf_node(self.name, Variant::UInt(value))])
    }
    fn serialize_f32(self, v: f32) -> Result<Vec<Node>, Error> {
        Ok(vec![leaf_node(self.name, Variant::Float(v))])
    }
    fn serialize_f64(self, v: f64) -> Result<Vec<Node>, Error> {
        Ok(vec![leaf_node(self.name, Variant::Double(v))])
    }
    fn serialize_char(self, v: char) -> Result<Vec<Node>, Error> {
        let mut buf = [0u8; 4];
        Ok(vec![leaf_node(
            self.name,
            Variant::String(String::from(v.encode_utf8(&mut buf))),
        )])
    }
    fn serialize_str(self, v: &str) -> Result<Vec<Node>, Error> {
        Ok(vec![leaf_node(self.name, Variant::String(String::from(v)))])
    }
    fn serialize_bytes(self, _v: &[u8]) -> Result<Vec<Node>, Error> {
        Err(ser::Error::custom("cannot serialize bytes as child nodes"))
    }
    fn serialize_none(self) -> Result<Vec<Node>, Error> {
        Ok(Vec::new())
    }
    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<Vec<Node>, Error> {
        value.serialize(self)
    }
    fn serialize_unit(self) -> Result<Vec<Node>, Error> {
        Ok(Vec::new())
    }
    fn serialize_unit_struct(self, _name: &'static str) -> Result<Vec<Node>, Error> {
        Ok(Vec::new())
    }
    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _idx: u32,
        variant: &'static str,
    ) -> Result<Vec<Node>, Error> {
        Ok(vec![leaf_node(
            self.name,
            Variant::String(String::from(variant)),
        )])
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<Vec<Node>, Error> {
        value.serialize(self)
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        _idx: u32,
        _variant: &'static str,
        _value: &T,
    ) -> Result<Vec<Node>, Error> {
        Err(ser::Error::custom(
            "cannot serialize newtype variant as children",
        ))
    }
    fn serialize_seq(self, len: Option<usize>) -> Result<Self::SerializeSeq, Error> {
        Ok(ChildrenSeqSerializer {
            name: self.name,
            nodes: Vec::with_capacity(len.unwrap_or(0)),
        })
    }
    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple, Error> {
        Err(ser::Error::custom("cannot serialize tuple as children"))
    }
    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleStruct, Error> {
        Err(ser::Error::custom(
            "cannot serialize tuple struct as children",
        ))
    }
    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _idx: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant, Error> {
        Err(ser::Error::custom(
            "cannot serialize tuple variant as children",
        ))
    }
    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap, Error> {
        Err(ser::Error::custom("cannot serialize map as children"))
    }
    fn serialize_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStruct, Error> {
        Ok(ChildrenStructSerializer {
            node: Node::new(self.name),
        })
    }
    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _idx: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant, Error> {
        Err(ser::Error::custom(
            "cannot serialize struct variant as children",
        ))
    }
}

// ---------------------------------------------------------------------------
// ChildrenSeqSerializer — serialize_seq state for ChildrenSerializer
// ---------------------------------------------------------------------------

/// Accumulates sequence elements into `Vec<Node>`.
struct ChildrenSeqSerializer {
    name: String,
    nodes: Vec<Node>,
}

impl ser::SerializeSeq for ChildrenSeqSerializer {
    type Ok = Vec<Node>;
    type Error = Error;

    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        let node = value.serialize(NodeSerializer {
            name: self.name.clone(),
        })?;
        self.nodes.push(node);
        Ok(())
    }

    fn end(self) -> Result<Vec<Node>, Error> {
        Ok(self.nodes)
    }
}

// ---------------------------------------------------------------------------
// ChildrenStructSerializer — serialize_struct state for ChildrenSerializer
// ---------------------------------------------------------------------------

/// Wraps a single struct into a one-element `Vec<Node>`.
struct ChildrenStructSerializer {
    node: Node,
}

impl ser::SerializeStruct for ChildrenStructSerializer {
    type Ok = Vec<Node>;
    type Error = Error;

    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        add_field_to_node(&mut self.node, key, value)
    }

    fn end(self) -> Result<Vec<Node>, Error> {
        Ok(vec![self.node])
    }
}
