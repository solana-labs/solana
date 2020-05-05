use crate::{cli::build_balance_message, display::writeln_name_value};
use chrono::{DateTime, NaiveDateTime, SecondsFormat, Utc};
use console::{style, Emoji};
use inflector::cases::titlecase::to_title_case;
use serde::Serialize;
use serde_json::{Map, Value};
use solana_client::rpc_response::{RpcEpochInfo, RpcKeyedAccount, RpcVoteAccountInfo};
use solana_sdk::{
    clock::{self, Epoch, Slot, UnixTimestamp},
    stake_history::StakeHistoryEntry,
};
use solana_stake_program::stake_state::{Authorized, Lockup};
use solana_vote_program::{
    authorized_voters::AuthorizedVoters,
    vote_state::{BlockTimestamp, Lockout},
};
use std::{collections::BTreeMap, fmt, time::Duration};

static WARNING: Emoji = Emoji("⚠️", "!");

#[derive(PartialEq)]
pub enum OutputFormat {
    Display,
    Json,
    JsonCompact,
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
                println!("{}", serde_json::to_string_pretty(item).unwrap());
            }
            OutputFormat::JsonCompact => {
                println!("{}", serde_json::to_value(item).unwrap());
            }
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct CliAccount {
    #[serde(flatten)]
    pub keyed_account: RpcKeyedAccount,
    #[serde(skip_serializing)]
    pub use_lamports_unit: bool,
}

impl fmt::Display for CliAccount {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f)?;
        writeln_name_value(f, "Public Key:", &self.keyed_account.pubkey)?;
        writeln_name_value(
            f,
            "Balance:",
            &build_balance_message(
                self.keyed_account.account.lamports,
                self.use_lamports_unit,
                true,
            ),
        )?;
        writeln_name_value(f, "Owner:", &self.keyed_account.account.owner)?;
        writeln_name_value(
            f,
            "Executable:",
            &self.keyed_account.account.executable.to_string(),
        )?;
        writeln_name_value(
            f,
            "Rent Epoch:",
            &self.keyed_account.account.rent_epoch.to_string(),
        )?;
        Ok(())
    }
}

#[derive(Default, Serialize, Deserialize)]
pub struct CliBlockProduction {
    pub epoch: Epoch,
    pub start_slot: Slot,
    pub end_slot: Slot,
    pub total_slots: usize,
    pub total_blocks_produced: usize,
    pub total_slots_skipped: usize,
    pub leaders: Vec<CliBlockProductionEntry>,
    pub individual_slot_status: Vec<CliSlotStatus>,
    #[serde(skip_serializing)]
    pub verbose: bool,
}

impl fmt::Display for CliBlockProduction {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f)?;
        writeln!(
            f,
            "{}",
            style(format!(
                "  {:<44}  {:>15}  {:>15}  {:>15}  {:>23}",
                "Identity Pubkey",
                "Leader Slots",
                "Blocks Produced",
                "Skipped Slots",
                "Skipped Slot Percentage",
            ))
            .bold()
        )?;
        for leader in &self.leaders {
            writeln!(
                f,
                "  {:<44}  {:>15}  {:>15}  {:>15}  {:>22.2}%",
                leader.identity_pubkey,
                leader.leader_slots,
                leader.blocks_produced,
                leader.skipped_slots,
                leader.skipped_slots as f64 / leader.leader_slots as f64 * 100.
            )?;
        }
        writeln!(f)?;
        writeln!(
            f,
            "  {:<44}  {:>15}  {:>15}  {:>15}  {:>22.2}%",
            format!("Epoch {} total:", self.epoch),
            self.total_slots,
            self.total_blocks_produced,
            self.total_slots_skipped,
            self.total_slots_skipped as f64 / self.total_slots as f64 * 100.
        )?;
        writeln!(
            f,
            "  (using data from {} slots: {} to {})",
            self.total_slots, self.start_slot, self.end_slot
        )?;
        if self.verbose {
            writeln!(f)?;
            writeln!(f)?;
            writeln!(
                f,
                "{}",
                style(format!("  {:<15} {:<44}", "Slot", "Identity Pubkey")).bold(),
            )?;
            for status in &self.individual_slot_status {
                if status.skipped {
                    writeln!(
                        f,
                        "{}",
                        style(format!(
                            "  {:<15} {:<44} SKIPPED",
                            status.slot, status.leader
                        ))
                        .red()
                    )?;
                } else {
                    writeln!(
                        f,
                        "{}",
                        style(format!("  {:<15} {:<44}", status.slot, status.leader))
                    )?;
                }
            }
        }
        Ok(())
    }
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliBlockProductionEntry {
    pub identity_pubkey: String,
    pub leader_slots: u64,
    pub blocks_produced: u64,
    pub skipped_slots: u64,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliSlotStatus {
    pub slot: Slot,
    pub leader: String,
    pub skipped: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliEpochInfo {
    #[serde(flatten)]
    pub epoch_info: RpcEpochInfo,
}

impl From<RpcEpochInfo> for CliEpochInfo {
    fn from(epoch_info: RpcEpochInfo) -> Self {
        Self { epoch_info }
    }
}

impl fmt::Display for CliEpochInfo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f)?;
        writeln_name_value(f, "Slot:", &self.epoch_info.absolute_slot.to_string())?;
        writeln_name_value(f, "Epoch:", &self.epoch_info.epoch.to_string())?;
        let start_slot = self.epoch_info.absolute_slot - self.epoch_info.slot_index;
        let end_slot = start_slot + self.epoch_info.slots_in_epoch;
        writeln_name_value(
            f,
            "Epoch Slot Range:",
            &format!("[{}..{})", start_slot, end_slot),
        )?;
        writeln_name_value(
            f,
            "Epoch Completed Percent:",
            &format!(
                "{:>3.3}%",
                self.epoch_info.slot_index as f64 / self.epoch_info.slots_in_epoch as f64 * 100_f64
            ),
        )?;
        let remaining_slots_in_epoch = self.epoch_info.slots_in_epoch - self.epoch_info.slot_index;
        writeln_name_value(
            f,
            "Epoch Completed Slots:",
            &format!(
                "{}/{} ({} remaining)",
                self.epoch_info.slot_index,
                self.epoch_info.slots_in_epoch,
                remaining_slots_in_epoch
            ),
        )?;
        writeln_name_value(
            f,
            "Epoch Completed Time:",
            &format!(
                "{}/{} ({} remaining)",
                slot_to_human_time(self.epoch_info.slot_index),
                slot_to_human_time(self.epoch_info.slots_in_epoch),
                slot_to_human_time(remaining_slots_in_epoch)
            ),
        )
    }
}

fn slot_to_human_time(slot: Slot) -> String {
    humantime::format_duration(Duration::from_secs(
        slot * clock::DEFAULT_TICKS_PER_SLOT / clock::DEFAULT_TICKS_PER_SECOND,
    ))
    .to_string()
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliValidators {
    pub total_active_stake: u64,
    pub total_current_stake: u64,
    pub total_deliquent_stake: u64,
    pub current_validators: Vec<CliValidator>,
    pub delinquent_validators: Vec<CliValidator>,
    #[serde(skip_serializing)]
    pub use_lamports_unit: bool,
}

impl fmt::Display for CliValidators {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fn write_vote_account(
            f: &mut fmt::Formatter,
            validator: &CliValidator,
            total_active_stake: u64,
            use_lamports_unit: bool,
            delinquent: bool,
        ) -> fmt::Result {
            fn non_zero_or_dash(v: u64) -> String {
                if v == 0 {
                    "-".into()
                } else {
                    format!("{}", v)
                }
            }

            writeln!(
                f,
                "{} {:<44}  {:<44}  {:>9}%   {:>8}  {:>10}  {:>7}  {}",
                if delinquent {
                    WARNING.to_string()
                } else {
                    " ".to_string()
                },
                validator.identity_pubkey,
                validator.vote_account_pubkey,
                validator.commission,
                non_zero_or_dash(validator.last_vote),
                non_zero_or_dash(validator.root_slot),
                validator.credits,
                if validator.activated_stake > 0 {
                    format!(
                        "{} ({:.2}%)",
                        build_balance_message(validator.activated_stake, use_lamports_unit, true),
                        100. * validator.activated_stake as f64 / total_active_stake as f64
                    )
                } else {
                    "-".into()
                },
            )
        }
        writeln_name_value(
            f,
            "Active Stake:",
            &build_balance_message(self.total_active_stake, self.use_lamports_unit, true),
        )?;
        if self.total_deliquent_stake > 0 {
            writeln_name_value(
                f,
                "Current Stake:",
                &format!(
                    "{} ({:0.2}%)",
                    &build_balance_message(self.total_current_stake, self.use_lamports_unit, true),
                    100. * self.total_current_stake as f64 / self.total_active_stake as f64
                ),
            )?;
            writeln_name_value(
                f,
                "Delinquent Stake:",
                &format!(
                    "{} ({:0.2}%)",
                    &build_balance_message(
                        self.total_deliquent_stake,
                        self.use_lamports_unit,
                        true
                    ),
                    100. * self.total_deliquent_stake as f64 / self.total_active_stake as f64
                ),
            )?;
        }
        writeln!(f)?;
        writeln!(
            f,
            "{}",
            style(format!(
                "  {:<44}  {:<44}  {}  {}  {}  {:>7}  {}",
                "Identity Pubkey",
                "Vote Account Pubkey",
                "Commission",
                "Last Vote",
                "Root Block",
                "Credits",
                "Active Stake",
            ))
            .bold()
        )?;
        for validator in &self.current_validators {
            write_vote_account(
                f,
                validator,
                self.total_active_stake,
                self.use_lamports_unit,
                false,
            )?;
        }
        for validator in &self.delinquent_validators {
            write_vote_account(
                f,
                validator,
                self.total_active_stake,
                self.use_lamports_unit,
                true,
            )?;
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliValidator {
    pub identity_pubkey: String,
    pub vote_account_pubkey: String,
    pub commission: u8,
    pub last_vote: u64,
    pub root_slot: u64,
    pub credits: u64,
    pub activated_stake: u64,
}

impl CliValidator {
    pub fn new(vote_account: &RpcVoteAccountInfo, current_epoch: Epoch) -> Self {
        Self {
            identity_pubkey: vote_account.node_pubkey.to_string(),
            vote_account_pubkey: vote_account.vote_pubkey.to_string(),
            commission: vote_account.commission,
            last_vote: vote_account.last_vote,
            root_slot: vote_account.root_slot,
            credits: vote_account
                .epoch_credits
                .iter()
                .find_map(|(epoch, credits, _)| {
                    if *epoch == current_epoch {
                        Some(*credits)
                    } else {
                        None
                    }
                })
                .unwrap_or(0),
            activated_stake: vote_account.activated_stake,
        }
    }
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliNonceAccount {
    pub balance: u64,
    pub minimum_balance_for_rent_exemption: u64,
    pub nonce: Option<String>,
    pub lamports_per_signature: Option<u64>,
    pub authority: Option<String>,
    #[serde(skip_serializing)]
    pub use_lamports_unit: bool,
}

impl fmt::Display for CliNonceAccount {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(
            f,
            "Balance: {}",
            build_balance_message(self.balance, self.use_lamports_unit, true)
        )?;
        writeln!(
            f,
            "Minimum Balance Required: {}",
            build_balance_message(
                self.minimum_balance_for_rent_exemption,
                self.use_lamports_unit,
                true
            )
        )?;
        let nonce = self.nonce.as_deref().unwrap_or("uninitialized");
        writeln!(f, "Nonce: {}", nonce)?;
        if let Some(fees) = self.lamports_per_signature {
            writeln!(f, "Fee: {} lamports per signature", fees)?;
        } else {
            writeln!(f, "Fees: uninitialized")?;
        }
        let authority = self.authority.as_deref().unwrap_or("uninitialized");
        writeln!(f, "Authority: {}", authority)
    }
}

#[derive(Serialize, Deserialize)]
pub struct CliStakeVec(Vec<CliKeyedStakeState>);

impl CliStakeVec {
    pub fn new(list: Vec<CliKeyedStakeState>) -> Self {
        Self(list)
    }
}

impl fmt::Display for CliStakeVec {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        for state in &self.0 {
            writeln!(f)?;
            write!(f, "{}", state)?;
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliKeyedStakeState {
    pub stake_pubkey: String,
    #[serde(flatten)]
    pub stake_state: CliStakeState,
}

impl fmt::Display for CliKeyedStakeState {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "Stake Pubkey: {}", self.stake_pubkey)?;
        write!(f, "{}", self.stake_state)
    }
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliStakeState {
    pub stake_type: CliStakeType,
    pub total_stake: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delegated_stake: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delegated_vote_account_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activation_epoch: Option<Epoch>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deactivation_epoch: Option<Epoch>,
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub authorized: Option<CliAuthorized>,
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub lockup: Option<CliLockup>,
    #[serde(skip_serializing)]
    pub use_lamports_unit: bool,
}

impl fmt::Display for CliStakeState {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fn show_authorized(f: &mut fmt::Formatter, authorized: &CliAuthorized) -> fmt::Result {
            writeln!(f, "Stake Authority: {}", authorized.staker)?;
            writeln!(f, "Withdraw Authority: {}", authorized.withdrawer)?;
            Ok(())
        }
        fn show_lockup(f: &mut fmt::Formatter, lockup: &CliLockup) -> fmt::Result {
            writeln!(
                f,
                "Lockup Timestamp: {} (UnixTimestamp: {})",
                DateTime::<Utc>::from_utc(
                    NaiveDateTime::from_timestamp(lockup.unix_timestamp, 0),
                    Utc
                )
                .to_rfc3339_opts(SecondsFormat::Secs, true),
                lockup.unix_timestamp
            )?;
            writeln!(f, "Lockup Epoch: {}", lockup.epoch)?;
            writeln!(f, "Lockup Custodian: {}", lockup.custodian)?;
            Ok(())
        }

        match self.stake_type {
            CliStakeType::RewardsPool => writeln!(f, "Stake account is a rewards pool")?,
            CliStakeType::Uninitialized => writeln!(f, "Stake account is uninitialized")?,
            CliStakeType::Initialized => {
                writeln!(
                    f,
                    "Total Stake: {}",
                    build_balance_message(self.total_stake, self.use_lamports_unit, true)
                )?;
                writeln!(f, "Stake account is undelegated")?;
                show_authorized(f, self.authorized.as_ref().unwrap())?;
                show_lockup(f, self.lockup.as_ref().unwrap())?;
            }
            CliStakeType::Stake => {
                writeln!(
                    f,
                    "Total Stake: {}",
                    build_balance_message(self.total_stake, self.use_lamports_unit, true)
                )?;
                writeln!(
                    f,
                    "Delegated Stake: {}",
                    build_balance_message(
                        self.delegated_stake.unwrap(),
                        self.use_lamports_unit,
                        true
                    )
                )?;
                if let Some(delegated_vote_account_address) = &self.delegated_vote_account_address {
                    writeln!(
                        f,
                        "Delegated Vote Account Address: {}",
                        delegated_vote_account_address
                    )?;
                }
                writeln!(
                    f,
                    "Stake activates starting from epoch: {}",
                    self.activation_epoch.unwrap()
                )?;
                if let Some(deactivation_epoch) = self.deactivation_epoch {
                    writeln!(
                        f,
                        "Stake deactivates starting from epoch: {}",
                        deactivation_epoch
                    )?;
                }
                show_authorized(f, self.authorized.as_ref().unwrap())?;
                show_lockup(f, self.lockup.as_ref().unwrap())?;
            }
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
pub enum CliStakeType {
    Stake,
    RewardsPool,
    Uninitialized,
    Initialized,
}

impl Default for CliStakeType {
    fn default() -> Self {
        Self::Uninitialized
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliStakeHistory {
    pub entries: Vec<CliStakeHistoryEntry>,
    #[serde(skip_serializing)]
    pub use_lamports_unit: bool,
}

impl fmt::Display for CliStakeHistory {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f)?;
        writeln!(
            f,
            "{}",
            style(format!(
                "  {:<5}  {:>20}  {:>20}  {:>20}",
                "Epoch", "Effective Stake", "Activating Stake", "Deactivating Stake",
            ))
            .bold()
        )?;
        for entry in &self.entries {
            writeln!(
                f,
                "  {:>5}  {:>20}  {:>20}  {:>20} {}",
                entry.epoch,
                build_balance_message(entry.effective_stake, self.use_lamports_unit, false),
                build_balance_message(entry.activating_stake, self.use_lamports_unit, false),
                build_balance_message(entry.deactivating_stake, self.use_lamports_unit, false),
                if self.use_lamports_unit {
                    "lamports"
                } else {
                    "SOL"
                }
            )?;
        }
        Ok(())
    }
}

impl From<&(Epoch, StakeHistoryEntry)> for CliStakeHistoryEntry {
    fn from((epoch, entry): &(Epoch, StakeHistoryEntry)) -> Self {
        Self {
            epoch: *epoch,
            effective_stake: entry.effective,
            activating_stake: entry.activating,
            deactivating_stake: entry.deactivating,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliStakeHistoryEntry {
    pub epoch: Epoch,
    pub effective_stake: u64,
    pub activating_stake: u64,
    pub deactivating_stake: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliAuthorized {
    pub staker: String,
    pub withdrawer: String,
}

impl From<&Authorized> for CliAuthorized {
    fn from(authorized: &Authorized) -> Self {
        Self {
            staker: authorized.staker.to_string(),
            withdrawer: authorized.withdrawer.to_string(),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliLockup {
    pub unix_timestamp: UnixTimestamp,
    pub epoch: Epoch,
    pub custodian: String,
}

impl From<&Lockup> for CliLockup {
    fn from(lockup: &Lockup) -> Self {
        Self {
            unix_timestamp: lockup.unix_timestamp,
            epoch: lockup.epoch,
            custodian: lockup.custodian.to_string(),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct CliValidatorInfoVec(Vec<CliValidatorInfo>);

impl CliValidatorInfoVec {
    pub fn new(list: Vec<CliValidatorInfo>) -> Self {
        Self(list)
    }
}

impl fmt::Display for CliValidatorInfoVec {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.0.is_empty() {
            writeln!(f, "No validator info accounts found")?;
        }
        for validator_info in &self.0 {
            writeln!(f)?;
            write!(f, "{}", validator_info)?;
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliValidatorInfo {
    pub identity_pubkey: String,
    pub info_pubkey: String,
    pub info: Map<String, Value>,
}

impl fmt::Display for CliValidatorInfo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln_name_value(f, "Validator Identity Pubkey:", &self.identity_pubkey)?;
        writeln_name_value(f, "  Info Pubkey:", &self.info_pubkey)?;
        for (key, value) in self.info.iter() {
            writeln_name_value(
                f,
                &format!("  {}:", to_title_case(key)),
                &value.as_str().unwrap_or("?"),
            )?;
        }
        Ok(())
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

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliBlockTime {
    pub slot: Slot,
    pub timestamp: UnixTimestamp,
}

impl fmt::Display for CliBlockTime {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln_name_value(f, "Block:", &self.slot.to_string())?;
        writeln_name_value(
            f,
            "Date:",
            &format!(
                "{} (UnixTimestamp: {})",
                DateTime::<Utc>::from_utc(NaiveDateTime::from_timestamp(self.timestamp, 0), Utc)
                    .to_rfc3339_opts(SecondsFormat::Secs, true),
                self.timestamp
            ),
        )
    }
}
