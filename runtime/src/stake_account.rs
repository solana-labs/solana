#[cfg(RUSTC_WITH_SPECIALIZATION)]
use solana_frozen_abi::abi_example::AbiExample;
use {
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount},
        account_utils::StateMut,
        instruction::InstructionError,
        pubkey::Pubkey,
        stake::state::{Delegation, StakeStateV2},
    },
    std::marker::PhantomData,
    thiserror::Error,
};

/// An account and a stake state deserialized from the account.
/// Generic type T enforces type-safety so that StakeAccount<Delegation> can
/// only wrap a stake-state which is a Delegation; whereas StakeAccount<()>
/// wraps any account with stake state.
#[derive(Clone, Debug, Default)]
pub struct StakeAccount<T> {
    account: AccountSharedData,
    stake_state: StakeStateV2,
    _phantom: PhantomData<T>,
}

#[derive(Debug, Error)]
#[allow(clippy::enum_variant_names)]
pub enum Error {
    #[error(transparent)]
    InstructionError(#[from] InstructionError),
    #[error("Invalid delegation: {0:?}")]
    InvalidDelegation(Box<StakeStateV2>),
    #[error("Invalid stake account owner: {0}")]
    InvalidOwner(/*owner:*/ Pubkey),
}

impl<T> StakeAccount<T> {
    #[inline]
    pub(crate) fn lamports(&self) -> u64 {
        self.account.lamports()
    }

    #[inline]
    pub(crate) fn stake_state(&self) -> &StakeStateV2 {
        &self.stake_state
    }
}

impl StakeAccount<Delegation> {
    #[inline]
    pub(crate) fn delegation(&self) -> Delegation {
        // Safe to unwrap here because StakeAccount<Delegation> will always
        // only wrap a stake-state which is a delegation.
        self.stake_state.delegation().unwrap()
    }
}

impl TryFrom<AccountSharedData> for StakeAccount<Delegation> {
    type Error = Error;
    fn try_from(account: AccountSharedData) -> Result<Self, Self::Error> {
        if account.owner() != &solana_stake_program::id() {
            return Err(Error::InvalidOwner(*account.owner()));
        }
        let stake_state: StakeStateV2 = account.state()?;
        if stake_state.delegation().is_none() {
            return Err(Error::InvalidDelegation(Box::new(stake_state)));
        }
        Ok(Self {
            account,
            stake_state,
            _phantom: PhantomData,
        })
    }
}

impl<T> From<StakeAccount<T>> for (AccountSharedData, StakeStateV2) {
    #[inline]
    fn from(stake_account: StakeAccount<T>) -> Self {
        (stake_account.account, stake_account.stake_state)
    }
}

impl<S, T> PartialEq<StakeAccount<S>> for StakeAccount<T> {
    fn eq(&self, other: &StakeAccount<S>) -> bool {
        let StakeAccount {
            account,
            stake_state,
            _phantom,
        } = other;
        account == &self.account && stake_state == &self.stake_state
    }
}

#[cfg(RUSTC_WITH_SPECIALIZATION)]
impl AbiExample for StakeAccount<Delegation> {
    fn example() -> Self {
        use solana_sdk::{
            account::Account,
            stake::{
                stake_flags::StakeFlags,
                state::{Meta, Stake},
            },
        };
        let stake_state =
            StakeStateV2::Stake(Meta::example(), Stake::example(), StakeFlags::example());
        let mut account = Account::example();
        account.data.resize(200, 0u8);
        account.owner = solana_stake_program::id();
        account.set_state(&stake_state).unwrap();
        Self::try_from(AccountSharedData::from(account)).unwrap()
    }
}
