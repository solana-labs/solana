#![allow(clippy::arithmetic_side_effects)]
// Remove the following `allow` when `StakeState` is removed, required to avoid
// warnings from uses of deprecated types during trait derivations.
#![allow(deprecated)]

use {
    crate::{
        clock::{Clock, Epoch, UnixTimestamp},
        instruction::InstructionError,
        pubkey::Pubkey,
        stake::{
            instruction::{LockupArgs, StakeError},
            stake_flags::StakeFlags,
        },
        stake_history::{StakeHistory, StakeHistoryEntry},
    },
    borsh::{maybestd::io, BorshDeserialize, BorshSchema, BorshSerialize},
    std::collections::HashSet,
};

pub type StakeActivationStatus = StakeHistoryEntry;

// means that no more than RATE of current effective stake may be added or subtracted per
// epoch
pub const DEFAULT_WARMUP_COOLDOWN_RATE: f64 = 0.25;
pub const NEW_WARMUP_COOLDOWN_RATE: f64 = 0.09;
pub const DEFAULT_SLASH_PENALTY: u8 = ((5 * std::u8::MAX as usize) / 100) as u8;

pub fn warmup_cooldown_rate(current_epoch: Epoch, new_rate_activation_epoch: Option<Epoch>) -> f64 {
    if current_epoch < new_rate_activation_epoch.unwrap_or(u64::MAX) {
        DEFAULT_WARMUP_COOLDOWN_RATE
    } else {
        NEW_WARMUP_COOLDOWN_RATE
    }
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Clone, Copy, AbiExample)]
#[allow(clippy::large_enum_variant)]
#[deprecated(
    since = "1.17.0",
    note = "Please use `StakeStateV2` instead, and match the third `StakeFlags` field when matching `StakeStateV2::Stake` to resolve any breakage. For example, `if let StakeState::Stake(meta, stake)` becomes `if let StakeStateV2::Stake(meta, stake, _stake_flags)`."
)]
pub enum StakeState {
    #[default]
    Uninitialized,
    Initialized(Meta),
    Stake(Meta, Stake),
    RewardsPool,
}
impl BorshDeserialize for StakeState {
    fn deserialize_reader<R: io::Read>(reader: &mut R) -> io::Result<Self> {
        let enum_value = u32::deserialize_reader(reader)?;
        match enum_value {
            0 => Ok(StakeState::Uninitialized),
            1 => {
                let meta = Meta::deserialize_reader(reader)?;
                Ok(StakeState::Initialized(meta))
            }
            2 => {
                let meta: Meta = BorshDeserialize::deserialize_reader(reader)?;
                let stake: Stake = BorshDeserialize::deserialize_reader(reader)?;
                Ok(StakeState::Stake(meta, stake))
            }
            3 => Ok(StakeState::RewardsPool),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid enum value",
            )),
        }
    }
}
impl BorshSerialize for StakeState {
    fn serialize<W: io::Write>(&self, writer: &mut W) -> io::Result<()> {
        match self {
            StakeState::Uninitialized => writer.write_all(&0u32.to_le_bytes()),
            StakeState::Initialized(meta) => {
                writer.write_all(&1u32.to_le_bytes())?;
                meta.serialize(writer)
            }
            StakeState::Stake(meta, stake) => {
                writer.write_all(&2u32.to_le_bytes())?;
                meta.serialize(writer)?;
                stake.serialize(writer)
            }
            StakeState::RewardsPool => writer.write_all(&3u32.to_le_bytes()),
        }
    }
}
impl StakeState {
    /// The fixed number of bytes used to serialize each stake account
    pub const fn size_of() -> usize {
        200 // see test_size_of
    }

    pub fn stake(&self) -> Option<Stake> {
        match self {
            StakeState::Stake(_meta, stake) => Some(*stake),
            _ => None,
        }
    }

    pub fn delegation(&self) -> Option<Delegation> {
        match self {
            StakeState::Stake(_meta, stake) => Some(stake.delegation),
            _ => None,
        }
    }

    pub fn authorized(&self) -> Option<Authorized> {
        match self {
            StakeState::Stake(meta, _stake) => Some(meta.authorized),
            StakeState::Initialized(meta) => Some(meta.authorized),
            _ => None,
        }
    }

    pub fn lockup(&self) -> Option<Lockup> {
        self.meta().map(|meta| meta.lockup)
    }

    pub fn meta(&self) -> Option<Meta> {
        match self {
            StakeState::Stake(meta, _stake) => Some(*meta),
            StakeState::Initialized(meta) => Some(*meta),
            _ => None,
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Clone, Copy, AbiExample)]
#[allow(clippy::large_enum_variant)]
pub enum StakeStateV2 {
    #[default]
    Uninitialized,
    Initialized(Meta),
    Stake(Meta, Stake, StakeFlags),
    RewardsPool,
}

impl BorshDeserialize for StakeStateV2 {
    fn deserialize_reader<R: io::Read>(reader: &mut R) -> io::Result<Self> {
        let enum_value = u32::deserialize_reader(reader)?;
        match enum_value {
            0 => Ok(StakeStateV2::Uninitialized),
            1 => {
                let meta = Meta::deserialize_reader(reader)?;
                Ok(StakeStateV2::Initialized(meta))
            }
            2 => {
                let meta: Meta = BorshDeserialize::deserialize_reader(reader)?;
                let stake: Stake = BorshDeserialize::deserialize_reader(reader)?;
                let stake_flags: StakeFlags = BorshDeserialize::deserialize_reader(reader)?;
                Ok(StakeStateV2::Stake(meta, stake, stake_flags))
            }
            3 => Ok(StakeStateV2::RewardsPool),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid enum value",
            )),
        }
    }
}

impl BorshSerialize for StakeStateV2 {
    fn serialize<W: io::Write>(&self, writer: &mut W) -> io::Result<()> {
        match self {
            StakeStateV2::Uninitialized => writer.write_all(&0u32.to_le_bytes()),
            StakeStateV2::Initialized(meta) => {
                writer.write_all(&1u32.to_le_bytes())?;
                meta.serialize(writer)
            }
            StakeStateV2::Stake(meta, stake, stake_flags) => {
                writer.write_all(&2u32.to_le_bytes())?;
                meta.serialize(writer)?;
                stake.serialize(writer)?;
                stake_flags.serialize(writer)
            }
            StakeStateV2::RewardsPool => writer.write_all(&3u32.to_le_bytes()),
        }
    }
}

impl StakeStateV2 {
    /// The fixed number of bytes used to serialize each stake account
    pub const fn size_of() -> usize {
        200 // see test_size_of
    }

    pub fn stake(&self) -> Option<Stake> {
        match self {
            StakeStateV2::Stake(_meta, stake, _stake_flags) => Some(*stake),
            _ => None,
        }
    }

    pub fn delegation(&self) -> Option<Delegation> {
        match self {
            StakeStateV2::Stake(_meta, stake, _stake_flags) => Some(stake.delegation),
            _ => None,
        }
    }

    pub fn authorized(&self) -> Option<Authorized> {
        match self {
            StakeStateV2::Stake(meta, _stake, _stake_flags) => Some(meta.authorized),
            StakeStateV2::Initialized(meta) => Some(meta.authorized),
            _ => None,
        }
    }

    pub fn lockup(&self) -> Option<Lockup> {
        self.meta().map(|meta| meta.lockup)
    }

    pub fn meta(&self) -> Option<Meta> {
        match self {
            StakeStateV2::Stake(meta, _stake, _stake_flags) => Some(*meta),
            StakeStateV2::Initialized(meta) => Some(*meta),
            _ => None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone, Copy, AbiExample)]
pub enum StakeAuthorize {
    Staker,
    Withdrawer,
}

#[derive(
    Default,
    Debug,
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    Clone,
    Copy,
    AbiExample,
    BorshDeserialize,
    BorshSchema,
    BorshSerialize,
)]
pub struct Lockup {
    /// UnixTimestamp at which this stake will allow withdrawal, unless the
    ///   transaction is signed by the custodian
    pub unix_timestamp: UnixTimestamp,
    /// epoch height at which this stake will allow withdrawal, unless the
    ///   transaction is signed by the custodian
    pub epoch: Epoch,
    /// custodian signature on a transaction exempts the operation from
    ///  lockup constraints
    pub custodian: Pubkey,
}

impl Lockup {
    pub fn is_in_force(&self, clock: &Clock, custodian: Option<&Pubkey>) -> bool {
        if custodian == Some(&self.custodian) {
            return false;
        }
        self.unix_timestamp > clock.unix_timestamp || self.epoch > clock.epoch
    }
}

#[derive(
    Default,
    Debug,
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    Clone,
    Copy,
    AbiExample,
    BorshDeserialize,
    BorshSchema,
    BorshSerialize,
)]
pub struct Authorized {
    pub staker: Pubkey,
    pub withdrawer: Pubkey,
}

impl Authorized {
    pub fn auto(authorized: &Pubkey) -> Self {
        Self {
            staker: *authorized,
            withdrawer: *authorized,
        }
    }
    pub fn check(
        &self,
        signers: &HashSet<Pubkey>,
        stake_authorize: StakeAuthorize,
    ) -> Result<(), InstructionError> {
        match stake_authorize {
            StakeAuthorize::Staker if signers.contains(&self.staker) => Ok(()),
            StakeAuthorize::Withdrawer if signers.contains(&self.withdrawer) => Ok(()),
            _ => Err(InstructionError::MissingRequiredSignature),
        }
    }

    pub fn authorize(
        &mut self,
        signers: &HashSet<Pubkey>,
        new_authorized: &Pubkey,
        stake_authorize: StakeAuthorize,
        lockup_custodian_args: Option<(&Lockup, &Clock, Option<&Pubkey>)>,
    ) -> Result<(), InstructionError> {
        match stake_authorize {
            StakeAuthorize::Staker => {
                // Allow either the staker or the withdrawer to change the staker key
                if !signers.contains(&self.staker) && !signers.contains(&self.withdrawer) {
                    return Err(InstructionError::MissingRequiredSignature);
                }
                self.staker = *new_authorized
            }
            StakeAuthorize::Withdrawer => {
                if let Some((lockup, clock, custodian)) = lockup_custodian_args {
                    if lockup.is_in_force(clock, None) {
                        match custodian {
                            None => {
                                return Err(StakeError::CustodianMissing.into());
                            }
                            Some(custodian) => {
                                if !signers.contains(custodian) {
                                    return Err(StakeError::CustodianSignatureMissing.into());
                                }

                                if lockup.is_in_force(clock, Some(custodian)) {
                                    return Err(StakeError::LockupInForce.into());
                                }
                            }
                        }
                    }
                }
                self.check(signers, stake_authorize)?;
                self.withdrawer = *new_authorized
            }
        }
        Ok(())
    }
}

#[derive(
    Default,
    Debug,
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    Clone,
    Copy,
    AbiExample,
    BorshDeserialize,
    BorshSchema,
    BorshSerialize,
)]
pub struct Meta {
    pub rent_exempt_reserve: u64,
    pub authorized: Authorized,
    pub lockup: Lockup,
}

impl Meta {
    pub fn set_lockup(
        &mut self,
        lockup: &LockupArgs,
        signers: &HashSet<Pubkey>,
        clock: &Clock,
    ) -> Result<(), InstructionError> {
        // post-stake_program_v4 behavior:
        // * custodian can update the lockup while in force
        // * withdraw authority can set a new lockup
        if self.lockup.is_in_force(clock, None) {
            if !signers.contains(&self.lockup.custodian) {
                return Err(InstructionError::MissingRequiredSignature);
            }
        } else if !signers.contains(&self.authorized.withdrawer) {
            return Err(InstructionError::MissingRequiredSignature);
        }
        if let Some(unix_timestamp) = lockup.unix_timestamp {
            self.lockup.unix_timestamp = unix_timestamp;
        }
        if let Some(epoch) = lockup.epoch {
            self.lockup.epoch = epoch;
        }
        if let Some(custodian) = lockup.custodian {
            self.lockup.custodian = custodian;
        }
        Ok(())
    }

    pub fn auto(authorized: &Pubkey) -> Self {
        Self {
            authorized: Authorized::auto(authorized),
            ..Meta::default()
        }
    }
}

#[derive(
    Debug,
    Serialize,
    Deserialize,
    PartialEq,
    Clone,
    Copy,
    AbiExample,
    BorshDeserialize,
    BorshSchema,
    BorshSerialize,
)]
pub struct Delegation {
    /// to whom the stake is delegated
    pub voter_pubkey: Pubkey,
    /// activated stake amount, set at delegate() time
    pub stake: u64,
    /// epoch at which this stake was activated, std::Epoch::MAX if is a bootstrap stake
    pub activation_epoch: Epoch,
    /// epoch the stake was deactivated, std::Epoch::MAX if not deactivated
    pub deactivation_epoch: Epoch,
    /// how much stake we can activate per-epoch as a fraction of currently effective stake
    #[deprecated(
        since = "1.16.7",
        note = "Please use `solana_sdk::stake::state::warmup_cooldown_rate()` instead"
    )]
    pub warmup_cooldown_rate: f64,
}

impl Default for Delegation {
    fn default() -> Self {
        #[allow(deprecated)]
        Self {
            voter_pubkey: Pubkey::default(),
            stake: 0,
            activation_epoch: 0,
            deactivation_epoch: std::u64::MAX,
            warmup_cooldown_rate: DEFAULT_WARMUP_COOLDOWN_RATE,
        }
    }
}

impl Delegation {
    pub fn new(voter_pubkey: &Pubkey, stake: u64, activation_epoch: Epoch) -> Self {
        Self {
            voter_pubkey: *voter_pubkey,
            stake,
            activation_epoch,
            ..Delegation::default()
        }
    }
    pub fn is_bootstrap(&self) -> bool {
        self.activation_epoch == std::u64::MAX
    }

    pub fn stake(
        &self,
        epoch: Epoch,
        history: Option<&StakeHistory>,
        new_rate_activation_epoch: Option<Epoch>,
    ) -> u64 {
        self.stake_activating_and_deactivating(epoch, history, new_rate_activation_epoch)
            .effective
    }

    #[allow(clippy::comparison_chain)]
    pub fn stake_activating_and_deactivating(
        &self,
        target_epoch: Epoch,
        history: Option<&StakeHistory>,
        new_rate_activation_epoch: Option<Epoch>,
    ) -> StakeActivationStatus {
        // first, calculate an effective and activating stake
        let (effective_stake, activating_stake) =
            self.stake_and_activating(target_epoch, history, new_rate_activation_epoch);

        // then de-activate some portion if necessary
        if target_epoch < self.deactivation_epoch {
            // not deactivated
            if activating_stake == 0 {
                StakeActivationStatus::with_effective(effective_stake)
            } else {
                StakeActivationStatus::with_effective_and_activating(
                    effective_stake,
                    activating_stake,
                )
            }
        } else if target_epoch == self.deactivation_epoch {
            // can only deactivate what's activated
            StakeActivationStatus::with_deactivating(effective_stake)
        } else if let Some((history, mut prev_epoch, mut prev_cluster_stake)) =
            history.and_then(|history| {
                history
                    .get(self.deactivation_epoch)
                    .map(|cluster_stake_at_deactivation_epoch| {
                        (
                            history,
                            self.deactivation_epoch,
                            cluster_stake_at_deactivation_epoch,
                        )
                    })
            })
        {
            // target_epoch > self.deactivation_epoch

            // loop from my deactivation epoch until the target epoch
            // current effective stake is updated using its previous epoch's cluster stake
            let mut current_epoch;
            let mut current_effective_stake = effective_stake;
            loop {
                current_epoch = prev_epoch + 1;
                // if there is no deactivating stake at prev epoch, we should have been
                // fully undelegated at this moment
                if prev_cluster_stake.deactivating == 0 {
                    break;
                }

                // I'm trying to get to zero, how much of the deactivation in stake
                //   this account is entitled to take
                let weight =
                    current_effective_stake as f64 / prev_cluster_stake.deactivating as f64;
                let warmup_cooldown_rate =
                    warmup_cooldown_rate(current_epoch, new_rate_activation_epoch);

                // portion of newly not-effective cluster stake I'm entitled to at current epoch
                let newly_not_effective_cluster_stake =
                    prev_cluster_stake.effective as f64 * warmup_cooldown_rate;
                let newly_not_effective_stake =
                    ((weight * newly_not_effective_cluster_stake) as u64).max(1);

                current_effective_stake =
                    current_effective_stake.saturating_sub(newly_not_effective_stake);
                if current_effective_stake == 0 {
                    break;
                }

                if current_epoch >= target_epoch {
                    break;
                }
                if let Some(current_cluster_stake) = history.get(current_epoch) {
                    prev_epoch = current_epoch;
                    prev_cluster_stake = current_cluster_stake;
                } else {
                    break;
                }
            }

            // deactivating stake should equal to all of currently remaining effective stake
            StakeActivationStatus::with_deactivating(current_effective_stake)
        } else {
            // no history or I've dropped out of history, so assume fully deactivated
            StakeActivationStatus::default()
        }
    }

    // returned tuple is (effective, activating) stake
    fn stake_and_activating(
        &self,
        target_epoch: Epoch,
        history: Option<&StakeHistory>,
        new_rate_activation_epoch: Option<Epoch>,
    ) -> (u64, u64) {
        let delegated_stake = self.stake;

        if self.is_bootstrap() {
            // fully effective immediately
            (delegated_stake, 0)
        } else if self.activation_epoch == self.deactivation_epoch {
            // activated but instantly deactivated; no stake at all regardless of target_epoch
            // this must be after the bootstrap check and before all-is-activating check
            (0, 0)
        } else if target_epoch == self.activation_epoch {
            // all is activating
            (0, delegated_stake)
        } else if target_epoch < self.activation_epoch {
            // not yet enabled
            (0, 0)
        } else if let Some((history, mut prev_epoch, mut prev_cluster_stake)) =
            history.and_then(|history| {
                history
                    .get(self.activation_epoch)
                    .map(|cluster_stake_at_activation_epoch| {
                        (
                            history,
                            self.activation_epoch,
                            cluster_stake_at_activation_epoch,
                        )
                    })
            })
        {
            // target_epoch > self.activation_epoch

            // loop from my activation epoch until the target epoch summing up my entitlement
            // current effective stake is updated using its previous epoch's cluster stake
            let mut current_epoch;
            let mut current_effective_stake = 0;
            loop {
                current_epoch = prev_epoch + 1;
                // if there is no activating stake at prev epoch, we should have been
                // fully effective at this moment
                if prev_cluster_stake.activating == 0 {
                    break;
                }

                // how much of the growth in stake this account is
                //  entitled to take
                let remaining_activating_stake = delegated_stake - current_effective_stake;
                let weight =
                    remaining_activating_stake as f64 / prev_cluster_stake.activating as f64;
                let warmup_cooldown_rate =
                    warmup_cooldown_rate(current_epoch, new_rate_activation_epoch);

                // portion of newly effective cluster stake I'm entitled to at current epoch
                let newly_effective_cluster_stake =
                    prev_cluster_stake.effective as f64 * warmup_cooldown_rate;
                let newly_effective_stake =
                    ((weight * newly_effective_cluster_stake) as u64).max(1);

                current_effective_stake += newly_effective_stake;
                if current_effective_stake >= delegated_stake {
                    current_effective_stake = delegated_stake;
                    break;
                }

                if current_epoch >= target_epoch || current_epoch >= self.deactivation_epoch {
                    break;
                }
                if let Some(current_cluster_stake) = history.get(current_epoch) {
                    prev_epoch = current_epoch;
                    prev_cluster_stake = current_cluster_stake;
                } else {
                    break;
                }
            }

            (
                current_effective_stake,
                delegated_stake - current_effective_stake,
            )
        } else {
            // no history or I've dropped out of history, so assume fully effective
            (delegated_stake, 0)
        }
    }
}

#[derive(
    Debug,
    Default,
    Serialize,
    Deserialize,
    PartialEq,
    Clone,
    Copy,
    AbiExample,
    BorshDeserialize,
    BorshSchema,
    BorshSerialize,
)]
pub struct Stake {
    pub delegation: Delegation,
    /// credits observed is credits from vote account state when delegated or redeemed
    pub credits_observed: u64,
}

impl Stake {
    pub fn stake(
        &self,
        epoch: Epoch,
        history: Option<&StakeHistory>,
        new_rate_activation_epoch: Option<Epoch>,
    ) -> u64 {
        self.delegation
            .stake(epoch, history, new_rate_activation_epoch)
    }

    pub fn split(
        &mut self,
        remaining_stake_delta: u64,
        split_stake_amount: u64,
    ) -> Result<Self, StakeError> {
        if remaining_stake_delta > self.delegation.stake {
            return Err(StakeError::InsufficientStake);
        }
        self.delegation.stake -= remaining_stake_delta;
        let new = Self {
            delegation: Delegation {
                stake: split_stake_amount,
                ..self.delegation
            },
            ..*self
        };
        Ok(new)
    }

    pub fn deactivate(&mut self, epoch: Epoch) -> Result<(), StakeError> {
        if self.delegation.deactivation_epoch != std::u64::MAX {
            Err(StakeError::AlreadyDeactivated)
        } else {
            self.delegation.deactivation_epoch = epoch;
            Ok(())
        }
    }
}

#[cfg(test)]
mod test {
    use {
        super::*, crate::borsh0_10::try_from_slice_unchecked, assert_matches::assert_matches,
        bincode::serialize,
    };

    fn check_borsh_deserialization(stake: StakeStateV2) {
        let serialized = serialize(&stake).unwrap();
        let deserialized = StakeStateV2::try_from_slice(&serialized).unwrap();
        assert_eq!(stake, deserialized);
    }

    fn check_borsh_serialization(stake: StakeStateV2) {
        let bincode_serialized = serialize(&stake).unwrap();
        let borsh_serialized = StakeStateV2::try_to_vec(&stake).unwrap();
        assert_eq!(bincode_serialized, borsh_serialized);
    }

    #[test]
    fn test_size_of() {
        assert_eq!(StakeStateV2::size_of(), std::mem::size_of::<StakeStateV2>());
    }

    #[test]
    fn bincode_vs_borsh_deserialization() {
        check_borsh_deserialization(StakeStateV2::Uninitialized);
        check_borsh_deserialization(StakeStateV2::RewardsPool);
        check_borsh_deserialization(StakeStateV2::Initialized(Meta {
            rent_exempt_reserve: u64::MAX,
            authorized: Authorized {
                staker: Pubkey::new_unique(),
                withdrawer: Pubkey::new_unique(),
            },
            lockup: Lockup::default(),
        }));
        check_borsh_deserialization(StakeStateV2::Stake(
            Meta {
                rent_exempt_reserve: 1,
                authorized: Authorized {
                    staker: Pubkey::new_unique(),
                    withdrawer: Pubkey::new_unique(),
                },
                lockup: Lockup::default(),
            },
            Stake {
                delegation: Delegation {
                    voter_pubkey: Pubkey::new_unique(),
                    stake: u64::MAX,
                    activation_epoch: Epoch::MAX,
                    deactivation_epoch: Epoch::MAX,
                    ..Delegation::default()
                },
                credits_observed: 1,
            },
            StakeFlags::empty(),
        ));
    }

    #[test]
    fn bincode_vs_borsh_serialization() {
        check_borsh_serialization(StakeStateV2::Uninitialized);
        check_borsh_serialization(StakeStateV2::RewardsPool);
        check_borsh_serialization(StakeStateV2::Initialized(Meta {
            rent_exempt_reserve: u64::MAX,
            authorized: Authorized {
                staker: Pubkey::new_unique(),
                withdrawer: Pubkey::new_unique(),
            },
            lockup: Lockup::default(),
        }));
        check_borsh_serialization(StakeStateV2::Stake(
            Meta {
                rent_exempt_reserve: 1,
                authorized: Authorized {
                    staker: Pubkey::new_unique(),
                    withdrawer: Pubkey::new_unique(),
                },
                lockup: Lockup::default(),
            },
            Stake {
                delegation: Delegation {
                    voter_pubkey: Pubkey::new_unique(),
                    stake: u64::MAX,
                    activation_epoch: Epoch::MAX,
                    deactivation_epoch: Epoch::MAX,
                    ..Default::default()
                },
                credits_observed: 1,
            },
            StakeFlags::MUST_FULLY_ACTIVATE_BEFORE_DEACTIVATION_IS_PERMITTED,
        ));
    }

    #[test]
    fn borsh_deserialization_live_data() {
        let data = [
            1, 0, 0, 0, 128, 213, 34, 0, 0, 0, 0, 0, 133, 0, 79, 231, 141, 29, 73, 61, 232, 35,
            119, 124, 168, 12, 120, 216, 195, 29, 12, 166, 139, 28, 36, 182, 186, 154, 246, 149,
            224, 109, 52, 100, 133, 0, 79, 231, 141, 29, 73, 61, 232, 35, 119, 124, 168, 12, 120,
            216, 195, 29, 12, 166, 139, 28, 36, 182, 186, 154, 246, 149, 224, 109, 52, 100, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0,
        ];
        // As long as we get the 4-byte enum and the first field right, then
        // we're sure the rest works out
        let deserialized = try_from_slice_unchecked::<StakeStateV2>(&data).unwrap();
        assert_matches!(
            deserialized,
            StakeStateV2::Initialized(Meta {
                rent_exempt_reserve: 2282880,
                ..
            })
        );
    }

    #[test]
    fn stake_flag_member_offset() {
        const FLAG_OFFSET: usize = 196;
        let check_flag = |flag, expected| {
            let stake = StakeStateV2::Stake(
                Meta {
                    rent_exempt_reserve: 1,
                    authorized: Authorized {
                        staker: Pubkey::new_unique(),
                        withdrawer: Pubkey::new_unique(),
                    },
                    lockup: Lockup::default(),
                },
                Stake {
                    delegation: Delegation {
                        voter_pubkey: Pubkey::new_unique(),
                        stake: u64::MAX,
                        activation_epoch: Epoch::MAX,
                        deactivation_epoch: Epoch::MAX,
                        warmup_cooldown_rate: f64::MAX,
                    },
                    credits_observed: 1,
                },
                flag,
            );

            let bincode_serialized = serialize(&stake).unwrap();
            let borsh_serialized = StakeStateV2::try_to_vec(&stake).unwrap();

            assert_eq!(bincode_serialized[FLAG_OFFSET], expected);
            assert_eq!(borsh_serialized[FLAG_OFFSET], expected);
        };
        check_flag(
            StakeFlags::MUST_FULLY_ACTIVATE_BEFORE_DEACTIVATION_IS_PERMITTED,
            1,
        );
        check_flag(StakeFlags::empty(), 0);
    }

    mod deprecated {
        use super::*;
        fn check_borsh_deserialization(stake: StakeState) {
            let serialized = serialize(&stake).unwrap();
            let deserialized = StakeState::try_from_slice(&serialized).unwrap();
            assert_eq!(stake, deserialized);
        }

        fn check_borsh_serialization(stake: StakeState) {
            let bincode_serialized = serialize(&stake).unwrap();
            let borsh_serialized = StakeState::try_to_vec(&stake).unwrap();
            assert_eq!(bincode_serialized, borsh_serialized);
        }

        #[test]
        fn test_size_of() {
            assert_eq!(StakeState::size_of(), std::mem::size_of::<StakeState>());
        }

        #[test]
        fn bincode_vs_borsh_deserialization() {
            check_borsh_deserialization(StakeState::Uninitialized);
            check_borsh_deserialization(StakeState::RewardsPool);
            check_borsh_deserialization(StakeState::Initialized(Meta {
                rent_exempt_reserve: u64::MAX,
                authorized: Authorized {
                    staker: Pubkey::new_unique(),
                    withdrawer: Pubkey::new_unique(),
                },
                lockup: Lockup::default(),
            }));
            check_borsh_deserialization(StakeState::Stake(
                Meta {
                    rent_exempt_reserve: 1,
                    authorized: Authorized {
                        staker: Pubkey::new_unique(),
                        withdrawer: Pubkey::new_unique(),
                    },
                    lockup: Lockup::default(),
                },
                Stake {
                    delegation: Delegation {
                        voter_pubkey: Pubkey::new_unique(),
                        stake: u64::MAX,
                        activation_epoch: Epoch::MAX,
                        deactivation_epoch: Epoch::MAX,
                        warmup_cooldown_rate: f64::MAX,
                    },
                    credits_observed: 1,
                },
            ));
        }

        #[test]
        fn bincode_vs_borsh_serialization() {
            check_borsh_serialization(StakeState::Uninitialized);
            check_borsh_serialization(StakeState::RewardsPool);
            check_borsh_serialization(StakeState::Initialized(Meta {
                rent_exempt_reserve: u64::MAX,
                authorized: Authorized {
                    staker: Pubkey::new_unique(),
                    withdrawer: Pubkey::new_unique(),
                },
                lockup: Lockup::default(),
            }));
            check_borsh_serialization(StakeState::Stake(
                Meta {
                    rent_exempt_reserve: 1,
                    authorized: Authorized {
                        staker: Pubkey::new_unique(),
                        withdrawer: Pubkey::new_unique(),
                    },
                    lockup: Lockup::default(),
                },
                Stake {
                    delegation: Delegation {
                        voter_pubkey: Pubkey::new_unique(),
                        stake: u64::MAX,
                        activation_epoch: Epoch::MAX,
                        deactivation_epoch: Epoch::MAX,
                        warmup_cooldown_rate: f64::MAX,
                    },
                    credits_observed: 1,
                },
            ));
        }

        #[test]
        fn borsh_deserialization_live_data() {
            let data = [
                1, 0, 0, 0, 128, 213, 34, 0, 0, 0, 0, 0, 133, 0, 79, 231, 141, 29, 73, 61, 232, 35,
                119, 124, 168, 12, 120, 216, 195, 29, 12, 166, 139, 28, 36, 182, 186, 154, 246,
                149, 224, 109, 52, 100, 133, 0, 79, 231, 141, 29, 73, 61, 232, 35, 119, 124, 168,
                12, 120, 216, 195, 29, 12, 166, 139, 28, 36, 182, 186, 154, 246, 149, 224, 109, 52,
                100, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            ];
            // As long as we get the 4-byte enum and the first field right, then
            // we're sure the rest works out
            let deserialized = try_from_slice_unchecked::<StakeState>(&data).unwrap();
            assert_matches!(
                deserialized,
                StakeState::Initialized(Meta {
                    rent_exempt_reserve: 2282880,
                    ..
                })
            );
        }
    }
}
