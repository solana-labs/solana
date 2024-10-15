use {
    crate::{
        account_overrides::AccountOverrides,
        nonce_info::NonceInfo,
        rollback_accounts::RollbackAccounts,
        transaction_error_metrics::TransactionErrorMetrics,
        transaction_processing_callback::{AccountState, TransactionProcessingCallback},
    },
    itertools::Itertools,
    solana_compute_budget::compute_budget_limits::ComputeBudgetLimits,
    solana_feature_set::{self as feature_set, FeatureSet},
    solana_program_runtime::loaded_programs::{ProgramCacheEntry, ProgramCacheForTxBatch},
    solana_sdk::{
        account::{Account, AccountSharedData, ReadableAccount, WritableAccount},
        fee::FeeDetails,
        native_loader,
        nonce::State as NonceState,
        pubkey::Pubkey,
        rent::RentDue,
        rent_collector::{CollectedInfo, RENT_EXEMPT_RENT_EPOCH},
        rent_debits::RentDebits,
        saturating_add_assign,
        sysvar::{
            self,
            instructions::{construct_instructions_data, BorrowedAccountMeta, BorrowedInstruction},
        },
        transaction::{Result, TransactionError},
        transaction_context::{IndexOfAccount, TransactionAccount},
    },
    solana_svm_rent_collector::svm_rent_collector::SVMRentCollector,
    solana_svm_transaction::svm_message::SVMMessage,
    solana_system_program::{get_system_account_kind, SystemAccountKind},
    std::num::NonZeroU32,
};

// for the load instructions
pub(crate) type TransactionRent = u64;
pub(crate) type TransactionProgramIndices = Vec<Vec<IndexOfAccount>>;
pub type TransactionCheckResult = Result<CheckedTransactionDetails>;
pub type TransactionValidationResult = Result<ValidatedTransactionDetails>;

#[derive(PartialEq, Eq, Debug)]
pub enum TransactionLoadResult {
    /// All transaction accounts were loaded successfully
    Loaded(LoadedTransaction),
    /// Some transaction accounts needed for execution were unable to be loaded
    /// but the fee payer and any nonce account needed for fee collection were
    /// loaded successfully
    FeesOnly(FeesOnlyTransaction),
    /// Some transaction accounts needed for fee collection were unable to be
    /// loaded
    NotLoaded(TransactionError),
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct CheckedTransactionDetails {
    pub nonce: Option<NonceInfo>,
    pub lamports_per_signature: u64,
}

#[derive(PartialEq, Eq, Debug, Clone)]
#[cfg_attr(feature = "dev-context-only-utils", derive(Default))]
pub struct ValidatedTransactionDetails {
    pub rollback_accounts: RollbackAccounts,
    pub compute_budget_limits: ComputeBudgetLimits,
    pub fee_details: FeeDetails,
    pub loaded_fee_payer_account: LoadedTransactionAccount,
}

#[derive(PartialEq, Eq, Debug, Clone)]
#[cfg_attr(feature = "dev-context-only-utils", derive(Default))]
pub struct LoadedTransactionAccount {
    pub(crate) account: AccountSharedData,
    pub(crate) loaded_size: usize,
    pub(crate) rent_collected: u64,
}

#[derive(PartialEq, Eq, Debug, Clone)]
#[cfg_attr(feature = "dev-context-only-utils", derive(Default))]
pub struct LoadedTransaction {
    pub accounts: Vec<TransactionAccount>,
    pub program_indices: TransactionProgramIndices,
    pub fee_details: FeeDetails,
    pub rollback_accounts: RollbackAccounts,
    pub compute_budget_limits: ComputeBudgetLimits,
    pub rent: TransactionRent,
    pub rent_debits: RentDebits,
    pub loaded_accounts_data_size: u32,
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct FeesOnlyTransaction {
    pub load_error: TransactionError,
    pub rollback_accounts: RollbackAccounts,
    pub fee_details: FeeDetails,
}

/// Collect rent from an account if rent is still enabled and regardless of
/// whether rent is enabled, set the rent epoch to u64::MAX if the account is
/// rent exempt.
pub fn collect_rent_from_account(
    feature_set: &FeatureSet,
    rent_collector: &dyn SVMRentCollector,
    address: &Pubkey,
    account: &mut AccountSharedData,
) -> CollectedInfo {
    if !feature_set.is_active(&feature_set::disable_rent_fees_collection::id()) {
        rent_collector.collect_rent(address, account)
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
    rent_collector: &dyn SVMRentCollector,
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
            rent_collector
                .get_rent()
                .minimum_balance(NonceState::size())
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

    let payer_pre_rent_state = rent_collector.get_account_rent_state(payer_account);
    payer_account
        .checked_sub_lamports(fee)
        .map_err(|_| TransactionError::InsufficientFundsForFee)?;

    let payer_post_rent_state = rent_collector.get_account_rent_state(payer_account);
    rent_collector.check_rent_state_with_account(
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
    txs: &[impl SVMMessage],
    validation_results: Vec<TransactionValidationResult>,
    error_metrics: &mut TransactionErrorMetrics,
    account_overrides: Option<&AccountOverrides>,
    feature_set: &FeatureSet,
    rent_collector: &dyn SVMRentCollector,
    loaded_programs: &ProgramCacheForTxBatch,
) -> Vec<TransactionLoadResult> {
    txs.iter()
        .zip(validation_results)
        .map(|(transaction, validation_result)| {
            load_transaction(
                callbacks,
                transaction,
                validation_result,
                error_metrics,
                account_overrides,
                feature_set,
                rent_collector,
                loaded_programs,
            )
        })
        .collect()
}

fn load_transaction<CB: TransactionProcessingCallback>(
    callbacks: &CB,
    message: &impl SVMMessage,
    validation_result: TransactionValidationResult,
    error_metrics: &mut TransactionErrorMetrics,
    account_overrides: Option<&AccountOverrides>,
    feature_set: &FeatureSet,
    rent_collector: &dyn SVMRentCollector,
    loaded_programs: &ProgramCacheForTxBatch,
) -> TransactionLoadResult {
    match validation_result {
        Err(e) => TransactionLoadResult::NotLoaded(e),
        Ok(tx_details) => {
            let load_result = load_transaction_accounts(
                callbacks,
                message,
                tx_details.loaded_fee_payer_account,
                &tx_details.compute_budget_limits,
                error_metrics,
                account_overrides,
                feature_set,
                rent_collector,
                loaded_programs,
            );

            match load_result {
                Ok(loaded_tx_accounts) => TransactionLoadResult::Loaded(LoadedTransaction {
                    accounts: loaded_tx_accounts.accounts,
                    program_indices: loaded_tx_accounts.program_indices,
                    fee_details: tx_details.fee_details,
                    rent: loaded_tx_accounts.rent,
                    rent_debits: loaded_tx_accounts.rent_debits,
                    rollback_accounts: tx_details.rollback_accounts,
                    compute_budget_limits: tx_details.compute_budget_limits,
                    loaded_accounts_data_size: loaded_tx_accounts.loaded_accounts_data_size,
                }),
                Err(err) => TransactionLoadResult::FeesOnly(FeesOnlyTransaction {
                    load_error: err,
                    fee_details: tx_details.fee_details,
                    rollback_accounts: tx_details.rollback_accounts,
                }),
            }
        }
    }
}

#[derive(PartialEq, Eq, Debug, Clone)]
struct LoadedTransactionAccounts {
    pub accounts: Vec<TransactionAccount>,
    pub program_indices: TransactionProgramIndices,
    pub rent: TransactionRent,
    pub rent_debits: RentDebits,
    pub loaded_accounts_data_size: u32,
}

fn load_transaction_accounts<CB: TransactionProcessingCallback>(
    callbacks: &CB,
    message: &impl SVMMessage,
    loaded_fee_payer_account: LoadedTransactionAccount,
    compute_budget_limits: &ComputeBudgetLimits,
    error_metrics: &mut TransactionErrorMetrics,
    account_overrides: Option<&AccountOverrides>,
    feature_set: &FeatureSet,
    rent_collector: &dyn SVMRentCollector,
    loaded_programs: &ProgramCacheForTxBatch,
) -> Result<LoadedTransactionAccounts> {
    let mut tx_rent: TransactionRent = 0;
    let account_keys = message.account_keys();
    let mut accounts = Vec::with_capacity(account_keys.len());
    let mut accounts_found = Vec::with_capacity(account_keys.len());
    let mut rent_debits = RentDebits::default();
    let mut accumulated_accounts_data_size: u32 = 0;

    let instruction_accounts = message
        .instructions_iter()
        .flat_map(|instruction| instruction.accounts)
        .unique()
        .collect::<Vec<&u8>>();

    let mut collect_loaded_account = |key, (loaded_account, found)| -> Result<()> {
        let LoadedTransactionAccount {
            account,
            loaded_size,
            rent_collected,
        } = loaded_account;

        accumulate_and_check_loaded_account_data_size(
            &mut accumulated_accounts_data_size,
            loaded_size,
            compute_budget_limits.loaded_accounts_bytes,
            error_metrics,
        )?;

        tx_rent += rent_collected;
        rent_debits.insert(key, rent_collected, account.lamports());

        accounts.push((*key, account));
        accounts_found.push(found);
        Ok(())
    };

    // Since the fee payer is always the first account, collect it first. Note
    // that account overrides are already applied during fee payer validation so
    // it's fine to use the fee payer directly here rather than checking account
    // overrides again.
    collect_loaded_account(message.fee_payer(), (loaded_fee_payer_account, true))?;

    // Attempt to load and collect remaining non-fee payer accounts
    for (account_index, account_key) in account_keys.iter().enumerate().skip(1) {
        let (loaded_account, account_found) = load_transaction_account(
            callbacks,
            message,
            account_key,
            account_index,
            &instruction_accounts[..],
            account_overrides,
            feature_set,
            rent_collector,
            loaded_programs,
        )?;
        collect_loaded_account(account_key, (loaded_account, account_found))?;
    }

    let builtins_start_index = accounts.len();
    let program_indices = message
        .instructions_iter()
        .map(|instruction| {
            let mut account_indices = Vec::with_capacity(2);
            let program_index = instruction.program_id_index as usize;
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
            if !accounts
                .get(builtins_start_index..)
                .ok_or(TransactionError::ProgramAccountNotFound)?
                .iter()
                .any(|(key, _)| key == owner_id)
            {
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
                        compute_budget_limits.loaded_accounts_bytes,
                        error_metrics,
                    )?;
                    accounts.push((*owner_id, owner_account));
                } else {
                    error_metrics.account_not_found += 1;
                    return Err(TransactionError::ProgramAccountNotFound);
                }
            }
            Ok(account_indices)
        })
        .collect::<Result<Vec<Vec<IndexOfAccount>>>>()?;

    Ok(LoadedTransactionAccounts {
        accounts,
        program_indices,
        rent: tx_rent,
        rent_debits,
        loaded_accounts_data_size: accumulated_accounts_data_size,
    })
}

fn load_transaction_account<CB: TransactionProcessingCallback>(
    callbacks: &CB,
    message: &impl SVMMessage,
    account_key: &Pubkey,
    account_index: usize,
    instruction_accounts: &[&u8],
    account_overrides: Option<&AccountOverrides>,
    feature_set: &FeatureSet,
    rent_collector: &dyn SVMRentCollector,
    loaded_programs: &ProgramCacheForTxBatch,
) -> Result<(LoadedTransactionAccount, bool)> {
    let mut account_found = true;
    let is_instruction_account = u8::try_from(account_index)
        .map(|i| instruction_accounts.contains(&&i))
        .unwrap_or(false);
    let is_writable = message.is_writable(account_index);
    let loaded_account = if solana_sdk::sysvar::instructions::check_id(account_key) {
        // Since the instructions sysvar is constructed by the SVM and modified
        // for each transaction instruction, it cannot be overridden.
        LoadedTransactionAccount {
            loaded_size: 0,
            account: construct_instructions_account(message),
            rent_collected: 0,
        }
    } else if let Some(account_override) =
        account_overrides.and_then(|overrides| overrides.get(account_key))
    {
        LoadedTransactionAccount {
            loaded_size: account_override.data().len(),
            account: account_override.clone(),
            rent_collected: 0,
        }
    } else if let Some(program) = (!is_instruction_account && !is_writable)
        .then_some(())
        .and_then(|_| loaded_programs.find(account_key))
    {
        callbacks
            .get_account_shared_data(account_key)
            .ok_or(TransactionError::AccountNotFound)?;
        // Optimization to skip loading of accounts which are only used as
        // programs in top-level instructions and not passed as instruction accounts.
        LoadedTransactionAccount {
            loaded_size: program.account_size,
            account: account_shared_data_from_program(&program),
            rent_collected: 0,
        }
    } else {
        callbacks
            .get_account_shared_data(account_key)
            .map(|mut account| {
                // Inspect the account prior to collecting rent, since
                // rent collection can modify the account.
                callbacks.inspect_account(account_key, AccountState::Alive(&account), is_writable);

                let rent_collected = if is_writable {
                    collect_rent_from_account(
                        feature_set,
                        rent_collector,
                        account_key,
                        &mut account,
                    )
                    .rent_amount
                } else {
                    0
                };

                LoadedTransactionAccount {
                    loaded_size: account.data().len(),
                    account,
                    rent_collected,
                }
            })
            .unwrap_or_else(|| {
                callbacks.inspect_account(account_key, AccountState::Dead, is_writable);

                account_found = false;
                let mut default_account = AccountSharedData::default();

                // All new accounts must be rent-exempt (enforced in Bank::execute_loaded_transaction).
                // Currently, rent collection sets rent_epoch to u64::MAX, but initializing the account
                // with this field already set would allow us to skip rent collection for these accounts.
                default_account.set_rent_epoch(RENT_EXEMPT_RENT_EPOCH);
                LoadedTransactionAccount {
                    loaded_size: default_account.data().len(),
                    account: default_account,
                    rent_collected: 0,
                }
            })
    };

    Ok((loaded_account, account_found))
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
/// `accumulated_accounts_data_size` exceeds
/// `requested_loaded_accounts_data_size_limit`.
fn accumulate_and_check_loaded_account_data_size(
    accumulated_loaded_accounts_data_size: &mut u32,
    account_data_size: usize,
    requested_loaded_accounts_data_size_limit: NonZeroU32,
    error_metrics: &mut TransactionErrorMetrics,
) -> Result<()> {
    let Ok(account_data_size) = u32::try_from(account_data_size) else {
        error_metrics.max_loaded_accounts_data_size_exceeded += 1;
        return Err(TransactionError::MaxLoadedAccountsDataSizeExceeded);
    };
    saturating_add_assign!(*accumulated_loaded_accounts_data_size, account_data_size);
    if *accumulated_loaded_accounts_data_size > requested_loaded_accounts_data_size_limit.get() {
        error_metrics.max_loaded_accounts_data_size_exceeded += 1;
        Err(TransactionError::MaxLoadedAccountsDataSizeExceeded)
    } else {
        Ok(())
    }
}

fn construct_instructions_account(message: &impl SVMMessage) -> AccountSharedData {
    let account_keys = message.account_keys();
    let mut decompiled_instructions = Vec::with_capacity(message.num_instructions());
    for (program_id, instruction) in message.program_instructions_iter() {
        let accounts = instruction
            .accounts
            .iter()
            .map(|account_index| {
                let account_index = usize::from(*account_index);
                BorrowedAccountMeta {
                    is_signer: message.is_signer(account_index),
                    is_writable: message.is_writable(account_index),
                    pubkey: account_keys.get(account_index).unwrap(),
                }
            })
            .collect();

        decompiled_instructions.push(BorrowedInstruction {
            accounts,
            data: instruction.data,
            program_id,
        });
    }

    AccountSharedData::from(Account {
        data: construct_instructions_data(&decompiled_instructions),
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
        solana_compute_budget::{compute_budget::ComputeBudget, compute_budget_limits},
        solana_feature_set::FeatureSet,
        solana_program_runtime::loaded_programs::{
            ProgramCacheEntry, ProgramCacheEntryOwner, ProgramCacheForTxBatch,
        },
        solana_sdk::{
            account::{Account, AccountSharedData, ReadableAccount, WritableAccount},
            bpf_loader, bpf_loader_upgradeable,
            epoch_schedule::EpochSchedule,
            hash::Hash,
            instruction::{AccountMeta, CompiledInstruction, Instruction},
            message::{
                v0::{LoadedAddresses, LoadedMessage},
                LegacyMessage, Message, MessageHeader, SanitizedMessage,
            },
            native_loader,
            native_token::{sol_to_lamports, LAMPORTS_PER_SOL},
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
        std::{borrow::Cow, cell::RefCell, collections::HashMap, sync::Arc},
    };

    #[derive(Default)]
    struct TestCallbacks {
        accounts_map: HashMap<Pubkey, AccountSharedData>,
        #[allow(clippy::type_complexity)]
        inspected_accounts:
            RefCell<HashMap<Pubkey, Vec<(Option<AccountSharedData>, /* is_writable */ bool)>>>,
    }

    impl TransactionProcessingCallback for TestCallbacks {
        fn account_matches_owners(&self, _account: &Pubkey, _owners: &[Pubkey]) -> Option<usize> {
            None
        }

        fn get_account_shared_data(&self, pubkey: &Pubkey) -> Option<AccountSharedData> {
            self.accounts_map.get(pubkey).cloned()
        }

        fn inspect_account(
            &self,
            address: &Pubkey,
            account_state: AccountState,
            is_writable: bool,
        ) {
            let account = match account_state {
                AccountState::Dead => None,
                AccountState::Alive(account) => Some(account.clone()),
            };
            self.inspected_accounts
                .borrow_mut()
                .entry(*address)
                .or_default()
                .push((account, is_writable));
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
        let callbacks = TestCallbacks {
            accounts_map,
            ..Default::default()
        };
        load_accounts(
            &callbacks,
            &[sanitized_tx],
            vec![Ok(ValidatedTransactionDetails {
                loaded_fee_payer_account: LoadedTransactionAccount {
                    account: fee_payer_account,
                    ..LoadedTransactionAccount::default()
                },
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

    fn new_unchecked_sanitized_transaction_with_writable_program(
        program_id: Pubkey,
        fee_payer: Pubkey,
    ) -> SanitizedTransaction {
        let mut message = Message::new(
            &[Instruction::new_with_bytes(program_id, &[], vec![])],
            Some(&fee_payer),
        );
        message.header.num_readonly_unsigned_accounts = 0;

        let legacy_message = LegacyMessage {
            message: Cow::Owned(message),
            is_writable_account_cache: vec![true, true],
        };

        SanitizedTransaction::new_for_tests(
            SanitizedMessage::Legacy(legacy_message),
            vec![Signature::default()],
            false,
        )
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

        let load_results = load_accounts_aux_test(tx, &accounts, &mut error_metrics);

        assert_eq!(error_metrics.account_not_found, 1);
        assert_eq!(load_results.len(), 1);
        assert!(matches!(
            load_results[0],
            TransactionLoadResult::FeesOnly(FeesOnlyTransaction {
                load_error: TransactionError::ProgramAccountNotFound,
                ..
            }),
        ));
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
            TransactionLoadResult::Loaded(loaded_transaction) => {
                assert_eq!(loaded_transaction.accounts.len(), 3);
                assert_eq!(loaded_transaction.accounts[0].1, accounts[0].1);
                assert_eq!(loaded_transaction.program_indices.len(), 1);
                assert_eq!(loaded_transaction.program_indices[0].len(), 0);
            }
            TransactionLoadResult::FeesOnly(fees_only_tx) => panic!("{}", fees_only_tx.load_error),
            TransactionLoadResult::NotLoaded(e) => panic!("{e}"),
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

        let load_results = load_accounts_aux_test(tx, &accounts, &mut error_metrics);

        assert_eq!(error_metrics.account_not_found, 1);
        assert_eq!(load_results.len(), 1);
        assert!(matches!(
            load_results[0],
            TransactionLoadResult::FeesOnly(FeesOnlyTransaction {
                load_error: TransactionError::ProgramAccountNotFound,
                ..
            }),
        ));
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

        let load_results = load_accounts_aux_test(tx, &accounts, &mut error_metrics);

        assert_eq!(error_metrics.invalid_program_for_execution, 1);
        assert_eq!(load_results.len(), 1);
        assert!(matches!(
            load_results[0],
            TransactionLoadResult::FeesOnly(FeesOnlyTransaction {
                load_error: TransactionError::InvalidProgramForExecution,
                ..
            }),
        ));
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
            TransactionLoadResult::Loaded(loaded_transaction) => {
                assert_eq!(loaded_transaction.accounts.len(), 4);
                assert_eq!(loaded_transaction.accounts[0].1, accounts[0].1);
                assert_eq!(loaded_transaction.program_indices.len(), 2);
                assert_eq!(loaded_transaction.program_indices[0], &[1]);
                assert_eq!(loaded_transaction.program_indices[1], &[2]);
            }
            TransactionLoadResult::FeesOnly(fees_only_tx) => panic!("{}", fees_only_tx.load_error),
            TransactionLoadResult::NotLoaded(e) => panic!("{e}"),
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
        let callbacks = TestCallbacks {
            accounts_map,
            ..Default::default()
        };
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

        let load_results = load_accounts_no_store(&[], tx, None);
        assert_eq!(load_results.len(), 1);
        assert!(matches!(
            load_results[0],
            TransactionLoadResult::FeesOnly(FeesOnlyTransaction {
                load_error: TransactionError::ProgramAccountNotFound,
                ..
            }),
        ));
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
        match &loaded_accounts[0] {
            TransactionLoadResult::Loaded(loaded_transaction) => {
                assert_eq!(loaded_transaction.accounts[0].0, keypair.pubkey());
                assert_eq!(loaded_transaction.accounts[1].0, slot_history_id);
                assert_eq!(loaded_transaction.accounts[1].1.lamports(), 42);
            }
            TransactionLoadResult::FeesOnly(fees_only_tx) => panic!("{}", fees_only_tx.load_error),
            TransactionLoadResult::NotLoaded(e) => panic!("{e}"),
        }
    }

    #[test]
    fn test_accumulate_and_check_loaded_account_data_size() {
        let mut error_metrics = TransactionErrorMetrics::default();
        let mut accumulated_data_size: u32 = 0;
        let data_size: usize = 123;
        let requested_data_size_limit = NonZeroU32::new(data_size as u32).unwrap();

        // OK - loaded data size is up to limit
        assert!(accumulate_and_check_loaded_account_data_size(
            &mut accumulated_data_size,
            data_size,
            requested_data_size_limit,
            &mut error_metrics
        )
        .is_ok());
        assert_eq!(data_size as u32, accumulated_data_size);

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
        let mut fee_payer_account = AccountSharedData::default();
        fee_payer_account.set_lamports(fee_payer_balance);
        mock_bank
            .accounts_map
            .insert(fee_payer_address, fee_payer_account.clone());
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
            LoadedTransactionAccount {
                loaded_size: fee_payer_account.data().len(),
                account: fee_payer_account.clone(),
                rent_collected: fee_payer_rent_debit,
            },
            &ComputeBudgetLimits::default(),
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
            LoadedTransactionAccounts {
                accounts: vec![(fee_payer_address, fee_payer_account)],
                program_indices: vec![],
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
        let mut fee_payer_account = AccountSharedData::default();
        fee_payer_account.set_lamports(200);
        mock_bank
            .accounts_map
            .insert(key1.pubkey(), fee_payer_account.clone());

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
            LoadedTransactionAccount {
                account: fee_payer_account.clone(),
                ..LoadedTransactionAccount::default()
            },
            &ComputeBudgetLimits::default(),
            &mut error_metrics,
            None,
            &FeatureSet::default(),
            &RentCollector::default(),
            &loaded_programs,
        );

        assert_eq!(
            result.unwrap(),
            LoadedTransactionAccounts {
                accounts: vec![
                    (key1.pubkey(), fee_payer_account),
                    (
                        native_loader::id(),
                        mock_bank.accounts_map[&native_loader::id()].clone()
                    )
                ],
                program_indices: vec![vec![]],
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
            LoadedTransactionAccount::default(),
            &ComputeBudgetLimits::default(),
            &mut error_metrics,
            None,
            &FeatureSet::default(),
            &RentCollector::default(),
            &loaded_programs,
        );

        assert_eq!(result.err(), Some(TransactionError::AccountNotFound));
    }

    #[test]
    fn test_load_transaction_accounts_program_account_executable_bypass() {
        // currently, the account loader retrieves read-only non-instruction accounts from the program cache
        // it creates a mock AccountSharedData with the executable flag set to true
        // however, it does not check whether these accounts are actually executable before doing so
        // this affects consensus: a transaction that uses a cached non-executable program executes and fails
        // but if the transaction gets the program from accounts-db, it will be dropped during account loading
        // this test enforces the current behavior, so that future account loader changes do not break consensus

        let mut mock_bank = TestCallbacks::default();
        let account_keypair = Keypair::new();
        let program_keypair = Keypair::new();

        let mut account_data = AccountSharedData::default();
        account_data.set_lamports(200);
        mock_bank
            .accounts_map
            .insert(account_keypair.pubkey(), account_data.clone());

        let mut program_data = AccountSharedData::default();
        program_data.set_lamports(200);
        program_data.set_owner(bpf_loader::id());
        mock_bank
            .accounts_map
            .insert(program_keypair.pubkey(), program_data);

        let mut loader_data = AccountSharedData::default();
        loader_data.set_lamports(200);
        loader_data.set_executable(true);
        loader_data.set_owner(native_loader::id());
        mock_bank
            .accounts_map
            .insert(bpf_loader::id(), loader_data.clone());
        mock_bank
            .accounts_map
            .insert(native_loader::id(), loader_data);

        let mut error_metrics = TransactionErrorMetrics::default();
        let mut loaded_programs = ProgramCacheForTxBatch::default();

        let transaction =
            SanitizedTransaction::from_transaction_for_tests(Transaction::new_signed_with_payer(
                &[Instruction::new_with_bytes(
                    program_keypair.pubkey(),
                    &[],
                    vec![],
                )],
                Some(&account_keypair.pubkey()),
                &[&account_keypair],
                Hash::default(),
            ));

        let result = load_transaction_accounts(
            &mock_bank,
            transaction.message(),
            LoadedTransactionAccount {
                account: account_data.clone(),
                ..LoadedTransactionAccount::default()
            },
            &ComputeBudgetLimits::default(),
            &mut error_metrics,
            None,
            &FeatureSet::default(),
            &RentCollector::default(),
            &loaded_programs,
        );

        // without cache, program is invalid
        assert_eq!(
            result.err(),
            Some(TransactionError::InvalidProgramForExecution)
        );

        loaded_programs.replenish(
            program_keypair.pubkey(),
            Arc::new(ProgramCacheEntry::default()),
        );

        let result = load_transaction_accounts(
            &mock_bank,
            transaction.message(),
            LoadedTransactionAccount {
                account: account_data.clone(),
                ..LoadedTransactionAccount::default()
            },
            &ComputeBudgetLimits::default(),
            &mut error_metrics,
            None,
            &FeatureSet::default(),
            &RentCollector::default(),
            &loaded_programs,
        );

        // with cache, executable flag is bypassed
        let mut cached_program = AccountSharedData::default();
        cached_program.set_owner(native_loader::id());
        cached_program.set_executable(true);

        assert_eq!(
            result.unwrap(),
            LoadedTransactionAccounts {
                accounts: vec![
                    (account_keypair.pubkey(), account_data.clone()),
                    (program_keypair.pubkey(), cached_program),
                ],
                program_indices: vec![vec![1]],
                rent: 0,
                rent_debits: RentDebits::default(),
                loaded_accounts_data_size: 0,
            }
        );

        let transaction =
            SanitizedTransaction::from_transaction_for_tests(Transaction::new_signed_with_payer(
                &[Instruction::new_with_bytes(
                    program_keypair.pubkey(),
                    &[],
                    vec![AccountMeta::new_readonly(program_keypair.pubkey(), false)],
                )],
                Some(&account_keypair.pubkey()),
                &[&account_keypair],
                Hash::default(),
            ));

        let result = load_transaction_accounts(
            &mock_bank,
            transaction.message(),
            LoadedTransactionAccount {
                account: account_data.clone(),
                ..LoadedTransactionAccount::default()
            },
            &ComputeBudgetLimits::default(),
            &mut error_metrics,
            None,
            &FeatureSet::default(),
            &RentCollector::default(),
            &loaded_programs,
        );

        // including program as instruction account bypasses executable bypass
        assert_eq!(
            result.err(),
            Some(TransactionError::InvalidProgramForExecution)
        );

        let transaction = new_unchecked_sanitized_transaction_with_writable_program(
            program_keypair.pubkey(),
            account_keypair.pubkey(),
        );

        let result = load_transaction_accounts(
            &mock_bank,
            transaction.message(),
            LoadedTransactionAccount {
                account: account_data.clone(),
                ..LoadedTransactionAccount::default()
            },
            &ComputeBudgetLimits::default(),
            &mut error_metrics,
            None,
            &FeatureSet::default(),
            &RentCollector::default(),
            &loaded_programs,
        );

        // including program as writable bypasses executable bypass
        assert_eq!(
            result.err(),
            Some(TransactionError::InvalidProgramForExecution)
        );
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
            LoadedTransactionAccount::default(),
            &ComputeBudgetLimits::default(),
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
            LoadedTransactionAccount::default(),
            &ComputeBudgetLimits::default(),
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

        let mut fee_payer_account = AccountSharedData::default();
        fee_payer_account.set_lamports(200);
        mock_bank
            .accounts_map
            .insert(key2.pubkey(), fee_payer_account.clone());
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
            LoadedTransactionAccount {
                account: fee_payer_account.clone(),
                ..LoadedTransactionAccount::default()
            },
            &ComputeBudgetLimits::default(),
            &mut error_metrics,
            None,
            &FeatureSet::default(),
            &RentCollector::default(),
            &loaded_programs,
        );

        assert_eq!(
            result.unwrap(),
            LoadedTransactionAccounts {
                accounts: vec![
                    (key2.pubkey(), fee_payer_account),
                    (
                        key1.pubkey(),
                        mock_bank.accounts_map[&key1.pubkey()].clone()
                    ),
                ],
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
            LoadedTransactionAccount::default(),
            &ComputeBudgetLimits::default(),
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
            LoadedTransactionAccount::default(),
            &ComputeBudgetLimits::default(),
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

        let mut fee_payer_account = AccountSharedData::default();
        fee_payer_account.set_lamports(200);
        mock_bank
            .accounts_map
            .insert(key2.pubkey(), fee_payer_account.clone());

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
            LoadedTransactionAccount {
                account: fee_payer_account.clone(),
                ..LoadedTransactionAccount::default()
            },
            &ComputeBudgetLimits::default(),
            &mut error_metrics,
            None,
            &FeatureSet::default(),
            &RentCollector::default(),
            &loaded_programs,
        );

        assert_eq!(
            result.unwrap(),
            LoadedTransactionAccounts {
                accounts: vec![
                    (key2.pubkey(), fee_payer_account),
                    (
                        key1.pubkey(),
                        mock_bank.accounts_map[&key1.pubkey()].clone()
                    ),
                    (
                        key3.pubkey(),
                        mock_bank.accounts_map[&key3.pubkey()].clone()
                    ),
                ],
                program_indices: vec![vec![1]],
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

        let mut fee_payer_account = AccountSharedData::default();
        fee_payer_account.set_lamports(200);
        mock_bank
            .accounts_map
            .insert(key2.pubkey(), fee_payer_account.clone());

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
            LoadedTransactionAccount {
                account: fee_payer_account.clone(),
                ..LoadedTransactionAccount::default()
            },
            &ComputeBudgetLimits::default(),
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
            LoadedTransactionAccounts {
                accounts: vec![
                    (key2.pubkey(), fee_payer_account),
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
                program_indices: vec![vec![1], vec![1]],
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
        let mut load_results = load_accounts(
            &bank,
            &[sanitized_tx.clone()],
            vec![Ok(ValidatedTransactionDetails::default())],
            &mut error_metrics,
            None,
            &FeatureSet::default(),
            &RentCollector::default(),
            &ProgramCacheForTxBatch::default(),
        );

        let TransactionLoadResult::Loaded(loaded_transaction) = load_results.swap_remove(0) else {
            panic!("transaction loading failed");
        };

        let compute_budget = ComputeBudget::new(u64::from(
            compute_budget_limits::DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT,
        ));
        let rent_collector = RentCollector::default();
        let transaction_context = TransactionContext::new(
            loaded_transaction.accounts,
            rent_collector.get_rent().clone(),
            compute_budget.max_instruction_stack_depth,
            compute_budget.max_instruction_trace_length,
        );

        assert_eq!(
            TransactionAccountStateInfo::new(
                &transaction_context,
                sanitized_tx.message(),
                &rent_collector,
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

        let mut fee_payer_account = AccountSharedData::default();
        fee_payer_account.set_lamports(200);
        mock_bank
            .accounts_map
            .insert(key2.pubkey(), fee_payer_account.clone());

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
            loaded_fee_payer_account: LoadedTransactionAccount {
                account: fee_payer_account,
                ..LoadedTransactionAccount::default()
            },
            ..ValidatedTransactionDetails::default()
        });

        let mut load_results = load_accounts(
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

        assert_eq!(load_results.len(), 1);
        let TransactionLoadResult::Loaded(loaded_transaction) = load_results.swap_remove(0) else {
            panic!("transaction loading failed");
        };
        assert_eq!(
            loaded_transaction,
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
                program_indices: vec![vec![1], vec![1]],
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
        let load_results = load_accounts(
            &mock_bank,
            &[sanitized_transaction.clone()],
            vec![validation_result.clone()],
            &mut TransactionErrorMetrics::default(),
            None,
            &feature_set,
            &rent_collector,
            &ProgramCacheForTxBatch::default(),
        );

        assert!(matches!(
            load_results[0],
            TransactionLoadResult::FeesOnly(FeesOnlyTransaction {
                load_error: TransactionError::InvalidProgramForExecution,
                ..
            }),
        ));

        let validation_result = Err(TransactionError::InvalidWritableAccount);

        let load_results = load_accounts(
            &mock_bank,
            &[sanitized_transaction.clone()],
            vec![validation_result],
            &mut TransactionErrorMetrics::default(),
            None,
            &feature_set,
            &rent_collector,
            &ProgramCacheForTxBatch::default(),
        );

        assert!(matches!(
            load_results[0],
            TransactionLoadResult::NotLoaded(TransactionError::InvalidWritableAccount),
        ));
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

    // Ensure `TransactionProcessingCallback::inspect_account()` is called when
    // loading accounts for transaction processing.
    #[test]
    fn test_inspect_account_non_fee_payer() {
        let mut mock_bank = TestCallbacks::default();

        let address0 = Pubkey::new_unique(); // <-- fee payer
        let address1 = Pubkey::new_unique(); // <-- initially alive
        let address2 = Pubkey::new_unique(); // <-- initially dead
        let address3 = Pubkey::new_unique(); // <-- program

        let mut account0 = AccountSharedData::default();
        account0.set_lamports(1_000_000_000);
        mock_bank.accounts_map.insert(address0, account0.clone());

        let mut account1 = AccountSharedData::default();
        account1.set_lamports(2_000_000_000);
        mock_bank.accounts_map.insert(address1, account1.clone());

        // account2 *not* added to the bank's accounts_map

        let mut account3 = AccountSharedData::default();
        account3.set_lamports(4_000_000_000);
        account3.set_executable(true);
        account3.set_owner(native_loader::id());
        mock_bank.accounts_map.insert(address3, account3.clone());

        let message = Message {
            account_keys: vec![address0, address1, address2, address3],
            header: MessageHeader::default(),
            instructions: vec![
                CompiledInstruction {
                    program_id_index: 3,
                    accounts: vec![0],
                    data: vec![],
                },
                CompiledInstruction {
                    program_id_index: 3,
                    accounts: vec![1, 2],
                    data: vec![],
                },
                CompiledInstruction {
                    program_id_index: 3,
                    accounts: vec![1],
                    data: vec![],
                },
            ],
            recent_blockhash: Hash::new_unique(),
        };
        let sanitized_message = new_unchecked_sanitized_message(message);
        let sanitized_transaction = SanitizedTransaction::new_for_tests(
            sanitized_message,
            vec![Signature::new_unique()],
            false,
        );
        let validation_result = Ok(ValidatedTransactionDetails {
            loaded_fee_payer_account: LoadedTransactionAccount {
                account: account0.clone(),
                ..LoadedTransactionAccount::default()
            },
            ..ValidatedTransactionDetails::default()
        });
        let _load_results = load_accounts(
            &mock_bank,
            &[sanitized_transaction],
            vec![validation_result],
            &mut TransactionErrorMetrics::default(),
            None,
            &FeatureSet::default(),
            &RentCollector::default(),
            &ProgramCacheForTxBatch::default(),
        );

        // ensure the loaded accounts are inspected
        let mut actual_inspected_accounts: Vec<_> = mock_bank
            .inspected_accounts
            .borrow()
            .iter()
            .map(|(k, v)| (*k, v.clone()))
            .collect();
        actual_inspected_accounts.sort_unstable_by(|a, b| a.0.cmp(&b.0));

        let mut expected_inspected_accounts = vec![
            // *not* key0, since it is loaded during fee payer validation
            (address1, vec![(Some(account1), true)]),
            (address2, vec![(None, true)]),
            (address3, vec![(Some(account3), false)]),
        ];
        expected_inspected_accounts.sort_unstable_by(|a, b| a.0.cmp(&b.0));

        assert_eq!(actual_inspected_accounts, expected_inspected_accounts,);
    }

    #[test]
    fn test_load_transaction_accounts_data_sizes() {
        let mut mock_bank = TestCallbacks::default();

        let mut next_size = 1;
        let mut make_account = |pubkey, owner, executable| {
            let size = next_size;
            let account = AccountSharedData::create(
                LAMPORTS_PER_SOL,
                vec![0; size],
                owner,
                executable,
                u64::MAX,
            );

            mock_bank.accounts_map.insert(pubkey, account.clone());

            // accounts are counted at most twice
            // by multiplying account size by 4, we ensure all totals are unique
            next_size *= 4;

            (size as u32, account)
        };

        let (native_loader_size, _) = make_account(native_loader::id(), native_loader::id(), true);
        let (bpf_loader_size, _) = make_account(bpf_loader::id(), native_loader::id(), true);
        let (upgradeable_loader_size, _) =
            make_account(bpf_loader_upgradeable::id(), native_loader::id(), true);

        let program1_keypair = Keypair::new();
        let program1 = program1_keypair.pubkey();
        let (program1_size, _) = make_account(program1, bpf_loader::id(), true);

        let program2 = Pubkey::new_unique();
        let (program2_size, _) = make_account(program2, bpf_loader_upgradeable::id(), true);

        let programdata2 = Pubkey::new_unique();
        let (programdata2_size, _) =
            make_account(programdata2, bpf_loader_upgradeable::id(), false);

        let fee_payer_keypair = Keypair::new();
        let fee_payer = fee_payer_keypair.pubkey();
        let (fee_payer_size, fee_payer_account) =
            make_account(fee_payer, system_program::id(), false);

        let account1 = Pubkey::new_unique();
        let (account1_size, _) = make_account(account1, program1, false);

        let account2 = Pubkey::new_unique();
        let (account2_size, _) = make_account(account2, program2, false);

        let test_transaction_data_size_with_cache = |transaction, cache, expected_size| {
            let loaded_transaction_accounts = load_transaction_accounts(
                &mock_bank,
                &transaction,
                LoadedTransactionAccount {
                    account: fee_payer_account.clone(),
                    loaded_size: fee_payer_size as usize,
                    rent_collected: 0,
                },
                &ComputeBudgetLimits::default(),
                &mut TransactionErrorMetrics::default(),
                None,
                &FeatureSet::default(),
                &RentCollector::default(),
                &cache,
            )
            .unwrap();

            assert_eq!(
                loaded_transaction_accounts.loaded_accounts_data_size,
                expected_size
            );
        };

        let test_data_size_with_cache = |instructions: Vec<_>, cache, expected_size| {
            let transaction = SanitizedTransaction::from_transaction_for_tests(
                Transaction::new_signed_with_payer(
                    &instructions,
                    Some(&fee_payer),
                    &[&fee_payer_keypair],
                    Hash::default(),
                ),
            );

            test_transaction_data_size_with_cache(transaction, cache, expected_size)
        };

        for account_meta in [AccountMeta::new, AccountMeta::new_readonly] {
            let test_data_size = |instructions, expected_size| {
                test_data_size_with_cache(
                    instructions,
                    ProgramCacheForTxBatch::default(),
                    expected_size,
                )
            };

            // one program plus loader
            let ixns = vec![Instruction::new_with_bytes(program1, &[], vec![])];
            test_data_size(ixns, program1_size + bpf_loader_size + fee_payer_size);

            // two programs, two loaders, two accounts
            let ixns = vec![
                Instruction::new_with_bytes(program1, &[], vec![account_meta(account1, false)]),
                Instruction::new_with_bytes(program2, &[], vec![account_meta(account2, false)]),
            ];
            test_data_size(
                ixns,
                account1_size
                    + account2_size
                    + program1_size
                    + program2_size
                    + bpf_loader_size
                    + upgradeable_loader_size
                    + fee_payer_size,
            );

            // ordinary owners not counted
            let ixns = vec![Instruction::new_with_bytes(
                program1,
                &[],
                vec![account_meta(account2, false)],
            )];
            test_data_size(
                ixns,
                account2_size + program1_size + bpf_loader_size + fee_payer_size,
            );

            // program and loader counted once
            let ixns = vec![
                Instruction::new_with_bytes(program1, &[], vec![]),
                Instruction::new_with_bytes(program1, &[], vec![]),
            ];
            test_data_size(ixns, program1_size + bpf_loader_size + fee_payer_size);

            // native loader not counted if loader
            let ixns = vec![Instruction::new_with_bytes(bpf_loader::id(), &[], vec![])];
            test_data_size(ixns, bpf_loader_size + fee_payer_size);

            // native loader counted if instruction
            let ixns = vec![Instruction::new_with_bytes(
                bpf_loader::id(),
                &[],
                vec![account_meta(native_loader::id(), false)],
            )];
            test_data_size(ixns, bpf_loader_size + native_loader_size + fee_payer_size);

            // native loader counted if invoked
            let ixns = vec![Instruction::new_with_bytes(
                native_loader::id(),
                &[],
                vec![],
            )];
            test_data_size(ixns, native_loader_size + fee_payer_size);

            // native loader counted once if invoked and instruction
            let ixns = vec![Instruction::new_with_bytes(
                native_loader::id(),
                &[],
                vec![account_meta(native_loader::id(), false)],
            )];
            test_data_size(ixns, native_loader_size + fee_payer_size);

            // loader counted twice if included in instruction
            let ixns = vec![Instruction::new_with_bytes(
                program1,
                &[],
                vec![account_meta(bpf_loader::id(), false)],
            )];
            test_data_size(ixns, program1_size + bpf_loader_size * 2 + fee_payer_size);

            // cover that case with multiple loaders to be sure
            let ixns = vec![
                Instruction::new_with_bytes(
                    program1,
                    &[],
                    vec![
                        account_meta(bpf_loader::id(), false),
                        account_meta(bpf_loader_upgradeable::id(), false),
                    ],
                ),
                Instruction::new_with_bytes(program2, &[], vec![account_meta(account1, false)]),
                Instruction::new_with_bytes(
                    bpf_loader_upgradeable::id(),
                    &[],
                    vec![account_meta(account1, false)],
                ),
            ];
            test_data_size(
                ixns,
                account1_size
                    + program1_size
                    + program2_size
                    + bpf_loader_size * 2
                    + upgradeable_loader_size * 2
                    + fee_payer_size,
            );

            // loader counted twice even if included first
            let ixns = vec![
                Instruction::new_with_bytes(bpf_loader::id(), &[], vec![]),
                Instruction::new_with_bytes(program1, &[], vec![]),
            ];
            test_data_size(ixns, program1_size + bpf_loader_size * 2 + fee_payer_size);

            // fee-payer counted once
            let ixns = vec![Instruction::new_with_bytes(
                program1,
                &[],
                vec![account_meta(fee_payer, false)],
            )];
            test_data_size(ixns, program1_size + bpf_loader_size + fee_payer_size);

            // edge cases involving program cache
            let mut program_cache = ProgramCacheForTxBatch::default();

            let program2_entry = ProgramCacheEntry {
                account_size: (program2_size + programdata2_size) as usize,
                account_owner: ProgramCacheEntryOwner::LoaderV3,
                ..ProgramCacheEntry::default()
            };
            program_cache.replenish(program2, Arc::new(program2_entry));

            // normal function call uses the combined cache size
            let ixns = vec![Instruction::new_with_bytes(program2, &[], vec![])];
            test_data_size_with_cache(
                ixns,
                program_cache.clone(),
                program2_size + programdata2_size + upgradeable_loader_size + fee_payer_size,
            );

            // program as instruction account bypasses the cache
            let ixns = vec![Instruction::new_with_bytes(
                program2,
                &[],
                vec![account_meta(program2, false)],
            )];
            test_data_size_with_cache(
                ixns,
                program_cache.clone(),
                program2_size + upgradeable_loader_size + fee_payer_size,
            );

            // programdata as instruction account double-counts it
            let ixns = vec![Instruction::new_with_bytes(
                program2,
                &[],
                vec![account_meta(programdata2, false)],
            )];
            test_data_size_with_cache(
                ixns,
                program_cache.clone(),
                program2_size + programdata2_size * 2 + upgradeable_loader_size + fee_payer_size,
            );

            // both as instruction accounts, for completeness
            let ixns = vec![Instruction::new_with_bytes(
                program2,
                &[],
                vec![
                    account_meta(program2, false),
                    account_meta(programdata2, false),
                ],
            )];
            test_data_size_with_cache(
                ixns,
                program_cache.clone(),
                program2_size + programdata2_size + upgradeable_loader_size + fee_payer_size,
            );

            // writable program bypasses the cache
            let tx = new_unchecked_sanitized_transaction_with_writable_program(program2, fee_payer);
            test_transaction_data_size_with_cache(
                tx,
                program_cache.clone(),
                program2_size + upgradeable_loader_size + fee_payer_size,
            );

            // NOTE for the new loader we *must* also test arbitrary permutations of the cache transactions
            // to ensure that the batched loading is overridden on a tx-per-tx basis
        }
    }
}
