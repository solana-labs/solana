use solana_sdk::hash::{hash, hashv, Hash};

#[derive(Debug)]
pub struct MerkleTree {
    leaf_count: usize,
    nodes: Vec<Hash>,
}

#[derive(Debug, PartialEq)]
pub struct ProofEntry<'a>(&'a Hash, Option<&'a Hash>, Option<&'a Hash>);

impl<'a> ProofEntry<'a> {
    pub fn new(
        target: &'a Hash,
        left_sibling: Option<&'a Hash>,
        right_sibling: Option<&'a Hash>,
    ) -> Self {
        assert!((None == left_sibling) ^ (None == right_sibling));
        Self(target, left_sibling, right_sibling)
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct Proof<'a>(Vec<ProofEntry<'a>>);

impl<'a> Proof<'a> {
    pub fn push(&mut self, entry: ProofEntry<'a>) {
        self.0.push(entry)
    }

    pub fn verify(&self, candidate: Hash) -> bool {
        let result = self.0.iter().try_fold(candidate, |candidate, pe| {
            let lsib = pe.1.unwrap_or(&candidate);
            let rsib = pe.2.unwrap_or(&candidate);
            let hash = hashv(&[lsib.as_ref(), rsib.as_ref()]);

            if hash == *pe.0 {
                Some(hash)
            } else {
                None
            }
        });
        match result {
            Some(_) => true,
            _ => false,
        }
    }
}

impl MerkleTree {
    #[inline]
    fn next_level_len(level_len: usize) -> usize {
        if level_len == 1 {
            0
        } else {
            (level_len + 1) / 2
        }
    }

    fn calculate_vec_capacity(mut leaf_count: usize) -> usize {
        let mut capacity = 0;
        while leaf_count > 0 {
            capacity += leaf_count;
            leaf_count = MerkleTree::next_level_len(leaf_count);
        }
        capacity
    }

    pub fn new(items: &[&[u8]]) -> Self {
        let cap = MerkleTree::calculate_vec_capacity(items.len());
        let mut mt = MerkleTree {
            leaf_count: items.len(),
            nodes: Vec::with_capacity(cap),
        };

        for item in items {
            let hash = hash(item);
            mt.nodes.push(hash);
        }

        let mut level_len = MerkleTree::next_level_len(items.len());
        let mut level_start = items.len();
        let mut prev_level_len = items.len();
        let mut prev_level_start = 0;
        while level_len > 0 {
            for i in 0..level_len {
                let prev_level_idx = 2 * i;
                let lsib = &mt.nodes[prev_level_start + prev_level_idx];
                let rsib = if prev_level_idx + 1 < prev_level_len {
                    &mt.nodes[prev_level_start + prev_level_idx + 1]
                } else {
                    // Duplicate last entry if the level length is odd
                    &mt.nodes[prev_level_start + prev_level_idx]
                };

                let hash = hashv(&[lsib.as_ref(), rsib.as_ref()]);
                mt.nodes.push(hash);
            }
            prev_level_start = level_start;
            prev_level_len = level_len;
            level_start += level_len;
            level_len = MerkleTree::next_level_len(level_len);
        }

        mt
    }

    pub fn get_root(&self) -> Option<&Hash> {
        self.nodes.iter().last()
    }

    pub fn find_path(&self, index: usize) -> Option<Proof> {
        if index >= self.leaf_count {
            return None;
        }

        let mut level_len = self.leaf_count;
        let mut level_start = 0;
        let mut path = Proof::default();
        let mut node_index = index;
        let mut lsib = None;
        let mut rsib = None;
        while level_len > 0 {
            let level = &self.nodes[level_start..(level_start + level_len)];

            let target = &level[node_index];
            if lsib != None || rsib != None {
                path.push(ProofEntry::new(target, lsib, rsib));
            }
            if node_index % 2 == 0 {
                lsib = None;
                rsib = if node_index + 1 < level.len() {
                    Some(&level[node_index + 1])
                } else {
                    Some(&level[node_index])
                };
            } else {
                lsib = Some(&level[node_index - 1]);
                rsib = None;
            }
            node_index /= 2;

            level_start += level_len;
            level_len = MerkleTree::next_level_len(level_len);
        }
        Some(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hex;

    const TEST: &'static [&'static [u8]] = &[
        b"my", b"very", b"eager", b"mother", b"just", b"served", b"us", b"nine", b"pizzas",
        b"make", b"prime",
    ];
    const BAD: &'static [&'static [u8]] = &[b"bad", b"missing", b"false"];

    #[test]
    fn test_tree_from_empty() {
        let mt = MerkleTree::new(&[]);
        assert_eq!(mt.get_root(), None);
    }

    #[test]
    fn test_tree_from_one() {
        let input = b"test";
        let mt = MerkleTree::new(&[input]);
        let expected = hash(input);
        assert_eq!(mt.get_root(), Some(&expected));
    }

    #[test]
    fn test_tree_from_many() {
        let mt = MerkleTree::new(TEST);
        // This golden hash will need to be updated whenever the contents of `TEST` change in any
        // way, including addition, removal and reordering
        let bytes = hex::decode("7e6791d2b9a6338e1446ad4776f267b97e7e4ed968ae2592cfd9c1607bd7dbbb")
            .unwrap();
        let expected = Hash::new(&bytes);
        assert_eq!(mt.get_root(), Some(&expected));
    }

    #[test]
    fn test_path_creation() {
        let mt = MerkleTree::new(TEST);
        for (i, _s) in TEST.iter().enumerate() {
            let _path = mt.find_path(i).unwrap();
        }
    }

    #[test]
    fn test_path_creation_bad_index() {
        let mt = MerkleTree::new(TEST);
        assert_eq!(mt.find_path(TEST.len()), None);
    }

    #[test]
    fn test_path_verify_good() {
        let mt = MerkleTree::new(TEST);
        for (i, s) in TEST.iter().enumerate() {
            let hash = hash(s);
            let path = mt.find_path(i).unwrap();
            assert!(path.verify(hash));
        }
    }

    #[test]
    fn test_path_verify_bad() {
        let mt = MerkleTree::new(TEST);
        for (i, s) in BAD.iter().enumerate() {
            let hash = hash(s);
            let path = mt.find_path(i).unwrap();
            assert!(!path.verify(hash));
        }
    }

    #[test]
    fn test_proof_entry_instantiation_lsib_set() {
        ProofEntry::new(&Hash::default(), Some(&Hash::default()), None);
    }

    #[test]
    fn test_proof_entry_instantiation_rsib_set() {
        ProofEntry::new(&Hash::default(), None, Some(&Hash::default()));
    }

    #[test]
    #[should_panic]
    fn test_proof_entry_instantiation_both_clear() {
        ProofEntry::new(&Hash::default(), None, None);
    }

    #[test]
    #[should_panic]
    fn test_proof_entry_instantiation_both_set() {
        ProofEntry::new(
            &Hash::default(),
            Some(&Hash::default()),
            Some(&Hash::default()),
        );
    }
}
