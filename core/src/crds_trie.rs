use crate::crds::VersionedCrdsValue;
use crate::crds_gossip_pull::CrdsFilter;
use crate::crds_value::CrdsValueLabel;

/// Most significant bit in u64.
const MSB_U64: u64 = 1u64 << 63;

#[derive(Clone)]
pub enum CrdsTrie {
    // Left subtree, size of subtree (num of values at leaves), right subtree.
    // Size is used in pruning to drop empty branches.
    Node(Link, usize, Link),
    // Since values are placed at hashes, collisions should be rare, and so a
    // vector is more efficient than a (hash) set.
    Leaf(Vec<CrdsValueLabel>),
}

type Link = Option<Box<CrdsTrie>>;

fn new_link(leaf: bool) -> Link {
    let node = if leaf {
        CrdsTrie::Leaf(vec![])
    } else {
        CrdsTrie::Node(None, 0, None)
    };
    Some(Box::<CrdsTrie>::new(node))
}

/// Unboxes a &Option<Box<CrdsTrie>> to Option<&CrdsTrie>.
#[inline]
fn unbox_link(link: &Link) -> Option<&CrdsTrie> {
    match link {
        None => None,
        Some(node) => Some(&*node),
    }
}

impl Default for CrdsTrie {
    fn default() -> Self {
        Self::new()
    }
}

impl CrdsTrie {
    pub fn new() -> Self {
        Self::Node(None, 0, None)
    }

    #[must_use]
    pub fn insert(&mut self, value: &VersionedCrdsValue) -> bool {
        self.insert_node(
            CrdsFilter::hash_as_u64(&value.value_hash),
            value.value.label(),
        )
    }

    #[must_use]
    pub fn remove(&mut self, value: &VersionedCrdsValue) -> bool {
        self.remove_node(
            CrdsFilter::hash_as_u64(&value.value_hash),
            &value.value.label(),
        )
    }

    /// Returns all the values where the mask_bits most significant bits of
    /// their hash is the same as the given bit-mask.
    pub fn find(&self, mask: u64, mask_bits: u32) -> impl Iterator<Item = &CrdsValueLabel> {
        let node = self.find_node(mask, mask_bits);
        CrdsTrieIterator::new(node).map(|v| v.iter()).flatten()
    }

    /// Traverses the trie following the bit pattern of the given hash value
    /// and inserts the value at the final leaf node. Returns false if the trie
    /// does not satisfy required invariants (which should indicate a bug).
    fn insert_node(&mut self, hash: u64, value: CrdsValueLabel) -> bool {
        let mut node = self;
        let mut bit = MSB_U64;
        while bit != 0 {
            let branch = match node {
                CrdsTrie::Leaf(_) => return false,
                CrdsTrie::Node(left, size, right) => {
                    *size += 1;
                    if bit & hash == 0 {
                        left
                    } else {
                        right
                    }
                }
            };
            // Initialize the branch if needed.
            if branch.is_none() {
                *branch = new_link(bit == 1);
            }
            node = branch.as_mut().unwrap();
            bit >>= 1;
        }
        match node {
            CrdsTrie::Node(_, _, _) => false,
            CrdsTrie::Leaf(values) => {
                values.push(value);
                true
            }
        }
    }

    /// Traverses the trie following the bit pattern of the given hash value
    /// and removes the value at the final leaf node. Returns false if the
    /// value is not found.
    fn remove_node(&mut self, hash: u64, value: &CrdsValueLabel) -> bool {
        if self.remove_noprune(hash, &value) {
            self.prune(hash);
            true
        } else {
            false
        }
    }

    fn remove_noprune(&mut self, hash: u64, value: &CrdsValueLabel) -> bool {
        let mut node = self;
        let mut bit = MSB_U64;
        while bit != 0 {
            let branch = match node {
                CrdsTrie::Leaf(_) => return false,
                CrdsTrie::Node(left, _, right) => {
                    if bit & hash == 0 {
                        left
                    } else {
                        right
                    }
                }
            };
            node = match branch {
                None => return false,
                Some(node) => node,
            };
            bit >>= 1;
        }
        if let CrdsTrie::Leaf(values) = node {
            if let Some(index) = values.iter().position(|v| v == value) {
                values.swap_remove(index);
                return true;
            }
        }
        false
    }

    /// Prunes the trie along the bit pattern of the given hash value, removing
    /// branches with no values down at leaves.
    fn prune(&mut self, hash: u64) {
        #[inline]
        fn should_drop(link: &Link) -> bool {
            match link {
                None => false,
                Some(node) => matches!(**node, CrdsTrie::Node(_, 1, _)),
            }
        }
        let mut node = self;
        let mut bit = MSB_U64;
        while bit != 0 {
            let branch = match node {
                CrdsTrie::Leaf(_) => return,
                CrdsTrie::Node(left, size, right) => {
                    *size -= 1;
                    if bit & hash == 0 {
                        left
                    } else {
                        right
                    }
                }
            };
            if should_drop(&branch) {
                *branch = None;
                return;
            }
            node = match branch {
                None => return,
                Some(node) => node,
            };
            bit >>= 1;
        }
    }

    /// Traverses the trie following the mask_bits most significant bits of the
    /// given bit mask returning the final node reached.
    fn find_node(&self, mask: u64, mask_bits: u32) -> Option<&Self> {
        let mut node = self;
        let mut bit = MSB_U64;
        for _ in 0..mask_bits.min(64) {
            let branch = match node {
                CrdsTrie::Leaf(_) => &None,
                CrdsTrie::Node(left, _, right) => {
                    if bit & mask == 0 {
                        left
                    } else {
                        right
                    }
                }
            };
            node = branch.as_ref()?;
            bit >>= 1;
        }
        Some(node)
    }
}

struct CrdsTrieIterator<'a> {
    stack: Vec<&'a CrdsTrie>,
    node: Option<&'a CrdsTrie>,
}

impl<'a> CrdsTrieIterator<'a> {
    fn new(node: Option<&'a CrdsTrie>) -> CrdsTrieIterator<'a> {
        CrdsTrieIterator {
            stack: vec![],
            node,
        }
    }
}

impl<'a> Iterator for CrdsTrieIterator<'a> {
    type Item = &'a Vec<CrdsValueLabel>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.node {
                None => match self.stack.pop() {
                    None => return None,
                    node => self.node = node,
                },
                Some(CrdsTrie::Leaf(labels)) => {
                    self.node = self.stack.pop();
                    return Some(labels);
                }
                Some(CrdsTrie::Node(left, _, right)) => {
                    self.node = unbox_link(&left);
                    if let Some(node) = unbox_link(&right) {
                        self.stack.push(node);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use rand::{thread_rng, Rng};
    use solana_sdk::pubkey::Pubkey;
    use std::collections::HashMap;

    fn new_test_crds_value_label<R: ?Sized>(rng: &mut R) -> CrdsValueLabel
    where
        R: Rng,
    {
        let mut pubkey = [0u8; 32];
        rng.fill(&mut pubkey);
        let pubkey = Pubkey::new(&pubkey);
        match rng.gen_range(0, 5) {
            0 => CrdsValueLabel::ContactInfo(pubkey),
            1 => CrdsValueLabel::LowestSlot(pubkey),
            2 => CrdsValueLabel::SnapshotHashes(pubkey),
            3 => CrdsValueLabel::AccountsHashes(pubkey),
            _ => CrdsValueLabel::Version(pubkey),
        }
    }

    fn new_test_hash_value<R: ?Sized>(rng: &mut R) -> u64
    where
        R: Rng,
    {
        // In order to simulate hash collisions, only 10 bits are random.
        const BASE: u64 = 0xf6b6a84572956f51;
        BASE ^ rng.gen_range(0, 1024)
    }

    // Returns true if the first mask_bits most significant bits of hash is the
    // same as the given bit mask.
    fn check_mask(hash: u64, mask: u64, mask_bits: u32) -> bool {
        let ones = (!0u64).checked_shr(mask_bits).unwrap_or(0u64);
        (hash | ones) == (mask | ones)
    }

    #[test]
    fn test_trie_round_trip() {
        let mut rng = thread_rng();
        let mut values: Vec<(u64, CrdsValueLabel)> = vec![];
        // Generate some random hash and crds value labels.
        for _ in 0..4096 {
            let index = rng.gen_range(0, 4096);
            let value = if index < values.len() {
                values[index].1.clone()
            } else {
                new_test_crds_value_label(&mut rng)
            };
            values.push((new_test_hash_value(&mut rng), value));
        }
        // Add some duplicates.
        let dups: Vec<_> = (0..100)
            .map(|_| {
                let index = rng.gen_range(0, values.len());
                values[index].clone()
            })
            .collect();
        values.extend(dups);
        // Insert everything into the trie.
        let mut trie = CrdsTrie::new();
        for (hash, value) in &values {
            assert!(trie.insert_node(*hash, value.clone()));
        }
        // Remove some of the values.
        for _ in 0..500 {
            let index = rng.gen_range(0, values.len());
            let (hash, value) = values.swap_remove(index);
            assert!(trie.remove_node(hash, &value));
        }
        assert_eq!(
            values.len(),
            match &trie {
                CrdsTrie::Node(_, size, _) => *size,
                _ => 0,
            }
        );
        for _ in 0..10 {
            let mask = new_test_hash_value(&mut rng);
            for mask_bits in 53..65 {
                // Assert that all values are observed the same number of times
                // as brute-force filtering.
                let mut table = HashMap::new();
                for (hash, value) in &values {
                    if check_mask(*hash, mask, mask_bits) {
                        let count = table.entry(value.clone()).or_insert(0);
                        *count += 1;
                    }
                }
                for value in trie.find(mask, mask_bits) {
                    match table.get_mut(value) {
                        None => panic!("value does not exist!"),
                        Some(count) => {
                            *count -= 1;
                            if *count == 0 {
                                table.remove(value);
                            }
                        }
                    }
                }
                assert!(table.is_empty());
            }
        }
        // Remove everything and assert that the trie becomes empty.
        for (hash, value) in values {
            assert!(trie.remove_node(hash, &value));
        }
        match trie {
            CrdsTrie::Node(None, 0, None) => (),
            _ => panic!("trie is not empty"),
        }
    }
}
