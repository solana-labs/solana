pub mod account_rent_state;

use {
    crate::accounts::account_rent_state::{check_rent_state_with_account, RentState},
    itertools::Itertools,
    log::warn,
    solana_accounts_db::{
        account_overrides::AccountOverrides,
        accounts::{LoadedTransaction, RewardInterval, TransactionLoadResult, TransactionRent},
        accounts_db::AccountsDb,
        ancestors::Ancestors,
        blockhash_queue::BlockhashQueue,
        nonce_info::{NonceFull, NonceInfo},
        rent_collector::{RentCollector, RENT_EXEMPT_RENT_EPOCH},
        rent_debits::RentDebits,
        transaction_error_metrics::TransactionErrorMetrics,
        transaction_results::TransactionCheckResult,
    },
    solana_program_runtime::{
        compute_budget_processor::process_compute_budget_instructions,
        loaded_programs::LoadedProgramsForTxBatch,
    },
    solana_sdk::{
        account::{Account, AccountSharedData, ReadableAccount, WritableAccount},
        account_utils::StateMut,
        bpf_loader_upgradeable::{self, UpgradeableLoaderState},
        feature_set::{
            self, include_loaded_accounts_data_size_in_fee_calculation,
            remove_congestion_multiplier_from_fee_calculation,
            simplify_writable_program_account_check, FeatureSet,
        },
        fee::FeeStructure,
        message::SanitizedMessage,
        native_loader,
        nonce::State as NonceState,
        pubkey::Pubkey,
        rent::RentDue,
        saturating_add_assign,
        sysvar::{self, instructions::construct_instructions_data},
        transaction::{Result, SanitizedTransaction, TransactionError},
        transaction_context::IndexOfAccount,
    },
    solana_system_program::{get_system_account_kind, SystemAccountKind},
    std::{collections::HashMap, num::NonZeroUsize},
};

#[allow(clippy::too_many_arguments)]
pub(super) fn load_accounts(
    accounts_db: &AccountsDb,
    ancestors: &Ancestors,
    txs: &[SanitizedTransaction],
    lock_results: Vec<TransactionCheckResult>,
    hash_queue: &BlockhashQueue,
    error_counters: &mut TransactionErrorMetrics,
    rent_collector: &RentCollector,
    feature_set: &FeatureSet,
    fee_structure: &FeeStructure,
    account_overrides: Option<&AccountOverrides>,
    in_reward_interval: RewardInterval,
    program_accounts: &HashMap<Pubkey, (&Pubkey, u64)>,
    loaded_programs: &LoadedProgramsForTxBatch,
    should_collect_rent: bool,
) -> Vec<TransactionLoadResult> {
    txs.iter()
        .zip(lock_results)
        .map(|etx| match etx {
            (tx, (Ok(()), nonce)) => {
                let lamports_per_signature = nonce
                    .as_ref()
                    .map(|nonce| nonce.lamports_per_signature())
                    .unwrap_or_else(|| {
                        hash_queue.get_lamports_per_signature(tx.message().recent_blockhash())
                    });
                let fee = if let Some(lamports_per_signature) = lamports_per_signature {
                    fee_structure.calculate_fee(
                        tx.message(),
                        lamports_per_signature,
                        &process_compute_budget_instructions(
                            tx.message().program_instructions_iter(),
                            feature_set,
                        )
                        .unwrap_or_default()
                        .into(),
                        feature_set
                            .is_active(&remove_congestion_multiplier_from_fee_calculation::id()),
                        feature_set
                            .is_active(&include_loaded_accounts_data_size_in_fee_calculation::id()),
                    )
                } else {
                    return (Err(TransactionError::BlockhashNotFound), None);
                };

                // load transactions
                let loaded_transaction = match load_transaction_accounts(
                    accounts_db,
                    ancestors,
                    tx,
                    fee,
                    error_counters,
                    rent_collector,
                    feature_set,
                    account_overrides,
                    in_reward_interval,
                    program_accounts,
                    loaded_programs,
                    should_collect_rent,
                ) {
                    Ok(loaded_transaction) => loaded_transaction,
                    Err(e) => return (Err(e), None),
                };

                // Update nonce with fee-subtracted accounts
                let nonce = if let Some(nonce) = nonce {
                    match NonceFull::from_partial(
                        nonce,
                        tx.message(),
                        &loaded_transaction.accounts,
                        &loaded_transaction.rent_debits,
                    ) {
                        Ok(nonce) => Some(nonce),
                        Err(e) => return (Err(e), None),
                    }
                } else {
                    None
                };

                (Ok(loaded_transaction), nonce)
            }
            (_, (Err(e), _nonce)) => (Err(e), None),
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn load_transaction_accounts(
    accounts_db: &AccountsDb,
    ancestors: &Ancestors,
    tx: &SanitizedTransaction,
    fee: u64,
    error_counters: &mut TransactionErrorMetrics,
    rent_collector: &RentCollector,
    feature_set: &FeatureSet,
    account_overrides: Option<&AccountOverrides>,
    reward_interval: RewardInterval,
    program_accounts: &HashMap<Pubkey, (&Pubkey, u64)>,
    loaded_programs: &LoadedProgramsForTxBatch,
    should_collect_rent: bool,
) -> Result<LoadedTransaction> {
    let in_reward_interval = reward_interval == RewardInterval::InsideInterval;

    // NOTE: this check will never fail because `tx` is sanitized
    if tx.signatures().is_empty() && fee != 0 {
        return Err(TransactionError::MissingSignatureForFee);
    }

    // There is no way to predict what program will execute without an error
    // If a fee can pay for execution then the program will be scheduled
    let mut validated_fee_payer = false;
    let mut tx_rent: TransactionRent = 0;
    let message = tx.message();
    let account_keys = message.account_keys();
    let mut accounts_found = Vec::with_capacity(account_keys.len());
    let mut account_deps = Vec::with_capacity(account_keys.len());
    let mut rent_debits = RentDebits::default();

    let set_exempt_rent_epoch_max =
        feature_set.is_active(&solana_sdk::feature_set::set_exempt_rent_epoch_max::id());

    let requested_loaded_accounts_data_size_limit =
        get_requested_loaded_accounts_data_size_limit(tx, feature_set)?;
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
                } else if let Some(program) = (feature_set
                    .is_active(&simplify_writable_program_account_check::id())
                    && !instruction_account
                    && !message.is_writable(i))
                .then_some(())
                .and_then(|_| loaded_programs.find(key))
                {
                    // This condition block does special handling for accounts that are passed
                    // as instruction account to any of the instructions in the transaction.
                    // It's been noticed that some programs are reading other program accounts
                    // (that are passed to the program as instruction accounts). So such accounts
                    // are needed to be loaded even though corresponding compiled program may
                    // already be present in the cache.
                    account_shared_data_from_program(key, program_accounts)
                        .map(|program_account| (program.account_size, program_account, 0))?
                } else {
                    accounts_db
                        .load_with_fixed_root(ancestors, key)
                        .map(|(mut account, _)| {
                            if message.is_writable(i) {
                                if should_collect_rent {
                                    let rent_due = rent_collector
                                        .collect_from_existing_account(
                                            key,
                                            &mut account,
                                            set_exempt_rent_epoch_max,
                                        )
                                        .rent_amount;

                                    (account.data().len(), account, rent_due)
                                } else {
                                    // When rent fee collection is disabled, we won't collect rent for any account. If there
                                    // are any rent paying accounts, their `rent_epoch` won't change either. However, if the
                                    // account itself is rent-exempted but its `rent_epoch` is not u64::MAX, we will set its
                                    // `rent_epoch` to u64::MAX. In such case, the behavior stays the same as before.
                                    if set_exempt_rent_epoch_max
                                        && (account.rent_epoch() != RENT_EXEMPT_RENT_EPOCH
                                            && rent_collector.get_rent_due(&account)
                                                == RentDue::Exempt)
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
                            if set_exempt_rent_epoch_max {
                                // All new accounts must be rent-exempt (enforced in Bank::execute_loaded_transaction).
                                // Currently, rent collection sets rent_epoch to u64::MAX, but initializing the account
                                // with this field already set would allow us to skip rent collection for these accounts.
                                default_account.set_rent_epoch(RENT_EXEMPT_RENT_EPOCH);
                            }
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
                        warn!("Payer index should be 0! {:?}", tx);
                    }

                    validate_fee_payer(
                        key,
                        &mut account,
                        i as IndexOfAccount,
                        error_counters,
                        rent_collector,
                        feature_set,
                        fee,
                    )?;

                    validated_fee_payer = true;
                }

                if !feature_set.is_active(&simplify_writable_program_account_check::id()) {
                    if bpf_loader_upgradeable::check_id(account.owner()) {
                        if message.is_writable(i) && !message.is_upgradeable_loader_present() {
                            error_counters.invalid_writable_account += 1;
                            return Err(TransactionError::InvalidWritableAccount);
                        }

                        if account.executable() {
                            // The upgradeable loader requires the derived ProgramData account
                            if let Ok(UpgradeableLoaderState::Program {
                                programdata_address,
                            }) = account.state()
                            {
                                if accounts_db
                                    .load_with_fixed_root(ancestors, &programdata_address)
                                    .is_none()
                                {
                                    error_counters.account_not_found += 1;
                                    return Err(TransactionError::ProgramAccountNotFound);
                                }
                            } else {
                                error_counters.invalid_program_for_execution += 1;
                                return Err(TransactionError::InvalidProgramForExecution);
                            }
                        }
                    } else if account.executable() && message.is_writable(i) {
                        error_counters.invalid_writable_account += 1;
                        return Err(TransactionError::InvalidWritableAccount);
                    }
                }

                if in_reward_interval
                    && message.is_writable(i)
                    && solana_stake_program::check_id(account.owner())
                {
                    error_counters.program_execution_temporarily_restricted += 1;
                    return Err(TransactionError::ProgramExecutionTemporarilyRestricted {
                        account_index: i as u8,
                    });
                }

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

    // Appends the account_deps at the end of the accounts,
    // this way they can be accessed in a uniform way.
    // At places where only the accounts are needed,
    // the account_deps are truncated using e.g:
    // accounts.iter().take(message.account_keys.len())
    accounts.append(&mut account_deps);

    let builtins_start_index = accounts.len();
    let program_indices = message
        .instructions()
        .iter()
        .map(|instruction| {
            let mut account_indices = Vec::new();
            let mut program_index = instruction.program_id_index as usize;
            let (program_id, program_account) = accounts
                .get(program_index)
                .ok_or(TransactionError::ProgramAccountNotFound)?;
            let account_found = accounts_found.get(program_index).unwrap_or(&true);
            if native_loader::check_id(program_id) {
                return Ok(account_indices);
            }
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
                if let Some((owner_account, _)) =
                    accounts_db.load_with_fixed_root(ancestors, owner_id)
                {
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

/// If feature `cap_transaction_accounts_data_size` is active, total accounts data a
/// transaction can load is limited to
///   if `set_tx_loaded_accounts_data_size` instruction is not activated or not used, then
///     default value of 64MiB to not break anyone in Mainnet-beta today
///   else
///     user requested loaded accounts size.
///     Note, requesting zero bytes will result transaction error
fn get_requested_loaded_accounts_data_size_limit(
    tx: &SanitizedTransaction,
    feature_set: &FeatureSet,
) -> Result<Option<NonZeroUsize>> {
    if feature_set.is_active(&feature_set::cap_transaction_accounts_data_size::id()) {
        let compute_budget_limits = process_compute_budget_instructions(
            tx.message().program_instructions_iter(),
            feature_set,
        )
        .unwrap_or_default();
        // sanitize against setting size limit to zero
        NonZeroUsize::new(
            usize::try_from(compute_budget_limits.loaded_accounts_bytes).unwrap_or_default(),
        )
        .map_or(
            Err(TransactionError::InvalidLoadedAccountsDataSizeLimit),
            |v| Ok(Some(v)),
        )
    } else {
        // feature not activated, no loaded accounts data limit imposed.
        Ok(None)
    }
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

fn validate_fee_payer(
    payer_address: &Pubkey,
    payer_account: &mut AccountSharedData,
    payer_index: IndexOfAccount,
    error_counters: &mut TransactionErrorMetrics,
    rent_collector: &RentCollector,
    feature_set: &FeatureSet,
    fee: u64,
) -> Result<()> {
    if payer_account.lamports() == 0 {
        error_counters.account_not_found += 1;
        return Err(TransactionError::AccountNotFound);
    }
    let min_balance = match get_system_account_kind(payer_account).ok_or_else(|| {
        error_counters.invalid_account_for_fee += 1;
        TransactionError::InvalidAccountForFee
    })? {
        SystemAccountKind::System => 0,
        SystemAccountKind::Nonce => {
            // Should we ever allow a fees charge to zero a nonce account's
            // balance. The state MUST be set to uninitialized in that case
            rent_collector.rent.minimum_balance(NonceState::size())
        }
    };

    // allow collapsible-else-if to make removing the feature gate safer once activated
    #[allow(clippy::collapsible_else_if)]
    if feature_set.is_active(&feature_set::checked_arithmetic_in_fee_validation::id()) {
        payer_account
            .lamports()
            .checked_sub(min_balance)
            .and_then(|v| v.checked_sub(fee))
            .ok_or_else(|| {
                error_counters.insufficient_funds += 1;
                TransactionError::InsufficientFundsForFee
            })?;
    } else {
        if payer_account.lamports() < fee + min_balance {
            error_counters.insufficient_funds += 1;
            return Err(TransactionError::InsufficientFundsForFee);
        }
    }

    let payer_pre_rent_state = RentState::from_account(payer_account, &rent_collector.rent);
    payer_account
        .checked_sub_lamports(fee)
        .map_err(|_| TransactionError::InsufficientFundsForFee)?;

    let payer_post_rent_state = RentState::from_account(payer_account, &rent_collector.rent);
    check_rent_state_with_account(
        &payer_pre_rent_state,
        &payer_post_rent_state,
        payer_address,
        payer_account,
        payer_index,
    )
}

pub fn construct_instructions_account(message: &SanitizedMessage) -> AccountSharedData {
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
        nonce::state::Versions as NonceVersions,
        solana_accounts_db::{
            accounts::Accounts, accounts_db::AccountShrinkThreshold,
            accounts_index::AccountSecondaryIndexes, rent_collector::RentCollector,
        },
        solana_program_runtime::{
            compute_budget_processor,
            prioritization_fee::{PrioritizationFeeDetails, PrioritizationFeeType},
        },
        solana_sdk::{
            account::{AccountSharedData, WritableAccount},
            compute_budget::ComputeBudgetInstruction,
            epoch_schedule::EpochSchedule,
            genesis_config::ClusterType,
            hash::Hash,
            instruction::CompiledInstruction,
            message::{Message, SanitizedMessage},
            nonce,
            rent::Rent,
            signature::{Keypair, Signer},
            system_program, sysvar,
            transaction::{Result, Transaction, TransactionError},
            transaction_context::TransactionAccount,
        },
        std::convert::TryFrom,
    };

    fn load_accounts_with_fee_and_rent(
        tx: Transaction,
        ka: &[TransactionAccount],
        lamports_per_signature: u64,
        rent_collector: &RentCollector,
        error_counters: &mut TransactionErrorMetrics,
        feature_set: &FeatureSet,
        fee_structure: &FeeStructure,
    ) -> Vec<TransactionLoadResult> {
        let mut hash_queue = BlockhashQueue::new(100);
        hash_queue.register_hash(&tx.message().recent_blockhash, lamports_per_signature);
        let accounts = Accounts::new_with_config_for_tests(
            Vec::new(),
            &ClusterType::Development,
            AccountSecondaryIndexes::default(),
            AccountShrinkThreshold::default(),
        );
        for ka in ka.iter() {
            accounts.accounts_db.store_for_tests(0, &[(&ka.0, &ka.1)]);
        }

        let ancestors = vec![(0, 0)].into_iter().collect();
        let sanitized_tx = SanitizedTransaction::from_transaction_for_tests(tx);
        load_accounts(
            &accounts.accounts_db,
            &ancestors,
            &[sanitized_tx],
            vec![(Ok(()), None)],
            &hash_queue,
            error_counters,
            rent_collector,
            feature_set,
            fee_structure,
            None,
            RewardInterval::OutsideInterval,
            &HashMap::new(),
            &LoadedProgramsForTxBatch::default(),
            true,
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
            &all_features_except(exclude_features),
            &FeeStructure::default(),
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

        let mut feature_set = FeatureSet::all_enabled();
        feature_set.deactivate(&solana_sdk::feature_set::remove_deprecated_request_unit_ix::id());

        let message = SanitizedMessage::try_from(tx.message().clone()).unwrap();
        let fee = FeeStructure::default().calculate_fee(
            &message,
            lamports_per_signature,
            &process_compute_budget_instructions(message.program_instructions_iter(), &feature_set)
                .unwrap_or_default()
                .into(),
            true,
            false,
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
            &all_features_except(None),
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
            &FeatureSet::all_enabled(),
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
            &FeatureSet::all_enabled(),
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

        let account = AccountSharedData::new(40, 1, &native_loader::id());
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
        let key1 = Pubkey::from([5u8; 32]);
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

    #[test]
    fn test_load_accounts_executable_with_write_lock() {
        let mut accounts: Vec<TransactionAccount> = Vec::new();
        let mut error_counters = TransactionErrorMetrics::default();

        let keypair = Keypair::new();
        let key0 = keypair.pubkey();
        let key1 = Pubkey::from([5u8; 32]);
        let key2 = Pubkey::from([6u8; 32]);

        let mut account = AccountSharedData::new(1, 0, &Pubkey::default());
        account.set_rent_epoch(1);
        accounts.push((key0, account));

        let mut account = AccountSharedData::new(40, 1, &native_loader::id());
        account.set_executable(true);
        account.set_rent_epoch(1);
        accounts.push((key1, account));

        let mut account = AccountSharedData::new(40, 1, &native_loader::id());
        account.set_executable(true);
        account.set_rent_epoch(1);
        accounts.push((key2, account));

        let instructions = vec![CompiledInstruction::new(2, &(), vec![0, 1])];
        let mut message = Message::new_with_compiled_instructions(
            1,
            0,
            1, // only one executable marked as readonly
            vec![key0, key1, key2],
            Hash::default(),
            instructions,
        );
        let tx = Transaction::new(&[&keypair], message.clone(), Hash::default());
        let loaded_accounts = load_accounts_with_excluded_features(
            tx,
            &accounts,
            &mut error_counters,
            Some(&[simplify_writable_program_account_check::id()]),
        );

        assert_eq!(error_counters.invalid_writable_account, 1);
        assert_eq!(loaded_accounts.len(), 1);
        assert_eq!(
            loaded_accounts[0],
            (Err(TransactionError::InvalidWritableAccount), None)
        );

        // Mark executables as readonly
        message.account_keys = vec![key0, key1, key2]; // revert key change
        message.header.num_readonly_unsigned_accounts = 2; // mark both executables as readonly
        let tx = Transaction::new(&[&keypair], message, Hash::default());
        let loaded_accounts = load_accounts_with_excluded_features(
            tx,
            &accounts,
            &mut error_counters,
            Some(&[simplify_writable_program_account_check::id()]),
        );

        assert_eq!(error_counters.invalid_writable_account, 1);
        assert_eq!(loaded_accounts.len(), 1);
        let result = loaded_accounts[0].0.as_ref().unwrap();
        assert_eq!(result.accounts[..2], accounts[..2]);
        assert_eq!(
            result.accounts[result.program_indices[0][0] as usize],
            accounts[2]
        );
    }

    #[test]
    fn test_load_accounts_upgradeable_with_write_lock() {
        let mut accounts: Vec<TransactionAccount> = Vec::new();
        let mut error_counters = TransactionErrorMetrics::default();

        let keypair = Keypair::new();
        let key0 = keypair.pubkey();
        let key1 = Pubkey::from([5u8; 32]);
        let key2 = Pubkey::from([6u8; 32]);
        let programdata_key1 = Pubkey::from([7u8; 32]);
        let programdata_key2 = Pubkey::from([8u8; 32]);

        let mut account = AccountSharedData::new(1, 0, &Pubkey::default());
        account.set_rent_epoch(1);
        accounts.push((key0, account));

        let program_data = UpgradeableLoaderState::ProgramData {
            slot: 42,
            upgrade_authority_address: None,
        };

        let program = UpgradeableLoaderState::Program {
            programdata_address: programdata_key1,
        };
        let mut account =
            AccountSharedData::new_data(40, &program, &bpf_loader_upgradeable::id()).unwrap();
        account.set_executable(true);
        account.set_rent_epoch(1);
        accounts.push((key1, account));
        let mut account =
            AccountSharedData::new_data(40, &program_data, &bpf_loader_upgradeable::id()).unwrap();
        account.set_rent_epoch(1);
        accounts.push((programdata_key1, account));

        let program = UpgradeableLoaderState::Program {
            programdata_address: programdata_key2,
        };
        let mut account =
            AccountSharedData::new_data(40, &program, &bpf_loader_upgradeable::id()).unwrap();
        account.set_executable(true);
        account.set_rent_epoch(1);
        accounts.push((key2, account));
        let mut account =
            AccountSharedData::new_data(40, &program_data, &bpf_loader_upgradeable::id()).unwrap();
        account.set_rent_epoch(1);
        accounts.push((programdata_key2, account));

        let mut account = AccountSharedData::new(40, 1, &native_loader::id()); // create mock bpf_loader_upgradeable
        account.set_executable(true);
        account.set_rent_epoch(1);
        accounts.push((bpf_loader_upgradeable::id(), account));

        let instructions = vec![CompiledInstruction::new(2, &(), vec![0, 1])];
        let mut message = Message::new_with_compiled_instructions(
            1,
            0,
            1, // only one executable marked as readonly
            vec![key0, key1, key2],
            Hash::default(),
            instructions,
        );
        let tx = Transaction::new(&[&keypair], message.clone(), Hash::default());
        let loaded_accounts = load_accounts_with_excluded_features(
            tx.clone(),
            &accounts,
            &mut error_counters,
            Some(&[simplify_writable_program_account_check::id()]),
        );

        assert_eq!(error_counters.invalid_writable_account, 1);
        assert_eq!(loaded_accounts.len(), 1);
        assert_eq!(
            loaded_accounts[0],
            (Err(TransactionError::InvalidWritableAccount), None)
        );

        // Solution 0: Include feature simplify_writable_program_account_check
        let loaded_accounts =
            load_accounts_with_excluded_features(tx, &accounts, &mut error_counters, None);

        assert_eq!(error_counters.invalid_writable_account, 1);
        assert_eq!(loaded_accounts.len(), 1);

        // Solution 1: include bpf_loader_upgradeable account
        message.account_keys = vec![key0, key1, bpf_loader_upgradeable::id()];
        let tx = Transaction::new(&[&keypair], message.clone(), Hash::default());
        let loaded_accounts = load_accounts_with_excluded_features(
            tx,
            &accounts,
            &mut error_counters,
            Some(&[simplify_writable_program_account_check::id()]),
        );

        assert_eq!(error_counters.invalid_writable_account, 1);
        assert_eq!(loaded_accounts.len(), 1);
        let result = loaded_accounts[0].0.as_ref().unwrap();
        assert_eq!(result.accounts[..2], accounts[..2]);
        assert_eq!(
            result.accounts[result.program_indices[0][0] as usize],
            accounts[5]
        );

        // Solution 2: mark programdata as readonly
        message.account_keys = vec![key0, key1, key2]; // revert key change
        message.header.num_readonly_unsigned_accounts = 2; // mark both executables as readonly
        let tx = Transaction::new(&[&keypair], message, Hash::default());
        let loaded_accounts = load_accounts_with_excluded_features(
            tx,
            &accounts,
            &mut error_counters,
            Some(&[simplify_writable_program_account_check::id()]),
        );

        assert_eq!(error_counters.invalid_writable_account, 1);
        assert_eq!(loaded_accounts.len(), 1);
        let result = loaded_accounts[0].0.as_ref().unwrap();
        assert_eq!(result.accounts[..2], accounts[..2]);
        assert_eq!(
            result.accounts[result.program_indices[0][0] as usize],
            accounts[5]
        );
        assert_eq!(
            result.accounts[result.program_indices[0][1] as usize],
            accounts[3]
        );
    }

    #[test]
    fn test_load_accounts_programdata_with_write_lock() {
        let mut accounts: Vec<TransactionAccount> = Vec::new();
        let mut error_counters = TransactionErrorMetrics::default();

        let keypair = Keypair::new();
        let key0 = keypair.pubkey();
        let key1 = Pubkey::from([5u8; 32]);
        let key2 = Pubkey::from([6u8; 32]);

        let mut account = AccountSharedData::new(1, 0, &Pubkey::default());
        account.set_rent_epoch(1);
        accounts.push((key0, account));

        let program_data = UpgradeableLoaderState::ProgramData {
            slot: 42,
            upgrade_authority_address: None,
        };
        let mut account =
            AccountSharedData::new_data(40, &program_data, &bpf_loader_upgradeable::id()).unwrap();
        account.set_rent_epoch(1);
        accounts.push((key1, account));

        let mut account = AccountSharedData::new(40, 1, &native_loader::id());
        account.set_executable(true);
        account.set_rent_epoch(1);
        accounts.push((key2, account));

        let instructions = vec![CompiledInstruction::new(2, &(), vec![0, 1])];
        let mut message = Message::new_with_compiled_instructions(
            1,
            0,
            1, // only the program marked as readonly
            vec![key0, key1, key2],
            Hash::default(),
            instructions,
        );
        let tx = Transaction::new(&[&keypair], message.clone(), Hash::default());
        let loaded_accounts = load_accounts_with_excluded_features(
            tx.clone(),
            &accounts,
            &mut error_counters,
            Some(&[simplify_writable_program_account_check::id()]),
        );

        assert_eq!(error_counters.invalid_writable_account, 1);
        assert_eq!(loaded_accounts.len(), 1);
        assert_eq!(
            loaded_accounts[0],
            (Err(TransactionError::InvalidWritableAccount), None)
        );

        // Solution 0: Include feature simplify_writable_program_account_check
        let loaded_accounts =
            load_accounts_with_excluded_features(tx, &accounts, &mut error_counters, None);

        assert_eq!(error_counters.invalid_writable_account, 1);
        assert_eq!(loaded_accounts.len(), 1);

        // Solution 1: include bpf_loader_upgradeable account
        let mut account = AccountSharedData::new(40, 1, &native_loader::id()); // create mock bpf_loader_upgradeable
        account.set_executable(true);
        account.set_rent_epoch(1);
        let accounts_with_upgradeable_loader = vec![
            accounts[0].clone(),
            accounts[1].clone(),
            (bpf_loader_upgradeable::id(), account),
        ];
        message.account_keys = vec![key0, key1, bpf_loader_upgradeable::id()];
        let tx = Transaction::new(&[&keypair], message.clone(), Hash::default());
        let loaded_accounts = load_accounts_with_excluded_features(
            tx,
            &accounts_with_upgradeable_loader,
            &mut error_counters,
            Some(&[simplify_writable_program_account_check::id()]),
        );

        assert_eq!(error_counters.invalid_writable_account, 1);
        assert_eq!(loaded_accounts.len(), 1);
        let result = loaded_accounts[0].0.as_ref().unwrap();
        assert_eq!(result.accounts[..2], accounts_with_upgradeable_loader[..2]);
        assert_eq!(
            result.accounts[result.program_indices[0][0] as usize],
            accounts_with_upgradeable_loader[2]
        );

        // Solution 2: mark programdata as readonly
        message.account_keys = vec![key0, key1, key2]; // revert key change
        message.header.num_readonly_unsigned_accounts = 2; // extend readonly set to include programdata
        let tx = Transaction::new(&[&keypair], message, Hash::default());
        let loaded_accounts = load_accounts_with_excluded_features(
            tx,
            &accounts,
            &mut error_counters,
            Some(&[simplify_writable_program_account_check::id()]),
        );

        assert_eq!(error_counters.invalid_writable_account, 1);
        assert_eq!(loaded_accounts.len(), 1);
        let result = loaded_accounts[0].0.as_ref().unwrap();
        assert_eq!(result.accounts[..2], accounts[..2]);
        assert_eq!(
            result.accounts[result.program_indices[0][0] as usize],
            accounts[2]
        );
    }

    fn load_accounts_no_store(
        accounts: &Accounts,
        tx: Transaction,
        account_overrides: Option<&AccountOverrides>,
    ) -> Vec<TransactionLoadResult> {
        let tx = SanitizedTransaction::from_transaction_for_tests(tx);
        let rent_collector = RentCollector::default();
        let mut hash_queue = BlockhashQueue::new(100);
        hash_queue.register_hash(tx.message().recent_blockhash(), 10);

        let ancestors = vec![(0, 0)].into_iter().collect();
        let mut error_counters = TransactionErrorMetrics::default();
        load_accounts(
            &accounts.accounts_db,
            &ancestors,
            &[tx],
            vec![(Ok(()), None)],
            &hash_queue,
            &mut error_counters,
            &rent_collector,
            &FeatureSet::all_enabled(),
            &FeeStructure::default(),
            account_overrides,
            RewardInterval::OutsideInterval,
            &HashMap::new(),
            &LoadedProgramsForTxBatch::default(),
            true,
        )
    }

    #[test]
    fn test_instructions() {
        solana_logger::setup();
        let accounts = Accounts::new_with_config_for_tests(
            Vec::new(),
            &ClusterType::Development,
            AccountSecondaryIndexes::default(),
            AccountShrinkThreshold::default(),
        );

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

        let loaded_accounts = load_accounts_no_store(&accounts, tx, None);
        assert_eq!(loaded_accounts.len(), 1);
        assert!(loaded_accounts[0].0.is_err());
    }

    #[test]
    fn test_overrides() {
        solana_logger::setup();
        let accounts = Accounts::new_with_config_for_tests(
            Vec::new(),
            &ClusterType::Development,
            AccountSecondaryIndexes::default(),
            AccountShrinkThreshold::default(),
        );
        let mut account_overrides = AccountOverrides::default();
        let slot_history_id = sysvar::slot_history::id();
        let account = AccountSharedData::new(42, 0, &Pubkey::default());
        account_overrides.set_slot_history(Some(account));

        let keypair = Keypair::new();
        let account = AccountSharedData::new(1_000_000, 0, &Pubkey::default());
        accounts.store_slow_uncached(0, &keypair.pubkey(), &account);

        let instructions = vec![CompiledInstruction::new(2, &(), vec![0])];
        let tx = Transaction::new_with_compiled_instructions(
            &[&keypair],
            &[slot_history_id],
            Hash::default(),
            vec![native_loader::id()],
            instructions,
        );

        let loaded_accounts = load_accounts_no_store(&accounts, tx, Some(&account_overrides));
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
            feature_set: &FeatureSet,
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
                get_requested_loaded_accounts_data_size_limit(&tx, feature_set)
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

        let result_no_limit = Ok(None);
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

        let mut feature_set = FeatureSet::default();

        // if `cap_transaction_accounts_data_size feature` is disable,
        // the result will always be no limit
        test(tx_not_set_limit, &feature_set, &result_no_limit);
        test(tx_set_limit_99, &feature_set, &result_no_limit);
        test(tx_set_limit_0, &feature_set, &result_no_limit);

        // if `cap_transaction_accounts_data_size` is enabled, and
        //    `add_set_tx_loaded_accounts_data_size_instruction` is disabled,
        // the result will always be default limit (64MiB)
        feature_set.activate(&feature_set::cap_transaction_accounts_data_size::id(), 0);
        test(tx_not_set_limit, &feature_set, &result_default_limit);
        test(tx_set_limit_99, &feature_set, &result_default_limit);
        test(tx_set_limit_0, &feature_set, &result_default_limit);

        // if `cap_transaction_accounts_data_size` and
        //    `add_set_tx_loaded_accounts_data_size_instruction` are both enabled,
        // the results are:
        //    if tx doesn't set limit, then default limit (64MiB)
        //    if tx sets limit, then requested limit
        //    if tx sets limit to zero, then TransactionError::InvalidLoadedAccountsDataSizeLimit
        feature_set.activate(
            &solana_sdk::feature_set::add_set_tx_loaded_accounts_data_size_instruction::id(),
            0,
        );
        test(tx_not_set_limit, &feature_set, &result_default_limit);
        test(tx_set_limit_99, &feature_set, &result_requested_limit);
        test(tx_set_limit_0, &feature_set, &result_invalid_limit);
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

        let mut feature_set = FeatureSet::all_enabled();
        feature_set.deactivate(&solana_sdk::feature_set::remove_deprecated_request_unit_ix::id());

        let message = SanitizedMessage::try_from(tx.message().clone()).unwrap();
        let fee = FeeStructure::default().calculate_fee(
            &message,
            lamports_per_signature,
            &process_compute_budget_instructions(message.program_instructions_iter(), &feature_set)
                .unwrap_or_default()
                .into(),
            true,
            false,
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
        feature_checked_arithmmetic_enable: bool,
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
        let mut feature_set = FeatureSet::default();
        if test_parameter.feature_checked_arithmmetic_enable {
            feature_set.activate(&feature_set::checked_arithmetic_in_fee_validation::id(), 0);
        };
        let result = validate_fee_payer(
            &payer_account_keys.pubkey(),
            &mut account,
            0,
            &mut TransactionErrorMetrics::default(),
            rent_collector,
            &feature_set,
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
            for feature_checked_arithmmetic_enable in [true, false] {
                for (is_nonce, min_balance) in [(true, min_balance), (false, 0)] {
                    validate_fee_payer_account(
                        ValidateFeePayerTestParameter {
                            is_nonce,
                            payer_init_balance: min_balance + fee,
                            fee,
                            expected_result: Ok(()),
                            payer_post_balance: min_balance,
                            feature_checked_arithmmetic_enable,
                        },
                        &rent_collector,
                    );
                }
            }
        }

        // If payer account has no balance, expected AccountNotFound Error
        // regardless feature gate status, or if payer is nonce account.
        {
            for feature_checked_arithmmetic_enable in [true, false] {
                for is_nonce in [true, false] {
                    validate_fee_payer_account(
                        ValidateFeePayerTestParameter {
                            is_nonce,
                            payer_init_balance: 0,
                            fee,
                            expected_result: Err(TransactionError::AccountNotFound),
                            payer_post_balance: 0,
                            feature_checked_arithmmetic_enable,
                        },
                        &rent_collector,
                    );
                }
            }
        }

        // If payer account has insufficent balance, expect InsufficientFundsForFee error
        // regardless feature gate status, or if payer is nonce account.
        {
            for feature_checked_arithmmetic_enable in [true, false] {
                for (is_nonce, min_balance) in [(true, min_balance), (false, 0)] {
                    validate_fee_payer_account(
                        ValidateFeePayerTestParameter {
                            is_nonce,
                            payer_init_balance: min_balance + fee - 1,
                            fee,
                            expected_result: Err(TransactionError::InsufficientFundsForFee),
                            payer_post_balance: min_balance + fee - 1,
                            feature_checked_arithmmetic_enable,
                        },
                        &rent_collector,
                    );
                }
            }
        }

        // normal payer account has balance of u64::MAX, so does fee; since it does not  require
        // min_balance, expect successful fee deduction, regardless of feature gate status
        {
            for feature_checked_arithmmetic_enable in [true, false] {
                validate_fee_payer_account(
                    ValidateFeePayerTestParameter {
                        is_nonce: false,
                        payer_init_balance: u64::MAX,
                        fee: u64::MAX,
                        expected_result: Ok(()),
                        payer_post_balance: 0,
                        feature_checked_arithmmetic_enable,
                    },
                    &rent_collector,
                );
            }
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
                feature_checked_arithmmetic_enable: true,
            },
            &rent_collector,
        );
    }

    #[test]
    #[should_panic]
    fn test_validate_nonce_fee_payer_without_checked_arithmetic() {
        let rent_collector = RentCollector::new(
            0,
            EpochSchedule::default(),
            500_000.0,
            Rent {
                lamports_per_byte_year: 1,
                ..Rent::default()
            },
        );

        // same test setup as `test_validate_nonce_fee_payer_with_checked_arithmetic`:
        // nonce payer account has balance of u64::MAX, so does fee; and nonce account
        // requires additional min_balance, if feature gate is not enabled, in `debug`
        // mode, `u64::MAX + min_balance` would panic on "attempt to add with overflow";
        // in `release` mode, the addition will wrap, so the expected result would be
        // `Ok(())` with post payer balance `0`, therefore fails test with a panic.
        validate_fee_payer_account(
            ValidateFeePayerTestParameter {
                is_nonce: true,
                payer_init_balance: u64::MAX,
                fee: u64::MAX,
                expected_result: Err(TransactionError::InsufficientFundsForFee),
                payer_post_balance: u64::MAX,
                feature_checked_arithmmetic_enable: false,
            },
            &rent_collector,
        );
    }
}
