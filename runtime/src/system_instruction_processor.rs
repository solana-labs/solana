use {
    crate::nonce_keyed_account::NonceKeyedAccount,
    log::*,
    solana_program_runtime::{
        ic_msg, invoke_context::InvokeContext, sysvar_cache::get_sysvar_with_account_check,
    },
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount, WritableAccount},
        account_utils::StateMut,
        feature_set,
        instruction::InstructionError,
        keyed_account::{get_signers, keyed_account_at_index, KeyedAccount},
        nonce,
        program_utils::limited_deserialize,
        pubkey::Pubkey,
        system_instruction::{
            NonceError, SystemError, SystemInstruction, MAX_PERMITTED_DATA_LENGTH,
        },
        system_program,
    },
    std::collections::HashSet,
};

// represents an address that may or may not have been generated
//  from a seed
#[derive(PartialEq, Default, Debug)]
struct Address {
    address: Pubkey,
    base: Option<Pubkey>,
}

impl Address {
    fn is_signer(&self, signers: &HashSet<Pubkey>) -> bool {
        if let Some(base) = self.base {
            signers.contains(&base)
        } else {
            signers.contains(&self.address)
        }
    }
    fn create(
        address: &Pubkey,
        with_seed: Option<(&Pubkey, &str, &Pubkey)>,
        invoke_context: &InvokeContext,
    ) -> Result<Self, InstructionError> {
        let base = if let Some((base, seed, owner)) = with_seed {
            let address_with_seed = Pubkey::create_with_seed(base, seed, owner)?;
            // re-derive the address, must match the supplied address
            if *address != address_with_seed {
                ic_msg!(
                    invoke_context,
                    "Create: address {} does not match derived address {}",
                    address,
                    address_with_seed
                );
                return Err(SystemError::AddressWithSeedMismatch.into());
            }
            Some(*base)
        } else {
            None
        };

        Ok(Self {
            address: *address,
            base,
        })
    }
}

fn allocate(
    account: &mut AccountSharedData,
    address: &Address,
    space: u64,
    signers: &HashSet<Pubkey>,
    invoke_context: &InvokeContext,
) -> Result<(), InstructionError> {
    if !address.is_signer(signers) {
        ic_msg!(
            invoke_context,
            "Allocate: 'to' account {:?} must sign",
            address
        );
        return Err(InstructionError::MissingRequiredSignature);
    }

    // if it looks like the `to` account is already in use, bail
    //   (note that the id check is also enforced by message_processor)
    if !account.data().is_empty() || !system_program::check_id(account.owner()) {
        ic_msg!(
            invoke_context,
            "Allocate: account {:?} already in use",
            address
        );
        return Err(SystemError::AccountAlreadyInUse.into());
    }

    if space > MAX_PERMITTED_DATA_LENGTH {
        ic_msg!(
            invoke_context,
            "Allocate: requested {}, max allowed {}",
            space,
            MAX_PERMITTED_DATA_LENGTH
        );
        return Err(SystemError::InvalidAccountDataLength.into());
    }

    account.set_data(vec![0; space as usize]);

    Ok(())
}

fn assign(
    account: &mut AccountSharedData,
    address: &Address,
    owner: &Pubkey,
    signers: &HashSet<Pubkey>,
    invoke_context: &InvokeContext,
) -> Result<(), InstructionError> {
    // no work to do, just return
    if account.owner() == owner {
        return Ok(());
    }

    if !address.is_signer(signers) {
        ic_msg!(invoke_context, "Assign: account {:?} must sign", address);
        return Err(InstructionError::MissingRequiredSignature);
    }

    account.set_owner(*owner);
    Ok(())
}

fn allocate_and_assign(
    to: &mut AccountSharedData,
    to_address: &Address,
    space: u64,
    owner: &Pubkey,
    signers: &HashSet<Pubkey>,
    invoke_context: &InvokeContext,
) -> Result<(), InstructionError> {
    allocate(to, to_address, space, signers, invoke_context)?;
    assign(to, to_address, owner, signers, invoke_context)
}

fn create_account(
    from: &KeyedAccount,
    to: &KeyedAccount,
    to_address: &Address,
    lamports: u64,
    space: u64,
    owner: &Pubkey,
    signers: &HashSet<Pubkey>,
    invoke_context: &InvokeContext,
) -> Result<(), InstructionError> {
    // if it looks like the `to` account is already in use, bail
    {
        let to = &mut to.try_account_ref_mut()?;
        if to.lamports() > 0 {
            ic_msg!(
                invoke_context,
                "Create Account: account {:?} already in use",
                to_address
            );
            return Err(SystemError::AccountAlreadyInUse.into());
        }

        allocate_and_assign(to, to_address, space, owner, signers, invoke_context)?;
    }
    transfer(from, to, lamports, invoke_context)
}

fn transfer_verified(
    from: &KeyedAccount,
    to: &KeyedAccount,
    lamports: u64,
    invoke_context: &InvokeContext,
) -> Result<(), InstructionError> {
    if !from.data_is_empty()? {
        ic_msg!(invoke_context, "Transfer: `from` must not carry data");
        return Err(InstructionError::InvalidArgument);
    }
    if lamports > from.lamports()? {
        ic_msg!(
            invoke_context,
            "Transfer: insufficient lamports {}, need {}",
            from.lamports()?,
            lamports
        );
        return Err(SystemError::ResultWithNegativeLamports.into());
    }

    from.try_account_ref_mut()?.checked_sub_lamports(lamports)?;
    to.try_account_ref_mut()?.checked_add_lamports(lamports)?;
    Ok(())
}

fn transfer(
    from: &KeyedAccount,
    to: &KeyedAccount,
    lamports: u64,
    invoke_context: &InvokeContext,
) -> Result<(), InstructionError> {
    if !invoke_context
        .feature_set
        .is_active(&feature_set::system_transfer_zero_check::id())
        && lamports == 0
    {
        return Ok(());
    }

    if from.signer_key().is_none() {
        ic_msg!(
            invoke_context,
            "Transfer: `from` account {} must sign",
            from.unsigned_key()
        );
        return Err(InstructionError::MissingRequiredSignature);
    }

    transfer_verified(from, to, lamports, invoke_context)
}

fn transfer_with_seed(
    from: &KeyedAccount,
    from_base: &KeyedAccount,
    from_seed: &str,
    from_owner: &Pubkey,
    to: &KeyedAccount,
    lamports: u64,
    invoke_context: &InvokeContext,
) -> Result<(), InstructionError> {
    if !invoke_context
        .feature_set
        .is_active(&feature_set::system_transfer_zero_check::id())
        && lamports == 0
    {
        return Ok(());
    }

    if from_base.signer_key().is_none() {
        ic_msg!(
            invoke_context,
            "Transfer: 'from' account {:?} must sign",
            from_base
        );
        return Err(InstructionError::MissingRequiredSignature);
    }

    let address_from_seed =
        Pubkey::create_with_seed(from_base.unsigned_key(), from_seed, from_owner)?;
    if *from.unsigned_key() != address_from_seed {
        ic_msg!(
            invoke_context,
            "Transfer: 'from' address {} does not match derived address {}",
            from.unsigned_key(),
            address_from_seed
        );
        return Err(SystemError::AddressWithSeedMismatch.into());
    }

    transfer_verified(from, to, lamports, invoke_context)
}

pub fn process_instruction(
    first_instruction_account: usize,
    instruction_data: &[u8],
    invoke_context: &mut InvokeContext,
) -> Result<(), InstructionError> {
    let keyed_accounts = invoke_context.get_keyed_accounts()?;
    let instruction = limited_deserialize(instruction_data)?;

    trace!("process_instruction: {:?}", instruction);
    trace!("keyed_accounts: {:?}", keyed_accounts);

    let _ = keyed_account_at_index(keyed_accounts, first_instruction_account)?;
    let signers = get_signers(&keyed_accounts[first_instruction_account..]);
    match instruction {
        SystemInstruction::CreateAccount {
            lamports,
            space,
            owner,
        } => {
            let from = keyed_account_at_index(keyed_accounts, first_instruction_account)?;
            let to = keyed_account_at_index(keyed_accounts, first_instruction_account + 1)?;
            let to_address = Address::create(to.unsigned_key(), None, invoke_context)?;
            create_account(
                from,
                to,
                &to_address,
                lamports,
                space,
                &owner,
                &signers,
                invoke_context,
            )
        }
        SystemInstruction::CreateAccountWithSeed {
            base,
            seed,
            lamports,
            space,
            owner,
        } => {
            let from = keyed_account_at_index(keyed_accounts, first_instruction_account)?;
            let to = keyed_account_at_index(keyed_accounts, first_instruction_account + 1)?;
            let to_address = Address::create(
                to.unsigned_key(),
                Some((&base, &seed, &owner)),
                invoke_context,
            )?;
            create_account(
                from,
                to,
                &to_address,
                lamports,
                space,
                &owner,
                &signers,
                invoke_context,
            )
        }
        SystemInstruction::Assign { owner } => {
            let keyed_account = keyed_account_at_index(keyed_accounts, first_instruction_account)?;
            let mut account = keyed_account.try_account_ref_mut()?;
            let address = Address::create(keyed_account.unsigned_key(), None, invoke_context)?;
            assign(&mut account, &address, &owner, &signers, invoke_context)
        }
        SystemInstruction::Transfer { lamports } => {
            let from = keyed_account_at_index(keyed_accounts, first_instruction_account)?;
            let to = keyed_account_at_index(keyed_accounts, first_instruction_account + 1)?;
            transfer(from, to, lamports, invoke_context)
        }
        SystemInstruction::TransferWithSeed {
            lamports,
            from_seed,
            from_owner,
        } => {
            let from = keyed_account_at_index(keyed_accounts, first_instruction_account)?;
            let base = keyed_account_at_index(keyed_accounts, first_instruction_account + 1)?;
            let to = keyed_account_at_index(keyed_accounts, first_instruction_account + 2)?;
            transfer_with_seed(
                from,
                base,
                &from_seed,
                &from_owner,
                to,
                lamports,
                invoke_context,
            )
        }
        SystemInstruction::AdvanceNonceAccount => {
            let me = &mut keyed_account_at_index(keyed_accounts, first_instruction_account)?;
            #[allow(deprecated)]
            let recent_blockhashes = get_sysvar_with_account_check::recent_blockhashes(
                keyed_account_at_index(keyed_accounts, first_instruction_account + 1)?,
                invoke_context,
            )?;
            if recent_blockhashes.is_empty() {
                ic_msg!(
                    invoke_context,
                    "Advance nonce account: recent blockhash list is empty",
                );
                return Err(NonceError::NoRecentBlockhashes.into());
            }
            me.advance_nonce_account(&signers, invoke_context)
        }
        SystemInstruction::WithdrawNonceAccount(lamports) => {
            let me = &mut keyed_account_at_index(keyed_accounts, first_instruction_account)?;
            let to = &mut keyed_account_at_index(keyed_accounts, first_instruction_account + 1)?;
            #[allow(deprecated)]
            let _recent_blockhashes = get_sysvar_with_account_check::recent_blockhashes(
                keyed_account_at_index(keyed_accounts, first_instruction_account + 2)?,
                invoke_context,
            )?;
            let rent = get_sysvar_with_account_check::rent(
                keyed_account_at_index(keyed_accounts, first_instruction_account + 3)?,
                invoke_context,
            )?;
            me.withdraw_nonce_account(lamports, to, &rent, &signers, invoke_context)
        }
        SystemInstruction::InitializeNonceAccount(authorized) => {
            let me = &mut keyed_account_at_index(keyed_accounts, first_instruction_account)?;
            #[allow(deprecated)]
            let recent_blockhashes = get_sysvar_with_account_check::recent_blockhashes(
                keyed_account_at_index(keyed_accounts, first_instruction_account + 1)?,
                invoke_context,
            )?;
            if recent_blockhashes.is_empty() {
                ic_msg!(
                    invoke_context,
                    "Initialize nonce account: recent blockhash list is empty",
                );
                return Err(NonceError::NoRecentBlockhashes.into());
            }
            let rent = get_sysvar_with_account_check::rent(
                keyed_account_at_index(keyed_accounts, first_instruction_account + 2)?,
                invoke_context,
            )?;
            me.initialize_nonce_account(&authorized, &rent, invoke_context)
        }
        SystemInstruction::AuthorizeNonceAccount(nonce_authority) => {
            let me = &mut keyed_account_at_index(keyed_accounts, first_instruction_account)?;
            me.authorize_nonce_account(&nonce_authority, &signers, invoke_context)
        }
        SystemInstruction::Allocate { space } => {
            let keyed_account = keyed_account_at_index(keyed_accounts, first_instruction_account)?;
            let mut account = keyed_account.try_account_ref_mut()?;
            let address = Address::create(keyed_account.unsigned_key(), None, invoke_context)?;
            allocate(&mut account, &address, space, &signers, invoke_context)
        }
        SystemInstruction::AllocateWithSeed {
            base,
            seed,
            space,
            owner,
        } => {
            let keyed_account = keyed_account_at_index(keyed_accounts, first_instruction_account)?;
            let mut account = keyed_account.try_account_ref_mut()?;
            let address = Address::create(
                keyed_account.unsigned_key(),
                Some((&base, &seed, &owner)),
                invoke_context,
            )?;
            allocate_and_assign(
                &mut account,
                &address,
                space,
                &owner,
                &signers,
                invoke_context,
            )
        }
        SystemInstruction::AssignWithSeed { base, seed, owner } => {
            let keyed_account = keyed_account_at_index(keyed_accounts, first_instruction_account)?;
            let mut account = keyed_account.try_account_ref_mut()?;
            let address = Address::create(
                keyed_account.unsigned_key(),
                Some((&base, &seed, &owner)),
                invoke_context,
            )?;
            assign(&mut account, &address, &owner, &signers, invoke_context)
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SystemAccountKind {
    System,
    Nonce,
}

pub fn get_system_account_kind(account: &AccountSharedData) -> Option<SystemAccountKind> {
    if system_program::check_id(account.owner()) {
        if account.data().is_empty() {
            Some(SystemAccountKind::System)
        } else if account.data().len() == nonce::State::size() {
            match account.state().ok()? {
                nonce::state::Versions::Current(state) => match *state {
                    nonce::State::Initialized(_) => Some(SystemAccountKind::Nonce),
                    _ => None,
                },
            }
        } else {
            None
        }
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    #[allow(deprecated)]
    use solana_sdk::{
        account::{self, Account, AccountSharedData},
        client::SyncClient,
        genesis_config::create_genesis_config,
        hash::{hash, Hash},
        instruction::{AccountMeta, Instruction, InstructionError},
        message::Message,
        nonce, nonce_account, recent_blockhashes_account,
        signature::{Keypair, Signer},
        system_instruction, system_program,
        sysvar::{self, recent_blockhashes::IterItem, rent::Rent},
        transaction::TransactionError,
        transaction_context::TransactionContext,
    };
    use {
        super::*,
        crate::{bank::Bank, bank_client::BankClient},
        bincode::serialize,
        solana_program_runtime::invoke_context::{
            mock_process_instruction, InvokeContext, ProcessInstructionWithContext,
        },
        std::sync::Arc,
    };

    impl From<Pubkey> for Address {
        fn from(address: Pubkey) -> Self {
            Self {
                address,
                base: None,
            }
        }
    }

    fn process_instruction(
        instruction_data: &[u8],
        transaction_accounts: Vec<(Pubkey, AccountSharedData)>,
        instruction_accounts: Vec<AccountMeta>,
        expected_result: Result<(), InstructionError>,
        process_instruction: ProcessInstructionWithContext,
    ) -> Vec<AccountSharedData> {
        mock_process_instruction(
            &system_program::id(),
            Vec::new(),
            instruction_data,
            transaction_accounts,
            instruction_accounts,
            expected_result,
            process_instruction,
        )
    }

    fn create_default_account() -> AccountSharedData {
        AccountSharedData::new(0, 0, &Pubkey::new_unique())
    }
    fn create_default_recent_blockhashes_account() -> AccountSharedData {
        #[allow(deprecated)]
        recent_blockhashes_account::create_account_with_data_for_test(
            vec![IterItem(0u64, &Hash::default(), 0); sysvar::recent_blockhashes::MAX_ENTRIES]
                .into_iter(),
        )
    }
    fn create_default_rent_account() -> AccountSharedData {
        account::create_account_shared_data_for_test(&Rent::free())
    }

    #[test]
    fn test_create_account() {
        let new_owner = Pubkey::new(&[9; 32]);
        let from = Pubkey::new_unique();
        let to = Pubkey::new_unique();
        let from_account = AccountSharedData::new(100, 0, &system_program::id());
        let to_account = AccountSharedData::new(0, 0, &Pubkey::default());

        let accounts = process_instruction(
            &bincode::serialize(&SystemInstruction::CreateAccount {
                lamports: 50,
                space: 2,
                owner: new_owner,
            })
            .unwrap(),
            vec![(from, from_account), (to, to_account)],
            vec![
                AccountMeta {
                    pubkey: from,
                    is_signer: true,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: to,
                    is_signer: true,
                    is_writable: false,
                },
            ],
            Ok(()),
            super::process_instruction,
        );
        assert_eq!(accounts[0].lamports(), 50);
        assert_eq!(accounts[1].lamports(), 50);
        assert_eq!(accounts[1].owner(), &new_owner);
        assert_eq!(accounts[1].data(), &[0, 0]);
    }

    #[test]
    fn test_create_account_with_seed() {
        let new_owner = Pubkey::new(&[9; 32]);
        let from = Pubkey::new_unique();
        let seed = "shiny pepper";
        let to = Pubkey::create_with_seed(&from, seed, &new_owner).unwrap();
        let from_account = AccountSharedData::new(100, 0, &system_program::id());
        let to_account = AccountSharedData::new(0, 0, &Pubkey::default());

        let accounts = process_instruction(
            &bincode::serialize(&SystemInstruction::CreateAccountWithSeed {
                base: from,
                seed: seed.to_string(),
                lamports: 50,
                space: 2,
                owner: new_owner,
            })
            .unwrap(),
            vec![(from, from_account), (to, to_account)],
            vec![
                AccountMeta {
                    pubkey: from,
                    is_signer: true,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: to,
                    is_signer: true,
                    is_writable: false,
                },
            ],
            Ok(()),
            super::process_instruction,
        );
        assert_eq!(accounts[0].lamports(), 50);
        assert_eq!(accounts[1].lamports(), 50);
        assert_eq!(accounts[1].owner(), &new_owner);
        assert_eq!(accounts[1].data(), &[0, 0]);
    }

    #[test]
    fn test_create_account_with_seed_separate_base_account() {
        let new_owner = Pubkey::new(&[9; 32]);
        let from = Pubkey::new_unique();
        let base = Pubkey::new_unique();
        let seed = "shiny pepper";
        let to = Pubkey::create_with_seed(&base, seed, &new_owner).unwrap();
        let from_account = AccountSharedData::new(100, 0, &system_program::id());
        let to_account = AccountSharedData::new(0, 0, &Pubkey::default());
        let base_account = AccountSharedData::new(0, 0, &Pubkey::default());

        let accounts = process_instruction(
            &bincode::serialize(&SystemInstruction::CreateAccountWithSeed {
                base,
                seed: seed.to_string(),
                lamports: 50,
                space: 2,
                owner: new_owner,
            })
            .unwrap(),
            vec![(from, from_account), (to, to_account), (base, base_account)],
            vec![
                AccountMeta {
                    pubkey: from,
                    is_signer: true,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: to,
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: base,
                    is_signer: true,
                    is_writable: false,
                },
            ],
            Ok(()),
            super::process_instruction,
        );
        assert_eq!(accounts[0].lamports(), 50);
        assert_eq!(accounts[1].lamports(), 50);
        assert_eq!(accounts[1].owner(), &new_owner);
        assert_eq!(accounts[1].data(), &[0, 0]);
    }

    #[test]
    fn test_address_create_with_seed_mismatch() {
        let mut transaction_context = TransactionContext::new(Vec::new(), 1, 1);
        let invoke_context = InvokeContext::new_mock(&mut transaction_context, &[]);
        let from = Pubkey::new_unique();
        let seed = "dull boy";
        let to = Pubkey::new_unique();
        let owner = Pubkey::new_unique();

        assert_eq!(
            Address::create(&to, Some((&from, seed, &owner)), &invoke_context),
            Err(SystemError::AddressWithSeedMismatch.into())
        );
    }

    #[test]
    fn test_create_account_with_seed_missing_sig() {
        let new_owner = Pubkey::new(&[9; 32]);
        let from = Pubkey::new_unique();
        let seed = "dull boy";
        let to = Pubkey::create_with_seed(&from, seed, &new_owner).unwrap();
        let from_account = AccountSharedData::new(100, 0, &system_program::id());
        let to_account = AccountSharedData::new(0, 0, &Pubkey::default());

        let accounts = process_instruction(
            &bincode::serialize(&SystemInstruction::CreateAccount {
                lamports: 50,
                space: 2,
                owner: new_owner,
            })
            .unwrap(),
            vec![(from, from_account), (to, to_account)],
            vec![
                AccountMeta {
                    pubkey: from,
                    is_signer: true,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: to,
                    is_signer: false,
                    is_writable: false,
                },
            ],
            Err(InstructionError::MissingRequiredSignature),
            super::process_instruction,
        );
        assert_eq!(accounts[0].lamports(), 100);
        assert_eq!(accounts[1], AccountSharedData::default());
    }

    #[test]
    fn test_create_with_zero_lamports() {
        // create account with zero lamports transferred
        let new_owner = Pubkey::new(&[9; 32]);
        let from = Pubkey::new_unique();
        let from_account = AccountSharedData::new(100, 0, &Pubkey::new_unique()); // not from system account
        let to = Pubkey::new_unique();
        let to_account = AccountSharedData::new(0, 0, &Pubkey::default());

        let accounts = process_instruction(
            &bincode::serialize(&SystemInstruction::CreateAccount {
                lamports: 0,
                space: 2,
                owner: new_owner,
            })
            .unwrap(),
            vec![(from, from_account), (to, to_account)],
            vec![
                AccountMeta {
                    pubkey: from,
                    is_signer: true,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: to,
                    is_signer: true,
                    is_writable: false,
                },
            ],
            Ok(()),
            super::process_instruction,
        );
        assert_eq!(accounts[0].lamports(), 100);
        assert_eq!(accounts[1].lamports(), 0);
        assert_eq!(*accounts[1].owner(), new_owner);
        assert_eq!(accounts[1].data(), &[0, 0]);
    }

    #[test]
    fn test_create_negative_lamports() {
        // Attempt to create account with more lamports than from_account has
        let new_owner = Pubkey::new(&[9; 32]);
        let from = Pubkey::new_unique();
        let from_account = AccountSharedData::new(100, 0, &Pubkey::new_unique());
        let to = Pubkey::new_unique();
        let to_account = AccountSharedData::new(0, 0, &Pubkey::default());

        process_instruction(
            &bincode::serialize(&SystemInstruction::CreateAccount {
                lamports: 150,
                space: 2,
                owner: new_owner,
            })
            .unwrap(),
            vec![(from, from_account), (to, to_account)],
            vec![
                AccountMeta {
                    pubkey: from,
                    is_signer: true,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: to,
                    is_signer: true,
                    is_writable: false,
                },
            ],
            Err(SystemError::ResultWithNegativeLamports.into()),
            super::process_instruction,
        );
    }

    #[test]
    fn test_request_more_than_allowed_data_length() {
        let from = Pubkey::new_unique();
        let from_account = AccountSharedData::new(100, 0, &Pubkey::new_unique());
        let to = Pubkey::new_unique();
        let to_account = AccountSharedData::new(0, 0, &Pubkey::default());
        let instruction_accounts = vec![
            AccountMeta {
                pubkey: from,
                is_signer: true,
                is_writable: false,
            },
            AccountMeta {
                pubkey: to,
                is_signer: true,
                is_writable: false,
            },
        ];

        // Trying to request more data length than permitted will result in failure
        process_instruction(
            &bincode::serialize(&SystemInstruction::CreateAccount {
                lamports: 50,
                space: MAX_PERMITTED_DATA_LENGTH + 1,
                owner: system_program::id(),
            })
            .unwrap(),
            vec![(from, from_account.clone()), (to, to_account.clone())],
            instruction_accounts.clone(),
            Err(SystemError::InvalidAccountDataLength.into()),
            super::process_instruction,
        );

        // Trying to request equal or less data length than permitted will be successful
        let accounts = process_instruction(
            &bincode::serialize(&SystemInstruction::CreateAccount {
                lamports: 50,
                space: MAX_PERMITTED_DATA_LENGTH,
                owner: system_program::id(),
            })
            .unwrap(),
            vec![(from, from_account), (to, to_account)],
            instruction_accounts,
            Ok(()),
            super::process_instruction,
        );
        assert_eq!(accounts[1].lamports(), 50);
        assert_eq!(accounts[1].data().len() as u64, MAX_PERMITTED_DATA_LENGTH);
    }

    #[test]
    fn test_create_already_in_use() {
        let new_owner = Pubkey::new(&[9; 32]);
        let from = Pubkey::new_unique();
        let from_account = AccountSharedData::new(100, 0, &system_program::id());
        let owned_key = Pubkey::new_unique();

        // Attempt to create system account in account already owned by another program
        let original_program_owner = Pubkey::new(&[5; 32]);
        let owned_account = AccountSharedData::new(0, 0, &original_program_owner);
        let unchanged_account = owned_account.clone();
        let accounts = process_instruction(
            &bincode::serialize(&SystemInstruction::CreateAccount {
                lamports: 50,
                space: 2,
                owner: new_owner,
            })
            .unwrap(),
            vec![(from, from_account.clone()), (owned_key, owned_account)],
            vec![
                AccountMeta {
                    pubkey: from,
                    is_signer: true,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: owned_key,
                    is_signer: true,
                    is_writable: false,
                },
            ],
            Err(SystemError::AccountAlreadyInUse.into()),
            super::process_instruction,
        );
        assert_eq!(accounts[0].lamports(), 100);
        assert_eq!(accounts[1], unchanged_account);

        // Attempt to create system account in account that already has data
        let owned_account = AccountSharedData::new(0, 1, &Pubkey::default());
        let unchanged_account = owned_account.clone();
        let accounts = process_instruction(
            &bincode::serialize(&SystemInstruction::CreateAccount {
                lamports: 50,
                space: 2,
                owner: new_owner,
            })
            .unwrap(),
            vec![(from, from_account.clone()), (owned_key, owned_account)],
            vec![
                AccountMeta {
                    pubkey: from,
                    is_signer: true,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: owned_key,
                    is_signer: true,
                    is_writable: false,
                },
            ],
            Err(SystemError::AccountAlreadyInUse.into()),
            super::process_instruction,
        );
        assert_eq!(accounts[0].lamports(), 100);
        assert_eq!(accounts[1], unchanged_account);

        // Attempt to create an account that already has lamports
        let owned_account = AccountSharedData::new(1, 0, &Pubkey::default());
        let unchanged_account = owned_account.clone();
        let accounts = process_instruction(
            &bincode::serialize(&SystemInstruction::CreateAccount {
                lamports: 50,
                space: 2,
                owner: new_owner,
            })
            .unwrap(),
            vec![(from, from_account), (owned_key, owned_account)],
            vec![
                AccountMeta {
                    pubkey: from,
                    is_signer: true,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: owned_key,
                    is_signer: true,
                    is_writable: false,
                },
            ],
            Err(SystemError::AccountAlreadyInUse.into()),
            super::process_instruction,
        );
        assert_eq!(accounts[0].lamports(), 100);
        assert_eq!(accounts[1], unchanged_account);
    }

    #[test]
    fn test_create_unsigned() {
        // Attempt to create an account without signing the transfer
        let new_owner = Pubkey::new(&[9; 32]);
        let from = Pubkey::new_unique();
        let from_account = AccountSharedData::new(100, 0, &system_program::id());
        let owned_key = Pubkey::new_unique();
        let owned_account = AccountSharedData::new(0, 0, &Pubkey::default());

        // Haven't signed from account
        process_instruction(
            &bincode::serialize(&SystemInstruction::CreateAccount {
                lamports: 50,
                space: 2,
                owner: new_owner,
            })
            .unwrap(),
            vec![
                (from, from_account.clone()),
                (owned_key, owned_account.clone()),
            ],
            vec![
                AccountMeta {
                    pubkey: from,
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: owned_key,
                    is_signer: false,
                    is_writable: false,
                },
            ],
            Err(InstructionError::MissingRequiredSignature),
            super::process_instruction,
        );

        // Haven't signed to account
        process_instruction(
            &bincode::serialize(&SystemInstruction::CreateAccount {
                lamports: 50,
                space: 2,
                owner: new_owner,
            })
            .unwrap(),
            vec![(from, from_account.clone()), (owned_key, owned_account)],
            vec![
                AccountMeta {
                    pubkey: from,
                    is_signer: true,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: owned_key,
                    is_signer: false,
                    is_writable: false,
                },
            ],
            Err(InstructionError::MissingRequiredSignature),
            super::process_instruction,
        );

        // Don't support unsigned creation with zero lamports (ephemeral account)
        let owned_account = AccountSharedData::new(0, 0, &Pubkey::default());
        process_instruction(
            &bincode::serialize(&SystemInstruction::CreateAccount {
                lamports: 50,
                space: 2,
                owner: new_owner,
            })
            .unwrap(),
            vec![(from, from_account), (owned_key, owned_account)],
            vec![
                AccountMeta {
                    pubkey: from,
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: owned_key,
                    is_signer: false,
                    is_writable: false,
                },
            ],
            Err(InstructionError::MissingRequiredSignature),
            super::process_instruction,
        );
    }

    #[test]
    fn test_create_sysvar_invalid_id_with_feature() {
        // Attempt to create system account in account already owned by another program
        let from = Pubkey::new_unique();
        let from_account = AccountSharedData::new(100, 0, &system_program::id());
        let to = Pubkey::new_unique();
        let to_account = AccountSharedData::new(0, 0, &system_program::id());

        // fail to create a sysvar::id() owned account
        process_instruction(
            &bincode::serialize(&SystemInstruction::CreateAccount {
                lamports: 50,
                space: 2,
                owner: sysvar::id(),
            })
            .unwrap(),
            vec![(from, from_account), (to, to_account)],
            vec![
                AccountMeta {
                    pubkey: from,
                    is_signer: true,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: to,
                    is_signer: true,
                    is_writable: false,
                },
            ],
            Ok(()),
            super::process_instruction,
        );
    }

    #[test]
    fn test_create_data_populated() {
        // Attempt to create system account in account with populated data
        let new_owner = Pubkey::new(&[9; 32]);
        let from = Pubkey::new_unique();
        let from_account = AccountSharedData::new(100, 0, &system_program::id());
        let populated_key = Pubkey::new_unique();
        let populated_account = AccountSharedData::from(Account {
            data: vec![0, 1, 2, 3],
            ..Account::default()
        });

        process_instruction(
            &bincode::serialize(&SystemInstruction::CreateAccount {
                lamports: 50,
                space: 2,
                owner: new_owner,
            })
            .unwrap(),
            vec![(from, from_account), (populated_key, populated_account)],
            vec![
                AccountMeta {
                    pubkey: from,
                    is_signer: true,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: populated_key,
                    is_signer: true,
                    is_writable: false,
                },
            ],
            Err(SystemError::AccountAlreadyInUse.into()),
            super::process_instruction,
        );
    }

    #[test]
    fn test_create_from_account_is_nonce_fail() {
        let nonce = Pubkey::new_unique();
        let nonce_account = AccountSharedData::new_data(
            42,
            &nonce::state::Versions::new_current(nonce::State::Initialized(
                nonce::state::Data::default(),
            )),
            &system_program::id(),
        )
        .unwrap();
        let new = Pubkey::new_unique();
        let new_account = AccountSharedData::new(0, 0, &system_program::id());

        process_instruction(
            &bincode::serialize(&SystemInstruction::CreateAccount {
                lamports: 42,
                space: 0,
                owner: Pubkey::new_unique(),
            })
            .unwrap(),
            vec![(nonce, nonce_account), (new, new_account)],
            vec![
                AccountMeta {
                    pubkey: nonce,
                    is_signer: true,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: new,
                    is_signer: true,
                    is_writable: false,
                },
            ],
            Err(InstructionError::InvalidArgument),
            super::process_instruction,
        );
    }

    #[test]
    fn test_assign() {
        let mut transaction_context = TransactionContext::new(Vec::new(), 1, 1);
        let invoke_context = InvokeContext::new_mock(&mut transaction_context, &[]);
        let new_owner = Pubkey::new(&[9; 32]);
        let pubkey = Pubkey::new_unique();
        let mut account = AccountSharedData::new(100, 0, &system_program::id());

        assert_eq!(
            assign(
                &mut account,
                &pubkey.into(),
                &new_owner,
                &HashSet::new(),
                &invoke_context,
            ),
            Err(InstructionError::MissingRequiredSignature)
        );

        // no change, no signature needed
        assert_eq!(
            assign(
                &mut account,
                &pubkey.into(),
                &system_program::id(),
                &HashSet::new(),
                &invoke_context,
            ),
            Ok(())
        );

        process_instruction(
            &bincode::serialize(&SystemInstruction::Assign { owner: new_owner }).unwrap(),
            vec![(pubkey, account)],
            vec![AccountMeta {
                pubkey,
                is_signer: true,
                is_writable: false,
            }],
            Ok(()),
            super::process_instruction,
        );
    }

    #[test]
    fn test_assign_to_sysvar_with_feature() {
        let mut transaction_context = TransactionContext::new(Vec::new(), 1, 1);
        let invoke_context = InvokeContext::new_mock(&mut transaction_context, &[]);
        let new_owner = sysvar::id();
        let from = Pubkey::new_unique();
        let mut from_account = AccountSharedData::new(100, 0, &system_program::id());

        assert_eq!(
            assign(
                &mut from_account,
                &from.into(),
                &new_owner,
                &[from].iter().cloned().collect::<HashSet<_>>(),
                &invoke_context,
            ),
            Ok(())
        );
    }

    #[test]
    fn test_process_bogus_instruction() {
        // Attempt to assign with no accounts
        let instruction = SystemInstruction::Assign {
            owner: Pubkey::new_unique(),
        };
        let data = serialize(&instruction).unwrap();
        process_instruction(
            &data,
            Vec::new(),
            Vec::new(),
            Err(InstructionError::NotEnoughAccountKeys),
            super::process_instruction,
        );

        // Attempt to transfer with no destination
        let from = Pubkey::new_unique();
        let from_account = AccountSharedData::new(100, 0, &system_program::id());
        let instruction = SystemInstruction::Transfer { lamports: 0 };
        let data = serialize(&instruction).unwrap();
        process_instruction(
            &data,
            vec![(from, from_account)],
            vec![AccountMeta {
                pubkey: from,
                is_signer: true,
                is_writable: false,
            }],
            Err(InstructionError::NotEnoughAccountKeys),
            super::process_instruction,
        );
    }

    #[test]
    fn test_transfer_lamports() {
        let from = Pubkey::new_unique();
        let from_account = AccountSharedData::new(100, 0, &Pubkey::new(&[2; 32])); // account owner should not matter
        let to = Pubkey::new(&[3; 32]);
        let to_account = AccountSharedData::new(1, 0, &to); // account owner should not matter
        let transaction_accounts = vec![(from, from_account), (to, to_account)];
        let instruction_accounts = vec![
            AccountMeta {
                pubkey: from,
                is_signer: true,
                is_writable: false,
            },
            AccountMeta {
                pubkey: to,
                is_signer: false,
                is_writable: false,
            },
        ];

        // Success case
        let accounts = process_instruction(
            &bincode::serialize(&SystemInstruction::Transfer { lamports: 50 }).unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Ok(()),
            super::process_instruction,
        );
        assert_eq!(accounts[0].lamports(), 50);
        assert_eq!(accounts[1].lamports(), 51);

        // Attempt to move more lamports than from_account has
        let accounts = process_instruction(
            &bincode::serialize(&SystemInstruction::Transfer { lamports: 101 }).unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Err(SystemError::ResultWithNegativeLamports.into()),
            super::process_instruction,
        );
        assert_eq!(accounts[0].lamports(), 100);
        assert_eq!(accounts[1].lamports(), 1);

        // test signed transfer of zero
        let accounts = process_instruction(
            &bincode::serialize(&SystemInstruction::Transfer { lamports: 0 }).unwrap(),
            transaction_accounts.clone(),
            instruction_accounts,
            Ok(()),
            super::process_instruction,
        );
        assert_eq!(accounts[0].lamports(), 100);
        assert_eq!(accounts[1].lamports(), 1);

        // test unsigned transfer of zero
        let accounts = process_instruction(
            &bincode::serialize(&SystemInstruction::Transfer { lamports: 0 }).unwrap(),
            transaction_accounts,
            vec![
                AccountMeta {
                    pubkey: from,
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: to,
                    is_signer: false,
                    is_writable: false,
                },
            ],
            Err(InstructionError::MissingRequiredSignature),
            super::process_instruction,
        );
        assert_eq!(accounts[0].lamports(), 100);
        assert_eq!(accounts[1].lamports(), 1);
    }

    #[test]
    fn test_transfer_with_seed() {
        let base = Pubkey::new_unique();
        let base_account = AccountSharedData::new(100, 0, &Pubkey::new(&[2; 32])); // account owner should not matter
        let from_seed = "42".to_string();
        let from_owner = system_program::id();
        let from = Pubkey::create_with_seed(&base, from_seed.as_str(), &from_owner).unwrap();
        let from_account = AccountSharedData::new(100, 0, &Pubkey::new(&[2; 32])); // account owner should not matter
        let to = Pubkey::new(&[3; 32]);
        let to_account = AccountSharedData::new(1, 0, &to); // account owner should not matter
        let transaction_accounts =
            vec![(from, from_account), (base, base_account), (to, to_account)];
        let instruction_accounts = vec![
            AccountMeta {
                pubkey: from,
                is_signer: true,
                is_writable: false,
            },
            AccountMeta {
                pubkey: base,
                is_signer: true,
                is_writable: false,
            },
            AccountMeta {
                pubkey: to,
                is_signer: false,
                is_writable: false,
            },
        ];

        // Success case
        let accounts = process_instruction(
            &bincode::serialize(&SystemInstruction::TransferWithSeed {
                lamports: 50,
                from_seed: from_seed.clone(),
                from_owner,
            })
            .unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Ok(()),
            super::process_instruction,
        );
        assert_eq!(accounts[0].lamports(), 50);
        assert_eq!(accounts[2].lamports(), 51);

        // Attempt to move more lamports than from_account has
        let accounts = process_instruction(
            &bincode::serialize(&SystemInstruction::TransferWithSeed {
                lamports: 101,
                from_seed: from_seed.clone(),
                from_owner,
            })
            .unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Err(SystemError::ResultWithNegativeLamports.into()),
            super::process_instruction,
        );
        assert_eq!(accounts[0].lamports(), 100);
        assert_eq!(accounts[2].lamports(), 1);

        // Test unsigned transfer of zero
        let accounts = process_instruction(
            &bincode::serialize(&SystemInstruction::TransferWithSeed {
                lamports: 0,
                from_seed,
                from_owner,
            })
            .unwrap(),
            transaction_accounts,
            instruction_accounts,
            Ok(()),
            super::process_instruction,
        );
        assert_eq!(accounts[0].lamports(), 100);
        assert_eq!(accounts[2].lamports(), 1);
    }

    #[test]
    fn test_transfer_lamports_from_nonce_account_fail() {
        let from = Pubkey::new_unique();
        let from_account = AccountSharedData::new_data(
            100,
            &nonce::state::Versions::new_current(nonce::State::Initialized(nonce::state::Data {
                authority: from,
                ..nonce::state::Data::default()
            })),
            &system_program::id(),
        )
        .unwrap();
        assert_eq!(
            get_system_account_kind(&from_account),
            Some(SystemAccountKind::Nonce)
        );
        let to = Pubkey::new(&[3; 32]);
        let to_account = AccountSharedData::new(1, 0, &to); // account owner should not matter

        process_instruction(
            &bincode::serialize(&SystemInstruction::Transfer { lamports: 50 }).unwrap(),
            vec![(from, from_account), (to, to_account)],
            vec![
                AccountMeta {
                    pubkey: from,
                    is_signer: true,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: to,
                    is_signer: false,
                    is_writable: false,
                },
            ],
            Err(InstructionError::InvalidArgument),
            super::process_instruction,
        );
    }

    #[test]
    fn test_allocate() {
        let (genesis_config, mint_keypair) = create_genesis_config(100);
        let bank = Bank::new_for_tests(&genesis_config);
        let bank_client = BankClient::new(bank);

        let alice_keypair = Keypair::new();
        let alice_pubkey = alice_keypair.pubkey();
        let seed = "seed";
        let owner = Pubkey::new_unique();
        let alice_with_seed = Pubkey::create_with_seed(&alice_pubkey, seed, &owner).unwrap();

        bank_client
            .transfer_and_confirm(50, &mint_keypair, &alice_pubkey)
            .unwrap();

        let allocate_with_seed = Message::new(
            &[system_instruction::allocate_with_seed(
                &alice_with_seed,
                &alice_pubkey,
                seed,
                2,
                &owner,
            )],
            Some(&alice_pubkey),
        );

        assert!(bank_client
            .send_and_confirm_message(&[&alice_keypair], allocate_with_seed)
            .is_ok());

        let allocate = system_instruction::allocate(&alice_pubkey, 2);

        assert!(bank_client
            .send_and_confirm_instruction(&alice_keypair, allocate)
            .is_ok());
    }

    fn with_create_zero_lamport<F>(callback: F)
    where
        F: Fn(&Bank),
    {
        solana_logger::setup();

        let alice_keypair = Keypair::new();
        let bob_keypair = Keypair::new();

        let alice_pubkey = alice_keypair.pubkey();
        let bob_pubkey = bob_keypair.pubkey();

        let program = Pubkey::new_unique();
        let collector = Pubkey::new_unique();

        let mint_lamports = 10000;
        let len1 = 123;
        let len2 = 456;

        // create initial bank and fund the alice account
        let (genesis_config, mint_keypair) = create_genesis_config(mint_lamports);
        let bank = Arc::new(Bank::new_for_tests(&genesis_config));
        let bank_client = BankClient::new_shared(&bank);
        bank_client
            .transfer_and_confirm(mint_lamports, &mint_keypair, &alice_pubkey)
            .unwrap();

        // create zero-lamports account to be cleaned
        let bank = Arc::new(Bank::new_from_parent(&bank, &collector, bank.slot() + 1));
        let bank_client = BankClient::new_shared(&bank);
        let ix = system_instruction::create_account(&alice_pubkey, &bob_pubkey, 0, len1, &program);
        let message = Message::new(&[ix], Some(&alice_keypair.pubkey()));
        let r = bank_client.send_and_confirm_message(&[&alice_keypair, &bob_keypair], message);
        assert!(r.is_ok());

        // transfer some to bogus pubkey just to make previous bank (=slot) really cleanable
        let bank = Arc::new(Bank::new_from_parent(&bank, &collector, bank.slot() + 1));
        let bank_client = BankClient::new_shared(&bank);
        bank_client
            .transfer_and_confirm(50, &alice_keypair, &Pubkey::new_unique())
            .unwrap();

        // super fun time; callback chooses to .clean_accounts(None) or not
        callback(&*bank);

        // create a normal account at the same pubkey as the zero-lamports account
        let bank = Arc::new(Bank::new_from_parent(&bank, &collector, bank.slot() + 1));
        let bank_client = BankClient::new_shared(&bank);
        let ix = system_instruction::create_account(&alice_pubkey, &bob_pubkey, 1, len2, &program);
        let message = Message::new(&[ix], Some(&alice_pubkey));
        let r = bank_client.send_and_confirm_message(&[&alice_keypair, &bob_keypair], message);
        assert!(r.is_ok());
    }

    #[test]
    fn test_create_zero_lamport_with_clean() {
        with_create_zero_lamport(|bank| {
            bank.freeze();
            bank.squash();
            bank.force_flush_accounts_cache();
            // do clean and assert that it actually did its job
            assert_eq!(3, bank.get_snapshot_storages(None).len());
            bank.clean_accounts(false, false, None);
            assert_eq!(2, bank.get_snapshot_storages(None).len());
        });
    }

    #[test]
    fn test_create_zero_lamport_without_clean() {
        with_create_zero_lamport(|_| {
            // just do nothing; this should behave identically with test_create_zero_lamport_with_clean
        });
    }

    #[test]
    fn test_assign_with_seed() {
        let (genesis_config, mint_keypair) = create_genesis_config(100);
        let bank = Bank::new_for_tests(&genesis_config);
        let bank_client = BankClient::new(bank);

        let alice_keypair = Keypair::new();
        let alice_pubkey = alice_keypair.pubkey();
        let seed = "seed";
        let owner = Pubkey::new_unique();
        let alice_with_seed = Pubkey::create_with_seed(&alice_pubkey, seed, &owner).unwrap();

        bank_client
            .transfer_and_confirm(50, &mint_keypair, &alice_pubkey)
            .unwrap();

        let assign_with_seed = Message::new(
            &[system_instruction::assign_with_seed(
                &alice_with_seed,
                &alice_pubkey,
                seed,
                &owner,
            )],
            Some(&alice_pubkey),
        );

        assert!(bank_client
            .send_and_confirm_message(&[&alice_keypair], assign_with_seed)
            .is_ok());
    }

    #[test]
    fn test_system_unsigned_transaction() {
        let (genesis_config, alice_keypair) = create_genesis_config(100);
        let alice_pubkey = alice_keypair.pubkey();
        let mallory_keypair = Keypair::new();
        let mallory_pubkey = mallory_keypair.pubkey();

        // Fund to account to bypass AccountNotFound error
        let bank = Bank::new_for_tests(&genesis_config);
        let bank_client = BankClient::new(bank);
        bank_client
            .transfer_and_confirm(50, &alice_keypair, &mallory_pubkey)
            .unwrap();

        // Erroneously sign transaction with recipient account key
        // No signature case is tested by bank `test_zero_signatures()`
        let account_metas = vec![
            AccountMeta::new(alice_pubkey, false),
            AccountMeta::new(mallory_pubkey, true),
        ];
        let malicious_instruction = Instruction::new_with_bincode(
            system_program::id(),
            &SystemInstruction::Transfer { lamports: 10 },
            account_metas,
        );
        assert_eq!(
            bank_client
                .send_and_confirm_instruction(&mallory_keypair, malicious_instruction)
                .unwrap_err()
                .unwrap(),
            TransactionError::InstructionError(0, InstructionError::MissingRequiredSignature)
        );
        assert_eq!(bank_client.get_balance(&alice_pubkey).unwrap(), 50);
        assert_eq!(bank_client.get_balance(&mallory_pubkey).unwrap(), 50);
    }

    fn process_nonce_instruction(
        instruction: Instruction,
        expected_result: Result<(), InstructionError>,
    ) -> Vec<AccountSharedData> {
        let transaction_accounts = instruction
            .accounts
            .iter()
            .map(|meta| {
                #[allow(deprecated)]
                (
                    meta.pubkey,
                    if sysvar::recent_blockhashes::check_id(&meta.pubkey) {
                        create_default_recent_blockhashes_account()
                    } else if sysvar::rent::check_id(&meta.pubkey) {
                        account::create_account_shared_data_for_test(&Rent::free())
                    } else {
                        AccountSharedData::new(0, 0, &Pubkey::new_unique())
                    },
                )
            })
            .collect();
        process_instruction(
            &instruction.data,
            transaction_accounts,
            instruction.accounts,
            expected_result,
            super::process_instruction,
        )
    }

    #[test]
    fn test_process_nonce_ix_no_acc_data_fail() {
        let none_address = Pubkey::new_unique();
        process_nonce_instruction(
            system_instruction::advance_nonce_account(&none_address, &none_address),
            Err(InstructionError::InvalidAccountData),
        );
    }

    #[test]
    fn test_process_nonce_ix_no_keyed_accs_fail() {
        process_instruction(
            &serialize(&SystemInstruction::AdvanceNonceAccount).unwrap(),
            Vec::new(),
            Vec::new(),
            Err(InstructionError::NotEnoughAccountKeys),
            super::process_instruction,
        );
    }

    #[test]
    fn test_process_nonce_ix_only_nonce_acc_fail() {
        let pubkey = Pubkey::new_unique();
        process_instruction(
            &serialize(&SystemInstruction::AdvanceNonceAccount).unwrap(),
            vec![(pubkey, create_default_account())],
            vec![AccountMeta {
                pubkey,
                is_signer: true,
                is_writable: true,
            }],
            Err(InstructionError::NotEnoughAccountKeys),
            super::process_instruction,
        );
    }

    #[test]
    fn test_process_nonce_ix_ok() {
        let nonce_address = Pubkey::new_unique();
        let nonce_account = nonce_account::create_account(1_000_000).into_inner();
        #[allow(deprecated)]
        let blockhash_id = sysvar::recent_blockhashes::id();
        let accounts = process_instruction(
            &serialize(&SystemInstruction::InitializeNonceAccount(nonce_address)).unwrap(),
            vec![
                (nonce_address, nonce_account),
                (blockhash_id, create_default_recent_blockhashes_account()),
                (sysvar::rent::id(), create_default_rent_account()),
            ],
            vec![
                AccountMeta {
                    pubkey: nonce_address,
                    is_signer: true,
                    is_writable: true,
                },
                AccountMeta {
                    pubkey: blockhash_id,
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: sysvar::rent::id(),
                    is_signer: false,
                    is_writable: false,
                },
            ],
            Ok(()),
            super::process_instruction,
        );
        let blockhash = hash(&serialize(&0).unwrap());
        #[allow(deprecated)]
        let new_recent_blockhashes_account =
            solana_sdk::recent_blockhashes_account::create_account_with_data_for_test(
                vec![IterItem(0u64, &blockhash, 0); sysvar::recent_blockhashes::MAX_ENTRIES]
                    .into_iter(),
            );
        process_instruction(
            &serialize(&SystemInstruction::AdvanceNonceAccount).unwrap(),
            vec![
                (nonce_address, accounts[0].clone()),
                (blockhash_id, new_recent_blockhashes_account),
            ],
            vec![
                AccountMeta {
                    pubkey: nonce_address,
                    is_signer: true,
                    is_writable: true,
                },
                AccountMeta {
                    pubkey: blockhash_id,
                    is_signer: false,
                    is_writable: false,
                },
            ],
            Ok(()),
            |first_instruction_account: usize,
             instruction_data: &[u8],
             invoke_context: &mut InvokeContext| {
                invoke_context.blockhash = hash(&serialize(&0).unwrap());
                super::process_instruction(
                    first_instruction_account,
                    instruction_data,
                    invoke_context,
                )
            },
        );
    }

    #[test]
    fn test_process_withdraw_ix_no_acc_data_fail() {
        let nonce_address = Pubkey::new_unique();
        process_nonce_instruction(
            system_instruction::withdraw_nonce_account(
                &nonce_address,
                &Pubkey::new_unique(),
                &nonce_address,
                1,
            ),
            Err(InstructionError::InvalidAccountData),
        );
    }

    #[test]
    fn test_process_withdraw_ix_no_keyed_accs_fail() {
        process_instruction(
            &serialize(&SystemInstruction::WithdrawNonceAccount(42)).unwrap(),
            Vec::new(),
            Vec::new(),
            Err(InstructionError::NotEnoughAccountKeys),
            super::process_instruction,
        );
    }

    #[test]
    fn test_process_withdraw_ix_only_nonce_acc_fail() {
        let nonce_address = Pubkey::new_unique();
        process_instruction(
            &serialize(&SystemInstruction::WithdrawNonceAccount(42)).unwrap(),
            vec![(nonce_address, create_default_account())],
            vec![AccountMeta {
                pubkey: nonce_address,
                is_signer: true,
                is_writable: true,
            }],
            Err(InstructionError::NotEnoughAccountKeys),
            super::process_instruction,
        );
    }

    #[test]
    fn test_process_withdraw_ix_ok() {
        let nonce_address = Pubkey::new_unique();
        let nonce_account = nonce_account::create_account(1_000_000).into_inner();
        let pubkey = Pubkey::new_unique();
        #[allow(deprecated)]
        let blockhash_id = sysvar::recent_blockhashes::id();
        process_instruction(
            &serialize(&SystemInstruction::WithdrawNonceAccount(42)).unwrap(),
            vec![
                (nonce_address, nonce_account),
                (pubkey, create_default_account()),
                (blockhash_id, create_default_recent_blockhashes_account()),
                (sysvar::rent::id(), create_default_rent_account()),
            ],
            vec![
                AccountMeta {
                    pubkey: nonce_address,
                    is_signer: true,
                    is_writable: true,
                },
                AccountMeta {
                    pubkey,
                    is_signer: true,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: blockhash_id,
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: sysvar::rent::id(),
                    is_signer: false,
                    is_writable: false,
                },
            ],
            Ok(()),
            super::process_instruction,
        );
    }

    #[test]
    fn test_process_initialize_ix_no_keyed_accs_fail() {
        process_instruction(
            &serialize(&SystemInstruction::InitializeNonceAccount(Pubkey::default())).unwrap(),
            Vec::new(),
            Vec::new(),
            Err(InstructionError::NotEnoughAccountKeys),
            super::process_instruction,
        );
    }

    #[test]
    fn test_process_initialize_ix_only_nonce_acc_fail() {
        let nonce_address = Pubkey::new_unique();
        let nonce_account = nonce_account::create_account(1_000_000).into_inner();
        process_instruction(
            &serialize(&SystemInstruction::InitializeNonceAccount(nonce_address)).unwrap(),
            vec![(nonce_address, nonce_account)],
            vec![AccountMeta {
                pubkey: nonce_address,
                is_signer: true,
                is_writable: true,
            }],
            Err(InstructionError::NotEnoughAccountKeys),
            super::process_instruction,
        );
    }

    #[test]
    fn test_process_initialize_ix_ok() {
        let nonce_address = Pubkey::new_unique();
        let nonce_account = nonce_account::create_account(1_000_000).into_inner();
        #[allow(deprecated)]
        let blockhash_id = sysvar::recent_blockhashes::id();
        process_instruction(
            &serialize(&SystemInstruction::InitializeNonceAccount(nonce_address)).unwrap(),
            vec![
                (nonce_address, nonce_account),
                (blockhash_id, create_default_recent_blockhashes_account()),
                (sysvar::rent::id(), create_default_rent_account()),
            ],
            vec![
                AccountMeta {
                    pubkey: nonce_address,
                    is_signer: true,
                    is_writable: true,
                },
                AccountMeta {
                    pubkey: blockhash_id,
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: sysvar::rent::id(),
                    is_signer: false,
                    is_writable: false,
                },
            ],
            Ok(()),
            super::process_instruction,
        );
    }

    #[test]
    fn test_process_authorize_ix_ok() {
        let nonce_address = Pubkey::new_unique();
        let nonce_account = nonce_account::create_account(1_000_000).into_inner();
        #[allow(deprecated)]
        let blockhash_id = sysvar::recent_blockhashes::id();
        let accounts = process_instruction(
            &serialize(&SystemInstruction::InitializeNonceAccount(nonce_address)).unwrap(),
            vec![
                (nonce_address, nonce_account),
                (blockhash_id, create_default_recent_blockhashes_account()),
                (sysvar::rent::id(), create_default_rent_account()),
            ],
            vec![
                AccountMeta {
                    pubkey: nonce_address,
                    is_signer: true,
                    is_writable: true,
                },
                AccountMeta {
                    pubkey: blockhash_id,
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: sysvar::rent::id(),
                    is_signer: false,
                    is_writable: false,
                },
            ],
            Ok(()),
            super::process_instruction,
        );
        process_instruction(
            &serialize(&SystemInstruction::AuthorizeNonceAccount(nonce_address)).unwrap(),
            vec![(nonce_address, accounts[0].clone())],
            vec![AccountMeta {
                pubkey: nonce_address,
                is_signer: true,
                is_writable: true,
            }],
            Ok(()),
            super::process_instruction,
        );
    }

    #[test]
    fn test_process_authorize_bad_account_data_fail() {
        let nonce_address = Pubkey::new_unique();
        process_nonce_instruction(
            system_instruction::authorize_nonce_account(
                &nonce_address,
                &Pubkey::new_unique(),
                &nonce_address,
            ),
            Err(InstructionError::InvalidAccountData),
        );
    }

    #[test]
    fn test_get_system_account_kind_system_ok() {
        let system_account = AccountSharedData::default();
        assert_eq!(
            get_system_account_kind(&system_account),
            Some(SystemAccountKind::System)
        );
    }

    #[test]
    fn test_get_system_account_kind_nonce_ok() {
        let nonce_account = AccountSharedData::new_data(
            42,
            &nonce::state::Versions::new_current(nonce::State::Initialized(
                nonce::state::Data::default(),
            )),
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
            get_system_account_kind(&nonce_account::create_account(42).borrow()),
            None
        );
    }

    #[test]
    fn test_get_system_account_kind_system_owner_nonzero_nonnonce_data_fail() {
        let other_data_account =
            AccountSharedData::new_data(42, b"other", &Pubkey::default()).unwrap();
        assert_eq!(get_system_account_kind(&other_data_account), None);
    }

    #[test]
    fn test_get_system_account_kind_nonsystem_owner_with_nonce_data_fail() {
        let nonce_account = AccountSharedData::new_data(
            42,
            &nonce::state::Versions::new_current(nonce::State::Initialized(
                nonce::state::Data::default(),
            )),
            &Pubkey::new_unique(),
        )
        .unwrap();
        assert_eq!(get_system_account_kind(&nonce_account), None);
    }

    #[test]
    fn test_nonce_initialize_with_empty_recent_blockhashes_fail() {
        let nonce_address = Pubkey::new_unique();
        let nonce_account = nonce_account::create_account(1_000_000).into_inner();
        #[allow(deprecated)]
        let blockhash_id = sysvar::recent_blockhashes::id();
        #[allow(deprecated)]
        let new_recent_blockhashes_account =
            solana_sdk::recent_blockhashes_account::create_account_with_data_for_test(
                vec![].into_iter(),
            );
        process_instruction(
            &serialize(&SystemInstruction::InitializeNonceAccount(nonce_address)).unwrap(),
            vec![
                (nonce_address, nonce_account),
                (blockhash_id, new_recent_blockhashes_account),
                (sysvar::rent::id(), create_default_rent_account()),
            ],
            vec![
                AccountMeta {
                    pubkey: nonce_address,
                    is_signer: true,
                    is_writable: true,
                },
                AccountMeta {
                    pubkey: blockhash_id,
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: sysvar::rent::id(),
                    is_signer: false,
                    is_writable: false,
                },
            ],
            Err(NonceError::NoRecentBlockhashes.into()),
            super::process_instruction,
        );
    }

    #[test]
    fn test_nonce_advance_with_empty_recent_blockhashes_fail() {
        let nonce_address = Pubkey::new_unique();
        let nonce_account = nonce_account::create_account(1_000_000).into_inner();
        #[allow(deprecated)]
        let blockhash_id = sysvar::recent_blockhashes::id();
        let accounts = process_instruction(
            &serialize(&SystemInstruction::InitializeNonceAccount(nonce_address)).unwrap(),
            vec![
                (nonce_address, nonce_account),
                (blockhash_id, create_default_recent_blockhashes_account()),
                (sysvar::rent::id(), create_default_rent_account()),
            ],
            vec![
                AccountMeta {
                    pubkey: nonce_address,
                    is_signer: true,
                    is_writable: true,
                },
                AccountMeta {
                    pubkey: blockhash_id,
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: sysvar::rent::id(),
                    is_signer: false,
                    is_writable: false,
                },
            ],
            Ok(()),
            super::process_instruction,
        );
        #[allow(deprecated)]
        let new_recent_blockhashes_account =
            solana_sdk::recent_blockhashes_account::create_account_with_data_for_test(
                vec![].into_iter(),
            );
        process_instruction(
            &serialize(&SystemInstruction::AdvanceNonceAccount).unwrap(),
            vec![
                (nonce_address, accounts[0].clone()),
                (blockhash_id, new_recent_blockhashes_account),
            ],
            vec![
                AccountMeta {
                    pubkey: nonce_address,
                    is_signer: true,
                    is_writable: true,
                },
                AccountMeta {
                    pubkey: blockhash_id,
                    is_signer: false,
                    is_writable: false,
                },
            ],
            Err(NonceError::NoRecentBlockhashes.into()),
            |first_instruction_account: usize,
             instruction_data: &[u8],
             invoke_context: &mut InvokeContext| {
                invoke_context.blockhash = hash(&serialize(&0).unwrap());
                super::process_instruction(
                    first_instruction_account,
                    instruction_data,
                    invoke_context,
                )
            },
        );
    }
}
