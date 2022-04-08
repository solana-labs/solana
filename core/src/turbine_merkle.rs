use sha2::{Digest, Sha256};

pub const TURBINE_MERKLE_HASH_BYTES: usize = 10;

#[derive(Default, Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TurbineMerkleHash(pub [u8; TURBINE_MERKLE_HASH_BYTES]);

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TurbineMerkleProof(pub Vec<TurbineMerkleHash>);

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TurbineMerkleTree {
    tree: Vec<TurbineMerkleHash>,
}

impl TurbineMerkleHash {
    pub fn hash(bufs: &[&[u8]]) -> TurbineMerkleHash {
        let mut hasher = Sha256::new();
        bufs.iter().for_each(|b| hasher.update(b));
        let h = hasher.finalize();
        let mut ret = TurbineMerkleHash::default();
        ret.0[..].copy_from_slice(&h.as_slice()[0..TURBINE_MERKLE_HASH_BYTES]);
        ret
    }
}

impl TurbineMerkleProof {
    fn compute_root(&self, leaf_hash: &TurbineMerkleHash, leaf_index: usize) -> TurbineMerkleHash {
        let mut hash = *leaf_hash;
        let mut idx = leaf_index;
        for i in 0..self.0.len() {
            hash = if idx % 2 == 0 {
                TurbineMerkleHash::hash(&[&hash.0, &self.0[i].0])
            } else {
                TurbineMerkleHash::hash(&[&self.0[i].0, &hash.0])
            };
            idx /= 2;
        }
        hash
    }

    pub fn verify(
        &self,
        root_hash: &TurbineMerkleHash,
        leaf_hash: &TurbineMerkleHash,
        leaf_index: usize,
    ) -> bool {
        &self.compute_root(leaf_hash, leaf_index) == root_hash
    }
}

impl TurbineMerkleTree {
    pub fn new_from_leaves(leaves: &[TurbineMerkleHash]) -> Self {
        // TODO assert leaves.len() is a power of 2
        let tree_size = leaves.len() * 2 - 1;
        let mut tree = Vec::with_capacity(tree_size);

        for leaf in leaves {
            tree.push(*leaf);
        }

        let mut base = 0;
        let mut level_leaves = leaves.len();
        while level_leaves > 1 {
            for i in (0..level_leaves).step_by(2) {
                let hash = TurbineMerkleHash::hash(&[&tree[base + i].0, &tree[base + i + 1].0]);
                tree.push(hash);
            }
            base += level_leaves;
            level_leaves >>= 1;
        }

        Self { tree }
    }

    pub fn new_from_bufs(bufs: &[&[u8]]) -> Self {
        // TODO assert bufs.len() is power of 2 or pad to power of 2
        let leaves: Vec<_> = bufs.iter().map(|b| TurbineMerkleHash::hash(&[b])).collect();
        Self::new_from_leaves(&leaves)
    }

    pub fn leaf_count(&self) -> usize {
        (self.tree.len() + 1) / 2
    }

    pub fn root(&self) -> TurbineMerkleHash {
        self.tree[self.tree.len() - 1]
    }

    pub fn node(&self, index: usize) -> TurbineMerkleHash {
        self.tree[index]
    }

    pub fn prove(&self, leaf_index: usize) -> TurbineMerkleProof {
        let mut proof = Vec::new();
        let mut level_leaves = self.leaf_count();
        let mut i = leaf_index;
        let mut base = 0;
        while level_leaves > 1 {
            if i & 1 == 0 {
                proof.push(self.tree[base + i + 1]);
            } else {
                proof.push(self.tree[base + i - 1]);
            }
            base += level_leaves;
            i >>= 1;
            level_leaves >>= 1;
        }
        TurbineMerkleProof(proof)
    }

    fn _print(&self) {
        let mut base = 0;
        let mut level_nodes = self.leaf_count();
        while level_nodes > 0 {
            println!("{:?}", &self.tree[base..base + level_nodes]);
            base += level_nodes;
            level_nodes /= 2;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_random_packets(npackets: usize) -> Vec<Vec<u8>> {
        let mut packets = Vec::default();
        for _i in 0..npackets {
            let buf: Vec<u8> = (0..1024).map(|_| rand::random::<u8>()).collect();
            packets.push(buf);
        }
        packets
    }

    #[test]
    fn test_merkle() {
        let packets = create_random_packets(16);
        let leaves: Vec<TurbineMerkleHash> = packets
            .iter()
            .map(|p| TurbineMerkleHash::hash(&[p]))
            .collect();

        let tree = TurbineMerkleTree::new_from_leaves(&leaves);
        let root = tree.root();
        let proof5 = tree.prove(5);

        assert!(proof5.verify(&root, &leaves[5], 5));
        assert!(proof5.verify(&root, &tree.node(5), 5));
    }
}
