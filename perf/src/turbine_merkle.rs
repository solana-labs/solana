#![allow(clippy::integer_arithmetic)]
use {
    rayon::{iter::ParallelIterator, prelude::*},
    serde::{Deserialize, Serialize},
    sha2::{Digest, Sha256},
    thiserror::Error,
};

pub const TURBINE_MERKLE_HASH_BYTES: usize = 20;
pub const TURBINE_MERKLE_ROOT_BYTES: usize = TURBINE_MERKLE_HASH_BYTES;

// Defense against second preimage attack:
// https://en.wikipedia.org/wiki/Merkle_tree#Second_preimage_attack
const LEAF_PREFIX: &[u8] = &[0];
const INTERMEDIATE_PREFIX: &[u8] = &[1];

#[derive(Debug, Error)]
#[allow(clippy::enum_variant_names)]
pub enum Error {
    #[error(transparent)]
    TryFromSliceError(#[from] std::array::TryFromSliceError),
    #[error("Invalid proof buf size: {0}")]
    InvalidProofBufSize(/*buf size*/ usize),
}

#[repr(transparent)]
#[derive(Default, Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TurbineMerkleHash([u8; TURBINE_MERKLE_HASH_BYTES]);

impl TurbineMerkleHash {
    pub fn hash<T, U>(bufs: &[T], prefix: Option<U>) -> TurbineMerkleHash
    where
        T: AsRef<[u8]> + Sync,
        U: AsRef<[u8]> + Sync,
    {
        let mut hasher = Sha256::new();
        if let Some(prefix) = prefix {
            hasher.update(prefix);
        }
        bufs.iter().for_each(|b| hasher.update(b));
        let h = hasher.finalize();
        let mut ret = TurbineMerkleHash::default();
        ret.0[..].copy_from_slice(&h.as_slice()[0..TURBINE_MERKLE_HASH_BYTES]);
        ret
    }

    #[inline]
    pub fn hash_leaf<T>(bufs: &[T]) -> TurbineMerkleHash
    where
        T: AsRef<[u8]> + Sync,
    {
        Self::hash(bufs, Some(LEAF_PREFIX))
    }

    #[inline]
    pub fn hash_intermediate<T>(bufs: &[T]) -> TurbineMerkleHash
    where
        T: AsRef<[u8]> + Sync,
    {
        Self::hash(bufs, Some(INTERMEDIATE_PREFIX))
    }
}

impl AsRef<[u8]> for TurbineMerkleHash {
    fn as_ref(&self) -> &[u8] {
        &self.0[..]
    }
}

impl TryFrom<&[u8]> for TurbineMerkleHash {
    type Error = Error;
    fn try_from(buf: &[u8]) -> Result<Self, Self::Error> {
        Ok(TurbineMerkleHash(buf.try_into()?))
    }
}

impl From<[u8; TURBINE_MERKLE_HASH_BYTES]> for TurbineMerkleHash {
    fn from(buf: [u8; TURBINE_MERKLE_HASH_BYTES]) -> Self {
        TurbineMerkleHash(buf)
    }
}

#[repr(transparent)]
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TurbineMerkleProof(Vec<TurbineMerkleHash>);

impl TurbineMerkleProof {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut v = vec![0u8; TURBINE_MERKLE_HASH_BYTES * self.0.len()];
        v.chunks_exact_mut(TURBINE_MERKLE_HASH_BYTES)
            .enumerate()
            .for_each(|(i, b)| b.copy_from_slice(self.0[i].as_ref()));
        v
    }

    fn generic_compute_root(
        proof: &[TurbineMerkleHash],
        leaf_hash: &TurbineMerkleHash,
        leaf_index: usize,
    ) -> TurbineMerkleHash {
        let mut hash = *leaf_hash;
        let mut idx = leaf_index;
        for elem in proof {
            hash = if idx & 1 == 0 {
                TurbineMerkleHash::hash_intermediate(&[&hash.0, &elem.0])
            } else {
                TurbineMerkleHash::hash_intermediate(&[&elem.0, &hash.0])
            };
            idx >>= 1;
        }
        hash
    }

    pub fn compute_root_buf(
        proof: &[u8],
        leaf_hash: &TurbineMerkleHash,
        leaf_index: usize,
    ) -> TurbineMerkleHash {
        let mut hash = *leaf_hash;
        let mut idx = leaf_index;
        for chunk in proof.chunks_exact(TURBINE_MERKLE_HASH_BYTES) {
            hash = if idx & 1 == 0 {
                TurbineMerkleHash::hash_intermediate(&[&hash.0[..], chunk])
            } else {
                TurbineMerkleHash::hash_intermediate(&[chunk, &hash.0[..]])
            };
            idx >>= 1;
        }
        hash
    }

    #[inline]
    pub fn compute_root(
        &self,
        leaf_hash: &TurbineMerkleHash,
        leaf_index: usize,
    ) -> TurbineMerkleHash {
        Self::generic_compute_root(&self.0, leaf_hash, leaf_index)
    }

    #[inline]
    pub fn verify(
        &self,
        root_hash: &TurbineMerkleHash,
        leaf_hash: &TurbineMerkleHash,
        leaf_index: usize,
    ) -> bool {
        &Self::generic_compute_root(&self.0, leaf_hash, leaf_index) == root_hash
    }

    #[inline]
    pub fn verify_buf(
        root_hash: &TurbineMerkleHash,
        proof: &[u8],
        leaf_hash: &TurbineMerkleHash,
        leaf_index: usize,
    ) -> bool {
        &Self::compute_root_buf(proof, leaf_hash, leaf_index) == root_hash
    }
}

impl TryFrom<&[u8]> for TurbineMerkleProof {
    type Error = Error;
    fn try_from(buf: &[u8]) -> Result<Self, Self::Error> {
        if buf.len() % TURBINE_MERKLE_HASH_BYTES != 0 {
            return Err(Error::InvalidProofBufSize(buf.len()));
        }
        let v: Vec<TurbineMerkleHash> = buf
            .chunks_exact(TURBINE_MERKLE_HASH_BYTES)
            .map(|x| x.try_into().unwrap())
            .collect();
        Ok(TurbineMerkleProof(v))
    }
}

#[derive(Default, Debug, Clone, PartialEq)]
pub struct TurbineMerkleTree {
    tree: Vec<TurbineMerkleHash>,
}

impl TurbineMerkleTree {
    pub fn new_from_leaves(leaves: &[TurbineMerkleHash]) -> Self {
        let leaves_len = leaves.len().next_power_of_two();
        let tree_size = leaves_len * 2 - 1;
        let mut tree = Vec::with_capacity(tree_size);
        for leaf in leaves {
            tree.push(*leaf);
        }
        if leaves_len != leaves.len() {
            let empty_leaf = TurbineMerkleHash([0u8; TURBINE_MERKLE_HASH_BYTES]);
            for _ in 0..(leaves_len - leaves.len()) {
                tree.push(empty_leaf);
            }
        }
        let mut base = 0;
        let mut level_leaves = leaves_len;
        while level_leaves > 1 {
            for i in (0..level_leaves).step_by(2) {
                let hash = TurbineMerkleHash::hash_intermediate(&[
                    &tree[base + i].0,
                    &tree[base + i + 1].0,
                ]);
                tree.push(hash);
            }
            base += level_leaves;
            level_leaves >>= 1;
        }
        Self { tree }
    }

    pub fn new_from_bufs_vec_par<T>(bufs_vec: &Vec<Vec<T>>, chunk: usize) -> Self
    where
        T: AsRef<[u8]> + Sync,
    {
        let base_width = bufs_vec.len().next_power_of_two();
        assert_eq!(chunk, chunk.next_power_of_two());
        assert_eq!(base_width % chunk, 0);
        assert!(chunk <= base_width);
        assert_eq!(base_width / chunk, (base_width / chunk).next_power_of_two());

        let empty_vec = vec![];
        let mut expanded_bufs_vec = Vec::with_capacity(base_width);
        for b in bufs_vec {
            expanded_bufs_vec.push(b);
        }
        for _ in 0..(base_width - bufs_vec.len()) {
            expanded_bufs_vec.push(&empty_vec);
        }

        // compute subtrees of chunk width in parallel
        let sub_trees: Vec<Vec<TurbineMerkleHash>> = expanded_bufs_vec
            .par_chunks(chunk)
            .map(|slice: &[&Vec<T>]| {
                let mut tree = Vec::with_capacity(chunk * 2 - 1);
                slice.iter().for_each(|v: &&Vec<T>| {
                    if v.is_empty() {
                        tree.push(TurbineMerkleHash([0u8; TURBINE_MERKLE_HASH_BYTES]));
                    } else {
                        tree.push(TurbineMerkleHash::hash_leaf(v));
                    }
                });

                let mut base = 0;
                let mut level_leaves = slice.len();
                while level_leaves > 1 {
                    for i in (0..level_leaves).step_by(2) {
                        let hash = TurbineMerkleHash::hash_intermediate(&[
                            &tree[base + i].0,
                            &tree[base + i + 1].0,
                        ]);
                        tree.push(hash);
                    }
                    base += level_leaves;
                    level_leaves >>= 1;
                }
                tree
            })
            .collect();

        let tree_size = base_width * 2 - 1;
        let mut tree = Vec::with_capacity(tree_size);
        let mut level_nodes = chunk;
        let mut base = 0;
        while level_nodes >= 1 {
            for sub_tree in sub_trees.iter() {
                for i in 0..level_nodes {
                    tree.push(sub_tree[base + i]);
                }
            }
            base += level_nodes;
            level_nodes >>= 1;
        }

        // compute final levels of tree
        level_nodes = base_width / chunk;
        base = (chunk * 2 - 1) * level_nodes - level_nodes;
        while level_nodes > 1 {
            for i in (0..level_nodes).step_by(2) {
                let hash = TurbineMerkleHash::hash_intermediate(&[
                    &tree[base + i].0,
                    &tree[base + i + 1].0,
                ]);
                tree.push(hash);
            }
            base += level_nodes;
            level_nodes >>= 1;
        }

        Self { tree }
    }

    pub fn leaf_count(&self) -> usize {
        (self.tree.len() + 1) / 2
    }

    pub fn root(&self) -> TurbineMerkleHash {
        self.tree[self.tree.len() - 1]
    }

    pub fn prove(&self, leaf_index: usize) -> TurbineMerkleProof {
        let mut proof = Vec::new();
        let mut level_nodes = self.leaf_count();
        let mut i = leaf_index;
        let mut base = 0;
        while level_nodes > 1 {
            if i & 1 == 0 {
                proof.push(self.tree[base + i + 1]);
            } else {
                proof.push(self.tree[base + i - 1]);
            }
            base += level_nodes;
            i >>= 1;
            level_nodes >>= 1;
        }
        TurbineMerkleProof(proof)
    }

    pub fn prove_to_buf(&self, leaf_index: usize, proof_buf: &mut [u8]) -> Result<(), Error> {
        let mut level_nodes = self.leaf_count();
        let mut i = leaf_index;
        let mut base = 0;
        let mut proof_idx = 0;
        while level_nodes > 1 {
            let proof_offset = proof_idx * TURBINE_MERKLE_HASH_BYTES;
            let hash = if i & 1 == 0 {
                &self.tree[base + i + 1]
            } else {
                &self.tree[base + i - 1]
            };
            let b = proof_buf.get_mut(proof_offset..proof_offset + TURBINE_MERKLE_HASH_BYTES);
            if b.is_none() {
                return Err(Error::InvalidProofBufSize(proof_buf.len()));
            }
            b.unwrap().copy_from_slice(hash.as_ref());
            base += level_nodes;
            i >>= 1;
            level_nodes >>= 1;
            proof_idx += 1;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use {super::*, matches::assert_matches};

    fn create_random_packets(npackets: usize) -> Vec<Vec<u8>> {
        let mut packets = Vec::default();
        for _i in 0..npackets {
            let buf: Vec<u8> = (0..1024).map(|_| rand::random::<u8>()).collect();
            packets.push(buf);
        }
        packets
    }

    fn new_from_bufs<T>(bufs: &[T]) -> TurbineMerkleTree
    where
        T: Sync + AsRef<[u8]>,
    {
        let leaves: Vec<_> = bufs
            .iter()
            .map(|b| TurbineMerkleHash::hash_leaf(&[b.as_ref()]))
            .collect();
        TurbineMerkleTree::new_from_leaves(&leaves)
    }

    #[test]
    fn test_merkle() {
        let npackets = 64;
        let packets = create_random_packets(npackets);
        let leaves: Vec<TurbineMerkleHash> = packets
            .iter()
            .map(|p| TurbineMerkleHash::hash_leaf(&[p]))
            .collect();

        let tree = TurbineMerkleTree::new_from_leaves(&leaves);
        let root = tree.root();

        for i in 0..npackets {
            let proof = tree.prove(i);
            assert!(proof.verify(&root, &tree.tree[i], i));
        }

        for i in 0..npackets {
            let proof = tree.prove(i);
            let proof_buf = proof.to_bytes();
            assert!(TurbineMerkleProof::verify_buf(
                &root,
                &proof_buf,
                &tree.tree[i],
                i
            ));
        }
    }

    fn test_merkle_from_buf_vecs_par_width(leaf_count: usize, chunk: usize) {
        let packets = create_random_packets(leaf_count);
        let ref_tree = new_from_bufs(&packets[..]);
        let bufs_vec: Vec<_> = packets.iter().map(|p| vec![&p[..20], &p[20..]]).collect();
        let tree = TurbineMerkleTree::new_from_bufs_vec_par(&bufs_vec, chunk);
        assert_eq!(&ref_tree, &tree);
    }

    #[test]
    fn test_merkle_from_buf_vecs_par() {
        let packets = create_random_packets(64);
        let ref_tree = new_from_bufs(&packets[..]);
        let bufs_vec: Vec<_> = packets.iter().map(|p| vec![&p[..20], &p[20..]]).collect();
        let mut chunk_width = 1;
        while chunk_width <= 64 {
            let tree = TurbineMerkleTree::new_from_bufs_vec_par(&bufs_vec, chunk_width);
            assert_eq!(&ref_tree, &tree);
            chunk_width *= 2;
        }
    }

    #[test]
    fn test_merkle_from_buf_vecs_par_widths() {
        for leaf_count in [1, 2, 5, 9, 16, 29, 64, 96, 111, 128] {
            for chunk in [4, 8, 16, 32] {
                if chunk <= leaf_count {
                    test_merkle_from_buf_vecs_par_width(leaf_count, chunk);
                }
            }
        }
    }

    #[test]
    fn test_merkle_prove_verify() {
        let packets = create_random_packets(64);
        let tree = new_from_bufs(&packets[..]);
        {
            let mut buf = vec![0u8; 6 * TURBINE_MERKLE_HASH_BYTES];
            assert_matches!(tree.prove_to_buf(5, &mut buf), Ok(()));
            let mut buf = vec![0u8; 5 * TURBINE_MERKLE_HASH_BYTES];
            assert_matches!(
                tree.prove_to_buf(5, &mut buf),
                Err(Error::InvalidProofBufSize(100))
            );
        }
        {
            let proof1 = tree.prove(11).to_bytes();
            let mut proof2 = vec![0u8; 6 * TURBINE_MERKLE_HASH_BYTES];
            tree.prove_to_buf(11, &mut proof2).unwrap();
            assert_eq!(&proof1, &proof2);
        }
    }
}
