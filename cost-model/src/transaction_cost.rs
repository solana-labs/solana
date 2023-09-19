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
    pub loaded_accounts_data_size_cost: u64,
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
            loaded_accounts_data_size_cost: 0u64,
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
            && self.loaded_accounts_data_size_cost == other.loaded_accounts_data_size_cost
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
        if self.is_simple_vote {
            self.signature_cost
                .saturating_add(self.write_lock_cost)
                .saturating_add(self.data_bytes_cost)
                .saturating_add(self.builtins_execution_cost)
        } else {
            self.signature_cost
                .saturating_add(self.write_lock_cost)
                .saturating_add(self.data_bytes_cost)
                .saturating_add(self.builtins_execution_cost)
                .saturating_add(self.bpf_execution_cost)
                .saturating_add(self.loaded_accounts_data_size_cost)
        }
    }
}

#[cfg(test)]
mod tests {
    use {
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

        // expected vote tx cost: 2 write locks, 2 sig, 1 vite ix, and 11 CU tx data cost
        let expected_vote_cost = 4151;
        // expected non-vote tx cost would include default loaded accounts size cost (16384) additionally
        let expected_none_vote_cost = 20535;

        let vote_cost = CostModel::calculate_cost(&vote_transaction, &FeatureSet::all_enabled());
        let none_vote_cost =
            CostModel::calculate_cost(&none_vote_transaction, &FeatureSet::all_enabled());

        assert_eq!(expected_vote_cost, vote_cost.sum());
        assert_eq!(expected_none_vote_cost, none_vote_cost.sum());
    }
}
