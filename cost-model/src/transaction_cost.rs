use {crate::block_cost_limits, solana_sdk::pubkey::Pubkey};

/// TransactionCost is used to represent resources required to process
/// a transaction, denominated in CU (eg. Compute Units).
/// Resources required to process a regular transaction often include
/// an array of variables, such as execution cost, loaded bytes, write
/// lock and read lock etc.
/// SimpleVote has a simpler and pre-determined format: it has 1 or 2 signatures,
/// 2 write locks, a vote instruction and less than 32k (page size) accounts to load.
/// It's cost therefore can be static #33269.
const SIMPLE_VOTE_USAGE_COST: u64 = 3428;

#[derive(Debug)]
pub enum TransactionCost {
    SimpleVote { writable_accounts: Vec<Pubkey> },
    Transaction(UsageCostDetails),
}

impl TransactionCost {
    pub fn sum(&self) -> u64 {
        match self {
            Self::SimpleVote { .. } => SIMPLE_VOTE_USAGE_COST,
            Self::Transaction(usage_cost) => usage_cost.sum(),
        }
    }

    pub fn bpf_execution_cost(&self) -> u64 {
        match self {
            Self::SimpleVote { .. } => 0,
            Self::Transaction(usage_cost) => usage_cost.bpf_execution_cost,
        }
    }

    pub fn is_simple_vote(&self) -> bool {
        match self {
            Self::SimpleVote { .. } => true,
            Self::Transaction(_) => false,
        }
    }

    pub fn data_bytes_cost(&self) -> u64 {
        match self {
            Self::SimpleVote { .. } => 0,
            Self::Transaction(usage_cost) => usage_cost.data_bytes_cost,
        }
    }

    pub fn account_data_size(&self) -> u64 {
        match self {
            Self::SimpleVote { .. } => 0,
            Self::Transaction(usage_cost) => usage_cost.account_data_size,
        }
    }

    pub fn loaded_accounts_data_size_cost(&self) -> u64 {
        match self {
            Self::SimpleVote { .. } => 8, // simple-vote loads less than 32K account data,
            // the cost round up to be one page (32K) cost: 8CU
            Self::Transaction(usage_cost) => usage_cost.loaded_accounts_data_size_cost,
        }
    }

    pub fn signature_cost(&self) -> u64 {
        match self {
            Self::SimpleVote { .. } => block_cost_limits::SIGNATURE_COST,
            Self::Transaction(usage_cost) => usage_cost.signature_cost,
        }
    }

    pub fn write_lock_cost(&self) -> u64 {
        match self {
            Self::SimpleVote { .. } => block_cost_limits::WRITE_LOCK_UNITS.saturating_mul(2),
            Self::Transaction(usage_cost) => usage_cost.write_lock_cost,
        }
    }

    pub fn builtins_execution_cost(&self) -> u64 {
        match self {
            Self::SimpleVote { .. } => solana_vote_program::vote_processor::DEFAULT_COMPUTE_UNITS,
            Self::Transaction(usage_cost) => usage_cost.builtins_execution_cost,
        }
    }

    pub fn writable_accounts(&self) -> &[Pubkey] {
        match self {
            Self::SimpleVote { writable_accounts } => writable_accounts,
            Self::Transaction(usage_cost) => &usage_cost.writable_accounts,
        }
    }
}

const MAX_WRITABLE_ACCOUNTS: usize = 256;

// costs are stored in number of 'compute unit's
#[derive(Debug)]
pub struct UsageCostDetails {
    pub writable_accounts: Vec<Pubkey>,
    pub signature_cost: u64,
    pub write_lock_cost: u64,
    pub data_bytes_cost: u64,
    pub builtins_execution_cost: u64,
    pub bpf_execution_cost: u64,
    pub loaded_accounts_data_size_cost: u64,
    pub account_data_size: u64,
}

impl Default for UsageCostDetails {
    fn default() -> Self {
        Self {
            writable_accounts: Vec::with_capacity(MAX_WRITABLE_ACCOUNTS),
            signature_cost: 0u64,
            write_lock_cost: 0u64,
            data_bytes_cost: 0u64,
            builtins_execution_cost: 0u64,
            bpf_execution_cost: 0u64,
            loaded_accounts_data_size_cost: 0u64,
            account_data_size: 0u64,
        }
    }
}

#[cfg(test)]
impl PartialEq for UsageCostDetails {
    fn eq(&self, other: &Self) -> bool {
        fn to_hash_set(v: &[Pubkey]) -> std::collections::HashSet<&Pubkey> {
            v.iter().collect()
        }

        self.signature_cost == other.signature_cost
            && self.write_lock_cost == other.write_lock_cost
            && self.data_bytes_cost == other.data_bytes_cost
            && self.builtins_execution_cost == other.builtins_execution_cost
            && self.bpf_execution_cost == other.bpf_execution_cost
            && self.loaded_accounts_data_size_cost == other.loaded_accounts_data_size_cost
            && self.account_data_size == other.account_data_size
            && to_hash_set(&self.writable_accounts) == to_hash_set(&other.writable_accounts)
    }
}

#[cfg(test)]
impl Eq for UsageCostDetails {}

impl UsageCostDetails {
    #[cfg(test)]
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
            .saturating_add(self.loaded_accounts_data_size_cost)
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::cost_model::CostModel,
        solana_sdk::{
            feature_set::FeatureSet,
            hash::Hash,
            message::SimpleAddressLoader,
            signer::keypair::Keypair,
            transaction::{MessageHash, SanitizedTransaction, VersionedTransaction},
        },
        solana_vote_program::vote_transaction,
    };

    #[test]
    fn test_vote_transaction_cost() {
        solana_logger::setup();
        let node_keypair = Keypair::new();
        let vote_keypair = Keypair::new();
        let auth_keypair = Keypair::new();
        let transaction = vote_transaction::new_vote_transaction(
            vec![],
            Hash::default(),
            Hash::default(),
            &node_keypair,
            &vote_keypair,
            &auth_keypair,
            None,
        );

        // create a sanitized vote transaction
        let vote_transaction = SanitizedTransaction::try_create(
            VersionedTransaction::from(transaction.clone()),
            MessageHash::Compute,
            Some(true),
            SimpleAddressLoader::Disabled,
        )
        .unwrap();

        // create a identical sanitized transaction, but identified as non-vote
        let none_vote_transaction = SanitizedTransaction::try_create(
            VersionedTransaction::from(transaction),
            MessageHash::Compute,
            Some(false),
            SimpleAddressLoader::Disabled,
        )
        .unwrap();

        // expected vote tx cost: 2 write locks, 1 sig, 1 vote ix, 8cu of loaded accounts size,
        let expected_vote_cost = SIMPLE_VOTE_USAGE_COST;
        // expected non-vote tx cost would include default loaded accounts size cost (16384) additionally
        let expected_none_vote_cost = 20535;

        let vote_cost = CostModel::calculate_cost(&vote_transaction, &FeatureSet::all_enabled());
        let none_vote_cost =
            CostModel::calculate_cost(&none_vote_transaction, &FeatureSet::all_enabled());

        assert_eq!(expected_vote_cost, vote_cost.sum());
        assert_eq!(expected_none_vote_cost, none_vote_cost.sum());
    }
}
