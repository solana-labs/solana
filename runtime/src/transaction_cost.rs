use solana_sdk::pubkey::Pubkey;

const MAX_WRITABLE_ACCOUNTS: usize = 256;

// costs are stored in number of 'compute unit's
#[derive(Debug)]
pub struct TransactionCost {
    pub writable_accounts: Vec<Pubkey>,
    pub signature_cost: u64,
    pub write_lock_cost: u64,
    pub data_bytes_cost: u64,
    pub builtins_execution_cost: u64,
    pub bpf_execution_cost: u64,
    pub account_data_size: u64,
    pub is_simple_vote: bool,
}

impl Default for TransactionCost {
    fn default() -> Self {
        Self {
            writable_accounts: Vec::with_capacity(MAX_WRITABLE_ACCOUNTS),
            signature_cost: 0u64,
            write_lock_cost: 0u64,
            data_bytes_cost: 0u64,
            builtins_execution_cost: 0u64,
            bpf_execution_cost: 0u64,
            account_data_size: 0u64,
            is_simple_vote: false,
        }
    }
}

#[cfg(test)]
impl PartialEq for TransactionCost {
    fn eq(&self, other: &Self) -> bool {
        fn to_hash_set(v: &[Pubkey]) -> std::collections::HashSet<&Pubkey> {
            v.iter().collect()
        }

        self.signature_cost == other.signature_cost
            && self.write_lock_cost == other.write_lock_cost
            && self.data_bytes_cost == other.data_bytes_cost
            && self.builtins_execution_cost == other.builtins_execution_cost
            && self.bpf_execution_cost == other.bpf_execution_cost
            && self.account_data_size == other.account_data_size
            && self.is_simple_vote == other.is_simple_vote
            && to_hash_set(&self.writable_accounts) == to_hash_set(&other.writable_accounts)
    }
}

#[cfg(test)]
impl Eq for TransactionCost {}

impl TransactionCost {
    pub fn new_with_capacity(capacity: usize) -> Self {
        Self {
            writable_accounts: Vec::with_capacity(capacity),
            ..Self::default()
        }
    }

    pub fn new_with_default_capacity() -> Self {
        Self::default()
    }

    pub fn sum(&self) -> u64 {
        self.signature_cost
            .saturating_add(self.write_lock_cost)
            .saturating_add(self.data_bytes_cost)
            .saturating_add(self.builtins_execution_cost)
            .saturating_add(self.bpf_execution_cost)
    }
}
