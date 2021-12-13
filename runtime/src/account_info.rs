use crate::{accounts_db::AppendVecId, accounts_index::ZeroLamport};

#[derive(Default, Debug, PartialEq, Clone, Copy)]
pub struct AccountInfo {
    /// index identifying the append storage
    pub store_id: AppendVecId,

    /// offset into the storage
    pub offset: usize,

    /// needed to track shrink candidacy in bytes. Used to update the number
    /// of alive bytes in an AppendVec as newer slots purge outdated entries
    pub stored_size: usize,

    /// lamports in the account used when squashing kept for optimization
    /// purposes to remove accounts with zero balance.
    pub lamports: u64,
}

impl ZeroLamport for AccountInfo {
    fn is_zero_lamport(&self) -> bool {
        self.lamports == 0
    }
}

impl AccountInfo {
    pub fn new(store_id: AppendVecId, offset: usize, stored_size: usize, lamports: u64) -> Self {
        Self {
            store_id,
            offset,
            stored_size,
            lamports,
        }
    }
}
