use crate::cli::build_balance_message;
use serde::Serialize;
use solana_sdk::clock::{Epoch, Slot};
use solana_vote_program::{
    authorized_voters::AuthorizedVoters,
    vote_state::{BlockTimestamp, Lockout},
};
use std::{collections::BTreeMap, fmt};

pub enum OutputFormat {
    Display,
    Json,
}

impl OutputFormat {
    pub fn formatted_print<T>(&self, item: &T)
    where
        T: Serialize + fmt::Display,
    {
        match self {
            OutputFormat::Display => {
                println!("{}", item);
            }
            OutputFormat::Json => {
                println!("{}", serde_json::to_value(item).unwrap());
            }
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliVoteAccount {
    pub account_balance: u64,
    pub validator_identity: String,
    #[serde(flatten)]
    pub authorized_voters: CliAuthorizedVoters,
    pub authorized_withdrawer: String,
    pub credits: u64,
    pub commission: u8,
    pub root_slot: Option<Slot>,
    pub recent_timestamp: BlockTimestamp,
    pub votes: Vec<CliLockout>,
    pub epoch_voting_history: Vec<CliEpochVotingHistory>,
    #[serde(skip_serializing)]
    pub use_lamports_unit: bool,
}

impl fmt::Display for CliVoteAccount {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(
            f,
            "Account Balance: {}",
            build_balance_message(self.account_balance, self.use_lamports_unit, true)
        )?;
        writeln!(f, "Validator Identity: {}", self.validator_identity)?;
        writeln!(f, "Authorized Voters: {}", self.authorized_voters)?;
        writeln!(f, "Authorized Withdrawer: {}", self.authorized_withdrawer)?;
        writeln!(f, "Credits: {}", self.credits)?;
        writeln!(f, "Commission: {}%", self.commission)?;
        writeln!(
            f,
            "Root Slot: {}",
            match self.root_slot {
                Some(slot) => slot.to_string(),
                None => "~".to_string(),
            }
        )?;
        writeln!(f, "Recent Timestamp: {:?}", self.recent_timestamp)?;
        if !self.votes.is_empty() {
            writeln!(f, "Recent Votes:")?;
            for vote in &self.votes {
                writeln!(
                    f,
                    "- slot: {}\n  confirmation count: {}",
                    vote.slot, vote.confirmation_count
                )?;
            }
            writeln!(f, "Epoch Voting History:")?;
            for epoch_info in &self.epoch_voting_history {
                writeln!(
                    f,
                    "- epoch: {}\n  slots in epoch: {}\n  credits earned: {}",
                    epoch_info.epoch, epoch_info.slots_in_epoch, epoch_info.credits_earned,
                )?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliAuthorizedVoters {
    authorized_voters: BTreeMap<Epoch, String>,
}

impl fmt::Display for CliAuthorizedVoters {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.authorized_voters)
    }
}

impl From<&AuthorizedVoters> for CliAuthorizedVoters {
    fn from(authorized_voters: &AuthorizedVoters) -> Self {
        let mut voter_map: BTreeMap<Epoch, String> = BTreeMap::new();
        for (epoch, voter) in authorized_voters.iter() {
            voter_map.insert(*epoch, voter.to_string());
        }
        Self {
            authorized_voters: voter_map,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliEpochVotingHistory {
    pub epoch: Epoch,
    pub slots_in_epoch: u64,
    pub credits_earned: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliLockout {
    pub slot: Slot,
    pub confirmation_count: u32,
}

impl From<&Lockout> for CliLockout {
    fn from(lockout: &Lockout) -> Self {
        Self {
            slot: lockout.slot,
            confirmation_count: lockout.confirmation_count,
        }
    }
}
