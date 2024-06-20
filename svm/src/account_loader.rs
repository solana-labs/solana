use {
    crate::{
        account_overrides::AccountOverrides, account_rent_state::RentState,
        nonce_info::NoncePartial, rollback_accounts::RollbackAccounts,
        transaction_error_metrics::TransactionErrorMetrics,
        transaction_processing_callback::TransactionProcessingCallback,
    },
    itertools::Itertools,
    solana_compute_budget::compute_budget_processor::{
        process_compute_budget_instructions, ComputeBudgetLimits,
    },
    solana_program_runtime::loaded_programs::{ProgramCacheEntry, ProgramCacheForTxBatch},
    solana_sdk::{
        account::{Account, AccountSharedData, ReadableAccount, WritableAccount},
        feature_set::{self, FeatureSet},
        fee::FeeDetails,
        message::SanitizedMessage,
        native_loader,
        nonce::State as NonceState,
        pubkey::Pubkey,
        rent::RentDue,
        rent_collector::{CollectedInfo, RentCollector, RENT_EXEMPT_RENT_EPOCH},
        rent_debits::RentDebits,
        saturating_add_assign,
        sysvar::{self, instructions::construct_instructions_data},
        transaction::{Result, SanitizedTransaction, TransactionError},
        transaction_context::{IndexOfAccount, TransactionAccount},
    },
    solana_system_program::{get_system_account_kind, SystemAccountKind},
    std::num::NonZeroUsize,
};

// for the load instructions
pub(crate) type TransactionRent = u64;
pub(crate) type TransactionProgramIndices = Vec<Vec<IndexOfAccount>>;
pub type TransactionCheckResult = Result<CheckedTransactionDetails>;
pub type TransactionValidationResult = Result<ValidatedTransactionDetails>;
pub type TransactionLoadResult = Result<LoadedTransaction>;

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct CheckedTransactionDetails {
    pub nonce: Option<NoncePartial>,
    pub lamports_per_signature: u64,
}

#[derive(PartialEq, Eq, Debug, Clone)]
#[cfg_attr(feature = "dev-context-only-utils", derive(Default))]
pub struct ValidatedTransactionDetails {
    pub rollback_accounts: RollbackAccounts,
    pub compute_budget_limits: ComputeBudgetLimits,
    pub fee_details: FeeDetails,
    pub fee_payer_account: AccountSharedData,
    pub fee_payer_rent_debit: u64,
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct LoadedTransaction {
    pub accounts: Vec<TransactionAccount>,
    pub program_indices: TransactionProgramIndices,
    pub fee_details: FeeDetails,
    pub rollback_accounts: RollbackAccounts,
    pub compute_budget_limits: ComputeBudgetLimits,
    pub rent: TransactionRent,
    pub rent_debits: RentDebits,
    pub loaded_accounts_data_size: usize,
}

/// Collect rent from an account if rent is still enabled and regardless of
/// whether rent is enabled, set the rent epoch to u64::MAX if the account is
/// rent exempt.
pub fn collect_rent_from_account(
    feature_set: &FeatureSet,
    rent_collector: &RentCollector,
    address: &Pubkey,
    account: &mut AccountSharedData,
) -> CollectedInfo {
    if !feature_set.is_active(&feature_set::disable_rent_fees_collection::id()) {
        rent_collector.collect_from_existing_account(address, account)
    } else {
        // When rent fee collection is disabled, we won't collect rent for any account. If there
        // are any rent paying accounts, their `rent_epoch` won't change either. However, if the
        // account itself is rent-exempted but its `rent_epoch` is not u64::MAX, we will set its
        // `rent_epoch` to u64::MAX. In such case, the behavior stays the same as before.
        if account.rent_epoch() != RENT_EXEMPT_RENT_EPOCH
            && rent_collector.get_rent_due(
                account.lamports(),
                account.data().len(),
                account.rent_epoch(),
            ) == RentDue::Exempt
        {
            account.set_rent_epoch(RENT_EXEMPT_RENT_EPOCH);
        }

        CollectedInfo::default()
    }
}

/// Check whether the payer_account is capable of paying the fee. The
/// side effect is to subtract the fee amount from the payer_account
/// balance of lamports. If the payer_acount is not able to pay the
/// fee, the error_metrics is incremented, and a specific error is
/// returned.
pub fn validate_fee_payer(
    payer_address: &Pubkey,
    payer_account: &mut AccountSharedData,
    payer_index: IndexOfAccount,
    error_metrics: &mut TransactionErrorMetrics,
    rent_collector: &RentCollector,
    fee: u64,
) -> Result<()> {
    if payer_account.lamports() == 0 {
        error_metrics.account_not_found += 1;
        return Err(TransactionError::AccountNotFound);
    }
    let system_account_kind = get_system_account_kind(payer_account).ok_or_else(|| {
        error_metrics.invalid_account_for_fee += 1;
        TransactionError::InvalidAccountForFee
    })?;
    let min_balance = match system_account_kind {
        SystemAccountKind::System => 0,
        SystemAccountKind::Nonce => {
            // Should we ever allow a fees charge to zero a nonce account's
            // balance. The state MUST be set to uninitialized in that case
            rent_collector.rent.minimum_balance(NonceState::size())
        }
    };

    payer_account
        .lamports()
        .checked_sub(min_balance)
        .and_then(|v| v.checked_sub(fee))
        .ok_or_else(|| {
            error_metrics.insufficient_funds += 1;
            TransactionError::InsufficientFundsForFee
        })?;

    let payer_pre_rent_state = RentState::from_account(payer_account, &rent_collector.rent);
    payer_account
        .checked_sub_lamports(fee)
        .map_err(|_| TransactionError::InsufficientFundsForFee)?;

    let payer_post_rent_state = RentState::from_account(payer_account, &rent_collector.rent);
    RentState::check_rent_state_with_account(
        &payer_pre_rent_state,
        &payer_post_rent_state,
        payer_address,
        payer_account,
        payer_index,
    )
}

/// Collect information about accounts used in txs transactions and
/// return vector of tuples, one for each transaction in the
/// batch. Each tuple contains struct of information about accounts as
/// its first element and an optional transaction nonce info as its
/// second element.
pub(crate) fn load_accounts<CB: TransactionProcessingCallback>(
    callbacks: &CB,
    txs: &[SanitizedTransaction],
    validation_results: Vec<TransactionValidationResult>,
    error_metrics: &mut TransactionErrorMetrics,
    account_overrides: Option<&AccountOverrides>,
    feature_set: &FeatureSet,
    rent_collector: &RentCollector,
    loaded_programs: &ProgramCacheForTxBatch,
) -> Vec<TransactionLoadResult> {
    txs.iter()
        .zip(validation_results)
        .map(|etx| match etx {
            (tx, Ok(tx_details)) => {
                let message = tx.message();

                // load transactions
                load_transaction_accounts(
                    callbacks,
                    message,
                    tx_details,
                    error_metrics,
                    account_overrides,
                    feature_set,
                    rent_collector,
                    loaded_programs,
                )
            }
            (_, Err(e)) => Err(e),
        })
        .collect()
}

fn load_transaction_accounts<CB: TransactionProcessingCallback>(
    callbacks: &CB,
    message: &SanitizedMessage,
    tx_details: ValidatedTransactionDetails,
    error_metrics: &mut TransactionErrorMetrics,
    account_overrides: Option<&AccountOverrides>,
    feature_set: &FeatureSet,
    rent_collector: &RentCollector,
    loaded_programs: &ProgramCacheForTxBatch,
) -> Result<LoadedTransaction> {
    let mut tx_rent: TransactionRent = 0;
    let account_keys = message.account_keys();
    let mut accounts_found = Vec::with_capacity(account_keys.len());
    let mut rent_debits = RentDebits::default();

    let requested_loaded_accounts_data_size_limit =
        get_requested_loaded_accounts_data_size_limit(message)?;
    let mut accumulated_accounts_data_size: usize = 0;

    let instruction_accounts = message
        .instructions()
        .iter()
        .flat_map(|instruction| &instruction.accounts)
        .unique()
        .collect::<Vec<&u8>>();

    let mut accounts = account_keys
        .iter()
        .enumerate()
        .map(|(i, key)| {
            let mut account_found = true;
            #[allow(clippy::collapsible_else_if)]
            let account = if solana_sdk::sysvar::instructions::check_id(key) {
                construct_instructions_account(message)
            } else {
                let is_fee_payer = i == 0;
                let instruction_account = u8::try_from(i)
                    .map(|i| instruction_accounts.contains(&&i))
                    .unwrap_or(false);
                let (account_size, account, rent) = if is_fee_payer {
                    (
                        tx_details.fee_payer_account.data().len(),
                        tx_details.fee_payer_account.clone(),
                        tx_details.fee_payer_rent_debit,
                    )
                } else if let Some(account_override) =
                    account_overrides.and_then(|overrides| overrides.get(key))
                {
                    (account_override.data().len(), account_override.clone(), 0)
                } else if let Some(program) = (!instruction_account && !message.is_writable(i))
                    .then_some(())
                    .and_then(|_| loaded_programs.find(key))
                {
                    callbacks
                        .get_account_shared_data(key)
                        .ok_or(TransactionError::AccountNotFound)?;
                    // Optimization to skip loading of accounts which are only used as
                    // programs in top-level instructions and not passed as instruction accounts.
                    let program_account = account_shared_data_from_program(&program);
                    (program.account_size, program_account, 0)
                } else {
                    callbacks
                        .get_account_shared_data(key)
                        .map(|mut account| {
                            if message.is_writable(i) {
                                let rent_due = collect_rent_from_account(
                                    feature_set,
                                    rent_collector,
                                    key,
                                    &mut account,
                                )
                                .rent_amount;

                                (account.data().len(), account, rent_due)
                            } else {
                                (account.data().len(), account, 0)
                            }
                        })
                        .unwrap_or_else(|| {
                            account_found = false;
                            let mut default_account = AccountSharedData::default();
                            // All new accounts must be rent-exempt (enforced in Bank::execute_loaded_transaction).
                            // Currently, rent collection sets rent_epoch to u64::MAX, but initializing the account
                            // with this field already set would allow us to skip rent collection for these accounts.
                            default_account.set_rent_epoch(RENT_EXEMPT_RENT_EPOCH);
                            (default_account.data().len(), default_account, 0)
                        })
                };
                accumulate_and_check_loaded_account_data_size(
                    &mut accumulated_accounts_data_size,
                    account_size,
                    requested_loaded_accounts_data_size_limit,
                    error_metrics,
                )?;

                tx_rent += rent;
                rent_debits.insert(key, rent, account.lamports());

                account
            };

            accounts_found.push(account_found);
            Ok((*key, account))
        })
        .collect::<Result<Vec<_>>>()?;

    let builtins_start_index = accounts.len();
    let program_indices = message
        .instructions()
        .iter()
        .map(|instruction| {
            let mut account_indices = Vec::with_capacity(2);
            let mut program_index = instruction.program_id_index as usize;
            // This command may never return error, because the transaction is sanitized
            let (program_id, program_account) = accounts
                .get(program_index)
                .ok_or(TransactionError::ProgramAccountNotFound)?;
            if native_loader::check_id(program_id) {
                return Ok(account_indices);
            }

            let account_found = accounts_found.get(program_index).unwrap_or(&true);
            if !account_found {
                error_metrics.account_not_found += 1;
                return Err(TransactionError::ProgramAccountNotFound);
            }

            if !program_account.executable() {
                error_metrics.invalid_program_for_execution += 1;
                return Err(TransactionError::InvalidProgramForExecution);
            }
            account_indices.insert(0, program_index as IndexOfAccount);
            let owner_id = program_account.owner();
            if native_loader::check_id(owner_id) {
                return Ok(account_indices);
            }
            program_index = if let Some(owner_index) = accounts
                .get(builtins_start_index..)
                .ok_or(TransactionError::ProgramAccountNotFound)?
                .iter()
                .position(|(key, _)| key == owner_id)
            {
                builtins_start_index.saturating_add(owner_index)
            } else {
                let owner_index = accounts.len();
                if let Some(owner_account) = callbacks.get_account_shared_data(owner_id) {
                    if !native_loader::check_id(owner_account.owner())
                        || !owner_account.executable()
                    {
                        error_metrics.invalid_program_for_execution += 1;
                        return Err(TransactionError::InvalidProgramForExecution);
                    }
                    accumulate_and_check_loaded_account_data_size(
                        &mut accumulated_accounts_data_size,
                        owner_account.data().len(),
                        requested_loaded_accounts_data_size_limit,
                        error_metrics,
                    )?;
                    accounts.push((*owner_id, owner_account));
                } else {
                    error_metrics.account_not_found += 1;
                    return Err(TransactionError::ProgramAccountNotFound);
                }
                owner_index
            };
            account_indices.insert(0, program_index as IndexOfAccount);
            Ok(account_indices)
        })
        .collect::<Result<Vec<Vec<IndexOfAccount>>>>()?;

    Ok(LoadedTransaction {
        accounts,
        program_indices,
        fee_details: tx_details.fee_details,
        rollback_accounts: tx_details.rollback_accounts,
        compute_budget_limits: tx_details.compute_budget_limits,
        rent: tx_rent,
        rent_debits,
        loaded_accounts_data_size: accumulated_accounts_data_size,
    })
}

/// Total accounts data a transaction can load is limited to
///   if `set_tx_loaded_accounts_data_size` instruction is not activated or not used, then
///     default value of 64MiB to not break anyone in Mainnet-beta today
///   else
///     user requested loaded accounts size.
///     Note, requesting zero bytes will result transaction error
fn get_requested_loaded_accounts_data_size_limit(
    sanitized_message: &SanitizedMessage,
) -> Result<Option<NonZeroUsize>> {
    let compute_budget_limits =
        process_compute_budget_instructions(sanitized_message.program_instructions_iter())
            .unwrap_or_default();
    // sanitize against setting size limit to zero
    NonZeroUsize::new(
        usize::try_from(compute_budget_limits.loaded_accounts_bytes).unwrap_or_default(),
    )
    .map_or(
        Err(TransactionError::InvalidLoadedAccountsDataSizeLimit),
        |v| Ok(Some(v)),
    )
}

fn account_shared_data_from_program(loaded_program: &ProgramCacheEntry) -> AccountSharedData {
    // It's an executable program account. The program is already loaded in the cache.
    // So the account data is not needed. Return a dummy AccountSharedData with meta
    // information.
    let mut program_account = AccountSharedData::default();
    program_account.set_owner(loaded_program.account_owner());
    program_account.set_executable(true);
    program_account
}

/// Accumulate loaded account data size into `accumulated_accounts_data_size`.
/// Returns TransactionErr::MaxLoadedAccountsDataSizeExceeded if
/// `requested_loaded_accounts_data_size_limit` is specified and
/// `accumulated_accounts_data_size` exceeds it.
fn accumulate_and_check_loaded_account_data_size(
    accumulated_loaded_accounts_data_size: &mut usize,
    account_data_size: usize,
    requested_loaded_accounts_data_size_limit: Option<NonZeroUsize>,
    error_metrics: &mut TransactionErrorMetrics,
) -> Result<()> {
    if let Some(requested_loaded_accounts_data_size) = requested_loaded_accounts_data_size_limit {
        saturating_add_assign!(*accumulated_loaded_accounts_data_size, account_data_size);
        if *accumulated_loaded_accounts_data_size > requested_loaded_accounts_data_size.get() {
            error_metrics.max_loaded_accounts_data_size_exceeded += 1;
            Err(TransactionError::MaxLoadedAccountsDataSizeExceeded)
        } else {
            Ok(())
        }
    } else {
        Ok(())
    }
}

fn construct_instructions_account(message: &SanitizedMessage) -> AccountSharedData {
    AccountSharedData::from(Account {
        data: construct_instructions_data(&message.decompile_instructions()),
        owner: sysvar::id(),
        ..Account::default()
    })
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            transaction_account_state_info::TransactionAccountStateInfo,
            transaction_processing_callback::TransactionProcessingCallback,
        },
        nonce::state::Versions as NonceVersions,
        solana_compute_budget::{compute_budget::ComputeBudget, compute_budget_processor},
        solana_program_runtime::loaded_programs::{ProgramCacheEntry, ProgramCacheForTxBatch},
        solana_sdk::{
            account::{Account, AccountSharedData, ReadableAccount, WritableAccount},
            bpf_loader_upgradeable,
            epoch_schedule::EpochSchedule,
            feature_set::FeatureSet,
            hash::Hash,
            instruction::CompiledInstruction,
            message::{
                v0::{LoadedAddresses, LoadedMessage},
                LegacyMessage, Message, MessageHeader, SanitizedMessage,
            },
            native_loader,
            native_token::sol_to_lamports,
            nonce,
            pubkey::Pubkey,
            rent::Rent,
            rent_collector::{RentCollector, RENT_EXEMPT_RENT_EPOCH},
            rent_debits::RentDebits,
            reserved_account_keys::ReservedAccountKeys,
            signature::{Keypair, Signature, Signer},
            system_program, system_transaction, sysvar,
            transaction::{Result, SanitizedTransaction, Transaction, TransactionError},
            transaction_context::{TransactionAccount, TransactionContext},
        },
        std::{borrow::Cow, collections::HashMap, convert::TryFrom, sync::Arc},
    };

    #[derive(Default)]
    struct TestCallbacks {
        accounts_map: HashMap<Pubkey, AccountSharedData>,
    }

    impl TransactionProcessingCallback for TestCallbacks {
        fn account_matches_owners(&self, _account: &Pubkey, _owners: &[Pubkey]) -> Option<usize> {
            None
        }

        fn get_account_shared_data(&self, pubkey: &Pubkey) -> Option<AccountSharedData> {
            self.accounts_map.get(pubkey).cloned()
        }
    }

    fn load_accounts_with_features_and_rent(
        tx: Transaction,
        accounts: &[TransactionAccount],
        rent_collector: &RentCollector,
        error_metrics: &mut TransactionErrorMetrics,
        feature_set: &mut FeatureSet,
    ) -> Vec<TransactionLoadResult> {
        feature_set.deactivate(&feature_set::disable_rent_fees_collection::id());
        let sanitized_tx = SanitizedTransaction::from_transaction_for_tests(tx);
        let fee_payer_account = accounts[0].1.clone();
        let mut accounts_map = HashMap::new();
        for (pubkey, account) in accounts {
            accounts_map.insert(*pubkey, account.clone());
        }
        let callbacks = TestCallbacks { accounts_map };
        load_accounts(
            &callbacks,
            &[sanitized_tx],
            vec![Ok(ValidatedTransactionDetails {
                fee_payer_account,
                ..ValidatedTransactionDetails::default()
            })],
            error_metrics,
            None,
            feature_set,
            rent_collector,
            &ProgramCacheForTxBatch::default(),
        )
    }

    /// get a feature set with all features activated
    /// with the optional except of 'exclude'
    fn all_features_except(exclude: Option<&[Pubkey]>) -> FeatureSet {
        let mut features = FeatureSet::all_enabled();
        if let Some(exclude) = exclude {
            features.active.retain(|k, _v| !exclude.contains(k));
        }
        features
    }

    fn new_unchecked_sanitized_message(message: Message) -> SanitizedMessage {
        SanitizedMessage::Legacy(LegacyMessage::new(
            message,
            &ReservedAccountKeys::empty_key_set(),
        ))
    }

    fn load_accounts_aux_test(
        tx: Transaction,
        accounts: &[TransactionAccount],
        error_metrics: &mut TransactionErrorMetrics,
    ) -> Vec<TransactionLoadResult> {
        load_accounts_with_features_and_rent(
            tx,
            accounts,
            &RentCollector::default(),
            error_metrics,
            &mut FeatureSet::all_enabled(),
        )
    }

    fn load_accounts_with_excluded_features(
        tx: Transaction,
        accounts: &[TransactionAccount],
        error_metrics: &mut TransactionErrorMetrics,
        exclude_features: Option<&[Pubkey]>,
    ) -> Vec<TransactionLoadResult> {
        load_accounts_with_features_and_rent(
            tx,
            accounts,
            &RentCollector::default(),
            error_metrics,
            &mut all_features_except(exclude_features),
        )
    }

    #[test]
    fn test_load_accounts_unknown_program_id() {
        let mut accounts: Vec<TransactionAccount> = Vec::new();
        let mut error_metrics = TransactionErrorMetrics::default();

        let keypair = Keypair::new();
        let key0 = keypair.pubkey();
        let key1 = Pubkey::from([5u8; 32]);

        let account = AccountSharedData::new(1, 0, &Pubkey::default());
        accounts.push((key0, account));

        let account = AccountSharedData::new(2, 1, &Pubkey::default());
        accounts.push((key1, account));

        let instructions = vec![CompiledInstruction::new(1, &(), vec![0])];
        let tx = Transaction::new_with_compiled_instructions(
            &[&keypair],
            &[],
            Hash::default(),
            vec![Pubkey::default()],
            instructions,
        );

        let loaded_accounts = load_accounts_aux_test(tx, &accounts, &mut error_metrics);

        assert_eq!(error_metrics.account_not_found, 1);
        assert_eq!(loaded_accounts.len(), 1);
        assert_eq!(
            loaded_accounts[0],
            Err(TransactionError::ProgramAccountNotFound)
        );
    }

    #[test]
    fn test_load_accounts_no_loaders() {
        let mut accounts: Vec<TransactionAccount> = Vec::new();
        let mut error_metrics = TransactionErrorMetrics::default();

        let keypair = Keypair::new();
        let key0 = keypair.pubkey();
        let key1 = Pubkey::from([5u8; 32]);

        let mut account = AccountSharedData::new(1, 0, &Pubkey::default());
        account.set_rent_epoch(1);
        accounts.push((key0, account));

        let mut account = AccountSharedData::new(2, 1, &Pubkey::default());
        account.set_rent_epoch(1);
        accounts.push((key1, account));

        let instructions = vec![CompiledInstruction::new(2, &(), vec![0, 1])];
        let tx = Transaction::new_with_compiled_instructions(
            &[&keypair],
            &[key1],
            Hash::default(),
            vec![native_loader::id()],
            instructions,
        );

        let loaded_accounts =
            load_accounts_with_excluded_features(tx, &accounts, &mut error_metrics, None);

        assert_eq!(error_metrics.account_not_found, 0);
        assert_eq!(loaded_accounts.len(), 1);
        match &loaded_accounts[0] {
            Ok(loaded_transaction) => {
                assert_eq!(loaded_transaction.accounts.len(), 3);
                assert_eq!(loaded_transaction.accounts[0].1, accounts[0].1);
                assert_eq!(loaded_transaction.program_indices.len(), 1);
                assert_eq!(loaded_transaction.program_indices[0].len(), 0);
            }
            Err(e) => panic!("{e}"),
        }
    }

    #[test]
    fn test_load_accounts_bad_owner() {
        let mut accounts: Vec<TransactionAccount> = Vec::new();
        let mut error_metrics = TransactionErrorMetrics::default();

        let keypair = Keypair::new();
        let key0 = keypair.pubkey();
        let key1 = Pubkey::from([5u8; 32]);

        let account = AccountSharedData::new(1, 0, &Pubkey::default());
        accounts.push((key0, account));

        let mut account = AccountSharedData::new(40, 1, &Pubkey::default());
        account.set_owner(bpf_loader_upgradeable::id());
        account.set_executable(true);
        accounts.push((key1, account));

        let instructions = vec![CompiledInstruction::new(1, &(), vec![0])];
        let tx = Transaction::new_with_compiled_instructions(
            &[&keypair],
            &[],
            Hash::default(),
            vec![key1],
            instructions,
        );

        let loaded_accounts = load_accounts_aux_test(tx, &accounts, &mut error_metrics);

        assert_eq!(error_metrics.account_not_found, 1);
        assert_eq!(loaded_accounts.len(), 1);
        assert_eq!(
            loaded_accounts[0],
            Err(TransactionError::ProgramAccountNotFound)
        );
    }

    #[test]
    fn test_load_accounts_not_executable() {
        let mut accounts: Vec<TransactionAccount> = Vec::new();
        let mut error_metrics = TransactionErrorMetrics::default();

        let keypair = Keypair::new();
        let key0 = keypair.pubkey();
        let key1 = Pubkey::from([5u8; 32]);

        let account = AccountSharedData::new(1, 0, &Pubkey::default());
        accounts.push((key0, account));

        let account = AccountSharedData::new(40, 0, &native_loader::id());
        accounts.push((key1, account));

        let instructions = vec![CompiledInstruction::new(1, &(), vec![0])];
        let tx = Transaction::new_with_compiled_instructions(
            &[&keypair],
            &[],
            Hash::default(),
            vec![key1],
            instructions,
        );

        let loaded_accounts = load_accounts_aux_test(tx, &accounts, &mut error_metrics);

        assert_eq!(error_metrics.invalid_program_for_execution, 1);
        assert_eq!(loaded_accounts.len(), 1);
        assert_eq!(
            loaded_accounts[0],
            Err(TransactionError::InvalidProgramForExecution)
        );
    }

    #[test]
    fn test_load_accounts_multiple_loaders() {
        let mut accounts: Vec<TransactionAccount> = Vec::new();
        let mut error_metrics = TransactionErrorMetrics::default();

        let keypair = Keypair::new();
        let key0 = keypair.pubkey();
        let key1 = bpf_loader_upgradeable::id();
        let key2 = Pubkey::from([6u8; 32]);

        let mut account = AccountSharedData::new(1, 0, &Pubkey::default());
        account.set_rent_epoch(1);
        accounts.push((key0, account));

        let mut account = AccountSharedData::new(40, 1, &Pubkey::default());
        account.set_executable(true);
        account.set_rent_epoch(1);
        account.set_owner(native_loader::id());
        accounts.push((key1, account));

        let mut account = AccountSharedData::new(41, 1, &Pubkey::default());
        account.set_executable(true);
        account.set_rent_epoch(1);
        account.set_owner(key1);
        accounts.push((key2, account));

        let instructions = vec![
            CompiledInstruction::new(1, &(), vec![0]),
            CompiledInstruction::new(2, &(), vec![0]),
        ];
        let tx = Transaction::new_with_compiled_instructions(
            &[&keypair],
            &[],
            Hash::default(),
            vec![key1, key2],
            instructions,
        );

        let loaded_accounts =
            load_accounts_with_excluded_features(tx, &accounts, &mut error_metrics, None);

        assert_eq!(error_metrics.account_not_found, 0);
        assert_eq!(loaded_accounts.len(), 1);
        match &loaded_accounts[0] {
            Ok(loaded_transaction) => {
                assert_eq!(loaded_transaction.accounts.len(), 4);
                assert_eq!(loaded_transaction.accounts[0].1, accounts[0].1);
                assert_eq!(loaded_transaction.program_indices.len(), 2);
                assert_eq!(loaded_transaction.program_indices[0].len(), 1);
                assert_eq!(loaded_transaction.program_indices[1].len(), 2);
                for program_indices in loaded_transaction.program_indices.iter() {
                    for (i, program_index) in program_indices.iter().enumerate() {
                        // +1 to skip first not loader account
                        assert_eq!(
                            loaded_transaction.accounts[*program_index as usize].0,
                            accounts[i + 1].0
                        );
                        assert_eq!(
                            loaded_transaction.accounts[*program_index as usize].1,
                            accounts[i + 1].1
                        );
                    }
                }
            }
            Err(e) => panic!("{e}"),
        }
    }

    fn load_accounts_no_store(
        accounts: &[TransactionAccount],
        tx: Transaction,
        account_overrides: Option<&AccountOverrides>,
    ) -> Vec<TransactionLoadResult> {
        let tx = SanitizedTransaction::from_transaction_for_tests(tx);

        let mut error_metrics = TransactionErrorMetrics::default();
        let mut accounts_map = HashMap::new();
        for (pubkey, account) in accounts {
            accounts_map.insert(*pubkey, account.clone());
        }
        let callbacks = TestCallbacks { accounts_map };
        load_accounts(
            &callbacks,
            &[tx],
            vec![Ok(ValidatedTransactionDetails::default())],
            &mut error_metrics,
            account_overrides,
            &FeatureSet::all_enabled(),
            &RentCollector::default(),
            &ProgramCacheForTxBatch::default(),
        )
    }

    #[test]
    fn test_instructions() {
        solana_logger::setup();
        let instructions_key = solana_sdk::sysvar::instructions::id();
        let keypair = Keypair::new();
        let instructions = vec![CompiledInstruction::new(1, &(), vec![0, 1])];
        let tx = Transaction::new_with_compiled_instructions(
            &[&keypair],
            &[solana_sdk::pubkey::new_rand(), instructions_key],
            Hash::default(),
            vec![native_loader::id()],
            instructions,
        );

        let loaded_accounts = load_accounts_no_store(&[], tx, None);
        assert_eq!(loaded_accounts.len(), 1);
        assert!(loaded_accounts[0].is_err());
    }

    #[test]
    fn test_overrides() {
        solana_logger::setup();
        let mut account_overrides = AccountOverrides::default();
        let slot_history_id = sysvar::slot_history::id();
        let account = AccountSharedData::new(42, 0, &Pubkey::default());
        account_overrides.set_slot_history(Some(account));

        let keypair = Keypair::new();
        let account = AccountSharedData::new(1_000_000, 0, &Pubkey::default());

        let instructions = vec![CompiledInstruction::new(2, &(), vec![0])];
        let tx = Transaction::new_with_compiled_instructions(
            &[&keypair],
            &[slot_history_id],
            Hash::default(),
            vec![native_loader::id()],
            instructions,
        );

        let loaded_accounts =
            load_accounts_no_store(&[(keypair.pubkey(), account)], tx, Some(&account_overrides));
        assert_eq!(loaded_accounts.len(), 1);
        let loaded_transaction = loaded_accounts[0].as_ref().unwrap();
        assert_eq!(loaded_transaction.accounts[0].0, keypair.pubkey());
        assert_eq!(loaded_transaction.accounts[1].0, slot_history_id);
        assert_eq!(loaded_transaction.accounts[1].1.lamports(), 42);
    }

    #[test]
    fn test_accumulate_and_check_loaded_account_data_size() {
        let mut error_metrics = TransactionErrorMetrics::default();

        // assert check is OK if data limit is not enabled
        {
            let mut accumulated_data_size: usize = 0;
            let data_size = usize::MAX;
            let requested_data_size_limit = None;

            assert!(accumulate_and_check_loaded_account_data_size(
                &mut accumulated_data_size,
                data_size,
                requested_data_size_limit,
                &mut error_metrics
            )
            .is_ok());
        }

        // assert check will fail with correct error if loaded data exceeds limit
        {
            let mut accumulated_data_size: usize = 0;
            let data_size: usize = 123;
            let requested_data_size_limit = NonZeroUsize::new(data_size);

            // OK - loaded data size is up to limit
            assert!(accumulate_and_check_loaded_account_data_size(
                &mut accumulated_data_size,
                data_size,
                requested_data_size_limit,
                &mut error_metrics
            )
            .is_ok());
            assert_eq!(data_size, accumulated_data_size);

            // fail - loading more data that would exceed limit
            let another_byte: usize = 1;
            assert_eq!(
                accumulate_and_check_loaded_account_data_size(
                    &mut accumulated_data_size,
                    another_byte,
                    requested_data_size_limit,
                    &mut error_metrics
                ),
                Err(TransactionError::MaxLoadedAccountsDataSizeExceeded)
            );
        }
    }

    #[test]
    fn test_get_requested_loaded_accounts_data_size_limit() {
        // an prrivate helper function
        fn test(
            instructions: &[solana_sdk::instruction::Instruction],
            expected_result: &Result<Option<NonZeroUsize>>,
        ) {
            let payer_keypair = Keypair::new();
            let tx = SanitizedTransaction::from_transaction_for_tests(Transaction::new(
                &[&payer_keypair],
                Message::new(instructions, Some(&payer_keypair.pubkey())),
                Hash::default(),
            ));
            assert_eq!(
                *expected_result,
                get_requested_loaded_accounts_data_size_limit(tx.message())
            );
        }

        let tx_not_set_limit = &[solana_sdk::instruction::Instruction::new_with_bincode(
            Pubkey::new_unique(),
            &0_u8,
            vec![],
        )];
        let tx_set_limit_99 =
            &[
                solana_sdk::compute_budget::ComputeBudgetInstruction::set_loaded_accounts_data_size_limit(99u32),
                solana_sdk::instruction::Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
            ];
        let tx_set_limit_0 =
            &[
                solana_sdk::compute_budget::ComputeBudgetInstruction::set_loaded_accounts_data_size_limit(0u32),
                solana_sdk::instruction::Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
            ];

        let result_default_limit = Ok(Some(
            NonZeroUsize::new(
                usize::try_from(compute_budget_processor::MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES)
                    .unwrap(),
            )
            .unwrap(),
        ));
        let result_requested_limit: Result<Option<NonZeroUsize>> =
            Ok(Some(NonZeroUsize::new(99).unwrap()));
        let result_invalid_limit = Err(TransactionError::InvalidLoadedAccountsDataSizeLimit);

        // the results should be:
        //    if tx doesn't set limit, then default limit (64MiB)
        //    if tx sets limit, then requested limit
        //    if tx sets limit to zero, then TransactionError::InvalidLoadedAccountsDataSizeLimit
        test(tx_not_set_limit, &result_default_limit);
        test(tx_set_limit_99, &result_requested_limit);
        test(tx_set_limit_0, &result_invalid_limit);
    }

    struct ValidateFeePayerTestParameter {
        is_nonce: bool,
        payer_init_balance: u64,
        fee: u64,
        expected_result: Result<()>,
        payer_post_balance: u64,
    }
    fn validate_fee_payer_account(
        test_parameter: ValidateFeePayerTestParameter,
        rent_collector: &RentCollector,
    ) {
        let payer_account_keys = Keypair::new();
        let mut account = if test_parameter.is_nonce {
            AccountSharedData::new_data(
                test_parameter.payer_init_balance,
                &NonceVersions::new(NonceState::Initialized(nonce::state::Data::default())),
                &system_program::id(),
            )
            .unwrap()
        } else {
            AccountSharedData::new(test_parameter.payer_init_balance, 0, &system_program::id())
        };
        let result = validate_fee_payer(
            &payer_account_keys.pubkey(),
            &mut account,
            0,
            &mut TransactionErrorMetrics::default(),
            rent_collector,
            test_parameter.fee,
        );

        assert_eq!(result, test_parameter.expected_result);
        assert_eq!(account.lamports(), test_parameter.payer_post_balance);
    }

    #[test]
    fn test_validate_fee_payer() {
        let rent_collector = RentCollector::new(
            0,
            EpochSchedule::default(),
            500_000.0,
            Rent {
                lamports_per_byte_year: 1,
                ..Rent::default()
            },
        );
        let min_balance = rent_collector.rent.minimum_balance(NonceState::size());
        let fee = 5_000;

        // If payer account has sufficient balance, expect successful fee deduction,
        // regardless feature gate status, or if payer is nonce account.
        {
            for (is_nonce, min_balance) in [(true, min_balance), (false, 0)] {
                validate_fee_payer_account(
                    ValidateFeePayerTestParameter {
                        is_nonce,
                        payer_init_balance: min_balance + fee,
                        fee,
                        expected_result: Ok(()),
                        payer_post_balance: min_balance,
                    },
                    &rent_collector,
                );
            }
        }

        // If payer account has no balance, expected AccountNotFound Error
        // regardless feature gate status, or if payer is nonce account.
        {
            for is_nonce in [true, false] {
                validate_fee_payer_account(
                    ValidateFeePayerTestParameter {
                        is_nonce,
                        payer_init_balance: 0,
                        fee,
                        expected_result: Err(TransactionError::AccountNotFound),
                        payer_post_balance: 0,
                    },
                    &rent_collector,
                );
            }
        }

        // If payer account has insufficient balance, expect InsufficientFundsForFee error
        // regardless feature gate status, or if payer is nonce account.
        {
            for (is_nonce, min_balance) in [(true, min_balance), (false, 0)] {
                validate_fee_payer_account(
                    ValidateFeePayerTestParameter {
                        is_nonce,
                        payer_init_balance: min_balance + fee - 1,
                        fee,
                        expected_result: Err(TransactionError::InsufficientFundsForFee),
                        payer_post_balance: min_balance + fee - 1,
                    },
                    &rent_collector,
                );
            }
        }

        // normal payer account has balance of u64::MAX, so does fee; since it does not  require
        // min_balance, expect successful fee deduction, regardless of feature gate status
        {
            validate_fee_payer_account(
                ValidateFeePayerTestParameter {
                    is_nonce: false,
                    payer_init_balance: u64::MAX,
                    fee: u64::MAX,
                    expected_result: Ok(()),
                    payer_post_balance: 0,
                },
                &rent_collector,
            );
        }
    }

    #[test]
    fn test_validate_nonce_fee_payer_with_checked_arithmetic() {
        let rent_collector = RentCollector::new(
            0,
            EpochSchedule::default(),
            500_000.0,
            Rent {
                lamports_per_byte_year: 1,
                ..Rent::default()
            },
        );

        // nonce payer account has balance of u64::MAX, so does fee; due to nonce account
        // requires additional min_balance, expect InsufficientFundsForFee error if feature gate is
        // enabled
        validate_fee_payer_account(
            ValidateFeePayerTestParameter {
                is_nonce: true,
                payer_init_balance: u64::MAX,
                fee: u64::MAX,
                expected_result: Err(TransactionError::InsufficientFundsForFee),
                payer_post_balance: u64::MAX,
            },
            &rent_collector,
        );
    }

    #[test]
    fn test_construct_instructions_account() {
        let loaded_message = LoadedMessage {
            message: Cow::Owned(solana_sdk::message::v0::Message::default()),
            loaded_addresses: Cow::Owned(LoadedAddresses::default()),
            is_writable_account_cache: vec![false],
        };
        let message = SanitizedMessage::V0(loaded_message);
        let shared_data = construct_instructions_account(&message);
        let expected = AccountSharedData::from(Account {
            data: construct_instructions_data(&message.decompile_instructions()),
            owner: sysvar::id(),
            ..Account::default()
        });
        assert_eq!(shared_data, expected);
    }

    #[test]
    fn test_load_transaction_accounts_fee_payer() {
        let fee_payer_address = Pubkey::new_unique();
        let message = Message {
            account_keys: vec![fee_payer_address],
            header: MessageHeader::default(),
            instructions: vec![],
            recent_blockhash: Hash::default(),
        };

        let sanitized_message = new_unchecked_sanitized_message(message);
        let mut mock_bank = TestCallbacks::default();

        let fee_payer_balance = 200;
        let mut fee_payer_account_data = AccountSharedData::default();
        fee_payer_account_data.set_lamports(fee_payer_balance);
        mock_bank
            .accounts_map
            .insert(fee_payer_address, fee_payer_account_data.clone());
        let fee_payer_rent_debit = 42;

        let mut error_metrics = TransactionErrorMetrics::default();
        let loaded_programs = ProgramCacheForTxBatch::default();

        let sanitized_transaction = SanitizedTransaction::new_for_tests(
            sanitized_message,
            vec![Signature::new_unique()],
            false,
        );
        let result = load_transaction_accounts(
            &mock_bank,
            sanitized_transaction.message(),
            ValidatedTransactionDetails {
                fee_payer_account: fee_payer_account_data.clone(),
                fee_payer_rent_debit,
                ..ValidatedTransactionDetails::default()
            },
            &mut error_metrics,
            None,
            &FeatureSet::default(),
            &RentCollector::default(),
            &loaded_programs,
        );

        let expected_rent_debits = {
            let mut rent_debits = RentDebits::default();
            rent_debits.insert(&fee_payer_address, fee_payer_rent_debit, fee_payer_balance);
            rent_debits
        };
        assert_eq!(
            result.unwrap(),
            LoadedTransaction {
                accounts: vec![(fee_payer_address, fee_payer_account_data),],
                program_indices: vec![],
                fee_details: FeeDetails::default(),
                rollback_accounts: RollbackAccounts::default(),
                compute_budget_limits: ComputeBudgetLimits::default(),
                rent: fee_payer_rent_debit,
                rent_debits: expected_rent_debits,
                loaded_accounts_data_size: 0,
            }
        );
    }

    #[test]
    fn test_load_transaction_accounts_native_loader() {
        let key1 = Keypair::new();
        let message = Message {
            account_keys: vec![key1.pubkey(), native_loader::id()],
            header: MessageHeader::default(),
            instructions: vec![CompiledInstruction {
                program_id_index: 1,
                accounts: vec![0],
                data: vec![],
            }],
            recent_blockhash: Hash::default(),
        };

        let sanitized_message = new_unchecked_sanitized_message(message);
        let mut mock_bank = TestCallbacks::default();
        mock_bank
            .accounts_map
            .insert(native_loader::id(), AccountSharedData::default());
        let mut fee_payer_account_data = AccountSharedData::default();
        fee_payer_account_data.set_lamports(200);
        mock_bank
            .accounts_map
            .insert(key1.pubkey(), fee_payer_account_data.clone());

        let mut error_metrics = TransactionErrorMetrics::default();
        let loaded_programs = ProgramCacheForTxBatch::default();

        let sanitized_transaction = SanitizedTransaction::new_for_tests(
            sanitized_message,
            vec![Signature::new_unique()],
            false,
        );
        let result = load_transaction_accounts(
            &mock_bank,
            sanitized_transaction.message(),
            ValidatedTransactionDetails {
                fee_payer_account: fee_payer_account_data.clone(),
                ..ValidatedTransactionDetails::default()
            },
            &mut error_metrics,
            None,
            &FeatureSet::default(),
            &RentCollector::default(),
            &loaded_programs,
        );

        assert_eq!(
            result.unwrap(),
            LoadedTransaction {
                accounts: vec![
                    (key1.pubkey(), fee_payer_account_data),
                    (
                        native_loader::id(),
                        mock_bank.accounts_map[&native_loader::id()].clone()
                    )
                ],
                program_indices: vec![vec![]],
                fee_details: FeeDetails::default(),
                rollback_accounts: RollbackAccounts::default(),
                compute_budget_limits: ComputeBudgetLimits::default(),
                rent: 0,
                rent_debits: RentDebits::default(),
                loaded_accounts_data_size: 0,
            }
        );
    }

    #[test]
    fn test_load_transaction_accounts_program_account_not_found_but_loaded() {
        let key1 = Keypair::new();
        let key2 = Keypair::new();

        let message = Message {
            account_keys: vec![key1.pubkey(), key2.pubkey()],
            header: MessageHeader::default(),
            instructions: vec![CompiledInstruction {
                program_id_index: 1,
                accounts: vec![0],
                data: vec![],
            }],
            recent_blockhash: Hash::default(),
        };

        let sanitized_message = new_unchecked_sanitized_message(message);
        let mut mock_bank = TestCallbacks::default();
        let mut account_data = AccountSharedData::default();
        account_data.set_lamports(200);
        mock_bank.accounts_map.insert(key1.pubkey(), account_data);

        let mut error_metrics = TransactionErrorMetrics::default();
        let mut loaded_programs = ProgramCacheForTxBatch::default();
        loaded_programs.replenish(key2.pubkey(), Arc::new(ProgramCacheEntry::default()));

        let sanitized_transaction = SanitizedTransaction::new_for_tests(
            sanitized_message,
            vec![Signature::new_unique()],
            false,
        );
        let result = load_transaction_accounts(
            &mock_bank,
            sanitized_transaction.message(),
            ValidatedTransactionDetails::default(),
            &mut error_metrics,
            None,
            &FeatureSet::default(),
            &RentCollector::default(),
            &loaded_programs,
        );

        assert_eq!(result.err(), Some(TransactionError::AccountNotFound));
    }

    #[test]
    fn test_load_transaction_accounts_program_account_no_data() {
        let key1 = Keypair::new();
        let key2 = Keypair::new();

        let message = Message {
            account_keys: vec![key1.pubkey(), key2.pubkey()],
            header: MessageHeader::default(),
            instructions: vec![CompiledInstruction {
                program_id_index: 1,
                accounts: vec![0, 1],
                data: vec![],
            }],
            recent_blockhash: Hash::default(),
        };

        let sanitized_message = new_unchecked_sanitized_message(message);
        let mut mock_bank = TestCallbacks::default();
        let mut account_data = AccountSharedData::default();
        account_data.set_lamports(200);
        mock_bank.accounts_map.insert(key1.pubkey(), account_data);

        let mut error_metrics = TransactionErrorMetrics::default();
        let loaded_programs = ProgramCacheForTxBatch::default();

        let sanitized_transaction = SanitizedTransaction::new_for_tests(
            sanitized_message,
            vec![Signature::new_unique()],
            false,
        );
        let result = load_transaction_accounts(
            &mock_bank,
            sanitized_transaction.message(),
            ValidatedTransactionDetails::default(),
            &mut error_metrics,
            None,
            &FeatureSet::default(),
            &RentCollector::default(),
            &loaded_programs,
        );

        assert_eq!(result.err(), Some(TransactionError::ProgramAccountNotFound));
    }

    #[test]
    fn test_load_transaction_accounts_invalid_program_for_execution() {
        let key1 = Keypair::new();
        let key2 = Keypair::new();

        let message = Message {
            account_keys: vec![key1.pubkey(), key2.pubkey()],
            header: MessageHeader::default(),
            instructions: vec![CompiledInstruction {
                program_id_index: 0,
                accounts: vec![0, 1],
                data: vec![],
            }],
            recent_blockhash: Hash::default(),
        };

        let sanitized_message = new_unchecked_sanitized_message(message);
        let mut mock_bank = TestCallbacks::default();
        let mut account_data = AccountSharedData::default();
        account_data.set_lamports(200);
        mock_bank.accounts_map.insert(key1.pubkey(), account_data);

        let mut error_metrics = TransactionErrorMetrics::default();
        let loaded_programs = ProgramCacheForTxBatch::default();

        let sanitized_transaction = SanitizedTransaction::new_for_tests(
            sanitized_message,
            vec![Signature::new_unique()],
            false,
        );
        let result = load_transaction_accounts(
            &mock_bank,
            sanitized_transaction.message(),
            ValidatedTransactionDetails::default(),
            &mut error_metrics,
            None,
            &FeatureSet::default(),
            &RentCollector::default(),
            &loaded_programs,
        );

        assert_eq!(
            result.err(),
            Some(TransactionError::InvalidProgramForExecution)
        );
    }

    #[test]
    fn test_load_transaction_accounts_native_loader_owner() {
        let key1 = Keypair::new();
        let key2 = Keypair::new();

        let message = Message {
            account_keys: vec![key2.pubkey(), key1.pubkey()],
            header: MessageHeader::default(),
            instructions: vec![CompiledInstruction {
                program_id_index: 1,
                accounts: vec![0],
                data: vec![],
            }],
            recent_blockhash: Hash::default(),
        };

        let sanitized_message = new_unchecked_sanitized_message(message);
        let mut mock_bank = TestCallbacks::default();
        let mut account_data = AccountSharedData::default();
        account_data.set_owner(native_loader::id());
        account_data.set_executable(true);
        mock_bank.accounts_map.insert(key1.pubkey(), account_data);

        let mut fee_payer_account_data = AccountSharedData::default();
        fee_payer_account_data.set_lamports(200);
        mock_bank
            .accounts_map
            .insert(key2.pubkey(), fee_payer_account_data.clone());
        let mut error_metrics = TransactionErrorMetrics::default();
        let loaded_programs = ProgramCacheForTxBatch::default();

        let sanitized_transaction = SanitizedTransaction::new_for_tests(
            sanitized_message,
            vec![Signature::new_unique()],
            false,
        );
        let result = load_transaction_accounts(
            &mock_bank,
            sanitized_transaction.message(),
            ValidatedTransactionDetails {
                fee_payer_account: fee_payer_account_data.clone(),
                ..ValidatedTransactionDetails::default()
            },
            &mut error_metrics,
            None,
            &FeatureSet::default(),
            &RentCollector::default(),
            &loaded_programs,
        );

        assert_eq!(
            result.unwrap(),
            LoadedTransaction {
                accounts: vec![
                    (key2.pubkey(), fee_payer_account_data),
                    (
                        key1.pubkey(),
                        mock_bank.accounts_map[&key1.pubkey()].clone()
                    ),
                ],
                fee_details: FeeDetails::default(),
                rollback_accounts: RollbackAccounts::default(),
                compute_budget_limits: ComputeBudgetLimits::default(),
                program_indices: vec![vec![1]],
                rent: 0,
                rent_debits: RentDebits::default(),
                loaded_accounts_data_size: 0,
            }
        );
    }

    #[test]
    fn test_load_transaction_accounts_program_account_not_found_after_all_checks() {
        let key1 = Keypair::new();
        let key2 = Keypair::new();

        let message = Message {
            account_keys: vec![key2.pubkey(), key1.pubkey()],
            header: MessageHeader::default(),
            instructions: vec![CompiledInstruction {
                program_id_index: 1,
                accounts: vec![0],
                data: vec![],
            }],
            recent_blockhash: Hash::default(),
        };

        let sanitized_message = new_unchecked_sanitized_message(message);
        let mut mock_bank = TestCallbacks::default();
        let mut account_data = AccountSharedData::default();
        account_data.set_executable(true);
        mock_bank.accounts_map.insert(key1.pubkey(), account_data);

        let mut account_data = AccountSharedData::default();
        account_data.set_lamports(200);
        mock_bank.accounts_map.insert(key2.pubkey(), account_data);
        let mut error_metrics = TransactionErrorMetrics::default();
        let loaded_programs = ProgramCacheForTxBatch::default();

        let sanitized_transaction = SanitizedTransaction::new_for_tests(
            sanitized_message,
            vec![Signature::new_unique()],
            false,
        );
        let result = load_transaction_accounts(
            &mock_bank,
            sanitized_transaction.message(),
            ValidatedTransactionDetails::default(),
            &mut error_metrics,
            None,
            &FeatureSet::default(),
            &RentCollector::default(),
            &loaded_programs,
        );

        assert_eq!(result.err(), Some(TransactionError::ProgramAccountNotFound));
    }

    #[test]
    fn test_load_transaction_accounts_program_account_invalid_program_for_execution_last_check() {
        let key1 = Keypair::new();
        let key2 = Keypair::new();
        let key3 = Keypair::new();

        let message = Message {
            account_keys: vec![key2.pubkey(), key1.pubkey()],
            header: MessageHeader::default(),
            instructions: vec![CompiledInstruction {
                program_id_index: 1,
                accounts: vec![0],
                data: vec![],
            }],
            recent_blockhash: Hash::default(),
        };

        let sanitized_message = new_unchecked_sanitized_message(message);
        let mut mock_bank = TestCallbacks::default();
        let mut account_data = AccountSharedData::default();
        account_data.set_executable(true);
        account_data.set_owner(key3.pubkey());
        mock_bank.accounts_map.insert(key1.pubkey(), account_data);

        let mut account_data = AccountSharedData::default();
        account_data.set_lamports(200);
        mock_bank.accounts_map.insert(key2.pubkey(), account_data);

        mock_bank
            .accounts_map
            .insert(key3.pubkey(), AccountSharedData::default());
        let mut error_metrics = TransactionErrorMetrics::default();
        let loaded_programs = ProgramCacheForTxBatch::default();

        let sanitized_transaction = SanitizedTransaction::new_for_tests(
            sanitized_message,
            vec![Signature::new_unique()],
            false,
        );
        let result = load_transaction_accounts(
            &mock_bank,
            sanitized_transaction.message(),
            ValidatedTransactionDetails::default(),
            &mut error_metrics,
            None,
            &FeatureSet::default(),
            &RentCollector::default(),
            &loaded_programs,
        );

        assert_eq!(
            result.err(),
            Some(TransactionError::InvalidProgramForExecution)
        );
    }

    #[test]
    fn test_load_transaction_accounts_program_success_complete() {
        let key1 = Keypair::new();
        let key2 = Keypair::new();
        let key3 = Keypair::new();

        let message = Message {
            account_keys: vec![key2.pubkey(), key1.pubkey()],
            header: MessageHeader::default(),
            instructions: vec![CompiledInstruction {
                program_id_index: 1,
                accounts: vec![0],
                data: vec![],
            }],
            recent_blockhash: Hash::default(),
        };

        let sanitized_message = new_unchecked_sanitized_message(message);
        let mut mock_bank = TestCallbacks::default();
        let mut account_data = AccountSharedData::default();
        account_data.set_executable(true);
        account_data.set_owner(key3.pubkey());
        mock_bank.accounts_map.insert(key1.pubkey(), account_data);

        let mut fee_payer_account_data = AccountSharedData::default();
        fee_payer_account_data.set_lamports(200);
        mock_bank
            .accounts_map
            .insert(key2.pubkey(), fee_payer_account_data.clone());

        let mut account_data = AccountSharedData::default();
        account_data.set_executable(true);
        account_data.set_owner(native_loader::id());
        mock_bank.accounts_map.insert(key3.pubkey(), account_data);

        let mut error_metrics = TransactionErrorMetrics::default();
        let loaded_programs = ProgramCacheForTxBatch::default();

        let sanitized_transaction = SanitizedTransaction::new_for_tests(
            sanitized_message,
            vec![Signature::new_unique()],
            false,
        );
        let result = load_transaction_accounts(
            &mock_bank,
            sanitized_transaction.message(),
            ValidatedTransactionDetails {
                fee_payer_account: fee_payer_account_data.clone(),
                ..ValidatedTransactionDetails::default()
            },
            &mut error_metrics,
            None,
            &FeatureSet::default(),
            &RentCollector::default(),
            &loaded_programs,
        );

        assert_eq!(
            result.unwrap(),
            LoadedTransaction {
                accounts: vec![
                    (key2.pubkey(), fee_payer_account_data),
                    (
                        key1.pubkey(),
                        mock_bank.accounts_map[&key1.pubkey()].clone()
                    ),
                    (
                        key3.pubkey(),
                        mock_bank.accounts_map[&key3.pubkey()].clone()
                    ),
                ],
                program_indices: vec![vec![2, 1]],
                fee_details: FeeDetails::default(),
                rollback_accounts: RollbackAccounts::default(),
                compute_budget_limits: ComputeBudgetLimits::default(),
                rent: 0,
                rent_debits: RentDebits::default(),
                loaded_accounts_data_size: 0,
            }
        );
    }

    #[test]
    fn test_load_transaction_accounts_program_builtin_saturating_add() {
        let key1 = Keypair::new();
        let key2 = Keypair::new();
        let key3 = Keypair::new();
        let key4 = Keypair::new();

        let message = Message {
            account_keys: vec![key2.pubkey(), key1.pubkey(), key4.pubkey()],
            header: MessageHeader::default(),
            instructions: vec![
                CompiledInstruction {
                    program_id_index: 1,
                    accounts: vec![0],
                    data: vec![],
                },
                CompiledInstruction {
                    program_id_index: 1,
                    accounts: vec![2],
                    data: vec![],
                },
            ],
            recent_blockhash: Hash::default(),
        };

        let sanitized_message = new_unchecked_sanitized_message(message);
        let mut mock_bank = TestCallbacks::default();
        let mut account_data = AccountSharedData::default();
        account_data.set_executable(true);
        account_data.set_owner(key3.pubkey());
        mock_bank.accounts_map.insert(key1.pubkey(), account_data);

        let mut fee_payer_account_data = AccountSharedData::default();
        fee_payer_account_data.set_lamports(200);
        mock_bank
            .accounts_map
            .insert(key2.pubkey(), fee_payer_account_data.clone());

        let mut account_data = AccountSharedData::default();
        account_data.set_executable(true);
        account_data.set_owner(native_loader::id());
        mock_bank.accounts_map.insert(key3.pubkey(), account_data);

        let mut error_metrics = TransactionErrorMetrics::default();
        let loaded_programs = ProgramCacheForTxBatch::default();

        let sanitized_transaction = SanitizedTransaction::new_for_tests(
            sanitized_message,
            vec![Signature::new_unique()],
            false,
        );
        let result = load_transaction_accounts(
            &mock_bank,
            sanitized_transaction.message(),
            ValidatedTransactionDetails {
                fee_payer_account: fee_payer_account_data.clone(),
                ..ValidatedTransactionDetails::default()
            },
            &mut error_metrics,
            None,
            &FeatureSet::default(),
            &RentCollector::default(),
            &loaded_programs,
        );

        let mut account_data = AccountSharedData::default();
        account_data.set_rent_epoch(RENT_EXEMPT_RENT_EPOCH);
        assert_eq!(
            result.unwrap(),
            LoadedTransaction {
                accounts: vec![
                    (key2.pubkey(), fee_payer_account_data),
                    (
                        key1.pubkey(),
                        mock_bank.accounts_map[&key1.pubkey()].clone()
                    ),
                    (key4.pubkey(), account_data),
                    (
                        key3.pubkey(),
                        mock_bank.accounts_map[&key3.pubkey()].clone()
                    ),
                ],
                program_indices: vec![vec![3, 1], vec![3, 1]],
                fee_details: FeeDetails::default(),
                rollback_accounts: RollbackAccounts::default(),
                compute_budget_limits: ComputeBudgetLimits::default(),
                rent: 0,
                rent_debits: RentDebits::default(),
                loaded_accounts_data_size: 0,
            }
        );
    }

    #[test]
    fn test_rent_state_list_len() {
        let mint_keypair = Keypair::new();
        let mut bank = TestCallbacks::default();
        let recipient = Pubkey::new_unique();
        let last_block_hash = Hash::new_unique();

        let mut system_data = AccountSharedData::default();
        system_data.set_executable(true);
        system_data.set_owner(native_loader::id());
        bank.accounts_map
            .insert(Pubkey::new_from_array([0u8; 32]), system_data);

        let mut mint_data = AccountSharedData::default();
        mint_data.set_lamports(2);
        bank.accounts_map.insert(mint_keypair.pubkey(), mint_data);

        bank.accounts_map
            .insert(recipient, AccountSharedData::default());

        let tx = system_transaction::transfer(
            &mint_keypair,
            &recipient,
            sol_to_lamports(1.),
            last_block_hash,
        );
        let num_accounts = tx.message().account_keys.len();
        let sanitized_tx = SanitizedTransaction::from_transaction_for_tests(tx);
        let mut error_metrics = TransactionErrorMetrics::default();
        let loaded_txs = load_accounts(
            &bank,
            &[sanitized_tx.clone()],
            vec![Ok(ValidatedTransactionDetails::default())],
            &mut error_metrics,
            None,
            &FeatureSet::default(),
            &RentCollector::default(),
            &ProgramCacheForTxBatch::default(),
        );

        let compute_budget = ComputeBudget::new(u64::from(
            compute_budget_processor::DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT,
        ));
        let transaction_context = TransactionContext::new(
            loaded_txs[0].as_ref().unwrap().accounts.clone(),
            Rent::default(),
            compute_budget.max_instruction_stack_depth,
            compute_budget.max_instruction_trace_length,
        );

        assert_eq!(
            TransactionAccountStateInfo::new(
                &Rent::default(),
                &transaction_context,
                sanitized_tx.message()
            )
            .len(),
            num_accounts,
        );
    }

    #[test]
    fn test_load_accounts_success() {
        let key1 = Keypair::new();
        let key2 = Keypair::new();
        let key3 = Keypair::new();
        let key4 = Keypair::new();

        let message = Message {
            account_keys: vec![key2.pubkey(), key1.pubkey(), key4.pubkey()],
            header: MessageHeader::default(),
            instructions: vec![
                CompiledInstruction {
                    program_id_index: 1,
                    accounts: vec![0],
                    data: vec![],
                },
                CompiledInstruction {
                    program_id_index: 1,
                    accounts: vec![2],
                    data: vec![],
                },
            ],
            recent_blockhash: Hash::default(),
        };

        let sanitized_message = new_unchecked_sanitized_message(message);
        let mut mock_bank = TestCallbacks::default();
        let mut account_data = AccountSharedData::default();
        account_data.set_executable(true);
        account_data.set_owner(key3.pubkey());
        mock_bank.accounts_map.insert(key1.pubkey(), account_data);

        let mut fee_payer_account_data = AccountSharedData::default();
        fee_payer_account_data.set_lamports(200);
        mock_bank
            .accounts_map
            .insert(key2.pubkey(), fee_payer_account_data.clone());

        let mut account_data = AccountSharedData::default();
        account_data.set_executable(true);
        account_data.set_owner(native_loader::id());
        mock_bank.accounts_map.insert(key3.pubkey(), account_data);

        let mut error_metrics = TransactionErrorMetrics::default();
        let loaded_programs = ProgramCacheForTxBatch::default();

        let sanitized_transaction = SanitizedTransaction::new_for_tests(
            sanitized_message,
            vec![Signature::new_unique()],
            false,
        );
        let validation_result = Ok(ValidatedTransactionDetails {
            fee_payer_account: fee_payer_account_data,
            ..ValidatedTransactionDetails::default()
        });

        let results = load_accounts(
            &mock_bank,
            &[sanitized_transaction],
            vec![validation_result],
            &mut error_metrics,
            None,
            &FeatureSet::default(),
            &RentCollector::default(),
            &loaded_programs,
        );

        let mut account_data = AccountSharedData::default();
        account_data.set_rent_epoch(RENT_EXEMPT_RENT_EPOCH);

        assert_eq!(results.len(), 1);
        let loaded_result = results[0].clone();
        assert_eq!(
            loaded_result.unwrap(),
            LoadedTransaction {
                accounts: vec![
                    (
                        key2.pubkey(),
                        mock_bank.accounts_map[&key2.pubkey()].clone()
                    ),
                    (
                        key1.pubkey(),
                        mock_bank.accounts_map[&key1.pubkey()].clone()
                    ),
                    (key4.pubkey(), account_data),
                    (
                        key3.pubkey(),
                        mock_bank.accounts_map[&key3.pubkey()].clone()
                    ),
                ],
                program_indices: vec![vec![3, 1], vec![3, 1]],
                fee_details: FeeDetails::default(),
                rollback_accounts: RollbackAccounts::default(),
                compute_budget_limits: ComputeBudgetLimits::default(),
                rent: 0,
                rent_debits: RentDebits::default(),
                loaded_accounts_data_size: 0,
            }
        );
    }

    #[test]
    fn test_load_accounts_error() {
        let mock_bank = TestCallbacks::default();
        let feature_set = FeatureSet::default();
        let rent_collector = RentCollector::default();

        let message = Message {
            account_keys: vec![Pubkey::new_from_array([0; 32])],
            header: MessageHeader::default(),
            instructions: vec![CompiledInstruction {
                program_id_index: 0,
                accounts: vec![],
                data: vec![],
            }],
            recent_blockhash: Hash::default(),
        };

        let sanitized_message = new_unchecked_sanitized_message(message);
        let sanitized_transaction = SanitizedTransaction::new_for_tests(
            sanitized_message,
            vec![Signature::new_unique()],
            false,
        );

        let validation_result = Ok(ValidatedTransactionDetails::default());
        let result = load_accounts(
            &mock_bank,
            &[sanitized_transaction.clone()],
            vec![validation_result.clone()],
            &mut TransactionErrorMetrics::default(),
            None,
            &feature_set,
            &rent_collector,
            &ProgramCacheForTxBatch::default(),
        );

        assert_eq!(
            result,
            vec![Err(TransactionError::InvalidProgramForExecution)]
        );

        let validation_result = Err(TransactionError::InvalidWritableAccount);

        let result = load_accounts(
            &mock_bank,
            &[sanitized_transaction.clone()],
            vec![validation_result],
            &mut TransactionErrorMetrics::default(),
            None,
            &feature_set,
            &rent_collector,
            &ProgramCacheForTxBatch::default(),
        );

        assert_eq!(result, vec![Err(TransactionError::InvalidWritableAccount)]);
    }

    #[test]
    fn test_collect_rent_from_account() {
        let feature_set = FeatureSet::all_enabled();
        let rent_collector = RentCollector {
            epoch: 1,
            ..RentCollector::default()
        };

        let address = Pubkey::new_unique();
        let min_exempt_balance = rent_collector.rent.minimum_balance(0);
        let mut account = AccountSharedData::from(Account {
            lamports: min_exempt_balance,
            ..Account::default()
        });

        assert_eq!(
            collect_rent_from_account(&feature_set, &rent_collector, &address, &mut account),
            CollectedInfo::default()
        );
        assert_eq!(account.rent_epoch(), RENT_EXEMPT_RENT_EPOCH);
    }

    #[test]
    fn test_collect_rent_from_account_rent_paying() {
        let feature_set = FeatureSet::all_enabled();
        let rent_collector = RentCollector {
            epoch: 1,
            ..RentCollector::default()
        };

        let address = Pubkey::new_unique();
        let mut account = AccountSharedData::from(Account {
            lamports: 1,
            ..Account::default()
        });

        assert_eq!(
            collect_rent_from_account(&feature_set, &rent_collector, &address, &mut account),
            CollectedInfo::default()
        );
        assert_eq!(account.rent_epoch(), 0);
        assert_eq!(account.lamports(), 1);
    }

    #[test]
    fn test_collect_rent_from_account_rent_enabled() {
        let feature_set =
            all_features_except(Some(&[feature_set::disable_rent_fees_collection::id()]));
        let rent_collector = RentCollector {
            epoch: 1,
            ..RentCollector::default()
        };

        let address = Pubkey::new_unique();
        let mut account = AccountSharedData::from(Account {
            lamports: 1,
            data: vec![0],
            ..Account::default()
        });

        assert_eq!(
            collect_rent_from_account(&feature_set, &rent_collector, &address, &mut account),
            CollectedInfo {
                rent_amount: 1,
                account_data_len_reclaimed: 1
            }
        );
        assert_eq!(account.rent_epoch(), 0);
        assert_eq!(account.lamports(), 0);
    }
}
