use {
    rand::{Rng, SeedableRng},
    sha2::{Digest, Sha256},
};

//type TurbineMerkleHash = [u8; 1];

#[derive(Default, Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TurbineMerkleHash(pub [u8; 1]);

pub type TurbineMerkleProof = Vec<TurbineMerkleHash>;

pub struct TurbineMerkleTree {
    tree: Vec<TurbineMerkleHash>,
    nleaves: usize,
}

impl TurbineMerkleTree {
    pub fn new(_leaf_count: usize) -> Self {
        Self {
            tree: Vec::with_capacity(16 * 2 - 1),
            nleaves: 16,
        }
    }
}

impl TurbineMerkleHash {
    pub fn hash(bufs: &[&[u8]]) -> TurbineMerkleHash {
        let mut hasher = Sha256::new();
        for b in bufs {
            hasher.update(b);
        }
        let h = hasher.finalize();
        let mut ret = TurbineMerkleHash::default();
        let len = ret.0.len();
        ret.0[..].copy_from_slice(&h.as_slice()[0..len]);
        ret
    }
}

fn qhash(bufs: &[&[u8]]) -> TurbineMerkleHash {
    let mut hasher = Sha256::new();
    for b in bufs {
        hasher.update(b);
    }
    let h = hasher.finalize();
    //let mut ret = [0u8; 1];
    let mut ret = TurbineMerkleHash::default();
    let len = ret.0.len();
    ret.0[..].copy_from_slice(&h.as_slice()[0..len]);
    ret
}

// leaves padded to power of 2
fn gen_tree(leaves: &Vec<TurbineMerkleHash>) -> Vec<TurbineMerkleHash> {
    let tree_size = leaves.len() * 2 - 1;
    let mut tree = Vec::with_capacity(tree_size);

    println!("--- gen tree ---");

    for i in 0..leaves.len() {
        //tree[i] = leaves[i];
        tree.push(leaves[i]);
    }

    let mut base = 0;
    let mut level_leaves = leaves.len();
    while level_leaves > 1 {
        for i in (0..level_leaves).step_by(2) {
            let hash = qhash(&[&tree[base + i].0, &tree[base + i + 1].0]);
            //tree[base+i/2] = hash;
            println!("push({:?})", &hash);
            tree.push(hash);
        }
        println!("");
        base += level_leaves;
        level_leaves /= 2;
    }

    tree
}

fn gen_proof(tree: &Vec<TurbineMerkleHash>, nleaves: usize, idx: usize) -> TurbineMerkleProof {
    let mut proof = Vec::new();
    let mut level_leaves = nleaves;
    let mut i = idx;
    let mut base = 0;
    while level_leaves > 1 {
        if i % 2 == 0 {
            proof.push(tree[base + i + 1]);
        } else {
            proof.push(tree[base + i - 1]);
        }
        base += level_leaves;
        i /= 2;
        level_leaves /= 2;
    }
    proof
}

fn check_proof(
    proof: &TurbineMerkleProof,
    root: &TurbineMerkleHash,
    start: &TurbineMerkleHash,
    idx: usize,
) -> bool {
    println!("--- proving {:?} for {:?} --- ", start, root);
    let mut hash = start.clone();
    let mut j = idx;
    for i in 0..proof.len() {
        hash = if j % 2 == 0 {
            qhash(&[&hash.0, &proof[i].0])
        } else {
            qhash(&[&proof[i].0, &hash.0])
        };
        println!("{:?}", &hash);
        j /= 2;
    }
    &hash == root
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_random_packets() -> Vec<Vec<u8>> {
        let rng = rand::thread_rng();
        let mut packets = Vec::default();
        for i in 0..16 {
            let buf: Vec<u8> = (0..1024).map(|_| rand::random::<u8>()).collect();
            packets.push(buf);
        }
        packets
    }

    #[test]
    fn test_merkle() {
        let packets = create_random_packets();
        let leaves: Vec<TurbineMerkleHash> = packets.iter().map(|p| qhash(&[&p])).collect();
        let tree = gen_tree(&leaves);

        let mut base = 0;
        let mut nleaves = 16;
        while nleaves > 0 {
            println!("{:?}", &tree[base..base + nleaves]);
            base += nleaves;
            nleaves /= 2;
        }

        println!("tree: {:?}", &tree);

        let root = tree[tree.len() - 1];
        println!("root: {:?}", &root);

        let proof5 = gen_proof(&tree, 16, 5);
        println!("proof5: {:?}", &proof5);

        let res = check_proof(&proof5, &root, &leaves[5], 5);
        println!("res: {}", res);

        assert!(res);
    }
}
