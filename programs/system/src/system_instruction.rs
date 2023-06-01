use {
    solana_program_runtime::{ic_msg, invoke_context::InvokeContext},
    solana_sdk::{
        instruction::{checked_add, InstructionError},
        nonce::{
            self,
            state::{AuthorizeNonceError, DurableNonce, Versions},
            State,
        },
        pubkey::Pubkey,
        system_instruction::SystemError,
        sysvar::rent::Rent,
        transaction_context::{
            BorrowedAccount, IndexOfAccount, InstructionContext, TransactionContext,
        },
    },
    std::collections::HashSet,
};

pub fn advance_nonce_account(
    account: &mut BorrowedAccount,
    signers: &HashSet<Pubkey>,
    invoke_context: &InvokeContext,
) -> Result<(), InstructionError> {
    if !account.is_writable() {
        ic_msg!(
            invoke_context,
            "Advance nonce account: Account {} must be writeable",
            account.get_key()
        );
        return Err(InstructionError::InvalidArgument);
    }

    let state: Versions = account.get_state()?;
    match state.state() {
        State::Initialized(data) => {
            if !signers.contains(&data.authority) {
                ic_msg!(
                    invoke_context,
                    "Advance nonce account: Account {} must be a signer",
                    data.authority
                );
                return Err(InstructionError::MissingRequiredSignature);
            }
            let next_durable_nonce = DurableNonce::from_blockhash(&invoke_context.blockhash);
            if data.durable_nonce == next_durable_nonce {
                ic_msg!(
                    invoke_context,
                    "Advance nonce account: nonce can only advance once per slot"
                );
                return Err(SystemError::NonceBlockhashNotExpired.into());
            }

            let new_data = nonce::state::Data::new(
                data.authority,
                next_durable_nonce,
                invoke_context.lamports_per_signature,
            );
            account.set_state(&Versions::new(State::Initialized(new_data)))
        }
        State::Uninitialized => {
            ic_msg!(
                invoke_context,
                "Advance nonce account: Account {} state is invalid",
                account.get_key()
            );
            Err(InstructionError::InvalidAccountData)
        }
    }
}

pub fn withdraw_nonce_account(
    from_account_index: IndexOfAccount,
    lamports: u64,
    to_account_index: IndexOfAccount,
    rent: &Rent,
    signers: &HashSet<Pubkey>,
    invoke_context: &InvokeContext,
    transaction_context: &TransactionContext,
    instruction_context: &InstructionContext,
) -> Result<(), InstructionError> {
    let mut from = instruction_context
        .try_borrow_instruction_account(transaction_context, from_account_index)?;
    if !from.is_writable() {
        ic_msg!(
            invoke_context,
            "Withdraw nonce account: Account {} must be writeable",
            from.get_key()
        );
        return Err(InstructionError::InvalidArgument);
    }

    let state: Versions = from.get_state()?;
    let signer = match state.state() {
        State::Uninitialized => {
            if lamports > from.get_lamports() {
                ic_msg!(
                    invoke_context,
                    "Withdraw nonce account: insufficient lamports {}, need {}",
                    from.get_lamports(),
                    lamports,
                );
                return Err(InstructionError::InsufficientFunds);
            }
            *from.get_key()
        }
        State::Initialized(ref data) => {
            if lamports == from.get_lamports() {
                let durable_nonce = DurableNonce::from_blockhash(&invoke_context.blockhash);
                if data.durable_nonce == durable_nonce {
                    ic_msg!(
                        invoke_context,
                        "Withdraw nonce account: nonce can only advance once per slot"
                    );
                    return Err(SystemError::NonceBlockhashNotExpired.into());
                }
                from.set_state(&Versions::new(State::Uninitialized))?;
            } else {
                let min_balance = rent.minimum_balance(from.get_data().len());
                let amount = checked_add(lamports, min_balance)?;
                if amount > from.get_lamports() {
                    ic_msg!(
                        invoke_context,
                        "Withdraw nonce account: insufficient lamports {}, need {}",
                        from.get_lamports(),
                        amount,
                    );
                    return Err(InstructionError::InsufficientFunds);
                }
            }
            data.authority
        }
    };

    if !signers.contains(&signer) {
        ic_msg!(
            invoke_context,
            "Withdraw nonce account: Account {} must sign",
            signer
        );
        return Err(InstructionError::MissingRequiredSignature);
    }

    from.checked_sub_lamports(lamports)?;
    drop(from);
    let mut to = instruction_context
        .try_borrow_instruction_account(transaction_context, to_account_index)?;
    to.checked_add_lamports(lamports)?;

    Ok(())
}

pub fn initialize_nonce_account(
    account: &mut BorrowedAccount,
    nonce_authority: &Pubkey,
    rent: &Rent,
    invoke_context: &InvokeContext,
) -> Result<(), InstructionError> {
    if !account.is_writable() {
        ic_msg!(
            invoke_context,
            "Initialize nonce account: Account {} must be writeable",
            account.get_key()
        );
        return Err(InstructionError::InvalidArgument);
    }

    match account.get_state::<Versions>()?.state() {
        State::Uninitialized => {
            let min_balance = rent.minimum_balance(account.get_data().len());
            if account.get_lamports() < min_balance {
                ic_msg!(
                    invoke_context,
                    "Initialize nonce account: insufficient lamports {}, need {}",
                    account.get_lamports(),
                    min_balance
                );
                return Err(InstructionError::InsufficientFunds);
            }
            let durable_nonce = DurableNonce::from_blockhash(&invoke_context.blockhash);
            let data = nonce::state::Data::new(
                *nonce_authority,
                durable_nonce,
                invoke_context.lamports_per_signature,
            );
            let state = State::Initialized(data);
            account.set_state(&Versions::new(state))
        }
        State::Initialized(_) => {
            ic_msg!(
                invoke_context,
                "Initialize nonce account: Account {} state is invalid",
                account.get_key()
            );
            Err(InstructionError::InvalidAccountData)
        }
    }
}

pub fn authorize_nonce_account(
    account: &mut BorrowedAccount,
    nonce_authority: &Pubkey,
    signers: &HashSet<Pubkey>,
    invoke_context: &InvokeContext,
) -> Result<(), InstructionError> {
    if !account.is_writable() {
        ic_msg!(
            invoke_context,
            "Authorize nonce account: Account {} must be writeable",
            account.get_key()
        );
        return Err(InstructionError::InvalidArgument);
    }
    match account
        .get_state::<Versions>()?
        .authorize(signers, *nonce_authority)
    {
        Ok(versions) => account.set_state(&versions),
        Err(AuthorizeNonceError::Uninitialized) => {
            ic_msg!(
                invoke_context,
                "Authorize nonce account: Account {} state is invalid",
                account.get_key()
            );
            Err(InstructionError::InvalidAccountData)
        }
        Err(AuthorizeNonceError::MissingRequiredSignature(account_authority)) => {
            ic_msg!(
                invoke_context,
                "Authorize nonce account: Account {} must sign",
                account_authority
            );
            Err(InstructionError::MissingRequiredSignature)
        }
    }
}

#[cfg(test)]
mod test {
    use {
        super::*,
        assert_matches::assert_matches,
        solana_program_runtime::with_mock_invoke_context,
        solana_sdk::{
            account::AccountSharedData,
            hash::hash,
            nonce::{self, State},
            nonce_account::{create_account, verify_nonce_account},
            system_program,
            transaction_context::InstructionAccount,
        },
    };

    pub const NONCE_ACCOUNT_INDEX: IndexOfAccount = 0;
    pub const WITHDRAW_TO_ACCOUNT_INDEX: IndexOfAccount = 1;

    macro_rules! push_instruction_context {
        ($invoke_context:expr, $transaction_context:ident, $instruction_context:ident, $instruction_accounts:ident) => {
            $invoke_context
                .transaction_context
                .get_next_instruction_context()
                .unwrap()
                .configure(&[2], &$instruction_accounts, &[]);
            $invoke_context.push().unwrap();
            let $transaction_context = &$invoke_context.transaction_context;
            let $instruction_context = $transaction_context
                .get_current_instruction_context()
                .unwrap();
        };
    }

    macro_rules! prepare_mockup {
        ($invoke_context:ident, $instruction_accounts:ident, $rent:ident) => {
            let $rent = Rent {
                lamports_per_byte_year: 42,
                ..Rent::default()
            };
            let from_lamports = $rent.minimum_balance(State::size()) + 42;
            let transaction_accounts = vec![
                (
                    Pubkey::new_unique(),
                    create_account(from_lamports).into_inner(),
                ),
                (Pubkey::new_unique(), create_account(42).into_inner()),
                (system_program::id(), AccountSharedData::default()),
            ];
            let $instruction_accounts = vec![
                InstructionAccount {
                    index_in_transaction: 0,
                    index_in_caller: 0,
                    index_in_callee: 0,
                    is_signer: true,
                    is_writable: true,
                },
                InstructionAccount {
                    index_in_transaction: 1,
                    index_in_caller: 1,
                    index_in_callee: 1,
                    is_signer: false,
                    is_writable: true,
                },
            ];
            with_mock_invoke_context!($invoke_context, transaction_context, transaction_accounts);
        };
    }

    macro_rules! set_invoke_context_blockhash {
        ($invoke_context:expr, $seed:expr) => {
            $invoke_context.blockhash = hash(&bincode::serialize(&$seed).unwrap());
            $invoke_context.lamports_per_signature = ($seed as u64).saturating_mul(100);
        };
    }

    #[test]
    fn default_is_uninitialized() {
        assert_eq!(State::default(), State::Uninitialized)
    }

    #[test]
    fn expected_behavior() {
        prepare_mockup!(invoke_context, instruction_accounts, rent);
        push_instruction_context!(
            invoke_context,
            transaction_context,
            instruction_context,
            instruction_accounts
        );
        let mut nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        let data = nonce::state::Data {
            authority: *nonce_account.get_key(),
            ..nonce::state::Data::default()
        };
        let mut signers = HashSet::new();
        signers.insert(*nonce_account.get_key());
        let versions = nonce_account.get_state::<Versions>().unwrap();
        // New is in Uninitialzed state
        assert_eq!(versions.state(), &State::Uninitialized);
        set_invoke_context_blockhash!(invoke_context, 95);
        let authorized = *nonce_account.get_key();
        initialize_nonce_account(&mut nonce_account, &authorized, &rent, &invoke_context).unwrap();
        let versions = nonce_account.get_state::<Versions>().unwrap();
        let data = nonce::state::Data::new(
            data.authority,
            DurableNonce::from_blockhash(&invoke_context.blockhash),
            invoke_context.lamports_per_signature,
        );
        // First nonce instruction drives state from Uninitialized to Initialized
        assert_eq!(versions.state(), &State::Initialized(data.clone()));
        set_invoke_context_blockhash!(invoke_context, 63);
        advance_nonce_account(&mut nonce_account, &signers, &invoke_context).unwrap();
        let versions = nonce_account.get_state::<Versions>().unwrap();
        let data = nonce::state::Data::new(
            data.authority,
            DurableNonce::from_blockhash(&invoke_context.blockhash),
            invoke_context.lamports_per_signature,
        );
        // Second nonce instruction consumes and replaces stored nonce
        assert_eq!(versions.state(), &State::Initialized(data.clone()));
        set_invoke_context_blockhash!(invoke_context, 31);
        advance_nonce_account(&mut nonce_account, &signers, &invoke_context).unwrap();
        let versions = nonce_account.get_state::<Versions>().unwrap();
        let data = nonce::state::Data::new(
            data.authority,
            DurableNonce::from_blockhash(&invoke_context.blockhash),
            invoke_context.lamports_per_signature,
        );
        // Third nonce instruction for fun and profit
        assert_eq!(versions.state(), &State::Initialized(data));

        set_invoke_context_blockhash!(invoke_context, 0);
        let to_account = instruction_context
            .try_borrow_instruction_account(transaction_context, WITHDRAW_TO_ACCOUNT_INDEX)
            .unwrap();
        let withdraw_lamports = nonce_account.get_lamports();
        let expect_nonce_lamports = nonce_account.get_lamports() - withdraw_lamports;
        let expect_to_lamports = to_account.get_lamports() + withdraw_lamports;
        drop(nonce_account);
        drop(to_account);
        withdraw_nonce_account(
            NONCE_ACCOUNT_INDEX,
            withdraw_lamports,
            WITHDRAW_TO_ACCOUNT_INDEX,
            &rent,
            &signers,
            &invoke_context,
            transaction_context,
            instruction_context,
        )
        .unwrap();
        let nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        let to_account = instruction_context
            .try_borrow_instruction_account(transaction_context, WITHDRAW_TO_ACCOUNT_INDEX)
            .unwrap();
        // Empties Account balance
        assert_eq!(nonce_account.get_lamports(), expect_nonce_lamports);
        // Account balance goes to `to`
        assert_eq!(to_account.get_lamports(), expect_to_lamports);
        let versions = nonce_account.get_state::<Versions>().unwrap();
        // Empty balance deinitializes data
        assert_eq!(versions.state(), &State::Uninitialized);
    }

    #[test]
    fn nonce_inx_initialized_account_not_signer_fail() {
        prepare_mockup!(invoke_context, instruction_accounts, rent);
        push_instruction_context!(
            invoke_context,
            transaction_context,
            instruction_context,
            instruction_accounts
        );
        let mut nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        set_invoke_context_blockhash!(invoke_context, 31);
        let authority = *nonce_account.get_key();
        initialize_nonce_account(&mut nonce_account, &authority, &rent, &invoke_context).unwrap();
        let versions = nonce_account.get_state::<Versions>().unwrap();
        let data = nonce::state::Data::new(
            authority,
            DurableNonce::from_blockhash(&invoke_context.blockhash),
            invoke_context.lamports_per_signature,
        );
        assert_eq!(versions.state(), &State::Initialized(data));
        // Nonce account did not sign
        let signers = HashSet::new();
        set_invoke_context_blockhash!(invoke_context, 0);
        let result = advance_nonce_account(&mut nonce_account, &signers, &invoke_context);
        assert_eq!(result, Err(InstructionError::MissingRequiredSignature));
    }

    #[test]
    fn nonce_inx_too_early_fail() {
        prepare_mockup!(invoke_context, instruction_accounts, rent);
        push_instruction_context!(
            invoke_context,
            transaction_context,
            instruction_context,
            instruction_accounts
        );
        let mut nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        let mut signers = HashSet::new();
        signers.insert(*nonce_account.get_key());
        set_invoke_context_blockhash!(invoke_context, 63);
        let authorized = *nonce_account.get_key();
        initialize_nonce_account(&mut nonce_account, &authorized, &rent, &invoke_context).unwrap();
        let result = advance_nonce_account(&mut nonce_account, &signers, &invoke_context);
        assert_eq!(result, Err(SystemError::NonceBlockhashNotExpired.into()));
    }

    #[test]
    fn nonce_inx_uninitialized_account_fail() {
        prepare_mockup!(invoke_context, instruction_accounts, rent);
        push_instruction_context!(
            invoke_context,
            transaction_context,
            instruction_context,
            instruction_accounts
        );
        let mut nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        let mut signers = HashSet::new();
        signers.insert(*nonce_account.get_key());
        set_invoke_context_blockhash!(invoke_context, 63);
        let result = advance_nonce_account(&mut nonce_account, &signers, &invoke_context);
        assert_eq!(result, Err(InstructionError::InvalidAccountData));
    }

    #[test]
    fn nonce_inx_independent_nonce_authority_ok() {
        prepare_mockup!(invoke_context, instruction_accounts, rent);
        push_instruction_context!(
            invoke_context,
            transaction_context,
            instruction_context,
            instruction_accounts
        );
        let mut nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        let nonce_authority = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX + 1)
            .unwrap();
        let mut signers = HashSet::new();
        signers.insert(*nonce_account.get_key());
        set_invoke_context_blockhash!(invoke_context, 63);
        let authorized = *nonce_authority.get_key();
        initialize_nonce_account(&mut nonce_account, &authorized, &rent, &invoke_context).unwrap();
        let mut signers = HashSet::new();
        signers.insert(authorized);
        set_invoke_context_blockhash!(invoke_context, 31);
        let result = advance_nonce_account(&mut nonce_account, &signers, &invoke_context);
        assert_eq!(result, Ok(()));
    }

    #[test]
    fn nonce_inx_no_nonce_authority_sig_fail() {
        prepare_mockup!(invoke_context, instruction_accounts, rent);
        push_instruction_context!(
            invoke_context,
            transaction_context,
            instruction_context,
            instruction_accounts
        );
        let mut nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        let nonce_authority = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX + 1)
            .unwrap();
        let mut signers = HashSet::new();
        signers.insert(*nonce_account.get_key());
        set_invoke_context_blockhash!(invoke_context, 63);
        let authorized = *nonce_authority.get_key();
        initialize_nonce_account(&mut nonce_account, &authorized, &rent, &invoke_context).unwrap();
        let result = advance_nonce_account(&mut nonce_account, &signers, &invoke_context);
        assert_eq!(result, Err(InstructionError::MissingRequiredSignature));
    }

    #[test]
    fn withdraw_inx_unintialized_acc_ok() {
        prepare_mockup!(invoke_context, instruction_accounts, rent);
        push_instruction_context!(
            invoke_context,
            transaction_context,
            instruction_context,
            instruction_accounts
        );
        let nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        let to_account = instruction_context
            .try_borrow_instruction_account(transaction_context, WITHDRAW_TO_ACCOUNT_INDEX)
            .unwrap();
        let versions = nonce_account.get_state::<Versions>().unwrap();
        assert_eq!(versions.state(), &State::Uninitialized);
        let mut signers = HashSet::new();
        signers.insert(*nonce_account.get_key());
        set_invoke_context_blockhash!(invoke_context, 0);
        let withdraw_lamports = nonce_account.get_lamports();
        let expect_from_lamports = nonce_account.get_lamports() - withdraw_lamports;
        let expect_to_lamports = to_account.get_lamports() + withdraw_lamports;
        drop(nonce_account);
        drop(to_account);
        withdraw_nonce_account(
            NONCE_ACCOUNT_INDEX,
            withdraw_lamports,
            WITHDRAW_TO_ACCOUNT_INDEX,
            &rent,
            &signers,
            &invoke_context,
            transaction_context,
            instruction_context,
        )
        .unwrap();
        let nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        let to_account = instruction_context
            .try_borrow_instruction_account(transaction_context, WITHDRAW_TO_ACCOUNT_INDEX)
            .unwrap();
        let versions = nonce_account.get_state::<Versions>().unwrap();
        assert_eq!(versions.state(), &State::Uninitialized);
        assert_eq!(nonce_account.get_lamports(), expect_from_lamports);
        assert_eq!(to_account.get_lamports(), expect_to_lamports);
    }

    #[test]
    fn withdraw_inx_unintialized_acc_unsigned_fail() {
        prepare_mockup!(invoke_context, instruction_accounts, rent);
        push_instruction_context!(
            invoke_context,
            transaction_context,
            instruction_context,
            instruction_accounts
        );
        let nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        let to_account = instruction_context
            .try_borrow_instruction_account(transaction_context, WITHDRAW_TO_ACCOUNT_INDEX)
            .unwrap();
        let versions = nonce_account.get_state::<Versions>().unwrap();
        assert_eq!(versions.state(), &State::Uninitialized);
        let signers = HashSet::new();
        set_invoke_context_blockhash!(invoke_context, 0);
        let withdraw_lamports = nonce_account.get_lamports();
        drop(nonce_account);
        drop(to_account);
        let result = withdraw_nonce_account(
            NONCE_ACCOUNT_INDEX,
            withdraw_lamports,
            WITHDRAW_TO_ACCOUNT_INDEX,
            &rent,
            &signers,
            &invoke_context,
            transaction_context,
            instruction_context,
        );
        assert_eq!(result, Err(InstructionError::MissingRequiredSignature));
    }

    #[test]
    fn withdraw_inx_unintialized_acc_insuff_funds_fail() {
        prepare_mockup!(invoke_context, instruction_accounts, rent);
        push_instruction_context!(
            invoke_context,
            transaction_context,
            instruction_context,
            instruction_accounts
        );
        let nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        let versions = nonce_account.get_state::<Versions>().unwrap();
        assert_eq!(versions.state(), &State::Uninitialized);
        let mut signers = HashSet::new();
        signers.insert(*nonce_account.get_key());
        set_invoke_context_blockhash!(invoke_context, 0);
        let withdraw_lamports = nonce_account.get_lamports() + 1;
        drop(nonce_account);
        let result = withdraw_nonce_account(
            NONCE_ACCOUNT_INDEX,
            withdraw_lamports,
            WITHDRAW_TO_ACCOUNT_INDEX,
            &rent,
            &signers,
            &invoke_context,
            transaction_context,
            instruction_context,
        );
        assert_eq!(result, Err(InstructionError::InsufficientFunds));
    }

    #[test]
    fn withdraw_inx_uninitialized_acc_two_withdraws_ok() {
        prepare_mockup!(invoke_context, instruction_accounts, rent);
        push_instruction_context!(
            invoke_context,
            transaction_context,
            instruction_context,
            instruction_accounts
        );
        let nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        let to_account = instruction_context
            .try_borrow_instruction_account(transaction_context, WITHDRAW_TO_ACCOUNT_INDEX)
            .unwrap();
        let mut signers = HashSet::new();
        signers.insert(*nonce_account.get_key());
        set_invoke_context_blockhash!(invoke_context, 0);
        let withdraw_lamports = nonce_account.get_lamports() / 2;
        let from_expect_lamports = nonce_account.get_lamports() - withdraw_lamports;
        let to_expect_lamports = to_account.get_lamports() + withdraw_lamports;
        drop(nonce_account);
        drop(to_account);
        withdraw_nonce_account(
            NONCE_ACCOUNT_INDEX,
            withdraw_lamports,
            WITHDRAW_TO_ACCOUNT_INDEX,
            &rent,
            &signers,
            &invoke_context,
            transaction_context,
            instruction_context,
        )
        .unwrap();
        let nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        let to_account = instruction_context
            .try_borrow_instruction_account(transaction_context, WITHDRAW_TO_ACCOUNT_INDEX)
            .unwrap();
        let versions = nonce_account.get_state::<Versions>().unwrap();
        assert_eq!(versions.state(), &State::Uninitialized);
        assert_eq!(nonce_account.get_lamports(), from_expect_lamports);
        assert_eq!(to_account.get_lamports(), to_expect_lamports);
        let withdraw_lamports = nonce_account.get_lamports();
        let from_expect_lamports = nonce_account.get_lamports() - withdraw_lamports;
        let to_expect_lamports = to_account.get_lamports() + withdraw_lamports;
        drop(nonce_account);
        drop(to_account);
        withdraw_nonce_account(
            NONCE_ACCOUNT_INDEX,
            withdraw_lamports,
            WITHDRAW_TO_ACCOUNT_INDEX,
            &rent,
            &signers,
            &invoke_context,
            transaction_context,
            instruction_context,
        )
        .unwrap();
        let nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        let to_account = instruction_context
            .try_borrow_instruction_account(transaction_context, WITHDRAW_TO_ACCOUNT_INDEX)
            .unwrap();
        let versions = nonce_account.get_state::<Versions>().unwrap();
        assert_eq!(versions.state(), &State::Uninitialized);
        assert_eq!(nonce_account.get_lamports(), from_expect_lamports);
        assert_eq!(to_account.get_lamports(), to_expect_lamports);
    }

    #[test]
    fn withdraw_inx_initialized_acc_two_withdraws_ok() {
        prepare_mockup!(invoke_context, instruction_accounts, rent);
        push_instruction_context!(
            invoke_context,
            transaction_context,
            instruction_context,
            instruction_accounts
        );
        let mut nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        let to_account = instruction_context
            .try_borrow_instruction_account(transaction_context, WITHDRAW_TO_ACCOUNT_INDEX)
            .unwrap();
        let mut signers = HashSet::new();
        signers.insert(*nonce_account.get_key());
        set_invoke_context_blockhash!(invoke_context, 31);
        let authority = *nonce_account.get_key();
        initialize_nonce_account(&mut nonce_account, &authority, &rent, &invoke_context).unwrap();
        let versions = nonce_account.get_state::<Versions>().unwrap();
        let data = nonce::state::Data::new(
            authority,
            DurableNonce::from_blockhash(&invoke_context.blockhash),
            invoke_context.lamports_per_signature,
        );
        assert_eq!(versions.state(), &State::Initialized(data.clone()));
        let withdraw_lamports = 42;
        let from_expect_lamports = nonce_account.get_lamports() - withdraw_lamports;
        let to_expect_lamports = to_account.get_lamports() + withdraw_lamports;
        drop(nonce_account);
        drop(to_account);
        withdraw_nonce_account(
            NONCE_ACCOUNT_INDEX,
            withdraw_lamports,
            WITHDRAW_TO_ACCOUNT_INDEX,
            &rent,
            &signers,
            &invoke_context,
            transaction_context,
            instruction_context,
        )
        .unwrap();
        let nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        let to_account = instruction_context
            .try_borrow_instruction_account(transaction_context, WITHDRAW_TO_ACCOUNT_INDEX)
            .unwrap();
        let versions = nonce_account.get_state::<Versions>().unwrap();
        let data = nonce::state::Data::new(
            data.authority,
            DurableNonce::from_blockhash(&invoke_context.blockhash),
            invoke_context.lamports_per_signature,
        );
        assert_eq!(versions.state(), &State::Initialized(data));
        assert_eq!(nonce_account.get_lamports(), from_expect_lamports);
        assert_eq!(to_account.get_lamports(), to_expect_lamports);
        set_invoke_context_blockhash!(invoke_context, 0);
        let withdraw_lamports = nonce_account.get_lamports();
        let from_expect_lamports = nonce_account.get_lamports() - withdraw_lamports;
        let to_expect_lamports = to_account.get_lamports() + withdraw_lamports;
        drop(nonce_account);
        drop(to_account);
        withdraw_nonce_account(
            NONCE_ACCOUNT_INDEX,
            withdraw_lamports,
            WITHDRAW_TO_ACCOUNT_INDEX,
            &rent,
            &signers,
            &invoke_context,
            transaction_context,
            instruction_context,
        )
        .unwrap();
        let nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        let to_account = instruction_context
            .try_borrow_instruction_account(transaction_context, WITHDRAW_TO_ACCOUNT_INDEX)
            .unwrap();
        let versions = nonce_account.get_state::<Versions>().unwrap();
        assert_eq!(versions.state(), &State::Uninitialized);
        assert_eq!(nonce_account.get_lamports(), from_expect_lamports);
        assert_eq!(to_account.get_lamports(), to_expect_lamports);
    }

    #[test]
    fn withdraw_inx_initialized_acc_nonce_too_early_fail() {
        prepare_mockup!(invoke_context, instruction_accounts, rent);
        push_instruction_context!(
            invoke_context,
            transaction_context,
            instruction_context,
            instruction_accounts
        );
        let mut nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        let to_account = instruction_context
            .try_borrow_instruction_account(transaction_context, WITHDRAW_TO_ACCOUNT_INDEX)
            .unwrap();
        set_invoke_context_blockhash!(invoke_context, 0);
        let authorized = *nonce_account.get_key();
        initialize_nonce_account(&mut nonce_account, &authorized, &rent, &invoke_context).unwrap();
        let mut signers = HashSet::new();
        signers.insert(*nonce_account.get_key());
        let withdraw_lamports = nonce_account.get_lamports();
        drop(nonce_account);
        drop(to_account);
        let result = withdraw_nonce_account(
            NONCE_ACCOUNT_INDEX,
            withdraw_lamports,
            WITHDRAW_TO_ACCOUNT_INDEX,
            &rent,
            &signers,
            &invoke_context,
            transaction_context,
            instruction_context,
        );
        assert_eq!(result, Err(SystemError::NonceBlockhashNotExpired.into()));
    }

    #[test]
    fn withdraw_inx_initialized_acc_insuff_funds_fail() {
        prepare_mockup!(invoke_context, instruction_accounts, rent);
        push_instruction_context!(
            invoke_context,
            transaction_context,
            instruction_context,
            instruction_accounts
        );
        let mut nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        set_invoke_context_blockhash!(invoke_context, 95);
        let authorized = *nonce_account.get_key();
        initialize_nonce_account(&mut nonce_account, &authorized, &rent, &invoke_context).unwrap();
        set_invoke_context_blockhash!(invoke_context, 63);
        let mut signers = HashSet::new();
        signers.insert(*nonce_account.get_key());
        let withdraw_lamports = nonce_account.get_lamports() + 1;
        drop(nonce_account);
        let result = withdraw_nonce_account(
            NONCE_ACCOUNT_INDEX,
            withdraw_lamports,
            WITHDRAW_TO_ACCOUNT_INDEX,
            &rent,
            &signers,
            &invoke_context,
            transaction_context,
            instruction_context,
        );
        assert_eq!(result, Err(InstructionError::InsufficientFunds));
    }

    #[test]
    fn withdraw_inx_initialized_acc_insuff_rent_fail() {
        prepare_mockup!(invoke_context, instruction_accounts, rent);
        push_instruction_context!(
            invoke_context,
            transaction_context,
            instruction_context,
            instruction_accounts
        );
        let mut nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        set_invoke_context_blockhash!(invoke_context, 95);
        let authorized = *nonce_account.get_key();
        initialize_nonce_account(&mut nonce_account, &authorized, &rent, &invoke_context).unwrap();
        set_invoke_context_blockhash!(invoke_context, 63);
        let mut signers = HashSet::new();
        signers.insert(*nonce_account.get_key());
        let withdraw_lamports = 42 + 1;
        drop(nonce_account);
        let result = withdraw_nonce_account(
            NONCE_ACCOUNT_INDEX,
            withdraw_lamports,
            WITHDRAW_TO_ACCOUNT_INDEX,
            &rent,
            &signers,
            &invoke_context,
            transaction_context,
            instruction_context,
        );
        assert_eq!(result, Err(InstructionError::InsufficientFunds));
    }

    #[test]
    fn withdraw_inx_overflow() {
        prepare_mockup!(invoke_context, instruction_accounts, rent);
        push_instruction_context!(
            invoke_context,
            transaction_context,
            instruction_context,
            instruction_accounts
        );
        let mut nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        set_invoke_context_blockhash!(invoke_context, 95);
        let authorized = *nonce_account.get_key();
        initialize_nonce_account(&mut nonce_account, &authorized, &rent, &invoke_context).unwrap();
        set_invoke_context_blockhash!(invoke_context, 63);
        let mut signers = HashSet::new();
        signers.insert(*nonce_account.get_key());
        let withdraw_lamports = u64::MAX - 54;
        drop(nonce_account);
        let result = withdraw_nonce_account(
            NONCE_ACCOUNT_INDEX,
            withdraw_lamports,
            WITHDRAW_TO_ACCOUNT_INDEX,
            &rent,
            &signers,
            &invoke_context,
            transaction_context,
            instruction_context,
        );
        assert_eq!(result, Err(InstructionError::InsufficientFunds));
    }

    #[test]
    fn initialize_inx_ok() {
        prepare_mockup!(invoke_context, instruction_accounts, rent);
        push_instruction_context!(
            invoke_context,
            transaction_context,
            instruction_context,
            instruction_accounts
        );
        let mut nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        let versions = nonce_account.get_state::<Versions>().unwrap();
        assert_eq!(versions.state(), &State::Uninitialized);
        let mut signers = HashSet::new();
        signers.insert(*nonce_account.get_key());
        set_invoke_context_blockhash!(invoke_context, 0);
        let authorized = *nonce_account.get_key();
        let result =
            initialize_nonce_account(&mut nonce_account, &authorized, &rent, &invoke_context);
        let data = nonce::state::Data::new(
            authorized,
            DurableNonce::from_blockhash(&invoke_context.blockhash),
            invoke_context.lamports_per_signature,
        );
        assert_eq!(result, Ok(()));
        let versions = nonce_account.get_state::<Versions>().unwrap();
        assert_eq!(versions.state(), &State::Initialized(data));
    }

    #[test]
    fn initialize_inx_initialized_account_fail() {
        prepare_mockup!(invoke_context, instruction_accounts, rent);
        push_instruction_context!(
            invoke_context,
            transaction_context,
            instruction_context,
            instruction_accounts
        );
        let mut nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        set_invoke_context_blockhash!(invoke_context, 31);
        let authorized = *nonce_account.get_key();
        initialize_nonce_account(&mut nonce_account, &authorized, &rent, &invoke_context).unwrap();
        set_invoke_context_blockhash!(invoke_context, 0);
        let result =
            initialize_nonce_account(&mut nonce_account, &authorized, &rent, &invoke_context);
        assert_eq!(result, Err(InstructionError::InvalidAccountData));
    }

    #[test]
    fn initialize_inx_uninitialized_acc_insuff_funds_fail() {
        prepare_mockup!(invoke_context, instruction_accounts, rent);
        push_instruction_context!(
            invoke_context,
            transaction_context,
            instruction_context,
            instruction_accounts
        );
        let mut nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        nonce_account.checked_sub_lamports(42 * 2).unwrap();
        set_invoke_context_blockhash!(invoke_context, 63);
        let authorized = *nonce_account.get_key();
        let result =
            initialize_nonce_account(&mut nonce_account, &authorized, &rent, &invoke_context);
        assert_eq!(result, Err(InstructionError::InsufficientFunds));
    }

    #[test]
    fn authorize_inx_ok() {
        prepare_mockup!(invoke_context, instruction_accounts, rent);
        push_instruction_context!(
            invoke_context,
            transaction_context,
            instruction_context,
            instruction_accounts
        );
        let mut nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        let mut signers = HashSet::new();
        signers.insert(*nonce_account.get_key());
        set_invoke_context_blockhash!(invoke_context, 31);
        let authorized = *nonce_account.get_key();
        initialize_nonce_account(&mut nonce_account, &authorized, &rent, &invoke_context).unwrap();
        let authority = Pubkey::default();
        let data = nonce::state::Data::new(
            authority,
            DurableNonce::from_blockhash(&invoke_context.blockhash),
            invoke_context.lamports_per_signature,
        );
        authorize_nonce_account(&mut nonce_account, &authority, &signers, &invoke_context).unwrap();
        let versions = nonce_account.get_state::<Versions>().unwrap();
        assert_eq!(versions.state(), &State::Initialized(data));
    }

    #[test]
    fn authorize_inx_uninitialized_state_fail() {
        prepare_mockup!(invoke_context, instruction_accounts, rent);
        push_instruction_context!(
            invoke_context,
            transaction_context,
            instruction_context,
            instruction_accounts
        );
        let mut nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        let mut signers = HashSet::new();
        signers.insert(*nonce_account.get_key());
        let result = authorize_nonce_account(
            &mut nonce_account,
            &Pubkey::default(),
            &signers,
            &invoke_context,
        );
        assert_eq!(result, Err(InstructionError::InvalidAccountData));
    }

    #[test]
    fn authorize_inx_bad_authority_fail() {
        prepare_mockup!(invoke_context, instruction_accounts, rent);
        push_instruction_context!(
            invoke_context,
            transaction_context,
            instruction_context,
            instruction_accounts
        );
        let mut nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        let mut signers = HashSet::new();
        signers.insert(*nonce_account.get_key());
        set_invoke_context_blockhash!(invoke_context, 31);
        let authorized = Pubkey::default();
        initialize_nonce_account(&mut nonce_account, &authorized, &rent, &invoke_context).unwrap();
        let result =
            authorize_nonce_account(&mut nonce_account, &authorized, &signers, &invoke_context);
        assert_eq!(result, Err(InstructionError::MissingRequiredSignature));
    }

    #[test]
    fn verify_nonce_ok() {
        prepare_mockup!(invoke_context, instruction_accounts, rent);
        push_instruction_context!(
            invoke_context,
            transaction_context,
            instruction_context,
            instruction_accounts
        );
        let mut nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        let mut signers = HashSet::new();
        signers.insert(nonce_account.get_key());
        let versions: Versions = nonce_account.get_state().unwrap();
        // New is in Uninitialzed state
        assert_eq!(versions.state(), &State::Uninitialized);
        set_invoke_context_blockhash!(invoke_context, 0);
        let authorized = *nonce_account.get_key();
        initialize_nonce_account(&mut nonce_account, &authorized, &rent, &invoke_context).unwrap();
        drop(nonce_account);
        assert_matches!(
            verify_nonce_account(
                &transaction_context
                    .get_account_at_index(NONCE_ACCOUNT_INDEX)
                    .unwrap()
                    .borrow(),
                DurableNonce::from_blockhash(&invoke_context.blockhash).as_hash(),
            ),
            Some(_)
        );
    }

    #[test]
    fn verify_nonce_bad_acc_state_fail() {
        prepare_mockup!(invoke_context, instruction_accounts, rent);
        push_instruction_context!(
            invoke_context,
            transaction_context,
            _instruction_context,
            instruction_accounts
        );
        assert_eq!(
            verify_nonce_account(
                &transaction_context
                    .get_account_at_index(NONCE_ACCOUNT_INDEX)
                    .unwrap()
                    .borrow(),
                &Hash::default(),
            ),
            None
        );
    }

    #[test]
    fn verify_nonce_bad_query_hash_fail() {
        prepare_mockup!(invoke_context, instruction_accounts, rent);
        push_instruction_context!(
            invoke_context,
            transaction_context,
            instruction_context,
            instruction_accounts
        );
        let mut nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        let mut signers = HashSet::new();
        signers.insert(nonce_account.get_key());
        let versions: Versions = nonce_account.get_state().unwrap();
        // New is in Uninitialzed state
        assert_eq!(versions.state(), &State::Uninitialized);
        set_invoke_context_blockhash!(invoke_context, 0);
        let authorized = *nonce_account.get_key();
        initialize_nonce_account(
            &mut nonce_account,
            &authorized,
            &Rent::free(),
            &invoke_context,
        )
        .unwrap();
        set_invoke_context_blockhash!(invoke_context, 1);
        drop(nonce_account);
        assert_eq!(
            verify_nonce_account(
                &transaction_context
                    .get_account_at_index(NONCE_ACCOUNT_INDEX)
                    .unwrap()
                    .borrow(),
                DurableNonce::from_blockhash(&invoke_context.blockhash).as_hash(),
            ),
            None
        );
    }
}
