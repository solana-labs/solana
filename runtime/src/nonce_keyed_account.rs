use {
    solana_program_runtime::{ic_msg, invoke_context::InvokeContext},
    solana_sdk::{
        feature_set::{self, nonce_must_be_writable},
        instruction::{checked_add, InstructionError},
        nonce::{self, state::Versions, State},
        pubkey::Pubkey,
        system_instruction::{nonce_to_instruction_error, NonceError},
        sysvar::rent::Rent,
        transaction_context::InstructionContext,
    },
    std::collections::HashSet,
};

pub fn advance_nonce_account(
    invoke_context: &InvokeContext,
    instruction_context: &InstructionContext,
    signers: &HashSet<Pubkey>,
    nonce_account_index: usize,
) -> Result<(), InstructionError> {
    let mut account = instruction_context
        .try_borrow_instruction_account(invoke_context.transaction_context, nonce_account_index)?;
    let merge_nonce_error_into_system_error = invoke_context
        .feature_set
        .is_active(&feature_set::merge_nonce_error_into_system_error::id());

    if invoke_context
        .feature_set
        .is_active(&nonce_must_be_writable::id())
        && !account.is_writable()
    {
        ic_msg!(
            invoke_context,
            "Advance nonce account: Account {} must be writeable",
            account.get_key()
        );
        return Err(InstructionError::InvalidArgument);
    }

    let state = account.get_state::<Versions>()?.convert_to_current();
    match state {
        State::Initialized(data) => {
            if !signers.contains(&data.authority) {
                ic_msg!(
                    invoke_context,
                    "Advance nonce account: Account {} must be a signer",
                    data.authority
                );
                return Err(InstructionError::MissingRequiredSignature);
            }
            let recent_blockhash = invoke_context.blockhash;
            if data.blockhash == recent_blockhash {
                ic_msg!(
                    invoke_context,
                    "Advance nonce account: nonce can only advance once per slot"
                );
                return Err(nonce_to_instruction_error(
                    NonceError::NotExpired,
                    merge_nonce_error_into_system_error,
                ));
            }

            let new_data = nonce::state::Data::new(
                data.authority,
                recent_blockhash,
                invoke_context.lamports_per_signature,
            );
            account.set_state(&Versions::new_current(State::Initialized(new_data)))
        }
        _ => {
            ic_msg!(
                invoke_context,
                "Advance nonce account: Account {} state is invalid",
                account.get_key()
            );
            Err(nonce_to_instruction_error(
                NonceError::BadAccountState,
                merge_nonce_error_into_system_error,
            ))
        }
    }
}

pub fn withdraw_nonce_account(
    invoke_context: &InvokeContext,
    instruction_context: &InstructionContext,
    signers: &HashSet<Pubkey>,
    nonce_account_index: usize,
    withdraw_to_account_index: usize,
    lamports: u64,
    rent: &Rent,
) -> Result<(), InstructionError> {
    let mut nonce_account = instruction_context
        .try_borrow_instruction_account(invoke_context.transaction_context, nonce_account_index)?;
    let merge_nonce_error_into_system_error = invoke_context
        .feature_set
        .is_active(&feature_set::merge_nonce_error_into_system_error::id());

    if invoke_context
        .feature_set
        .is_active(&nonce_must_be_writable::id())
        && !nonce_account.is_writable()
    {
        ic_msg!(
            invoke_context,
            "Withdraw nonce account: Account {} must be writeable",
            nonce_account.get_key()
        );
        return Err(InstructionError::InvalidArgument);
    }

    let signer = match nonce_account.get_state::<Versions>()?.convert_to_current() {
        State::Uninitialized => {
            if lamports > nonce_account.get_lamports() {
                ic_msg!(
                    invoke_context,
                    "Withdraw nonce account: insufficient lamports {}, need {}",
                    nonce_account.get_lamports(),
                    lamports,
                );
                return Err(InstructionError::InsufficientFunds);
            }
            *nonce_account.get_key()
        }
        State::Initialized(ref data) => {
            if lamports == nonce_account.get_lamports() {
                if data.blockhash == invoke_context.blockhash {
                    ic_msg!(
                        invoke_context,
                        "Withdraw nonce account: nonce can only advance once per slot"
                    );
                    return Err(nonce_to_instruction_error(
                        NonceError::NotExpired,
                        merge_nonce_error_into_system_error,
                    ));
                }
                nonce_account.set_state(&Versions::new_current(State::Uninitialized))?;
            } else {
                let min_balance = rent.minimum_balance(nonce_account.get_data().len());
                let amount = checked_add(lamports, min_balance)?;
                if amount > nonce_account.get_lamports() {
                    ic_msg!(
                        invoke_context,
                        "Withdraw nonce account: insufficient lamports {}, need {}",
                        nonce_account.get_lamports(),
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

    nonce_account.checked_sub_lamports(lamports)?;
    drop(nonce_account);
    let mut withdraw_to_account = instruction_context.try_borrow_instruction_account(
        invoke_context.transaction_context,
        withdraw_to_account_index,
    )?;
    withdraw_to_account.checked_add_lamports(lamports)?;

    Ok(())
}

pub fn initialize_nonce_account(
    invoke_context: &InvokeContext,
    instruction_context: &InstructionContext,
    nonce_account_index: usize,
    nonce_authority: &Pubkey,
    rent: &Rent,
) -> Result<(), InstructionError> {
    let mut account = instruction_context
        .try_borrow_instruction_account(invoke_context.transaction_context, nonce_account_index)?;
    let merge_nonce_error_into_system_error = invoke_context
        .feature_set
        .is_active(&feature_set::merge_nonce_error_into_system_error::id());

    if invoke_context
        .feature_set
        .is_active(&nonce_must_be_writable::id())
        && !account.is_writable()
    {
        ic_msg!(
            invoke_context,
            "Initialize nonce account: Account {} must be writeable",
            account.get_key()
        );
        return Err(InstructionError::InvalidArgument);
    }

    match account.get_state::<Versions>()?.convert_to_current() {
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
            let data = nonce::state::Data::new(
                *nonce_authority,
                invoke_context.blockhash,
                invoke_context.lamports_per_signature,
            );
            account.set_state(&Versions::new_current(State::Initialized(data)))
        }
        _ => {
            ic_msg!(
                invoke_context,
                "Initialize nonce account: Account {} state is invalid",
                account.get_key()
            );
            Err(nonce_to_instruction_error(
                NonceError::BadAccountState,
                merge_nonce_error_into_system_error,
            ))
        }
    }
}

pub fn authorize_nonce_account(
    invoke_context: &InvokeContext,
    instruction_context: &InstructionContext,
    signers: &HashSet<Pubkey>,
    nonce_account_index: usize,
    nonce_authority: &Pubkey,
) -> Result<(), InstructionError> {
    let mut account = instruction_context
        .try_borrow_instruction_account(invoke_context.transaction_context, nonce_account_index)?;
    let merge_nonce_error_into_system_error = invoke_context
        .feature_set
        .is_active(&feature_set::merge_nonce_error_into_system_error::id());

    if invoke_context
        .feature_set
        .is_active(&nonce_must_be_writable::id())
        && !account.is_writable()
    {
        ic_msg!(
            invoke_context,
            "Authorize nonce account: Account {} must be writeable",
            account.get_key()
        );
        return Err(InstructionError::InvalidArgument);
    }

    match account.get_state::<Versions>()?.convert_to_current() {
        State::Initialized(data) => {
            if !signers.contains(&data.authority) {
                ic_msg!(
                    invoke_context,
                    "Authorize nonce account: Account {} must sign",
                    data.authority
                );
                return Err(InstructionError::MissingRequiredSignature);
            }
            let new_data = nonce::state::Data::new(
                *nonce_authority,
                data.blockhash,
                data.get_lamports_per_signature(),
            );
            account.set_state(&Versions::new_current(State::Initialized(new_data)))
        }
        _ => {
            ic_msg!(
                invoke_context,
                "Authorize nonce account: Account {} state is invalid",
                account.get_key()
            );
            Err(nonce_to_instruction_error(
                NonceError::BadAccountState,
                merge_nonce_error_into_system_error,
            ))
        }
    }
}

#[cfg(test)]
mod test {
    use {
        super::*,
        solana_program_runtime::invoke_context::InvokeContext,
        solana_sdk::{
            account::AccountSharedData,
            hash::{hash, Hash},
            nonce::{self, State},
            nonce_account::{create_account, verify_nonce_account},
            system_instruction::SystemError,
            system_program,
            transaction_context::{InstructionAccount, TransactionContext},
        },
    };

    macro_rules! push_instruction_context {
        ($invoke_context:expr, $transaction_context:ident, $instruction_context:ident, $instruction_accounts:ident) => {
            $invoke_context
                .push(&$instruction_accounts, &[2], &[])
                .unwrap();
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
            let accounts = vec![
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
                    is_signer: true,
                    is_writable: true,
                },
                InstructionAccount {
                    index_in_transaction: 1,
                    index_in_caller: 1,
                    is_signer: false,
                    is_writable: true,
                },
            ];
            let mut transaction_context = TransactionContext::new(accounts, 1, 2);
            let mut $invoke_context = InvokeContext::new_mock(&mut transaction_context, &[]);
        };
    }

    const NONCE_ACCOUNT_INDEX: usize = 0;
    const WITHDRAW_TO_ACCOUNT_INDEX: usize = 1;

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
        let nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        let data = nonce::state::Data {
            authority: *nonce_account.get_key(),
            ..nonce::state::Data::default()
        };
        let mut signers = HashSet::new();
        signers.insert(*nonce_account.get_key());
        let state = nonce_account
            .get_state::<Versions>()
            .unwrap()
            .convert_to_current();
        // New is in Uninitialzed state
        assert_eq!(state, State::Uninitialized);
        set_invoke_context_blockhash!(invoke_context, 95);
        let authorized = *nonce_account.get_key();
        drop(nonce_account);
        initialize_nonce_account(
            &invoke_context,
            instruction_context,
            NONCE_ACCOUNT_INDEX,
            &authorized,
            &rent,
        )
        .unwrap();
        let nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        let state = nonce_account
            .get_state::<Versions>()
            .unwrap()
            .convert_to_current();
        let data = nonce::state::Data::new(
            data.authority,
            invoke_context.blockhash,
            invoke_context.lamports_per_signature,
        );
        // First nonce instruction drives state from Uninitialized to Initialized
        assert_eq!(state, State::Initialized(data.clone()));
        set_invoke_context_blockhash!(invoke_context, 63);
        drop(nonce_account);
        advance_nonce_account(
            &invoke_context,
            instruction_context,
            &signers,
            NONCE_ACCOUNT_INDEX,
        )
        .unwrap();
        let nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        let state = nonce_account
            .get_state::<Versions>()
            .unwrap()
            .convert_to_current();
        let data = nonce::state::Data::new(
            data.authority,
            invoke_context.blockhash,
            invoke_context.lamports_per_signature,
        );
        // Second nonce instruction consumes and replaces stored nonce
        assert_eq!(state, State::Initialized(data.clone()));
        set_invoke_context_blockhash!(invoke_context, 31);
        drop(nonce_account);
        advance_nonce_account(
            &invoke_context,
            instruction_context,
            &signers,
            NONCE_ACCOUNT_INDEX,
        )
        .unwrap();
        let nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        let state = nonce_account
            .get_state::<Versions>()
            .unwrap()
            .convert_to_current();
        let data = nonce::state::Data::new(
            data.authority,
            invoke_context.blockhash,
            invoke_context.lamports_per_signature,
        );
        // Third nonce instruction for fun and profit
        assert_eq!(state, State::Initialized(data));

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
            &invoke_context,
            instruction_context,
            &signers,
            NONCE_ACCOUNT_INDEX,
            WITHDRAW_TO_ACCOUNT_INDEX,
            withdraw_lamports,
            &rent,
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
        let state = nonce_account
            .get_state::<Versions>()
            .unwrap()
            .convert_to_current();
        // Empty balance deinitializes data
        assert_eq!(state, State::Uninitialized);
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
        let nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        set_invoke_context_blockhash!(invoke_context, 31);
        let authority = *nonce_account.get_key();
        drop(nonce_account);
        initialize_nonce_account(
            &invoke_context,
            instruction_context,
            NONCE_ACCOUNT_INDEX,
            &authority,
            &rent,
        )
        .unwrap();
        let nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        let state = nonce_account
            .get_state::<Versions>()
            .unwrap()
            .convert_to_current();
        let data = nonce::state::Data::new(
            authority,
            invoke_context.blockhash,
            invoke_context.lamports_per_signature,
        );
        assert_eq!(state, State::Initialized(data));
        drop(nonce_account);
        // Nonce account did not sign
        let signers = HashSet::new();
        set_invoke_context_blockhash!(invoke_context, 0);
        let result = advance_nonce_account(
            &invoke_context,
            instruction_context,
            &signers,
            NONCE_ACCOUNT_INDEX,
        );
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
        let nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        let mut signers = HashSet::new();
        signers.insert(*nonce_account.get_key());
        set_invoke_context_blockhash!(invoke_context, 63);
        let authorized = *nonce_account.get_key();
        drop(nonce_account);
        initialize_nonce_account(
            &invoke_context,
            instruction_context,
            NONCE_ACCOUNT_INDEX,
            &authorized,
            &rent,
        )
        .unwrap();
        let result = advance_nonce_account(
            &invoke_context,
            instruction_context,
            &signers,
            NONCE_ACCOUNT_INDEX,
        );
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
        let nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        let mut signers = HashSet::new();
        signers.insert(*nonce_account.get_key());
        set_invoke_context_blockhash!(invoke_context, 63);
        drop(nonce_account);
        let result = advance_nonce_account(
            &invoke_context,
            instruction_context,
            &signers,
            NONCE_ACCOUNT_INDEX,
        );
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
        let nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        let nonce_authority = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX + 1)
            .unwrap();
        let mut signers = HashSet::new();
        signers.insert(*nonce_account.get_key());
        set_invoke_context_blockhash!(invoke_context, 63);
        let authorized = *nonce_authority.get_key();
        drop(nonce_account);
        drop(nonce_authority);
        initialize_nonce_account(
            &invoke_context,
            instruction_context,
            NONCE_ACCOUNT_INDEX,
            &authorized,
            &rent,
        )
        .unwrap();
        let mut signers = HashSet::new();
        signers.insert(authorized);
        set_invoke_context_blockhash!(invoke_context, 31);
        let result = advance_nonce_account(
            &invoke_context,
            instruction_context,
            &signers,
            NONCE_ACCOUNT_INDEX,
        );
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
        let nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        let nonce_authority = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX + 1)
            .unwrap();
        let mut signers = HashSet::new();
        signers.insert(*nonce_account.get_key());
        set_invoke_context_blockhash!(invoke_context, 63);
        let authorized = *nonce_authority.get_key();
        drop(nonce_account);
        drop(nonce_authority);
        initialize_nonce_account(
            &invoke_context,
            instruction_context,
            NONCE_ACCOUNT_INDEX,
            &authorized,
            &rent,
        )
        .unwrap();
        let result = advance_nonce_account(
            &invoke_context,
            instruction_context,
            &signers,
            NONCE_ACCOUNT_INDEX,
        );
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
        let state = nonce_account
            .get_state::<Versions>()
            .unwrap()
            .convert_to_current();
        assert_eq!(state, State::Uninitialized);
        let mut signers = HashSet::new();
        signers.insert(*nonce_account.get_key());
        set_invoke_context_blockhash!(invoke_context, 0);
        let withdraw_lamports = nonce_account.get_lamports();
        let expect_from_lamports = nonce_account.get_lamports() - withdraw_lamports;
        let expect_to_lamports = to_account.get_lamports() + withdraw_lamports;
        drop(nonce_account);
        drop(to_account);
        withdraw_nonce_account(
            &invoke_context,
            instruction_context,
            &signers,
            NONCE_ACCOUNT_INDEX,
            WITHDRAW_TO_ACCOUNT_INDEX,
            withdraw_lamports,
            &rent,
        )
        .unwrap();
        let nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        let to_account = instruction_context
            .try_borrow_instruction_account(transaction_context, WITHDRAW_TO_ACCOUNT_INDEX)
            .unwrap();
        let state = nonce_account
            .get_state::<Versions>()
            .unwrap()
            .convert_to_current();
        assert_eq!(state, State::Uninitialized);
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
        let state = nonce_account
            .get_state::<Versions>()
            .unwrap()
            .convert_to_current();
        assert_eq!(state, State::Uninitialized);
        let signers = HashSet::new();
        set_invoke_context_blockhash!(invoke_context, 0);
        let withdraw_lamports = nonce_account.get_lamports();
        drop(nonce_account);
        drop(to_account);
        let result = withdraw_nonce_account(
            &invoke_context,
            instruction_context,
            &signers,
            NONCE_ACCOUNT_INDEX,
            WITHDRAW_TO_ACCOUNT_INDEX,
            withdraw_lamports,
            &rent,
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
        let state = nonce_account
            .get_state::<Versions>()
            .unwrap()
            .convert_to_current();
        assert_eq!(state, State::Uninitialized);
        let mut signers = HashSet::new();
        signers.insert(*nonce_account.get_key());
        set_invoke_context_blockhash!(invoke_context, 0);
        let withdraw_lamports = nonce_account.get_lamports() + 1;
        drop(nonce_account);
        let result = withdraw_nonce_account(
            &invoke_context,
            instruction_context,
            &signers,
            NONCE_ACCOUNT_INDEX,
            WITHDRAW_TO_ACCOUNT_INDEX,
            withdraw_lamports,
            &rent,
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
            &invoke_context,
            instruction_context,
            &signers,
            NONCE_ACCOUNT_INDEX,
            WITHDRAW_TO_ACCOUNT_INDEX,
            withdraw_lamports,
            &rent,
        )
        .unwrap();
        let nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        let to_account = instruction_context
            .try_borrow_instruction_account(transaction_context, WITHDRAW_TO_ACCOUNT_INDEX)
            .unwrap();
        let state = nonce_account
            .get_state::<Versions>()
            .unwrap()
            .convert_to_current();
        assert_eq!(state, State::Uninitialized);
        assert_eq!(nonce_account.get_lamports(), from_expect_lamports);
        assert_eq!(to_account.get_lamports(), to_expect_lamports);
        let withdraw_lamports = nonce_account.get_lamports();
        let from_expect_lamports = nonce_account.get_lamports() - withdraw_lamports;
        let to_expect_lamports = to_account.get_lamports() + withdraw_lamports;
        drop(nonce_account);
        drop(to_account);
        withdraw_nonce_account(
            &invoke_context,
            instruction_context,
            &signers,
            NONCE_ACCOUNT_INDEX,
            WITHDRAW_TO_ACCOUNT_INDEX,
            withdraw_lamports,
            &rent,
        )
        .unwrap();
        let nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        let to_account = instruction_context
            .try_borrow_instruction_account(transaction_context, WITHDRAW_TO_ACCOUNT_INDEX)
            .unwrap();
        let state = nonce_account
            .get_state::<Versions>()
            .unwrap()
            .convert_to_current();
        assert_eq!(state, State::Uninitialized);
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
        let nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        let to_account = instruction_context
            .try_borrow_instruction_account(transaction_context, WITHDRAW_TO_ACCOUNT_INDEX)
            .unwrap();
        let mut signers = HashSet::new();
        signers.insert(*nonce_account.get_key());
        set_invoke_context_blockhash!(invoke_context, 31);
        let authority = *nonce_account.get_key();
        drop(nonce_account);
        drop(to_account);
        initialize_nonce_account(
            &invoke_context,
            instruction_context,
            NONCE_ACCOUNT_INDEX,
            &authority,
            &rent,
        )
        .unwrap();
        let nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        let to_account = instruction_context
            .try_borrow_instruction_account(transaction_context, WITHDRAW_TO_ACCOUNT_INDEX)
            .unwrap();
        let state = nonce_account
            .get_state::<Versions>()
            .unwrap()
            .convert_to_current();
        let data = nonce::state::Data::new(
            authority,
            invoke_context.blockhash,
            invoke_context.lamports_per_signature,
        );
        assert_eq!(state, State::Initialized(data.clone()));
        let withdraw_lamports = 42;
        let from_expect_lamports = nonce_account.get_lamports() - withdraw_lamports;
        let to_expect_lamports = to_account.get_lamports() + withdraw_lamports;
        drop(nonce_account);
        drop(to_account);
        withdraw_nonce_account(
            &invoke_context,
            instruction_context,
            &signers,
            NONCE_ACCOUNT_INDEX,
            WITHDRAW_TO_ACCOUNT_INDEX,
            withdraw_lamports,
            &rent,
        )
        .unwrap();
        let nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        let to_account = instruction_context
            .try_borrow_instruction_account(transaction_context, WITHDRAW_TO_ACCOUNT_INDEX)
            .unwrap();
        let state = nonce_account
            .get_state::<Versions>()
            .unwrap()
            .convert_to_current();
        let data = nonce::state::Data::new(
            data.authority,
            invoke_context.blockhash,
            invoke_context.lamports_per_signature,
        );
        assert_eq!(state, State::Initialized(data));
        assert_eq!(nonce_account.get_lamports(), from_expect_lamports);
        assert_eq!(to_account.get_lamports(), to_expect_lamports);
        set_invoke_context_blockhash!(invoke_context, 0);
        let withdraw_lamports = nonce_account.get_lamports();
        let from_expect_lamports = nonce_account.get_lamports() - withdraw_lamports;
        let to_expect_lamports = to_account.get_lamports() + withdraw_lamports;
        drop(nonce_account);
        drop(to_account);
        withdraw_nonce_account(
            &invoke_context,
            instruction_context,
            &signers,
            NONCE_ACCOUNT_INDEX,
            WITHDRAW_TO_ACCOUNT_INDEX,
            withdraw_lamports,
            &rent,
        )
        .unwrap();
        let nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        let to_account = instruction_context
            .try_borrow_instruction_account(transaction_context, WITHDRAW_TO_ACCOUNT_INDEX)
            .unwrap();
        let state = nonce_account
            .get_state::<Versions>()
            .unwrap()
            .convert_to_current();
        assert_eq!(state, State::Uninitialized);
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
        let nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        let to_account = instruction_context
            .try_borrow_instruction_account(transaction_context, WITHDRAW_TO_ACCOUNT_INDEX)
            .unwrap();
        set_invoke_context_blockhash!(invoke_context, 0);
        let authorized = *nonce_account.get_key();
        drop(nonce_account);
        drop(to_account);
        initialize_nonce_account(
            &invoke_context,
            instruction_context,
            NONCE_ACCOUNT_INDEX,
            &authorized,
            &rent,
        )
        .unwrap();
        let nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        let mut signers = HashSet::new();
        signers.insert(*nonce_account.get_key());
        let withdraw_lamports = nonce_account.get_lamports();
        drop(nonce_account);
        let result = withdraw_nonce_account(
            &invoke_context,
            instruction_context,
            &signers,
            NONCE_ACCOUNT_INDEX,
            WITHDRAW_TO_ACCOUNT_INDEX,
            withdraw_lamports,
            &rent,
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
        let nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        set_invoke_context_blockhash!(invoke_context, 95);
        let authorized = *nonce_account.get_key();
        drop(nonce_account);
        initialize_nonce_account(
            &invoke_context,
            instruction_context,
            NONCE_ACCOUNT_INDEX,
            &authorized,
            &rent,
        )
        .unwrap();
        set_invoke_context_blockhash!(invoke_context, 63);
        let nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        let mut signers = HashSet::new();
        signers.insert(*nonce_account.get_key());
        let withdraw_lamports = nonce_account.get_lamports() + 1;
        drop(nonce_account);
        let result = withdraw_nonce_account(
            &invoke_context,
            instruction_context,
            &signers,
            NONCE_ACCOUNT_INDEX,
            WITHDRAW_TO_ACCOUNT_INDEX,
            withdraw_lamports,
            &rent,
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
        let nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        set_invoke_context_blockhash!(invoke_context, 95);
        let authorized = *nonce_account.get_key();
        drop(nonce_account);
        initialize_nonce_account(
            &invoke_context,
            instruction_context,
            NONCE_ACCOUNT_INDEX,
            &authorized,
            &rent,
        )
        .unwrap();
        let nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        set_invoke_context_blockhash!(invoke_context, 63);
        let mut signers = HashSet::new();
        signers.insert(*nonce_account.get_key());
        let withdraw_lamports = 42 + 1;
        drop(nonce_account);
        let result = withdraw_nonce_account(
            &invoke_context,
            instruction_context,
            &signers,
            NONCE_ACCOUNT_INDEX,
            WITHDRAW_TO_ACCOUNT_INDEX,
            withdraw_lamports,
            &rent,
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
        let nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        set_invoke_context_blockhash!(invoke_context, 95);
        let authorized = *nonce_account.get_key();
        drop(nonce_account);
        initialize_nonce_account(
            &invoke_context,
            instruction_context,
            NONCE_ACCOUNT_INDEX,
            &authorized,
            &rent,
        )
        .unwrap();
        let nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        set_invoke_context_blockhash!(invoke_context, 63);
        let mut signers = HashSet::new();
        signers.insert(*nonce_account.get_key());
        let withdraw_lamports = u64::MAX - 54;
        drop(nonce_account);
        let result = withdraw_nonce_account(
            &invoke_context,
            instruction_context,
            &signers,
            NONCE_ACCOUNT_INDEX,
            WITHDRAW_TO_ACCOUNT_INDEX,
            withdraw_lamports,
            &rent,
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
        let nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        let state = nonce_account
            .get_state::<Versions>()
            .unwrap()
            .convert_to_current();
        assert_eq!(state, State::Uninitialized);
        let mut signers = HashSet::new();
        signers.insert(*nonce_account.get_key());
        set_invoke_context_blockhash!(invoke_context, 0);
        let authorized = *nonce_account.get_key();
        drop(nonce_account);
        let result = initialize_nonce_account(
            &invoke_context,
            instruction_context,
            NONCE_ACCOUNT_INDEX,
            &authorized,
            &rent,
        );
        let nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        let data = nonce::state::Data::new(
            authorized,
            invoke_context.blockhash,
            invoke_context.lamports_per_signature,
        );
        assert_eq!(result, Ok(()));
        let state = nonce_account
            .get_state::<Versions>()
            .unwrap()
            .convert_to_current();
        assert_eq!(state, State::Initialized(data));
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
        let nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        set_invoke_context_blockhash!(invoke_context, 31);
        let authorized = *nonce_account.get_key();
        drop(nonce_account);
        initialize_nonce_account(
            &invoke_context,
            instruction_context,
            NONCE_ACCOUNT_INDEX,
            &authorized,
            &rent,
        )
        .unwrap();
        set_invoke_context_blockhash!(invoke_context, 0);
        let result = initialize_nonce_account(
            &invoke_context,
            instruction_context,
            NONCE_ACCOUNT_INDEX,
            &authorized,
            &rent,
        );
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
        drop(nonce_account);
        let result = initialize_nonce_account(
            &invoke_context,
            instruction_context,
            NONCE_ACCOUNT_INDEX,
            &authorized,
            &rent,
        );
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
        let nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        let mut signers = HashSet::new();
        signers.insert(*nonce_account.get_key());
        set_invoke_context_blockhash!(invoke_context, 31);
        let authorized = *nonce_account.get_key();
        drop(nonce_account);
        initialize_nonce_account(
            &invoke_context,
            instruction_context,
            NONCE_ACCOUNT_INDEX,
            &authorized,
            &rent,
        )
        .unwrap();
        let authority = Pubkey::default();
        let data = nonce::state::Data::new(
            authority,
            invoke_context.blockhash,
            invoke_context.lamports_per_signature,
        );
        authorize_nonce_account(
            &invoke_context,
            instruction_context,
            &signers,
            NONCE_ACCOUNT_INDEX,
            &authority,
        )
        .unwrap();
        let nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        let state = nonce_account
            .get_state::<Versions>()
            .unwrap()
            .convert_to_current();
        assert_eq!(state, State::Initialized(data));
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
        let nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        let mut signers = HashSet::new();
        signers.insert(*nonce_account.get_key());
        drop(nonce_account);
        let result = authorize_nonce_account(
            &invoke_context,
            instruction_context,
            &signers,
            NONCE_ACCOUNT_INDEX,
            &Pubkey::default(),
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
        let nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        let mut signers = HashSet::new();
        signers.insert(*nonce_account.get_key());
        set_invoke_context_blockhash!(invoke_context, 31);
        let authorized = Pubkey::default();
        drop(nonce_account);
        initialize_nonce_account(
            &invoke_context,
            instruction_context,
            NONCE_ACCOUNT_INDEX,
            &authorized,
            &rent,
        )
        .unwrap();
        let result = authorize_nonce_account(
            &invoke_context,
            instruction_context,
            &signers,
            NONCE_ACCOUNT_INDEX,
            &authorized,
        );
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
        let nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        let mut signers = HashSet::new();
        signers.insert(nonce_account.get_key());
        let state: State = nonce_account.get_state().unwrap();
        // New is in Uninitialzed state
        assert_eq!(state, State::Uninitialized);
        set_invoke_context_blockhash!(invoke_context, 0);
        let authorized = *nonce_account.get_key();
        drop(nonce_account);
        initialize_nonce_account(
            &invoke_context,
            instruction_context,
            NONCE_ACCOUNT_INDEX,
            &authorized,
            &rent,
        )
        .unwrap();
        assert!(verify_nonce_account(
            &transaction_context
                .get_account_at_index(NONCE_ACCOUNT_INDEX)
                .unwrap()
                .borrow(),
            &invoke_context.blockhash,
        ));
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
        assert!(!verify_nonce_account(
            &transaction_context
                .get_account_at_index(NONCE_ACCOUNT_INDEX)
                .unwrap()
                .borrow(),
            &Hash::default()
        ));
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
        let nonce_account = instruction_context
            .try_borrow_instruction_account(transaction_context, NONCE_ACCOUNT_INDEX)
            .unwrap();
        let mut signers = HashSet::new();
        signers.insert(nonce_account.get_key());
        let state: State = nonce_account.get_state().unwrap();
        // New is in Uninitialzed state
        assert_eq!(state, State::Uninitialized);
        set_invoke_context_blockhash!(invoke_context, 0);
        let authorized = *nonce_account.get_key();
        drop(nonce_account);
        initialize_nonce_account(
            &invoke_context,
            instruction_context,
            NONCE_ACCOUNT_INDEX,
            &authorized,
            &Rent::free(),
        )
        .unwrap();
        set_invoke_context_blockhash!(invoke_context, 1);
        assert!(!verify_nonce_account(
            &transaction_context
                .get_account_at_index(NONCE_ACCOUNT_INDEX)
                .unwrap()
                .borrow(),
            &invoke_context.blockhash,
        ));
    }
}
