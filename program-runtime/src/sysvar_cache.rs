#[allow(deprecated)]
use solana_sdk::sysvar::{fees::Fees, recent_blockhashes::RecentBlockhashes};
use {
    crate::invoke_context::InvokeContext,
    serde::de::DeserializeOwned,
    solana_sdk::{
        instruction::InstructionError,
        pubkey::Pubkey,
        sysvar::{
            self, clock::Clock, epoch_rewards::EpochRewards, epoch_schedule::EpochSchedule,
            last_restart_slot::LastRestartSlot, rent::Rent, slot_hashes::SlotHashes,
            stake_history::StakeHistory, Sysvar, SysvarId,
        },
        transaction_context::{IndexOfAccount, InstructionContext, TransactionContext},
    },
    solana_type_overrides::sync::Arc,
};

#[cfg(feature = "frozen-abi")]
impl ::solana_frozen_abi::abi_example::AbiExample for SysvarCache {
    fn example() -> Self {
        // SysvarCache is not Serialize so just rely on Default.
        SysvarCache::default()
    }
}

#[derive(Default, Clone, Debug)]
pub struct SysvarCache {
    // full account data as provided by bank, including any trailing zero bytes
    clock: Option<Vec<u8>>,
    epoch_schedule: Option<Vec<u8>>,
    epoch_rewards: Option<Vec<u8>>,
    rent: Option<Vec<u8>>,
    slot_hashes: Option<Vec<u8>>,
    stake_history: Option<Vec<u8>>,
    last_restart_slot: Option<Vec<u8>>,

    // object representations of large sysvars for convenience
    // these are used by the stake and vote builtin programs
    // these should be removed once those programs are ported to bpf
    slot_hashes_obj: Option<Arc<SlotHashes>>,
    stake_history_obj: Option<Arc<StakeHistory>>,

    // deprecated sysvars, these should be removed once practical
    #[allow(deprecated)]
    fees: Option<Fees>,
    #[allow(deprecated)]
    recent_blockhashes: Option<RecentBlockhashes>,
}

// declare_deprecated_sysvar_id doesn't support const.
// These sysvars are going away anyway.
const FEES_ID: Pubkey = Pubkey::from_str_const("SysvarFees111111111111111111111111111111111");
const RECENT_BLOCKHASHES_ID: Pubkey =
    Pubkey::from_str_const("SysvarRecentB1ockHashes11111111111111111111");

impl SysvarCache {
    /// Overwrite a sysvar. For testing purposes only.
    #[allow(deprecated)]
    pub fn set_sysvar_for_tests<T: Sysvar + SysvarId>(&mut self, sysvar: &T) {
        let data = bincode::serialize(sysvar).expect("Failed to serialize sysvar.");
        let sysvar_id = T::id();
        match sysvar_id {
            sysvar::clock::ID => {
                self.clock = Some(data);
            }
            sysvar::epoch_rewards::ID => {
                self.epoch_rewards = Some(data);
            }
            sysvar::epoch_schedule::ID => {
                self.epoch_schedule = Some(data);
            }
            FEES_ID => {
                let fees: Fees =
                    bincode::deserialize(&data).expect("Failed to deserialize Fees sysvar.");
                self.fees = Some(fees);
            }
            sysvar::last_restart_slot::ID => {
                self.last_restart_slot = Some(data);
            }
            RECENT_BLOCKHASHES_ID => {
                let recent_blockhashes: RecentBlockhashes = bincode::deserialize(&data)
                    .expect("Failed to deserialize RecentBlockhashes sysvar.");
                self.recent_blockhashes = Some(recent_blockhashes);
            }
            sysvar::rent::ID => {
                self.rent = Some(data);
            }
            sysvar::slot_hashes::ID => {
                let slot_hashes: SlotHashes =
                    bincode::deserialize(&data).expect("Failed to deserialize SlotHashes sysvar.");
                self.slot_hashes = Some(data);
                self.slot_hashes_obj = Some(Arc::new(slot_hashes));
            }
            sysvar::stake_history::ID => {
                let stake_history: StakeHistory = bincode::deserialize(&data)
                    .expect("Failed to deserialize StakeHistory sysvar.");
                self.stake_history = Some(data);
                self.stake_history_obj = Some(Arc::new(stake_history));
            }
            _ => panic!("Unrecognized Sysvar ID: {sysvar_id}"),
        }
    }

    // this is exposed for SyscallGetSysvar and should not otherwise be used
    pub fn sysvar_id_to_buffer(&self, sysvar_id: &Pubkey) -> &Option<Vec<u8>> {
        if Clock::check_id(sysvar_id) {
            &self.clock
        } else if EpochSchedule::check_id(sysvar_id) {
            &self.epoch_schedule
        } else if EpochRewards::check_id(sysvar_id) {
            &self.epoch_rewards
        } else if Rent::check_id(sysvar_id) {
            &self.rent
        } else if SlotHashes::check_id(sysvar_id) {
            &self.slot_hashes
        } else if StakeHistory::check_id(sysvar_id) {
            &self.stake_history
        } else if LastRestartSlot::check_id(sysvar_id) {
            &self.last_restart_slot
        } else {
            &None
        }
    }

    // most if not all of the obj getter functions can be removed once builtins transition to bpf
    // the Arc<T> wrapper is to preserve the existing public interface
    fn get_sysvar_obj<T: DeserializeOwned>(
        &self,
        sysvar_id: &Pubkey,
    ) -> Result<Arc<T>, InstructionError> {
        if let Some(ref sysvar_buf) = self.sysvar_id_to_buffer(sysvar_id) {
            bincode::deserialize(sysvar_buf)
                .map(Arc::new)
                .map_err(|_| InstructionError::UnsupportedSysvar)
        } else {
            Err(InstructionError::UnsupportedSysvar)
        }
    }

    pub fn get_clock(&self) -> Result<Arc<Clock>, InstructionError> {
        self.get_sysvar_obj(&Clock::id())
    }

    pub fn get_epoch_schedule(&self) -> Result<Arc<EpochSchedule>, InstructionError> {
        self.get_sysvar_obj(&EpochSchedule::id())
    }

    pub fn get_epoch_rewards(&self) -> Result<Arc<EpochRewards>, InstructionError> {
        self.get_sysvar_obj(&EpochRewards::id())
    }

    pub fn get_rent(&self) -> Result<Arc<Rent>, InstructionError> {
        self.get_sysvar_obj(&Rent::id())
    }

    pub fn get_last_restart_slot(&self) -> Result<Arc<LastRestartSlot>, InstructionError> {
        self.get_sysvar_obj(&LastRestartSlot::id())
    }

    pub fn get_stake_history(&self) -> Result<Arc<StakeHistory>, InstructionError> {
        self.stake_history_obj
            .clone()
            .ok_or(InstructionError::UnsupportedSysvar)
    }

    pub fn get_slot_hashes(&self) -> Result<Arc<SlotHashes>, InstructionError> {
        self.slot_hashes_obj
            .clone()
            .ok_or(InstructionError::UnsupportedSysvar)
    }

    #[deprecated]
    #[allow(deprecated)]
    pub fn get_fees(&self) -> Result<Arc<Fees>, InstructionError> {
        self.fees
            .clone()
            .ok_or(InstructionError::UnsupportedSysvar)
            .map(Arc::new)
    }

    #[deprecated]
    #[allow(deprecated)]
    pub fn get_recent_blockhashes(&self) -> Result<Arc<RecentBlockhashes>, InstructionError> {
        self.recent_blockhashes
            .clone()
            .ok_or(InstructionError::UnsupportedSysvar)
            .map(Arc::new)
    }

    pub fn fill_missing_entries<F: FnMut(&Pubkey, &mut dyn FnMut(&[u8]))>(
        &mut self,
        mut get_account_data: F,
    ) {
        if self.clock.is_none() {
            get_account_data(&Clock::id(), &mut |data: &[u8]| {
                if bincode::deserialize::<Clock>(data).is_ok() {
                    self.clock = Some(data.to_vec());
                }
            });
        }

        if self.epoch_schedule.is_none() {
            get_account_data(&EpochSchedule::id(), &mut |data: &[u8]| {
                if bincode::deserialize::<EpochSchedule>(data).is_ok() {
                    self.epoch_schedule = Some(data.to_vec());
                }
            });
        }

        if self.epoch_rewards.is_none() {
            get_account_data(&EpochRewards::id(), &mut |data: &[u8]| {
                if bincode::deserialize::<EpochRewards>(data).is_ok() {
                    self.epoch_rewards = Some(data.to_vec());
                }
            });
        }

        if self.rent.is_none() {
            get_account_data(&Rent::id(), &mut |data: &[u8]| {
                if bincode::deserialize::<Rent>(data).is_ok() {
                    self.rent = Some(data.to_vec());
                }
            });
        }

        if self.slot_hashes.is_none() {
            get_account_data(&SlotHashes::id(), &mut |data: &[u8]| {
                if let Ok(obj) = bincode::deserialize::<SlotHashes>(data) {
                    self.slot_hashes = Some(data.to_vec());
                    self.slot_hashes_obj = Some(Arc::new(obj));
                }
            });
        }

        if self.stake_history.is_none() {
            get_account_data(&StakeHistory::id(), &mut |data: &[u8]| {
                if let Ok(obj) = bincode::deserialize::<StakeHistory>(data) {
                    self.stake_history = Some(data.to_vec());
                    self.stake_history_obj = Some(Arc::new(obj));
                }
            });
        }

        if self.last_restart_slot.is_none() {
            get_account_data(&LastRestartSlot::id(), &mut |data: &[u8]| {
                if bincode::deserialize::<LastRestartSlot>(data).is_ok() {
                    self.last_restart_slot = Some(data.to_vec());
                }
            });
        }

        #[allow(deprecated)]
        if self.fees.is_none() {
            get_account_data(&Fees::id(), &mut |data: &[u8]| {
                if let Ok(fees) = bincode::deserialize(data) {
                    self.fees = Some(fees);
                }
            });
        }

        #[allow(deprecated)]
        if self.recent_blockhashes.is_none() {
            get_account_data(&RecentBlockhashes::id(), &mut |data: &[u8]| {
                if let Ok(recent_blockhashes) = bincode::deserialize(data) {
                    self.recent_blockhashes = Some(recent_blockhashes);
                }
            });
        }
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// These methods facilitate a transition from fetching sysvars from keyed
/// accounts to fetching from the sysvar cache without breaking consensus. In
/// order to keep consistent behavior, they continue to enforce the same checks
/// as `solana_sdk::keyed_account::from_keyed_account` despite dynamically
/// loading them instead of deserializing from account data.
pub mod get_sysvar_with_account_check {
    use super::*;

    fn check_sysvar_account<S: Sysvar>(
        transaction_context: &TransactionContext,
        instruction_context: &InstructionContext,
        instruction_account_index: IndexOfAccount,
    ) -> Result<(), InstructionError> {
        let index_in_transaction = instruction_context
            .get_index_of_instruction_account_in_transaction(instruction_account_index)?;
        if !S::check_id(transaction_context.get_key_of_account_at_index(index_in_transaction)?) {
            return Err(InstructionError::InvalidArgument);
        }
        Ok(())
    }

    pub fn clock(
        invoke_context: &InvokeContext,
        instruction_context: &InstructionContext,
        instruction_account_index: IndexOfAccount,
    ) -> Result<Arc<Clock>, InstructionError> {
        check_sysvar_account::<Clock>(
            invoke_context.transaction_context,
            instruction_context,
            instruction_account_index,
        )?;
        invoke_context.get_sysvar_cache().get_clock()
    }

    pub fn rent(
        invoke_context: &InvokeContext,
        instruction_context: &InstructionContext,
        instruction_account_index: IndexOfAccount,
    ) -> Result<Arc<Rent>, InstructionError> {
        check_sysvar_account::<Rent>(
            invoke_context.transaction_context,
            instruction_context,
            instruction_account_index,
        )?;
        invoke_context.get_sysvar_cache().get_rent()
    }

    pub fn slot_hashes(
        invoke_context: &InvokeContext,
        instruction_context: &InstructionContext,
        instruction_account_index: IndexOfAccount,
    ) -> Result<Arc<SlotHashes>, InstructionError> {
        check_sysvar_account::<SlotHashes>(
            invoke_context.transaction_context,
            instruction_context,
            instruction_account_index,
        )?;
        invoke_context.get_sysvar_cache().get_slot_hashes()
    }

    #[allow(deprecated)]
    pub fn recent_blockhashes(
        invoke_context: &InvokeContext,
        instruction_context: &InstructionContext,
        instruction_account_index: IndexOfAccount,
    ) -> Result<Arc<RecentBlockhashes>, InstructionError> {
        check_sysvar_account::<RecentBlockhashes>(
            invoke_context.transaction_context,
            instruction_context,
            instruction_account_index,
        )?;
        invoke_context.get_sysvar_cache().get_recent_blockhashes()
    }

    pub fn stake_history(
        invoke_context: &InvokeContext,
        instruction_context: &InstructionContext,
        instruction_account_index: IndexOfAccount,
    ) -> Result<Arc<StakeHistory>, InstructionError> {
        check_sysvar_account::<StakeHistory>(
            invoke_context.transaction_context,
            instruction_context,
            instruction_account_index,
        )?;
        invoke_context.get_sysvar_cache().get_stake_history()
    }

    pub fn last_restart_slot(
        invoke_context: &InvokeContext,
        instruction_context: &InstructionContext,
        instruction_account_index: IndexOfAccount,
    ) -> Result<Arc<LastRestartSlot>, InstructionError> {
        check_sysvar_account::<LastRestartSlot>(
            invoke_context.transaction_context,
            instruction_context,
            instruction_account_index,
        )?;
        invoke_context.get_sysvar_cache().get_last_restart_slot()
    }
}

#[cfg(test)]
mod tests {
    use {super::*, test_case::test_case};

    // sysvar cache provides the full account data of a sysvar
    // the setters MUST NOT be changed to serialize an object representation
    // it is required that the syscall be able to access the full buffer as it exists onchain
    // this is meant to cover the cases:
    // * account data is larger than struct sysvar
    // * vector sysvar has fewer than its maximum entries
    // if at any point the data is roundtripped through bincode, the vector will shrink
    #[test_case(Clock::default(); "clock")]
    #[test_case(EpochSchedule::default(); "epoch_schedule")]
    #[test_case(EpochRewards::default(); "epoch_rewards")]
    #[test_case(Rent::default(); "rent")]
    #[test_case(SlotHashes::default(); "slot_hashes")]
    #[test_case(StakeHistory::default(); "stake_history")]
    #[test_case(LastRestartSlot::default(); "last_restart_slot")]
    fn test_sysvar_cache_preserves_bytes<T: Sysvar>(_: T) {
        let id = T::id();
        let size = T::size_of().saturating_mul(2);
        let in_buf = vec![0; size];

        let mut sysvar_cache = SysvarCache::default();
        sysvar_cache.fill_missing_entries(|pubkey, callback| {
            if *pubkey == id {
                callback(&in_buf)
            }
        });
        let sysvar_cache = sysvar_cache;

        let out_buf = sysvar_cache.sysvar_id_to_buffer(&id).clone().unwrap();

        assert_eq!(out_buf, in_buf);
    }
}
