use {
    crate::{hash::Hash, pubkey::Pubkey},
    serde_derive::{Deserialize, Serialize},
};

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub enum EpochRewardsPartitionDataVersion {
    V0(PartitionData),
}

#[repr(u8)]
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
    /// Kind of hasher used to generate partitions
    pub hasher_kind: HasherKind,
}

pub fn get_epoch_rewards_partition_data_address(epoch: u64) -> Pubkey {
    let (address, _bump_seed) = Pubkey::find_program_address(
        &[b"EpochRewardsPartitionData", &epoch.to_le_bytes()],
        &crate::stake::program::id(),
    );
    address
}
