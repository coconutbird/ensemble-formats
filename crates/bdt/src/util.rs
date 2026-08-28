//! Shared utility functions for tree assembly and string reading.

use alloc::string::String;
use alloc::vec::Vec;

use crate::node::Node;

/// Assemble a tree from flat nodes by cloning children into parents.
///
/// `child_indices[i]` lists the child node indices for node `i`.
/// Children are attached in reverse order so leaf nodes are fully built
/// before their parents clone them.
pub(crate) fn assemble_tree(
    mut nodes: Vec<Node>,
    child_indices: &[Vec<usize>],
    root: usize,
) -> Option<Node> {
    if nodes.is_empty() {
        return None;
    }

    for i in (0..nodes.len()).rev() {
        for &child_idx in &child_indices[i] {
            if child_idx < nodes.len() {
                let child = nodes[child_idx].clone();
                nodes[i].children.push(child);
            }
        }
    }

    Some(nodes[root].clone())
}

/// Read a null-terminated UTF-8 string from a byte slice at the given offset.
pub(crate) fn read_null_terminated_string(data: &[u8], offset: usize) -> String {
    if offset >= data.len() {
        return String::new();
    }
    let end = data[offset..]
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(data.len() - offset);
    String::from_utf8_lossy(&data[offset..offset + end]).into_owned()
}

/// Read a null-terminated UTF-16 string from a byte slice at the given offset.
pub(crate) fn read_null_terminated_wstring(data: &[u8], offset: usize, big_endian: bool) -> String {
    if offset >= data.len() {
        return String::new();
    }
    let mut chars = Vec::new();
    let mut i = offset;
    while i + 1 < data.len() {
        let c = if big_endian {
            u16::from_be_bytes([data[i], data[i + 1]])
        } else {
            u16::from_le_bytes([data[i], data[i + 1]])
        };
        if c == 0 {
            break;
        }
        chars.push(c);
        i += 2;
    }
    String::from_utf16_lossy(&chars)
}

/// Decode a direct-encoded string from the lower 24 bits of a variant value.
pub(crate) fn decode_direct_string(data_bits: u32) -> String {
    let mut bytes = Vec::new();
    let b0 = (data_bits & 0xFF) as u8;
    let b1 = ((data_bits >> 8) & 0xFF) as u8;
    let b2 = ((data_bits >> 16) & 0xFF) as u8;
    if b0 != 0 {
        bytes.push(b0);
    }
    if b1 != 0 {
        bytes.push(b1);
    }
    if b2 != 0 {
        bytes.push(b2);
    }
    String::from_utf8_lossy(&bytes).into_owned()
}
