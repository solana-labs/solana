use {
    crate::block_cost_limits,
    solana_sdk::{message::TransactionSignatureDetails, pubkey::Pubkey},
    solana_svm_transaction::svm_message::SVMMessage,
};

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
pub enum TransactionCost<'a, Tx: SVMMessage> {
    SimpleVote { transaction: &'a Tx },
    Transaction(UsageCostDetails<'a, Tx>),
}

impl<'a, Tx: SVMMessage> TransactionCost<'a, Tx> {
    pub fn sum(&self) -> u64 {
        #![allow(clippy::assertions_on_constants)]
        match self {
            Self::SimpleVote { .. } => {
                const _: () = assert!(
                    SIMPLE_VOTE_USAGE_COST
                        == solana_vote_program::vote_processor::DEFAULT_COMPUTE_UNITS
                            + block_cost_limits::SIGNATURE_COST
                            + 2 * block_cost_limits::WRITE_LOCK_UNITS
                            + 8
                );

                SIMPLE_VOTE_USAGE_COST
            }
            Self::Transaction(usage_cost) => usage_cost.sum(),
        }
    }

    pub fn programs_execution_cost(&self) -> u64 {
        match self {
            Self::SimpleVote { .. } => solana_vote_program::vote_processor::DEFAULT_COMPUTE_UNITS,
            Self::Transaction(usage_cost) => usage_cost.programs_execution_cost,
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

    pub fn allocated_accounts_data_size(&self) -> u64 {
        match self {
            Self::SimpleVote { .. } => 0,
            Self::Transaction(usage_cost) => usage_cost.allocated_accounts_data_size,
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

    pub fn writable_accounts(&self) -> impl Iterator<Item = &Pubkey> {
        let transaction = match self {
            Self::SimpleVote { transaction } => transaction,
            Self::Transaction(usage_cost) => usage_cost.transaction,
        };
        transaction
            .account_keys()
            .iter()
            .enumerate()
            .filter_map(|(index, key)| transaction.is_writable(index).then_some(key))
    }

    pub fn num_transaction_signatures(&self) -> u64 {
        match self {
            Self::SimpleVote { .. } => 1,
            Self::Transaction(usage_cost) => {
                usage_cost.signature_details.num_transaction_signatures()
            }
        }
    }

    pub fn num_secp256k1_instruction_signatures(&self) -> u64 {
        match self {
            Self::SimpleVote { .. } => 0,
            Self::Transaction(usage_cost) => usage_cost
                .signature_details
                .num_secp256k1_instruction_signatures(),
        }
    }

    pub fn num_ed25519_instruction_signatures(&self) -> u64 {
        match self {
            Self::SimpleVote { .. } => 0,
            Self::Transaction(usage_cost) => usage_cost
                .signature_details
                .num_ed25519_instruction_signatures(),
        }
    }
}

// costs are stored in number of 'compute unit's
#[derive(Debug)]
pub struct UsageCostDetails<'a, Tx: SVMMessage> {
    pub transaction: &'a Tx,
    pub signature_cost: u64,
    pub write_lock_cost: u64,
    pub data_bytes_cost: u64,
    pub programs_execution_cost: u64,
    pub loaded_accounts_data_size_cost: u64,
    pub allocated_accounts_data_size: u64,
    pub signature_details: TransactionSignatureDetails,
}

impl<'a, Tx: SVMMessage> UsageCostDetails<'a, Tx> {
    pub fn sum(&self) -> u64 {
        self.signature_cost
            .saturating_add(self.write_lock_cost)
            .saturating_add(self.data_bytes_cost)
            .saturating_add(self.programs_execution_cost)
            .saturating_add(self.loaded_accounts_data_size_cost)
    }
}

#[cfg(feature = "dev-context-only-utils")]
#[derive(Debug)]
pub struct WritableKeysTransaction(pub Vec<Pubkey>);

#[cfg(feature = "dev-context-only-utils")]
impl SVMMessage for WritableKeysTransaction {
    fn num_total_signatures(&self) -> u64 {
        unimplemented!("WritableKeysTransaction::num_total_signatures")
    }

    fn num_write_locks(&self) -> u64 {
        unimplemented!("WritableKeysTransaction::num_write_locks")
    }

    fn recent_blockhash(&self) -> &solana_sdk::hash::Hash {
        unimplemented!("WritableKeysTransaction::recent_blockhash")
    }

    fn num_instructions(&self) -> usize {
        unimplemented!("WritableKeysTransaction::num_instructions")
    }

    fn instructions_iter(
        &self,
    ) -> impl Iterator<Item = solana_svm_transaction::instruction::SVMInstruction> {
        core::iter::empty()
    }

    fn program_instructions_iter(
        &self,
    ) -> impl Iterator<Item = (&Pubkey, solana_svm_transaction::instruction::SVMInstruction)> {
        core::iter::empty()
    }

    fn account_keys(&self) -> solana_sdk::message::AccountKeys {
        solana_sdk::message::AccountKeys::new(&self.0, None)
    }

    fn fee_payer(&self) -> &Pubkey {
        unimplemented!("WritableKeysTransaction::fee_payer")
    }

    fn is_writable(&self, _index: usize) -> bool {
        true
    }

    fn is_signer(&self, _index: usize) -> bool {
        unimplemented!("WritableKeysTransaction::is_signer")
    }

    fn is_invoked(&self, _key_index: usize) -> bool {
        unimplemented!("WritableKeysTransaction::is_invoked")
    }

    fn num_lookup_tables(&self) -> usize {
        unimplemented!("WritableKeysTransaction::num_lookup_tables")
    }

    fn message_address_table_lookups(
        &self,
    ) -> impl Iterator<
        Item = solana_svm_transaction::message_address_table_lookup::SVMMessageAddressTableLookup,
    > {
        core::iter::empty()
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::cost_model::CostModel,
        solana_feature_set::FeatureSet,
        solana_sdk::{
            hash::Hash,
            message::SimpleAddressLoader,
            reserved_account_keys::ReservedAccountKeys,
            signer::keypair::Keypair,
            transaction::{MessageHash, SanitizedTransaction, VersionedTransaction},
        },
        solana_vote_program::{vote_state::TowerSync, vote_transaction},
    };

    #[test]
    fn test_vote_transaction_cost() {
        solana_logger::setup();
        let node_keypair = Keypair::new();
        let vote_keypair = Keypair::new();
        let auth_keypair = Keypair::new();
        let transaction = vote_transaction::new_tower_sync_transaction(
            TowerSync::default(),
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
            &ReservedAccountKeys::empty_key_set(),
        )
        .unwrap();

        // create a identical sanitized transaction, but identified as non-vote
        let none_vote_transaction = SanitizedTransaction::try_create(
            VersionedTransaction::from(transaction),
            MessageHash::Compute,
            Some(false),
            SimpleAddressLoader::Disabled,
            &ReservedAccountKeys::empty_key_set(),
        )
        .unwrap();

        // expected vote tx cost: 2 write locks, 1 sig, 1 vote ix, 8cu of loaded accounts size,
        let expected_vote_cost = SIMPLE_VOTE_USAGE_COST;
        // expected non-vote tx cost would include default loaded accounts size cost (16384) additionally
        let expected_none_vote_cost = 20543;

        let vote_cost = CostModel::calculate_cost(&vote_transaction, &FeatureSet::all_enabled());
        let none_vote_cost =
            CostModel::calculate_cost(&none_vote_transaction, &FeatureSet::all_enabled());

        assert_eq!(expected_vote_cost, vote_cost.sum());
        assert_eq!(expected_none_vote_cost, none_vote_cost.sum());
    }
}
