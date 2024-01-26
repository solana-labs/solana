use {
    crate::{hash::Hash, pubkey::Pubkey},
    serde_derive::{Deserialize, Serialize},
};

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub enum EpochRewardsPartitionDataVersion {
    V0(PartitionData),
}

impl EpochRewardsPartitionDataVersion {
    pub fn get_hasher_kind(&self) -> HasherKind {
        match self {
            EpochRewardsPartitionDataVersion::V0(_) => HasherKind::Sip13,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub enum HasherKind {
    Sip13,
}

/// Data about a rewards partitions for an epoch
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub struct PartitionData {
    /// Number of partitions used for epoch rewards this epoch
    pub num_partitions: usize,
    /// Blockhash of the last block of the previous epoch, used to create EpochRewardsHasher
    pub parent_blockhash: Hash,
}

pub fn get_epoch_rewards_partition_data_address(epoch: u64) -> Pubkey {
    let (address, _bump_seed) = Pubkey::find_program_address(
        &[b"EpochRewards", b"PartitionData", &epoch.to_le_bytes()],
        &crate::sysvar::id(),
    );
    address
}
