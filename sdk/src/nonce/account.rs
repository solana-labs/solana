use crate::{
    account::{self, KeyedAccount},
    account_utils::State as AccountUtilsState,
    instruction::InstructionError,
    nonce::{self, State},
    pubkey::Pubkey,
    system_instruction::NonceError,
    system_program,
    sysvar::{recent_blockhashes::RecentBlockhashes, rent::Rent},
};
use std::{cell::RefCell, collections::HashSet};

pub trait Account {
    fn advance_nonce_account(
        &self,
        recent_blockhashes: &RecentBlockhashes,
        signers: &HashSet<Pubkey>,
    ) -> Result<(), InstructionError>;
    fn withdraw_nonce_account(
        &self,
        lamports: u64,
        to: &KeyedAccount,
        recent_blockhashes: &RecentBlockhashes,
        rent: &Rent,
        signers: &HashSet<Pubkey>,
    ) -> Result<(), InstructionError>;
    fn initialize_nonce_account(
        &self,
        nonce_authority: &Pubkey,
        recent_blockhashes: &RecentBlockhashes,
        rent: &Rent,
    ) -> Result<(), InstructionError>;
    fn authorize_nonce_account(
        &self,
        nonce_authority: &Pubkey,
        signers: &HashSet<Pubkey>,
    ) -> Result<(), InstructionError>;
}

impl<'a> Account for KeyedAccount<'a> {
    fn advance_nonce_account(
        &self,
        recent_blockhashes: &RecentBlockhashes,
        signers: &HashSet<Pubkey>,
    ) -> Result<(), InstructionError> {
        if recent_blockhashes.is_empty() {
            return Err(NonceError::NoRecentBlockhashes.into());
        }

        let meta = match self.state()? {
            State::Initialized(meta, ref hash) => {
                if !signers.contains(&meta.nonce_authority) {
                    return Err(InstructionError::MissingRequiredSignature);
                }
                if *hash == recent_blockhashes[0] {
                    return Err(NonceError::NotExpired.into());
                }
                meta
            }
            _ => return Err(NonceError::BadAccountState.into()),
        };

        self.set_state(&State::Initialized(meta, recent_blockhashes[0]))
    }

    fn withdraw_nonce_account(
        &self,
        lamports: u64,
        to: &KeyedAccount,
        recent_blockhashes: &RecentBlockhashes,
        rent: &Rent,
        signers: &HashSet<Pubkey>,
    ) -> Result<(), InstructionError> {
        let signer = match self.state()? {
            State::Uninitialized => {
                if lamports > self.lamports()? {
                    return Err(InstructionError::InsufficientFunds);
                }
                *self.unsigned_key()
            }
            State::Initialized(meta, ref hash) => {
                if lamports == self.lamports()? {
                    if *hash == recent_blockhashes[0] {
                        return Err(NonceError::NotExpired.into());
                    }
                } else {
                    let min_balance = rent.minimum_balance(self.data_len()?);
                    if lamports + min_balance > self.lamports()? {
                        return Err(InstructionError::InsufficientFunds);
                    }
                }
                meta.nonce_authority
            }
        };

        if !signers.contains(&signer) {
            return Err(InstructionError::MissingRequiredSignature);
        }

        self.try_account_ref_mut()?.lamports -= lamports;
        to.try_account_ref_mut()?.lamports += lamports;

        Ok(())
    }

    fn initialize_nonce_account(
        &self,
        nonce_authority: &Pubkey,
        recent_blockhashes: &RecentBlockhashes,
        rent: &Rent,
    ) -> Result<(), InstructionError> {
        if recent_blockhashes.is_empty() {
            return Err(NonceError::NoRecentBlockhashes.into());
        }

        let meta = match self.state()? {
            State::Uninitialized => {
                let min_balance = rent.minimum_balance(self.data_len()?);
                if self.lamports()? < min_balance {
                    return Err(InstructionError::InsufficientFunds);
                }
                nonce::state::Meta::new(nonce_authority)
            }
            _ => return Err(NonceError::BadAccountState.into()),
        };

        self.set_state(&State::Initialized(meta, recent_blockhashes[0]))
    }

    fn authorize_nonce_account(
        &self,
        nonce_authority: &Pubkey,
        signers: &HashSet<Pubkey>,
    ) -> Result<(), InstructionError> {
        match self.state()? {
            State::Initialized(meta, nonce) => {
                if !signers.contains(&meta.nonce_authority) {
                    return Err(InstructionError::MissingRequiredSignature);
                }
                self.set_state(&State::Initialized(
                    nonce::state::Meta::new(nonce_authority),
                    nonce,
                ))
            }
            _ => Err(NonceError::BadAccountState.into()),
        }
    }
}

pub fn create_account(lamports: u64) -> RefCell<account::Account> {
    RefCell::new(
        account::Account::new_data_with_space(
            lamports,
            &State::Uninitialized,
            State::size(),
            &system_program::id(),
        )
        .expect("nonce_account"),
    )
}

/// Convenience function for working with keyed accounts in tests
#[cfg(not(feature = "program"))]
pub fn with_test_keyed_account<F>(lamports: u64, signer: bool, f: F)
where
    F: Fn(&KeyedAccount),
{
    let pubkey = Pubkey::new_rand();
    let account = create_account(lamports);
    let keyed_account = KeyedAccount::new(&pubkey, signer, &account);
    f(&keyed_account)
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{
        account::KeyedAccount,
        nonce::{self, State},
        system_instruction::NonceError,
        sysvar::recent_blockhashes::{create_test_recent_blockhashes, RecentBlockhashes},
    };
    use std::iter::FromIterator;

    #[test]
    fn default_is_uninitialized() {
        assert_eq!(State::default(), State::Uninitialized)
    }

    #[test]
    fn new_meta() {
        let nonce_authority = Pubkey::default();
        assert_eq!(
            nonce::state::Meta::new(&nonce_authority),
            nonce::state::Meta { nonce_authority }
        );
    }

    #[test]
    fn keyed_account_expected_behavior() {
        let rent = Rent {
            lamports_per_byte_year: 42,
            ..Rent::default()
        };
        let min_lamports = rent.minimum_balance(State::size());
        with_test_keyed_account(min_lamports + 42, true, |keyed_account| {
            let meta = nonce::state::Meta::new(&keyed_account.unsigned_key());
            let mut signers = HashSet::new();
            signers.insert(keyed_account.signer_key().unwrap().clone());
            let state: State = keyed_account.state().unwrap();
            // New is in Uninitialzed state
            assert_eq!(state, State::Uninitialized);
            let recent_blockhashes = create_test_recent_blockhashes(95);
            let authorized = keyed_account.unsigned_key().clone();
            keyed_account
                .initialize_nonce_account(&authorized, &recent_blockhashes, &rent)
                .unwrap();
            let state: State = keyed_account.state().unwrap();
            let stored = recent_blockhashes[0];
            // First nonce instruction drives state from Uninitialized to Initialized
            assert_eq!(state, State::Initialized(meta, stored));
            let recent_blockhashes = create_test_recent_blockhashes(63);
            keyed_account
                .advance_nonce_account(&recent_blockhashes, &signers)
                .unwrap();
            let state: State = keyed_account.state().unwrap();
            let stored = recent_blockhashes[0];
            // Second nonce instruction consumes and replaces stored nonce
            assert_eq!(state, State::Initialized(meta, stored));
            let recent_blockhashes = create_test_recent_blockhashes(31);
            keyed_account
                .advance_nonce_account(&recent_blockhashes, &signers)
                .unwrap();
            let state: State = keyed_account.state().unwrap();
            let stored = recent_blockhashes[0];
            // Third nonce instruction for fun and profit
            assert_eq!(state, State::Initialized(meta, stored));
            with_test_keyed_account(42, false, |to_keyed| {
                let recent_blockhashes = create_test_recent_blockhashes(0);
                let withdraw_lamports = keyed_account.account.borrow().lamports;
                let expect_nonce_lamports =
                    keyed_account.account.borrow().lamports - withdraw_lamports;
                let expect_to_lamports = to_keyed.account.borrow().lamports + withdraw_lamports;
                keyed_account
                    .withdraw_nonce_account(
                        withdraw_lamports,
                        &to_keyed,
                        &recent_blockhashes,
                        &rent,
                        &signers,
                    )
                    .unwrap();
                // Empties Account balance
                assert_eq!(
                    keyed_account.account.borrow().lamports,
                    expect_nonce_lamports
                );
                // Account balance goes to `to`
                assert_eq!(to_keyed.account.borrow().lamports, expect_to_lamports);
            })
        })
    }

    #[test]
    fn nonce_inx_initialized_account_not_signer_fail() {
        let rent = Rent {
            lamports_per_byte_year: 42,
            ..Rent::default()
        };
        let min_lamports = rent.minimum_balance(State::size());
        with_test_keyed_account(min_lamports + 42, true, |nonce_account| {
            let recent_blockhashes = create_test_recent_blockhashes(31);
            let stored = recent_blockhashes[0];
            let authorized = nonce_account.unsigned_key().clone();
            let meta = nonce::state::Meta::new(&authorized);
            nonce_account
                .initialize_nonce_account(&authorized, &recent_blockhashes, &rent)
                .unwrap();
            let pubkey = nonce_account.account.borrow().owner.clone();
            let nonce_account = KeyedAccount::new(&pubkey, false, nonce_account.account);
            let state: State = nonce_account.state().unwrap();
            assert_eq!(state, State::Initialized(meta, stored));
            let signers = HashSet::new();
            let recent_blockhashes = create_test_recent_blockhashes(0);
            let result = nonce_account.advance_nonce_account(&recent_blockhashes, &signers);
            assert_eq!(result, Err(InstructionError::MissingRequiredSignature),);
        })
    }

    #[test]
    fn nonce_inx_with_empty_recent_blockhashes_fail() {
        let rent = Rent {
            lamports_per_byte_year: 42,
            ..Rent::default()
        };
        let min_lamports = rent.minimum_balance(State::size());
        with_test_keyed_account(min_lamports + 42, true, |keyed_account| {
            let mut signers = HashSet::new();
            signers.insert(keyed_account.signer_key().unwrap().clone());
            let recent_blockhashes = create_test_recent_blockhashes(0);
            let authorized = keyed_account.unsigned_key().clone();
            keyed_account
                .initialize_nonce_account(&authorized, &recent_blockhashes, &rent)
                .unwrap();
            let recent_blockhashes = RecentBlockhashes::from_iter(vec![].into_iter());
            let result = keyed_account.advance_nonce_account(&recent_blockhashes, &signers);
            assert_eq!(result, Err(NonceError::NoRecentBlockhashes.into()));
        })
    }

    #[test]
    fn nonce_inx_too_early_fail() {
        let rent = Rent {
            lamports_per_byte_year: 42,
            ..Rent::default()
        };
        let min_lamports = rent.minimum_balance(State::size());
        with_test_keyed_account(min_lamports + 42, true, |keyed_account| {
            let mut signers = HashSet::new();
            signers.insert(keyed_account.signer_key().unwrap().clone());
            let recent_blockhashes = create_test_recent_blockhashes(63);
            let authorized = keyed_account.unsigned_key().clone();
            keyed_account
                .initialize_nonce_account(&authorized, &recent_blockhashes, &rent)
                .unwrap();
            let result = keyed_account.advance_nonce_account(&recent_blockhashes, &signers);
            assert_eq!(result, Err(NonceError::NotExpired.into()));
        })
    }

    #[test]
    fn nonce_inx_uninitialized_account_fail() {
        let rent = Rent {
            lamports_per_byte_year: 42,
            ..Rent::default()
        };
        let min_lamports = rent.minimum_balance(State::size());
        with_test_keyed_account(min_lamports + 42, true, |keyed_account| {
            let mut signers = HashSet::new();
            signers.insert(keyed_account.signer_key().unwrap().clone());
            let recent_blockhashes = create_test_recent_blockhashes(63);
            let result = keyed_account.advance_nonce_account(&recent_blockhashes, &signers);
            assert_eq!(result, Err(NonceError::BadAccountState.into()));
        })
    }

    #[test]
    fn nonce_inx_independent_nonce_authority_ok() {
        let rent = Rent {
            lamports_per_byte_year: 42,
            ..Rent::default()
        };
        let min_lamports = rent.minimum_balance(State::size());
        with_test_keyed_account(min_lamports + 42, true, |nonce_account| {
            with_test_keyed_account(42, true, |nonce_authority| {
                let mut signers = HashSet::new();
                signers.insert(nonce_account.signer_key().unwrap().clone());
                let recent_blockhashes = create_test_recent_blockhashes(63);
                let authorized = nonce_authority.unsigned_key().clone();
                nonce_account
                    .initialize_nonce_account(&authorized, &recent_blockhashes, &rent)
                    .unwrap();
                let mut signers = HashSet::new();
                signers.insert(nonce_authority.signer_key().unwrap().clone());
                let recent_blockhashes = create_test_recent_blockhashes(31);
                let result = nonce_account.advance_nonce_account(&recent_blockhashes, &signers);
                assert_eq!(result, Ok(()));
            });
        });
    }

    #[test]
    fn nonce_inx_no_nonce_authority_sig_fail() {
        let rent = Rent {
            lamports_per_byte_year: 42,
            ..Rent::default()
        };
        let min_lamports = rent.minimum_balance(State::size());
        with_test_keyed_account(min_lamports + 42, true, |nonce_account| {
            with_test_keyed_account(42, false, |nonce_authority| {
                let mut signers = HashSet::new();
                signers.insert(nonce_account.signer_key().unwrap().clone());
                let recent_blockhashes = create_test_recent_blockhashes(63);
                let authorized = nonce_authority.unsigned_key().clone();
                nonce_account
                    .initialize_nonce_account(&authorized, &recent_blockhashes, &rent)
                    .unwrap();
                let result = nonce_account.advance_nonce_account(&recent_blockhashes, &signers);
                assert_eq!(result, Err(InstructionError::MissingRequiredSignature),);
            });
        });
    }

    #[test]
    fn withdraw_inx_unintialized_acc_ok() {
        let rent = Rent {
            lamports_per_byte_year: 42,
            ..Rent::default()
        };
        let min_lamports = rent.minimum_balance(State::size());
        with_test_keyed_account(min_lamports + 42, true, |nonce_keyed| {
            let state: State = nonce_keyed.state().unwrap();
            assert_eq!(state, State::Uninitialized);
            with_test_keyed_account(42, false, |to_keyed| {
                let mut signers = HashSet::new();
                signers.insert(nonce_keyed.signer_key().unwrap().clone());
                let recent_blockhashes = create_test_recent_blockhashes(0);
                let withdraw_lamports = nonce_keyed.account.borrow().lamports;
                let expect_nonce_lamports =
                    nonce_keyed.account.borrow().lamports - withdraw_lamports;
                let expect_to_lamports = to_keyed.account.borrow().lamports + withdraw_lamports;
                nonce_keyed
                    .withdraw_nonce_account(
                        withdraw_lamports,
                        &to_keyed,
                        &recent_blockhashes,
                        &rent,
                        &signers,
                    )
                    .unwrap();
                let state: State = nonce_keyed.state().unwrap();
                // Withdraw instruction...
                // Deinitializes Account state
                assert_eq!(state, State::Uninitialized);
                // Empties Account balance
                assert_eq!(nonce_keyed.account.borrow().lamports, expect_nonce_lamports);
                // Account balance goes to `to`
                assert_eq!(to_keyed.account.borrow().lamports, expect_to_lamports);
            })
        })
    }

    #[test]
    fn withdraw_inx_unintialized_acc_unsigned_fail() {
        let rent = Rent {
            lamports_per_byte_year: 42,
            ..Rent::default()
        };
        let min_lamports = rent.minimum_balance(State::size());
        with_test_keyed_account(min_lamports + 42, false, |nonce_keyed| {
            let state: State = nonce_keyed.state().unwrap();
            assert_eq!(state, State::Uninitialized);
            with_test_keyed_account(42, false, |to_keyed| {
                let signers = HashSet::new();
                let recent_blockhashes = create_test_recent_blockhashes(0);
                let lamports = nonce_keyed.account.borrow().lamports;
                let result = nonce_keyed.withdraw_nonce_account(
                    lamports,
                    &to_keyed,
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
        let min_lamports = rent.minimum_balance(State::size());
        with_test_keyed_account(min_lamports + 42, true, |nonce_keyed| {
            let state: State = nonce_keyed.state().unwrap();
            assert_eq!(state, State::Uninitialized);
            with_test_keyed_account(42, false, |to_keyed| {
                let mut signers = HashSet::new();
                signers.insert(nonce_keyed.signer_key().unwrap().clone());
                let recent_blockhashes = create_test_recent_blockhashes(0);
                let lamports = nonce_keyed.account.borrow().lamports + 1;
                let result = nonce_keyed.withdraw_nonce_account(
                    lamports,
                    &to_keyed,
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
        let min_lamports = rent.minimum_balance(State::size());
        with_test_keyed_account(min_lamports + 42, true, |nonce_keyed| {
            with_test_keyed_account(42, false, |to_keyed| {
                let mut signers = HashSet::new();
                signers.insert(nonce_keyed.signer_key().unwrap().clone());
                let recent_blockhashes = create_test_recent_blockhashes(0);
                let withdraw_lamports = nonce_keyed.account.borrow().lamports / 2;
                let nonce_expect_lamports =
                    nonce_keyed.account.borrow().lamports - withdraw_lamports;
                let to_expect_lamports = to_keyed.account.borrow().lamports + withdraw_lamports;
                nonce_keyed
                    .withdraw_nonce_account(
                        withdraw_lamports,
                        &to_keyed,
                        &recent_blockhashes,
                        &rent,
                        &signers,
                    )
                    .unwrap();
                let state: State = nonce_keyed.state().unwrap();
                assert_eq!(state, State::Uninitialized);
                assert_eq!(nonce_keyed.account.borrow().lamports, nonce_expect_lamports);
                assert_eq!(to_keyed.account.borrow().lamports, to_expect_lamports);
                let withdraw_lamports = nonce_keyed.account.borrow().lamports;
                let nonce_expect_lamports =
                    nonce_keyed.account.borrow().lamports - withdraw_lamports;
                let to_expect_lamports = to_keyed.account.borrow().lamports + withdraw_lamports;
                nonce_keyed
                    .withdraw_nonce_account(
                        withdraw_lamports,
                        &to_keyed,
                        &recent_blockhashes,
                        &rent,
                        &signers,
                    )
                    .unwrap();
                let state: State = nonce_keyed.state().unwrap();
                assert_eq!(state, State::Uninitialized);
                assert_eq!(nonce_keyed.account.borrow().lamports, nonce_expect_lamports);
                assert_eq!(to_keyed.account.borrow().lamports, to_expect_lamports);
            })
        })
    }

    #[test]
    fn withdraw_inx_initialized_acc_two_withdraws_ok() {
        let rent = Rent {
            lamports_per_byte_year: 42,
            ..Rent::default()
        };
        let min_lamports = rent.minimum_balance(State::size());
        with_test_keyed_account(min_lamports + 42, true, |nonce_keyed| {
            let mut signers = HashSet::new();
            signers.insert(nonce_keyed.signer_key().unwrap().clone());
            let recent_blockhashes = create_test_recent_blockhashes(31);
            let authorized = nonce_keyed.unsigned_key().clone();
            let meta = nonce::state::Meta::new(&authorized);
            nonce_keyed
                .initialize_nonce_account(&authorized, &recent_blockhashes, &rent)
                .unwrap();
            let state: State = nonce_keyed.state().unwrap();
            let stored = recent_blockhashes[0];
            assert_eq!(state, State::Initialized(meta, stored));
            with_test_keyed_account(42, false, |to_keyed| {
                let withdraw_lamports = nonce_keyed.account.borrow().lamports - min_lamports;
                let nonce_expect_lamports =
                    nonce_keyed.account.borrow().lamports - withdraw_lamports;
                let to_expect_lamports = to_keyed.account.borrow().lamports + withdraw_lamports;
                nonce_keyed
                    .withdraw_nonce_account(
                        withdraw_lamports,
                        &to_keyed,
                        &recent_blockhashes,
                        &rent,
                        &signers,
                    )
                    .unwrap();
                let state: State = nonce_keyed.state().unwrap();
                let stored = recent_blockhashes[0];
                assert_eq!(state, State::Initialized(meta, stored));
                assert_eq!(nonce_keyed.account.borrow().lamports, nonce_expect_lamports);
                assert_eq!(to_keyed.account.borrow().lamports, to_expect_lamports);
                let recent_blockhashes = create_test_recent_blockhashes(0);
                let withdraw_lamports = nonce_keyed.account.borrow().lamports;
                let nonce_expect_lamports =
                    nonce_keyed.account.borrow().lamports - withdraw_lamports;
                let to_expect_lamports = to_keyed.account.borrow().lamports + withdraw_lamports;
                nonce_keyed
                    .withdraw_nonce_account(
                        withdraw_lamports,
                        &to_keyed,
                        &recent_blockhashes,
                        &rent,
                        &signers,
                    )
                    .unwrap();
                assert_eq!(nonce_keyed.account.borrow().lamports, nonce_expect_lamports);
                assert_eq!(to_keyed.account.borrow().lamports, to_expect_lamports);
            })
        })
    }

    #[test]
    fn withdraw_inx_initialized_acc_nonce_too_early_fail() {
        let rent = Rent {
            lamports_per_byte_year: 42,
            ..Rent::default()
        };
        let min_lamports = rent.minimum_balance(State::size());
        with_test_keyed_account(min_lamports + 42, true, |nonce_keyed| {
            let recent_blockhashes = create_test_recent_blockhashes(0);
            let authorized = nonce_keyed.unsigned_key().clone();
            nonce_keyed
                .initialize_nonce_account(&authorized, &recent_blockhashes, &rent)
                .unwrap();
            with_test_keyed_account(42, false, |to_keyed| {
                let mut signers = HashSet::new();
                signers.insert(nonce_keyed.signer_key().unwrap().clone());
                let withdraw_lamports = nonce_keyed.account.borrow().lamports;
                let result = nonce_keyed.withdraw_nonce_account(
                    withdraw_lamports,
                    &to_keyed,
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
        let min_lamports = rent.minimum_balance(State::size());
        with_test_keyed_account(min_lamports + 42, true, |nonce_keyed| {
            let recent_blockhashes = create_test_recent_blockhashes(95);
            let authorized = nonce_keyed.unsigned_key().clone();
            nonce_keyed
                .initialize_nonce_account(&authorized, &recent_blockhashes, &rent)
                .unwrap();
            with_test_keyed_account(42, false, |to_keyed| {
                let recent_blockhashes = create_test_recent_blockhashes(63);
                let mut signers = HashSet::new();
                signers.insert(nonce_keyed.signer_key().unwrap().clone());
                let withdraw_lamports = nonce_keyed.account.borrow().lamports + 1;
                let result = nonce_keyed.withdraw_nonce_account(
                    withdraw_lamports,
                    &to_keyed,
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
        let min_lamports = rent.minimum_balance(State::size());
        with_test_keyed_account(min_lamports + 42, true, |nonce_keyed| {
            let recent_blockhashes = create_test_recent_blockhashes(95);
            let authorized = nonce_keyed.unsigned_key().clone();
            nonce_keyed
                .initialize_nonce_account(&authorized, &recent_blockhashes, &rent)
                .unwrap();
            with_test_keyed_account(42, false, |to_keyed| {
                let recent_blockhashes = create_test_recent_blockhashes(63);
                let mut signers = HashSet::new();
                signers.insert(nonce_keyed.signer_key().unwrap().clone());
                let withdraw_lamports = nonce_keyed.account.borrow().lamports - min_lamports + 1;
                let result = nonce_keyed.withdraw_nonce_account(
                    withdraw_lamports,
                    &to_keyed,
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
        let min_lamports = rent.minimum_balance(State::size());
        with_test_keyed_account(min_lamports + 42, true, |keyed_account| {
            let state: State = keyed_account.state().unwrap();
            assert_eq!(state, State::Uninitialized);
            let mut signers = HashSet::new();
            signers.insert(keyed_account.signer_key().unwrap().clone());
            let recent_blockhashes = create_test_recent_blockhashes(0);
            let stored = recent_blockhashes[0];
            let authorized = keyed_account.unsigned_key().clone();
            let meta = nonce::state::Meta::new(&authorized);
            let result =
                keyed_account.initialize_nonce_account(&authorized, &recent_blockhashes, &rent);
            assert_eq!(result, Ok(()));
            let state: State = keyed_account.state().unwrap();
            assert_eq!(state, State::Initialized(meta, stored));
        })
    }

    #[test]
    fn initialize_inx_empty_recent_blockhashes_fail() {
        let rent = Rent {
            lamports_per_byte_year: 42,
            ..Rent::default()
        };
        let min_lamports = rent.minimum_balance(State::size());
        with_test_keyed_account(min_lamports + 42, true, |keyed_account| {
            let mut signers = HashSet::new();
            signers.insert(keyed_account.signer_key().unwrap().clone());
            let recent_blockhashes = RecentBlockhashes::from_iter(vec![].into_iter());
            let authorized = keyed_account.unsigned_key().clone();
            let result =
                keyed_account.initialize_nonce_account(&authorized, &recent_blockhashes, &rent);
            assert_eq!(result, Err(NonceError::NoRecentBlockhashes.into()));
        })
    }

    #[test]
    fn initialize_inx_initialized_account_fail() {
        let rent = Rent {
            lamports_per_byte_year: 42,
            ..Rent::default()
        };
        let min_lamports = rent.minimum_balance(State::size());
        with_test_keyed_account(min_lamports + 42, true, |keyed_account| {
            let recent_blockhashes = create_test_recent_blockhashes(31);
            let authorized = keyed_account.unsigned_key().clone();
            keyed_account
                .initialize_nonce_account(&authorized, &recent_blockhashes, &rent)
                .unwrap();
            let recent_blockhashes = create_test_recent_blockhashes(0);
            let result =
                keyed_account.initialize_nonce_account(&authorized, &recent_blockhashes, &rent);
            assert_eq!(result, Err(NonceError::BadAccountState.into()));
        })
    }

    #[test]
    fn initialize_inx_uninitialized_acc_insuff_funds_fail() {
        let rent = Rent {
            lamports_per_byte_year: 42,
            ..Rent::default()
        };
        let min_lamports = rent.minimum_balance(State::size());
        with_test_keyed_account(min_lamports - 42, true, |keyed_account| {
            let recent_blockhashes = create_test_recent_blockhashes(63);
            let authorized = keyed_account.unsigned_key().clone();
            let result =
                keyed_account.initialize_nonce_account(&authorized, &recent_blockhashes, &rent);
            assert_eq!(result, Err(InstructionError::InsufficientFunds));
        })
    }

    #[test]
    fn authorize_inx_ok() {
        let rent = Rent {
            lamports_per_byte_year: 42,
            ..Rent::default()
        };
        let min_lamports = rent.minimum_balance(State::size());
        with_test_keyed_account(min_lamports + 42, true, |nonce_account| {
            let mut signers = HashSet::new();
            signers.insert(nonce_account.signer_key().unwrap().clone());
            let recent_blockhashes = create_test_recent_blockhashes(31);
            let stored = recent_blockhashes[0];
            let authorized = nonce_account.unsigned_key().clone();
            nonce_account
                .initialize_nonce_account(&authorized, &recent_blockhashes, &rent)
                .unwrap();
            let authorized = &Pubkey::default().clone();
            let meta = nonce::state::Meta::new(&authorized);
            let result = nonce_account.authorize_nonce_account(&Pubkey::default(), &signers);
            assert_eq!(result, Ok(()));
            let state: State = nonce_account.state().unwrap();
            assert_eq!(state, State::Initialized(meta, stored));
        })
    }

    #[test]
    fn authorize_inx_uninitialized_state_fail() {
        let rent = Rent {
            lamports_per_byte_year: 42,
            ..Rent::default()
        };
        let min_lamports = rent.minimum_balance(State::size());
        with_test_keyed_account(min_lamports + 42, true, |nonce_account| {
            let mut signers = HashSet::new();
            signers.insert(nonce_account.signer_key().unwrap().clone());
            let result = nonce_account.authorize_nonce_account(&Pubkey::default(), &signers);
            assert_eq!(result, Err(NonceError::BadAccountState.into()));
        })
    }

    #[test]
    fn authorize_inx_bad_authority_fail() {
        let rent = Rent {
            lamports_per_byte_year: 42,
            ..Rent::default()
        };
        let min_lamports = rent.minimum_balance(State::size());
        with_test_keyed_account(min_lamports + 42, true, |nonce_account| {
            let mut signers = HashSet::new();
            signers.insert(nonce_account.signer_key().unwrap().clone());
            let recent_blockhashes = create_test_recent_blockhashes(31);
            let authorized = &Pubkey::default().clone();
            nonce_account
                .initialize_nonce_account(&authorized, &recent_blockhashes, &rent)
                .unwrap();
            let result = nonce_account.authorize_nonce_account(&Pubkey::default(), &signers);
            assert_eq!(result, Err(InstructionError::MissingRequiredSignature));
        })
    }
}
