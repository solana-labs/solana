use {
    firedancer_sys::ballet::{
        fd_bmtree32_commit_append, fd_bmtree32_commit_fini, fd_bmtree32_commit_t,
        fd_bmtree32_hash_leaf, fd_bmtree32_node,
    },
    log::warn,
    solana_sdk::hash::Hash,
    std::{mem, os::raw::c_void, slice::from_raw_parts},
};

pub fn hash_leaf(node: &mut fd_bmtree32_node, data: &mut &[&[u8]]) {
    // &mut &[&[u8]; 2])
    unsafe {
        fd_bmtree32_hash_leaf(
            node,
            data as *mut _ as *mut c_void,
            mem::size_of::<&[&[u8]; 2]>().try_into().unwrap(),
        )
    };
}

pub struct ReceiptTree {
    pub tree: fd_bmtree32_commit_t,
    pub root: [u8; 32],
}
impl ReceiptTree {
    /// Creates an empty tree
    pub fn new() -> Self {
        let mut nodes = vec![];
        for _ in 0..63 {
            let n = fd_bmtree32_node { hash: [0u8; 32] };
            nodes.push(n);
        }
        let state = fd_bmtree32_commit_t {
            leaf_cnt: 0,
            __bindgen_padding_0: [0u64; 3],
            node_buf: convert_to_array(nodes),
        };

        ReceiptTree {
            tree: state,
            root: [0u8; 32],
        }
    }
    // Append just one leaf to the tree
    pub fn append_leaf(&mut self, mut leaf: &[&[u8]]) {
        let mut leaf_node = fd_bmtree32_node { hash: [0u8; 32] };
        // let mut k = &[data[i].0.as_slice(), &data[i].1.to_be_bytes()];
        hash_leaf(&mut leaf_node, &mut leaf);
        unsafe {
            fd_bmtree32_commit_append(&mut self.tree, &leaf_node, 1);
        }
    }
    /// Finalise the commitment and get the 32 byte root
    pub fn get_root(&mut self) -> Result<Hash, TryFromSliceError> {
        let byte = unsafe { fd_bmtree32_commit_fini(&mut self.tree) };
        let root = unsafe { from_raw_parts(byte, 32) };
        match slice_to_array_32(root) {
            Ok(hash) => Ok(Hash::new_from_array(*hash)),
            Err(e) => Err(e),
        }
    }
}
fn convert_to_array<T>(v: Vec<T>) -> [T; 63] {
    v.try_into()
        .unwrap_or_else(|v: Vec<T>| panic!("Expected a Vec of length 63 but it was {}", v.len()))
}

#[derive(Debug)]
pub struct TryFromSliceError(String);

fn slice_to_array_32<T>(slice: &[T]) -> Result<&[T; 32], TryFromSliceError> {
    if slice.len() <= 32 {
        let ptr = slice.as_ptr() as *const [T; 32];
        unsafe { Ok(&*ptr) }
    } else {
        Err(TryFromSliceError(format!(
            "The array was expected to be of length 32 but is actually {}",
            slice.len()
        )))
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        rand::{self, Rng},
        solana_sdk::signature::Signature,
    };

    #[test]
    fn create_receipt_tree_for_one() {
        let mut sigs = vec![];
        let mut statuses = vec![];
        for _ in 0..1 {
            sigs.push(Signature::new_unique().to_string().as_bytes().to_owned());
            statuses.push(rand::thread_rng().gen_range(0..2) as u8);
        }
        let data: Vec<(Vec<u8>, u8)> = sigs.into_iter().zip(statuses.clone().into_iter()).collect();
        let mut receipt_tree = ReceiptTree::new();
        receipt_tree.append_leaf(&[data[0].0.as_slice(), &data[0].1.to_be_bytes()]);
        let root = receipt_tree.get_root();
        match root {
            Ok(hash) => {
                println!("Hash: {:?}", hash.to_string());
            }
            Err(e) => {
                println!("Error: {:?}", e);
            }
        }
    }
    #[test]
    fn create_receipt_tree_for_many() {
        let mut sigs = vec![];
        let mut statuses = vec![];
        for _ in 0..100000 {
            sigs.push(Signature::new_unique().to_string().as_bytes().to_owned());
            statuses.push(rand::thread_rng().gen_range(0..2) as u8);
        }
        let data: Vec<(Vec<u8>, u8)> = sigs.into_iter().zip(statuses.clone().into_iter()).collect();
        let mut receipt_tree = ReceiptTree::new();
        for i in 0..data.len() {
            receipt_tree.append_leaf(&[data[i].0.as_slice(), &data[i].1.to_be_bytes()]);
        }

        let root = receipt_tree.get_root();
        match root {
            Ok(hash) => {
                println!("Hash: {:?}", hash.to_string());
            }
            Err(e) => {
                println!("Error: {:?}", e);
            }
        }
    }
}
