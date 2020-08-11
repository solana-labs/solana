#[derive(Serialize, Deserialize, Default, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CommitmentConfig {
    pub commitment: CommitmentLevel,
}

impl CommitmentConfig {
    pub fn recent() -> Self {
        Self {
            commitment: CommitmentLevel::Recent,
        }
    }

    pub fn max() -> Self {
        Self {
            commitment: CommitmentLevel::Max,
        }
    }

    pub fn root() -> Self {
        Self {
            commitment: CommitmentLevel::Root,
        }
    }

    pub fn single() -> Self {
        Self {
            commitment: CommitmentLevel::Single,
        }
    }

    pub fn single_gossip() -> Self {
        Self {
            commitment: CommitmentLevel::SingleGossip,
        }
    }

    pub fn ok(self) -> Option<Self> {
        if self == Self::default() {
            None
        } else {
            Some(self)
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
/// An attribute of a slot. It describes how finalized a block is at some point in time. For example, a slot
/// is said to be at the max level immediately after the cluster recognizes the block at that slot as
/// finalized. When querying the ledger state, use lower levels of commitment to report progress and higher
/// levels to ensure state changes will not be rolled back.
pub enum CommitmentLevel {
    /// The highest slot having reached max vote lockout, as recognized by a supermajority of the cluster.
    Max,

    /// The highest slot of the heaviest fork. Ledger state at this slot is not derived from a finalized
    /// block, but if multiple forks are present, is from the fork the validator believes is most likely
    /// to finalize.
    Recent,

    /// The highest slot having reached max vote lockout.
    Root,

    /// The highest slot having reached 1 confirmation.
    Single,

    /// The highest slot having reached 1 confirmation via gossip votes; may occur before or after Single,
    /// depending on gossip traffic.
    SingleGossip,
}

impl Default for CommitmentLevel {
    fn default() -> Self {
        Self::Max
    }
}
