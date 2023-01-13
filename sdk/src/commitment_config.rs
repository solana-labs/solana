//! Definitions of commitment levels.

#![allow(deprecated)]
#![cfg(feature = "full")]

use {std::str::FromStr, thiserror::Error};

#[derive(Serialize, Deserialize, Default, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct CommitmentConfig {
    pub commitment: CommitmentLevel,
}

impl CommitmentConfig {
    #[deprecated(
        since = "1.5.5",
        note = "Please use CommitmentConfig::processed() instead"
    )]
    pub fn recent() -> Self {
        Self {
            commitment: CommitmentLevel::Recent,
        }
    }

    #[deprecated(
        since = "1.5.5",
        note = "Please use CommitmentConfig::finalized() instead"
    )]
    pub fn max() -> Self {
        Self {
            commitment: CommitmentLevel::Max,
        }
    }

    #[deprecated(
        since = "1.5.5",
        note = "Please use CommitmentConfig::finalized() instead"
    )]
    pub fn root() -> Self {
        Self {
            commitment: CommitmentLevel::Root,
        }
    }

    #[deprecated(
        since = "1.5.5",
        note = "Please use CommitmentConfig::confirmed() instead"
    )]
    pub fn single() -> Self {
        Self {
            commitment: CommitmentLevel::Single,
        }
    }

    #[deprecated(
        since = "1.5.5",
        note = "Please use CommitmentConfig::confirmed() instead"
    )]
    pub fn single_gossip() -> Self {
        Self {
            commitment: CommitmentLevel::SingleGossip,
        }
    }

    pub const fn finalized() -> Self {
        Self {
            commitment: CommitmentLevel::Finalized,
        }
    }

    pub const fn confirmed() -> Self {
        Self {
            commitment: CommitmentLevel::Confirmed,
        }
    }

    pub const fn processed() -> Self {
        Self {
            commitment: CommitmentLevel::Processed,
        }
    }

    pub fn ok(self) -> Option<Self> {
        if self == Self::default() {
            None
        } else {
            Some(self)
        }
    }

    pub fn is_finalized(&self) -> bool {
        matches!(
            &self.commitment,
            CommitmentLevel::Finalized | CommitmentLevel::Max | CommitmentLevel::Root
        )
    }

    pub fn is_confirmed(&self) -> bool {
        matches!(
            &self.commitment,
            CommitmentLevel::Confirmed | CommitmentLevel::SingleGossip | CommitmentLevel::Single
        )
    }

    pub fn is_processed(&self) -> bool {
        matches!(
            &self.commitment,
            CommitmentLevel::Processed | CommitmentLevel::Recent
        )
    }

    pub fn is_at_least_confirmed(&self) -> bool {
        self.is_confirmed() || self.is_finalized()
    }

    pub fn use_deprecated_commitment(commitment: CommitmentConfig) -> Self {
        match commitment.commitment {
            CommitmentLevel::Finalized => CommitmentConfig::max(),
            CommitmentLevel::Confirmed => CommitmentConfig::single_gossip(),
            CommitmentLevel::Processed => CommitmentConfig::recent(),
            _ => commitment,
        }
    }
}

impl FromStr for CommitmentConfig {
    type Err = ParseCommitmentLevelError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        CommitmentLevel::from_str(s).map(|commitment| Self { commitment })
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
/// An attribute of a slot. It describes how finalized a block is at some point in time. For example, a slot
/// is said to be at the max level immediately after the cluster recognizes the block at that slot as
/// finalized. When querying the ledger state, use lower levels of commitment to report progress and higher
/// levels to ensure state changes will not be rolled back.
pub enum CommitmentLevel {
    /// (DEPRECATED) The highest slot having reached max vote lockout, as recognized by a supermajority of the cluster.
    #[deprecated(
        since = "1.5.5",
        note = "Please use CommitmentLevel::Finalized instead"
    )]
    Max,

    /// (DEPRECATED) The highest slot of the heaviest fork. Ledger state at this slot is not derived from a finalized
    /// block, but if multiple forks are present, is from the fork the validator believes is most likely
    /// to finalize.
    #[deprecated(
        since = "1.5.5",
        note = "Please use CommitmentLevel::Processed instead"
    )]
    Recent,

    /// (DEPRECATED) The highest slot having reached max vote lockout.
    #[deprecated(
        since = "1.5.5",
        note = "Please use CommitmentLevel::Finalized instead"
    )]
    Root,

    /// (DEPRECATED) The highest slot having reached 1 confirmation by supermajority of the cluster.
    #[deprecated(
        since = "1.5.5",
        note = "Please use CommitmentLevel::Confirmed instead"
    )]
    Single,

    /// (DEPRECATED) The highest slot that has been voted on by supermajority of the cluster
    /// This differs from `single` in that:
    /// 1) It incorporates votes from gossip and replay.
    /// 2) It does not count votes on descendants of a block, only direct votes on that block.
    /// 3) This confirmation level also upholds "optimistic confirmation" guarantees in
    /// release 1.3 and onwards.
    #[deprecated(
        since = "1.5.5",
        note = "Please use CommitmentLevel::Confirmed instead"
    )]
    SingleGossip,

    /// The highest slot of the heaviest fork processed by the node. Ledger state at this slot is
    /// not derived from a confirmed or finalized block, but if multiple forks are present, is from
    /// the fork the validator believes is most likely to finalize.
    Processed,

    /// The highest slot that has been voted on by supermajority of the cluster, ie. is confirmed.
    /// Confirmation incorporates votes from gossip and replay. It does not count votes on
    /// descendants of a block, only direct votes on that block, and upholds "optimistic
    /// confirmation" guarantees in release 1.3 and onwards.
    Confirmed,

    /// The highest slot having reached max vote lockout, as recognized by a supermajority of the
    /// cluster.
    Finalized,
}

impl Default for CommitmentLevel {
    fn default() -> Self {
        Self::Finalized
    }
}

impl FromStr for CommitmentLevel {
    type Err = ParseCommitmentLevelError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "max" => Ok(CommitmentLevel::Max),
            "recent" => Ok(CommitmentLevel::Recent),
            "root" => Ok(CommitmentLevel::Root),
            "single" => Ok(CommitmentLevel::Single),
            "singleGossip" => Ok(CommitmentLevel::SingleGossip),
            "processed" => Ok(CommitmentLevel::Processed),
            "confirmed" => Ok(CommitmentLevel::Confirmed),
            "finalized" => Ok(CommitmentLevel::Finalized),
            _ => Err(ParseCommitmentLevelError::Invalid),
        }
    }
}

impl std::fmt::Display for CommitmentLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let s = match self {
            CommitmentLevel::Max => "max",
            CommitmentLevel::Recent => "recent",
            CommitmentLevel::Root => "root",
            CommitmentLevel::Single => "single",
            CommitmentLevel::SingleGossip => "singleGossip",
            CommitmentLevel::Processed => "processed",
            CommitmentLevel::Confirmed => "confirmed",
            CommitmentLevel::Finalized => "finalized",
        };
        write!(f, "{s}")
    }
}

#[derive(Error, Debug)]
pub enum ParseCommitmentLevelError {
    #[error("invalid variant")]
    Invalid,
}
