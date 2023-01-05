use {
    crate::{
        block_metadata_notifier_interface::BlockMetadataNotifier,
        geyser_plugin_manager::GeyserPluginManager,
    },
    log::*,
    solana_geyser_plugin_interface::geyser_plugin_interface::{
        ReplicaBlockInfoV2, ReplicaBlockInfoVersions,
    },
    solana_measure::measure::Measure,
    solana_metrics::*,
    solana_runtime::bank::RewardInfo,
    solana_sdk::{clock::UnixTimestamp, pubkey::Pubkey},
    solana_transaction_status::{Reward, Rewards},
    std::sync::{Arc, RwLock},
};

pub(crate) struct BlockMetadataNotifierImpl {
    plugin_manager: Arc<RwLock<GeyserPluginManager>>,
}

impl BlockMetadataNotifier for BlockMetadataNotifierImpl {
    /// Notify the block metadata
    fn notify_block_metadata(
        &self,
        slot: u64,
        blockhash: &str,
        rewards: &RwLock<Vec<(Pubkey, RewardInfo)>>,
        block_time: Option<UnixTimestamp>,
        block_height: Option<u64>,
        executed_transaction_count: u64,
    ) {
        let mut plugin_manager = self.plugin_manager.write().unwrap();
        if plugin_manager.plugins.is_empty() {
            return;
        }
        let rewards = Self::build_rewards(rewards);

        for plugin in plugin_manager.plugins.iter_mut() {
            let mut measure = Measure::start("geyser-plugin-update-slot");
            let block_info = Self::build_replica_block_info(
                slot,
                blockhash,
                &rewards,
                block_time,
                block_height,
                executed_transaction_count,
            );
            let block_info = ReplicaBlockInfoVersions::V0_0_2(&block_info);
            match plugin.notify_block_metadata(block_info) {
                Err(err) => {
                    error!(
                        "Failed to update block metadata at slot {}, error: {} to plugin {}",
                        slot,
                        err,
                        plugin.name()
                    )
                }
                Ok(_) => {
                    trace!(
                        "Successfully updated block metadata at slot {} to plugin {}",
                        slot,
                        plugin.name()
                    );
                }
            }
            measure.stop();
            inc_new_counter_debug!(
                "geyser-plugin-update-block-metadata-us",
                measure.as_us() as usize,
                1000,
                1000
            );
        }
    }
}

impl BlockMetadataNotifierImpl {
    fn build_rewards(rewards: &RwLock<Vec<(Pubkey, RewardInfo)>>) -> Rewards {
        let rewards = rewards.read().unwrap();
        rewards
            .iter()
            .map(|(pubkey, reward)| Reward {
                pubkey: pubkey.to_string(),
                lamports: reward.lamports,
                post_balance: reward.post_balance,
                reward_type: Some(reward.reward_type),
                commission: reward.commission,
            })
            .collect()
    }

    fn build_replica_block_info<'a>(
        slot: u64,
        blockhash: &'a str,
        rewards: &'a [Reward],
        block_time: Option<UnixTimestamp>,
        block_height: Option<u64>,
        executed_transaction_count: u64,
    ) -> ReplicaBlockInfoV2<'a> {
        ReplicaBlockInfoV2 {
            slot,
            blockhash,
            rewards,
            block_time,
            block_height,
            executed_transaction_count,
        }
    }

    pub fn new(plugin_manager: Arc<RwLock<GeyserPluginManager>>) -> Self {
        Self { plugin_manager }
    }
}
