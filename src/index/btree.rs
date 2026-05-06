//! # B+ Tree Index
//!
//! An in-memory B+ tree index for fast key-based lookups and range scans.
//!
//! ## Design
//!
//! ```text
//!            [Internal Node: keys=[30, 70]]
//!           /           |              \
//!   [Leaf: 10,20]  [Leaf: 30,50]  [Leaf: 70,90]
//!       ↓    ↓         ↓    ↓         ↓    ↓
//!     TID  TID       TID  TID       TID  TID
//!
//!   Leaf nodes are linked: [10,20] → [30,50] → [70,90]
//! ```
//!
//! Keys are `Vec<u8>` for type-agnostic comparison (lexicographic ordering).
//! Values are `TupleId` references back to the heap file.

use crate::storage::page::TupleId;
use std::fmt;

/// Order of the B+ tree (max keys per node = ORDER - 1).
const ORDER: usize = 64;
/// Minimum keys per non-root node.
#[allow(dead_code)]
const MIN_KEYS: usize = ORDER / 2;

/// A key in the B+ tree — stored as bytes for type-agnostic comparison.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct IndexKey(pub Vec<u8>);

impl IndexKey {
    pub fn from_i32(v: i32) -> Self {
        // Use big-endian for correct lexicographic ordering
        IndexKey(v.to_be_bytes().to_vec())
    }

    pub fn from_i64(v: i64) -> Self {
        IndexKey(v.to_be_bytes().to_vec())
    }

    pub fn from_str(s: &str) -> Self {
        IndexKey(s.as_bytes().to_vec())
    }
}

impl fmt::Display for IndexKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.len() == 4 {
            let v = i32::from_be_bytes(self.0[..4].try_into().unwrap());
            write!(f, "{}", v)
        } else if self.0.len() == 8 {
            let v = i64::from_be_bytes(self.0[..8].try_into().unwrap());
            write!(f, "{}", v)
        } else {
            write!(f, "{}", String::from_utf8_lossy(&self.0))
        }
    }
}

/// A node in the B+ tree.
#[derive(Debug, Clone)]
enum BTreeNode {
    /// Internal node: keys + child pointers.
    Internal {
        keys: Vec<IndexKey>,
        children: Vec<usize>, // indices into the node array
    },
    /// Leaf node: keys + values + next leaf pointer.
    Leaf {
        keys: Vec<IndexKey>,
        values: Vec<TupleId>,
        next_leaf: Option<usize>,
    },
}

/// An in-memory B+ tree index.
pub struct BTree {
    /// All nodes stored in a flat vector (arena allocation).
    nodes: Vec<BTreeNode>,
    /// Root node index.
    root: usize,
    /// Name of this index.
    pub name: String,
    /// Number of entries.
    len: usize,
}

impl BTree {
    /// Create a new empty B+ tree.
    pub fn new(name: &str) -> Self {
        let root = BTreeNode::Leaf {
            keys: Vec::new(),
            values: Vec::new(),
            next_leaf: None,
        };
        Self {
            nodes: vec![root],
            root: 0,
            name: name.to_string(),
            len: 0,
        }
    }

    /// Insert a key-value pair.
    pub fn insert(&mut self, key: IndexKey, value: TupleId) {
        let root = self.root;
        let result = self.insert_recursive(root, key, value);

        if let Some((split_key, new_node_id)) = result {
            // Root split — create new root
            let new_root = BTreeNode::Internal {
                keys: vec![split_key],
                children: vec![self.root, new_node_id],
            };
            let new_root_id = self.nodes.len();
            self.nodes.push(new_root);
            self.root = new_root_id;
        }

        self.len += 1;
    }

    /// Recursive insert. Returns Some((split_key, new_node_id)) if a split occurred.
    fn insert_recursive(&mut self, node_id: usize, key: IndexKey, value: TupleId) -> Option<(IndexKey, usize)> {
        let node = self.nodes[node_id].clone();

        match node {
            BTreeNode::Leaf { mut keys, mut values, next_leaf } => {
                // Find insertion position
                let pos = keys.binary_search(&key).unwrap_or_else(|p| p);
                keys.insert(pos, key);
                values.insert(pos, value);

                if keys.len() < ORDER {
                    self.nodes[node_id] = BTreeNode::Leaf { keys, values, next_leaf };
                    None
                } else {
                    // Split
                    let mid = keys.len() / 2;
                    let split_key = keys[mid].clone();

                    let right_keys = keys.split_off(mid);
                    let right_values = values.split_off(mid);

                    let right_id = self.nodes.len();
                    self.nodes.push(BTreeNode::Leaf {
                        keys: right_keys,
                        values: right_values,
                        next_leaf,
                    });

                    self.nodes[node_id] = BTreeNode::Leaf {
                        keys,
                        values,
                        next_leaf: Some(right_id),
                    };

                    Some((split_key, right_id))
                }
            }
            BTreeNode::Internal { mut keys, mut children } => {
                // Find child to recurse into
                // If key == keys[pos], go right (children[pos+1]); else go to insert point
                let pos = match keys.binary_search(&key) {
                    Ok(p) => p + 1,
                    Err(p) => p,
                };
                let child_id = children[pos];
                let result = self.insert_recursive(child_id, key, value);

                if let Some((split_key, new_child_id)) = result {
                    keys.insert(pos, split_key);
                    children.insert(pos + 1, new_child_id);

                    if keys.len() < ORDER {
                        self.nodes[node_id] = BTreeNode::Internal { keys, children };
                        None
                    } else {
                        // Split internal node
                        let mid = keys.len() / 2;
                        let up_key = keys[mid].clone();

                        let mut right_keys = keys.split_off(mid);
                        right_keys.remove(0); // The middle key goes up
                        let right_children = children.split_off(mid + 1);

                        let right_id = self.nodes.len();
                        self.nodes.push(BTreeNode::Internal {
                            keys: right_keys,
                            children: right_children,
                        });

                        self.nodes[node_id] = BTreeNode::Internal { keys, children };
                        Some((up_key, right_id))
                    }
                } else {
                    None
                }
            }
        }
    }

    /// Search for a key. Returns the TupleId if found.
    pub fn search(&self, key: &IndexKey) -> Option<TupleId> {
        let mut node_id = self.root;
        loop {
            match &self.nodes[node_id] {
                BTreeNode::Leaf { keys, values, .. } => {
                    if let Ok(pos) = keys.binary_search(key) {
                        return Some(values[pos]);
                    }
                    return None;
                }
                BTreeNode::Internal { keys, children, .. } => {
                    // Go right if key >= separator
                    let pos = match keys.binary_search(key) {
                        Ok(p) => p + 1,
                        Err(p) => p,
                    };
                    node_id = children[pos];
                }
            }
        }
    }

    /// Range scan: returns all entries with key in [start, end] (inclusive).
    pub fn range_scan(&self, start: &IndexKey, end: &IndexKey) -> Vec<(IndexKey, TupleId)> {
        let mut results = Vec::new();

        // Find the leaf containing `start`
        let mut node_id = self.root;
        loop {
            match &self.nodes[node_id] {
                BTreeNode::Leaf { .. } => break,
                BTreeNode::Internal { keys, children, .. } => {
                    let pos = match keys.binary_search(start) {
                        Ok(p) => p + 1,
                        Err(p) => p,
                    };
                    node_id = children[pos];
                }
            }
        }

        // Scan through leaves
        loop {
            match &self.nodes[node_id] {
                BTreeNode::Leaf { keys, values, next_leaf } => {
                    for (i, k) in keys.iter().enumerate() {
                        if k >= start && k <= end {
                            results.push((k.clone(), values[i]));
                        } else if k > end {
                            return results;
                        }
                    }
                    if let Some(next) = next_leaf {
                        node_id = *next;
                    } else {
                        break;
                    }
                }
                _ => break,
            }
        }

        results
    }

    /// Delete a key from the index.
    pub fn delete(&mut self, key: &IndexKey) -> bool {
        self.delete_recursive(self.root, key)
    }

    fn delete_recursive(&mut self, node_id: usize, key: &IndexKey) -> bool {
        let node = self.nodes[node_id].clone();
        match node {
            BTreeNode::Leaf { mut keys, mut values, next_leaf } => {
                if let Ok(pos) = keys.binary_search(key) {
                    keys.remove(pos);
                    values.remove(pos);
                    self.nodes[node_id] = BTreeNode::Leaf { keys, values, next_leaf };
                    self.len -= 1;
                    true
                } else {
                    false
                }
            }
            BTreeNode::Internal { keys, children, .. } => {
                let pos = keys.binary_search(key).unwrap_or_else(|p| p);
                self.delete_recursive(children[pos], key)
            }
        }
    }

    /// Number of entries in the index.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether the index is empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Get all entries (full scan through leaves).
    pub fn scan_all(&self) -> Vec<(IndexKey, TupleId)> {
        let mut results = Vec::new();

        // Find leftmost leaf
        let mut node_id = self.root;
        loop {
            match &self.nodes[node_id] {
                BTreeNode::Leaf { .. } => break,
                BTreeNode::Internal { children, .. } => {
                    node_id = children[0];
                }
            }
        }

        // Scan all leaves via next pointers
        loop {
            match &self.nodes[node_id] {
                BTreeNode::Leaf { keys, values, next_leaf } => {
                    for (i, k) in keys.iter().enumerate() {
                        results.push((k.clone(), values[i]));
                    }
                    if let Some(next) = next_leaf {
                        node_id = *next;
                    } else {
                        break;
                    }
                }
                _ => break,
            }
        }

        results
    }

    /// Height of the tree.
    pub fn height(&self) -> usize {
        let mut h = 0;
        let mut node_id = self.root;
        loop {
            h += 1;
            match &self.nodes[node_id] {
                BTreeNode::Leaf { .. } => return h,
                BTreeNode::Internal { children, .. } => {
                    node_id = children[0];
                }
            }
        }
    }
}

impl fmt::Debug for BTree {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BTree")
            .field("name", &self.name)
            .field("entries", &self.len)
            .field("nodes", &self.nodes.len())
            .field("height", &self.height())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tid(page: u32, slot: u16) -> TupleId {
        TupleId { page_id: page, slot_id: slot }
    }

    #[test]
    fn test_insert_and_search() {
        let mut tree = BTree::new("idx_id");
        tree.insert(IndexKey::from_i32(10), tid(0, 0));
        tree.insert(IndexKey::from_i32(20), tid(0, 1));
        tree.insert(IndexKey::from_i32(5), tid(0, 2));

        assert_eq!(tree.search(&IndexKey::from_i32(10)), Some(tid(0, 0)));
        assert_eq!(tree.search(&IndexKey::from_i32(20)), Some(tid(0, 1)));
        assert_eq!(tree.search(&IndexKey::from_i32(5)), Some(tid(0, 2)));
        assert_eq!(tree.search(&IndexKey::from_i32(99)), None);
    }

    #[test]
    fn test_range_scan() {
        let mut tree = BTree::new("idx");
        for i in 0..100 {
            tree.insert(IndexKey::from_i32(i * 10), tid(0, i as u16));
        }

        let results = tree.range_scan(
            &IndexKey::from_i32(200),
            &IndexKey::from_i32(500),
        );
        assert_eq!(results.len(), 31); // 200, 210, ..., 500
    }

    #[test]
    fn test_delete() {
        let mut tree = BTree::new("idx");
        tree.insert(IndexKey::from_i32(10), tid(0, 0));
        tree.insert(IndexKey::from_i32(20), tid(0, 1));

        assert!(tree.delete(&IndexKey::from_i32(10)));
        assert_eq!(tree.search(&IndexKey::from_i32(10)), None);
        assert_eq!(tree.search(&IndexKey::from_i32(20)), Some(tid(0, 1)));
        assert_eq!(tree.len(), 1);
    }

    #[test]
    fn test_large_insert_triggers_splits() {
        let mut tree = BTree::new("idx");
        for i in 0..1000 {
            tree.insert(IndexKey::from_i32(i), tid((i / 100) as u32, (i % 100) as u16));
        }
        assert_eq!(tree.len(), 1000);
        assert!(tree.height() > 1);

        // Verify all searchable
        for i in 0..1000 {
            assert!(tree.search(&IndexKey::from_i32(i)).is_some(),
                "failed to find key {}", i);
        }
    }

    #[test]
    fn test_scan_all_ordered() {
        let mut tree = BTree::new("idx");
        // Insert in random order
        for &i in &[50, 10, 90, 30, 70, 20, 80, 40, 60] {
            tree.insert(IndexKey::from_i32(i), tid(0, i as u16));
        }

        let all = tree.scan_all();
        assert_eq!(all.len(), 9);
        // Should be sorted
        for w in all.windows(2) {
            assert!(w[0].0 <= w[1].0);
        }
    }

    #[test]
    fn test_string_keys() {
        let mut tree = BTree::new("idx_name");
        tree.insert(IndexKey::from_str("alice"), tid(0, 0));
        tree.insert(IndexKey::from_str("bob"), tid(0, 1));
        tree.insert(IndexKey::from_str("charlie"), tid(0, 2));

        assert_eq!(tree.search(&IndexKey::from_str("bob")), Some(tid(0, 1)));
        assert_eq!(tree.search(&IndexKey::from_str("dave")), None);
    }
}
