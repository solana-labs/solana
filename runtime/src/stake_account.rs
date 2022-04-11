#[cfg(RUSTC_WITH_SPECIALIZATION)]
use solana_frozen_abi::abi_example::AbiExample;
use {
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount},
        account_utils::StateMut,
        instruction::InstructionError,
        pubkey::Pubkey,
        stake::state::{Delegation, StakeState},
    },
    thiserror::Error,
};

/// An account and a stake state deserialized from the account.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StakeAccount(AccountSharedData, StakeState);

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    InstructionError(#[from] InstructionError),
    #[error("Invalid stake account owner: {owner:?}")]
    InvalidOwner { owner: Pubkey },
}

impl StakeAccount {
    #[inline]
    pub(crate) fn lamports(&self) -> u64 {
        self.0.lamports()
    }

    #[inline]
    pub(crate) fn stake_state(&self) -> &StakeState {
        &self.1
    }

    #[inline]
    pub(crate) fn delegation(&self) -> Option<Delegation> {
        self.1.delegation()
    }
}

impl TryFrom<AccountSharedData> for StakeAccount {
    type Error = Error;
    fn try_from(account: AccountSharedData) -> Result<Self, Self::Error> {
        if account.owner() != &solana_stake_program::id() {
            return Err(Error::InvalidOwner {
                owner: *account.owner(),
            });
        }
        let stake_state = account.state()?;
        Ok(Self(account, stake_state))
    }
}

impl From<StakeAccount> for (AccountSharedData, StakeState) {
    fn from(stake_account: StakeAccount) -> Self {
        (stake_account.0, stake_account.1)
    }
}

#[cfg(RUSTC_WITH_SPECIALIZATION)]
impl AbiExample for StakeAccount {
    fn example() -> Self {
        use solana_sdk::{
            account::Account,
            stake::state::{Meta, Stake},
        };
        let stake_state = StakeState::Stake(Meta::example(), Stake::example());
        let mut account = Account::example();
        account.data.resize(196, 0u8);
        account.owner = solana_stake_program::id();
        let _ = account.set_state(&stake_state).unwrap();
        Self::try_from(AccountSharedData::from(account)).unwrap()
    }
}
