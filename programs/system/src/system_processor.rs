use {
    crate::system_instruction::{
        advance_nonce_account, authorize_nonce_account, initialize_nonce_account,
        withdraw_nonce_account,
    },
    log::*,
    solana_program_runtime::{
        declare_process_instruction, ic_msg, invoke_context::InvokeContext,
        sysvar_cache::get_sysvar_with_account_check,
    },
    solana_sdk::{
        feature_set,
        instruction::InstructionError,
        nonce,
        program_utils::limited_deserialize,
        pubkey::Pubkey,
        system_instruction::{SystemError, SystemInstruction, MAX_PERMITTED_DATA_LENGTH},
        system_program,
        transaction_context::{
            BorrowedAccount, IndexOfAccount, InstructionContext, TransactionContext,
        },
    },
    std::collections::HashSet,
};

// represents an address that may or may not have been generated
//  from a seed
#[derive(PartialEq, Eq, Default, Debug)]
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
    account: &mut BorrowedAccount,
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
    if !account.get_data().is_empty() || !system_program::check_id(account.get_owner()) {
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

    account.set_data_length(space as usize)?;

    Ok(())
}

fn assign(
    account: &mut BorrowedAccount,
    address: &Address,
    owner: &Pubkey,
    signers: &HashSet<Pubkey>,
    invoke_context: &InvokeContext,
) -> Result<(), InstructionError> {
    // no work to do, just return
    if account.get_owner() == owner {
        return Ok(());
    }

    if !address.is_signer(signers) {
        ic_msg!(invoke_context, "Assign: account {:?} must sign", address);
        return Err(InstructionError::MissingRequiredSignature);
    }

    account.set_owner(&owner.to_bytes())
}

fn allocate_and_assign(
    to: &mut BorrowedAccount,
    to_address: &Address,
    space: u64,
    owner: &Pubkey,
    signers: &HashSet<Pubkey>,
    invoke_context: &InvokeContext,
) -> Result<(), InstructionError> {
    allocate(to, to_address, space, signers, invoke_context)?;
    assign(to, to_address, owner, signers, invoke_context)
}

#[allow(clippy::too_many_arguments)]
fn create_account(
    from_account_index: IndexOfAccount,
    to_account_index: IndexOfAccount,
    to_address: &Address,
    lamports: u64,
    space: u64,
    owner: &Pubkey,
    signers: &HashSet<Pubkey>,
    invoke_context: &InvokeContext,
    transaction_context: &TransactionContext,
    instruction_context: &InstructionContext,
) -> Result<(), InstructionError> {
    // if it looks like the `to` account is already in use, bail
    {
        let mut to = instruction_context
            .try_borrow_instruction_account(transaction_context, to_account_index)?;
        if to.get_lamports() > 0 {
            ic_msg!(
                invoke_context,
                "Create Account: account {:?} already in use",
                to_address
            );
            return Err(SystemError::AccountAlreadyInUse.into());
        }

        allocate_and_assign(&mut to, to_address, space, owner, signers, invoke_context)?;
    }
    transfer(
        from_account_index,
        to_account_index,
        lamports,
        invoke_context,
        transaction_context,
        instruction_context,
    )
}

fn transfer_verified(
    from_account_index: IndexOfAccount,
    to_account_index: IndexOfAccount,
    lamports: u64,
    invoke_context: &InvokeContext,
    transaction_context: &TransactionContext,
    instruction_context: &InstructionContext,
) -> Result<(), InstructionError> {
    let mut from = instruction_context
        .try_borrow_instruction_account(transaction_context, from_account_index)?;
    if !from.get_data().is_empty() {
        ic_msg!(invoke_context, "Transfer: `from` must not carry data");
        return Err(InstructionError::InvalidArgument);
    }
    if lamports > from.get_lamports() {
        ic_msg!(
            invoke_context,
            "Transfer: insufficient lamports {}, need {}",
            from.get_lamports(),
            lamports
        );
        return Err(SystemError::ResultWithNegativeLamports.into());
    }

    from.checked_sub_lamports(lamports)?;
    drop(from);
    let mut to = instruction_context
        .try_borrow_instruction_account(transaction_context, to_account_index)?;
    to.checked_add_lamports(lamports)?;
    Ok(())
}

fn transfer(
    from_account_index: IndexOfAccount,
    to_account_index: IndexOfAccount,
    lamports: u64,
    invoke_context: &InvokeContext,
    transaction_context: &TransactionContext,
    instruction_context: &InstructionContext,
) -> Result<(), InstructionError> {
    if !invoke_context
        .feature_set
        .is_active(&feature_set::system_transfer_zero_check::id())
        && lamports == 0
    {
        return Ok(());
    }

    if !instruction_context.is_instruction_account_signer(from_account_index)? {
        ic_msg!(
            invoke_context,
            "Transfer: `from` account {} must sign",
            transaction_context.get_key_of_account_at_index(
                instruction_context
                    .get_index_of_instruction_account_in_transaction(from_account_index)?,
            )?,
        );
        return Err(InstructionError::MissingRequiredSignature);
    }

    transfer_verified(
        from_account_index,
        to_account_index,
        lamports,
        invoke_context,
        transaction_context,
        instruction_context,
    )
}

fn transfer_with_seed(
    from_account_index: IndexOfAccount,
    from_base_account_index: IndexOfAccount,
    from_seed: &str,
    from_owner: &Pubkey,
    to_account_index: IndexOfAccount,
    lamports: u64,
    invoke_context: &InvokeContext,
    transaction_context: &TransactionContext,
    instruction_context: &InstructionContext,
) -> Result<(), InstructionError> {
    if !invoke_context
        .feature_set
        .is_active(&feature_set::system_transfer_zero_check::id())
        && lamports == 0
    {
        return Ok(());
    }

    if !instruction_context.is_instruction_account_signer(from_base_account_index)? {
        ic_msg!(
            invoke_context,
            "Transfer: 'from' account {:?} must sign",
            transaction_context.get_key_of_account_at_index(
                instruction_context
                    .get_index_of_instruction_account_in_transaction(from_base_account_index)?,
            )?,
        );
        return Err(InstructionError::MissingRequiredSignature);
    }
    let address_from_seed = Pubkey::create_with_seed(
        transaction_context.get_key_of_account_at_index(
            instruction_context
                .get_index_of_instruction_account_in_transaction(from_base_account_index)?,
        )?,
        from_seed,
        from_owner,
    )?;

    let from_key = transaction_context.get_key_of_account_at_index(
        instruction_context.get_index_of_instruction_account_in_transaction(from_account_index)?,
    )?;
    if *from_key != address_from_seed {
        ic_msg!(
            invoke_context,
            "Transfer: 'from' address {} does not match derived address {}",
            from_key,
            address_from_seed
        );
        return Err(SystemError::AddressWithSeedMismatch.into());
    }

    transfer_verified(
        from_account_index,
        to_account_index,
        lamports,
        invoke_context,
        transaction_context,
        instruction_context,
    )
}

pub const DEFAULT_COMPUTE_UNITS: u64 = 150;

declare_process_instruction!(Entrypoint, DEFAULT_COMPUTE_UNITS, |invoke_context| {
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;
    let instruction_data = instruction_context.get_instruction_data();
    let instruction = limited_deserialize(instruction_data)?;

    trace!("process_instruction: {:?}", instruction);

    let signers = instruction_context.get_signers(transaction_context)?;
    match instruction {
        SystemInstruction::CreateAccount {
            lamports,
            space,
            owner,
        } => {
            instruction_context.check_number_of_instruction_accounts(2)?;
            let to_address = Address::create(
                transaction_context.get_key_of_account_at_index(
                    instruction_context.get_index_of_instruction_account_in_transaction(1)?,
                )?,
                None,
                invoke_context,
            )?;
            create_account(
                0,
                1,
                &to_address,
                lamports,
                space,
                &owner,
                &signers,
                invoke_context,
                transaction_context,
                instruction_context,
            )
        }
        SystemInstruction::CreateAccountWithSeed {
            base,
            seed,
            lamports,
            space,
            owner,
        } => {
            instruction_context.check_number_of_instruction_accounts(2)?;
            let to_address = Address::create(
                transaction_context.get_key_of_account_at_index(
                    instruction_context.get_index_of_instruction_account_in_transaction(1)?,
                )?,
                Some((&base, &seed, &owner)),
                invoke_context,
            )?;
            create_account(
                0,
                1,
                &to_address,
                lamports,
                space,
                &owner,
                &signers,
                invoke_context,
                transaction_context,
                instruction_context,
            )
        }
        SystemInstruction::Assign { owner } => {
            instruction_context.check_number_of_instruction_accounts(1)?;
            let mut account =
                instruction_context.try_borrow_instruction_account(transaction_context, 0)?;
            let address = Address::create(
                transaction_context.get_key_of_account_at_index(
                    instruction_context.get_index_of_instruction_account_in_transaction(0)?,
                )?,
                None,
                invoke_context,
            )?;
            assign(&mut account, &address, &owner, &signers, invoke_context)
        }
        SystemInstruction::Transfer { lamports } => {
            instruction_context.check_number_of_instruction_accounts(2)?;
            transfer(
                0,
                1,
                lamports,
                invoke_context,
                transaction_context,
                instruction_context,
            )
        }
        SystemInstruction::TransferWithSeed {
            lamports,
            from_seed,
            from_owner,
        } => {
            instruction_context.check_number_of_instruction_accounts(3)?;
            transfer_with_seed(
                0,
                1,
                &from_seed,
                &from_owner,
                2,
                lamports,
                invoke_context,
                transaction_context,
                instruction_context,
            )
        }
        SystemInstruction::AdvanceNonceAccount => {
            instruction_context.check_number_of_instruction_accounts(1)?;
            let mut me =
                instruction_context.try_borrow_instruction_account(transaction_context, 0)?;
            #[allow(deprecated)]
            let recent_blockhashes = get_sysvar_with_account_check::recent_blockhashes(
                invoke_context,
                instruction_context,
                1,
            )?;
            if recent_blockhashes.is_empty() {
                ic_msg!(
                    invoke_context,
                    "Advance nonce account: recent blockhash list is empty",
                );
                return Err(SystemError::NonceNoRecentBlockhashes.into());
            }
            advance_nonce_account(&mut me, &signers, invoke_context)
        }
        SystemInstruction::WithdrawNonceAccount(lamports) => {
            instruction_context.check_number_of_instruction_accounts(2)?;
            #[allow(deprecated)]
            let _recent_blockhashes = get_sysvar_with_account_check::recent_blockhashes(
                invoke_context,
                instruction_context,
                2,
            )?;
            let rent = get_sysvar_with_account_check::rent(invoke_context, instruction_context, 3)?;
            withdraw_nonce_account(
                0,
                lamports,
                1,
                &rent,
                &signers,
                invoke_context,
                transaction_context,
                instruction_context,
            )
        }
        SystemInstruction::InitializeNonceAccount(authorized) => {
            instruction_context.check_number_of_instruction_accounts(1)?;
            let mut me =
                instruction_context.try_borrow_instruction_account(transaction_context, 0)?;
            #[allow(deprecated)]
            let recent_blockhashes = get_sysvar_with_account_check::recent_blockhashes(
                invoke_context,
                instruction_context,
                1,
            )?;
            if recent_blockhashes.is_empty() {
                ic_msg!(
                    invoke_context,
                    "Initialize nonce account: recent blockhash list is empty",
                );
                return Err(SystemError::NonceNoRecentBlockhashes.into());
            }
            let rent = get_sysvar_with_account_check::rent(invoke_context, instruction_context, 2)?;
            initialize_nonce_account(&mut me, &authorized, &rent, invoke_context)
        }
        SystemInstruction::AuthorizeNonceAccount(nonce_authority) => {
            instruction_context.check_number_of_instruction_accounts(1)?;
            let mut me =
                instruction_context.try_borrow_instruction_account(transaction_context, 0)?;
            authorize_nonce_account(&mut me, &nonce_authority, &signers, invoke_context)
        }
        SystemInstruction::UpgradeNonceAccount => {
            instruction_context.check_number_of_instruction_accounts(1)?;
            let mut nonce_account =
                instruction_context.try_borrow_instruction_account(transaction_context, 0)?;
            if !system_program::check_id(nonce_account.get_owner()) {
                return Err(InstructionError::InvalidAccountOwner);
            }
            if !nonce_account.is_writable() {
                return Err(InstructionError::InvalidArgument);
            }
            let nonce_versions: nonce::state::Versions = nonce_account.get_state()?;
            match nonce_versions.upgrade() {
                None => Err(InstructionError::InvalidArgument),
                Some(nonce_versions) => nonce_account.set_state(&nonce_versions),
            }
        }
        SystemInstruction::Allocate { space } => {
            instruction_context.check_number_of_instruction_accounts(1)?;
            let mut account =
                instruction_context.try_borrow_instruction_account(transaction_context, 0)?;
            let address = Address::create(
                transaction_context.get_key_of_account_at_index(
                    instruction_context.get_index_of_instruction_account_in_transaction(0)?,
                )?,
                None,
                invoke_context,
            )?;
            allocate(&mut account, &address, space, &signers, invoke_context)
        }
        SystemInstruction::AllocateWithSeed {
            base,
            seed,
            space,
            owner,
        } => {
            instruction_context.check_number_of_instruction_accounts(1)?;
            let mut account =
                instruction_context.try_borrow_instruction_account(transaction_context, 0)?;
            let address = Address::create(
                transaction_context.get_key_of_account_at_index(
                    instruction_context.get_index_of_instruction_account_in_transaction(0)?,
                )?,
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
            instruction_context.check_number_of_instruction_accounts(1)?;
            let mut account =
                instruction_context.try_borrow_instruction_account(transaction_context, 0)?;
            let address = Address::create(
                transaction_context.get_key_of_account_at_index(
                    instruction_context.get_index_of_instruction_account_in_transaction(0)?,
                )?,
                Some((&base, &seed, &owner)),
                invoke_context,
            )?;
            assign(&mut account, &address, &owner, &signers, invoke_context)
        }
    }
});

#[cfg(test)]
mod tests {
    #[allow(deprecated)]
    use solana_sdk::{
        account::{self, Account, AccountSharedData, ReadableAccount},
        fee_calculator::FeeCalculator,
        hash::{hash, Hash},
        instruction::{AccountMeta, Instruction, InstructionError},
        nonce::{
            self,
            state::{
                Data as NonceData, DurableNonce, State as NonceState, Versions as NonceVersions,
            },
        },
        nonce_account, recent_blockhashes_account, system_instruction, system_program,
        sysvar::{self, recent_blockhashes::IterItem, rent::Rent},
    };
    use {
        super::*,
        crate::{get_system_account_kind, SystemAccountKind},
        bincode::serialize,
        solana_program_runtime::{
            invoke_context::mock_process_instruction, with_mock_invoke_context,
        },
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
    ) -> Vec<AccountSharedData> {
        mock_process_instruction(
            &system_program::id(),
            Vec::new(),
            instruction_data,
            transaction_accounts,
            instruction_accounts,
            expected_result,
            Entrypoint::vm,
            |_invoke_context| {},
            |_invoke_context| {},
        )
    }

    fn create_default_account() -> AccountSharedData {
        AccountSharedData::new(0, 0, &Pubkey::new_unique())
    }
    fn create_default_recent_blockhashes_account() -> AccountSharedData {
        #[allow(deprecated)]
        recent_blockhashes_account::create_account_with_data_for_test(
            vec![IterItem(0u64, &Hash::default(), 0); sysvar::recent_blockhashes::MAX_ENTRIES],
        )
    }
    fn create_default_rent_account() -> AccountSharedData {
        account::create_account_shared_data_for_test(&Rent::free())
    }

    #[test]
    fn test_create_account() {
        let new_owner = Pubkey::from([9; 32]);
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
                    is_writable: true,
                },
                AccountMeta {
                    pubkey: to,
                    is_signer: true,
                    is_writable: true,
                },
            ],
            Ok(()),
        );
        assert_eq!(accounts[0].lamports(), 50);
        assert_eq!(accounts[1].lamports(), 50);
        assert_eq!(accounts[1].owner(), &new_owner);
        assert_eq!(accounts[1].data(), &[0, 0]);
    }

    #[test]
    fn test_create_account_with_seed() {
        let new_owner = Pubkey::from([9; 32]);
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
                    is_writable: true,
                },
                AccountMeta {
                    pubkey: to,
                    is_signer: true,
                    is_writable: true,
                },
            ],
            Ok(()),
        );
        assert_eq!(accounts[0].lamports(), 50);
        assert_eq!(accounts[1].lamports(), 50);
        assert_eq!(accounts[1].owner(), &new_owner);
        assert_eq!(accounts[1].data(), &[0, 0]);
    }

    #[test]
    fn test_create_account_with_seed_separate_base_account() {
        let new_owner = Pubkey::from([9; 32]);
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
                    is_writable: true,
                },
                AccountMeta {
                    pubkey: to,
                    is_signer: false,
                    is_writable: true,
                },
                AccountMeta {
                    pubkey: base,
                    is_signer: true,
                    is_writable: false,
                },
            ],
            Ok(()),
        );
        assert_eq!(accounts[0].lamports(), 50);
        assert_eq!(accounts[1].lamports(), 50);
        assert_eq!(accounts[1].owner(), &new_owner);
        assert_eq!(accounts[1].data(), &[0, 0]);
    }

    #[test]
    fn test_address_create_with_seed_mismatch() {
        with_mock_invoke_context!(invoke_context, transaction_context, Vec::new());
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
        let new_owner = Pubkey::from([9; 32]);
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
        );
        assert_eq!(accounts[0].lamports(), 100);
        assert_eq!(accounts[1], AccountSharedData::default());
    }

    #[test]
    fn test_create_with_zero_lamports() {
        // create account with zero lamports transferred
        let new_owner = Pubkey::from([9; 32]);
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
                    is_writable: true,
                },
                AccountMeta {
                    pubkey: to,
                    is_signer: true,
                    is_writable: true,
                },
            ],
            Ok(()),
        );
        assert_eq!(accounts[0].lamports(), 100);
        assert_eq!(accounts[1].lamports(), 0);
        assert_eq!(*accounts[1].owner(), new_owner);
        assert_eq!(accounts[1].data(), &[0, 0]);
    }

    #[test]
    fn test_create_negative_lamports() {
        // Attempt to create account with more lamports than from_account has
        let new_owner = Pubkey::from([9; 32]);
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
                    is_writable: true,
                },
                AccountMeta {
                    pubkey: to,
                    is_signer: true,
                    is_writable: true,
                },
            ],
            Err(SystemError::ResultWithNegativeLamports.into()),
        );
    }

    #[test]
    fn test_request_more_than_allowed_data_length() {
        let from = Pubkey::new_unique();
        let from_account = AccountSharedData::new(100, 0, &system_program::id());
        let to = Pubkey::new_unique();
        let to_account = AccountSharedData::new(0, 0, &Pubkey::default());
        let instruction_accounts = vec![
            AccountMeta {
                pubkey: from,
                is_signer: true,
                is_writable: true,
            },
            AccountMeta {
                pubkey: to,
                is_signer: true,
                is_writable: true,
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
        );
        assert_eq!(accounts[1].lamports(), 50);
        assert_eq!(accounts[1].data().len() as u64, MAX_PERMITTED_DATA_LENGTH);
    }

    #[test]
    fn test_create_already_in_use() {
        let new_owner = Pubkey::from([9; 32]);
        let from = Pubkey::new_unique();
        let from_account = AccountSharedData::new(100, 0, &system_program::id());
        let owned_key = Pubkey::new_unique();

        // Attempt to create system account in account already owned by another program
        let original_program_owner = Pubkey::from([5; 32]);
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
        );
        assert_eq!(accounts[0].lamports(), 100);
        assert_eq!(accounts[1], unchanged_account);
    }

    #[test]
    fn test_create_unsigned() {
        // Attempt to create an account without signing the transfer
        let new_owner = Pubkey::from([9; 32]);
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
                    is_writable: true,
                },
                AccountMeta {
                    pubkey: to,
                    is_signer: true,
                    is_writable: true,
                },
            ],
            Ok(()),
        );
    }

    #[test]
    fn test_create_data_populated() {
        // Attempt to create system account in account with populated data
        let new_owner = Pubkey::from([9; 32]);
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
        );
    }

    #[test]
    fn test_create_from_account_is_nonce_fail() {
        let nonce = Pubkey::new_unique();
        let nonce_account = AccountSharedData::new_data(
            42,
            &nonce::state::Versions::new(nonce::State::Initialized(nonce::state::Data::default())),
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
                    is_writable: true,
                },
            ],
            Err(InstructionError::InvalidArgument),
        );
    }

    #[test]
    fn test_assign() {
        let new_owner = Pubkey::from([9; 32]);
        let pubkey = Pubkey::new_unique();
        let account = AccountSharedData::new(100, 0, &system_program::id());

        // owner does not change, no signature needed
        process_instruction(
            &bincode::serialize(&SystemInstruction::Assign {
                owner: system_program::id(),
            })
            .unwrap(),
            vec![(pubkey, account.clone())],
            vec![AccountMeta {
                pubkey,
                is_signer: false,
                is_writable: true,
            }],
            Ok(()),
        );

        // owner does change, signature needed
        process_instruction(
            &bincode::serialize(&SystemInstruction::Assign { owner: new_owner }).unwrap(),
            vec![(pubkey, account.clone())],
            vec![AccountMeta {
                pubkey,
                is_signer: false,
                is_writable: true,
            }],
            Err(InstructionError::MissingRequiredSignature),
        );

        process_instruction(
            &bincode::serialize(&SystemInstruction::Assign { owner: new_owner }).unwrap(),
            vec![(pubkey, account.clone())],
            vec![AccountMeta {
                pubkey,
                is_signer: true,
                is_writable: true,
            }],
            Ok(()),
        );

        // assign to sysvar instead of system_program
        process_instruction(
            &bincode::serialize(&SystemInstruction::Assign {
                owner: sysvar::id(),
            })
            .unwrap(),
            vec![(pubkey, account)],
            vec![AccountMeta {
                pubkey,
                is_signer: true,
                is_writable: true,
            }],
            Ok(()),
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
        );
    }

    #[test]
    fn test_transfer_lamports() {
        let from = Pubkey::new_unique();
        let from_account = AccountSharedData::new(100, 0, &system_program::id());
        let to = Pubkey::from([3; 32]);
        let to_account = AccountSharedData::new(1, 0, &to); // account owner should not matter
        let transaction_accounts = vec![(from, from_account), (to, to_account)];
        let instruction_accounts = vec![
            AccountMeta {
                pubkey: from,
                is_signer: true,
                is_writable: true,
            },
            AccountMeta {
                pubkey: to,
                is_signer: false,
                is_writable: true,
            },
        ];

        // Success case
        let accounts = process_instruction(
            &bincode::serialize(&SystemInstruction::Transfer { lamports: 50 }).unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Ok(()),
        );
        assert_eq!(accounts[0].lamports(), 50);
        assert_eq!(accounts[1].lamports(), 51);

        // Attempt to move more lamports than from_account has
        let accounts = process_instruction(
            &bincode::serialize(&SystemInstruction::Transfer { lamports: 101 }).unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Err(SystemError::ResultWithNegativeLamports.into()),
        );
        assert_eq!(accounts[0].lamports(), 100);
        assert_eq!(accounts[1].lamports(), 1);

        // test signed transfer of zero
        let accounts = process_instruction(
            &bincode::serialize(&SystemInstruction::Transfer { lamports: 0 }).unwrap(),
            transaction_accounts.clone(),
            instruction_accounts,
            Ok(()),
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
                    is_writable: true,
                },
                AccountMeta {
                    pubkey: to,
                    is_signer: false,
                    is_writable: true,
                },
            ],
            Err(InstructionError::MissingRequiredSignature),
        );
        assert_eq!(accounts[0].lamports(), 100);
        assert_eq!(accounts[1].lamports(), 1);
    }

    #[test]
    fn test_transfer_with_seed() {
        let base = Pubkey::new_unique();
        let base_account = AccountSharedData::new(100, 0, &Pubkey::from([2; 32])); // account owner should not matter
        let from_seed = "42".to_string();
        let from_owner = system_program::id();
        let from = Pubkey::create_with_seed(&base, from_seed.as_str(), &from_owner).unwrap();
        let from_account = AccountSharedData::new(100, 0, &system_program::id());
        let to = Pubkey::from([3; 32]);
        let to_account = AccountSharedData::new(1, 0, &to); // account owner should not matter
        let transaction_accounts =
            vec![(from, from_account), (base, base_account), (to, to_account)];
        let instruction_accounts = vec![
            AccountMeta {
                pubkey: from,
                is_signer: true,
                is_writable: true,
            },
            AccountMeta {
                pubkey: base,
                is_signer: true,
                is_writable: false,
            },
            AccountMeta {
                pubkey: to,
                is_signer: false,
                is_writable: true,
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
        );
        assert_eq!(accounts[0].lamports(), 100);
        assert_eq!(accounts[2].lamports(), 1);
    }

    #[test]
    fn test_transfer_lamports_from_nonce_account_fail() {
        let from = Pubkey::new_unique();
        let from_account = AccountSharedData::new_data(
            100,
            &nonce::state::Versions::new(nonce::State::Initialized(nonce::state::Data {
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
        let to = Pubkey::from([3; 32]);
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
        );
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
        );
        let blockhash = hash(&serialize(&0).unwrap());
        #[allow(deprecated)]
        let new_recent_blockhashes_account =
            solana_sdk::recent_blockhashes_account::create_account_with_data_for_test(
                vec![IterItem(0u64, &blockhash, 0); sysvar::recent_blockhashes::MAX_ENTRIES],
            );
        mock_process_instruction(
            &system_program::id(),
            Vec::new(),
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
            Entrypoint::vm,
            |invoke_context: &mut InvokeContext| {
                invoke_context.blockhash = hash(&serialize(&0).unwrap());
            },
            |_invoke_context| {},
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
        );
    }

    #[test]
    fn test_process_initialize_ix_no_keyed_accs_fail() {
        process_instruction(
            &serialize(&SystemInstruction::InitializeNonceAccount(Pubkey::default())).unwrap(),
            Vec::new(),
            Vec::new(),
            Err(InstructionError::NotEnoughAccountKeys),
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
            &nonce::state::Versions::new(nonce::State::Initialized(nonce::state::Data::default())),
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
            &nonce::state::Versions::new(nonce::State::Initialized(nonce::state::Data::default())),
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
            solana_sdk::recent_blockhashes_account::create_account_with_data_for_test(vec![]);
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
            Err(SystemError::NonceNoRecentBlockhashes.into()),
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
        );
        #[allow(deprecated)]
        let new_recent_blockhashes_account =
            solana_sdk::recent_blockhashes_account::create_account_with_data_for_test(vec![]);
        mock_process_instruction(
            &system_program::id(),
            Vec::new(),
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
            Err(SystemError::NonceNoRecentBlockhashes.into()),
            Entrypoint::vm,
            |invoke_context: &mut InvokeContext| {
                invoke_context.blockhash = hash(&serialize(&0).unwrap());
            },
            |_invoke_context| {},
        );
    }

    #[test]
    fn test_nonce_account_upgrade_check_owner() {
        let nonce_address = Pubkey::new_unique();
        let versions = NonceVersions::Legacy(Box::new(NonceState::Uninitialized));
        let nonce_account = AccountSharedData::new_data(
            1_000_000,             // lamports
            &versions,             // state
            &Pubkey::new_unique(), // owner
        )
        .unwrap();
        let accounts = process_instruction(
            &serialize(&SystemInstruction::UpgradeNonceAccount).unwrap(),
            vec![(nonce_address, nonce_account.clone())],
            vec![AccountMeta {
                pubkey: nonce_address,
                is_signer: false,
                is_writable: true,
            }],
            Err(InstructionError::InvalidAccountOwner),
        );
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0], nonce_account);
    }

    fn new_nonce_account(versions: NonceVersions) -> AccountSharedData {
        let nonce_account = AccountSharedData::new_data(
            1_000_000,             // lamports
            &versions,             // state
            &system_program::id(), // owner
        )
        .unwrap();
        assert_eq!(
            nonce_account.deserialize_data::<NonceVersions>().unwrap(),
            versions
        );
        nonce_account
    }

    #[test]
    fn test_nonce_account_upgrade() {
        let nonce_address = Pubkey::new_unique();
        let versions = NonceVersions::Legacy(Box::new(NonceState::Uninitialized));
        let nonce_account = new_nonce_account(versions);
        let accounts = process_instruction(
            &serialize(&SystemInstruction::UpgradeNonceAccount).unwrap(),
            vec![(nonce_address, nonce_account.clone())],
            vec![AccountMeta {
                pubkey: nonce_address,
                is_signer: false,
                is_writable: true,
            }],
            Err(InstructionError::InvalidArgument),
        );
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0], nonce_account);
        let versions = NonceVersions::Current(Box::new(NonceState::Uninitialized));
        let nonce_account = new_nonce_account(versions);
        let accounts = process_instruction(
            &serialize(&SystemInstruction::UpgradeNonceAccount).unwrap(),
            vec![(nonce_address, nonce_account.clone())],
            vec![AccountMeta {
                pubkey: nonce_address,
                is_signer: false,
                is_writable: true,
            }],
            Err(InstructionError::InvalidArgument),
        );
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0], nonce_account);
        let blockhash = Hash::from([171; 32]);
        let durable_nonce = DurableNonce::from_blockhash(&blockhash);
        let data = NonceData {
            authority: Pubkey::new_unique(),
            durable_nonce,
            fee_calculator: FeeCalculator {
                lamports_per_signature: 2718,
            },
        };
        let versions = NonceVersions::Legacy(Box::new(NonceState::Initialized(data.clone())));
        let nonce_account = new_nonce_account(versions);
        let accounts = process_instruction(
            &serialize(&SystemInstruction::UpgradeNonceAccount).unwrap(),
            vec![(nonce_address, nonce_account.clone())],
            vec![AccountMeta {
                pubkey: nonce_address,
                is_signer: false,
                is_writable: false, // Should fail!
            }],
            Err(InstructionError::InvalidArgument),
        );
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0], nonce_account);
        let mut accounts = process_instruction(
            &serialize(&SystemInstruction::UpgradeNonceAccount).unwrap(),
            vec![(nonce_address, nonce_account)],
            vec![AccountMeta {
                pubkey: nonce_address,
                is_signer: false,
                is_writable: true,
            }],
            Ok(()),
        );
        assert_eq!(accounts.len(), 1);
        let nonce_account = accounts.remove(0);
        let durable_nonce = DurableNonce::from_blockhash(durable_nonce.as_hash());
        assert_ne!(data.durable_nonce, durable_nonce);
        let data = NonceData {
            durable_nonce,
            ..data
        };
        let upgraded_nonce_account =
            NonceVersions::Current(Box::new(NonceState::Initialized(data)));
        assert_eq!(
            nonce_account.deserialize_data::<NonceVersions>().unwrap(),
            upgraded_nonce_account
        );
        let accounts = process_instruction(
            &serialize(&SystemInstruction::UpgradeNonceAccount).unwrap(),
            vec![(nonce_address, nonce_account)],
            vec![AccountMeta {
                pubkey: nonce_address,
                is_signer: false,
                is_writable: true,
            }],
            Err(InstructionError::InvalidArgument),
        );
        assert_eq!(accounts.len(), 1);
        assert_eq!(
            accounts[0].deserialize_data::<NonceVersions>().unwrap(),
            upgraded_nonce_account
        );
    }
}
