use std::str::FromStr;
use thiserror::Error;

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
    /// The highest slot having reached max vote lockout, as recognized by a supermajority of the cluster.
    Max,

    /// The highest slot of the heaviest fork. Ledger state at this slot is not derived from a finalized
    /// block, but if multiple forks are present, is from the fork the validator believes is most likely
    /// to finalize.
    Recent,

    /// The highest slot having reached max vote lockout.
    Root,

    /// (DEPRECATED) The highest slot having reached 1 confirmation by supermajority of the cluster.
    Single,

    /// The highest slot that has been voted on by supermajority of the cluster
    /// This differs from `single` in that:
    /// 1) It incorporates votes from gossip and replay.
    /// 2) It does not count votes on descendants of a block, only direct votes on that block.
    /// 3) This confirmation level also upholds "optimistic confirmation" guarantees in
    /// release 1.3 and onwards.
    SingleGossip,
}

impl Default for CommitmentLevel {
    fn default() -> Self {
        Self::Max
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
        };
        write!(f, "{}", s)
    }
}

#[derive(Error, Debug)]
pub enum ParseCommitmentLevelError {
    #[error("invalid variant")]
    Invalid,
}
