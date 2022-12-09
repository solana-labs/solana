use {
    crate::immutable_deserialized_packet::ImmutableDeserializedPacket,
    solana_perf::packet::Packet,
    solana_runtime::{
        block_cost_limits,
        cost_model::CostModel,
        cost_tracker::{CostTracker, CostTrackerError},
    },
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
#[derive(Debug)]
pub struct ForwardBatch {
    cost_tracker: CostTracker,
    // `forwardable_packets` keeps forwardable packets in a vector in its
    // original fee prioritized order
    forwardable_packets: Vec<Arc<ImmutableDeserializedPacket>>,
}

impl Default for ForwardBatch {
    /// default ForwardBatch has cost_tracker with default limits
    fn default() -> Self {
        Self::new(1)
    }
}

impl ForwardBatch {
    /// `ForwardBatch` keeps forwardable packets in a vector in its original fee prioritized order,
    /// Number of packets are limited by `cost_tracker` with customized `limit_ratio` to lower
    /// (when `limit_ratio` > 1) `cost_tracker`'s default limits.
    /// Lower limits yield smaller batch for forwarding.
    fn new(limit_ratio: u32) -> Self {
        let mut cost_tracker = CostTracker::default();
        cost_tracker.set_limits(
            block_cost_limits::MAX_WRITABLE_ACCOUNT_UNITS.saturating_div(limit_ratio as u64),
            block_cost_limits::MAX_BLOCK_UNITS.saturating_div(limit_ratio as u64),
            block_cost_limits::MAX_VOTE_UNITS.saturating_div(limit_ratio as u64),
        );
        Self {
            cost_tracker,
            forwardable_packets: Vec::default(),
        }
    }

    fn try_add(
        &mut self,
        sanitized_transaction: &SanitizedTransaction,
        immutable_packet: Arc<ImmutableDeserializedPacket>,
        feature_set: &FeatureSet,
    ) -> Result<u64, CostTrackerError> {
        let tx_cost = CostModel::calculate_cost(sanitized_transaction, feature_set);
        let res = self.cost_tracker.try_add(&tx_cost);
        if res.is_ok() {
            self.forwardable_packets.push(immutable_packet);
        }
        res
    }

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
}

impl ForwardPacketBatchesByAccounts {
    pub fn new_with_default_batch_limits() -> Self {
        Self::new(FORWARDED_BLOCK_COMPUTE_RATIO, DEFAULT_NUMBER_OF_BATCHES)
    }

    pub fn new(limit_ratio: u32, number_of_batches: u32) -> Self {
        let forward_batches = (0..number_of_batches)
            .map(|_| ForwardBatch::new(limit_ratio))
            .collect();
        Self { forward_batches }
    }

    /// packets are filled into first available 'batch' that have space to fit it.
    pub fn try_add_packet(
        &mut self,
        sanitized_transaction: &SanitizedTransaction,
        immutable_packet: Arc<ImmutableDeserializedPacket>,
        feature_set: &FeatureSet,
    ) -> bool {
        for forward_batch in self.forward_batches.iter_mut() {
            if forward_batch
                .try_add(sanitized_transaction, immutable_packet.clone(), feature_set)
                .is_ok()
            {
                return true;
            }
        }
        false
    }

    pub fn iter_batches(&self) -> impl Iterator<Item = &ForwardBatch> {
        self.forward_batches.iter()
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::unprocessed_packet_batches::DeserializedPacket,
        solana_runtime::{
            bank::Bank,
            genesis_utils::{create_genesis_config, GenesisConfigInfo},
            transaction_priority_details::TransactionPriorityDetails,
        },
        solana_sdk::{
            feature_set::FeatureSet, hash::Hash, pubkey::Pubkey, signature::Keypair,
            system_transaction,
        },
        std::sync::Arc,
    };

    fn test_setup() -> (Keypair, Hash) {
        solana_logger::setup();
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(10);
        let bank = Arc::new(Bank::new_no_wallclock_throttle_for_tests(&genesis_config));
        let start_hash = bank.last_blockhash();
        (mint_keypair, start_hash)
    }

    /// build test transaction, return corresponding sanitized_transaction and deserialized_packet,
    /// and the batch limit_ratio that would only allow one transaction per bucket.
    fn build_test_transaction_and_packet(
        priority: u64,
        write_to_account: &Pubkey,
    ) -> (SanitizedTransaction, DeserializedPacket, u32) {
        let (mint_keypair, start_hash) = test_setup();
        let transaction =
            system_transaction::transfer(&mint_keypair, write_to_account, 2, start_hash);
        let sanitized_transaction =
            SanitizedTransaction::from_transaction_for_tests(transaction.clone());
        let tx_cost = CostModel::calculate_cost(&sanitized_transaction, &FeatureSet::all_enabled());
        let cost = tx_cost.sum();
        let deserialized_packet = DeserializedPacket::new_with_priority_details(
            Packet::from_data(None, transaction).unwrap(),
            TransactionPriorityDetails {
                priority,
                compute_unit_limit: cost,
            },
        )
        .unwrap();

        // set limit ratio so each batch can only have one test transaction
        let limit_ratio: u32 =
            ((block_cost_limits::MAX_WRITABLE_ACCOUNT_UNITS - cost + 1) / cost) as u32;
        (sanitized_transaction, deserialized_packet, limit_ratio)
    }

    #[test]
    fn test_try_add_to_forward_batch() {
        let (tx, packet, limit_ratio) =
            build_test_transaction_and_packet(0u64, &Pubkey::new_unique());
        let mut forward_batch = ForwardBatch::new(limit_ratio);

        // Assert first packet will be added to forwarding buffer
        assert!(forward_batch
            .try_add(
                &tx,
                packet.immutable_section().clone(),
                &FeatureSet::all_enabled(),
            )
            .is_ok());
        assert_eq!(1, forward_batch.forwardable_packets.len());

        // Assert second copy of same packet will hit write account limit, therefore
        // not be added to forwarding buffer
        assert!(forward_batch
            .try_add(
                &tx,
                packet.immutable_section().clone(),
                &FeatureSet::all_enabled(),
            )
            .is_err());
        assert_eq!(1, forward_batch.forwardable_packets.len());
    }

    #[test]
    fn test_try_add_packeti_to_multiple_batches() {
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
}
