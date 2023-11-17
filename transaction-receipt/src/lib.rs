use solana_merkle_tree::MerkleTree;
use solana_program::hash::{hashv, Hash};

pub const RECEIPT_VERSION: u64 = 0x1;

pub const RECEIPT_STATUS_SUCCESS: u8 = 0x0;
pub const RECEIPT_STATUS_FAILURE: u8 = 0x1;

pub const ROOT_PREFIX: &[u8] = &[0x80];
pub const LEAF_PREFIX: &[u8] = &[0x0];

pub const SENTINEL_ROOT: [u8; 32] = [0u8; 32];

#[macro_export]
macro_rules! hash_intermediate_root {
    {$d:ident, $n:expr} => {
        hashv(&[ROOT_PREFIX, $d.as_ref(), $n])
    }
}

#[macro_export]
macro_rules! hash_leaf {
    {$s:expr,$m:expr} => {
        hashv(&[LEAF_PREFIX, RECEIPT_VERSION.to_le_bytes().as_ref(),$s,$m])
    }
}

#[derive(Debug, Default, Clone)]
pub struct TransactionReceiptData {
    pub version: u64,
    pub message_hash: [u8; 32],
    pub status: u8,
}

pub enum ReceiptStatus {
    ReceiptStatusSuccess,
    ReceiptStatusFailure,
}

impl TransactionReceiptData {
    pub fn new(message_hash: [u8; 32], status: ReceiptStatus) -> Self {
        let status = match status {
            ReceiptStatus::ReceiptStatusSuccess => RECEIPT_STATUS_SUCCESS,
            ReceiptStatus::ReceiptStatusFailure => RECEIPT_STATUS_FAILURE,
        };
        Self {
            version: RECEIPT_VERSION,
            message_hash,
            status,
        }
    }

    pub fn get_leaf(&self) -> Hash {
        hash_leaf!(
            self.status.to_le_bytes().as_ref(),
            self.message_hash.as_slice()
        )
    }
}

pub struct TransactionReceiptTree {
    pub leaf_count: usize,
    pub tree: MerkleTree,
}

impl TransactionReceiptTree {
    pub fn new(leaves: Vec<Hash>) -> Self {
        let leaf_count = leaves.len();
        let tree = MerkleTree::new_from_leaves(leaves);
        Self { tree, leaf_count }
    }

    pub fn get_root(&self) -> Hash {
        let maybe_inter_root = self.tree.get_root();

        let final_root = match maybe_inter_root {
            Some(inter_root) => {
                hash_intermediate_root!(inter_root, &u64::to_le_bytes(self.leaf_count as u64))
            }
            None => hash_intermediate_root!(SENTINEL_ROOT, u64::to_le_bytes(0).as_slice()),
        };

        final_root
    }
}

impl Default for TransactionReceiptTree {
    fn default() -> Self {
        let leaves: Vec<Hash> = Vec::default();
        let tree = MerkleTree::new_from_leaves(leaves);
        Self {
            leaf_count: 0,
            tree,
        }
    }
}
