//! Shared utility functions for tree assembly and string reading.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::node::Node;

/// Assemble a tree from flat nodes by attaching children to their parents.
///
/// `child_indices[i]` lists the child node indices for node `i`.
/// Children are attached in reverse order so leaf nodes are fully built
/// before their parents take them. A parent moves the finished subtree of its
/// child at the last reference to the child. All other references get a copy of
/// the child at that point, so the result is the same as when each reference
/// copies the child.
pub(crate) fn assemble_tree(
    mut nodes: Vec<Node>,
    child_indices: &[Vec<usize>],
    root: usize,
) -> Option<Node> {
    if nodes.is_empty() {
        return None;
    }

    let mut references = vec![0_usize; nodes.len()];
    for &child_idx in child_indices.iter().flatten() {
        if let Some(count) = references.get_mut(child_idx) {
            *count += 1;
        }
    }

    for i in (0..nodes.len()).rev() {
        for &child_idx in &child_indices[i] {
            if child_idx < nodes.len() {
                references[child_idx] -= 1;
                // A child with a higher index is finished. Only the root and
                // children with later references must stay in place.
                let child = if child_idx > i && child_idx != root && references[child_idx] == 0 {
                    core::mem::take(&mut nodes[child_idx])
                } else {
                    nodes[child_idx].clone()
                };
                nodes[i].children.push(child);
            }
        }
    }

    (root < nodes.len()).then(|| nodes.swap_remove(root))
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

#[cfg(test)]
mod tests {
    use alloc::format;
    use alloc::vec;
    use alloc::vec::Vec;

    use super::assemble_tree;
    use crate::node::Node;

    /// Assemble the tree the slow way: each reference copies the child.
    fn assemble_by_copy(mut nodes: Vec<Node>, child_indices: &[Vec<usize>], root: usize) -> Node {
        for i in (0..nodes.len()).rev() {
            for &child_idx in &child_indices[i] {
                if child_idx < nodes.len() {
                    let child = nodes[child_idx].clone();
                    nodes[i].children.push(child);
                }
            }
        }
        nodes[root].clone()
    }

    fn flat_nodes(count: usize) -> Vec<Node> {
        (0..count).map(|i| Node::new(format!("n{i}"))).collect()
    }

    #[test]
    fn assemble_tree_matches_copying_each_reference() {
        let shapes: [(usize, Vec<Vec<usize>>, usize); 4] = [
            // A normal tree in pre-order: 0 -> (1 -> (2, 3), 4).
            (5, vec![vec![1, 4], vec![2, 3], vec![], vec![], vec![]], 0),
            // Node 3 is a child of 1 and of 2.
            (4, vec![vec![1, 2], vec![3], vec![3], vec![]], 0),
            // Node 2 refers back to node 1, to itself and to an absent node.
            (4, vec![vec![1], vec![2, 3], vec![1, 2, 9], vec![]], 0),
            // The root is node 2, and node 0 refers to the root.
            (3, vec![vec![2], vec![], vec![1]], 2),
        ];
        for (count, child_indices, root) in shapes {
            let expected = assemble_by_copy(flat_nodes(count), &child_indices, root);
            let actual = assemble_tree(flat_nodes(count), &child_indices, root).expect("tree");
            assert_eq!(format!("{actual:?}"), format!("{expected:?}"));
        }
    }
}
