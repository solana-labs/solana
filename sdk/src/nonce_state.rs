use crate::{
    account::{Account, KeyedAccount},
    account_utils::State,
    hash::Hash,
    instruction::InstructionError,
    nonce_instruction::NonceError,
    nonce_program,
    pubkey::Pubkey,
    sysvar::recent_blockhashes::RecentBlockhashes,
    sysvar::rent::Rent,
};
use serde_derive::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Clone, Copy)]
pub struct Meta {}

impl Meta {
    pub fn new() -> Self {
        Self {}
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone, Copy)]
pub enum NonceState {
    Uninitialized,
    Initialized(Meta, Hash),
}

impl Default for NonceState {
    fn default() -> Self {
        NonceState::Uninitialized
    }
}

impl NonceState {
    pub fn size() -> usize {
        bincode::serialized_size(&NonceState::Initialized(Meta::default(), Hash::default()))
            .unwrap() as usize
    }
}

pub trait NonceAccount {
    fn nonce(
        &mut self,
        recent_blockhashes: &RecentBlockhashes,
        signers: &HashSet<Pubkey>,
    ) -> Result<(), InstructionError>;
    fn withdraw(
        &mut self,
        lamports: u64,
        to: &mut KeyedAccount,
        recent_blockhashes: &RecentBlockhashes,
        rent: &Rent,
        signers: &HashSet<Pubkey>,
    ) -> Result<(), InstructionError>;
    fn initialize(
        &mut self,
        recent_blockhashes: &RecentBlockhashes,
        rent: &Rent,
        signers: &HashSet<Pubkey>,
    ) -> Result<(), InstructionError>;
}

impl<'a> NonceAccount for KeyedAccount<'a> {
    fn nonce(
        &mut self,
        recent_blockhashes: &RecentBlockhashes,
        signers: &HashSet<Pubkey>,
    ) -> Result<(), InstructionError> {
        if recent_blockhashes.is_empty() {
            return Err(NonceError::NoRecentBlockhashes.into());
        }

        if !signers.contains(self.unsigned_key()) {
            return Err(InstructionError::MissingRequiredSignature);
        }

        let meta = match self.state()? {
            NonceState::Initialized(meta, ref hash) => {
                if *hash == recent_blockhashes[0] {
                    return Err(NonceError::NotExpired.into());
                }
                meta
            }
            _ => return Err(NonceError::BadAccountState.into()),
        };

        self.set_state(&NonceState::Initialized(meta, recent_blockhashes[0]))
    }

    fn withdraw(
        &mut self,
        lamports: u64,
        to: &mut KeyedAccount,
        recent_blockhashes: &RecentBlockhashes,
        rent: &Rent,
        signers: &HashSet<Pubkey>,
    ) -> Result<(), InstructionError> {
        if !signers.contains(self.unsigned_key()) {
            return Err(InstructionError::MissingRequiredSignature);
        }

        match self.state()? {
            NonceState::Uninitialized => {
                if lamports > self.account.lamports {
                    return Err(InstructionError::InsufficientFunds);
                }
            }
            NonceState::Initialized(_meta, ref hash) => {
                if lamports == self.account.lamports {
                    if *hash == recent_blockhashes[0] {
                        return Err(NonceError::NotExpired.into());
                    }
                    self.set_state(&NonceState::Uninitialized)?;
                } else {
                    let min_balance = rent.minimum_balance(self.account.data.len());
                    if lamports + min_balance > self.account.lamports {
                        return Err(InstructionError::InsufficientFunds);
                    }
                }
            }
        }

        self.account.lamports -= lamports;
        to.account.lamports += lamports;

        Ok(())
    }

    fn initialize(
        &mut self,
        recent_blockhashes: &RecentBlockhashes,
        rent: &Rent,
        signers: &HashSet<Pubkey>,
    ) -> Result<(), InstructionError> {
        if recent_blockhashes.is_empty() {
            return Err(NonceError::NoRecentBlockhashes.into());
        }

        if !signers.contains(self.unsigned_key()) {
            return Err(InstructionError::MissingRequiredSignature);
        }

        let meta = match self.state()? {
            NonceState::Uninitialized => {
                let min_balance = rent.minimum_balance(self.account.data.len());
                if self.account.lamports < min_balance {
                    return Err(InstructionError::InsufficientFunds);
                }
                Meta::new()
            }
            _ => return Err(NonceError::BadAccountState.into()),
        };

        self.set_state(&NonceState::Initialized(meta, recent_blockhashes[0]))
    }
}

pub fn create_account(lamports: u64) -> Account {
    Account::new_data_with_space(
        lamports,
        &NonceState::Uninitialized,
        NonceState::size(),
        &nonce_program::id(),
    )
    .expect("nonce_account")
}

/// Convenience function for working with keyed accounts in tests
#[cfg(not(feature = "program"))]
pub fn with_test_keyed_account<F>(lamports: u64, signer: bool, mut f: F)
where
    F: FnMut(&mut KeyedAccount),
{
    let pubkey = Pubkey::new_rand();
    let mut account = create_account(lamports);
    let mut keyed_account = KeyedAccount::new(&pubkey, signer, &mut account);
    f(&mut keyed_account)
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{
        account::KeyedAccount,
        nonce_instruction::NonceError,
        sysvar::recent_blockhashes::{create_test_recent_blockhashes, RecentBlockhashes},
    };
    use std::iter::FromIterator;

    #[test]
    fn default_is_uninitialized() {
        assert_eq!(NonceState::default(), NonceState::Uninitialized)
    }

    #[test]
    fn new_meta() {
        assert_eq!(Meta::new(), Meta {});
    }

    #[test]
    fn keyed_account_expected_behavior() {
        let rent = Rent {
            lamports_per_byte_year: 42,
            ..Rent::default()
        };
        let min_lamports = rent.minimum_balance(NonceState::size());
        let meta = Meta::new();
        with_test_keyed_account(min_lamports + 42, true, |keyed_account| {
            let mut signers = HashSet::new();
            signers.insert(keyed_account.signer_key().unwrap().clone());
            let state: NonceState = keyed_account.state().unwrap();
            // New is in Uninitialzed state
            assert_eq!(state, NonceState::Uninitialized);
            let recent_blockhashes = create_test_recent_blockhashes(95);
            keyed_account
                .initialize(&recent_blockhashes, &rent, &signers)
                .unwrap();
            let state: NonceState = keyed_account.state().unwrap();
            let stored = recent_blockhashes[0];
            // First nonce instruction drives state from Uninitialized to Initialized
            assert_eq!(state, NonceState::Initialized(meta, stored));
            let recent_blockhashes = create_test_recent_blockhashes(63);
            keyed_account.nonce(&recent_blockhashes, &signers).unwrap();
            let state: NonceState = keyed_account.state().unwrap();
            let stored = recent_blockhashes[0];
            // Second nonce instruction consumes and replaces stored nonce
            assert_eq!(state, NonceState::Initialized(meta, stored));
            let recent_blockhashes = create_test_recent_blockhashes(31);
            keyed_account.nonce(&recent_blockhashes, &signers).unwrap();
            let state: NonceState = keyed_account.state().unwrap();
            let stored = recent_blockhashes[0];
            // Third nonce instruction for fun and profit
            assert_eq!(state, NonceState::Initialized(meta, stored));
            with_test_keyed_account(42, false, |mut to_keyed| {
                let recent_blockhashes = create_test_recent_blockhashes(0);
                let withdraw_lamports = keyed_account.account.lamports;
                let expect_nonce_lamports = keyed_account.account.lamports - withdraw_lamports;
                let expect_to_lamports = to_keyed.account.lamports + withdraw_lamports;
                keyed_account
                    .withdraw(
                        withdraw_lamports,
                        &mut to_keyed,
                        &recent_blockhashes,
                        &rent,
                        &signers,
                    )
                    .unwrap();
                let state: NonceState = keyed_account.state().unwrap();
                // Withdraw instruction...
                // Deinitializes NonceAccount state
                assert_eq!(state, NonceState::Uninitialized);
                // Empties NonceAccount balance
                assert_eq!(keyed_account.account.lamports, expect_nonce_lamports);
                // NonceAccount balance goes to `to`
                assert_eq!(to_keyed.account.lamports, expect_to_lamports);
            })
        })
    }

    #[test]
    fn nonce_inx_initialized_account_not_signer_fail() {
        let rent = Rent {
            lamports_per_byte_year: 42,
            ..Rent::default()
        };
        let min_lamports = rent.minimum_balance(NonceState::size());
        let meta = Meta::new();
        with_test_keyed_account(min_lamports + 42, true, |nonce_account| {
            let mut signers = HashSet::new();
            signers.insert(nonce_account.signer_key().unwrap().clone());
            let recent_blockhashes = create_test_recent_blockhashes(31);
            let stored = recent_blockhashes[0];
            nonce_account
                .initialize(&recent_blockhashes, &rent, &signers)
                .unwrap();
            let pubkey = nonce_account.account.owner.clone();
            let mut nonce_account = KeyedAccount::new(&pubkey, false, nonce_account.account);
            let state: NonceState = nonce_account.state().unwrap();
            assert_eq!(state, NonceState::Initialized(meta, stored));
            let signers = HashSet::new();
            let recent_blockhashes = create_test_recent_blockhashes(0);
            let result = nonce_account.nonce(&recent_blockhashes, &signers);
            assert_eq!(result, Err(InstructionError::MissingRequiredSignature),);
        })
    }

    #[test]
    fn nonce_inx_with_empty_recent_blockhashes_fail() {
        let rent = Rent {
            lamports_per_byte_year: 42,
            ..Rent::default()
        };
        let min_lamports = rent.minimum_balance(NonceState::size());
        with_test_keyed_account(min_lamports + 42, true, |keyed_account| {
            let mut signers = HashSet::new();
            signers.insert(keyed_account.signer_key().unwrap().clone());
            let recent_blockhashes = create_test_recent_blockhashes(0);
            keyed_account
                .initialize(&recent_blockhashes, &rent, &signers)
                .unwrap();
            let recent_blockhashes = RecentBlockhashes::from_iter(vec![].into_iter());
            let result = keyed_account.nonce(&recent_blockhashes, &signers);
            assert_eq!(result, Err(NonceError::NoRecentBlockhashes.into()));
        })
    }

    #[test]
    fn nonce_inx_too_early_fail() {
        let rent = Rent {
            lamports_per_byte_year: 42,
            ..Rent::default()
        };
        let min_lamports = rent.minimum_balance(NonceState::size());
        with_test_keyed_account(min_lamports + 42, true, |keyed_account| {
            let mut signers = HashSet::new();
            signers.insert(keyed_account.signer_key().unwrap().clone());
            let recent_blockhashes = create_test_recent_blockhashes(63);
            keyed_account
                .initialize(&recent_blockhashes, &rent, &signers)
                .unwrap();
            let result = keyed_account.nonce(&recent_blockhashes, &signers);
            assert_eq!(result, Err(NonceError::NotExpired.into()));
        })
    }

    #[test]
    fn nonce_inx_uninitialized_account_fail() {
        let rent = Rent {
            lamports_per_byte_year: 42,
            ..Rent::default()
        };
        let min_lamports = rent.minimum_balance(NonceState::size());
        with_test_keyed_account(min_lamports + 42, true, |keyed_account| {
            let mut signers = HashSet::new();
            signers.insert(keyed_account.signer_key().unwrap().clone());
            let recent_blockhashes = create_test_recent_blockhashes(63);
            let result = keyed_account.nonce(&recent_blockhashes, &signers);
            assert_eq!(result, Err(NonceError::BadAccountState.into()));
        })
    }

    #[test]
    fn withdraw_inx_unintialized_acc_ok() {
        let rent = Rent {
            lamports_per_byte_year: 42,
            ..Rent::default()
        };
        let min_lamports = rent.minimum_balance(NonceState::size());
        with_test_keyed_account(min_lamports + 42, true, |nonce_keyed| {
            let state: NonceState = nonce_keyed.state().unwrap();
            assert_eq!(state, NonceState::Uninitialized);
            with_test_keyed_account(42, false, |mut to_keyed| {
                let mut signers = HashSet::new();
                signers.insert(nonce_keyed.signer_key().unwrap().clone());
                let recent_blockhashes = create_test_recent_blockhashes(0);
                let withdraw_lamports = nonce_keyed.account.lamports;
                let expect_nonce_lamports = nonce_keyed.account.lamports - withdraw_lamports;
                let expect_to_lamports = to_keyed.account.lamports + withdraw_lamports;
                nonce_keyed
                    .withdraw(
                        withdraw_lamports,
                        &mut to_keyed,
                        &recent_blockhashes,
                        &rent,
                        &signers,
                    )
                    .unwrap();
                let state: NonceState = nonce_keyed.state().unwrap();
                // Withdraw instruction...
                // Deinitializes NonceAccount state
                assert_eq!(state, NonceState::Uninitialized);
                // Empties NonceAccount balance
                assert_eq!(nonce_keyed.account.lamports, expect_nonce_lamports);
                // NonceAccount balance goes to `to`
                assert_eq!(to_keyed.account.lamports, expect_to_lamports);
            })
        })
    }

    #[test]
    fn withdraw_inx_unintialized_acc_unsigned_fail() {
        let rent = Rent {
            lamports_per_byte_year: 42,
            ..Rent::default()
        };
        let min_lamports = rent.minimum_balance(NonceState::size());
        with_test_keyed_account(min_lamports + 42, false, |nonce_keyed| {
            let state: NonceState = nonce_keyed.state().unwrap();
            assert_eq!(state, NonceState::Uninitialized);
            with_test_keyed_account(42, false, |mut to_keyed| {
                let signers = HashSet::new();
                let recent_blockhashes = create_test_recent_blockhashes(0);
                let result = nonce_keyed.withdraw(
                    nonce_keyed.account.lamports,
                    &mut to_keyed,
                    &recent_blockhashes,
                    &rent,
                    &signers,
                );
                assert_eq!(result, Err(InstructionError::MissingRequiredSignature),);
            })
        })
    }

    #[test]
    fn withdraw_inx_unintialized_acc_insuff_funds_fail() {
        let rent = Rent {
            lamports_per_byte_year: 42,
            ..Rent::default()
        };
        let min_lamports = rent.minimum_balance(NonceState::size());
        with_test_keyed_account(min_lamports + 42, true, |nonce_keyed| {
            let state: NonceState = nonce_keyed.state().unwrap();
            assert_eq!(state, NonceState::Uninitialized);
            with_test_keyed_account(42, false, |mut to_keyed| {
                let mut signers = HashSet::new();
                signers.insert(nonce_keyed.signer_key().unwrap().clone());
                let recent_blockhashes = create_test_recent_blockhashes(0);
                let result = nonce_keyed.withdraw(
                    nonce_keyed.account.lamports + 1,
                    &mut to_keyed,
                    &recent_blockhashes,
                    &rent,
                    &signers,
                );
                assert_eq!(result, Err(InstructionError::InsufficientFunds));
            })
        })
    }

    #[test]
    fn withdraw_inx_uninitialized_acc_two_withdraws_ok() {
        let rent = Rent {
            lamports_per_byte_year: 42,
            ..Rent::default()
        };
        let min_lamports = rent.minimum_balance(NonceState::size());
        with_test_keyed_account(min_lamports + 42, true, |nonce_keyed| {
            with_test_keyed_account(42, false, |mut to_keyed| {
                let mut signers = HashSet::new();
                signers.insert(nonce_keyed.signer_key().unwrap().clone());
                let recent_blockhashes = create_test_recent_blockhashes(0);
                let withdraw_lamports = nonce_keyed.account.lamports / 2;
                let nonce_expect_lamports = nonce_keyed.account.lamports - withdraw_lamports;
                let to_expect_lamports = to_keyed.account.lamports + withdraw_lamports;
                nonce_keyed
                    .withdraw(
                        withdraw_lamports,
                        &mut to_keyed,
                        &recent_blockhashes,
                        &rent,
                        &signers,
                    )
                    .unwrap();
                let state: NonceState = nonce_keyed.state().unwrap();
                assert_eq!(state, NonceState::Uninitialized);
                assert_eq!(nonce_keyed.account.lamports, nonce_expect_lamports);
                assert_eq!(to_keyed.account.lamports, to_expect_lamports);
                let withdraw_lamports = nonce_keyed.account.lamports;
                let nonce_expect_lamports = nonce_keyed.account.lamports - withdraw_lamports;
                let to_expect_lamports = to_keyed.account.lamports + withdraw_lamports;
                nonce_keyed
                    .withdraw(
                        withdraw_lamports,
                        &mut to_keyed,
                        &recent_blockhashes,
                        &rent,
                        &signers,
                    )
                    .unwrap();
                let state: NonceState = nonce_keyed.state().unwrap();
                assert_eq!(state, NonceState::Uninitialized);
                assert_eq!(nonce_keyed.account.lamports, nonce_expect_lamports);
                assert_eq!(to_keyed.account.lamports, to_expect_lamports);
            })
        })
    }

    #[test]
    fn withdraw_inx_initialized_acc_two_withdraws_ok() {
        let rent = Rent {
            lamports_per_byte_year: 42,
            ..Rent::default()
        };
        let min_lamports = rent.minimum_balance(NonceState::size());
        let meta = Meta::new();
        with_test_keyed_account(min_lamports + 42, true, |nonce_keyed| {
            let mut signers = HashSet::new();
            signers.insert(nonce_keyed.signer_key().unwrap().clone());
            let recent_blockhashes = create_test_recent_blockhashes(31);
            nonce_keyed
                .initialize(&recent_blockhashes, &rent, &signers)
                .unwrap();
            let state: NonceState = nonce_keyed.state().unwrap();
            let stored = recent_blockhashes[0];
            assert_eq!(state, NonceState::Initialized(meta, stored));
            with_test_keyed_account(42, false, |mut to_keyed| {
                let withdraw_lamports = nonce_keyed.account.lamports - min_lamports;
                let nonce_expect_lamports = nonce_keyed.account.lamports - withdraw_lamports;
                let to_expect_lamports = to_keyed.account.lamports + withdraw_lamports;
                nonce_keyed
                    .withdraw(
                        withdraw_lamports,
                        &mut to_keyed,
                        &recent_blockhashes,
                        &rent,
                        &signers,
                    )
                    .unwrap();
                let state: NonceState = nonce_keyed.state().unwrap();
                let stored = recent_blockhashes[0];
                assert_eq!(state, NonceState::Initialized(meta, stored));
                assert_eq!(nonce_keyed.account.lamports, nonce_expect_lamports);
                assert_eq!(to_keyed.account.lamports, to_expect_lamports);
                let recent_blockhashes = create_test_recent_blockhashes(0);
                let withdraw_lamports = nonce_keyed.account.lamports;
                let nonce_expect_lamports = nonce_keyed.account.lamports - withdraw_lamports;
                let to_expect_lamports = to_keyed.account.lamports + withdraw_lamports;
                nonce_keyed
                    .withdraw(
                        withdraw_lamports,
                        &mut to_keyed,
                        &recent_blockhashes,
                        &rent,
                        &signers,
                    )
                    .unwrap();
                let state: NonceState = nonce_keyed.state().unwrap();
                assert_eq!(state, NonceState::Uninitialized);
                assert_eq!(nonce_keyed.account.lamports, nonce_expect_lamports);
                assert_eq!(to_keyed.account.lamports, to_expect_lamports);
            })
        })
    }

    #[test]
    fn withdraw_inx_initialized_acc_nonce_too_early_fail() {
        let rent = Rent {
            lamports_per_byte_year: 42,
            ..Rent::default()
        };
        let min_lamports = rent.minimum_balance(NonceState::size());
        with_test_keyed_account(min_lamports + 42, true, |nonce_keyed| {
            let mut signers = HashSet::new();
            signers.insert(nonce_keyed.signer_key().unwrap().clone());
            let recent_blockhashes = create_test_recent_blockhashes(0);
            nonce_keyed
                .initialize(&recent_blockhashes, &rent, &signers)
                .unwrap();
            with_test_keyed_account(42, false, |mut to_keyed| {
                let mut signers = HashSet::new();
                signers.insert(nonce_keyed.signer_key().unwrap().clone());
                let withdraw_lamports = nonce_keyed.account.lamports;
                let result = nonce_keyed.withdraw(
                    withdraw_lamports,
                    &mut to_keyed,
                    &recent_blockhashes,
                    &rent,
                    &signers,
                );
                assert_eq!(result, Err(NonceError::NotExpired.into()));
            })
        })
    }

    #[test]
    fn withdraw_inx_initialized_acc_insuff_funds_fail() {
        let rent = Rent {
            lamports_per_byte_year: 42,
            ..Rent::default()
        };
        let min_lamports = rent.minimum_balance(NonceState::size());
        with_test_keyed_account(min_lamports + 42, true, |nonce_keyed| {
            let mut signers = HashSet::new();
            signers.insert(nonce_keyed.signer_key().unwrap().clone());
            let recent_blockhashes = create_test_recent_blockhashes(95);
            nonce_keyed
                .initialize(&recent_blockhashes, &rent, &signers)
                .unwrap();
            with_test_keyed_account(42, false, |mut to_keyed| {
                let recent_blockhashes = create_test_recent_blockhashes(63);
                let mut signers = HashSet::new();
                signers.insert(nonce_keyed.signer_key().unwrap().clone());
                let withdraw_lamports = nonce_keyed.account.lamports + 1;
                let result = nonce_keyed.withdraw(
                    withdraw_lamports,
                    &mut to_keyed,
                    &recent_blockhashes,
                    &rent,
                    &signers,
                );
                assert_eq!(result, Err(InstructionError::InsufficientFunds));
            })
        })
    }

    #[test]
    fn withdraw_inx_initialized_acc_insuff_rent_fail() {
        let rent = Rent {
            lamports_per_byte_year: 42,
            ..Rent::default()
        };
        let min_lamports = rent.minimum_balance(NonceState::size());
        with_test_keyed_account(min_lamports + 42, true, |nonce_keyed| {
            let mut signers = HashSet::new();
            signers.insert(nonce_keyed.signer_key().unwrap().clone());
            let recent_blockhashes = create_test_recent_blockhashes(95);
            nonce_keyed
                .initialize(&recent_blockhashes, &rent, &signers)
                .unwrap();
            with_test_keyed_account(42, false, |mut to_keyed| {
                let recent_blockhashes = create_test_recent_blockhashes(63);
                let mut signers = HashSet::new();
                signers.insert(nonce_keyed.signer_key().unwrap().clone());
                let withdraw_lamports = nonce_keyed.account.lamports - min_lamports + 1;
                let result = nonce_keyed.withdraw(
                    withdraw_lamports,
                    &mut to_keyed,
                    &recent_blockhashes,
                    &rent,
                    &signers,
                );
                assert_eq!(result, Err(InstructionError::InsufficientFunds));
            })
        })
    }

    #[test]
    fn initialize_inx_ok() {
        let rent = Rent {
            lamports_per_byte_year: 42,
            ..Rent::default()
        };
        let min_lamports = rent.minimum_balance(NonceState::size());
        with_test_keyed_account(min_lamports + 42, true, |keyed_account| {
            let state: NonceState = keyed_account.state().unwrap();
            assert_eq!(state, NonceState::Uninitialized);
            let mut signers = HashSet::new();
            signers.insert(keyed_account.signer_key().unwrap().clone());
            let recent_blockhashes = create_test_recent_blockhashes(0);
            let stored = recent_blockhashes[0];
            let result = keyed_account.initialize(&recent_blockhashes, &rent, &signers);
            assert_eq!(result, Ok(()));
            let state: NonceState = keyed_account.state().unwrap();
            assert_eq!(state, NonceState::Initialized(Meta::new(), stored));
        })
    }

    #[test]
    fn initialize_inx_empty_recent_blockhashes_fail() {
        let rent = Rent {
            lamports_per_byte_year: 42,
            ..Rent::default()
        };
        let min_lamports = rent.minimum_balance(NonceState::size());
        with_test_keyed_account(min_lamports + 42, true, |keyed_account| {
            let mut signers = HashSet::new();
            signers.insert(keyed_account.signer_key().unwrap().clone());
            let recent_blockhashes = RecentBlockhashes::from_iter(vec![].into_iter());
            let result = keyed_account.initialize(&recent_blockhashes, &rent, &signers);
            assert_eq!(result, Err(NonceError::NoRecentBlockhashes.into()));
        })
    }

    #[test]
    fn initialize_inx_not_signer_fail() {
        let rent = Rent {
            lamports_per_byte_year: 42,
            ..Rent::default()
        };
        let min_lamports = rent.minimum_balance(NonceState::size());
        with_test_keyed_account(min_lamports + 42, false, |keyed_account| {
            let signers = HashSet::new();
            let recent_blockhashes = create_test_recent_blockhashes(0);
            let result = keyed_account.initialize(&recent_blockhashes, &rent, &signers);
            assert_eq!(result, Err(InstructionError::MissingRequiredSignature));
        })
    }

    #[test]
    fn initialize_inx_initialized_account_fail() {
        let rent = Rent {
            lamports_per_byte_year: 42,
            ..Rent::default()
        };
        let min_lamports = rent.minimum_balance(NonceState::size());
        with_test_keyed_account(min_lamports + 42, true, |keyed_account| {
            let mut signers = HashSet::new();
            signers.insert(keyed_account.signer_key().unwrap().clone());
            let recent_blockhashes = create_test_recent_blockhashes(31);
            keyed_account
                .initialize(&recent_blockhashes, &rent, &signers)
                .unwrap();
            let recent_blockhashes = create_test_recent_blockhashes(0);
            let result = keyed_account.initialize(&recent_blockhashes, &rent, &signers);
            assert_eq!(result, Err(NonceError::BadAccountState.into()));
        })
    }

    #[test]
    fn initialize_inx_uninitialized_acc_insuff_funds_fail() {
        let rent = Rent {
            lamports_per_byte_year: 42,
            ..Rent::default()
        };
        let min_lamports = rent.minimum_balance(NonceState::size());
        with_test_keyed_account(min_lamports - 42, true, |keyed_account| {
            let mut signers = HashSet::new();
            signers.insert(keyed_account.signer_key().unwrap().clone());
            let recent_blockhashes = create_test_recent_blockhashes(63);
            let result = keyed_account.initialize(&recent_blockhashes, &rent, &signers);
            assert_eq!(result, Err(InstructionError::InsufficientFunds));
        })
    }
}
