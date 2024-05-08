use {
    super::immutable_deserialized_packet::ImmutableDeserializedPacket,
    solana_cost_model::{
        block_cost_limits,
        cost_model::CostModel,
        cost_tracker::{CostTracker, UpdatedCosts},
        transaction_cost::TransactionCost,
    },
    solana_perf::packet::Packet,
    solana_sdk::{feature_set::FeatureSet, transaction::SanitizedTransaction},
    std::sync::Arc,
};

/// `ForwardBatch` to have half of default cost_tracker limits, as smaller batch
/// allows better granularity in composing forwarding transactions; e.g.,
/// transactions in each batch are potentially more evenly distributed across accounts.
const FORWARDED_BLOCK_COMPUTE_RATIO: u32 = 2;
/// this number divided by`FORWARDED_BLOCK_COMPUTE_RATIO` is the total blocks to forward.
/// To accommodate transactions without `compute_budget` instruction, which will
/// have default 200_000 compute units, it has 100 batches as default to forward
/// up to 12_000 such transaction. (120 such transactions fill up a batch, 100
/// batches allows 12_000 transactions)
const DEFAULT_NUMBER_OF_BATCHES: u32 = 100;

/// `ForwardBatch` represents one forwardable batch of transactions with a
/// limited number of total compute units
#[derive(Debug, Default)]
pub struct ForwardBatch {
    // `forwardable_packets` keeps forwardable packets in a vector in its
    // original fee prioritized order
    forwardable_packets: Vec<Arc<ImmutableDeserializedPacket>>,
}

impl ForwardBatch {
    pub fn get_forwardable_packets(&self) -> impl Iterator<Item = &Packet> {
        self.forwardable_packets
            .iter()
            .map(|immutable_packet| immutable_packet.original_packet())
    }

    pub fn len(&self) -> usize {
        self.forwardable_packets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.forwardable_packets.is_empty()
    }

    pub fn reset(&mut self) {
        self.forwardable_packets.clear();
    }
}

/// To avoid forward queue being saturated by transactions for single hot account,
/// the forwarder will group and send prioritized transactions by account limit
/// to allow transactions on non-congested accounts to be forwarded alongside higher fee
/// transactions that saturate those highly demanded accounts.
#[derive(Debug)]
pub struct ForwardPacketBatchesByAccounts {
    // Forwardable packets are staged in number of batches, each batch is limited
    // by cost_tracker on both account limit and block limits. Those limits are
    // set as `limit_ratio` of regular block limits to facilitate quicker iteration.
    forward_batches: Vec<ForwardBatch>,

    // Single cost tracker that imposes the total cu limits for forwarding.
    cost_tracker: CostTracker,

    // Compute Unit limits for each batch
    batch_vote_limit: u64,
    batch_block_limit: u64,
    batch_account_limit: u64,
}

impl ForwardPacketBatchesByAccounts {
    pub fn new_with_default_batch_limits() -> Self {
        Self::new(FORWARDED_BLOCK_COMPUTE_RATIO, DEFAULT_NUMBER_OF_BATCHES)
    }

    pub fn new(limit_ratio: u32, number_of_batches: u32) -> Self {
        let forward_batches = (0..number_of_batches)
            .map(|_| ForwardBatch::default())
            .collect();

        let batch_vote_limit = block_cost_limits::MAX_VOTE_UNITS.saturating_div(limit_ratio as u64);
        let batch_block_limit =
            block_cost_limits::MAX_BLOCK_UNITS.saturating_div(limit_ratio as u64);
        let batch_account_limit =
            block_cost_limits::MAX_WRITABLE_ACCOUNT_UNITS.saturating_div(limit_ratio as u64);

        let mut cost_tracker = CostTracker::default();
        cost_tracker.set_limits(
            batch_account_limit.saturating_mul(number_of_batches as u64),
            batch_block_limit.saturating_mul(number_of_batches as u64),
            batch_vote_limit.saturating_mul(number_of_batches as u64),
        );
        Self {
            forward_batches,
            cost_tracker,
            batch_vote_limit,
            batch_block_limit,
            batch_account_limit,
        }
    }

    pub fn try_add_packet(
        &mut self,
        sanitized_transaction: &SanitizedTransaction,
        immutable_packet: Arc<ImmutableDeserializedPacket>,
        feature_set: &FeatureSet,
    ) -> bool {
        let tx_cost = CostModel::calculate_cost(sanitized_transaction, feature_set);

        if let Ok(updated_costs) = self.cost_tracker.try_add(&tx_cost) {
            let batch_index = self.get_batch_index_by_updated_costs(&tx_cost, &updated_costs);

            if let Some(forward_batch) = self.forward_batches.get_mut(batch_index) {
                forward_batch.forwardable_packets.push(immutable_packet);
            } else {
                // A successfully added tx_cost means it does not exceed block limit, nor vote
                // limit, nor account limit. batch_index calculated as quotient from division
                // will not be out of bounds.
                unreachable!("batch_index out of bounds");
            }
            true
        } else {
            false
        }
    }

    pub fn iter_batches(&self) -> impl Iterator<Item = &ForwardBatch> {
        self.forward_batches.iter()
    }

    pub fn reset(&mut self) {
        for forward_batch in self.forward_batches.iter_mut() {
            forward_batch.reset();
        }

        self.cost_tracker.reset();
    }

    // Successfully added packet should be placed into the batch where no block/vote/account limits
    // would be exceeded. Eg, if by block limit, it can be put into batch #1; by vote limit, it can
    // be put into batch #2; and by account limit, it can be put into batch #3; then it should be
    // put into batch #3 to satisfy all batch limits.
    fn get_batch_index_by_updated_costs(
        &self,
        tx_cost: &TransactionCost,
        updated_costs: &UpdatedCosts,
    ) -> usize {
        let Some(batch_index_by_block_limit) =
            updated_costs.updated_block_cost.checked_div(match tx_cost {
                TransactionCost::SimpleVote { .. } => self.batch_vote_limit,
                TransactionCost::Transaction(_) => self.batch_block_limit,
            })
        else {
            unreachable!("batch vote limit or block limit must not be zero")
        };

        let batch_index_by_account_limit =
            updated_costs.updated_costliest_account_cost / self.batch_account_limit;

        batch_index_by_block_limit.max(batch_index_by_account_limit) as usize
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::banking_stage::unprocessed_packet_batches::DeserializedPacket,
        solana_cost_model::transaction_cost::UsageCostDetails,
        solana_sdk::{
            compute_budget::ComputeBudgetInstruction, feature_set::FeatureSet, message::Message,
            pubkey::Pubkey, system_instruction, transaction::Transaction,
        },
    };

    /// build test transaction, return corresponding sanitized_transaction and deserialized_packet,
    /// and the batch limit_ratio that would only allow one transaction per bucket.
    fn build_test_transaction_and_packet(
        priority: u64,
        write_to_account: &Pubkey,
    ) -> (SanitizedTransaction, DeserializedPacket, u32) {
        let from_account = solana_sdk::pubkey::new_rand();

        let transaction = Transaction::new_unsigned(Message::new(
            &[
                ComputeBudgetInstruction::set_compute_unit_price(priority),
                system_instruction::transfer(&from_account, write_to_account, 2),
            ],
            Some(&from_account),
        ));
        let sanitized_transaction =
            SanitizedTransaction::from_transaction_for_tests(transaction.clone());
        let tx_cost = CostModel::calculate_cost(&sanitized_transaction, &FeatureSet::all_enabled());
        let cost = tx_cost.sum();
        let deserialized_packet =
            DeserializedPacket::new(Packet::from_data(None, transaction).unwrap()).unwrap();

        // set limit ratio so each batch can only have one test transaction
        let limit_ratio: u32 =
            ((block_cost_limits::MAX_WRITABLE_ACCOUNT_UNITS - cost + 1) / cost) as u32;
        (sanitized_transaction, deserialized_packet, limit_ratio)
    }

    #[test]
    fn test_try_add_packet_to_multiple_batches() {
        // setup two transactions, one has high priority that writes to hot account, the
        // other write to non-contentious account with no priority
        let hot_account = solana_sdk::pubkey::new_rand();
        let other_account = solana_sdk::pubkey::new_rand();
        let (tx_high_priority, packet_high_priority, limit_ratio) =
            build_test_transaction_and_packet(10, &hot_account);
        let (tx_low_priority, packet_low_priority, _) =
            build_test_transaction_and_packet(0, &other_account);

        // setup forwarding with 2 buckets, each only allow one transaction
        let number_of_batches = 2;
        let mut forward_packet_batches_by_accounts =
            ForwardPacketBatchesByAccounts::new(limit_ratio, number_of_batches);

        // Assert initially both batches are empty
        {
            let mut batches = forward_packet_batches_by_accounts.iter_batches();
            assert_eq!(0, batches.next().unwrap().len());
            assert_eq!(0, batches.next().unwrap().len());
            assert!(batches.next().is_none());
        }

        // Assert one high-priority packet will be added to 1st bucket successfully
        {
            assert!(forward_packet_batches_by_accounts.try_add_packet(
                &tx_high_priority,
                packet_high_priority.immutable_section().clone(),
                &FeatureSet::all_enabled(),
            ));
            let mut batches = forward_packet_batches_by_accounts.iter_batches();
            assert_eq!(1, batches.next().unwrap().len());
            assert_eq!(0, batches.next().unwrap().len());
            assert!(batches.next().is_none());
        }

        // Assert second high-priority packet will not fit in first bucket, but will
        // be added to 2nd bucket
        {
            assert!(forward_packet_batches_by_accounts.try_add_packet(
                &tx_high_priority,
                packet_high_priority.immutable_section().clone(),
                &FeatureSet::all_enabled(),
            ));
            let mut batches = forward_packet_batches_by_accounts.iter_batches();
            assert_eq!(1, batches.next().unwrap().len());
            assert_eq!(1, batches.next().unwrap().len());
        }

        // Assert 3rd high-priority packet can not added since both buckets would
        // exceed hot-account limit
        {
            assert!(!forward_packet_batches_by_accounts.try_add_packet(
                &tx_high_priority,
                packet_high_priority.immutable_section().clone(),
                &FeatureSet::all_enabled(),
            ));
            let mut batches = forward_packet_batches_by_accounts.iter_batches();
            assert_eq!(1, batches.next().unwrap().len());
            assert_eq!(1, batches.next().unwrap().len());
            assert!(batches.next().is_none());
        }

        // Assert lower priority packet will be successfully added to first bucket
        // since non-contentious account is still free
        {
            assert!(forward_packet_batches_by_accounts.try_add_packet(
                &tx_low_priority,
                packet_low_priority.immutable_section().clone(),
                &FeatureSet::all_enabled(),
            ));
            let mut batches = forward_packet_batches_by_accounts.iter_batches();
            assert_eq!(2, batches.next().unwrap().len());
            assert_eq!(1, batches.next().unwrap().len());
            assert!(batches.next().is_none());
        }
    }

    #[test]
    fn test_try_add_packet_to_single_batch() {
        let (tx, packet, limit_ratio) =
            build_test_transaction_and_packet(10, &solana_sdk::pubkey::new_rand());
        let number_of_batches = 1;
        let mut forward_packet_batches_by_accounts =
            ForwardPacketBatchesByAccounts::new(limit_ratio, number_of_batches);

        // Assert initially batch is empty, and accepting new packets
        {
            let mut batches = forward_packet_batches_by_accounts.iter_batches();
            assert_eq!(0, batches.next().unwrap().len());
            assert!(batches.next().is_none());
        }

        // Assert can successfully add first packet to forwarding buffer
        {
            assert!(forward_packet_batches_by_accounts.try_add_packet(
                &tx,
                packet.immutable_section().clone(),
                &FeatureSet::all_enabled()
            ));

            let mut batches = forward_packet_batches_by_accounts.iter_batches();
            assert_eq!(1, batches.next().unwrap().len());
        }

        // Assert cannot add same packet to forwarding buffer again, due to reached account limit;
        {
            assert!(!forward_packet_batches_by_accounts.try_add_packet(
                &tx,
                packet.immutable_section().clone(),
                &FeatureSet::all_enabled()
            ));

            let mut batches = forward_packet_batches_by_accounts.iter_batches();
            assert_eq!(1, batches.next().unwrap().len());
        }

        // Assert can still add non-contentious packet to same batch
        {
            // build a small packet to a non-contentious account with high priority
            let (tx2, packet2, _) =
                build_test_transaction_and_packet(100, &solana_sdk::pubkey::new_rand());

            assert!(forward_packet_batches_by_accounts.try_add_packet(
                &tx2,
                packet2.immutable_section().clone(),
                &FeatureSet::all_enabled()
            ));

            let mut batches = forward_packet_batches_by_accounts.iter_batches();
            assert_eq!(2, batches.next().unwrap().len());
        }
    }

    #[test]
    fn test_get_batch_index_by_updated_costs() {
        let test_cost = 99;

        // check against vote limit only
        {
            let mut forward_packet_batches_by_accounts =
                ForwardPacketBatchesByAccounts::new_with_default_batch_limits();
            forward_packet_batches_by_accounts.batch_vote_limit = test_cost + 1;

            let transaction_cost = TransactionCost::SimpleVote {
                writable_accounts: vec![],
            };
            assert_eq!(
                0,
                forward_packet_batches_by_accounts.get_batch_index_by_updated_costs(
                    &transaction_cost,
                    &UpdatedCosts {
                        updated_block_cost: test_cost,
                        updated_costliest_account_cost: 0
                    }
                )
            );
            assert_eq!(
                1,
                forward_packet_batches_by_accounts.get_batch_index_by_updated_costs(
                    &transaction_cost,
                    &UpdatedCosts {
                        updated_block_cost: test_cost + 1,
                        updated_costliest_account_cost: 0
                    }
                )
            );
        }

        // check against block limit only
        {
            let mut forward_packet_batches_by_accounts =
                ForwardPacketBatchesByAccounts::new_with_default_batch_limits();
            forward_packet_batches_by_accounts.batch_block_limit = test_cost + 1;

            let transaction_cost = TransactionCost::Transaction(UsageCostDetails::default());
            assert_eq!(
                0,
                forward_packet_batches_by_accounts.get_batch_index_by_updated_costs(
                    &transaction_cost,
                    &UpdatedCosts {
                        updated_block_cost: test_cost,
                        updated_costliest_account_cost: 0
                    }
                )
            );
            assert_eq!(
                1,
                forward_packet_batches_by_accounts.get_batch_index_by_updated_costs(
                    &transaction_cost,
                    &UpdatedCosts {
                        updated_block_cost: test_cost + 1,
                        updated_costliest_account_cost: 0
                    }
                )
            );
        }

        // check against account limit only
        {
            let mut forward_packet_batches_by_accounts =
                ForwardPacketBatchesByAccounts::new_with_default_batch_limits();
            forward_packet_batches_by_accounts.batch_account_limit = test_cost + 1;

            let transaction_cost = TransactionCost::Transaction(UsageCostDetails::default());
            assert_eq!(
                0,
                forward_packet_batches_by_accounts.get_batch_index_by_updated_costs(
                    &transaction_cost,
                    &UpdatedCosts {
                        updated_block_cost: 0,
                        updated_costliest_account_cost: test_cost
                    }
                )
            );
            assert_eq!(
                1,
                forward_packet_batches_by_accounts.get_batch_index_by_updated_costs(
                    &transaction_cost,
                    &UpdatedCosts {
                        updated_block_cost: 0,
                        updated_costliest_account_cost: test_cost + 1
                    }
                )
            );
        }

        // by block limit, it can be put into batch #1;
        // by vote limit, it can be put into batch #2;
        // by account limit, it can be put into batch #3;
        // it should be put into batch #3 to satisfy all batch limits.
        {
            let mut forward_packet_batches_by_accounts =
                ForwardPacketBatchesByAccounts::new_with_default_batch_limits();
            forward_packet_batches_by_accounts.batch_block_limit = test_cost + 1;
            forward_packet_batches_by_accounts.batch_vote_limit = test_cost / 2 + 1;
            forward_packet_batches_by_accounts.batch_account_limit = test_cost / 3 + 1;

            let transaction_cost = TransactionCost::Transaction(UsageCostDetails::default());
            assert_eq!(
                2,
                forward_packet_batches_by_accounts.get_batch_index_by_updated_costs(
                    &transaction_cost,
                    &UpdatedCosts {
                        updated_block_cost: test_cost,
                        updated_costliest_account_cost: test_cost
                    }
                )
            );
        }
    }
}
