use {
    solana_runtime::bank::RewardInfo,
    solana_sdk::{clock::UnixTimestamp, pubkey::Pubkey},
    std::sync::{Arc, RwLock},
};

/// Interface for notifying block metadata changes
pub trait BlockMetadataNotifier {
    /// Notify the block metadata
    fn notify_block_metadata(
        &self,
        slot: u64,
        blockhash: &str,
        rewards: &RwLock<Vec<(Pubkey, RewardInfo)>>,
        block_time: Option<UnixTimestamp>,
        block_height: Option<u64>,
        executed_transaction_count: u64,
    );
}

pub type BlockMetadataNotifierLock = Arc<RwLock<dyn BlockMetadataNotifier + Sync + Send>>;
