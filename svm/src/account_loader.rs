use {
    crate::{
        account_overrides::AccountOverrides, account_rent_state::RentState,
        transaction_error_metrics::TransactionErrorMetrics,
        transaction_processor::TransactionProcessingCallback,
    },
    itertools::Itertools,
    log::warn,
    solana_program_runtime::{
        compute_budget_processor::process_compute_budget_instructions,
        loaded_programs::LoadedProgramsForTxBatch,
    },
    solana_sdk::{
        account::{Account, AccountSharedData, ReadableAccount, WritableAccount},
        feature_set::{
            self, include_loaded_accounts_data_size_in_fee_calculation,
            remove_rounding_in_fee_calculation,
        },
        fee::FeeStructure,
        message::SanitizedMessage,
        native_loader,
        nonce::State as NonceState,
        nonce_info::{NonceFull, NoncePartial},
        pubkey::Pubkey,
        rent::RentDue,
        rent_collector::{RentCollector, RENT_EXEMPT_RENT_EPOCH},
        rent_debits::RentDebits,
        saturating_add_assign,
        sysvar::{self, instructions::construct_instructions_data},
        transaction::{self, Result, SanitizedTransaction, TransactionError},
        transaction_context::{IndexOfAccount, TransactionAccount},
    },
    solana_system_program::{get_system_account_kind, SystemAccountKind},
    std::{collections::HashMap, num::NonZeroUsize},
};

// for the load instructions
pub(crate) type TransactionRent = u64;
pub(crate) type TransactionProgramIndices = Vec<Vec<IndexOfAccount>>;
pub type TransactionCheckResult = (transaction::Result<()>, Option<NoncePartial>, Option<u64>);
pub type TransactionLoadResult = (Result<LoadedTransaction>, Option<NonceFull>);

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct LoadedTransaction {
    pub accounts: Vec<TransactionAccount>,
    pub program_indices: TransactionProgramIndices,
    pub rent: TransactionRent,
    pub rent_debits: RentDebits,
}

/// Check whether the payer_account is capable of paying the fee. The
/// side effect is to subtract the fee amount from the payer_account
/// balance of lamports. If the payer_acount is not able to pay the
/// fee, the error_counters is incremented, and a specific error is
/// returned.
pub fn validate_fee_payer(
    payer_address: &Pubkey,
    payer_account: &mut AccountSharedData,
    payer_index: IndexOfAccount,
    error_counters: &mut TransactionErrorMetrics,
    rent_collector: &RentCollector,
    fee: u64,
) -> Result<()> {
    if payer_account.lamports() == 0 {
        error_counters.account_not_found += 1;
        return Err(TransactionError::AccountNotFound);
    }
    let system_account_kind = get_system_account_kind(payer_account).ok_or_else(|| {
        error_counters.invalid_account_for_fee += 1;
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
            error_counters.insufficient_funds += 1;
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
    lock_results: &[TransactionCheckResult],
    error_counters: &mut TransactionErrorMetrics,
    fee_structure: &FeeStructure,
    account_overrides: Option<&AccountOverrides>,
    program_accounts: &HashMap<Pubkey, (&Pubkey, u64)>,
    loaded_programs: &LoadedProgramsForTxBatch,
) -> Vec<TransactionLoadResult> {
    let feature_set = callbacks.get_feature_set();
    txs.iter()
        .zip(lock_results)
        .map(|etx| match etx {
            (tx, (Ok(()), nonce, lamports_per_signature)) => {
                let message = tx.message();
                let fee = if let Some(lamports_per_signature) = lamports_per_signature {
                    fee_structure.calculate_fee(
                        message,
                        *lamports_per_signature,
                        &process_compute_budget_instructions(message.program_instructions_iter())
                            .unwrap_or_default()
                            .into(),
                        feature_set
                            .is_active(&include_loaded_accounts_data_size_in_fee_calculation::id()),
                        feature_set.is_active(&remove_rounding_in_fee_calculation::id()),
                    )
                } else {
                    return (Err(TransactionError::BlockhashNotFound), None);
                };

                // load transactions
                let loaded_transaction = match load_transaction_accounts(
                    callbacks,
                    message,
                    fee,
                    error_counters,
                    account_overrides,
                    program_accounts,
                    loaded_programs,
                ) {
                    Ok(loaded_transaction) => loaded_transaction,
                    Err(e) => return (Err(e), None),
                };

                // Update nonce with fee-subtracted accounts
                let nonce = if let Some(nonce) = nonce {
                    match NonceFull::from_partial(
                        nonce,
                        message,
                        &loaded_transaction.accounts,
                        &loaded_transaction.rent_debits,
                    ) {
                        Ok(nonce) => Some(nonce),
                        // This error branch is never reached, because `load_transaction_accounts`
                        // already validates the fee payer account.
                        Err(e) => return (Err(e), None),
                    }
                } else {
                    None
                };

                (Ok(loaded_transaction), nonce)
            }
            (_, (Err(e), _nonce, _lamports_per_signature)) => (Err(e.clone()), None),
        })
        .collect()
}

fn load_transaction_accounts<CB: TransactionProcessingCallback>(
    callbacks: &CB,
    message: &SanitizedMessage,
    fee: u64,
    error_counters: &mut TransactionErrorMetrics,
    account_overrides: Option<&AccountOverrides>,
    program_accounts: &HashMap<Pubkey, (&Pubkey, u64)>,
    loaded_programs: &LoadedProgramsForTxBatch,
) -> Result<LoadedTransaction> {
    let feature_set = callbacks.get_feature_set();

    // There is no way to predict what program will execute without an error
    // If a fee can pay for execution then the program will be scheduled
    let mut validated_fee_payer = false;
    let mut tx_rent: TransactionRent = 0;
    let account_keys = message.account_keys();
    let mut accounts_found = Vec::with_capacity(account_keys.len());
    let mut rent_debits = RentDebits::default();
    let rent_collector = callbacks.get_rent_collector();

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
                let instruction_account = u8::try_from(i)
                    .map(|i| instruction_accounts.contains(&&i))
                    .unwrap_or(false);
                let (account_size, mut account, rent) = if let Some(account_override) =
                    account_overrides.and_then(|overrides| overrides.get(key))
                {
                    (account_override.data().len(), account_override.clone(), 0)
                } else if let Some(program) = (!instruction_account && !message.is_writable(i))
                    .then_some(())
                    .and_then(|_| loaded_programs.find(key))
                {
                    // Optimization to skip loading of accounts which are only used as
                    // programs in top-level instructions and not passed as instruction accounts.
                    account_shared_data_from_program(key, program_accounts)
                        .map(|program_account| (program.account_size, program_account, 0))?
                } else {
                    callbacks
                        .get_account_shared_data(key)
                        .map(|mut account| {
                            if message.is_writable(i) {
                                if !feature_set
                                    .is_active(&feature_set::disable_rent_fees_collection::id())
                                {
                                    let rent_due = rent_collector
                                        .collect_from_existing_account(key, &mut account)
                                        .rent_amount;

                                    (account.data().len(), account, rent_due)
                                } else {
                                    // When rent fee collection is disabled, we won't collect rent for any account. If there
                                    // are any rent paying accounts, their `rent_epoch` won't change either. However, if the
                                    // account itself is rent-exempted but its `rent_epoch` is not u64::MAX, we will set its
                                    // `rent_epoch` to u64::MAX. In such case, the behavior stays the same as before.
                                    if account.rent_epoch() != RENT_EXEMPT_RENT_EPOCH
                                        && rent_collector.get_rent_due(&account) == RentDue::Exempt
                                    {
                                        account.set_rent_epoch(RENT_EXEMPT_RENT_EPOCH);
                                    }
                                    (account.data().len(), account, 0)
                                }
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
                    error_counters,
                )?;

                if !validated_fee_payer && message.is_non_loader_key(i) {
                    if i != 0 {
                        warn!("Payer index should be 0! {:?}", message);
                    }

                    validate_fee_payer(
                        key,
                        &mut account,
                        i as IndexOfAccount,
                        error_counters,
                        rent_collector,
                        fee,
                    )?;

                    validated_fee_payer = true;
                }

                callbacks.check_account_access(message, i, &account, error_counters)?;

                tx_rent += rent;
                rent_debits.insert(key, rent, account.lamports());

                account
            };

            accounts_found.push(account_found);
            Ok((*key, account))
        })
        .collect::<Result<Vec<_>>>()?;

    if !validated_fee_payer {
        error_counters.account_not_found += 1;
        return Err(TransactionError::AccountNotFound);
    }

    let builtins_start_index = accounts.len();
    let program_indices = message
        .instructions()
        .iter()
        .map(|instruction| {
            let mut account_indices = Vec::new();
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
                error_counters.account_not_found += 1;
                return Err(TransactionError::ProgramAccountNotFound);
            }

            if !program_account.executable() {
                error_counters.invalid_program_for_execution += 1;
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
                        error_counters.invalid_program_for_execution += 1;
                        return Err(TransactionError::InvalidProgramForExecution);
                    }
                    accumulate_and_check_loaded_account_data_size(
                        &mut accumulated_accounts_data_size,
                        owner_account.data().len(),
                        requested_loaded_accounts_data_size_limit,
                        error_counters,
                    )?;
                    accounts.push((*owner_id, owner_account));
                } else {
                    error_counters.account_not_found += 1;
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
        rent: tx_rent,
        rent_debits,
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

fn account_shared_data_from_program(
    key: &Pubkey,
    program_accounts: &HashMap<Pubkey, (&Pubkey, u64)>,
) -> Result<AccountSharedData> {
    // It's an executable program account. The program is already loaded in the cache.
    // So the account data is not needed. Return a dummy AccountSharedData with meta
    // information.
    let mut program_account = AccountSharedData::default();
    let (program_owner, _count) = program_accounts
        .get(key)
        .ok_or(TransactionError::AccountNotFound)?;
    program_account.set_owner(**program_owner);
    program_account.set_executable(true);
    Ok(program_account)
}

/// Accumulate loaded account data size into `accumulated_accounts_data_size`.
/// Returns TransactionErr::MaxLoadedAccountsDataSizeExceeded if
/// `requested_loaded_accounts_data_size_limit` is specified and
/// `accumulated_accounts_data_size` exceeds it.
fn accumulate_and_check_loaded_account_data_size(
    accumulated_loaded_accounts_data_size: &mut usize,
    account_data_size: usize,
    requested_loaded_accounts_data_size_limit: Option<NonZeroUsize>,
    error_counters: &mut TransactionErrorMetrics,
) -> Result<()> {
    if let Some(requested_loaded_accounts_data_size) = requested_loaded_accounts_data_size_limit {
        saturating_add_assign!(*accumulated_loaded_accounts_data_size, account_data_size);
        if *accumulated_loaded_accounts_data_size > requested_loaded_accounts_data_size.get() {
            error_counters.max_loaded_accounts_data_size_exceeded += 1;
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
            transaction_processor::TransactionProcessingCallback,
        },
        nonce::state::Versions as NonceVersions,
        solana_program_runtime::{
            compute_budget::ComputeBudget,
            compute_budget_processor,
            loaded_programs::{LoadedProgram, LoadedProgramsForTxBatch},
            prioritization_fee::{PrioritizationFeeDetails, PrioritizationFeeType},
        },
        solana_sdk::{
            account::{AccountSharedData, ReadableAccount, WritableAccount},
            bpf_loader_upgradeable,
            compute_budget::ComputeBudgetInstruction,
            epoch_schedule::EpochSchedule,
            feature_set::FeatureSet,
            fee::FeeStructure,
            hash::Hash,
            instruction::CompiledInstruction,
            message::{
                v0::{LoadedAddresses, LoadedMessage},
                LegacyMessage, Message, MessageHeader, SanitizedMessage,
            },
            native_loader,
            native_token::sol_to_lamports,
            nonce,
            nonce_info::{NonceFull, NoncePartial},
            pubkey::Pubkey,
            rent::Rent,
            rent_collector::{RentCollector, RENT_EXEMPT_RENT_EPOCH},
            rent_debits::RentDebits,
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
        rent_collector: RentCollector,
        feature_set: Arc<FeatureSet>,
    }

    impl TransactionProcessingCallback for TestCallbacks {
        fn account_matches_owners(&self, _account: &Pubkey, _owners: &[Pubkey]) -> Option<usize> {
            None
        }

        fn get_account_shared_data(&self, pubkey: &Pubkey) -> Option<AccountSharedData> {
            self.accounts_map.get(pubkey).cloned()
        }

        fn get_last_blockhash_and_lamports_per_signature(&self) -> (Hash, u64) {
            (Hash::new_unique(), 0)
        }

        fn get_rent_collector(&self) -> &RentCollector {
            &self.rent_collector
        }

        fn get_feature_set(&self) -> Arc<FeatureSet> {
            self.feature_set.clone()
        }
    }

    fn load_accounts_with_fee_and_rent(
        tx: Transaction,
        ka: &[TransactionAccount],
        lamports_per_signature: u64,
        rent_collector: &RentCollector,
        error_counters: &mut TransactionErrorMetrics,
        feature_set: &mut FeatureSet,
        fee_structure: &FeeStructure,
    ) -> Vec<TransactionLoadResult> {
        feature_set.deactivate(&feature_set::disable_rent_fees_collection::id());
        let sanitized_tx = SanitizedTransaction::from_transaction_for_tests(tx);
        let mut accounts_map = HashMap::new();
        for (pubkey, account) in ka {
            accounts_map.insert(*pubkey, account.clone());
        }
        let callbacks = TestCallbacks {
            accounts_map,
            rent_collector: rent_collector.clone(),
            feature_set: Arc::new(feature_set.clone()),
        };
        load_accounts(
            &callbacks,
            &[sanitized_tx],
            &[(Ok(()), None, Some(lamports_per_signature))],
            error_counters,
            fee_structure,
            None,
            &HashMap::new(),
            &LoadedProgramsForTxBatch::default(),
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

    fn load_accounts_with_fee(
        tx: Transaction,
        ka: &[TransactionAccount],
        lamports_per_signature: u64,
        error_counters: &mut TransactionErrorMetrics,
        exclude_features: Option<&[Pubkey]>,
    ) -> Vec<TransactionLoadResult> {
        load_accounts_with_fee_and_rent(
            tx,
            ka,
            lamports_per_signature,
            &RentCollector::default(),
            error_counters,
            &mut all_features_except(exclude_features),
            &FeeStructure {
                lamports_per_signature,
                ..FeeStructure::default()
            },
        )
    }

    fn load_accounts_aux_test(
        tx: Transaction,
        ka: &[TransactionAccount],
        error_counters: &mut TransactionErrorMetrics,
    ) -> Vec<TransactionLoadResult> {
        load_accounts_with_fee(tx, ka, 0, error_counters, None)
    }

    fn load_accounts_with_excluded_features(
        tx: Transaction,
        ka: &[TransactionAccount],
        error_counters: &mut TransactionErrorMetrics,
        exclude_features: Option<&[Pubkey]>,
    ) -> Vec<TransactionLoadResult> {
        load_accounts_with_fee(tx, ka, 0, error_counters, exclude_features)
    }

    #[test]
    fn test_load_accounts_no_account_0_exists() {
        let accounts: Vec<TransactionAccount> = Vec::new();
        let mut error_counters = TransactionErrorMetrics::default();

        let keypair = Keypair::new();

        let instructions = vec![CompiledInstruction::new(1, &(), vec![0])];
        let tx = Transaction::new_with_compiled_instructions(
            &[&keypair],
            &[],
            Hash::default(),
            vec![native_loader::id()],
            instructions,
        );

        let loaded_accounts = load_accounts_aux_test(tx, &accounts, &mut error_counters);

        assert_eq!(error_counters.account_not_found, 1);
        assert_eq!(loaded_accounts.len(), 1);
        assert_eq!(
            loaded_accounts[0],
            (Err(TransactionError::AccountNotFound), None,),
        );
    }

    #[test]
    fn test_load_accounts_unknown_program_id() {
        let mut accounts: Vec<TransactionAccount> = Vec::new();
        let mut error_counters = TransactionErrorMetrics::default();

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

        let loaded_accounts = load_accounts_aux_test(tx, &accounts, &mut error_counters);

        assert_eq!(error_counters.account_not_found, 1);
        assert_eq!(loaded_accounts.len(), 1);
        assert_eq!(
            loaded_accounts[0],
            (Err(TransactionError::ProgramAccountNotFound), None,)
        );
    }

    #[test]
    fn test_load_accounts_insufficient_funds() {
        let lamports_per_signature = 5000;
        let mut accounts: Vec<TransactionAccount> = Vec::new();
        let mut error_counters = TransactionErrorMetrics::default();

        let keypair = Keypair::new();
        let key0 = keypair.pubkey();

        let account = AccountSharedData::new(1, 0, &Pubkey::default());
        accounts.push((key0, account));

        let instructions = vec![CompiledInstruction::new(1, &(), vec![0])];
        let tx = Transaction::new_with_compiled_instructions(
            &[&keypair],
            &[],
            Hash::default(),
            vec![native_loader::id()],
            instructions,
        );

        let message = SanitizedMessage::try_from_legacy_message(tx.message().clone()).unwrap();
        let fee = FeeStructure::default().calculate_fee(
            &message,
            lamports_per_signature,
            &process_compute_budget_instructions(message.program_instructions_iter())
                .unwrap_or_default()
                .into(),
            false,
            true,
        );
        assert_eq!(fee, lamports_per_signature);

        let loaded_accounts = load_accounts_with_fee(
            tx,
            &accounts,
            lamports_per_signature,
            &mut error_counters,
            None,
        );

        assert_eq!(error_counters.insufficient_funds, 1);
        assert_eq!(loaded_accounts.len(), 1);
        assert_eq!(
            loaded_accounts[0].clone(),
            (Err(TransactionError::InsufficientFundsForFee), None,),
        );
    }

    #[test]
    fn test_load_accounts_invalid_account_for_fee() {
        let mut accounts: Vec<TransactionAccount> = Vec::new();
        let mut error_counters = TransactionErrorMetrics::default();

        let keypair = Keypair::new();
        let key0 = keypair.pubkey();

        let account = AccountSharedData::new(1, 1, &solana_sdk::pubkey::new_rand()); // <-- owner is not the system program
        accounts.push((key0, account));

        let instructions = vec![CompiledInstruction::new(1, &(), vec![0])];
        let tx = Transaction::new_with_compiled_instructions(
            &[&keypair],
            &[],
            Hash::default(),
            vec![native_loader::id()],
            instructions,
        );

        let loaded_accounts = load_accounts_aux_test(tx, &accounts, &mut error_counters);

        assert_eq!(error_counters.invalid_account_for_fee, 1);
        assert_eq!(loaded_accounts.len(), 1);
        assert_eq!(
            loaded_accounts[0],
            (Err(TransactionError::InvalidAccountForFee), None,),
        );
    }

    #[test]
    fn test_load_accounts_fee_payer_is_nonce() {
        let lamports_per_signature = 5000;
        let mut error_counters = TransactionErrorMetrics::default();
        let rent_collector = RentCollector::new(
            0,
            EpochSchedule::default(),
            500_000.0,
            Rent {
                lamports_per_byte_year: 42,
                ..Rent::default()
            },
        );
        let min_balance = rent_collector.rent.minimum_balance(NonceState::size());
        let nonce = Keypair::new();
        let mut accounts = vec![(
            nonce.pubkey(),
            AccountSharedData::new_data(
                min_balance + lamports_per_signature,
                &NonceVersions::new(NonceState::Initialized(nonce::state::Data::default())),
                &system_program::id(),
            )
            .unwrap(),
        )];
        let instructions = vec![CompiledInstruction::new(1, &(), vec![0])];
        let tx = Transaction::new_with_compiled_instructions(
            &[&nonce],
            &[],
            Hash::default(),
            vec![native_loader::id()],
            instructions,
        );

        // Fee leaves min_balance balance succeeds
        let loaded_accounts = load_accounts_with_fee_and_rent(
            tx.clone(),
            &accounts,
            lamports_per_signature,
            &rent_collector,
            &mut error_counters,
            &mut all_features_except(None),
            &FeeStructure::default(),
        );
        assert_eq!(loaded_accounts.len(), 1);
        let (load_res, _nonce) = &loaded_accounts[0];
        let loaded_transaction = load_res.as_ref().unwrap();
        assert_eq!(loaded_transaction.accounts[0].1.lamports(), min_balance);

        // Fee leaves zero balance fails
        accounts[0].1.set_lamports(lamports_per_signature);
        let loaded_accounts = load_accounts_with_fee_and_rent(
            tx.clone(),
            &accounts,
            lamports_per_signature,
            &rent_collector,
            &mut error_counters,
            &mut FeatureSet::all_enabled(),
            &FeeStructure::default(),
        );
        assert_eq!(loaded_accounts.len(), 1);
        let (load_res, _nonce) = &loaded_accounts[0];
        assert_eq!(*load_res, Err(TransactionError::InsufficientFundsForFee));

        // Fee leaves non-zero, but sub-min_balance balance fails
        accounts[0]
            .1
            .set_lamports(lamports_per_signature + min_balance / 2);
        let loaded_accounts = load_accounts_with_fee_and_rent(
            tx,
            &accounts,
            lamports_per_signature,
            &rent_collector,
            &mut error_counters,
            &mut FeatureSet::all_enabled(),
            &FeeStructure::default(),
        );
        assert_eq!(loaded_accounts.len(), 1);
        let (load_res, _nonce) = &loaded_accounts[0];
        assert_eq!(*load_res, Err(TransactionError::InsufficientFundsForFee));
    }

    #[test]
    fn test_load_accounts_no_loaders() {
        let mut accounts: Vec<TransactionAccount> = Vec::new();
        let mut error_counters = TransactionErrorMetrics::default();

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
            load_accounts_with_excluded_features(tx, &accounts, &mut error_counters, None);

        assert_eq!(error_counters.account_not_found, 0);
        assert_eq!(loaded_accounts.len(), 1);
        match &loaded_accounts[0] {
            (Ok(loaded_transaction), _nonce) => {
                assert_eq!(loaded_transaction.accounts.len(), 3);
                assert_eq!(loaded_transaction.accounts[0].1, accounts[0].1);
                assert_eq!(loaded_transaction.program_indices.len(), 1);
                assert_eq!(loaded_transaction.program_indices[0].len(), 0);
            }
            (Err(e), _nonce) => panic!("{e}"),
        }
    }

    #[test]
    fn test_load_accounts_bad_owner() {
        let mut accounts: Vec<TransactionAccount> = Vec::new();
        let mut error_counters = TransactionErrorMetrics::default();

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

        let loaded_accounts = load_accounts_aux_test(tx, &accounts, &mut error_counters);

        assert_eq!(error_counters.account_not_found, 1);
        assert_eq!(loaded_accounts.len(), 1);
        assert_eq!(
            loaded_accounts[0],
            (Err(TransactionError::ProgramAccountNotFound), None,)
        );
    }

    #[test]
    fn test_load_accounts_not_executable() {
        let mut accounts: Vec<TransactionAccount> = Vec::new();
        let mut error_counters = TransactionErrorMetrics::default();

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

        let loaded_accounts = load_accounts_aux_test(tx, &accounts, &mut error_counters);

        assert_eq!(error_counters.invalid_program_for_execution, 1);
        assert_eq!(loaded_accounts.len(), 1);
        assert_eq!(
            loaded_accounts[0],
            (Err(TransactionError::InvalidProgramForExecution), None,)
        );
    }

    #[test]
    fn test_load_accounts_multiple_loaders() {
        let mut accounts: Vec<TransactionAccount> = Vec::new();
        let mut error_counters = TransactionErrorMetrics::default();

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
            load_accounts_with_excluded_features(tx, &accounts, &mut error_counters, None);

        assert_eq!(error_counters.account_not_found, 0);
        assert_eq!(loaded_accounts.len(), 1);
        match &loaded_accounts[0] {
            (Ok(loaded_transaction), _nonce) => {
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
            (Err(e), _nonce) => panic!("{e}"),
        }
    }

    fn load_accounts_no_store(
        ka: &[TransactionAccount],
        tx: Transaction,
        account_overrides: Option<&AccountOverrides>,
    ) -> Vec<TransactionLoadResult> {
        let tx = SanitizedTransaction::from_transaction_for_tests(tx);

        let mut error_counters = TransactionErrorMetrics::default();
        let mut accounts_map = HashMap::new();
        for (pubkey, account) in ka {
            accounts_map.insert(*pubkey, account.clone());
        }
        let callbacks = TestCallbacks {
            accounts_map,
            rent_collector: RentCollector::default(),
            feature_set: Arc::new(FeatureSet::all_enabled()),
        };
        load_accounts(
            &callbacks,
            &[tx],
            &[(Ok(()), None, Some(10))],
            &mut error_counters,
            &FeeStructure::default(),
            account_overrides,
            &HashMap::new(),
            &LoadedProgramsForTxBatch::default(),
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
        assert!(loaded_accounts[0].0.is_err());
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
        let loaded_transaction = loaded_accounts[0].0.as_ref().unwrap();
        assert_eq!(loaded_transaction.accounts[0].0, keypair.pubkey());
        assert_eq!(loaded_transaction.accounts[1].0, slot_history_id);
        assert_eq!(loaded_transaction.accounts[1].1.lamports(), 42);
    }

    #[test]
    fn test_accumulate_and_check_loaded_account_data_size() {
        let mut error_counter = TransactionErrorMetrics::default();

        // assert check is OK if data limit is not enabled
        {
            let mut accumulated_data_size: usize = 0;
            let data_size = usize::MAX;
            let requested_data_size_limit = None;

            assert!(accumulate_and_check_loaded_account_data_size(
                &mut accumulated_data_size,
                data_size,
                requested_data_size_limit,
                &mut error_counter
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
                &mut error_counter
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
                    &mut error_counter
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

    #[test]
    fn test_load_accounts_too_high_prioritization_fee() {
        solana_logger::setup();
        let lamports_per_signature = 5000_u64;
        let request_units = 1_000_000_u32;
        let request_unit_price = 2_000_000_000_u64;
        let prioritization_fee_details = PrioritizationFeeDetails::new(
            PrioritizationFeeType::ComputeUnitPrice(request_unit_price),
            request_units as u64,
        );
        let prioritization_fee = prioritization_fee_details.get_fee();

        let keypair = Keypair::new();
        let key0 = keypair.pubkey();
        // set up account with balance of `prioritization_fee`
        let account = AccountSharedData::new(prioritization_fee, 0, &Pubkey::default());
        let accounts = vec![(key0, account)];

        let instructions = &[
            ComputeBudgetInstruction::set_compute_unit_limit(request_units),
            ComputeBudgetInstruction::set_compute_unit_price(request_unit_price),
        ];
        let tx = Transaction::new(
            &[&keypair],
            Message::new(instructions, Some(&key0)),
            Hash::default(),
        );

        let message = SanitizedMessage::try_from_legacy_message(tx.message().clone()).unwrap();
        let fee = FeeStructure::default().calculate_fee(
            &message,
            lamports_per_signature,
            &process_compute_budget_instructions(message.program_instructions_iter())
                .unwrap_or_default()
                .into(),
            false,
            true,
        );
        assert_eq!(fee, lamports_per_signature + prioritization_fee);

        // assert fail to load account with 2B lamport balance for transaction asking for 2B
        // lamports as prioritization fee.
        let mut error_counters = TransactionErrorMetrics::default();
        let loaded_accounts = load_accounts_with_fee(
            tx,
            &accounts,
            lamports_per_signature,
            &mut error_counters,
            None,
        );

        assert_eq!(error_counters.insufficient_funds, 1);
        assert_eq!(loaded_accounts.len(), 1);
        assert_eq!(
            loaded_accounts[0].clone(),
            (Err(TransactionError::InsufficientFundsForFee), None),
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
    fn test_account_shared_data_from_program() {
        let key = Keypair::new().pubkey();
        let other_key = Keypair::new().pubkey();

        let mut accounts: HashMap<Pubkey, (&Pubkey, u64)> = HashMap::new();

        let result = account_shared_data_from_program(&key, &accounts);
        assert_eq!(result.err(), Some(TransactionError::AccountNotFound));

        accounts.insert(key, (&other_key, 32));

        let result = account_shared_data_from_program(&key, &accounts);
        let mut expected = AccountSharedData::default();
        expected.set_owner(other_key);
        expected.set_executable(true);

        assert_eq!(result.unwrap(), expected);
    }

    #[test]
    fn test_load_transaction_accounts_fail_to_validate_fee_payer() {
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

        let legacy = LegacyMessage::new(message);
        let sanitized_message = SanitizedMessage::Legacy(legacy);
        let mock_bank = TestCallbacks::default();
        let mut error_counter = TransactionErrorMetrics::default();
        let loaded_programs = LoadedProgramsForTxBatch::default();

        let sanitized_transaction = SanitizedTransaction::new_for_tests(
            sanitized_message,
            vec![Signature::new_unique()],
            false,
        );
        let result = load_transaction_accounts(
            &mock_bank,
            sanitized_transaction.message(),
            32,
            &mut error_counter,
            None,
            &HashMap::new(),
            &loaded_programs,
        );

        assert_eq!(result.err(), Some(TransactionError::AccountNotFound));
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

        let legacy = LegacyMessage::new(message);
        let sanitized_message = SanitizedMessage::Legacy(legacy);
        let mut mock_bank = TestCallbacks::default();
        mock_bank
            .accounts_map
            .insert(native_loader::id(), AccountSharedData::default());
        let mut account_data = AccountSharedData::default();
        account_data.set_lamports(200);
        mock_bank.accounts_map.insert(key1.pubkey(), account_data);

        let mut error_counter = TransactionErrorMetrics::default();
        let loaded_programs = LoadedProgramsForTxBatch::default();

        let sanitized_transaction = SanitizedTransaction::new_for_tests(
            sanitized_message,
            vec![Signature::new_unique()],
            false,
        );
        let result = load_transaction_accounts(
            &mock_bank,
            sanitized_transaction.message(),
            32,
            &mut error_counter,
            None,
            &HashMap::new(),
            &loaded_programs,
        );
        mock_bank
            .accounts_map
            .get_mut(&key1.pubkey())
            .unwrap()
            .set_lamports(200 - 32);

        assert_eq!(
            result.unwrap(),
            LoadedTransaction {
                accounts: vec![
                    (
                        key1.pubkey(),
                        mock_bank.accounts_map[&key1.pubkey()].clone()
                    ),
                    (
                        native_loader::id(),
                        mock_bank.accounts_map[&native_loader::id()].clone()
                    )
                ],
                program_indices: vec![vec![]],
                rent: 0,
                rent_debits: RentDebits::default()
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

        let legacy = LegacyMessage::new(message);
        let sanitized_message = SanitizedMessage::Legacy(legacy);
        let mut mock_bank = TestCallbacks::default();
        let mut account_data = AccountSharedData::default();
        account_data.set_lamports(200);
        mock_bank.accounts_map.insert(key1.pubkey(), account_data);

        let mut error_counter = TransactionErrorMetrics::default();
        let mut loaded_programs = LoadedProgramsForTxBatch::default();
        loaded_programs.replenish(key2.pubkey(), Arc::new(LoadedProgram::default()));

        let sanitized_transaction = SanitizedTransaction::new_for_tests(
            sanitized_message,
            vec![Signature::new_unique()],
            false,
        );
        let result = load_transaction_accounts(
            &mock_bank,
            sanitized_transaction.message(),
            32,
            &mut error_counter,
            None,
            &HashMap::new(),
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

        let legacy = LegacyMessage::new(message);
        let sanitized_message = SanitizedMessage::Legacy(legacy);
        let mut mock_bank = TestCallbacks::default();
        let mut account_data = AccountSharedData::default();
        account_data.set_lamports(200);
        mock_bank.accounts_map.insert(key1.pubkey(), account_data);

        let mut error_counter = TransactionErrorMetrics::default();
        let loaded_programs = LoadedProgramsForTxBatch::default();

        let sanitized_transaction = SanitizedTransaction::new_for_tests(
            sanitized_message,
            vec![Signature::new_unique()],
            false,
        );
        let result = load_transaction_accounts(
            &mock_bank,
            sanitized_transaction.message(),
            32,
            &mut error_counter,
            None,
            &HashMap::new(),
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

        let legacy = LegacyMessage::new(message);
        let sanitized_message = SanitizedMessage::Legacy(legacy);
        let mut mock_bank = TestCallbacks::default();
        let mut account_data = AccountSharedData::default();
        account_data.set_lamports(200);
        mock_bank.accounts_map.insert(key1.pubkey(), account_data);

        let mut error_counter = TransactionErrorMetrics::default();
        let loaded_programs = LoadedProgramsForTxBatch::default();

        let sanitized_transaction = SanitizedTransaction::new_for_tests(
            sanitized_message,
            vec![Signature::new_unique()],
            false,
        );
        let result = load_transaction_accounts(
            &mock_bank,
            sanitized_transaction.message(),
            32,
            &mut error_counter,
            None,
            &HashMap::new(),
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

        let legacy = LegacyMessage::new(message);
        let sanitized_message = SanitizedMessage::Legacy(legacy);
        let mut mock_bank = TestCallbacks::default();
        let mut account_data = AccountSharedData::default();
        account_data.set_owner(native_loader::id());
        account_data.set_executable(true);
        mock_bank.accounts_map.insert(key1.pubkey(), account_data);

        let mut account_data = AccountSharedData::default();
        account_data.set_lamports(200);
        mock_bank.accounts_map.insert(key2.pubkey(), account_data);
        let mut error_counter = TransactionErrorMetrics::default();
        let loaded_programs = LoadedProgramsForTxBatch::default();

        let sanitized_transaction = SanitizedTransaction::new_for_tests(
            sanitized_message,
            vec![Signature::new_unique()],
            false,
        );
        let result = load_transaction_accounts(
            &mock_bank,
            sanitized_transaction.message(),
            32,
            &mut error_counter,
            None,
            &HashMap::new(),
            &loaded_programs,
        );
        mock_bank
            .accounts_map
            .get_mut(&key2.pubkey())
            .unwrap()
            .set_lamports(200 - 32);

        assert_eq!(
            result.unwrap(),
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
                ],
                program_indices: vec![vec![1]],
                rent: 0,
                rent_debits: RentDebits::default()
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

        let legacy = LegacyMessage::new(message);
        let sanitized_message = SanitizedMessage::Legacy(legacy);
        let mut mock_bank = TestCallbacks::default();
        let mut account_data = AccountSharedData::default();
        account_data.set_executable(true);
        mock_bank.accounts_map.insert(key1.pubkey(), account_data);

        let mut account_data = AccountSharedData::default();
        account_data.set_lamports(200);
        mock_bank.accounts_map.insert(key2.pubkey(), account_data);
        let mut error_counter = TransactionErrorMetrics::default();
        let loaded_programs = LoadedProgramsForTxBatch::default();

        let sanitized_transaction = SanitizedTransaction::new_for_tests(
            sanitized_message,
            vec![Signature::new_unique()],
            false,
        );
        let result = load_transaction_accounts(
            &mock_bank,
            sanitized_transaction.message(),
            32,
            &mut error_counter,
            None,
            &HashMap::new(),
            &loaded_programs,
        );
        mock_bank
            .accounts_map
            .get_mut(&key2.pubkey())
            .unwrap()
            .set_lamports(200 - 32);

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

        let legacy = LegacyMessage::new(message);
        let sanitized_message = SanitizedMessage::Legacy(legacy);
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
        let mut error_counter = TransactionErrorMetrics::default();
        let loaded_programs = LoadedProgramsForTxBatch::default();

        let sanitized_transaction = SanitizedTransaction::new_for_tests(
            sanitized_message,
            vec![Signature::new_unique()],
            false,
        );
        let result = load_transaction_accounts(
            &mock_bank,
            sanitized_transaction.message(),
            32,
            &mut error_counter,
            None,
            &HashMap::new(),
            &loaded_programs,
        );
        mock_bank
            .accounts_map
            .get_mut(&key2.pubkey())
            .unwrap()
            .set_lamports(200 - 32);

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

        let legacy = LegacyMessage::new(message);
        let sanitized_message = SanitizedMessage::Legacy(legacy);
        let mut mock_bank = TestCallbacks::default();
        let mut account_data = AccountSharedData::default();
        account_data.set_executable(true);
        account_data.set_owner(key3.pubkey());
        mock_bank.accounts_map.insert(key1.pubkey(), account_data);

        let mut account_data = AccountSharedData::default();
        account_data.set_lamports(200);
        mock_bank.accounts_map.insert(key2.pubkey(), account_data);

        let mut account_data = AccountSharedData::default();
        account_data.set_executable(true);
        account_data.set_owner(native_loader::id());
        mock_bank.accounts_map.insert(key3.pubkey(), account_data);

        let mut error_counter = TransactionErrorMetrics::default();
        let loaded_programs = LoadedProgramsForTxBatch::default();

        let sanitized_transaction = SanitizedTransaction::new_for_tests(
            sanitized_message,
            vec![Signature::new_unique()],
            false,
        );
        let result = load_transaction_accounts(
            &mock_bank,
            sanitized_transaction.message(),
            32,
            &mut error_counter,
            None,
            &HashMap::new(),
            &loaded_programs,
        );
        mock_bank
            .accounts_map
            .get_mut(&key2.pubkey())
            .unwrap()
            .set_lamports(200 - 32);

        assert_eq!(
            result.unwrap(),
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
                    (
                        key3.pubkey(),
                        mock_bank.accounts_map[&key3.pubkey()].clone()
                    ),
                ],
                program_indices: vec![vec![2, 1]],
                rent: 0,
                rent_debits: RentDebits::default()
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

        let legacy = LegacyMessage::new(message);
        let sanitized_message = SanitizedMessage::Legacy(legacy);
        let mut mock_bank = TestCallbacks::default();
        let mut account_data = AccountSharedData::default();
        account_data.set_executable(true);
        account_data.set_owner(key3.pubkey());
        mock_bank.accounts_map.insert(key1.pubkey(), account_data);

        let mut account_data = AccountSharedData::default();
        account_data.set_lamports(200);
        mock_bank.accounts_map.insert(key2.pubkey(), account_data);

        let mut account_data = AccountSharedData::default();
        account_data.set_executable(true);
        account_data.set_owner(native_loader::id());
        mock_bank.accounts_map.insert(key3.pubkey(), account_data);

        let mut error_counter = TransactionErrorMetrics::default();
        let loaded_programs = LoadedProgramsForTxBatch::default();

        let sanitized_transaction = SanitizedTransaction::new_for_tests(
            sanitized_message,
            vec![Signature::new_unique()],
            false,
        );
        let result = load_transaction_accounts(
            &mock_bank,
            sanitized_transaction.message(),
            32,
            &mut error_counter,
            None,
            &HashMap::new(),
            &loaded_programs,
        );
        mock_bank
            .accounts_map
            .get_mut(&key2.pubkey())
            .unwrap()
            .set_lamports(200 - 32);

        let mut account_data = AccountSharedData::default();
        account_data.set_rent_epoch(RENT_EXEMPT_RENT_EPOCH);
        assert_eq!(
            result.unwrap(),
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
                rent: 0,
                rent_debits: RentDebits::default()
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
        let mut error_counters = TransactionErrorMetrics::default();
        let loaded_txs = load_accounts(
            &bank,
            &[sanitized_tx.clone()],
            &[(Ok(()), None, Some(0))],
            &mut error_counters,
            &FeeStructure::default(),
            None,
            &HashMap::new(),
            &LoadedProgramsForTxBatch::default(),
        );

        let compute_budget = ComputeBudget::new(u64::from(
            compute_budget_processor::DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT,
        ));
        let transaction_context = TransactionContext::new(
            loaded_txs[0].0.as_ref().unwrap().accounts.clone(),
            Rent::default(),
            compute_budget.max_invoke_stack_height,
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

        let legacy = LegacyMessage::new(message);
        let sanitized_message = SanitizedMessage::Legacy(legacy);
        let mut mock_bank = TestCallbacks::default();
        let mut account_data = AccountSharedData::default();
        account_data.set_executable(true);
        account_data.set_owner(key3.pubkey());
        mock_bank.accounts_map.insert(key1.pubkey(), account_data);

        let mut account_data = AccountSharedData::default();
        account_data.set_lamports(200);
        mock_bank.accounts_map.insert(key2.pubkey(), account_data);

        let mut account_data = AccountSharedData::default();
        account_data.set_executable(true);
        account_data.set_owner(native_loader::id());
        mock_bank.accounts_map.insert(key3.pubkey(), account_data);

        let mut error_counter = TransactionErrorMetrics::default();
        let loaded_programs = LoadedProgramsForTxBatch::default();

        let sanitized_transaction = SanitizedTransaction::new_for_tests(
            sanitized_message,
            vec![Signature::new_unique()],
            false,
        );
        let lock_results =
            (Ok(()), Some(NoncePartial::default()), Some(20u64)) as TransactionCheckResult;

        let results = load_accounts(
            &mock_bank,
            &[sanitized_transaction],
            &[lock_results],
            &mut error_counter,
            &FeeStructure::default(),
            None,
            &HashMap::new(),
            &loaded_programs,
        );

        let mut account_data = AccountSharedData::default();
        account_data.set_rent_epoch(RENT_EXEMPT_RENT_EPOCH);

        assert_eq!(results.len(), 1);
        let (loaded_result, nonce) = results[0].clone();
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
                rent: 0,
                rent_debits: RentDebits::default()
            }
        );

        assert_eq!(
            nonce.unwrap(),
            NonceFull::new(
                Pubkey::from([0; 32]),
                AccountSharedData::default(),
                Some(mock_bank.accounts_map[&key2.pubkey()].clone())
            )
        );
    }

    #[test]
    fn test_load_accounts_error() {
        let mock_bank = TestCallbacks::default();
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

        let legacy = LegacyMessage::new(message);
        let sanitized_message = SanitizedMessage::Legacy(legacy);
        let sanitized_transaction = SanitizedTransaction::new_for_tests(
            sanitized_message,
            vec![Signature::new_unique()],
            false,
        );

        let lock_results = (Ok(()), Some(NoncePartial::default()), None) as TransactionCheckResult;
        let fee_structure = FeeStructure::default();

        let result = load_accounts(
            &mock_bank,
            &[sanitized_transaction.clone()],
            &[lock_results],
            &mut TransactionErrorMetrics::default(),
            &fee_structure,
            None,
            &HashMap::new(),
            &LoadedProgramsForTxBatch::default(),
        );

        assert_eq!(
            result,
            vec![(Err(TransactionError::BlockhashNotFound), None)]
        );

        let lock_results =
            (Ok(()), Some(NoncePartial::default()), Some(20u64)) as TransactionCheckResult;

        let result = load_accounts(
            &mock_bank,
            &[sanitized_transaction.clone()],
            &[lock_results.clone()],
            &mut TransactionErrorMetrics::default(),
            &fee_structure,
            None,
            &HashMap::new(),
            &LoadedProgramsForTxBatch::default(),
        );

        assert_eq!(result, vec![(Err(TransactionError::AccountNotFound), None)]);

        let lock_results = (
            Err(TransactionError::InvalidWritableAccount),
            Some(NoncePartial::default()),
            Some(20u64),
        ) as TransactionCheckResult;

        let result = load_accounts(
            &mock_bank,
            &[sanitized_transaction.clone()],
            &[lock_results],
            &mut TransactionErrorMetrics::default(),
            &fee_structure,
            None,
            &HashMap::new(),
            &LoadedProgramsForTxBatch::default(),
        );

        assert_eq!(
            result,
            vec![(Err(TransactionError::InvalidWritableAccount), None)]
        );
    }
}
