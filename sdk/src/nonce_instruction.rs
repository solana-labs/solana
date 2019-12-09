use crate::{
    account::{get_signers, KeyedAccount},
    instruction::{AccountMeta, Instruction, InstructionError},
    instruction_processor_utils::{limited_deserialize, next_keyed_account, DecodeError},
    nonce_program::id,
    nonce_state::{NonceAccount, NonceState},
    pubkey::Pubkey,
    system_instruction,
    sysvar::{
        recent_blockhashes::{self, RecentBlockhashes},
        rent::{self, Rent},
        Sysvar,
    },
};
use num_derive::{FromPrimitive, ToPrimitive};
use serde_derive::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Error, Debug, Clone, PartialEq, FromPrimitive, ToPrimitive)]
pub enum NonceError {
    #[error("recent blockhash list is empty")]
    NoRecentBlockhashes,
    #[error("stored nonce is still in recent_blockhashes")]
    NotExpired,
    #[error("specified nonce does not match stored nonce")]
    UnexpectedValue,
    #[error("cannot handle request in current account state")]
    BadAccountState,
}

impl<E> DecodeError<E> for NonceError {
    fn type_of() -> &'static str {
        "NonceError"
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub enum NonceInstruction {
    /// `Nonce` consumes a stored nonce, replacing it with a successor
    ///
    /// Expects 3 Accounts:
    ///     0 - A NonceAccount
    ///     1 - RecentBlockhashes sysvar
    ///
    Nonce,

    /// `Withdraw` transfers funds out of the nonce account
    ///
    /// Expects 4 Accounts:
    ///     0 - A NonceAccount
    ///     1 - A system account to which the lamports will be transferred
    ///     2 - RecentBlockhashes sysvar
    ///     3 - Rent sysvar
    ///
    /// The `u64` parameter is the lamports to withdraw, which must leave the
    /// account balance above the rent exempt reserve or at zero.
    Withdraw(u64),

    /// `Initialize` drives state of Uninitalized NonceAccount to Initialized,
    /// setting the nonce value.
    ///
    /// Expects 3 Accounts:
    ///     0 - A NonceAccount in the Uninitialized state
    ///     1 - RecentBlockHashes sysvar
    ///     2 - Rent sysvar
    Initialize,
}

pub fn create_nonce_account(
    from_pubkey: &Pubkey,
    nonce_pubkey: &Pubkey,
    lamports: u64,
) -> Vec<Instruction> {
    vec![
        system_instruction::create_account(
            from_pubkey,
            nonce_pubkey,
            lamports,
            NonceState::size() as u64,
            &id(),
        ),
        initialize(nonce_pubkey),
    ]
}

fn initialize(nonce_pubkey: &Pubkey) -> Instruction {
    Instruction::new(
        id(),
        &NonceInstruction::Initialize,
        vec![
            AccountMeta::new(*nonce_pubkey, true),
            AccountMeta::new_readonly(recent_blockhashes::id(), false),
            AccountMeta::new_readonly(rent::id(), false),
        ],
    )
}

pub fn nonce(nonce_pubkey: &Pubkey) -> Instruction {
    Instruction::new(
        id(),
        &NonceInstruction::Nonce,
        vec![
            AccountMeta::new(*nonce_pubkey, true),
            AccountMeta::new_readonly(recent_blockhashes::id(), false),
        ],
    )
}

pub fn withdraw(nonce_pubkey: &Pubkey, to_pubkey: &Pubkey, lamports: u64) -> Instruction {
    Instruction::new(
        id(),
        &NonceInstruction::Withdraw(lamports),
        vec![
            AccountMeta::new(*nonce_pubkey, true),
            AccountMeta::new(*to_pubkey, false),
            AccountMeta::new_readonly(recent_blockhashes::id(), false),
            AccountMeta::new_readonly(rent::id(), false),
        ],
    )
}

pub fn process_instruction(
    _program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    data: &[u8],
) -> Result<(), InstructionError> {
    let signers = get_signers(keyed_accounts);

    let keyed_accounts = &mut keyed_accounts.iter_mut();
    let me = &mut next_keyed_account(keyed_accounts)?;

    match limited_deserialize(data)? {
        NonceInstruction::Nonce => me.nonce(
            &RecentBlockhashes::from_keyed_account(next_keyed_account(keyed_accounts)?)?,
            &signers,
        ),
        NonceInstruction::Withdraw(lamports) => {
            let to = &mut next_keyed_account(keyed_accounts)?;
            me.withdraw(
                lamports,
                to,
                &RecentBlockhashes::from_keyed_account(next_keyed_account(keyed_accounts)?)?,
                &Rent::from_keyed_account(next_keyed_account(keyed_accounts)?)?,
                &signers,
            )
        }
        NonceInstruction::Initialize => me.initialize(
            &RecentBlockhashes::from_keyed_account(next_keyed_account(keyed_accounts)?)?,
            &Rent::from_keyed_account(next_keyed_account(keyed_accounts)?)?,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        account::Account,
        hash::{hash, Hash},
        nonce_state, system_program, sysvar,
    };
    use bincode::serialize;

    fn process_instruction(instruction: &Instruction) -> Result<(), InstructionError> {
        let mut accounts: Vec<_> = instruction
            .accounts
            .iter()
            .map(|meta| {
                if sysvar::recent_blockhashes::check_id(&meta.pubkey) {
                    sysvar::recent_blockhashes::create_account_with_data(
                        1,
                        vec![(0u64, &Hash::default()); 32].into_iter(),
                    )
                } else if sysvar::rent::check_id(&meta.pubkey) {
                    sysvar::rent::create_account(1, &Rent::free())
                } else {
                    Account::default()
                }
            })
            .collect();

        {
            let mut keyed_accounts: Vec<_> = instruction
                .accounts
                .iter()
                .zip(accounts.iter_mut())
                .map(|(meta, account)| KeyedAccount::new(&meta.pubkey, meta.is_signer, account))
                .collect();
            super::process_instruction(&Pubkey::default(), &mut keyed_accounts, &instruction.data)
        }
    }

    #[test]
    fn test_create_account() {
        let from_pubkey = Pubkey::new_rand();
        let nonce_pubkey = Pubkey::new_rand();
        let ixs = create_nonce_account(&from_pubkey, &nonce_pubkey, 42);
        assert_eq!(ixs.len(), 2);
        let ix = &ixs[0];
        assert_eq!(ix.program_id, system_program::id());
        let pubkeys: Vec<_> = ix.accounts.iter().map(|am| am.pubkey).collect();
        assert!(pubkeys.contains(&from_pubkey));
        assert!(pubkeys.contains(&nonce_pubkey));
    }

    #[test]
    fn test_process_nonce_ix_no_acc_data_fail() {
        assert_eq!(
            process_instruction(&nonce(&Pubkey::default(),)),
            Err(InstructionError::InvalidAccountData),
        );
    }

    #[test]
    fn test_process_nonce_ix_no_keyed_accs_fail() {
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &mut [],
                &serialize(&NonceInstruction::Nonce).unwrap()
            ),
            Err(InstructionError::NotEnoughAccountKeys),
        );
    }

    #[test]
    fn test_process_nonce_ix_only_nonce_acc_fail() {
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &mut [KeyedAccount::new(
                    &Pubkey::default(),
                    true,
                    &mut Account::default(),
                ),],
                &serialize(&NonceInstruction::Nonce).unwrap(),
            ),
            Err(InstructionError::NotEnoughAccountKeys),
        );
    }

    #[test]
    fn test_process_nonce_ix_bad_recent_blockhash_state_fail() {
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &mut [
                    KeyedAccount::new(&Pubkey::default(), true, &mut Account::default(),),
                    KeyedAccount::new(
                        &sysvar::recent_blockhashes::id(),
                        false,
                        &mut Account::default(),
                    ),
                ],
                &serialize(&NonceInstruction::Nonce).unwrap(),
            ),
            Err(InstructionError::InvalidArgument),
        );
    }

    #[test]
    fn test_process_nonce_ix_ok() {
        let mut nonce_acc = nonce_state::create_account(1_000_000);
        super::process_instruction(
            &Pubkey::default(),
            &mut [
                KeyedAccount::new(&Pubkey::default(), true, &mut nonce_acc),
                KeyedAccount::new(
                    &sysvar::recent_blockhashes::id(),
                    false,
                    &mut sysvar::recent_blockhashes::create_account_with_data(
                        1,
                        vec![(0u64, &Hash::default()); 32].into_iter(),
                    ),
                ),
                KeyedAccount::new(
                    &sysvar::rent::id(),
                    false,
                    &mut sysvar::rent::create_account(1, &Rent::free()),
                ),
            ],
            &serialize(&NonceInstruction::Initialize).unwrap(),
        )
        .unwrap();
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &mut [
                    KeyedAccount::new(&Pubkey::default(), true, &mut nonce_acc,),
                    KeyedAccount::new(
                        &sysvar::recent_blockhashes::id(),
                        false,
                        &mut sysvar::recent_blockhashes::create_account_with_data(
                            1,
                            vec![(0u64, &hash(&serialize(&0).unwrap())); 32].into_iter(),
                        ),
                    ),
                ],
                &serialize(&NonceInstruction::Nonce).unwrap(),
            ),
            Ok(()),
        );
    }

    #[test]
    fn test_process_withdraw_ix_no_acc_data_fail() {
        assert_eq!(
            process_instruction(&withdraw(&Pubkey::default(), &Pubkey::default(), 1,)),
            Err(InstructionError::InvalidAccountData),
        );
    }

    #[test]
    fn test_process_withdraw_ix_no_keyed_accs_fail() {
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &mut [],
                &serialize(&NonceInstruction::Withdraw(42)).unwrap(),
            ),
            Err(InstructionError::NotEnoughAccountKeys),
        );
    }

    #[test]
    fn test_process_withdraw_ix_only_nonce_acc_fail() {
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &mut [KeyedAccount::new(
                    &Pubkey::default(),
                    true,
                    &mut Account::default(),
                ),],
                &serialize(&NonceInstruction::Withdraw(42)).unwrap(),
            ),
            Err(InstructionError::NotEnoughAccountKeys),
        );
    }

    #[test]
    fn test_process_withdraw_ix_bad_recent_blockhash_state_fail() {
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &mut [
                    KeyedAccount::new(&Pubkey::default(), true, &mut Account::default(),),
                    KeyedAccount::new(&Pubkey::default(), false, &mut Account::default(),),
                    KeyedAccount::new(
                        &sysvar::recent_blockhashes::id(),
                        false,
                        &mut Account::default(),
                    ),
                ],
                &serialize(&NonceInstruction::Withdraw(42)).unwrap(),
            ),
            Err(InstructionError::InvalidArgument),
        );
    }

    #[test]
    fn test_process_withdraw_ix_bad_rent_state_fail() {
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &mut [
                    KeyedAccount::new(
                        &Pubkey::default(),
                        true,
                        &mut nonce_state::create_account(1_000_000),
                    ),
                    KeyedAccount::new(&Pubkey::default(), true, &mut Account::default(),),
                    KeyedAccount::new(
                        &sysvar::recent_blockhashes::id(),
                        false,
                        &mut sysvar::recent_blockhashes::create_account_with_data(
                            1,
                            vec![(0u64, &Hash::default()); 32].into_iter(),
                        ),
                    ),
                    KeyedAccount::new(&sysvar::rent::id(), false, &mut Account::default(),),
                ],
                &serialize(&NonceInstruction::Withdraw(42)).unwrap(),
            ),
            Err(InstructionError::InvalidArgument),
        );
    }

    #[test]
    fn test_process_withdraw_ix_ok() {
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &mut [
                    KeyedAccount::new(
                        &Pubkey::default(),
                        true,
                        &mut nonce_state::create_account(1_000_000),
                    ),
                    KeyedAccount::new(&Pubkey::default(), true, &mut Account::default(),),
                    KeyedAccount::new(
                        &sysvar::recent_blockhashes::id(),
                        false,
                        &mut sysvar::recent_blockhashes::create_account_with_data(
                            1,
                            vec![(0u64, &Hash::default()); 32].into_iter(),
                        ),
                    ),
                    KeyedAccount::new(
                        &sysvar::rent::id(),
                        false,
                        &mut sysvar::rent::create_account(1, &Rent::free())
                    ),
                ],
                &serialize(&NonceInstruction::Withdraw(42)).unwrap(),
            ),
            Ok(()),
        );
    }

    #[test]
    fn test_process_initialize_ix_invalid_acc_data_fail() {
        assert_eq!(
            process_instruction(&initialize(&Pubkey::default())),
            Err(InstructionError::InvalidAccountData),
        );
    }

    #[test]
    fn test_process_initialize_ix_no_keyed_accs_fail() {
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &mut [],
                &serialize(&NonceInstruction::Initialize).unwrap(),
            ),
            Err(InstructionError::NotEnoughAccountKeys),
        );
    }

    #[test]
    fn test_process_initialize_ix_only_nonce_acc_fail() {
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &mut [KeyedAccount::new(
                    &Pubkey::default(),
                    true,
                    &mut nonce_state::create_account(1_000_000),
                ),],
                &serialize(&NonceInstruction::Initialize).unwrap(),
            ),
            Err(InstructionError::NotEnoughAccountKeys),
        );
    }

    #[test]
    fn test_process_initialize_bad_recent_blockhash_state_fail() {
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &mut [
                    KeyedAccount::new(
                        &Pubkey::default(),
                        true,
                        &mut nonce_state::create_account(1_000_000),
                    ),
                    KeyedAccount::new(
                        &sysvar::recent_blockhashes::id(),
                        false,
                        &mut Account::default(),
                    ),
                ],
                &serialize(&NonceInstruction::Initialize).unwrap(),
            ),
            Err(InstructionError::InvalidArgument),
        );
    }

    #[test]
    fn test_process_initialize_ix_bad_rent_state_fail() {
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &mut [
                    KeyedAccount::new(
                        &Pubkey::default(),
                        true,
                        &mut nonce_state::create_account(1_000_000),
                    ),
                    KeyedAccount::new(
                        &sysvar::recent_blockhashes::id(),
                        false,
                        &mut sysvar::recent_blockhashes::create_account_with_data(
                            1,
                            vec![(0u64, &Hash::default()); 32].into_iter(),
                        ),
                    ),
                    KeyedAccount::new(&sysvar::rent::id(), false, &mut Account::default(),),
                ],
                &serialize(&NonceInstruction::Initialize).unwrap(),
            ),
            Err(InstructionError::InvalidArgument),
        );
    }

    #[test]
    fn test_process_initialize_ix_ok() {
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &mut [
                    KeyedAccount::new(
                        &Pubkey::default(),
                        true,
                        &mut nonce_state::create_account(1_000_000),
                    ),
                    KeyedAccount::new(
                        &sysvar::recent_blockhashes::id(),
                        false,
                        &mut sysvar::recent_blockhashes::create_account_with_data(
                            1,
                            vec![(0u64, &Hash::default()); 32].into_iter(),
                        ),
                    ),
                    KeyedAccount::new(
                        &sysvar::rent::id(),
                        false,
                        &mut sysvar::rent::create_account(1, &Rent::free())
                    ),
                ],
                &serialize(&NonceInstruction::Initialize).unwrap(),
            ),
            Ok(()),
        );
    }

    #[test]
    fn test_custom_error_decode() {
        use num_traits::FromPrimitive;
        fn pretty_err<T>(err: InstructionError) -> String
        where
            T: 'static + std::error::Error + DecodeError<T> + FromPrimitive,
        {
            if let InstructionError::CustomError(code) = err {
                let specific_error: T = T::decode_custom_error_to_enum(code).unwrap();
                format!(
                    "{:?}: {}::{:?} - {}",
                    err,
                    T::type_of(),
                    specific_error,
                    specific_error,
                )
            } else {
                "".to_string()
            }
        }
        assert_eq!(
            "CustomError(0): NonceError::NoRecentBlockhashes - recent blockhash list is empty",
            pretty_err::<NonceError>(NonceError::NoRecentBlockhashes.into())
        );
        assert_eq!(
            "CustomError(1): NonceError::NotExpired - stored nonce is still in recent_blockhashes",
            pretty_err::<NonceError>(NonceError::NotExpired.into())
        );
        assert_eq!(
            "CustomError(2): NonceError::UnexpectedValue - specified nonce does not match stored nonce",
            pretty_err::<NonceError>(NonceError::UnexpectedValue.into())
        );
        assert_eq!(
            "CustomError(3): NonceError::BadAccountState - cannot handle request in current account state",
            pretty_err::<NonceError>(NonceError::BadAccountState.into())
        );
    }
}
