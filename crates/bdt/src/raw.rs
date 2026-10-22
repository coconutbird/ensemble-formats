//! Zero-copy binary structures for `BBinaryDataTree` packed formats.
//!
//! These structs map directly to the on-disk binary layout and can be
//! interpreted in-place from a byte slice via [`zerocopy::Ref`].
//!
//! Two format families are defined here:
//!
//! - **XMX** (variant format): used by XMB files. Two endianness variants:
//!   - [`XmxHeaderLe`] / [`XmxNodeLe`] — PC / Definitive Edition (64-bit pointers, 48-byte nodes)
//!   - [`XmxHeaderBe`] / [`XmxNodeBe`] — Xbox 360 (32-bit pointers, 28-byte nodes)
//!
//! - **Compact** (`BPackedHeader` format): used by material chunks and other
//!   non-XMB packed data.
//!   - [`BPackedHeader`] — 28-byte header with section sizes
//!   - [`CompactNodeRaw`] — 8-byte node with 16-bit indices
//!   - [`CompactNvRaw`] — 8-byte name-value pair with type flags

use zerocopy::{FromBytes, Immutable, KnownLayout};

/// XMX header for PC/LE format (36 bytes).
///
/// Layout: `pad(4) + nodes BPackedArray(16) + variant BPackedArray(16)`
#[derive(FromBytes, KnownLayout, Immutable, Debug)]
#[repr(C)]
pub struct XmxHeaderLe {
    /// Unused padding bytes.
    pub padding: [u8; 4],
    /// Number of nodes, as a little-endian `u32`.
    pub nodes_size: [u8; 4],
    /// Padding after `nodes_size`, as a little-endian `u32`.
    pub nodes_pad: [u8; 4],
    /// Absolute byte offset of the node array, as a little-endian `u64`.
    pub nodes_ptr: [u8; 8],
    /// Size of the variant data table in bytes, as a little-endian `u32`.
    pub variant_size: [u8; 4],
    /// Padding after `variant_size`, as a little-endian `u32`.
    pub variant_pad: [u8; 4],
    /// Absolute byte offset of the variant data table, as a little-endian `u64`.
    pub variant_ptr: [u8; 8],
}

impl XmxHeaderLe {
    /// Number of nodes in the tree.
    #[must_use]
    pub fn nodes_size(&self) -> u32 {
        u32::from_le_bytes(self.nodes_size)
    }

    /// Absolute byte offset to the node array.
    #[must_use]
    pub fn nodes_ptr(&self) -> Option<usize> {
        usize::try_from(u64::from_le_bytes(self.nodes_ptr)).ok()
    }

    /// Size of the variant data table in bytes.
    #[must_use]
    pub fn variant_size(&self) -> u32 {
        u32::from_le_bytes(self.variant_size)
    }

    /// Absolute byte offset to the variant data table.
    #[must_use]
    pub fn variant_ptr(&self) -> Option<usize> {
        usize::try_from(u64::from_le_bytes(self.variant_ptr)).ok()
    }
}

/// XMX node for PC/LE format (48 bytes).
#[derive(FromBytes, KnownLayout, Immutable, Debug)]
#[repr(C)]
pub struct XmxNodeLe {
    /// Index of the parent node, as a little-endian `u32`.
    pub parent_node: [u8; 4],
    /// Variant value of the node name, as a little-endian `u32`.
    pub name_variant: [u8; 4],
    /// Variant value of the node text, as a little-endian `u32`.
    pub text_variant: [u8; 4],
    /// Unused padding bytes.
    pub padding: [u8; 4],
    /// Number of attributes, as a little-endian `u32`.
    pub attrs_size: [u8; 4],
    /// Padding after `attrs_size`.
    pub attrs_pad: [u8; 4],
    /// Absolute byte offset of the attribute array, as a little-endian `u64`.
    pub attrs_ptr: [u8; 8],
    /// Number of child nodes, as a little-endian `u32`.
    pub children_size: [u8; 4],
    /// Padding after `children_size`.
    pub children_pad: [u8; 4],
    /// Absolute byte offset of the child index array, as a little-endian `u64`.
    pub children_ptr: [u8; 8],
}

impl XmxNodeLe {
    /// Returns the index of the parent node.
    #[must_use]
    pub fn parent_node(&self) -> u32 {
        u32::from_le_bytes(self.parent_node)
    }

    /// Returns the variant value of the node name.
    #[must_use]
    pub fn name_variant(&self) -> u32 {
        u32::from_le_bytes(self.name_variant)
    }

    /// Returns the variant value of the node text.
    #[must_use]
    pub fn text_variant(&self) -> u32 {
        u32::from_le_bytes(self.text_variant)
    }

    /// Returns the number of attributes.
    #[must_use]
    pub fn attrs_size(&self) -> u32 {
        u32::from_le_bytes(self.attrs_size)
    }

    /// Returns the absolute byte offset of the attribute array.
    #[must_use]
    pub fn attrs_ptr(&self) -> Option<usize> {
        usize::try_from(u64::from_le_bytes(self.attrs_ptr)).ok()
    }

    /// Returns the number of child nodes.
    #[must_use]
    pub fn children_size(&self) -> u32 {
        u32::from_le_bytes(self.children_size)
    }

    /// Returns the absolute byte offset of the child index array.
    #[must_use]
    pub fn children_ptr(&self) -> Option<usize> {
        usize::try_from(u64::from_le_bytes(self.children_ptr)).ok()
    }
}

/// XMX header for Xbox 360/BE format (16 bytes).
#[derive(FromBytes, KnownLayout, Immutable, Debug)]
#[repr(C)]
pub struct XmxHeaderBe {
    /// Number of nodes, as a big-endian `u32`.
    pub nodes_size: [u8; 4],
    /// Absolute byte offset of the node array, as a big-endian `u32`.
    pub nodes_ptr: [u8; 4],
    /// Size of the variant data table in bytes, as a big-endian `u32`.
    pub variant_size: [u8; 4],
    /// Absolute byte offset of the variant data table, as a big-endian `u32`.
    pub variant_ptr: [u8; 4],
}

impl XmxHeaderBe {
    /// Number of nodes in the tree.
    #[must_use]
    pub fn nodes_size(&self) -> u32 {
        u32::from_be_bytes(self.nodes_size)
    }

    /// Absolute byte offset to the node array.
    #[must_use]
    pub fn nodes_ptr(&self) -> usize {
        u32::from_be_bytes(self.nodes_ptr) as usize
    }

    /// Size of the variant data table in bytes.
    #[must_use]
    pub fn variant_size(&self) -> u32 {
        u32::from_be_bytes(self.variant_size)
    }

    /// Absolute byte offset to the variant data table.
    #[must_use]
    pub fn variant_ptr(&self) -> usize {
        u32::from_be_bytes(self.variant_ptr) as usize
    }
}

/// XMX node for Xbox 360/BE format (28 bytes).
#[derive(FromBytes, KnownLayout, Immutable, Debug)]
#[repr(C)]
pub struct XmxNodeBe {
    /// Index of the parent node, as a big-endian `u32`.
    pub parent_node: [u8; 4],
    /// Variant value of the node name, as a big-endian `u32`.
    pub name_variant: [u8; 4],
    /// Variant value of the node text, as a big-endian `u32`.
    pub text_variant: [u8; 4],
    /// Number of attributes, as a big-endian `u32`.
    pub attrs_size: [u8; 4],
    /// Absolute byte offset of the attribute array, as a big-endian `u32`.
    pub attrs_ptr: [u8; 4],
    /// Number of child nodes, as a big-endian `u32`.
    pub children_size: [u8; 4],
    /// Absolute byte offset of the child index array, as a big-endian `u32`.
    pub children_ptr: [u8; 4],
}

impl XmxNodeBe {
    /// Returns the index of the parent node.
    #[must_use]
    pub fn parent_node(&self) -> u32 {
        u32::from_be_bytes(self.parent_node)
    }

    /// Returns the variant value of the node name.
    #[must_use]
    pub fn name_variant(&self) -> u32 {
        u32::from_be_bytes(self.name_variant)
    }

    /// Returns the variant value of the node text.
    #[must_use]
    pub fn text_variant(&self) -> u32 {
        u32::from_be_bytes(self.text_variant)
    }

    /// Returns the number of attributes.
    #[must_use]
    pub fn attrs_size(&self) -> u32 {
        u32::from_be_bytes(self.attrs_size)
    }

    /// Returns the absolute byte offset of the attribute array.
    #[must_use]
    pub fn attrs_ptr(&self) -> usize {
        u32::from_be_bytes(self.attrs_ptr) as usize
    }

    /// Returns the number of child nodes.
    #[must_use]
    pub fn children_size(&self) -> u32 {
        u32::from_be_bytes(self.children_size)
    }

    /// Returns the absolute byte offset of the child index array.
    #[must_use]
    pub fn children_ptr(&self) -> usize {
        u32::from_be_bytes(self.children_ptr) as usize
    }
}

/// Attribute name/value pair (8 bytes). Endianness handled by caller.
#[derive(FromBytes, KnownLayout, Immutable, Debug, Clone, Copy)]
#[repr(C)]
pub struct AttrPairRaw {
    /// Variant value of the attribute name.
    pub name_var: [u8; 4],
    /// Variant value of the attribute value.
    pub value_var: [u8; 4],
}

/// `BPackedHeader` (28 bytes). First 4 bytes are single-byte fields.
#[derive(FromBytes, KnownLayout, Immutable, Debug)]
#[repr(C)]
pub struct BPackedHeader {
    /// Format signature byte.
    pub signature: u8,
    /// Format version.
    pub version: u8,
    /// Header flags.
    pub flags: u8,
    /// Number of user sections.
    pub num_user_sections: u8,
    /// CRC of the data, as a `u32`.
    pub crc: [u8; 4],
    /// Size of the data in bytes, as a `u32`.
    pub data_size: [u8; 4],
    /// Size of the node section in bytes, as a `u32`.
    pub node_section_size: [u8; 4],
    /// Size of the name-value section in bytes, as a `u32`.
    pub nv_section_size: [u8; 4],
    /// Size of the name data section in bytes, as a `u32`.
    pub name_data_size: [u8; 4],
    /// Size of the value data section in bytes, as a `u32`.
    pub value_data_size: [u8; 4],
}

impl BPackedHeader {
    /// Read a u32 field respecting the given endianness.
    fn read_u32(bytes: [u8; 4], big_endian: bool) -> u32 {
        if big_endian {
            u32::from_be_bytes(bytes)
        } else {
            u32::from_le_bytes(bytes)
        }
    }

    /// Size of the node section in bytes (each node is 8 bytes).
    #[must_use]
    pub fn node_section_size(&self, big_endian: bool) -> usize {
        Self::read_u32(self.node_section_size, big_endian) as usize
    }

    /// Size of the name-value section in bytes (each entry is 8 bytes).
    #[must_use]
    pub fn nv_section_size(&self, big_endian: bool) -> usize {
        Self::read_u32(self.nv_section_size, big_endian) as usize
    }

    /// Size of the name data section (null-terminated strings).
    #[must_use]
    pub fn name_data_size(&self, big_endian: bool) -> usize {
        Self::read_u32(self.name_data_size, big_endian) as usize
    }

    /// Size of the value data section (typed values, 16-byte aligned).
    #[must_use]
    pub fn value_data_size(&self, big_endian: bool) -> usize {
        Self::read_u32(self.value_data_size, big_endian) as usize
    }
}

/// Compact packed node (8 bytes). Endianness handled by caller for u16 fields.
#[derive(FromBytes, KnownLayout, Immutable, Debug, Clone, Copy)]
#[repr(C)]
pub struct CompactNodeRaw {
    /// Index of the parent node, as a `u16` (`0xFFFF` for the root).
    pub parent_index: [u8; 2],
    /// Index of the first child node, as a `u16`.
    pub child_node_index: [u8; 2],
    /// Index of the first name-value entry, as a `u16`.
    pub name_value_ofs: [u8; 2],
    /// Number of name-value entries of the node.
    pub num_name_values: u8,
    /// Number of child nodes.
    pub num_children: u8,
}

impl CompactNodeRaw {
    /// Index of this node's parent (`0xFFFF` for root).
    #[must_use]
    pub fn parent_index(&self, big_endian: bool) -> u16 {
        if big_endian {
            u16::from_be_bytes(self.parent_index)
        } else {
            u16::from_le_bytes(self.parent_index)
        }
    }

    /// Index of this node's first child in the node array.
    #[must_use]
    pub fn child_node_index(&self, big_endian: bool) -> u16 {
        if big_endian {
            u16::from_be_bytes(self.child_node_index)
        } else {
            u16::from_le_bytes(self.child_node_index)
        }
    }

    /// Offset into the name-value array for this node's first entry.
    #[must_use]
    pub fn name_value_ofs(&self, big_endian: bool) -> u16 {
        if big_endian {
            u16::from_be_bytes(self.name_value_ofs)
        } else {
            u16::from_le_bytes(self.name_value_ofs)
        }
    }
}

/// Compact packed name-value (8 bytes). Endianness handled by caller.
#[derive(FromBytes, KnownLayout, Immutable, Debug, Clone, Copy)]
#[repr(C)]
pub struct CompactNvRaw {
    /// Raw value, as a `u32`. The type flags set its meaning.
    pub value: [u8; 4],
    /// Offset of the name string in the name data section, as a `u16`.
    pub name_ofs: [u8; 2],
    /// Type and encoding flags, as a `u16`.
    pub flags: [u8; 2],
}

impl CompactNvRaw {
    /// Raw value field (interpretation depends on type flags).
    #[must_use]
    pub fn value(&self, big_endian: bool) -> u32 {
        if big_endian {
            u32::from_be_bytes(self.value)
        } else {
            u32::from_le_bytes(self.value)
        }
    }

    /// Offset into the name data section for this entry's name string.
    #[must_use]
    pub fn name_ofs(&self, big_endian: bool) -> u16 {
        if big_endian {
            u16::from_be_bytes(self.name_ofs)
        } else {
            u16::from_le_bytes(self.name_ofs)
        }
    }

    /// Type and encoding flags; `compact::nv_flags` defines the internal bit layout.
    #[must_use]
    pub fn flags(&self, big_endian: bool) -> u16 {
        if big_endian {
            u16::from_be_bytes(self.flags)
        } else {
            u16::from_le_bytes(self.flags)
        }
    }
}
