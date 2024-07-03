//! Definitions of commitment levels.

#![cfg(feature = "full")]

use {std::str::FromStr, thiserror::Error};

#[derive(Serialize, Deserialize, Default, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct CommitmentConfig {
    pub commitment: CommitmentLevel,
}

impl CommitmentConfig {
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
        self.commitment == CommitmentLevel::Finalized
    }

    pub fn is_confirmed(&self) -> bool {
        self.commitment == CommitmentLevel::Confirmed
    }

    pub fn is_processed(&self) -> bool {
        self.commitment == CommitmentLevel::Processed
    }

    pub fn is_at_least_confirmed(&self) -> bool {
        self.is_confirmed() || self.is_finalized()
    }

    #[deprecated(
        since = "2.0.2",
        note = "Returns self. Please do not use. Will be removed in the future."
    )]
    pub fn use_deprecated_commitment(commitment: CommitmentConfig) -> Self {
        commitment
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
