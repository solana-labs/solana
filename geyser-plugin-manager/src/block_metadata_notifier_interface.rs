use {
    solana_accounts_db::stake_rewards::RewardInfo,
    solana_sdk::{clock::UnixTimestamp, pubkey::Pubkey},
    std::sync::{Arc, RwLock},
};

/// Interface for notifying block metadata changes
pub trait BlockMetadataNotifier {
    /// Notify the block metadata
    #[allow(clippy::too_many_arguments)]
    fn notify_block_metadata(
        &self,
        parent_slot: u64,
        parent_blockhash: &str,
        slot: u64,
        blockhash: &str,
        rewards: &RwLock<Vec<(Pubkey, RewardInfo)>>,
        block_time: Option<UnixTimestamp>,
        block_height: Option<u64>,
        executed_transaction_count: u64,
        entry_count: u64,
    );
}

pub type BlockMetadataNotifierLock = Arc<RwLock<dyn BlockMetadataNotifier + Sync + Send>>;
