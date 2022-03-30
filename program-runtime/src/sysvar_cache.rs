#[allow(deprecated)]
use solana_sdk::sysvar::{fees::Fees, recent_blockhashes::RecentBlockhashes};
use {
    crate::invoke_context::InvokeContext,
    solana_sdk::{
        account::{create_account_with_fields, Account, AccountSharedData, ReadableAccount},
        clock::Epoch,
        instruction::InstructionError,
        pubkey::Pubkey,
        sysvar::{
            clock::Clock, epoch_schedule::EpochSchedule, rent::Rent, slot_hashes::SlotHashes,
            stake_history::StakeHistory, Sysvar, SysvarId,
        },
        transaction_context::{InstructionContext, TransactionContext},
    },
    std::{convert::TryFrom, sync::Arc},
};

#[cfg(RUSTC_WITH_SPECIALIZATION)]
impl ::solana_frozen_abi::abi_example::AbiExample for SysvarCache {
    fn example() -> Self {
        // SysvarCache is not Serialize so just rely on Default.
        SysvarCache::default()
    }
}

#[derive(Clone, Debug)]
pub struct SysvarWithLamports<T: Sysvar> {
    value: Arc<T>,
    lamports: u64,
}
impl<T: Sysvar> SysvarWithLamports<T> {
    pub fn new(value: T, lamports: u64) -> Self {
        Self {
            value: Arc::new(value),
            lamports,
        }
    }
    pub fn into_account(&self, rent_epoch: Epoch) -> Account {
        create_account_with_fields(&*self.value, (self.lamports, rent_epoch))
    }
}
impl<'a, T: Sysvar> TryFrom<&'a AccountSharedData> for SysvarWithLamports<T> {
    type Error = InstructionError;
    fn try_from(account: &'a AccountSharedData) -> Result<Self, Self::Error> {
        let value = bincode::deserialize(account.data())
            .map_err(|_| InstructionError::UnsupportedSysvar)?;
        Ok(SysvarWithLamports {
            value,
            lamports: account.lamports(),
        })
    }
}

#[derive(Default, Clone, Debug)]
pub struct SysvarCache {
    /// Used to simulate the rent_epoch on real accounts
    rent_epoch: Epoch,
    clock: Option<SysvarWithLamports<Clock>>,
    epoch_schedule: Option<SysvarWithLamports<EpochSchedule>>,
    #[allow(deprecated)]
    fees: Option<SysvarWithLamports<Fees>>,
    rent: Option<SysvarWithLamports<Rent>>,
    slot_hashes: Option<SysvarWithLamports<SlotHashes>>,
    #[allow(deprecated)]
    recent_blockhashes: Option<SysvarWithLamports<RecentBlockhashes>>,
    stake_history: Option<SysvarWithLamports<StakeHistory>>,
}

impl SysvarCache {
    pub fn new(rent_epoch: Epoch) -> Self {
        Self {
            rent_epoch,
            ..Self::default()
        }
    }

    pub fn get_account(&self, pubkey: &Pubkey) -> Result<AccountSharedData, InstructionError> {
        #[allow(deprecated)]
        let maybe_account = if pubkey == &Clock::id() {
            self.clock.as_ref().map(|v| v.into_account(self.rent_epoch))
        } else if pubkey == &EpochSchedule::id() {
            self.epoch_schedule
                .as_ref()
                .map(|v| v.into_account(self.rent_epoch))
        } else if pubkey == &Fees::id() {
            self.fees.as_ref().map(|v| v.into_account(self.rent_epoch))
        } else if pubkey == &Rent::id() {
            self.rent.as_ref().map(|v| v.into_account(self.rent_epoch))
        } else if pubkey == &SlotHashes::id() {
            self.slot_hashes
                .as_ref()
                .map(|v| v.into_account(self.rent_epoch))
        } else if pubkey == &RecentBlockhashes::id() {
            self.recent_blockhashes
                .as_ref()
                .map(|v| v.into_account(self.rent_epoch))
        } else if pubkey == &StakeHistory::id() {
            self.stake_history
                .as_ref()
                .map(|v| v.into_account(self.rent_epoch))
        } else {
            None
        };
        if let Some(account) = maybe_account {
            Ok(AccountSharedData::from(account))
        } else {
            Err(InstructionError::UnsupportedSysvar)
        }
    }

    pub fn set_account(
        &mut self,
        pubkey: &Pubkey,
        account: &AccountSharedData,
    ) -> Result<(), InstructionError> {
        #[allow(deprecated)]
        if pubkey == &Clock::id() {
            self.set_clock(account.try_into()?);
            Ok(())
        } else if pubkey == &EpochSchedule::id() {
            self.set_epoch_schedule(account.try_into()?);
            Ok(())
        } else if pubkey == &Fees::id() {
            self.set_fees(account.try_into()?);
            Ok(())
        } else if pubkey == &Rent::id() {
            self.set_rent(account.try_into()?);
            Ok(())
        } else if pubkey == &SlotHashes::id() {
            self.set_slot_hashes(account.try_into()?);
            Ok(())
        } else if pubkey == &RecentBlockhashes::id() {
            self.set_recent_blockhashes(account.try_into()?);
            Ok(())
        } else if pubkey == &StakeHistory::id() {
            self.set_stake_history(account.try_into()?);
            Ok(())
        } else {
            Err(InstructionError::UnsupportedSysvar)
        }
    }

    pub fn set_rent_epoch(&mut self, epoch: Epoch) {
        self.rent_epoch = epoch;
    }

    pub fn get_clock(&self) -> Result<Arc<Clock>, InstructionError> {
        self.clock
            .as_ref()
            .map(|v| v.value.clone())
            .ok_or(InstructionError::UnsupportedSysvar)
    }

    pub fn set_clock(&mut self, clock: SysvarWithLamports<Clock>) {
        self.clock = Some(clock);
    }

    pub fn get_epoch_schedule(&self) -> Result<Arc<EpochSchedule>, InstructionError> {
        self.epoch_schedule
            .as_ref()
            .map(|v| v.value.clone())
            .ok_or(InstructionError::UnsupportedSysvar)
    }

    pub fn set_epoch_schedule(&mut self, epoch_schedule: SysvarWithLamports<EpochSchedule>) {
        self.epoch_schedule = Some(epoch_schedule);
    }

    #[deprecated]
    #[allow(deprecated)]
    pub fn get_fees(&self) -> Result<Arc<Fees>, InstructionError> {
        self.fees
            .as_ref()
            .map(|v| v.value.clone())
            .ok_or(InstructionError::UnsupportedSysvar)
    }

    #[deprecated]
    #[allow(deprecated)]
    pub fn set_fees(&mut self, fees: SysvarWithLamports<Fees>) {
        self.fees = Some(fees);
    }

    pub fn get_rent(&self) -> Result<Arc<Rent>, InstructionError> {
        self.rent
            .as_ref()
            .map(|v| v.value.clone())
            .ok_or(InstructionError::UnsupportedSysvar)
    }

    pub fn set_rent(&mut self, rent: SysvarWithLamports<Rent>) {
        self.rent = Some(rent);
    }

    pub fn get_slot_hashes(&self) -> Result<Arc<SlotHashes>, InstructionError> {
        self.slot_hashes
            .as_ref()
            .map(|v| v.value.clone())
            .ok_or(InstructionError::UnsupportedSysvar)
    }

    pub fn set_slot_hashes(&mut self, slot_hashes: SysvarWithLamports<SlotHashes>) {
        self.slot_hashes = Some(slot_hashes);
    }

    #[deprecated]
    #[allow(deprecated)]
    pub fn get_recent_blockhashes(&self) -> Result<Arc<RecentBlockhashes>, InstructionError> {
        self.recent_blockhashes
            .as_ref()
            .map(|v| v.value.clone())
            .ok_or(InstructionError::UnsupportedSysvar)
    }

    #[deprecated]
    #[allow(deprecated)]
    pub fn set_recent_blockhashes(
        &mut self,
        recent_blockhashes: SysvarWithLamports<RecentBlockhashes>,
    ) {
        self.recent_blockhashes = Some(recent_blockhashes);
    }

    pub fn get_stake_history(&self) -> Result<Arc<StakeHistory>, InstructionError> {
        self.stake_history
            .as_ref()
            .map(|v| v.value.clone())
            .ok_or(InstructionError::UnsupportedSysvar)
    }

    pub fn set_stake_history(&mut self, stake_history: SysvarWithLamports<StakeHistory>) {
        self.stake_history = Some(stake_history);
    }

    pub fn fill_missing_entries<F: FnMut(&Pubkey) -> Option<AccountSharedData>>(
        &mut self,
        mut load_sysvar_account: F,
    ) {
        if self.get_clock().is_err() {
            if let Some(clock) = load_sysvar_account(&Clock::id())
                .as_ref()
                .and_then(|account| account.try_into().ok())
            {
                self.set_clock(clock);
            }
        }
        if self.get_epoch_schedule().is_err() {
            if let Some(epoch_schedule) = load_sysvar_account(&EpochSchedule::id())
                .as_ref()
                .and_then(|account| account.try_into().ok())
            {
                self.set_epoch_schedule(epoch_schedule);
            }
        }
        #[allow(deprecated)]
        if self.get_fees().is_err() {
            if let Some(fees) = load_sysvar_account(&Fees::id())
                .as_ref()
                .and_then(|account| account.try_into().ok())
            {
                self.set_fees(fees);
            }
        }
        if self.get_rent().is_err() {
            if let Some(rent) = load_sysvar_account(&Rent::id())
                .as_ref()
                .and_then(|account| account.try_into().ok())
            {
                self.set_rent(rent);
            }
        }
        if self.get_slot_hashes().is_err() {
            if let Some(slot_hashes) = load_sysvar_account(&SlotHashes::id())
                .as_ref()
                .and_then(|account| account.try_into().ok())
            {
                self.set_slot_hashes(slot_hashes);
            }
        }
        #[allow(deprecated)]
        if self.get_recent_blockhashes().is_err() {
            if let Some(recent_blockhashes) = load_sysvar_account(&RecentBlockhashes::id())
                .as_ref()
                .and_then(|account| account.try_into().ok())
            {
                self.set_recent_blockhashes(recent_blockhashes);
            }
        }
        if self.get_stake_history().is_err() {
            if let Some(stake_history) = load_sysvar_account(&StakeHistory::id())
                .as_ref()
                .and_then(|account| account.try_into().ok())
            {
                self.set_stake_history(stake_history);
            }
        }
    }

    pub fn reset(&mut self) {
        *self = SysvarCache::default();
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
        instruction_account_index: usize,
    ) -> Result<(), InstructionError> {
        if !S::check_id(
            instruction_context
                .get_instruction_account_key(transaction_context, instruction_account_index)?,
        ) {
            return Err(InstructionError::InvalidArgument);
        }
        Ok(())
    }

    pub fn clock(
        invoke_context: &InvokeContext,
        instruction_context: &InstructionContext,
        index_in_instruction: usize,
    ) -> Result<Arc<Clock>, InstructionError> {
        check_sysvar_account::<Clock>(
            invoke_context.transaction_context,
            instruction_context,
            index_in_instruction,
        )?;
        invoke_context.get_sysvar_cache().get_clock()
    }

    pub fn rent(
        invoke_context: &InvokeContext,
        instruction_context: &InstructionContext,
        index_in_instruction: usize,
    ) -> Result<Arc<Rent>, InstructionError> {
        check_sysvar_account::<Rent>(
            invoke_context.transaction_context,
            instruction_context,
            index_in_instruction,
        )?;
        invoke_context.get_sysvar_cache().get_rent()
    }

    pub fn slot_hashes(
        invoke_context: &InvokeContext,
        instruction_context: &InstructionContext,
        index_in_instruction: usize,
    ) -> Result<Arc<SlotHashes>, InstructionError> {
        check_sysvar_account::<SlotHashes>(
            invoke_context.transaction_context,
            instruction_context,
            index_in_instruction,
        )?;
        invoke_context.get_sysvar_cache().get_slot_hashes()
    }

    #[allow(deprecated)]
    pub fn recent_blockhashes(
        invoke_context: &InvokeContext,
        instruction_context: &InstructionContext,
        index_in_instruction: usize,
    ) -> Result<Arc<RecentBlockhashes>, InstructionError> {
        check_sysvar_account::<RecentBlockhashes>(
            invoke_context.transaction_context,
            instruction_context,
            index_in_instruction,
        )?;
        invoke_context.get_sysvar_cache().get_recent_blockhashes()
    }

    pub fn stake_history(
        invoke_context: &InvokeContext,
        instruction_context: &InstructionContext,
        index_in_instruction: usize,
    ) -> Result<Arc<StakeHistory>, InstructionError> {
        check_sysvar_account::<StakeHistory>(
            invoke_context.transaction_context,
            instruction_context,
            index_in_instruction,
        )?;
        invoke_context.get_sysvar_cache().get_stake_history()
    }
}
