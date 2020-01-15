use log::*;

use solana_sdk::{
    account::{get_signers, Account, KeyedAccount},
    account_utils::State,
    instruction::InstructionError,
    instruction_processor_utils::{limited_deserialize, next_keyed_account},
    nonce_state::{NonceAccount, NonceState},
    pubkey::Pubkey,
    system_instruction::{
        create_address_with_seed, SystemError, SystemInstruction, MAX_PERMITTED_DATA_LENGTH,
    },
    system_program,
    sysvar::{self, recent_blockhashes::RecentBlockhashes, rent::Rent, Sysvar},
};

use std::collections::HashSet;

fn create_account_with_seed(
    from: &mut KeyedAccount,
    to: &mut KeyedAccount,
    base: &Pubkey,
    seed: &str,
    lamports: u64,
    data_length: u64,
    program_id: &Pubkey,
    signers: &HashSet<Pubkey>,
) -> Result<(), InstructionError> {
    // `base` is the source of the derived address and must sign for
    //   rights to the address
    if !signers.contains(&base) {
        debug!("CreateAccountWithSeed: must carry signature of `base`");
        return Err(InstructionError::MissingRequiredSignature);
    }

    // re-derive the address, must match the `to` account
    let address = create_address_with_seed(base, seed, program_id)?;

    if to.unsigned_key() != &address {
        debug!(
            "CreateAccountWithSeed: invalid argument; derived {} does not match `to` {}",
            address,
            to.unsigned_key()
        );
        return Err(SystemError::AddressWithSeedMismatch.into());
    }

    // all of finish_create_account's rules apply
    finish_create_account(from, to, lamports, data_length, program_id)
}

fn create_account(
    from: &mut KeyedAccount,
    to: &mut KeyedAccount,
    lamports: u64,
    data_length: u64,
    program_id: &Pubkey,
) -> Result<(), InstructionError> {
    if to.signer_key().is_none() {
        debug!("CreateAccount: `to` must sign");
        return Err(InstructionError::MissingRequiredSignature);
    }

    finish_create_account(from, to, lamports, data_length, program_id)
}

fn finish_create_account(
    from: &mut KeyedAccount,
    to: &mut KeyedAccount,
    lamports: u64,
    data_length: u64,
    program_id: &Pubkey,
) -> Result<(), InstructionError> {
    // if lamports == 0, the `from` account isn't touched
    if lamports != 0 {
        if from.signer_key().is_none() {
            debug!("CreateAccount: `from` must sign transfer");
            return Err(InstructionError::MissingRequiredSignature);
        }
        if !from.account.data.is_empty() {
            debug!("CreateAccount: `from` must not carry data");
            return Err(InstructionError::InvalidArgument);
        }
    }

    // if it looks like the `to` account is already in use, bail
    //   (note that the id check is also enforced by message_processor)
    if !to.account.data.is_empty() || !system_program::check_id(&to.account.owner) {
        debug!(
            "CreateAccount: invalid argument; account {} already in use",
            to.unsigned_key()
        );
        return Err(SystemError::AccountAlreadyInUse.into());
    }

    if sysvar::is_sysvar_id(&to.unsigned_key()) {
        debug!("CreateAccount: account id {} invalid", program_id);
        return Err(SystemError::InvalidAccountId.into());
    }

    if lamports > from.account.lamports {
        debug!(
            "CreateAccount: insufficient lamports ({}, need {})",
            from.account.lamports, lamports
        );
        return Err(SystemError::ResultWithNegativeLamports.into());
    }

    if data_length > MAX_PERMITTED_DATA_LENGTH {
        debug!(
            "CreateAccount: requested data_length: {} is more than maximum allowed",
            data_length
        );
        return Err(SystemError::InvalidAccountDataLength.into());
    }

    // guard against sysvars being made
    if sysvar::check_id(&program_id) {
        debug!("Assign: program id {} invalid", program_id);
        return Err(SystemError::InvalidProgramId.into());
    }

    to.account.owner = *program_id;

    from.account.lamports -= lamports;
    to.account.lamports += lamports;
    to.account.data = vec![0; data_length as usize];
    to.account.executable = false;

    Ok(())
}

fn assign_account_to_program(
    keyed_account: &mut KeyedAccount,
    program_id: &Pubkey,
) -> Result<(), InstructionError> {
    if keyed_account.signer_key().is_none() {
        debug!("Assign: account must sign");
        return Err(InstructionError::MissingRequiredSignature);
    }

    // guard against sysvars being made
    if sysvar::check_id(&program_id) {
        debug!("Assign: program id {} invalid", program_id);
        return Err(SystemError::InvalidProgramId.into());
    }

    keyed_account.account.owner = *program_id;
    Ok(())
}

fn transfer_lamports(
    from: &mut KeyedAccount,
    to: &mut KeyedAccount,
    lamports: u64,
) -> Result<(), InstructionError> {
    if lamports == 0 {
        return Ok(());
    }

    if from.signer_key().is_none() {
        debug!("Transfer: from must sign");
        return Err(InstructionError::MissingRequiredSignature);
    }

    if !from.account.data.is_empty() {
        debug!("Transfer: `from` must not carry data");
        return Err(InstructionError::InvalidArgument);
    }

    if lamports > from.account.lamports {
        debug!(
            "Transfer: insufficient lamports ({}, need {})",
            from.account.lamports, lamports
        );
        return Err(SystemError::ResultWithNegativeLamports.into());
    }

    from.account.lamports -= lamports;
    to.account.lamports += lamports;
    Ok(())
}

pub fn process_instruction(
    _program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    instruction_data: &[u8],
) -> Result<(), InstructionError> {
    let instruction = limited_deserialize(instruction_data)?;

    trace!("process_instruction: {:?}", instruction);
    trace!("keyed_accounts: {:?}", keyed_accounts);

    let signers = get_signers(keyed_accounts);
    let keyed_accounts_iter = &mut keyed_accounts.iter_mut();

    match instruction {
        SystemInstruction::CreateAccount {
            lamports,
            space,
            program_id,
        } => {
            let from = next_keyed_account(keyed_accounts_iter)?;
            let to = next_keyed_account(keyed_accounts_iter)?;
            create_account(from, to, lamports, space, &program_id)
        }
        SystemInstruction::CreateAccountWithSeed {
            base,
            seed,
            lamports,
            space,
            program_id,
        } => {
            let from = next_keyed_account(keyed_accounts_iter)?;
            let to = next_keyed_account(keyed_accounts_iter)?;

            create_account_with_seed(
                from,
                to,
                &base,
                &seed,
                lamports,
                space,
                &program_id,
                &signers,
            )
        }
        SystemInstruction::Assign { program_id } => {
            let account = next_keyed_account(keyed_accounts_iter)?;
            assign_account_to_program(account, &program_id)
        }
        SystemInstruction::Transfer { lamports } => {
            let from = next_keyed_account(keyed_accounts_iter)?;
            let to = next_keyed_account(keyed_accounts_iter)?;
            transfer_lamports(from, to, lamports)
        }
        SystemInstruction::AdvanceNonceAccount => {
            let me = &mut next_keyed_account(keyed_accounts_iter)?;
            me.nonce_advance(
                &RecentBlockhashes::from_keyed_account(next_keyed_account(keyed_accounts_iter)?)?,
                &signers,
            )
        }
        SystemInstruction::WithdrawNonceAccount(lamports) => {
            let me = &mut next_keyed_account(keyed_accounts_iter)?;
            let to = &mut next_keyed_account(keyed_accounts_iter)?;
            me.nonce_withdraw(
                lamports,
                to,
                &RecentBlockhashes::from_keyed_account(next_keyed_account(keyed_accounts_iter)?)?,
                &Rent::from_keyed_account(next_keyed_account(keyed_accounts_iter)?)?,
                &signers,
            )
        }
        SystemInstruction::InitializeNonceAccount(authorized) => {
            let me = &mut next_keyed_account(keyed_accounts_iter)?;
            me.nonce_initialize(
                &authorized,
                &RecentBlockhashes::from_keyed_account(next_keyed_account(keyed_accounts_iter)?)?,
                &Rent::from_keyed_account(next_keyed_account(keyed_accounts_iter)?)?,
            )
        }
        SystemInstruction::AuthorizeNonceAccount(nonce_authority) => {
            let me = &mut next_keyed_account(keyed_accounts_iter)?;
            me.nonce_authorize(&nonce_authority, &signers)
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SystemAccountKind {
    System,
    Nonce,
}

pub fn get_system_account_kind(account: &Account) -> Option<SystemAccountKind> {
    if system_program::check_id(&account.owner) {
        if account.data.is_empty() {
            Some(SystemAccountKind::System)
        } else if let Ok(NonceState::Initialized(_, _)) = account.state() {
            Some(SystemAccountKind::Nonce)
        } else {
            None
        }
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bank::Bank;
    use crate::bank_client::BankClient;
    use bincode::serialize;
    use solana_sdk::account::Account;
    use solana_sdk::client::SyncClient;
    use solana_sdk::genesis_config::create_genesis_config;
    use solana_sdk::hash::{hash, Hash};
    use solana_sdk::instruction::{AccountMeta, Instruction, InstructionError};
    use solana_sdk::nonce_state;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::system_instruction;
    use solana_sdk::system_program;
    use solana_sdk::sysvar;
    use solana_sdk::transaction::TransactionError;

    #[test]
    fn test_create_account() {
        let new_program_owner = Pubkey::new(&[9; 32]);
        let from = Pubkey::new_rand();
        let to = Pubkey::new_rand();
        let mut from_account = Account::new(100, 0, &system_program::id());
        let mut to_account = Account::new(0, 0, &Pubkey::default());

        assert_eq!(
            process_instruction(
                &Pubkey::default(),
                &mut [
                    KeyedAccount::new(&from, true, &mut from_account),
                    KeyedAccount::new(&to, true, &mut to_account)
                ],
                &bincode::serialize(&SystemInstruction::CreateAccount {
                    lamports: 50,
                    space: 2,
                    program_id: new_program_owner
                })
                .unwrap()
            ),
            Ok(())
        );
        assert_eq!(from_account.lamports, 50);
        assert_eq!(to_account.lamports, 50);
        assert_eq!(to_account.owner, new_program_owner);
        assert_eq!(to_account.data, [0, 0]);
    }

    #[test]
    fn test_create_account_with_seed() {
        let new_program_owner = Pubkey::new(&[9; 32]);
        let from = Pubkey::new_rand();
        let seed = "shiny pepper";
        let to = create_address_with_seed(&from, seed, &new_program_owner).unwrap();

        let mut from_account = Account::new(100, 0, &system_program::id());
        let mut to_account = Account::new(0, 0, &Pubkey::default());

        assert_eq!(
            process_instruction(
                &Pubkey::default(),
                &mut [
                    KeyedAccount::new(&from, true, &mut from_account),
                    KeyedAccount::new(&to, false, &mut to_account)
                ],
                &bincode::serialize(&SystemInstruction::CreateAccountWithSeed {
                    base: from,
                    seed: seed.to_string(),
                    lamports: 50,
                    space: 2,
                    program_id: new_program_owner
                })
                .unwrap()
            ),
            Ok(())
        );
        assert_eq!(from_account.lamports, 50);
        assert_eq!(to_account.lamports, 50);
        assert_eq!(to_account.owner, new_program_owner);
        assert_eq!(to_account.data, [0, 0]);
    }

    #[test]
    fn test_create_account_with_seed_mismatch() {
        let new_program_owner = Pubkey::new(&[9; 32]);
        let from = Pubkey::new_rand();
        let seed = "dull boy";
        let to = Pubkey::new_rand();

        let mut from_account = Account::new(100, 0, &system_program::id());
        let mut to_account = Account::new(0, 0, &Pubkey::default());

        assert_eq!(
            create_account_with_seed(
                &mut KeyedAccount::new(&from, true, &mut from_account),
                &mut KeyedAccount::new(&to, false, &mut to_account),
                &from,
                seed,
                50,
                2,
                &new_program_owner,
                &[from].iter().cloned().collect::<HashSet<_>>(),
            ),
            Err(SystemError::AddressWithSeedMismatch.into())
        );
        assert_eq!(from_account.lamports, 100);
        assert_eq!(to_account, Account::default());
    }
    #[test]
    fn test_create_account_with_seed_missing_sig() {
        let new_program_owner = Pubkey::new(&[9; 32]);
        let from = Pubkey::new_rand();
        let seed = "dull boy";
        let to = create_address_with_seed(&from, seed, &new_program_owner).unwrap();

        let mut from_account = Account::new(100, 0, &system_program::id());
        let mut to_account = Account::new(0, 0, &Pubkey::default());

        assert_eq!(
            create_account_with_seed(
                &mut KeyedAccount::new(&from, false, &mut from_account),
                &mut KeyedAccount::new(&to, false, &mut to_account),
                &from,
                seed,
                50,
                2,
                &new_program_owner,
                &[from].iter().cloned().collect::<HashSet<_>>(),
            ),
            Err(InstructionError::MissingRequiredSignature)
        );
        assert_eq!(from_account.lamports, 100);
        assert_eq!(to_account, Account::default());
    }

    #[test]
    fn test_create_with_zero_lamports() {
        // create account with zero lamports tranferred
        let new_program_owner = Pubkey::new(&[9; 32]);
        let from = Pubkey::new_rand();
        let mut from_account = Account::new(100, 1, &Pubkey::new_rand()); // not from system account

        let to = Pubkey::new_rand();
        let mut to_account = Account::new(0, 0, &Pubkey::default());

        assert_eq!(
            create_account(
                &mut KeyedAccount::new(&from, false, &mut from_account), // no signer
                &mut KeyedAccount::new(&to, true, &mut to_account),
                0,
                2,
                &new_program_owner,
            ),
            Ok(())
        );

        let from_lamports = from_account.lamports;
        let to_lamports = to_account.lamports;
        let to_owner = to_account.owner;
        let to_data = to_account.data.clone();
        assert_eq!(from_lamports, 100);
        assert_eq!(to_lamports, 0);
        assert_eq!(to_owner, new_program_owner);
        assert_eq!(to_data, [0, 0]);
    }

    #[test]
    fn test_create_negative_lamports() {
        // Attempt to create account with more lamports than remaining in from_account
        let new_program_owner = Pubkey::new(&[9; 32]);
        let from = Pubkey::new_rand();
        let mut from_account = Account::new(100, 0, &system_program::id());

        let to = Pubkey::new_rand();
        let mut to_account = Account::new(0, 0, &Pubkey::default());
        let unchanged_account = to_account.clone();

        let result = create_account(
            &mut KeyedAccount::new(&from, true, &mut from_account),
            &mut KeyedAccount::new(&to, true, &mut to_account),
            150,
            2,
            &new_program_owner,
        );
        assert_eq!(result, Err(SystemError::ResultWithNegativeLamports.into()));
        let from_lamports = from_account.lamports;
        assert_eq!(from_lamports, 100);
        assert_eq!(to_account, unchanged_account);
    }

    #[test]
    fn test_request_more_than_allowed_data_length() {
        let mut from_account = Account::new(100, 0, &system_program::id());
        let from_account_key = Pubkey::new_rand();
        let mut to_account = Account::default();
        let to_account_key = Pubkey::new_rand();

        // Trying to request more data length than permitted will result in failure
        let result = create_account(
            &mut KeyedAccount::new(&from_account_key, true, &mut from_account),
            &mut KeyedAccount::new(&to_account_key, true, &mut to_account),
            50,
            MAX_PERMITTED_DATA_LENGTH + 1,
            &system_program::id(),
        );
        assert!(result.is_err());
        assert_eq!(
            result.err().unwrap(),
            SystemError::InvalidAccountDataLength.into()
        );

        // Trying to request equal or less data length than permitted will be successful
        let result = create_account(
            &mut KeyedAccount::new(&from_account_key, true, &mut from_account),
            &mut KeyedAccount::new(&to_account_key, true, &mut to_account),
            50,
            MAX_PERMITTED_DATA_LENGTH,
            &system_program::id(),
        );
        assert!(result.is_ok());
        assert_eq!(to_account.lamports, 50);
        assert_eq!(to_account.data.len() as u64, MAX_PERMITTED_DATA_LENGTH);
    }

    #[test]
    fn test_create_already_in_use() {
        solana_logger::setup();
        // Attempt to create system account in account already owned by another program
        let new_program_owner = Pubkey::new(&[9; 32]);
        let from = Pubkey::new_rand();
        let mut from_account = Account::new(100, 0, &system_program::id());

        let original_program_owner = Pubkey::new(&[5; 32]);
        let owned_key = Pubkey::new_rand();
        let mut owned_account = Account::new(0, 0, &original_program_owner);
        let unchanged_account = owned_account.clone();

        let result = create_account(
            &mut KeyedAccount::new(&from, true, &mut from_account),
            &mut KeyedAccount::new(&owned_key, true, &mut owned_account),
            50,
            2,
            &new_program_owner,
        );
        assert_eq!(result, Err(SystemError::AccountAlreadyInUse.into()));

        let from_lamports = from_account.lamports;
        assert_eq!(from_lamports, 100);
        assert_eq!(owned_account, unchanged_account);

        // Attempt to create system account in account that already has data
        let mut owned_account = Account::new(0, 1, &Pubkey::default());
        let unchanged_account = owned_account.clone();
        let result = create_account(
            &mut KeyedAccount::new(&from, true, &mut from_account),
            &mut KeyedAccount::new(&owned_key, true, &mut owned_account),
            50,
            2,
            &new_program_owner,
        );
        assert_eq!(result, Err(SystemError::AccountAlreadyInUse.into()));
        let from_lamports = from_account.lamports;
        assert_eq!(from_lamports, 100);
        assert_eq!(owned_account, unchanged_account);

        // Verify that create_account works even if `to` has a non-zero balance
        let mut owned_account = Account::new(1, 0, &Pubkey::default());
        let result = create_account(
            &mut KeyedAccount::new(&from, true, &mut from_account),
            &mut KeyedAccount::new(&owned_key, true, &mut owned_account),
            50,
            2,
            &new_program_owner,
        );
        assert_eq!(result, Ok(()));
        assert_eq!(from_account.lamports, from_lamports - 50);
        assert_eq!(owned_account.lamports, 1 + 50);
    }

    #[test]
    fn test_create_unsigned() {
        // Attempt to create an account without signing the transfer
        let new_program_owner = Pubkey::new(&[9; 32]);
        let from = Pubkey::new_rand();
        let mut from_account = Account::new(100, 0, &system_program::id());

        let owned_key = Pubkey::new_rand();
        let mut owned_account = Account::new(0, 0, &Pubkey::default());
        let unchanged_account = owned_account.clone();

        // Haven't signed from account
        let result = create_account(
            &mut KeyedAccount::new(&from, false, &mut from_account),
            &mut KeyedAccount::new(&owned_key, true, &mut owned_account),
            50,
            2,
            &new_program_owner,
        );
        assert_eq!(result, Err(InstructionError::MissingRequiredSignature));
        assert_eq!(from_account.lamports, 100);
        assert_eq!(owned_account, unchanged_account);

        // Haven't signed to account
        let result = create_account(
            &mut KeyedAccount::new(&from, true, &mut from_account),
            &mut KeyedAccount::new(&owned_key, false, &mut owned_account),
            50,
            2,
            &new_program_owner,
        );
        assert_eq!(result, Err(InstructionError::MissingRequiredSignature));
        assert_eq!(from_account.lamports, 100);
        assert_eq!(owned_account, unchanged_account);

        // support creation/assignment with zero lamports (ephemeral account)
        let result = create_account(
            &mut KeyedAccount::new(&from, false, &mut from_account),
            &mut KeyedAccount::new(&owned_key, true, &mut owned_account),
            0,
            2,
            &new_program_owner,
        );
        assert_eq!(result, Ok(()));
        assert_eq!(from_account.lamports, 100);
        assert_eq!(owned_account.owner, new_program_owner);
    }

    #[test]
    fn test_create_sysvar_invalid_id() {
        // Attempt to create system account in account already owned by another program
        let from = Pubkey::new_rand();
        let mut from_account = Account::new(100, 0, &system_program::id());

        let to = Pubkey::new_rand();
        let mut to_account = Account::default();

        // fail to create a sysvar::id() owned account
        let result = create_account(
            &mut KeyedAccount::new(&from, true, &mut from_account),
            &mut KeyedAccount::new(&to, true, &mut to_account),
            50,
            2,
            &sysvar::id(),
        );
        assert_eq!(result, Err(SystemError::InvalidProgramId.into()));

        let to = sysvar::fees::id();
        let mut to_account = Account::default();

        // fail to create an account with a sysvar id
        let result = create_account(
            &mut KeyedAccount::new(&from, true, &mut from_account),
            &mut KeyedAccount::new(&to, true, &mut to_account),
            50,
            2,
            &system_program::id(),
        );
        assert_eq!(result, Err(SystemError::InvalidAccountId.into()));

        let from_lamports = from_account.lamports;
        assert_eq!(from_lamports, 100);
    }

    #[test]
    fn test_create_data_populated() {
        // Attempt to create system account in account with populated data
        let new_program_owner = Pubkey::new(&[9; 32]);
        let from = Pubkey::new_rand();
        let mut from_account = Account::new(100, 0, &system_program::id());

        let populated_key = Pubkey::new_rand();
        let mut populated_account = Account {
            data: vec![0, 1, 2, 3],
            ..Account::default()
        };
        let unchanged_account = populated_account.clone();

        let result = create_account(
            &mut KeyedAccount::new(&from, true, &mut from_account),
            &mut KeyedAccount::new(&populated_key, true, &mut populated_account),
            50,
            2,
            &new_program_owner,
        );
        assert_eq!(result, Err(SystemError::AccountAlreadyInUse.into()));
        assert_eq!(from_account.lamports, 100);
        assert_eq!(populated_account, unchanged_account);
    }

    #[test]
    fn test_create_from_account_is_nonce_fail() {
        let nonce = Pubkey::new_rand();
        let mut nonce_account = Account::new_data(
            42,
            &nonce_state::NonceState::Initialized(
                nonce_state::Meta::new(&Pubkey::default()),
                Hash::default(),
            ),
            &system_program::id(),
        )
        .unwrap();
        let mut from = KeyedAccount::new(&nonce, true, &mut nonce_account);
        let new = Pubkey::new_rand();

        let mut new_account = Account::default();
        let mut to = KeyedAccount::new(&new, true, &mut new_account);
        assert_eq!(
            create_account(&mut from, &mut to, 42, 0, &Pubkey::new_rand()),
            Err(InstructionError::InvalidArgument),
        );
    }

    #[test]
    fn test_assign_account_to_program() {
        let new_program_owner = Pubkey::new(&[9; 32]);

        let from = Pubkey::new_rand();
        let mut from_account = Account::new(100, 0, &system_program::id());

        assert_eq!(
            assign_account_to_program(
                &mut KeyedAccount::new(&from, false, &mut from_account),
                &new_program_owner,
            ),
            Err(InstructionError::MissingRequiredSignature)
        );

        assert_eq!(
            process_instruction(
                &Pubkey::default(),
                &mut [KeyedAccount::new(&from, true, &mut from_account)],
                &bincode::serialize(&SystemInstruction::Assign {
                    program_id: new_program_owner
                })
                .unwrap()
            ),
            Ok(())
        );
    }

    #[test]
    fn test_assign_account_to_sysvar() {
        let new_program_owner = sysvar::id();

        let from = Pubkey::new_rand();
        let mut from_account = Account::new(100, 0, &system_program::id());

        assert_eq!(
            assign_account_to_program(
                &mut KeyedAccount::new(&from, true, &mut from_account),
                &new_program_owner,
            ),
            Err(SystemError::InvalidProgramId.into())
        );
    }

    #[test]
    fn test_process_bogus_instruction() {
        // Attempt to assign with no accounts
        let instruction = SystemInstruction::Assign {
            program_id: Pubkey::new_rand(),
        };
        let data = serialize(&instruction).unwrap();
        let result = process_instruction(&system_program::id(), &mut [], &data);
        assert_eq!(result, Err(InstructionError::NotEnoughAccountKeys));

        let from = Pubkey::new_rand();
        let mut from_account = Account::new(100, 0, &system_program::id());
        // Attempt to transfer with no destination
        let instruction = SystemInstruction::Transfer { lamports: 0 };
        let data = serialize(&instruction).unwrap();
        let result = process_instruction(
            &system_program::id(),
            &mut [KeyedAccount::new(&from, true, &mut from_account)],
            &data,
        );
        assert_eq!(result, Err(InstructionError::NotEnoughAccountKeys));
    }

    #[test]
    fn test_transfer_lamports() {
        let from = Pubkey::new_rand();
        let mut from_account = Account::new(100, 0, &Pubkey::new(&[2; 32])); // account owner should not matter
        let to = Pubkey::new_rand();
        let mut to_account = Account::new(1, 0, &Pubkey::new(&[3; 32])); // account owner should not matter
        transfer_lamports(
            &mut KeyedAccount::new(&from, true, &mut from_account),
            &mut KeyedAccount::new(&to, false, &mut to_account),
            50,
        )
        .unwrap();
        let from_lamports = from_account.lamports;
        let to_lamports = to_account.lamports;
        assert_eq!(from_lamports, 50);
        assert_eq!(to_lamports, 51);

        // Attempt to move more lamports than remaining in from_account
        let result = transfer_lamports(
            &mut KeyedAccount::new(&from, true, &mut from_account),
            &mut KeyedAccount::new(&to, false, &mut to_account),
            100,
        );
        assert_eq!(result, Err(SystemError::ResultWithNegativeLamports.into()));
        assert_eq!(from_account.lamports, 50);
        assert_eq!(to_account.lamports, 51);

        // test unsigned transfer of zero
        assert!(transfer_lamports(
            &mut KeyedAccount::new(&from, false, &mut from_account),
            &mut KeyedAccount::new(&to, false, &mut to_account),
            0,
        )
        .is_ok(),);
        assert_eq!(from_account.lamports, 50);
        assert_eq!(to_account.lamports, 51);
    }

    #[test]
    fn test_transfer_lamports_from_nonce_account_fail() {
        let from = Pubkey::new_rand();
        let mut from_account = Account::new_data(
            100,
            &nonce_state::NonceState::Initialized(nonce_state::Meta::new(&from), Hash::default()),
            &system_program::id(),
        )
        .unwrap();
        assert_eq!(
            get_system_account_kind(&from_account),
            Some(SystemAccountKind::Nonce)
        );
        let to = Pubkey::new_rand();
        let mut to_account = Account::new(1, 0, &Pubkey::new(&[3; 32])); // account owner should not matter
        assert_eq!(
            transfer_lamports(
                &mut KeyedAccount::new(&from, true, &mut from_account),
                &mut KeyedAccount::new(&to, false, &mut to_account),
                50,
            ),
            Err(InstructionError::InvalidArgument),
        )
    }

    #[test]
    fn test_system_unsigned_transaction() {
        let (genesis_config, alice_keypair) = create_genesis_config(100);
        let alice_pubkey = alice_keypair.pubkey();
        let mallory_keypair = Keypair::new();
        let mallory_pubkey = mallory_keypair.pubkey();

        // Fund to account to bypass AccountNotFound error
        let bank = Bank::new(&genesis_config);
        let bank_client = BankClient::new(bank);
        bank_client
            .transfer(50, &alice_keypair, &mallory_pubkey)
            .unwrap();

        // Erroneously sign transaction with recipient account key
        // No signature case is tested by bank `test_zero_signatures()`
        let account_metas = vec![
            AccountMeta::new(alice_pubkey, false),
            AccountMeta::new(mallory_pubkey, true),
        ];
        let malicious_instruction = Instruction::new(
            system_program::id(),
            &SystemInstruction::Transfer { lamports: 10 },
            account_metas,
        );
        assert_eq!(
            bank_client
                .send_instruction(&mallory_keypair, malicious_instruction)
                .unwrap_err()
                .unwrap(),
            TransactionError::InstructionError(0, InstructionError::MissingRequiredSignature)
        );
        assert_eq!(bank_client.get_balance(&alice_pubkey).unwrap(), 50);
        assert_eq!(bank_client.get_balance(&mallory_pubkey).unwrap(), 50);
    }

    fn process_nonce_instruction(instruction: &Instruction) -> Result<(), InstructionError> {
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
            let mut keyed_accounts_iter: Vec<_> = instruction
                .accounts
                .iter()
                .zip(accounts.iter_mut())
                .map(|(meta, account)| KeyedAccount::new(&meta.pubkey, meta.is_signer, account))
                .collect();
            super::process_instruction(
                &Pubkey::default(),
                &mut keyed_accounts_iter,
                &instruction.data,
            )
        }
    }

    #[test]
    fn test_process_nonce_ix_no_acc_data_fail() {
        assert_eq!(
            process_nonce_instruction(&system_instruction::nonce_advance(
                &Pubkey::default(),
                &Pubkey::default()
            )),
            Err(InstructionError::InvalidAccountData),
        );
    }

    #[test]
    fn test_process_nonce_ix_no_keyed_accs_fail() {
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &mut [],
                &serialize(&SystemInstruction::AdvanceNonceAccount).unwrap()
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
                &serialize(&SystemInstruction::AdvanceNonceAccount).unwrap(),
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
                &serialize(&SystemInstruction::AdvanceNonceAccount).unwrap(),
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
            &serialize(&SystemInstruction::InitializeNonceAccount(Pubkey::default())).unwrap(),
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
                &serialize(&SystemInstruction::AdvanceNonceAccount).unwrap(),
            ),
            Ok(()),
        );
    }

    #[test]
    fn test_process_withdraw_ix_no_acc_data_fail() {
        assert_eq!(
            process_nonce_instruction(&system_instruction::nonce_withdraw(
                &Pubkey::default(),
                &Pubkey::default(),
                &Pubkey::default(),
                1,
            )),
            Err(InstructionError::InvalidAccountData),
        );
    }

    #[test]
    fn test_process_withdraw_ix_no_keyed_accs_fail() {
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &mut [],
                &serialize(&SystemInstruction::WithdrawNonceAccount(42)).unwrap(),
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
                &serialize(&SystemInstruction::WithdrawNonceAccount(42)).unwrap(),
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
                &serialize(&SystemInstruction::WithdrawNonceAccount(42)).unwrap(),
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
                &serialize(&SystemInstruction::WithdrawNonceAccount(42)).unwrap(),
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
                &serialize(&SystemInstruction::WithdrawNonceAccount(42)).unwrap(),
            ),
            Ok(()),
        );
    }

    #[test]
    fn test_process_initialize_ix_no_keyed_accs_fail() {
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &mut [],
                &serialize(&SystemInstruction::InitializeNonceAccount(Pubkey::default())).unwrap(),
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
                &serialize(&SystemInstruction::InitializeNonceAccount(Pubkey::default())).unwrap(),
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
                &serialize(&SystemInstruction::InitializeNonceAccount(Pubkey::default())).unwrap(),
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
                &serialize(&SystemInstruction::InitializeNonceAccount(Pubkey::default())).unwrap(),
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
                &serialize(&SystemInstruction::InitializeNonceAccount(Pubkey::default())).unwrap(),
            ),
            Ok(()),
        );
    }

    #[test]
    fn test_process_authorize_ix_ok() {
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
            &serialize(&SystemInstruction::InitializeNonceAccount(Pubkey::default())).unwrap(),
        )
        .unwrap();
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &mut [KeyedAccount::new(&Pubkey::default(), true, &mut nonce_acc,),],
                &serialize(&SystemInstruction::AuthorizeNonceAccount(Pubkey::default(),)).unwrap(),
            ),
            Ok(()),
        );
    }

    #[test]
    fn test_process_authorize_bad_account_data_fail() {
        assert_eq!(
            process_nonce_instruction(&system_instruction::nonce_authorize(
                &Pubkey::default(),
                &Pubkey::default(),
                &Pubkey::default(),
            )),
            Err(InstructionError::InvalidAccountData),
        );
    }

    #[test]
    fn test_get_system_account_kind_system_ok() {
        let system_account = Account::default();
        assert_eq!(
            get_system_account_kind(&system_account),
            Some(SystemAccountKind::System)
        );
    }

    #[test]
    fn test_get_system_account_kind_nonce_ok() {
        let nonce_account = Account::new_data(
            42,
            &nonce_state::NonceState::Initialized(
                nonce_state::Meta::new(&Pubkey::default()),
                Hash::default(),
            ),
            &system_program::id(),
        )
        .unwrap();
        assert_eq!(
            get_system_account_kind(&nonce_account),
            Some(SystemAccountKind::Nonce)
        );
    }

    #[test]
    fn test_get_system_account_kind_uninitialized_nonce_account_fail() {
        assert_eq!(
            get_system_account_kind(&nonce_state::create_account(42)),
            None
        );
    }

    #[test]
    fn test_get_system_account_kind_system_owner_nonzero_nonnonce_data_fail() {
        let other_data_account = Account::new_data(42, b"other", &Pubkey::default()).unwrap();
        assert_eq!(get_system_account_kind(&other_data_account), None);
    }

    #[test]
    fn test_get_system_account_kind_nonsystem_owner_with_nonce_state_data_fail() {
        let nonce_account = Account::new_data(
            42,
            &nonce_state::NonceState::Initialized(
                nonce_state::Meta::new(&Pubkey::default()),
                Hash::default(),
            ),
            &Pubkey::new_rand(),
        )
        .unwrap();
        assert_eq!(get_system_account_kind(&nonce_account), None);
    }
}
