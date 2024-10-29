#![cfg(test)]
use {
    super::{
        test_utils::{goto_end_of_slot, update_vote_account_timestamp},
        *,
    },
    crate::{
        accounts_background_service::{PrunedBanksRequestHandler, SendDroppedBankCallback},
        bank_client::BankClient,
        bank_forks::BankForks,
        genesis_utils::{
            self, activate_all_features, activate_feature, bootstrap_validator_stake_lamports,
            create_genesis_config_with_leader, create_genesis_config_with_vote_accounts,
            genesis_sysvar_and_builtin_program_lamports, GenesisConfigInfo, ValidatorVoteKeypairs,
        },
        snapshot_bank_utils, snapshot_utils,
        status_cache::MAX_CACHE_ENTRIES,
    },
    agave_transaction_view::static_account_keys_frame::MAX_STATIC_ACCOUNTS_PER_PACKET,
    assert_matches::assert_matches,
    crossbeam_channel::{bounded, unbounded},
    itertools::Itertools,
    rand::Rng,
    rayon::ThreadPoolBuilder,
    serde::{Deserialize, Serialize},
    solana_accounts_db::{
        accounts::AccountAddressFilter,
        accounts_db::DEFAULT_ACCOUNTS_SHRINK_RATIO,
        accounts_hash::{AccountsDeltaHash, AccountsHasher},
        accounts_index::{
            AccountIndex, AccountSecondaryIndexes, IndexKey, ScanConfig, ScanError, ITER_BATCH_SIZE,
        },
        accounts_partition::{self, PartitionIndex, RentPayingAccountsByPartition},
        ancestors::Ancestors,
    },
    solana_compute_budget::{
        compute_budget::ComputeBudget,
        compute_budget_limits::{self, MAX_COMPUTE_UNIT_LIMIT},
        prioritization_fee::{PrioritizationFeeDetails, PrioritizationFeeType},
    },
    solana_feature_set::{self as feature_set, FeatureSet},
    solana_inline_spl::token,
    solana_logger,
    solana_program_runtime::{
        declare_process_instruction,
        loaded_programs::{ProgramCacheEntry, ProgramCacheEntryType},
    },
    solana_sdk::{
        account::{
            accounts_equal, create_account_shared_data_with_fields as create_account, from_account,
            Account, AccountSharedData, ReadableAccount, WritableAccount,
        },
        account_utils::StateMut,
        bpf_loader,
        bpf_loader_upgradeable::{self, UpgradeableLoaderState},
        client::SyncClient,
        clock::{
            BankId, Epoch, Slot, UnixTimestamp, DEFAULT_HASHES_PER_TICK, DEFAULT_SLOTS_PER_EPOCH,
            DEFAULT_TICKS_PER_SLOT, INITIAL_RENT_EPOCH, MAX_PROCESSING_AGE, MAX_RECENT_BLOCKHASHES,
            UPDATED_HASHES_PER_TICK2, UPDATED_HASHES_PER_TICK3, UPDATED_HASHES_PER_TICK4,
            UPDATED_HASHES_PER_TICK5, UPDATED_HASHES_PER_TICK6,
        },
        compute_budget::ComputeBudgetInstruction,
        entrypoint::MAX_PERMITTED_DATA_INCREASE,
        epoch_schedule::{EpochSchedule, MINIMUM_SLOTS_PER_EPOCH},
        feature::{self, Feature},
        fee::FeeStructure,
        fee_calculator::FeeRateGovernor,
        genesis_config::{ClusterType, GenesisConfig},
        hash::{hash, Hash},
        incinerator,
        instruction::{AccountMeta, CompiledInstruction, Instruction, InstructionError},
        loader_upgradeable_instruction::UpgradeableLoaderInstruction,
        message::{Message, MessageHeader, SanitizedMessage},
        native_loader,
        native_token::{sol_to_lamports, LAMPORTS_PER_SOL},
        nonce::{self, state::DurableNonce},
        packet::PACKET_DATA_SIZE,
        poh_config::PohConfig,
        program::MAX_RETURN_DATA,
        pubkey::Pubkey,
        rent::Rent,
        rent_collector::RENT_EXEMPT_RENT_EPOCH,
        reward_type::RewardType,
        secp256k1_program,
        signature::{keypair_from_seed, Keypair, Signature, Signer},
        stake::{
            instruction as stake_instruction,
            state::{Authorized, Delegation, Lockup, Stake},
        },
        system_instruction::{
            self, SystemError, MAX_PERMITTED_ACCOUNTS_DATA_ALLOCATIONS_PER_TRANSACTION,
            MAX_PERMITTED_DATA_LENGTH,
        },
        system_program, system_transaction, sysvar,
        timing::years_as_slots,
        transaction::{
            Result, SanitizedTransaction, Transaction, TransactionError,
            TransactionVerificationMode,
        },
        transaction_context::TransactionAccount,
    },
    solana_stake_program::stake_state::{self, StakeStateV2},
    solana_svm::{
        account_loader::{FeesOnlyTransaction, LoadedTransaction},
        rollback_accounts::RollbackAccounts,
        transaction_commit_result::TransactionCommitResultExtensions,
        transaction_execution_result::ExecutedTransaction,
    },
    solana_svm_transaction::svm_message::SVMMessage,
    solana_timings::ExecuteTimings,
    solana_vote_program::{
        vote_instruction,
        vote_state::{
            self, create_account_with_authorized, BlockTimestamp, Vote, VoteInit, VoteState,
            VoteStateVersions, MAX_LOCKOUT_HISTORY,
        },
    },
    std::{
        collections::{HashMap, HashSet},
        convert::TryInto,
        fs::File,
        io::Read,
        str::FromStr,
        sync::{
            atomic::{
                AtomicBool, AtomicU64,
                Ordering::{Relaxed, Release},
            },
            Arc,
        },
        thread::Builder,
        time::{Duration, Instant},
    },
    tempfile::TempDir,
    test_case::test_case,
};

impl VoteReward {
    pub fn new_random() -> Self {
        let mut rng = rand::thread_rng();

        let validator_pubkey = solana_sdk::pubkey::new_rand();
        let validator_stake_lamports = rng.gen_range(1..200);
        let validator_voting_keypair = Keypair::new();

        let validator_vote_account = vote_state::create_account(
            &validator_voting_keypair.pubkey(),
            &validator_pubkey,
            rng.gen_range(1..20),
            validator_stake_lamports,
        );

        Self {
            vote_account: validator_vote_account,
            commission: rng.gen_range(1..20),
            vote_rewards: rng.gen_range(1..200),
            vote_needs_store: rng.gen_range(1..20) > 10,
        }
    }
}

pub(in crate::bank) fn new_bank_from_parent_with_bank_forks(
    bank_forks: &RwLock<BankForks>,
    parent: Arc<Bank>,
    collector_id: &Pubkey,
    slot: Slot,
) -> Arc<Bank> {
    let bank = Bank::new_from_parent(parent, collector_id, slot);
    bank_forks
        .write()
        .unwrap()
        .insert(bank)
        .clone_without_scheduler()
}

fn create_genesis_config_no_tx_fee_no_rent(lamports: u64) -> (GenesisConfig, Keypair) {
    // genesis_util creates config with no tx fee and no rent
    let genesis_config_info = solana_runtime::genesis_utils::create_genesis_config(lamports);
    (
        genesis_config_info.genesis_config,
        genesis_config_info.mint_keypair,
    )
}

fn create_genesis_config_no_tx_fee(lamports: u64) -> (GenesisConfig, Keypair) {
    // genesis_config creates config with default fee rate and default rent
    // override to set fee rate to zero.
    let (mut genesis_config, mint_keypair) =
        solana_sdk::genesis_config::create_genesis_config(lamports);
    genesis_config.fee_rate_governor = FeeRateGovernor::new(0, 0);
    (genesis_config, mint_keypair)
}

pub(in crate::bank) fn create_genesis_config(lamports: u64) -> (GenesisConfig, Keypair) {
    solana_sdk::genesis_config::create_genesis_config(lamports)
}

pub(in crate::bank) fn new_sanitized_message(message: Message) -> SanitizedMessage {
    SanitizedMessage::try_from_legacy_message(message, &ReservedAccountKeys::empty_key_set())
        .unwrap()
}

#[test]
fn test_race_register_tick_freeze() {
    solana_logger::setup();

    let (mut genesis_config, _) = create_genesis_config(50);
    genesis_config.ticks_per_slot = 1;
    let p = solana_sdk::pubkey::new_rand();
    let hash = hash(p.as_ref());

    for _ in 0..1000 {
        let bank0 = Arc::new(Bank::new_for_tests(&genesis_config));
        let bank0_ = bank0.clone();
        let freeze_thread = Builder::new()
            .name("freeze".to_string())
            .spawn(move || loop {
                if bank0_.is_complete() {
                    assert_eq!(bank0_.last_blockhash(), hash);
                    break;
                }
            })
            .unwrap();

        let bank0_ = bank0.clone();
        let register_tick_thread = Builder::new()
            .name("register_tick".to_string())
            .spawn(move || {
                bank0_.register_tick_for_test(&hash);
            })
            .unwrap();

        register_tick_thread.join().unwrap();
        freeze_thread.join().unwrap();
    }
}

fn new_executed_processing_result(
    status: Result<()>,
    fee_details: FeeDetails,
) -> TransactionProcessingResult {
    Ok(ProcessedTransaction::Executed(Box::new(
        ExecutedTransaction {
            loaded_transaction: LoadedTransaction {
                fee_details,
                ..LoadedTransaction::default()
            },
            execution_details: TransactionExecutionDetails {
                status,
                log_messages: None,
                inner_instructions: None,
                return_data: None,
                executed_units: 0,
                accounts_data_len_delta: 0,
            },
            programs_modified_by_tx: HashMap::new(),
        },
    )))
}

impl Bank {
    fn clean_accounts_for_tests(&self) {
        self.rc.accounts.accounts_db.clean_accounts_for_tests()
    }
}

#[test]
fn test_bank_unix_timestamp_from_genesis() {
    let (genesis_config, _mint_keypair) = create_genesis_config(1);
    let mut bank = Arc::new(Bank::new_for_tests(&genesis_config));

    assert_eq!(
        genesis_config.creation_time,
        bank.unix_timestamp_from_genesis()
    );
    let slots_per_sec = (genesis_config.poh_config.target_tick_duration.as_secs_f32()
        * genesis_config.ticks_per_slot as f32)
        .recip();

    for _i in 0..slots_per_sec as usize + 1 {
        bank = Arc::new(new_from_parent(bank));
    }

    assert!(bank.unix_timestamp_from_genesis() - genesis_config.creation_time >= 1);
}

#[test]
fn test_bank_new() {
    let dummy_leader_pubkey = solana_sdk::pubkey::new_rand();
    let dummy_leader_stake_lamports = bootstrap_validator_stake_lamports();
    let mint_lamports = 10_000;
    let GenesisConfigInfo {
        mut genesis_config,
        mint_keypair,
        voting_keypair,
        ..
    } = create_genesis_config_with_leader(
        mint_lamports,
        &dummy_leader_pubkey,
        dummy_leader_stake_lamports,
    );

    genesis_config.rent = Rent {
        lamports_per_byte_year: 5,
        exemption_threshold: 1.2,
        burn_percent: 5,
    };

    let bank = Bank::new_for_tests(&genesis_config);
    assert_eq!(bank.get_balance(&mint_keypair.pubkey()), mint_lamports);
    assert_eq!(
        bank.get_balance(&voting_keypair.pubkey()),
        dummy_leader_stake_lamports /* 1 token goes to the vote account associated with dummy_leader_lamports */
    );

    let rent_account = bank.get_account(&sysvar::rent::id()).unwrap();
    let rent = from_account::<sysvar::rent::Rent, _>(&rent_account).unwrap();

    assert_eq!(rent.burn_percent, 5);
    assert_eq!(rent.exemption_threshold, 1.2);
    assert_eq!(rent.lamports_per_byte_year, 5);
}

pub(crate) fn create_simple_test_bank(lamports: u64) -> Bank {
    let (genesis_config, _mint_keypair) = create_genesis_config(lamports);
    Bank::new_for_tests(&genesis_config)
}

fn create_simple_test_arc_bank(lamports: u64) -> (Arc<Bank>, Arc<RwLock<BankForks>>) {
    let bank = create_simple_test_bank(lamports);
    bank.wrap_with_bank_forks_for_tests()
}

#[test]
fn test_bank_block_height() {
    let (bank0, _bank_forks) = create_simple_test_arc_bank(1);
    assert_eq!(bank0.block_height(), 0);
    let bank1 = Arc::new(new_from_parent(bank0));
    assert_eq!(bank1.block_height(), 1);
}

#[test]
fn test_bank_update_epoch_stakes() {
    #[allow(non_local_definitions)]
    impl Bank {
        fn epoch_stake_keys(&self) -> Vec<Epoch> {
            let mut keys: Vec<Epoch> = self.epoch_stakes.keys().copied().collect();
            keys.sort_unstable();
            keys
        }

        fn epoch_stake_key_info(&self) -> (Epoch, Epoch, usize) {
            let mut keys: Vec<Epoch> = self.epoch_stakes.keys().copied().collect();
            keys.sort_unstable();
            (*keys.first().unwrap(), *keys.last().unwrap(), keys.len())
        }
    }

    let mut bank = create_simple_test_bank(100_000);

    let initial_epochs = bank.epoch_stake_keys();
    assert_eq!(initial_epochs, vec![0, 1]);

    for existing_epoch in &initial_epochs {
        bank.update_epoch_stakes(*existing_epoch);
        assert_eq!(bank.epoch_stake_keys(), initial_epochs);
    }

    for epoch in (initial_epochs.len() as Epoch)..MAX_LEADER_SCHEDULE_STAKES {
        bank.update_epoch_stakes(epoch);
        assert_eq!(bank.epoch_stakes.len() as Epoch, epoch + 1);
    }

    assert_eq!(
        bank.epoch_stake_key_info(),
        (
            0,
            MAX_LEADER_SCHEDULE_STAKES - 1,
            MAX_LEADER_SCHEDULE_STAKES as usize
        )
    );

    bank.update_epoch_stakes(MAX_LEADER_SCHEDULE_STAKES);
    assert_eq!(
        bank.epoch_stake_key_info(),
        (
            0,
            MAX_LEADER_SCHEDULE_STAKES,
            MAX_LEADER_SCHEDULE_STAKES as usize + 1
        )
    );

    bank.update_epoch_stakes(MAX_LEADER_SCHEDULE_STAKES + 1);
    assert_eq!(
        bank.epoch_stake_key_info(),
        (
            1,
            MAX_LEADER_SCHEDULE_STAKES + 1,
            MAX_LEADER_SCHEDULE_STAKES as usize + 1
        )
    );
}

fn bank0_sysvar_delta() -> u64 {
    const SLOT_HISTORY_SYSVAR_MIN_BALANCE: u64 = 913_326_000;
    SLOT_HISTORY_SYSVAR_MIN_BALANCE
}

fn bank1_sysvar_delta() -> u64 {
    const SLOT_HASHES_SYSVAR_MIN_BALANCE: u64 = 143_487_360;
    SLOT_HASHES_SYSVAR_MIN_BALANCE
}

#[test]
fn test_bank_capitalization() {
    let bank0 = Arc::new(Bank::new_for_tests(&GenesisConfig {
        accounts: (0..42)
            .map(|_| {
                (
                    solana_sdk::pubkey::new_rand(),
                    Account::new(42, 0, &Pubkey::default()),
                )
            })
            .collect(),
        cluster_type: ClusterType::MainnetBeta,
        ..GenesisConfig::default()
    }));

    assert_eq!(
        bank0.capitalization(),
        42 * 42 + genesis_sysvar_and_builtin_program_lamports(),
    );

    bank0.freeze();

    assert_eq!(
        bank0.capitalization(),
        42 * 42 + genesis_sysvar_and_builtin_program_lamports() + bank0_sysvar_delta(),
    );

    let bank1 = Bank::new_from_parent(bank0, &Pubkey::default(), 1);
    assert_eq!(
        bank1.capitalization(),
        42 * 42
            + genesis_sysvar_and_builtin_program_lamports()
            + bank0_sysvar_delta()
            + bank1_sysvar_delta(),
    );
}

fn rent_with_exemption_threshold(exemption_threshold: f64) -> Rent {
    Rent {
        lamports_per_byte_year: 1,
        exemption_threshold,
        burn_percent: 10,
    }
}

#[test]
/// one thing being tested here is that a failed tx (due to rent collection using up all lamports) followed by rent collection
/// results in the same state as if just rent collection ran (and emptied the accounts that have too few lamports)
fn test_credit_debit_rent_no_side_effect_on_hash() {
    solana_logger::setup();

    let (mut genesis_config, _mint_keypair) = create_genesis_config_no_tx_fee(10);

    genesis_config.rent = rent_with_exemption_threshold(21.0);

    let slot = years_as_slots(
        2.0,
        &genesis_config.poh_config.target_tick_duration,
        genesis_config.ticks_per_slot,
    ) as u64;
    let (root_bank, bank_forks_1) = Bank::new_with_bank_forks_for_tests(&genesis_config);
    let bank = new_bank_from_parent_with_bank_forks(
        bank_forks_1.as_ref(),
        root_bank,
        &Pubkey::default(),
        slot,
    );

    let (root_bank_2, bank_forks_2) = Bank::new_with_bank_forks_for_tests(&genesis_config);
    let bank_with_success_txs = new_bank_from_parent_with_bank_forks(
        bank_forks_2.as_ref(),
        root_bank_2,
        &Pubkey::default(),
        slot,
    );

    assert_eq!(bank.last_blockhash(), genesis_config.hash());

    let plenty_of_lamports = 264;
    let too_few_lamports = 10;
    // Initialize credit-debit and credit only accounts
    let accounts = [
        AccountSharedData::new(plenty_of_lamports, 0, &Pubkey::default()),
        AccountSharedData::new(plenty_of_lamports, 1, &Pubkey::default()),
        AccountSharedData::new(plenty_of_lamports, 0, &Pubkey::default()),
        AccountSharedData::new(plenty_of_lamports, 1, &Pubkey::default()),
        // Transaction between these two accounts will fail
        AccountSharedData::new(too_few_lamports, 0, &Pubkey::default()),
        AccountSharedData::new(too_few_lamports, 1, &Pubkey::default()),
    ];

    let keypairs = accounts.iter().map(|_| Keypair::new()).collect::<Vec<_>>();
    {
        // make sure rent and epoch change are such that we collect all lamports in accounts 4 & 5
        let mut account_copy = accounts[4].clone();
        let expected_rent = bank
            .rent_collector()
            .collect_from_existing_account(&keypairs[4].pubkey(), &mut account_copy);
        assert_eq!(expected_rent.rent_amount, too_few_lamports);
        assert_eq!(account_copy.lamports(), 0);
    }

    for i in 0..accounts.len() {
        let account = &accounts[i];
        bank.store_account(&keypairs[i].pubkey(), account);
        bank_with_success_txs.store_account(&keypairs[i].pubkey(), account);
    }

    // Make builtin instruction loader rent exempt
    let system_program_id = system_program::id();
    let mut system_program_account = bank.get_account(&system_program_id).unwrap();
    system_program_account.set_lamports(
        bank.get_minimum_balance_for_rent_exemption(system_program_account.data().len()),
    );
    bank.store_account(&system_program_id, &system_program_account);
    bank_with_success_txs.store_account(&system_program_id, &system_program_account);

    let t1 = system_transaction::transfer(
        &keypairs[0],
        &keypairs[1].pubkey(),
        1,
        genesis_config.hash(),
    );
    let t2 = system_transaction::transfer(
        &keypairs[2],
        &keypairs[3].pubkey(),
        1,
        genesis_config.hash(),
    );
    // the idea is this transaction will result in both accounts being drained of all lamports due to rent collection
    let t3 = system_transaction::transfer(
        &keypairs[4],
        &keypairs[5].pubkey(),
        1,
        genesis_config.hash(),
    );

    let txs = vec![t1.clone(), t2.clone(), t3];
    let res = bank.process_transactions(txs.iter());

    assert_eq!(res.len(), 3);
    assert_eq!(res[0], Ok(()));
    assert_eq!(res[1], Ok(()));
    assert_eq!(res[2], Err(TransactionError::AccountNotFound));

    bank.freeze();

    let rwlockguard_bank_hash = bank.hash.read().unwrap();
    let bank_hash = rwlockguard_bank_hash.as_ref();

    let txs = vec![t2, t1];
    let res = bank_with_success_txs.process_transactions(txs.iter());

    assert_eq!(res.len(), 2);
    assert_eq!(res[0], Ok(()));
    assert_eq!(res[1], Ok(()));

    bank_with_success_txs.freeze();

    let rwlockguard_bank_with_success_txs_hash = bank_with_success_txs.hash.read().unwrap();
    let bank_with_success_txs_hash = rwlockguard_bank_with_success_txs_hash.as_ref();

    assert_eq!(bank_with_success_txs_hash, bank_hash);
}

fn store_accounts_for_rent_test(
    bank: &Bank,
    keypairs: &[Keypair],
    mock_program_id: Pubkey,
    generic_rent_due_for_system_account: u64,
) {
    let mut account_pairs: Vec<TransactionAccount> = Vec::with_capacity(keypairs.len() - 1);
    account_pairs.push((
        keypairs[0].pubkey(),
        AccountSharedData::new(
            generic_rent_due_for_system_account + 2,
            0,
            &Pubkey::default(),
        ),
    ));
    account_pairs.push((
        keypairs[1].pubkey(),
        AccountSharedData::new(
            generic_rent_due_for_system_account + 2,
            0,
            &Pubkey::default(),
        ),
    ));
    account_pairs.push((
        keypairs[2].pubkey(),
        AccountSharedData::new(
            generic_rent_due_for_system_account + 2,
            0,
            &Pubkey::default(),
        ),
    ));
    account_pairs.push((
        keypairs[3].pubkey(),
        AccountSharedData::new(
            generic_rent_due_for_system_account + 2,
            0,
            &Pubkey::default(),
        ),
    ));
    account_pairs.push((
        keypairs[4].pubkey(),
        AccountSharedData::new(10, 0, &Pubkey::default()),
    ));
    account_pairs.push((
        keypairs[5].pubkey(),
        AccountSharedData::new(10, 0, &Pubkey::default()),
    ));
    account_pairs.push((
        keypairs[6].pubkey(),
        AccountSharedData::new(
            (2 * generic_rent_due_for_system_account) + 24,
            0,
            &Pubkey::default(),
        ),
    ));

    account_pairs.push((
        keypairs[8].pubkey(),
        AccountSharedData::new(
            generic_rent_due_for_system_account + 2 + 929,
            0,
            &Pubkey::default(),
        ),
    ));
    account_pairs.push((
        keypairs[9].pubkey(),
        AccountSharedData::new(10, 0, &Pubkey::default()),
    ));

    // Feeding to MockProgram to test read only rent behaviour
    account_pairs.push((
        keypairs[10].pubkey(),
        AccountSharedData::new(
            generic_rent_due_for_system_account + 3,
            0,
            &Pubkey::default(),
        ),
    ));
    account_pairs.push((
        keypairs[11].pubkey(),
        AccountSharedData::new(generic_rent_due_for_system_account + 3, 0, &mock_program_id),
    ));
    account_pairs.push((
        keypairs[12].pubkey(),
        AccountSharedData::new(generic_rent_due_for_system_account + 3, 0, &mock_program_id),
    ));
    account_pairs.push((
        keypairs[13].pubkey(),
        AccountSharedData::new(14, 22, &mock_program_id),
    ));

    for account_pair in account_pairs.iter() {
        bank.store_account(&account_pair.0, &account_pair.1);
    }
}

fn create_child_bank_for_rent_test(
    root_bank: Arc<Bank>,
    genesis_config: &GenesisConfig,
    bank_forks: &RwLock<BankForks>,
    mock_builtin: Option<(Pubkey, BuiltinFunctionWithContext)>,
) -> Arc<Bank> {
    let mut bank = Bank::new_from_parent(
        root_bank,
        &Pubkey::default(),
        years_as_slots(
            2.0,
            &genesis_config.poh_config.target_tick_duration,
            genesis_config.ticks_per_slot,
        ) as u64,
    );
    bank.rent_collector.slots_per_year = 421_812.0;
    if let Some((program_id, builtin_function)) = mock_builtin {
        bank.add_mockup_builtin(program_id, builtin_function);
    }
    bank_forks
        .write()
        .unwrap()
        .insert(bank)
        .clone_without_scheduler()
}

/// if asserter returns true, check the capitalization
/// Checking the capitalization requires that the bank be a root and the slot be flushed.
/// All tests are getting converted to use the write cache, so over time, each caller will be visited to throttle this input.
/// Flushing the cache has a side effects on the test, so often the test has to be restarted to continue to function correctly.
fn assert_capitalization_diff(
    bank: &Bank,
    updater: impl Fn(),
    asserter: impl Fn(u64, u64) -> bool,
) {
    let old = bank.capitalization();
    updater();
    let new = bank.capitalization();
    if asserter(old, new) {
        add_root_and_flush_write_cache(bank);
        assert_eq!(bank.capitalization(), bank.calculate_capitalization(true));
    }
}

declare_process_instruction!(MockBuiltin, 1, |_invoke_context| {
    // Default for all tests which don't bring their own processor
    Ok(())
});

#[test]
fn test_store_account_and_update_capitalization_missing() {
    let bank = create_simple_test_bank(0);
    let pubkey = solana_sdk::pubkey::new_rand();

    let some_lamports = 400;
    let account = AccountSharedData::new(some_lamports, 0, &system_program::id());

    assert_capitalization_diff(
        &bank,
        || bank.store_account_and_update_capitalization(&pubkey, &account),
        |old, new| {
            assert_eq!(old + some_lamports, new);
            true
        },
    );
    assert_eq!(account, bank.get_account(&pubkey).unwrap());
}

#[test]
fn test_store_account_and_update_capitalization_increased() {
    let old_lamports = 400;
    let (genesis_config, mint_keypair) = create_genesis_config(old_lamports);
    let bank = Bank::new_for_tests(&genesis_config);
    let pubkey = mint_keypair.pubkey();

    let new_lamports = 500;
    let account = AccountSharedData::new(new_lamports, 0, &system_program::id());

    assert_capitalization_diff(
        &bank,
        || bank.store_account_and_update_capitalization(&pubkey, &account),
        |old, new| {
            assert_eq!(old + 100, new);
            true
        },
    );
    assert_eq!(account, bank.get_account(&pubkey).unwrap());
}

#[test]
fn test_store_account_and_update_capitalization_decreased() {
    let old_lamports = 400;
    let (genesis_config, mint_keypair) = create_genesis_config(old_lamports);
    let bank = Bank::new_for_tests(&genesis_config);
    let pubkey = mint_keypair.pubkey();

    let new_lamports = 100;
    let account = AccountSharedData::new(new_lamports, 0, &system_program::id());

    assert_capitalization_diff(
        &bank,
        || bank.store_account_and_update_capitalization(&pubkey, &account),
        |old, new| {
            assert_eq!(old - 300, new);
            true
        },
    );
    assert_eq!(account, bank.get_account(&pubkey).unwrap());
}

#[test]
fn test_store_account_and_update_capitalization_unchanged() {
    let lamports = 400;
    let (genesis_config, mint_keypair) = create_genesis_config(lamports);
    let bank = Bank::new_for_tests(&genesis_config);
    let pubkey = mint_keypair.pubkey();

    let account = AccountSharedData::new(lamports, 1, &system_program::id());

    assert_capitalization_diff(
        &bank,
        || bank.store_account_and_update_capitalization(&pubkey, &account),
        |old, new| {
            assert_eq!(old, new);
            true
        },
    );
    assert_eq!(account, bank.get_account(&pubkey).unwrap());
}

#[test]
#[ignore]
fn test_rent_distribution() {
    solana_logger::setup();

    let bootstrap_validator_pubkey = solana_sdk::pubkey::new_rand();
    let bootstrap_validator_stake_lamports = 30;
    let mut genesis_config = create_genesis_config_with_leader(
        10,
        &bootstrap_validator_pubkey,
        bootstrap_validator_stake_lamports,
    )
    .genesis_config;
    // While we are preventing new accounts left in a rent-paying state, not quite ready to rip
    // out all the rent assessment tests. Just deactivate the feature for now.
    genesis_config
        .accounts
        .remove(&feature_set::require_rent_exempt_accounts::id())
        .unwrap();

    genesis_config.epoch_schedule = EpochSchedule::custom(
        MINIMUM_SLOTS_PER_EPOCH,
        genesis_config.epoch_schedule.leader_schedule_slot_offset,
        false,
    );

    genesis_config.rent = rent_with_exemption_threshold(2.0);

    let rent = Rent::free();

    let validator_1_pubkey = solana_sdk::pubkey::new_rand();
    let validator_1_stake_lamports = 20;
    let validator_1_staking_keypair = Keypair::new();
    let validator_1_voting_keypair = Keypair::new();

    let validator_1_vote_account = vote_state::create_account(
        &validator_1_voting_keypair.pubkey(),
        &validator_1_pubkey,
        0,
        validator_1_stake_lamports,
    );

    let validator_1_stake_account = stake_state::create_account(
        &validator_1_staking_keypair.pubkey(),
        &validator_1_voting_keypair.pubkey(),
        &validator_1_vote_account,
        &rent,
        validator_1_stake_lamports,
    );

    genesis_config.accounts.insert(
        validator_1_pubkey,
        Account::new(42, 0, &system_program::id()),
    );
    genesis_config.accounts.insert(
        validator_1_staking_keypair.pubkey(),
        Account::from(validator_1_stake_account),
    );
    genesis_config.accounts.insert(
        validator_1_voting_keypair.pubkey(),
        Account::from(validator_1_vote_account),
    );

    let validator_2_pubkey = solana_sdk::pubkey::new_rand();
    let validator_2_stake_lamports = 20;
    let validator_2_staking_keypair = Keypair::new();
    let validator_2_voting_keypair = Keypair::new();

    let validator_2_vote_account = vote_state::create_account(
        &validator_2_voting_keypair.pubkey(),
        &validator_2_pubkey,
        0,
        validator_2_stake_lamports,
    );

    let validator_2_stake_account = stake_state::create_account(
        &validator_2_staking_keypair.pubkey(),
        &validator_2_voting_keypair.pubkey(),
        &validator_2_vote_account,
        &rent,
        validator_2_stake_lamports,
    );

    genesis_config.accounts.insert(
        validator_2_pubkey,
        Account::new(42, 0, &system_program::id()),
    );
    genesis_config.accounts.insert(
        validator_2_staking_keypair.pubkey(),
        Account::from(validator_2_stake_account),
    );
    genesis_config.accounts.insert(
        validator_2_voting_keypair.pubkey(),
        Account::from(validator_2_vote_account),
    );

    let validator_3_pubkey = solana_sdk::pubkey::new_rand();
    let validator_3_stake_lamports = 30;
    let validator_3_staking_keypair = Keypair::new();
    let validator_3_voting_keypair = Keypair::new();

    let validator_3_vote_account = vote_state::create_account(
        &validator_3_voting_keypair.pubkey(),
        &validator_3_pubkey,
        0,
        validator_3_stake_lamports,
    );

    let validator_3_stake_account = stake_state::create_account(
        &validator_3_staking_keypair.pubkey(),
        &validator_3_voting_keypair.pubkey(),
        &validator_3_vote_account,
        &rent,
        validator_3_stake_lamports,
    );

    genesis_config.accounts.insert(
        validator_3_pubkey,
        Account::new(42, 0, &system_program::id()),
    );
    genesis_config.accounts.insert(
        validator_3_staking_keypair.pubkey(),
        Account::from(validator_3_stake_account),
    );
    genesis_config.accounts.insert(
        validator_3_voting_keypair.pubkey(),
        Account::from(validator_3_vote_account),
    );

    genesis_config.rent = rent_with_exemption_threshold(10.0);

    let mut bank = Bank::new_for_tests(&genesis_config);
    // Enable rent collection
    bank.rent_collector.epoch = 5;
    bank.rent_collector.slots_per_year = 192.0;
    let (bank, _bank_forks) = bank.wrap_with_bank_forks_for_tests();

    let payer = Keypair::new();
    let payer_account = AccountSharedData::new(400, 0, &system_program::id());
    bank.store_account_and_update_capitalization(&payer.pubkey(), &payer_account);

    let payee = Keypair::new();
    let payee_account = AccountSharedData::new(70, 1, &system_program::id());
    bank.store_account_and_update_capitalization(&payee.pubkey(), &payee_account);

    let bootstrap_validator_initial_balance = bank.get_balance(&bootstrap_validator_pubkey);

    let tx = system_transaction::transfer(&payer, &payee.pubkey(), 180, genesis_config.hash());

    let result = bank.process_transaction(&tx);
    assert_eq!(result, Ok(()));

    let mut total_rent_deducted = 0;

    // 400 - 128(Rent) - 180(Transfer)
    assert_eq!(bank.get_balance(&payer.pubkey()), 92);
    total_rent_deducted += 128;

    // 70 - 70(Rent) + 180(Transfer) - 21(Rent)
    assert_eq!(bank.get_balance(&payee.pubkey()), 159);
    total_rent_deducted += 70 + 21;

    let previous_capitalization = bank.capitalization.load(Relaxed);

    bank.freeze();

    assert_eq!(bank.collected_rent.load(Relaxed), total_rent_deducted);

    let burned_portion =
        total_rent_deducted * u64::from(bank.rent_collector.rent.burn_percent) / 100;
    let rent_to_be_distributed = total_rent_deducted - burned_portion;

    let bootstrap_validator_portion =
        ((bootstrap_validator_stake_lamports * rent_to_be_distributed) as f64 / 100.0) as u64 + 1; // Leftover lamport
    assert_eq!(
        bank.get_balance(&bootstrap_validator_pubkey),
        bootstrap_validator_portion + bootstrap_validator_initial_balance
    );

    // Since, validator 1 and validator 2 has equal smallest stake, it comes down to comparison
    // between their pubkey.
    let tweak_1 = u64::from(validator_1_pubkey > validator_2_pubkey);
    let validator_1_portion =
        ((validator_1_stake_lamports * rent_to_be_distributed) as f64 / 100.0) as u64 + tweak_1;
    assert_eq!(
        bank.get_balance(&validator_1_pubkey),
        validator_1_portion + 42 - tweak_1,
    );

    // Since, validator 1 and validator 2 has equal smallest stake, it comes down to comparison
    // between their pubkey.
    let tweak_2 = u64::from(validator_2_pubkey > validator_1_pubkey);
    let validator_2_portion =
        ((validator_2_stake_lamports * rent_to_be_distributed) as f64 / 100.0) as u64 + tweak_2;
    assert_eq!(
        bank.get_balance(&validator_2_pubkey),
        validator_2_portion + 42 - tweak_2,
    );

    let validator_3_portion =
        ((validator_3_stake_lamports * rent_to_be_distributed) as f64 / 100.0) as u64 + 1;
    assert_eq!(
        bank.get_balance(&validator_3_pubkey),
        validator_3_portion + 42
    );

    let current_capitalization = bank.capitalization.load(Relaxed);

    // only slot history is newly created
    let sysvar_and_builtin_program_delta =
        min_rent_exempt_balance_for_sysvars(&bank, &[sysvar::slot_history::id()]);
    assert_eq!(
        previous_capitalization - (current_capitalization - sysvar_and_builtin_program_delta),
        burned_portion
    );

    assert!(bank.calculate_and_verify_capitalization(true));

    assert_eq!(
        rent_to_be_distributed,
        bank.rewards
            .read()
            .unwrap()
            .iter()
            .map(|(address, reward)| {
                if reward.lamports > 0 {
                    assert_eq!(reward.reward_type, RewardType::Rent);
                    if *address == validator_2_pubkey {
                        assert_eq!(reward.post_balance, validator_2_portion + 42 - tweak_2);
                    } else if *address == validator_3_pubkey {
                        assert_eq!(reward.post_balance, validator_3_portion + 42);
                    }
                    reward.lamports as u64
                } else {
                    0
                }
            })
            .sum::<u64>()
    );
}

#[test]
fn test_rent_exempt_executable_account() {
    let (mut genesis_config, mint_keypair) = create_genesis_config(100_000);
    genesis_config.rent = rent_with_exemption_threshold(1000.0);

    let (root_bank, bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);
    let bank =
        create_child_bank_for_rent_test(root_bank, &genesis_config, bank_forks.as_ref(), None);

    let account_pubkey = solana_sdk::pubkey::new_rand();
    let account_balance = 1;
    let mut account = AccountSharedData::new(account_balance, 0, &solana_sdk::pubkey::new_rand());
    account.set_executable(true);
    account.set_owner(bpf_loader_upgradeable::id());
    bank.store_account(&account_pubkey, &account);

    let transfer_lamports = 1;
    let tx = system_transaction::transfer(
        &mint_keypair,
        &account_pubkey,
        transfer_lamports,
        genesis_config.hash(),
    );
    assert_matches!(
        bank.process_transaction(&tx),
        Err(TransactionError::InstructionError(0, _))
    );
    assert_eq!(bank.get_balance(&account_pubkey), account_balance);
}

#[test]
#[ignore]
#[allow(clippy::cognitive_complexity)]
fn test_rent_complex() {
    solana_logger::setup();
    let mock_program_id = Pubkey::from([2u8; 32]);

    #[derive(Serialize, Deserialize)]
    enum MockInstruction {
        Deduction,
    }

    declare_process_instruction!(MockBuiltin, 1, |invoke_context| {
        let transaction_context = &invoke_context.transaction_context;
        let instruction_context = transaction_context.get_current_instruction_context()?;
        let instruction_data = instruction_context.get_instruction_data();
        if let Ok(instruction) = bincode::deserialize(instruction_data) {
            match instruction {
                MockInstruction::Deduction => {
                    instruction_context
                        .try_borrow_instruction_account(transaction_context, 1)?
                        .checked_add_lamports(1)?;
                    instruction_context
                        .try_borrow_instruction_account(transaction_context, 2)?
                        .checked_sub_lamports(1)?;
                    Ok(())
                }
            }
        } else {
            Err(InstructionError::InvalidInstructionData)
        }
    });

    let (mut genesis_config, _mint_keypair) = create_genesis_config(10);
    let mut keypairs: Vec<Keypair> = Vec::with_capacity(14);
    for _i in 0..14 {
        keypairs.push(Keypair::new());
    }

    genesis_config.rent = rent_with_exemption_threshold(1000.0);

    let (root_bank, bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);
    // until we completely transition to the eager rent collection,
    // we must ensure lazy rent collection doesn't get broken!
    root_bank.restore_old_behavior_for_fragile_tests();
    let bank = create_child_bank_for_rent_test(
        root_bank,
        &genesis_config,
        bank_forks.as_ref(),
        Some((mock_program_id, MockBuiltin::vm)),
    );

    assert_eq!(bank.last_blockhash(), genesis_config.hash());

    let slots_elapsed: u64 = (0..=bank.epoch)
        .map(|epoch| {
            bank.rent_collector
                .epoch_schedule
                .get_slots_in_epoch(epoch + 1)
        })
        .sum();
    let generic_rent_due_for_system_account = bank
        .rent_collector
        .rent
        .due(
            bank.get_minimum_balance_for_rent_exemption(0) - 1,
            0,
            slots_elapsed as f64 / bank.rent_collector.slots_per_year,
        )
        .lamports();

    store_accounts_for_rent_test(
        &bank,
        &keypairs,
        mock_program_id,
        generic_rent_due_for_system_account,
    );

    let magic_rent_number = 131; // yuck, derive this value programmatically one day

    let t1 = system_transaction::transfer(
        &keypairs[0],
        &keypairs[1].pubkey(),
        1,
        genesis_config.hash(),
    );
    let t2 = system_transaction::transfer(
        &keypairs[2],
        &keypairs[3].pubkey(),
        1,
        genesis_config.hash(),
    );
    let t3 = system_transaction::transfer(
        &keypairs[4],
        &keypairs[5].pubkey(),
        1,
        genesis_config.hash(),
    );
    let t4 = system_transaction::transfer(
        &keypairs[6],
        &keypairs[7].pubkey(),
        generic_rent_due_for_system_account + 1,
        genesis_config.hash(),
    );
    let t5 = system_transaction::transfer(
        &keypairs[8],
        &keypairs[9].pubkey(),
        929,
        genesis_config.hash(),
    );

    let account_metas = vec![
        AccountMeta::new(keypairs[10].pubkey(), true),
        AccountMeta::new(keypairs[11].pubkey(), true),
        AccountMeta::new(keypairs[12].pubkey(), true),
        AccountMeta::new_readonly(keypairs[13].pubkey(), false),
    ];
    let deduct_instruction =
        Instruction::new_with_bincode(mock_program_id, &MockInstruction::Deduction, account_metas);
    let t6 = Transaction::new_signed_with_payer(
        &[deduct_instruction],
        Some(&keypairs[10].pubkey()),
        &[&keypairs[10], &keypairs[11], &keypairs[12]],
        genesis_config.hash(),
    );

    let txs = vec![t6, t5, t1, t2, t3, t4];
    let res = bank.process_transactions(txs.iter());

    assert_eq!(res.len(), 6);
    assert_eq!(res[0], Ok(()));
    assert_eq!(res[1], Ok(()));
    assert_eq!(res[2], Ok(()));
    assert_eq!(res[3], Ok(()));
    assert_eq!(res[4], Err(TransactionError::AccountNotFound));
    assert_eq!(res[5], Ok(()));

    bank.freeze();

    let mut rent_collected = 0;

    // 48992 - generic_rent_due_for_system_account(Rent) - 1(transfer)
    assert_eq!(bank.get_balance(&keypairs[0].pubkey()), 1);
    rent_collected += generic_rent_due_for_system_account;

    // 48992 - generic_rent_due_for_system_account(Rent) + 1(transfer)
    assert_eq!(bank.get_balance(&keypairs[1].pubkey()), 3);
    rent_collected += generic_rent_due_for_system_account;

    // 48992 - generic_rent_due_for_system_account(Rent) - 1(transfer)
    assert_eq!(bank.get_balance(&keypairs[2].pubkey()), 1);
    rent_collected += generic_rent_due_for_system_account;

    // 48992 - generic_rent_due_for_system_account(Rent) + 1(transfer)
    assert_eq!(bank.get_balance(&keypairs[3].pubkey()), 3);
    rent_collected += generic_rent_due_for_system_account;

    // No rent deducted
    assert_eq!(bank.get_balance(&keypairs[4].pubkey()), 10);
    assert_eq!(bank.get_balance(&keypairs[5].pubkey()), 10);

    // 98004 - generic_rent_due_for_system_account(Rent) - 48991(transfer)
    assert_eq!(bank.get_balance(&keypairs[6].pubkey()), 23);
    rent_collected += generic_rent_due_for_system_account;

    // 0 + 48990(transfer) - magic_rent_number(Rent)
    assert_eq!(
        bank.get_balance(&keypairs[7].pubkey()),
        generic_rent_due_for_system_account + 1 - magic_rent_number
    );

    // Epoch should be updated
    // Rent deducted on store side
    let account8 = bank.get_account(&keypairs[7].pubkey()).unwrap();
    // Epoch should be set correctly.
    assert_eq!(account8.rent_epoch(), bank.epoch + 1);
    rent_collected += magic_rent_number;

    // 49921 - generic_rent_due_for_system_account(Rent) - 929(Transfer)
    assert_eq!(bank.get_balance(&keypairs[8].pubkey()), 2);
    rent_collected += generic_rent_due_for_system_account;

    let account10 = bank.get_account(&keypairs[9].pubkey()).unwrap();
    // Account was overwritten at load time, since it didn't have sufficient balance to pay rent
    // Then, at store time we deducted `magic_rent_number` rent for the current epoch, once it has balance
    assert_eq!(account10.rent_epoch(), bank.epoch + 1);
    // account data is blank now
    assert_eq!(account10.data().len(), 0);
    // 10 - 10(Rent) + 929(Transfer) - magic_rent_number(Rent)
    assert_eq!(account10.lamports(), 929 - magic_rent_number);
    rent_collected += magic_rent_number + 10;

    // 48993 - generic_rent_due_for_system_account(Rent)
    assert_eq!(bank.get_balance(&keypairs[10].pubkey()), 3);
    rent_collected += generic_rent_due_for_system_account;

    // 48993 - generic_rent_due_for_system_account(Rent) + 1(Addition by program)
    assert_eq!(bank.get_balance(&keypairs[11].pubkey()), 4);
    rent_collected += generic_rent_due_for_system_account;

    // 48993 - generic_rent_due_for_system_account(Rent) - 1(Deduction by program)
    assert_eq!(bank.get_balance(&keypairs[12].pubkey()), 2);
    rent_collected += generic_rent_due_for_system_account;

    // No rent for read-only account
    assert_eq!(bank.get_balance(&keypairs[13].pubkey()), 14);

    // Bank's collected rent should be sum of rent collected from all accounts
    assert_eq!(bank.collected_rent.load(Relaxed), rent_collected);
}

#[test]
fn test_rent_eager_across_epoch_without_gap() {
    let (mut bank, _bank_forks) = create_simple_test_arc_bank(1);
    assert_eq!(bank.rent_collection_partitions(), vec![(0, 0, 32)]);

    bank = Arc::new(new_from_parent(bank));
    assert_eq!(bank.rent_collection_partitions(), vec![(0, 1, 32)]);
    for _ in 2..32 {
        bank = Arc::new(new_from_parent(bank));
    }
    assert_eq!(bank.rent_collection_partitions(), vec![(30, 31, 32)]);
    bank = Arc::new(new_from_parent(bank));
    assert_eq!(bank.rent_collection_partitions(), vec![(0, 0, 64)]);
}

#[test]
fn test_rent_eager_across_epoch_without_gap_mnb() {
    solana_logger::setup();
    let (mut genesis_config, _mint_keypair) = create_genesis_config(1);
    genesis_config.cluster_type = ClusterType::MainnetBeta;

    let mut bank = Arc::new(Bank::new_for_tests(&genesis_config));
    assert_eq!(bank.rent_collection_partitions(), vec![(0, 0, 32)]);

    bank = Arc::new(new_from_parent(bank));
    assert_eq!(bank.rent_collection_partitions(), vec![(0, 1, 32)]);
    for _ in 2..32 {
        bank = Arc::new(new_from_parent(bank));
    }
    assert_eq!(bank.rent_collection_partitions(), vec![(30, 31, 32)]);
    bank = Arc::new(new_from_parent(bank));
    assert_eq!(bank.rent_collection_partitions(), vec![(0, 0, 64)]);
}

#[test]
fn test_rent_eager_across_epoch_with_full_gap() {
    let (mut genesis_config, _mint_keypair) = create_genesis_config(1);
    activate_all_features(&mut genesis_config);

    let mut bank = Arc::new(Bank::new_for_tests(&genesis_config));
    assert_eq!(bank.rent_collection_partitions(), vec![(0, 0, 32)]);

    bank = Arc::new(new_from_parent(bank));
    assert_eq!(bank.rent_collection_partitions(), vec![(0, 1, 32)]);
    for _ in 2..15 {
        bank = Arc::new(new_from_parent(bank));
    }
    assert_eq!(bank.rent_collection_partitions(), vec![(13, 14, 32)]);
    bank = Arc::new(Bank::new_from_parent(bank, &Pubkey::default(), 49));
    assert_eq!(
        bank.rent_collection_partitions(),
        vec![(14, 31, 32), (0, 0, 64), (0, 17, 64)]
    );
    bank = Arc::new(new_from_parent(bank));
    assert_eq!(bank.rent_collection_partitions(), vec![(17, 18, 64)]);
}

#[test]
fn test_rent_eager_across_epoch_with_half_gap() {
    let (mut genesis_config, _mint_keypair) = create_genesis_config(1);
    activate_all_features(&mut genesis_config);

    let mut bank = Arc::new(Bank::new_for_tests(&genesis_config));
    assert_eq!(bank.rent_collection_partitions(), vec![(0, 0, 32)]);

    bank = Arc::new(new_from_parent(bank));
    assert_eq!(bank.rent_collection_partitions(), vec![(0, 1, 32)]);
    for _ in 2..15 {
        bank = Arc::new(new_from_parent(bank));
    }
    assert_eq!(bank.rent_collection_partitions(), vec![(13, 14, 32)]);
    bank = Arc::new(Bank::new_from_parent(bank, &Pubkey::default(), 32));
    assert_eq!(
        bank.rent_collection_partitions(),
        vec![(14, 31, 32), (0, 0, 64)]
    );
    bank = Arc::new(new_from_parent(bank));
    assert_eq!(bank.rent_collection_partitions(), vec![(0, 1, 64)]);
}

#[test]
#[allow(clippy::cognitive_complexity)]
fn test_rent_eager_across_epoch_without_gap_under_multi_epoch_cycle() {
    let leader_pubkey = solana_sdk::pubkey::new_rand();
    let leader_lamports = 3;
    let mut genesis_config =
        create_genesis_config_with_leader(5, &leader_pubkey, leader_lamports).genesis_config;
    genesis_config.cluster_type = ClusterType::MainnetBeta;

    const SLOTS_PER_EPOCH: u64 = MINIMUM_SLOTS_PER_EPOCH;
    const LEADER_SCHEDULE_SLOT_OFFSET: u64 = SLOTS_PER_EPOCH * 3 - 3;
    genesis_config.epoch_schedule =
        EpochSchedule::custom(SLOTS_PER_EPOCH, LEADER_SCHEDULE_SLOT_OFFSET, false);

    let mut bank = Arc::new(Bank::new_for_tests(&genesis_config));
    assert_eq!(DEFAULT_SLOTS_PER_EPOCH, 432_000);
    assert_eq!(bank.get_slots_in_epoch(bank.epoch()), 32);
    assert_eq!(bank.get_epoch_and_slot_index(bank.slot()), (0, 0));
    assert_eq!(bank.rent_collection_partitions(), vec![(0, 0, 432_000)]);

    bank = Arc::new(new_from_parent(bank));
    assert_eq!(bank.get_slots_in_epoch(bank.epoch()), 32);
    assert_eq!(bank.get_epoch_and_slot_index(bank.slot()), (0, 1));
    assert_eq!(bank.rent_collection_partitions(), vec![(0, 1, 432_000)]);

    for _ in 2..32 {
        bank = Arc::new(new_from_parent(bank));
    }
    assert_eq!(bank.get_slots_in_epoch(bank.epoch()), 32);
    assert_eq!(bank.get_epoch_and_slot_index(bank.slot()), (0, 31));
    assert_eq!(bank.rent_collection_partitions(), vec![(30, 31, 432_000)]);

    bank = Arc::new(new_from_parent(bank));
    assert_eq!(bank.get_slots_in_epoch(bank.epoch()), 32);
    assert_eq!(bank.get_epoch_and_slot_index(bank.slot()), (1, 0));
    assert_eq!(bank.rent_collection_partitions(), vec![(31, 32, 432_000)]);

    bank = Arc::new(new_from_parent(bank));
    assert_eq!(bank.get_slots_in_epoch(bank.epoch()), 32);
    assert_eq!(bank.get_epoch_and_slot_index(bank.slot()), (1, 1));
    assert_eq!(bank.rent_collection_partitions(), vec![(32, 33, 432_000)]);

    bank = Arc::new(Bank::new_from_parent(bank, &Pubkey::default(), 1000));
    bank = Arc::new(Bank::new_from_parent(bank, &Pubkey::default(), 1001));
    assert_eq!(bank.get_slots_in_epoch(bank.epoch()), 32);
    assert_eq!(bank.get_epoch_and_slot_index(bank.slot()), (31, 9));
    assert_eq!(
        bank.rent_collection_partitions(),
        vec![(1000, 1001, 432_000)]
    );

    bank = Arc::new(Bank::new_from_parent(bank, &Pubkey::default(), 431_998));
    bank = Arc::new(Bank::new_from_parent(bank, &Pubkey::default(), 431_999));
    assert_eq!(bank.get_slots_in_epoch(bank.epoch()), 32);
    assert_eq!(bank.get_epoch_and_slot_index(bank.slot()), (13499, 31));
    assert_eq!(
        bank.rent_collection_partitions(),
        vec![(431_998, 431_999, 432_000)]
    );

    bank = Arc::new(new_from_parent(bank));
    assert_eq!(bank.get_slots_in_epoch(bank.epoch()), 32);
    assert_eq!(bank.get_epoch_and_slot_index(bank.slot()), (13500, 0));
    assert_eq!(bank.rent_collection_partitions(), vec![(0, 0, 432_000)]);

    bank = Arc::new(new_from_parent(bank));
    assert_eq!(bank.get_slots_in_epoch(bank.epoch()), 32);
    assert_eq!(bank.get_epoch_and_slot_index(bank.slot()), (13500, 1));
    assert_eq!(bank.rent_collection_partitions(), vec![(0, 1, 432_000)]);
}

#[test]
fn test_rent_eager_across_epoch_with_gap_under_multi_epoch_cycle() {
    let leader_pubkey = solana_sdk::pubkey::new_rand();
    let leader_lamports = 3;
    let mut genesis_config =
        create_genesis_config_with_leader(5, &leader_pubkey, leader_lamports).genesis_config;
    genesis_config.cluster_type = ClusterType::MainnetBeta;

    const SLOTS_PER_EPOCH: u64 = MINIMUM_SLOTS_PER_EPOCH;
    const LEADER_SCHEDULE_SLOT_OFFSET: u64 = SLOTS_PER_EPOCH * 3 - 3;
    genesis_config.epoch_schedule =
        EpochSchedule::custom(SLOTS_PER_EPOCH, LEADER_SCHEDULE_SLOT_OFFSET, false);

    let mut bank = Arc::new(Bank::new_for_tests(&genesis_config));
    assert_eq!(DEFAULT_SLOTS_PER_EPOCH, 432_000);
    assert_eq!(bank.get_slots_in_epoch(bank.epoch()), 32);
    assert_eq!(bank.get_epoch_and_slot_index(bank.slot()), (0, 0));
    assert_eq!(bank.rent_collection_partitions(), vec![(0, 0, 432_000)]);

    bank = Arc::new(new_from_parent(bank));
    assert_eq!(bank.get_slots_in_epoch(bank.epoch()), 32);
    assert_eq!(bank.get_epoch_and_slot_index(bank.slot()), (0, 1));
    assert_eq!(bank.rent_collection_partitions(), vec![(0, 1, 432_000)]);

    for _ in 2..19 {
        bank = Arc::new(new_from_parent(bank));
    }
    assert_eq!(bank.get_slots_in_epoch(bank.epoch()), 32);
    assert_eq!(bank.get_epoch_and_slot_index(bank.slot()), (0, 18));
    assert_eq!(bank.rent_collection_partitions(), vec![(17, 18, 432_000)]);

    bank = Arc::new(Bank::new_from_parent(bank, &Pubkey::default(), 44));
    assert_eq!(bank.get_slots_in_epoch(bank.epoch()), 32);
    assert_eq!(bank.get_epoch_and_slot_index(bank.slot()), (1, 12));
    assert_eq!(
        bank.rent_collection_partitions(),
        vec![(18, 31, 432_000), (31, 31, 432_000), (31, 44, 432_000)]
    );

    bank = Arc::new(new_from_parent(bank));
    assert_eq!(bank.get_slots_in_epoch(bank.epoch()), 32);
    assert_eq!(bank.get_epoch_and_slot_index(bank.slot()), (1, 13));
    assert_eq!(bank.rent_collection_partitions(), vec![(44, 45, 432_000)]);

    bank = Arc::new(Bank::new_from_parent(bank, &Pubkey::default(), 431_993));
    bank = Arc::new(Bank::new_from_parent(bank, &Pubkey::default(), 432_011));
    assert_eq!(bank.get_slots_in_epoch(bank.epoch()), 32);
    assert_eq!(bank.get_epoch_and_slot_index(bank.slot()), (13500, 11));
    assert_eq!(
        bank.rent_collection_partitions(),
        vec![
            (431_993, 431_999, 432_000),
            (0, 0, 432_000),
            (0, 11, 432_000)
        ]
    );
}

#[test]
fn test_rent_eager_with_warmup_epochs_under_multi_epoch_cycle() {
    let leader_pubkey = solana_sdk::pubkey::new_rand();
    let leader_lamports = 3;
    let mut genesis_config =
        create_genesis_config_with_leader(5, &leader_pubkey, leader_lamports).genesis_config;
    genesis_config.cluster_type = ClusterType::MainnetBeta;

    const SLOTS_PER_EPOCH: u64 = MINIMUM_SLOTS_PER_EPOCH * 8;
    const LEADER_SCHEDULE_SLOT_OFFSET: u64 = SLOTS_PER_EPOCH * 3 - 3;
    genesis_config.epoch_schedule =
        EpochSchedule::custom(SLOTS_PER_EPOCH, LEADER_SCHEDULE_SLOT_OFFSET, true);

    let mut bank = Arc::new(Bank::new_for_tests(&genesis_config));
    assert_eq!(DEFAULT_SLOTS_PER_EPOCH, 432_000);
    assert_eq!(bank.get_slots_in_epoch(bank.epoch()), 32);
    assert_eq!(bank.first_normal_epoch(), 3);
    assert_eq!(bank.get_epoch_and_slot_index(bank.slot()), (0, 0));
    assert_eq!(bank.rent_collection_partitions(), vec![(0, 0, 32)]);

    bank = Arc::new(Bank::new_from_parent(bank, &Pubkey::default(), 222));
    bank = Arc::new(new_from_parent(bank));
    assert_eq!(bank.get_slots_in_epoch(bank.epoch()), 128);
    assert_eq!(bank.get_epoch_and_slot_index(bank.slot()), (2, 127));
    assert_eq!(bank.rent_collection_partitions(), vec![(126, 127, 128)]);

    bank = Arc::new(new_from_parent(bank));
    assert_eq!(bank.get_slots_in_epoch(bank.epoch()), 256);
    assert_eq!(bank.get_epoch_and_slot_index(bank.slot()), (3, 0));
    assert_eq!(bank.rent_collection_partitions(), vec![(0, 0, 431_872)]);
    assert_eq!(431_872 % bank.get_slots_in_epoch(bank.epoch()), 0);

    bank = Arc::new(new_from_parent(bank));
    assert_eq!(bank.get_slots_in_epoch(bank.epoch()), 256);
    assert_eq!(bank.get_epoch_and_slot_index(bank.slot()), (3, 1));
    assert_eq!(bank.rent_collection_partitions(), vec![(0, 1, 431_872)]);

    bank = Arc::new(Bank::new_from_parent(
        bank,
        &Pubkey::default(),
        431_872 + 223 - 1,
    ));
    bank = Arc::new(new_from_parent(bank));
    assert_eq!(bank.get_slots_in_epoch(bank.epoch()), 256);
    assert_eq!(bank.get_epoch_and_slot_index(bank.slot()), (1689, 255));
    assert_eq!(
        bank.rent_collection_partitions(),
        vec![(431_870, 431_871, 431_872)]
    );

    bank = Arc::new(new_from_parent(bank));
    assert_eq!(bank.get_slots_in_epoch(bank.epoch()), 256);
    assert_eq!(bank.get_epoch_and_slot_index(bank.slot()), (1690, 0));
    assert_eq!(bank.rent_collection_partitions(), vec![(0, 0, 431_872)]);
}

#[test]
fn test_rent_eager_under_fixed_cycle_for_development() {
    solana_logger::setup();
    let leader_pubkey = solana_sdk::pubkey::new_rand();
    let leader_lamports = 3;
    let mut genesis_config =
        create_genesis_config_with_leader(5, &leader_pubkey, leader_lamports).genesis_config;

    const SLOTS_PER_EPOCH: u64 = MINIMUM_SLOTS_PER_EPOCH * 8;
    const LEADER_SCHEDULE_SLOT_OFFSET: u64 = SLOTS_PER_EPOCH * 3 - 3;
    genesis_config.epoch_schedule =
        EpochSchedule::custom(SLOTS_PER_EPOCH, LEADER_SCHEDULE_SLOT_OFFSET, true);

    let mut bank = Arc::new(Bank::new_for_tests(&genesis_config));
    assert_eq!(bank.get_slots_in_epoch(bank.epoch()), 32);
    assert_eq!(bank.first_normal_epoch(), 3);
    assert_eq!(bank.get_epoch_and_slot_index(bank.slot()), (0, 0));
    assert_eq!(bank.rent_collection_partitions(), vec![(0, 0, 432_000)]);

    bank = Arc::new(Bank::new_from_parent(bank, &Pubkey::default(), 222));
    bank = Arc::new(new_from_parent(bank));
    assert_eq!(bank.get_slots_in_epoch(bank.epoch()), 128);
    assert_eq!(bank.get_epoch_and_slot_index(bank.slot()), (2, 127));
    assert_eq!(bank.rent_collection_partitions(), vec![(222, 223, 432_000)]);

    bank = Arc::new(new_from_parent(bank));
    assert_eq!(bank.get_slots_in_epoch(bank.epoch()), 256);
    assert_eq!(bank.get_epoch_and_slot_index(bank.slot()), (3, 0));
    assert_eq!(bank.rent_collection_partitions(), vec![(223, 224, 432_000)]);

    bank = Arc::new(new_from_parent(bank));
    assert_eq!(bank.get_slots_in_epoch(bank.epoch()), 256);
    assert_eq!(bank.get_epoch_and_slot_index(bank.slot()), (3, 1));
    assert_eq!(bank.rent_collection_partitions(), vec![(224, 225, 432_000)]);

    bank = Arc::new(Bank::new_from_parent(bank, &Pubkey::default(), 432_000 - 2));
    bank = Arc::new(new_from_parent(bank));
    assert_eq!(
        bank.rent_collection_partitions(),
        vec![(431_998, 431_999, 432_000)]
    );
    bank = Arc::new(new_from_parent(bank));
    assert_eq!(bank.rent_collection_partitions(), vec![(0, 0, 432_000)]);
    bank = Arc::new(new_from_parent(bank));
    assert_eq!(bank.rent_collection_partitions(), vec![(0, 1, 432_000)]);

    bank = Arc::new(Bank::new_from_parent(
        bank,
        &Pubkey::default(),
        864_000 - 20,
    ));
    bank = Arc::new(Bank::new_from_parent(
        bank,
        &Pubkey::default(),
        864_000 + 39,
    ));
    assert_eq!(
        bank.rent_collection_partitions(),
        vec![
            (431_980, 431_999, 432_000),
            (0, 0, 432_000),
            (0, 39, 432_000)
        ]
    );
}

impl Bank {
    fn slots_by_pubkey(&self, pubkey: &Pubkey) -> Vec<Slot> {
        self.rc
            .accounts
            .accounts_db
            .accounts_index
            .get_and_then(pubkey, |entry| {
                let slots = entry
                    .map(|entry| {
                        entry
                            .slot_list
                            .read()
                            .unwrap()
                            .iter()
                            .map(|(slot, _)| *slot)
                            .collect()
                    })
                    .unwrap_or_default();
                (false, slots)
            })
    }
}

#[test_case(true; "enable rent fees collection")]
#[test_case(false; "disable rent fees collection")]
fn test_rent_eager_collect_rent_in_partition(should_collect_rent: bool) {
    solana_logger::setup();
    let (mut genesis_config, _mint_keypair) = create_genesis_config(1_000_000);
    for feature_id in FeatureSet::default().inactive {
        if feature_id != solana_feature_set::skip_rent_rewrites::id()
            && (!should_collect_rent
                || feature_id != solana_feature_set::disable_rent_fees_collection::id())
        {
            activate_feature(&mut genesis_config, feature_id);
        }
    }

    let zero_lamport_pubkey = solana_sdk::pubkey::new_rand();
    let rent_due_pubkey = solana_sdk::pubkey::new_rand();
    let rent_exempt_pubkey = solana_sdk::pubkey::new_rand();
    let mut bank = Arc::new(Bank::new_for_tests(&genesis_config));

    assert_eq!(should_collect_rent, bank.should_collect_rent());

    let zero_lamports = 0;
    let little_lamports = 1234;
    let large_lamports = 123_456_789;
    // genesis_config.epoch_schedule.slots_per_epoch == 432_000 and is unsuitable for this test
    let some_slot = MINIMUM_SLOTS_PER_EPOCH; // chosen to cause epoch to be +1
    let rent_collected = if bank.should_collect_rent() {
        1 /* this is a function of 'some_slot' */
    } else {
        0
    };

    bank.store_account(
        &zero_lamport_pubkey,
        &AccountSharedData::new(zero_lamports, 0, &Pubkey::default()),
    );
    bank.store_account(
        &rent_due_pubkey,
        &AccountSharedData::new(little_lamports, 0, &Pubkey::default()),
    );
    bank.store_account(
        &rent_exempt_pubkey,
        &AccountSharedData::new(large_lamports, 0, &Pubkey::default()),
    );

    let genesis_slot = 0;

    let previous_epoch = bank.epoch();
    bank = Arc::new(Bank::new_from_parent(bank, &Pubkey::default(), some_slot));
    let current_epoch = bank.epoch();
    assert_eq!(previous_epoch + 1, current_epoch);

    assert_eq!(bank.collected_rent.load(Relaxed), 0);
    assert_eq!(
        bank.get_account(&rent_due_pubkey).unwrap().lamports(),
        little_lamports
    );
    assert_eq!(bank.get_account(&rent_due_pubkey).unwrap().rent_epoch(), 0);
    assert_eq!(bank.slots_by_pubkey(&rent_due_pubkey), vec![genesis_slot]);
    assert_eq!(
        bank.slots_by_pubkey(&rent_exempt_pubkey),
        vec![genesis_slot]
    );
    assert_eq!(
        bank.slots_by_pubkey(&zero_lamport_pubkey),
        vec![genesis_slot]
    );

    assert_eq!(bank.collected_rent.load(Relaxed), 0);
    bank.collect_rent_in_partition((0, 0, 1), &RentMetrics::default()); // all range

    assert_eq!(bank.collected_rent.load(Relaxed), rent_collected);
    assert_eq!(
        bank.get_account(&rent_due_pubkey).unwrap().lamports(),
        little_lamports - rent_collected
    );
    assert!(
        bank.get_account(&rent_due_pubkey).unwrap().rent_epoch() == current_epoch + 1
            || !bank.should_collect_rent()
    );
    assert_eq!(
        bank.get_account(&rent_exempt_pubkey).unwrap().lamports(),
        large_lamports
    );
    assert_eq!(
        bank.get_account(&rent_exempt_pubkey).unwrap().rent_epoch(),
        RENT_EXEMPT_RENT_EPOCH
    );
    assert_eq!(
        bank.slots_by_pubkey(&rent_due_pubkey),
        vec![genesis_slot, some_slot]
    );
    assert_eq!(
        bank.slots_by_pubkey(&rent_exempt_pubkey),
        vec![genesis_slot, some_slot]
    );
    assert_eq!(
        bank.slots_by_pubkey(&zero_lamport_pubkey),
        vec![genesis_slot]
    );
}

fn new_from_parent_next_epoch(
    parent: Arc<Bank>,
    bank_forks: &RwLock<BankForks>,
    epochs: Epoch,
) -> Arc<Bank> {
    let mut slot = parent.slot();
    let mut epoch = parent.epoch();
    for _ in 0..epochs {
        slot += parent.epoch_schedule().get_slots_in_epoch(epoch);
        epoch = parent.epoch_schedule().get_epoch(slot);
    }

    new_bank_from_parent_with_bank_forks(bank_forks, parent, &Pubkey::default(), slot)
}

#[test]
/// tests that an account which has already had rent collected IN this slot does not skip rewrites
fn test_collect_rent_from_accounts() {
    solana_logger::setup();

    for skip_rewrites in [false, true] {
        let address1 = Pubkey::new_unique();
        let address2 = Pubkey::new_unique();
        let address3 = Pubkey::new_unique();

        let (genesis_bank, bank_forks) = create_simple_test_arc_bank(100000);
        let mut first_bank = new_from_parent(genesis_bank.clone());
        if skip_rewrites {
            first_bank.activate_feature(&feature_set::skip_rent_rewrites::id());
        }
        let first_bank = bank_forks
            .write()
            .unwrap()
            .insert(first_bank)
            .clone_without_scheduler();

        let first_slot = 1;
        assert_eq!(first_slot, first_bank.slot());
        let epoch_delta = 4;
        let later_bank = new_from_parent_next_epoch(first_bank, bank_forks.as_ref(), epoch_delta); // a bank a few epochs in the future
        let later_slot = later_bank.slot();
        assert!(later_bank.epoch() == genesis_bank.epoch() + epoch_delta);

        let data_size = 0; // make sure we're rent exempt
        let lamports = later_bank.get_minimum_balance_for_rent_exemption(data_size); // cannot be 0 or we zero out rent_epoch in rent collection and we need to be rent exempt
        let mut account1 = AccountSharedData::new(lamports, data_size, &Pubkey::default());
        let mut account2 = AccountSharedData::new(lamports, data_size, &Pubkey::default());
        let mut account3 = AccountSharedData::new(lamports, data_size, &Pubkey::default());
        account1.set_rent_epoch(later_bank.epoch() - 1); // non-zero, but less than later_bank's epoch
        account2.set_rent_epoch(RENT_EXEMPT_RENT_EPOCH); // already marked as rent exempt
        account3.set_rent_epoch(0); // stake accounts in genesis have a rent epoch of 0

        // loaded from previous slot, so we skip rent collection on it
        let _result = later_bank.collect_rent_from_accounts(
            vec![
                (address1, account1, later_slot - 1),
                (address2, account2, later_slot - 1),
                (address3, account3, later_slot - 1),
            ],
            None,
            PartitionIndex::default(),
        );

        let deltas = later_bank
            .rc
            .accounts
            .accounts_db
            .get_pubkey_hash_for_slot(later_slot)
            .0;

        // ensure account1 *is* stored because the account *did* change
        // (its rent epoch must be updated to RENT_EXEMPT_RENT_EPOCH)
        assert!(deltas.iter().map(|(pubkey, _)| pubkey).contains(&address1));

        // if doing rewrites, ensure account2 *is* stored
        // if skipping rewrites, ensure account2 is *not* stored
        // (because the account did *not* change)
        assert_eq!(
            deltas.iter().map(|(pubkey, _)| pubkey).contains(&address2),
            !skip_rewrites,
        );

        // ensure account3 *is* stored because the account *did* change
        // (same as account1 above)
        assert!(deltas.iter().map(|(pubkey, _)| pubkey).contains(&address3));
    }
}

#[test]
fn test_rent_eager_collect_rent_zero_lamport_deterministic() {
    solana_logger::setup();

    let (genesis_config, _mint_keypair) = create_genesis_config(1);

    let zero_lamport_pubkey = solana_sdk::pubkey::new_rand();

    let genesis_bank1 = Arc::new(Bank::new_for_tests(&genesis_config));
    let genesis_bank2 = Arc::new(Bank::new_for_tests(&genesis_config));
    let bank1_with_zero = Arc::new(new_from_parent(genesis_bank1.clone()));
    let bank1_without_zero = Arc::new(new_from_parent(genesis_bank2));

    let zero_lamports = 0;
    let data_size = 12345; // use non-zero data size to also test accounts_data_size
    let account = AccountSharedData::new(zero_lamports, data_size, &Pubkey::default());
    bank1_with_zero.store_account(&zero_lamport_pubkey, &account);
    bank1_without_zero.store_account(&zero_lamport_pubkey, &account);

    bank1_without_zero
        .rc
        .accounts
        .accounts_db
        .accounts_index
        .add_root(genesis_bank1.slot() + 1);
    bank1_without_zero
        .rc
        .accounts
        .accounts_db
        .accounts_index
        .purge_roots(&zero_lamport_pubkey);

    // genesis_config.epoch_schedule.slots_per_epoch == 432_000 and is unsuitable for this test
    let some_slot = MINIMUM_SLOTS_PER_EPOCH; // 1 epoch
    let bank2_with_zero = Arc::new(Bank::new_from_parent(
        bank1_with_zero.clone(),
        &Pubkey::default(),
        some_slot,
    ));
    assert_eq!(bank1_with_zero.epoch() + 1, bank2_with_zero.epoch());
    let bank2_without_zero = Arc::new(Bank::new_from_parent(
        bank1_without_zero.clone(),
        &Pubkey::default(),
        some_slot,
    ));
    let hash1_with_zero = bank1_with_zero.hash();
    let hash1_without_zero = bank1_without_zero.hash();
    assert_eq!(hash1_with_zero, hash1_without_zero);
    assert_ne!(hash1_with_zero, Hash::default());

    bank2_with_zero.collect_rent_in_partition((0, 0, 1), &RentMetrics::default()); // all
    bank2_without_zero.collect_rent_in_partition((0, 0, 1), &RentMetrics::default()); // all

    bank2_with_zero.freeze();
    let hash2_with_zero = bank2_with_zero.hash();
    bank2_without_zero.freeze();
    let hash2_without_zero = bank2_without_zero.hash();

    assert_eq!(hash2_with_zero, hash2_without_zero);
    assert_ne!(hash2_with_zero, Hash::default());
}

#[test]
fn test_bank_update_vote_stake_rewards() {
    let thread_pool = ThreadPoolBuilder::new().num_threads(1).build().unwrap();
    check_bank_update_vote_stake_rewards(|bank: &Bank| {
        bank._load_vote_and_stake_accounts(&thread_pool, null_tracer())
    });
}

#[cfg(test)]
fn check_bank_update_vote_stake_rewards<F>(load_vote_and_stake_accounts: F)
where
    F: Fn(&Bank) -> LoadVoteAndStakeAccountsResult,
{
    solana_logger::setup();

    // create a bank that ticks really slowly...
    let bank0 = Arc::new(Bank::new_for_tests(&GenesisConfig {
        accounts: (0..42)
            .map(|_| {
                (
                    solana_sdk::pubkey::new_rand(),
                    Account::new(1_000_000_000, 0, &Pubkey::default()),
                )
            })
            .collect(),
        // set it up so the first epoch is a full year long
        poh_config: PohConfig {
            target_tick_duration: Duration::from_secs(
                SECONDS_PER_YEAR as u64 / MINIMUM_SLOTS_PER_EPOCH / DEFAULT_TICKS_PER_SLOT,
            ),
            hashes_per_tick: None,
            target_tick_count: None,
        },
        cluster_type: ClusterType::MainnetBeta,

        ..GenesisConfig::default()
    }));

    // enable lazy rent collection because this test depends on rent-due accounts
    // not being eagerly-collected for exact rewards calculation
    bank0.restore_old_behavior_for_fragile_tests();

    assert_eq!(
        bank0.capitalization(),
        42 * 1_000_000_000 + genesis_sysvar_and_builtin_program_lamports(),
    );

    let ((vote_id, mut vote_account), (stake_id, stake_account)) =
        crate::stakes::tests::create_staked_node_accounts(10_000);
    let starting_vote_and_stake_balance = 10_000 + 1;

    // set up accounts
    bank0.store_account_and_update_capitalization(&stake_id, &stake_account);

    // generate some rewards
    let mut vote_state = Some(vote_state::from(&vote_account).unwrap());
    for i in 0..MAX_LOCKOUT_HISTORY + 42 {
        if let Some(v) = vote_state.as_mut() {
            vote_state::process_slot_vote_unchecked(v, i as u64)
        }
        let versioned = VoteStateVersions::Current(Box::new(vote_state.take().unwrap()));
        vote_state::to(&versioned, &mut vote_account).unwrap();
        bank0.store_account_and_update_capitalization(&vote_id, &vote_account);
        match versioned {
            VoteStateVersions::Current(v) => {
                vote_state = Some(*v);
            }
            _ => panic!("Has to be of type Current"),
        };
    }
    bank0.store_account_and_update_capitalization(&vote_id, &vote_account);
    bank0.freeze();

    assert_eq!(
        bank0.capitalization(),
        42 * 1_000_000_000
            + genesis_sysvar_and_builtin_program_lamports()
            + starting_vote_and_stake_balance
            + bank0_sysvar_delta(),
    );
    assert!(bank0.rewards.read().unwrap().is_empty());

    load_vote_and_stake_accounts(&bank0);

    // put a child bank in epoch 1, which calls update_rewards()...
    let bank1 = Bank::new_from_parent(
        bank0.clone(),
        &Pubkey::default(),
        bank0.get_slots_in_epoch(bank0.epoch()) + 1,
    );
    // verify that there's inflation
    assert_ne!(bank1.capitalization(), bank0.capitalization());

    // verify the inflation is represented in validator_points
    let paid_rewards = bank1.capitalization() - bank0.capitalization() - bank1_sysvar_delta();

    // this assumes that no new builtins or precompiles were activated in bank1
    let PrevEpochInflationRewards {
        validator_rewards, ..
    } = bank1.calculate_previous_epoch_inflation_rewards(bank0.capitalization(), bank0.epoch());

    // verify the stake and vote accounts are the right size
    assert!(
        ((bank1.get_balance(&stake_id) - stake_account.lamports() + bank1.get_balance(&vote_id)
            - vote_account.lamports()) as f64
            - validator_rewards as f64)
            .abs()
            < 1.0
    );

    // verify the rewards are the right size
    assert!((validator_rewards as f64 - paid_rewards as f64).abs() < 1.0); // rounding, truncating

    // verify validator rewards show up in bank1.rewards vector
    assert_eq!(
        *bank1.rewards.read().unwrap(),
        vec![
            (
                vote_id,
                RewardInfo {
                    reward_type: RewardType::Voting,
                    lamports: 0,
                    post_balance: bank1.get_balance(&vote_id),
                    commission: Some(0),
                }
            ),
            (
                stake_id,
                RewardInfo {
                    reward_type: RewardType::Staking,
                    lamports: validator_rewards as i64,
                    post_balance: bank1.get_balance(&stake_id),
                    commission: Some(0),
                }
            )
        ]
    );
    bank1.freeze();
    add_root_and_flush_write_cache(&bank0);
    add_root_and_flush_write_cache(&bank1);
    assert!(bank1.calculate_and_verify_capitalization(true));
}

fn do_test_bank_update_rewards_determinism() -> u64 {
    // create a bank that ticks really slowly...
    let bank = Arc::new(Bank::new_for_tests(&GenesisConfig {
        accounts: (0..42)
            .map(|_| {
                (
                    solana_sdk::pubkey::new_rand(),
                    Account::new(1_000_000_000, 0, &Pubkey::default()),
                )
            })
            .collect(),
        // set it up so the first epoch is a full year long
        poh_config: PohConfig {
            target_tick_duration: Duration::from_secs(
                SECONDS_PER_YEAR as u64 / MINIMUM_SLOTS_PER_EPOCH / DEFAULT_TICKS_PER_SLOT,
            ),
            hashes_per_tick: None,
            target_tick_count: None,
        },
        cluster_type: ClusterType::MainnetBeta,

        ..GenesisConfig::default()
    }));

    // enable lazy rent collection because this test depends on rent-due accounts
    // not being eagerly-collected for exact rewards calculation
    bank.restore_old_behavior_for_fragile_tests();

    assert_eq!(
        bank.capitalization(),
        42 * 1_000_000_000 + genesis_sysvar_and_builtin_program_lamports()
    );

    let vote_id = solana_sdk::pubkey::new_rand();
    let mut vote_account =
        vote_state::create_account(&vote_id, &solana_sdk::pubkey::new_rand(), 0, 100);
    let stake_id1 = solana_sdk::pubkey::new_rand();
    let stake_account1 = crate::stakes::tests::create_stake_account(123, &vote_id, &stake_id1);
    let stake_id2 = solana_sdk::pubkey::new_rand();
    let stake_account2 = crate::stakes::tests::create_stake_account(456, &vote_id, &stake_id2);

    // set up accounts
    bank.store_account_and_update_capitalization(&stake_id1, &stake_account1);
    bank.store_account_and_update_capitalization(&stake_id2, &stake_account2);

    // generate some rewards
    let mut vote_state = Some(vote_state::from(&vote_account).unwrap());
    for i in 0..MAX_LOCKOUT_HISTORY + 42 {
        if let Some(v) = vote_state.as_mut() {
            vote_state::process_slot_vote_unchecked(v, i as u64)
        }
        let versioned = VoteStateVersions::Current(Box::new(vote_state.take().unwrap()));
        vote_state::to(&versioned, &mut vote_account).unwrap();
        bank.store_account_and_update_capitalization(&vote_id, &vote_account);
        match versioned {
            VoteStateVersions::Current(v) => {
                vote_state = Some(*v);
            }
            _ => panic!("Has to be of type Current"),
        };
    }
    bank.store_account_and_update_capitalization(&vote_id, &vote_account);

    // put a child bank in epoch 1, which calls update_rewards()...
    let bank1 = Bank::new_from_parent(
        bank.clone(),
        &Pubkey::default(),
        bank.get_slots_in_epoch(bank.epoch()) + 1,
    );
    // verify that there's inflation
    assert_ne!(bank1.capitalization(), bank.capitalization());

    bank1.freeze();
    add_root_and_flush_write_cache(&bank);
    add_root_and_flush_write_cache(&bank1);
    assert!(bank1.calculate_and_verify_capitalization(true));

    // verify voting and staking rewards are recorded
    let rewards = bank1.rewards.read().unwrap();
    rewards
        .iter()
        .find(|(_address, reward)| reward.reward_type == RewardType::Voting)
        .unwrap();
    rewards
        .iter()
        .find(|(_address, reward)| reward.reward_type == RewardType::Staking)
        .unwrap();

    bank1.capitalization()
}

#[test]
fn test_bank_update_rewards_determinism() {
    solana_logger::setup();

    // The same reward should be distributed given same credits
    let expected_capitalization = do_test_bank_update_rewards_determinism();
    // Repeat somewhat large number of iterations to expose possible different behavior
    // depending on the randomly-seeded HashMap ordering
    for _ in 0..30 {
        let actual_capitalization = do_test_bank_update_rewards_determinism();
        assert_eq!(actual_capitalization, expected_capitalization);
    }
}

impl VerifyAccountsHashConfig {
    fn default_for_test() -> Self {
        Self {
            test_hash_calculation: true,
            ignore_mismatch: false,
            require_rooted_bank: false,
            run_in_background: false,
            store_hash_raw_data_for_debug: false,
        }
    }
}

// Test that purging 0 lamports accounts works.
#[test]
fn test_purge_empty_accounts() {
    // When using the write cache, flushing is destructive/cannot be undone
    //  so we have to stop at various points and restart to actively test.
    for pass in 0..3 {
        solana_logger::setup();
        let (genesis_config, mint_keypair) = create_genesis_config_no_tx_fee(sol_to_lamports(1.));
        let amount = genesis_config.rent.minimum_balance(0);
        let (mut bank, bank_forks) =
            Bank::new_for_tests(&genesis_config).wrap_with_bank_forks_for_tests();

        for _ in 0..10 {
            let blockhash = bank.last_blockhash();
            let pubkey = solana_sdk::pubkey::new_rand();
            let tx = system_transaction::transfer(&mint_keypair, &pubkey, 0, blockhash);
            bank.process_transaction(&tx).unwrap();
            bank.freeze();
            bank.squash();
            bank = new_from_parent_with_fork_next_slot(bank, bank_forks.as_ref());
        }

        bank.freeze();
        bank.squash();
        bank.force_flush_accounts_cache();
        let hash = bank.update_accounts_hash_for_tests();
        bank.clean_accounts_for_tests();
        assert_eq!(bank.update_accounts_hash_for_tests(), hash);

        let bank0 = new_from_parent_with_fork_next_slot(bank.clone(), bank_forks.as_ref());
        let blockhash = bank.last_blockhash();
        let keypair = Keypair::new();
        let tx = system_transaction::transfer(&mint_keypair, &keypair.pubkey(), amount, blockhash);
        bank0.process_transaction(&tx).unwrap();

        let bank1 = new_from_parent_with_fork_next_slot(bank0.clone(), bank_forks.as_ref());
        let pubkey = solana_sdk::pubkey::new_rand();
        let blockhash = bank.last_blockhash();
        let tx = system_transaction::transfer(&keypair, &pubkey, amount, blockhash);
        bank1.process_transaction(&tx).unwrap();

        assert_eq!(
            bank0.get_account(&keypair.pubkey()).unwrap().lamports(),
            amount
        );
        assert_eq!(bank1.get_account(&keypair.pubkey()), None);

        info!("bank0 purge");
        let hash = bank0.update_accounts_hash_for_tests();
        bank0.clean_accounts_for_tests();
        assert_eq!(bank0.update_accounts_hash_for_tests(), hash);

        assert_eq!(
            bank0.get_account(&keypair.pubkey()).unwrap().lamports(),
            amount
        );
        assert_eq!(bank1.get_account(&keypair.pubkey()), None);

        info!("bank1 purge");
        bank1.clean_accounts_for_tests();

        assert_eq!(
            bank0.get_account(&keypair.pubkey()).unwrap().lamports(),
            amount
        );
        assert_eq!(bank1.get_account(&keypair.pubkey()), None);

        if pass == 0 {
            add_root_and_flush_write_cache(&bank0);
            assert!(bank0.verify_accounts_hash(
                None,
                VerifyAccountsHashConfig::default_for_test(),
                None,
            ));
            continue;
        }

        // Squash and then verify hash_internal value
        bank0.freeze();
        bank0.squash();
        add_root_and_flush_write_cache(&bank0);
        if pass == 1 {
            assert!(bank0.verify_accounts_hash(
                None,
                VerifyAccountsHashConfig::default_for_test(),
                None,
            ));
            continue;
        }

        bank1.freeze();
        bank1.squash();
        add_root_and_flush_write_cache(&bank1);
        bank1.update_accounts_hash_for_tests();
        assert!(bank1.verify_accounts_hash(
            None,
            VerifyAccountsHashConfig::default_for_test(),
            None,
        ));

        // keypair should have 0 tokens on both forks
        assert_eq!(bank0.get_account(&keypair.pubkey()), None);
        assert_eq!(bank1.get_account(&keypair.pubkey()), None);

        bank1.clean_accounts_for_tests();

        assert!(bank1.verify_accounts_hash(
            None,
            VerifyAccountsHashConfig::default_for_test(),
            None,
        ));
    }
}

#[test]
fn test_two_payments_to_one_party() {
    let (genesis_config, mint_keypair) = create_genesis_config(sol_to_lamports(1.));
    let pubkey = solana_sdk::pubkey::new_rand();
    let (bank, _bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);
    let amount = genesis_config.rent.minimum_balance(0);
    assert_eq!(bank.last_blockhash(), genesis_config.hash());

    bank.transfer(amount, &mint_keypair, &pubkey).unwrap();
    assert_eq!(bank.get_balance(&pubkey), amount);

    bank.transfer(amount * 2, &mint_keypair, &pubkey).unwrap();
    assert_eq!(bank.get_balance(&pubkey), amount * 3);
    assert_eq!(bank.transaction_count(), 2);
    assert_eq!(bank.non_vote_transaction_count_since_restart(), 2);
}

#[test]
fn test_one_source_two_tx_one_batch() {
    let (genesis_config, mint_keypair) = create_genesis_config_no_tx_fee(sol_to_lamports(1.));
    let key1 = solana_sdk::pubkey::new_rand();
    let key2 = solana_sdk::pubkey::new_rand();
    let (bank, _bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);
    let amount = genesis_config.rent.minimum_balance(0);
    assert_eq!(bank.last_blockhash(), genesis_config.hash());

    let t1 = system_transaction::transfer(&mint_keypair, &key1, amount, genesis_config.hash());
    let t2 = system_transaction::transfer(&mint_keypair, &key2, amount, genesis_config.hash());
    let txs = vec![t1.clone(), t2.clone()];
    let res = bank.process_transactions(txs.iter());

    assert_eq!(res.len(), 2);
    assert_eq!(res[0], Ok(()));
    assert_eq!(res[1], Err(TransactionError::AccountInUse));
    assert_eq!(
        bank.get_balance(&mint_keypair.pubkey()),
        sol_to_lamports(1.) - amount
    );
    assert_eq!(bank.get_balance(&key1), amount);
    assert_eq!(bank.get_balance(&key2), 0);
    assert_eq!(bank.get_signature_status(&t1.signatures[0]), Some(Ok(())));
    // TODO: Transactions that fail to pay a fee could be dropped silently.
    // Non-instruction errors don't get logged in the signature cache
    assert_eq!(bank.get_signature_status(&t2.signatures[0]), None);
}

#[test]
fn test_one_tx_two_out_atomic_fail() {
    let amount = sol_to_lamports(1.);
    let (genesis_config, mint_keypair) = create_genesis_config_no_tx_fee_no_rent(amount);
    let key1 = solana_sdk::pubkey::new_rand();
    let key2 = solana_sdk::pubkey::new_rand();
    let (bank, _bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);
    let instructions = system_instruction::transfer_many(
        &mint_keypair.pubkey(),
        &[(key1, amount), (key2, amount)],
    );
    let message = Message::new(&instructions, Some(&mint_keypair.pubkey()));
    let tx = Transaction::new(&[&mint_keypair], message, genesis_config.hash());
    assert_eq!(
        bank.process_transaction(&tx).unwrap_err(),
        TransactionError::InstructionError(1, SystemError::ResultWithNegativeLamports.into())
    );
    assert_eq!(bank.get_balance(&mint_keypair.pubkey()), amount);
    assert_eq!(bank.get_balance(&key1), 0);
    assert_eq!(bank.get_balance(&key2), 0);
}

#[test]
fn test_one_tx_two_out_atomic_pass() {
    let (genesis_config, mint_keypair) = create_genesis_config_no_tx_fee(sol_to_lamports(1.));
    let key1 = solana_sdk::pubkey::new_rand();
    let key2 = solana_sdk::pubkey::new_rand();
    let (bank, _bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);
    let amount = genesis_config.rent.minimum_balance(0);
    let instructions = system_instruction::transfer_many(
        &mint_keypair.pubkey(),
        &[(key1, amount), (key2, amount)],
    );
    let message = Message::new(&instructions, Some(&mint_keypair.pubkey()));
    let tx = Transaction::new(&[&mint_keypair], message, genesis_config.hash());
    bank.process_transaction(&tx).unwrap();
    assert_eq!(
        bank.get_balance(&mint_keypair.pubkey()),
        sol_to_lamports(1.) - (2 * amount)
    );
    assert_eq!(bank.get_balance(&key1), amount);
    assert_eq!(bank.get_balance(&key2), amount);
}

// This test demonstrates that fees are paid even when a program fails.
#[test]
fn test_detect_failed_duplicate_transactions() {
    let (mut genesis_config, mint_keypair) = create_genesis_config(10_000);
    genesis_config.fee_rate_governor = FeeRateGovernor::new(5_000, 0);
    let (bank, _bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);

    let dest = Keypair::new();

    // source with 0 program context
    let tx =
        system_transaction::transfer(&mint_keypair, &dest.pubkey(), 10_000, genesis_config.hash());
    let signature = tx.signatures[0];
    assert!(!bank.has_signature(&signature));

    assert_eq!(
        bank.process_transaction(&tx),
        Err(TransactionError::InstructionError(
            0,
            SystemError::ResultWithNegativeLamports.into(),
        ))
    );

    // The lamports didn't move, but the from address paid the transaction fee.
    assert_eq!(bank.get_balance(&dest.pubkey()), 0);

    // This should be the original balance minus the transaction fee.
    assert_eq!(bank.get_balance(&mint_keypair.pubkey()), 5000);
}

#[test]
fn test_account_not_found() {
    solana_logger::setup();
    let (genesis_config, mint_keypair) = create_genesis_config(0);
    let (bank, _bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);
    let keypair = Keypair::new();
    assert_eq!(
        bank.transfer(
            genesis_config.rent.minimum_balance(0),
            &keypair,
            &mint_keypair.pubkey()
        ),
        Err(TransactionError::AccountNotFound)
    );
    assert_eq!(bank.transaction_count(), 0);
    assert_eq!(bank.non_vote_transaction_count_since_restart(), 0);
}

#[test]
fn test_insufficient_funds() {
    let mint_amount = sol_to_lamports(1.);
    let (genesis_config, mint_keypair) = create_genesis_config_no_tx_fee(mint_amount);
    let (bank, _bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);
    let pubkey = solana_sdk::pubkey::new_rand();
    let amount = genesis_config.rent.minimum_balance(0);
    bank.transfer(amount, &mint_keypair, &pubkey).unwrap();
    assert_eq!(bank.transaction_count(), 1);
    assert_eq!(bank.non_vote_transaction_count_since_restart(), 1);
    assert_eq!(bank.get_balance(&pubkey), amount);
    assert_eq!(
        bank.transfer((mint_amount - amount) + 1, &mint_keypair, &pubkey),
        Err(TransactionError::InstructionError(
            0,
            SystemError::ResultWithNegativeLamports.into(),
        ))
    );
    // transaction_count returns the count of all committed transactions since
    // bank_transaction_count_fix was activated, regardless of success
    assert_eq!(bank.transaction_count(), 2);
    assert_eq!(bank.non_vote_transaction_count_since_restart(), 2);

    let mint_pubkey = mint_keypair.pubkey();
    assert_eq!(bank.get_balance(&mint_pubkey), mint_amount - amount);
    assert_eq!(bank.get_balance(&pubkey), amount);
}

#[test]
fn test_executed_transaction_count_post_bank_transaction_count_fix() {
    let mint_amount = sol_to_lamports(1.);
    let (genesis_config, mint_keypair) = create_genesis_config(mint_amount);
    let (bank, bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);
    let pubkey = solana_sdk::pubkey::new_rand();
    let amount = genesis_config.rent.minimum_balance(0);
    bank.transfer(amount, &mint_keypair, &pubkey).unwrap();
    assert_eq!(
        bank.transfer((mint_amount - amount) + 1, &mint_keypair, &pubkey),
        Err(TransactionError::InstructionError(
            0,
            SystemError::ResultWithNegativeLamports.into(),
        ))
    );

    // With bank_transaction_count_fix, transaction_count should include both the successful and
    // failed transactions.
    assert_eq!(bank.transaction_count(), 2);
    assert_eq!(bank.executed_transaction_count(), 2);
    assert_eq!(bank.transaction_error_count(), 1);

    let bank2 = new_bank_from_parent_with_bank_forks(
        bank_forks.as_ref(),
        bank,
        &Pubkey::default(),
        genesis_config.epoch_schedule.first_normal_slot,
    );

    assert_eq!(
        bank2.transfer((mint_amount - amount) + 2, &mint_keypair, &pubkey),
        Err(TransactionError::InstructionError(
            0,
            SystemError::ResultWithNegativeLamports.into(),
        ))
    );

    // The transaction_count inherited from parent bank is 3: 2 from the parent bank and 1 at this bank2
    assert_eq!(bank2.transaction_count(), 3);
    assert_eq!(bank2.executed_transaction_count(), 1);
    assert_eq!(bank2.transaction_error_count(), 1);
}

#[test]
fn test_transfer_to_newb() {
    solana_logger::setup();
    let (genesis_config, mint_keypair) = create_genesis_config(sol_to_lamports(1.));
    let (bank, _bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);
    let amount = genesis_config.rent.minimum_balance(0);
    let pubkey = solana_sdk::pubkey::new_rand();
    bank.transfer(amount, &mint_keypair, &pubkey).unwrap();
    assert_eq!(bank.get_balance(&pubkey), amount);
}

#[test]
fn test_transfer_to_sysvar() {
    solana_logger::setup();
    let (genesis_config, mint_keypair) = create_genesis_config(sol_to_lamports(1.));
    let (bank, bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);
    let amount = genesis_config.rent.minimum_balance(0);

    let normal_pubkey = solana_sdk::pubkey::new_rand();
    let sysvar_pubkey = sysvar::clock::id();
    assert_eq!(bank.get_balance(&normal_pubkey), 0);
    assert_eq!(bank.get_balance(&sysvar_pubkey), 1_169_280);

    bank.transfer(amount, &mint_keypair, &normal_pubkey)
        .unwrap();
    bank.transfer(amount, &mint_keypair, &sysvar_pubkey)
        .unwrap_err();
    assert_eq!(bank.get_balance(&normal_pubkey), amount);
    assert_eq!(bank.get_balance(&sysvar_pubkey), 1_169_280);

    let bank = new_from_parent_with_fork_next_slot(bank, bank_forks.as_ref());
    assert_eq!(bank.get_balance(&normal_pubkey), amount);
    assert_eq!(bank.get_balance(&sysvar_pubkey), 1_169_280);
}

#[test]
fn test_bank_withdraw() {
    let bank = create_simple_test_bank(100);

    // Test no account
    let key = solana_sdk::pubkey::new_rand();
    assert_eq!(
        bank.withdraw(&key, 10),
        Err(TransactionError::AccountNotFound)
    );

    test_utils::deposit(&bank, &key, 3).unwrap();
    assert_eq!(bank.get_balance(&key), 3);

    // Low balance
    assert_eq!(
        bank.withdraw(&key, 10),
        Err(TransactionError::InsufficientFundsForFee)
    );

    // Enough balance
    assert_eq!(bank.withdraw(&key, 2), Ok(()));
    assert_eq!(bank.get_balance(&key), 1);
}

#[test]
fn test_bank_withdraw_from_nonce_account() {
    let (mut genesis_config, _mint_keypair) = create_genesis_config(100_000);
    genesis_config.rent.lamports_per_byte_year = 42;
    let bank = Bank::new_for_tests(&genesis_config);

    let min_balance = bank.get_minimum_balance_for_rent_exemption(nonce::State::size());
    let nonce = Keypair::new();
    let nonce_account = AccountSharedData::new_data(
        min_balance + 42,
        &nonce::state::Versions::new(nonce::State::Initialized(nonce::state::Data::default())),
        &system_program::id(),
    )
    .unwrap();
    bank.store_account(&nonce.pubkey(), &nonce_account);
    assert_eq!(bank.get_balance(&nonce.pubkey()), min_balance + 42);

    // Resulting in non-zero, but sub-min_balance balance fails
    assert_eq!(
        bank.withdraw(&nonce.pubkey(), min_balance / 2),
        Err(TransactionError::InsufficientFundsForFee)
    );
    assert_eq!(bank.get_balance(&nonce.pubkey()), min_balance + 42);

    // Resulting in exactly rent-exempt balance succeeds
    bank.withdraw(&nonce.pubkey(), 42).unwrap();
    assert_eq!(bank.get_balance(&nonce.pubkey()), min_balance);

    // Account closure fails
    assert_eq!(
        bank.withdraw(&nonce.pubkey(), min_balance),
        Err(TransactionError::InsufficientFundsForFee),
    );
}

#[test]
fn test_bank_tx_fee() {
    solana_logger::setup();

    let arbitrary_transfer_amount = 42_000;
    let mint = arbitrary_transfer_amount * 100;
    let leader = solana_sdk::pubkey::new_rand();
    let GenesisConfigInfo {
        mut genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config_with_leader(mint, &leader, 3);
    genesis_config.fee_rate_governor = FeeRateGovernor::new(5000, 0); // something divisible by 2

    let expected_fee_paid = genesis_config
        .fee_rate_governor
        .create_fee_calculator()
        .lamports_per_signature;
    let (expected_fee_collected, expected_fee_burned) =
        genesis_config.fee_rate_governor.burn(expected_fee_paid);

    let (bank, bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);

    let capitalization = bank.capitalization();

    let key = solana_sdk::pubkey::new_rand();
    let tx = system_transaction::transfer(
        &mint_keypair,
        &key,
        arbitrary_transfer_amount,
        bank.last_blockhash(),
    );

    let initial_balance = bank.get_balance(&leader);
    assert_eq!(bank.process_transaction(&tx), Ok(()));
    assert_eq!(bank.get_balance(&key), arbitrary_transfer_amount);
    assert_eq!(
        bank.get_balance(&mint_keypair.pubkey()),
        mint - arbitrary_transfer_amount - expected_fee_paid
    );

    assert_eq!(bank.get_balance(&leader), initial_balance);
    goto_end_of_slot(bank.clone());
    assert_eq!(bank.signature_count(), 1);
    assert_eq!(
        bank.get_balance(&leader),
        initial_balance + expected_fee_collected
    ); // Leader collects fee after the bank is frozen

    // verify capitalization
    let sysvar_and_builtin_program_delta = 1;
    assert_eq!(
        capitalization - expected_fee_burned + sysvar_and_builtin_program_delta,
        bank.capitalization()
    );

    assert_eq!(
        *bank.rewards.read().unwrap(),
        vec![(
            leader,
            RewardInfo {
                reward_type: RewardType::Fee,
                lamports: expected_fee_collected as i64,
                post_balance: initial_balance + expected_fee_collected,
                commission: None,
            }
        )]
    );

    // Verify that an InstructionError collects fees, too
    let bank = new_bank_from_parent_with_bank_forks(bank_forks.as_ref(), bank, &leader, 1);
    let mut tx = system_transaction::transfer(&mint_keypair, &key, 1, bank.last_blockhash());
    // Create a bogus instruction to system_program to cause an instruction error
    tx.message.instructions[0].data[0] = 40;

    bank.process_transaction(&tx)
        .expect_err("instruction error");
    assert_eq!(bank.get_balance(&key), arbitrary_transfer_amount); // no change
    assert_eq!(
        bank.get_balance(&mint_keypair.pubkey()),
        mint - arbitrary_transfer_amount - 2 * expected_fee_paid
    ); // mint_keypair still pays a fee
    goto_end_of_slot(bank.clone());
    assert_eq!(bank.signature_count(), 1);

    // Profit! 2 transaction signatures processed at 3 lamports each
    assert_eq!(
        bank.get_balance(&leader),
        initial_balance + 2 * expected_fee_collected
    );

    assert_eq!(
        *bank.rewards.read().unwrap(),
        vec![(
            leader,
            RewardInfo {
                reward_type: RewardType::Fee,
                lamports: expected_fee_collected as i64,
                post_balance: initial_balance + 2 * expected_fee_collected,
                commission: None,
            }
        )]
    );
}

#[test]
fn test_bank_tx_compute_unit_fee() {
    solana_logger::setup();

    let key = solana_sdk::pubkey::new_rand();
    let arbitrary_transfer_amount = 42;
    let mint = arbitrary_transfer_amount * 10_000_000;
    let leader = solana_sdk::pubkey::new_rand();
    let GenesisConfigInfo {
        mut genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config_with_leader(mint, &leader, 3);
    genesis_config.fee_rate_governor = FeeRateGovernor::new(4, 0); // something divisible by 2

    let (bank, bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);

    let expected_fee_paid = calculate_test_fee(
        &new_sanitized_message(Message::new(&[], Some(&Pubkey::new_unique()))),
        genesis_config
            .fee_rate_governor
            .create_fee_calculator()
            .lamports_per_signature,
        bank.fee_structure(),
    );

    let (expected_fee_collected, expected_fee_burned) =
        genesis_config.fee_rate_governor.burn(expected_fee_paid);

    let capitalization = bank.capitalization();

    let tx = system_transaction::transfer(
        &mint_keypair,
        &key,
        arbitrary_transfer_amount,
        bank.last_blockhash(),
    );

    let initial_balance = bank.get_balance(&leader);
    assert_eq!(bank.process_transaction(&tx), Ok(()));
    assert_eq!(bank.get_balance(&key), arbitrary_transfer_amount);
    assert_eq!(
        bank.get_balance(&mint_keypair.pubkey()),
        mint - arbitrary_transfer_amount - expected_fee_paid
    );

    assert_eq!(bank.get_balance(&leader), initial_balance);
    goto_end_of_slot(bank.clone());
    assert_eq!(bank.signature_count(), 1);
    assert_eq!(
        bank.get_balance(&leader),
        initial_balance + expected_fee_collected
    ); // Leader collects fee after the bank is frozen

    // verify capitalization
    let sysvar_and_builtin_program_delta = 1;
    assert_eq!(
        capitalization - expected_fee_burned + sysvar_and_builtin_program_delta,
        bank.capitalization()
    );

    assert_eq!(
        *bank.rewards.read().unwrap(),
        vec![(
            leader,
            RewardInfo {
                reward_type: RewardType::Fee,
                lamports: expected_fee_collected as i64,
                post_balance: initial_balance + expected_fee_collected,
                commission: None,
            }
        )]
    );

    // Verify that an InstructionError collects fees, too
    let bank = new_bank_from_parent_with_bank_forks(bank_forks.as_ref(), bank, &leader, 1);
    let mut tx = system_transaction::transfer(&mint_keypair, &key, 1, bank.last_blockhash());
    // Create a bogus instruction to system_program to cause an instruction error
    tx.message.instructions[0].data[0] = 40;

    bank.process_transaction(&tx)
        .expect_err("instruction error");
    assert_eq!(bank.get_balance(&key), arbitrary_transfer_amount); // no change
    assert_eq!(
        bank.get_balance(&mint_keypair.pubkey()),
        mint - arbitrary_transfer_amount - 2 * expected_fee_paid
    ); // mint_keypair still pays a fee
    goto_end_of_slot(bank.clone());
    assert_eq!(bank.signature_count(), 1);

    // Profit! 2 transaction signatures processed at 3 lamports each
    assert_eq!(
        bank.get_balance(&leader),
        initial_balance + 2 * expected_fee_collected
    );

    assert_eq!(
        *bank.rewards.read().unwrap(),
        vec![(
            leader,
            RewardInfo {
                reward_type: RewardType::Fee,
                lamports: expected_fee_collected as i64,
                post_balance: initial_balance + 2 * expected_fee_collected,
                commission: None,
            }
        )]
    );
}

#[test]
fn test_bank_blockhash_fee_structure() {
    //solana_logger::setup();

    let leader = solana_sdk::pubkey::new_rand();
    let GenesisConfigInfo {
        mut genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config_with_leader(1_000_000, &leader, 3);
    genesis_config
        .fee_rate_governor
        .target_lamports_per_signature = 5000;
    genesis_config.fee_rate_governor.target_signatures_per_slot = 0;

    let (bank, bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);
    goto_end_of_slot(bank.clone());
    let cheap_blockhash = bank.last_blockhash();
    let cheap_lamports_per_signature = bank.get_lamports_per_signature();
    assert_eq!(cheap_lamports_per_signature, 0);

    let bank = new_bank_from_parent_with_bank_forks(bank_forks.as_ref(), bank, &leader, 1);
    goto_end_of_slot(bank.clone());
    let expensive_blockhash = bank.last_blockhash();
    let expensive_lamports_per_signature = bank.get_lamports_per_signature();
    assert!(cheap_lamports_per_signature < expensive_lamports_per_signature);

    let bank = new_bank_from_parent_with_bank_forks(bank_forks.as_ref(), bank, &leader, 2);

    // Send a transfer using cheap_blockhash
    let key = solana_sdk::pubkey::new_rand();
    let initial_mint_balance = bank.get_balance(&mint_keypair.pubkey());
    let tx = system_transaction::transfer(&mint_keypair, &key, 1, cheap_blockhash);
    assert_eq!(bank.process_transaction(&tx), Ok(()));
    assert_eq!(bank.get_balance(&key), 1);
    let cheap_fee = calculate_test_fee(
        &new_sanitized_message(Message::new(&[], Some(&Pubkey::new_unique()))),
        cheap_lamports_per_signature,
        bank.fee_structure(),
    );
    assert_eq!(
        bank.get_balance(&mint_keypair.pubkey()),
        initial_mint_balance - 1 - cheap_fee
    );

    // Send a transfer using expensive_blockhash
    let key = solana_sdk::pubkey::new_rand();
    let initial_mint_balance = bank.get_balance(&mint_keypair.pubkey());
    let tx = system_transaction::transfer(&mint_keypair, &key, 1, expensive_blockhash);
    assert_eq!(bank.process_transaction(&tx), Ok(()));
    assert_eq!(bank.get_balance(&key), 1);
    let expensive_fee = calculate_test_fee(
        &new_sanitized_message(Message::new(&[], Some(&Pubkey::new_unique()))),
        expensive_lamports_per_signature,
        bank.fee_structure(),
    );
    assert_eq!(
        bank.get_balance(&mint_keypair.pubkey()),
        initial_mint_balance - 1 - expensive_fee
    );
}

#[test]
fn test_bank_blockhash_compute_unit_fee_structure() {
    //solana_logger::setup();

    let leader = solana_sdk::pubkey::new_rand();
    let GenesisConfigInfo {
        mut genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config_with_leader(1_000_000_000, &leader, 3);
    genesis_config
        .fee_rate_governor
        .target_lamports_per_signature = 1000;
    genesis_config.fee_rate_governor.target_signatures_per_slot = 1;

    let (bank, bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);
    goto_end_of_slot(bank.clone());
    let cheap_blockhash = bank.last_blockhash();
    let cheap_lamports_per_signature = bank.get_lamports_per_signature();
    assert_eq!(cheap_lamports_per_signature, 0);

    let bank = new_bank_from_parent_with_bank_forks(bank_forks.as_ref(), bank, &leader, 1);
    goto_end_of_slot(bank.clone());
    let expensive_blockhash = bank.last_blockhash();
    let expensive_lamports_per_signature = bank.get_lamports_per_signature();
    assert!(cheap_lamports_per_signature < expensive_lamports_per_signature);

    let bank = new_bank_from_parent_with_bank_forks(bank_forks.as_ref(), bank, &leader, 2);

    // Send a transfer using cheap_blockhash
    let key = solana_sdk::pubkey::new_rand();
    let initial_mint_balance = bank.get_balance(&mint_keypair.pubkey());
    let tx = system_transaction::transfer(&mint_keypair, &key, 1, cheap_blockhash);
    assert_eq!(bank.process_transaction(&tx), Ok(()));
    assert_eq!(bank.get_balance(&key), 1);
    let cheap_fee = calculate_test_fee(
        &new_sanitized_message(Message::new(&[], Some(&Pubkey::new_unique()))),
        cheap_lamports_per_signature,
        bank.fee_structure(),
    );
    assert_eq!(
        bank.get_balance(&mint_keypair.pubkey()),
        initial_mint_balance - 1 - cheap_fee
    );

    // Send a transfer using expensive_blockhash
    let key = solana_sdk::pubkey::new_rand();
    let initial_mint_balance = bank.get_balance(&mint_keypair.pubkey());
    let tx = system_transaction::transfer(&mint_keypair, &key, 1, expensive_blockhash);
    assert_eq!(bank.process_transaction(&tx), Ok(()));
    assert_eq!(bank.get_balance(&key), 1);
    let expensive_fee = calculate_test_fee(
        &new_sanitized_message(Message::new(&[], Some(&Pubkey::new_unique()))),
        expensive_lamports_per_signature,
        bank.fee_structure(),
    );
    assert_eq!(
        bank.get_balance(&mint_keypair.pubkey()),
        initial_mint_balance - 1 - expensive_fee
    );
}

#[test]
fn test_filter_program_errors_and_collect_fee() {
    let leader = solana_sdk::pubkey::new_rand();
    let GenesisConfigInfo { genesis_config, .. } =
        create_genesis_config_with_leader(100_000, &leader, 3);
    let mut bank = Bank::new_for_tests(&genesis_config);
    // this test is only for when `feature_set::reward_full_priority_fee` inactivated
    bank.deactivate_feature(&feature_set::reward_full_priority_fee::id());

    let tx_fee = 42;
    let fee_details = FeeDetails::new(tx_fee, 0, false);
    let processing_results = vec![
        Err(TransactionError::AccountNotFound),
        new_executed_processing_result(Ok(()), fee_details),
        new_executed_processing_result(
            Err(TransactionError::InstructionError(
                1,
                SystemError::ResultWithNegativeLamports.into(),
            )),
            fee_details,
        ),
        Ok(ProcessedTransaction::FeesOnly(Box::new(
            FeesOnlyTransaction {
                load_error: TransactionError::InvalidProgramForExecution,
                rollback_accounts: RollbackAccounts::default(),
                fee_details,
            },
        ))),
    ];
    let initial_balance = bank.get_balance(&leader);

    bank.filter_program_errors_and_collect_fee(&processing_results);
    bank.freeze();
    assert_eq!(
        bank.get_balance(&leader),
        initial_balance + bank.fee_rate_governor.burn(tx_fee * 3).0
    );
}

#[test]
fn test_filter_program_errors_and_collect_priority_fee() {
    let leader = solana_sdk::pubkey::new_rand();
    let GenesisConfigInfo { genesis_config, .. } =
        create_genesis_config_with_leader(1000000, &leader, 3);
    let mut bank = Bank::new_for_tests(&genesis_config);
    // this test is only for when `feature_set::reward_full_priority_fee` inactivated
    bank.deactivate_feature(&feature_set::reward_full_priority_fee::id());

    let priority_fee = 42;
    let fee_details: FeeDetails = FeeDetails::new(0, priority_fee, false);
    let processing_results = vec![
        Err(TransactionError::AccountNotFound),
        new_executed_processing_result(Ok(()), fee_details),
        new_executed_processing_result(
            Err(TransactionError::InstructionError(
                1,
                SystemError::ResultWithNegativeLamports.into(),
            )),
            fee_details,
        ),
        Ok(ProcessedTransaction::FeesOnly(Box::new(
            FeesOnlyTransaction {
                load_error: TransactionError::InvalidProgramForExecution,
                rollback_accounts: RollbackAccounts::default(),
                fee_details,
            },
        ))),
    ];
    let initial_balance = bank.get_balance(&leader);

    bank.filter_program_errors_and_collect_fee(&processing_results);
    bank.freeze();
    assert_eq!(
        bank.get_balance(&leader),
        initial_balance + bank.fee_rate_governor.burn(priority_fee * 3).0
    );
}

#[test]
fn test_debits_before_credits() {
    let (genesis_config, mint_keypair) =
        create_genesis_config_no_tx_fee_no_rent(sol_to_lamports(2.));
    let (bank, _bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);
    let keypair = Keypair::new();
    let tx0 = system_transaction::transfer(
        &mint_keypair,
        &keypair.pubkey(),
        sol_to_lamports(2.),
        genesis_config.hash(),
    );
    let tx1 = system_transaction::transfer(
        &keypair,
        &mint_keypair.pubkey(),
        sol_to_lamports(1.),
        genesis_config.hash(),
    );
    let txs = vec![tx0, tx1];
    let results = bank.process_transactions(txs.iter());
    assert!(results[1].is_err());

    // Assert bad transactions aren't counted.
    assert_eq!(bank.transaction_count(), 1);
    assert_eq!(bank.non_vote_transaction_count_since_restart(), 1);
}

#[test]
fn test_readonly_accounts() {
    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config_with_leader(500, &solana_sdk::pubkey::new_rand(), 0);
    let (bank, _bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);

    let vote_pubkey0 = solana_sdk::pubkey::new_rand();
    let vote_pubkey1 = solana_sdk::pubkey::new_rand();
    let vote_pubkey2 = solana_sdk::pubkey::new_rand();
    let authorized_voter = Keypair::new();
    let payer0 = Keypair::new();
    let payer1 = Keypair::new();

    // Create vote accounts
    let vote_account0 =
        vote_state::create_account(&vote_pubkey0, &authorized_voter.pubkey(), 0, 100);
    let vote_account1 =
        vote_state::create_account(&vote_pubkey1, &authorized_voter.pubkey(), 0, 100);
    let vote_account2 =
        vote_state::create_account(&vote_pubkey2, &authorized_voter.pubkey(), 0, 100);
    bank.store_account(&vote_pubkey0, &vote_account0);
    bank.store_account(&vote_pubkey1, &vote_account1);
    bank.store_account(&vote_pubkey2, &vote_account2);

    // Fund payers
    bank.transfer(10, &mint_keypair, &payer0.pubkey()).unwrap();
    bank.transfer(10, &mint_keypair, &payer1.pubkey()).unwrap();
    bank.transfer(1, &mint_keypair, &authorized_voter.pubkey())
        .unwrap();

    let vote = Vote::new(vec![1], Hash::default());
    let ix0 = vote_instruction::vote(&vote_pubkey0, &authorized_voter.pubkey(), vote.clone());
    let tx0 = Transaction::new_signed_with_payer(
        &[ix0],
        Some(&payer0.pubkey()),
        &[&payer0, &authorized_voter],
        bank.last_blockhash(),
    );
    let ix1 = vote_instruction::vote(&vote_pubkey1, &authorized_voter.pubkey(), vote.clone());
    let tx1 = Transaction::new_signed_with_payer(
        &[ix1],
        Some(&payer1.pubkey()),
        &[&payer1, &authorized_voter],
        bank.last_blockhash(),
    );
    let txs = vec![tx0, tx1];
    let results = bank.process_transactions(txs.iter());

    // If multiple transactions attempt to read the same account, they should succeed.
    // Vote authorized_voter and sysvar accounts are given read-only handling
    assert_eq!(results[0], Ok(()));
    assert_eq!(results[1], Ok(()));

    let ix0 = vote_instruction::vote(&vote_pubkey2, &authorized_voter.pubkey(), vote);
    let tx0 = Transaction::new_signed_with_payer(
        &[ix0],
        Some(&payer0.pubkey()),
        &[&payer0, &authorized_voter],
        bank.last_blockhash(),
    );
    let tx1 = system_transaction::transfer(
        &authorized_voter,
        &solana_sdk::pubkey::new_rand(),
        1,
        bank.last_blockhash(),
    );
    let txs = vec![tx0, tx1];
    let results = bank.process_transactions(txs.iter());
    // However, an account may not be locked as read-only and writable at the same time.
    assert_eq!(results[0], Ok(()));
    assert_eq!(results[1], Err(TransactionError::AccountInUse));
}

#[test]
fn test_interleaving_locks() {
    let (genesis_config, mint_keypair) = create_genesis_config(sol_to_lamports(1.));
    let (bank, _bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);
    let alice = Keypair::new();
    let bob = Keypair::new();
    let amount = genesis_config.rent.minimum_balance(0);

    let tx1 = system_transaction::transfer(
        &mint_keypair,
        &alice.pubkey(),
        amount,
        genesis_config.hash(),
    );
    let pay_alice = vec![tx1];

    let lock_result = bank.prepare_batch_for_tests(pay_alice);
    let commit_results = bank
        .load_execute_and_commit_transactions(
            &lock_result,
            MAX_PROCESSING_AGE,
            false,
            ExecutionRecordingConfig::new_single_setting(false),
            &mut ExecuteTimings::default(),
            None,
        )
        .0;
    assert!(commit_results[0].is_ok());

    // try executing an interleaved transfer twice
    assert_eq!(
        bank.transfer(amount, &mint_keypair, &bob.pubkey()),
        Err(TransactionError::AccountInUse)
    );
    // the second time should fail as well
    // this verifies that `unlock_accounts` doesn't unlock `AccountInUse` accounts
    assert_eq!(
        bank.transfer(amount, &mint_keypair, &bob.pubkey()),
        Err(TransactionError::AccountInUse)
    );

    drop(lock_result);

    assert!(bank
        .transfer(2 * amount, &mint_keypair, &bob.pubkey())
        .is_ok());
}

#[test_case(false; "disable fees-only transactions")]
#[test_case(true; "enable fees-only transactions")]
fn test_load_and_execute_commit_transactions_fees_only(enable_fees_only_txs: bool) {
    let GenesisConfigInfo {
        mut genesis_config, ..
    } = genesis_utils::create_genesis_config(100 * LAMPORTS_PER_SOL);
    if !enable_fees_only_txs {
        genesis_config
            .accounts
            .remove(&solana_sdk::feature_set::enable_transaction_loading_failure_fees::id());
    }
    genesis_config.rent = Rent::default();
    genesis_config.fee_rate_governor = FeeRateGovernor::new(5000, 0);
    let (bank, _bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);
    let bank = Bank::new_from_parent(
        bank,
        &Pubkey::new_unique(),
        genesis_config.epoch_schedule.get_first_slot_in_epoch(1),
    );

    // Use rent-paying fee payer to show that rent is not collected for fees
    // only transactions even when they use a rent-paying account.
    let rent_paying_fee_payer = Pubkey::new_unique();
    bank.store_account(
        &rent_paying_fee_payer,
        &AccountSharedData::new(
            genesis_config.rent.minimum_balance(0) - 1,
            0,
            &system_program::id(),
        ),
    );

    // Use nonce to show that loaded account stats also included loaded
    // nonce account size
    let nonce_size = nonce::State::size();
    let nonce_balance = genesis_config.rent.minimum_balance(nonce_size);
    let nonce_pubkey = Pubkey::new_unique();
    let nonce_authority = rent_paying_fee_payer;
    let nonce_initial_hash = DurableNonce::from_blockhash(&Hash::new_unique());
    let nonce_data = nonce::state::Data::new(nonce_authority, nonce_initial_hash, 5000);
    let nonce_account = AccountSharedData::new_data(
        nonce_balance,
        &nonce::state::Versions::new(nonce::State::Initialized(nonce_data.clone())),
        &system_program::id(),
    )
    .unwrap();
    bank.store_account(&nonce_pubkey, &nonce_account);

    // Invoke missing program to trigger load error in order to commit a
    // fees-only transaction
    let missing_program_id = Pubkey::new_unique();
    let transaction = Transaction::new_unsigned(Message::new_with_blockhash(
        &[
            system_instruction::advance_nonce_account(&nonce_pubkey, &rent_paying_fee_payer),
            Instruction::new_with_bincode(missing_program_id, &0, vec![]),
        ],
        Some(&rent_paying_fee_payer),
        &nonce_data.blockhash(),
    ));

    let batch = bank.prepare_batch_for_tests(vec![transaction]);
    let commit_results = bank
        .load_execute_and_commit_transactions(
            &batch,
            MAX_PROCESSING_AGE,
            true,
            ExecutionRecordingConfig::new_single_setting(true),
            &mut ExecuteTimings::default(),
            None,
        )
        .0;

    if enable_fees_only_txs {
        assert_eq!(
            commit_results,
            vec![Ok(CommittedTransaction {
                status: Err(TransactionError::ProgramAccountNotFound),
                log_messages: None,
                inner_instructions: None,
                return_data: None,
                executed_units: 0,
                fee_details: FeeDetails::new(5000, 0, true),
                rent_debits: RentDebits::default(),
                loaded_account_stats: TransactionLoadedAccountsStats {
                    loaded_accounts_count: 2,
                    loaded_accounts_data_size: nonce_size as u32,
                },
            })]
        );
    } else {
        assert_eq!(
            commit_results,
            vec![Err(TransactionError::ProgramAccountNotFound)]
        );
    }
}

#[test]
fn test_readonly_relaxed_locks() {
    let (genesis_config, _) = create_genesis_config(3);
    let bank = Bank::new_for_tests(&genesis_config);
    let key0 = Keypair::new();
    let key1 = Keypair::new();
    let key2 = Keypair::new();
    let key3 = solana_sdk::pubkey::new_rand();

    let message = Message {
        header: MessageHeader {
            num_required_signatures: 1,
            num_readonly_signed_accounts: 0,
            num_readonly_unsigned_accounts: 1,
        },
        account_keys: vec![key0.pubkey(), key3],
        recent_blockhash: Hash::default(),
        instructions: vec![],
    };
    let tx = Transaction::new(&[&key0], message, genesis_config.hash());
    let txs = vec![tx];

    let batch0 = bank.prepare_batch_for_tests(txs);
    assert!(batch0.lock_results()[0].is_ok());

    // Try locking accounts, locking a previously read-only account as writable
    // should fail
    let message = Message {
        header: MessageHeader {
            num_required_signatures: 1,
            num_readonly_signed_accounts: 0,
            num_readonly_unsigned_accounts: 0,
        },
        account_keys: vec![key1.pubkey(), key3],
        recent_blockhash: Hash::default(),
        instructions: vec![],
    };
    let tx = Transaction::new(&[&key1], message, genesis_config.hash());
    let txs = vec![tx];

    let batch1 = bank.prepare_batch_for_tests(txs);
    assert!(batch1.lock_results()[0].is_err());

    // Try locking a previously read-only account a 2nd time; should succeed
    let message = Message {
        header: MessageHeader {
            num_required_signatures: 1,
            num_readonly_signed_accounts: 0,
            num_readonly_unsigned_accounts: 1,
        },
        account_keys: vec![key2.pubkey(), key3],
        recent_blockhash: Hash::default(),
        instructions: vec![],
    };
    let tx = Transaction::new(&[&key2], message, genesis_config.hash());
    let txs = vec![tx];

    let batch2 = bank.prepare_batch_for_tests(txs);
    assert!(batch2.lock_results()[0].is_ok());
}

#[test]
fn test_bank_invalid_account_index() {
    let (genesis_config, mint_keypair) = create_genesis_config(1);
    let keypair = Keypair::new();
    let bank = Bank::new_for_tests(&genesis_config);

    let tx =
        system_transaction::transfer(&mint_keypair, &keypair.pubkey(), 1, genesis_config.hash());

    let mut tx_invalid_program_index = tx.clone();
    tx_invalid_program_index.message.instructions[0].program_id_index = 42;
    assert_eq!(
        bank.process_transaction(&tx_invalid_program_index),
        Err(TransactionError::SanitizeFailure)
    );

    let mut tx_invalid_account_index = tx;
    tx_invalid_account_index.message.instructions[0].accounts[0] = 42;
    assert_eq!(
        bank.process_transaction(&tx_invalid_account_index),
        Err(TransactionError::SanitizeFailure)
    );
}

#[test]
fn test_bank_pay_to_self() {
    let (genesis_config, mint_keypair) = create_genesis_config_no_tx_fee(sol_to_lamports(1.));
    let key1 = Keypair::new();
    let (bank, _bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);
    let amount = genesis_config.rent.minimum_balance(0);

    bank.transfer(amount, &mint_keypair, &key1.pubkey())
        .unwrap();
    assert_eq!(bank.get_balance(&key1.pubkey()), amount);
    let tx = system_transaction::transfer(&key1, &key1.pubkey(), amount, genesis_config.hash());
    let _res = bank.process_transaction(&tx);

    assert_eq!(bank.get_balance(&key1.pubkey()), amount);
    bank.get_signature_status(&tx.signatures[0])
        .unwrap()
        .unwrap();
}

fn new_from_parent(parent: Arc<Bank>) -> Bank {
    let slot = parent.slot() + 1;
    let collector_id = Pubkey::default();
    Bank::new_from_parent(parent, &collector_id, slot)
}

fn new_from_parent_with_fork_next_slot(parent: Arc<Bank>, fork: &RwLock<BankForks>) -> Arc<Bank> {
    let slot = parent.slot() + 1;
    new_bank_from_parent_with_bank_forks(fork, parent, &Pubkey::default(), slot)
}

/// Verify that the parent's vector is computed correctly
#[test]
fn test_bank_parents() {
    let (genesis_config, _) = create_genesis_config(1);
    let parent = Arc::new(Bank::new_for_tests(&genesis_config));

    let bank = new_from_parent(parent.clone());
    assert!(Arc::ptr_eq(&bank.parents()[0], &parent));
}

/// Verifies that transactions are dropped if they have already been processed
#[test]
fn test_tx_already_processed() {
    let (genesis_config, mint_keypair) = create_genesis_config(sol_to_lamports(1.));
    let (bank, _bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);

    let key1 = Keypair::new();
    let mut tx = system_transaction::transfer(
        &mint_keypair,
        &key1.pubkey(),
        genesis_config.rent.minimum_balance(0),
        genesis_config.hash(),
    );

    // First process `tx` so that the status cache is updated
    assert_eq!(bank.process_transaction(&tx), Ok(()));

    // Ensure that signature check works
    assert_eq!(
        bank.process_transaction(&tx),
        Err(TransactionError::AlreadyProcessed)
    );

    // Change transaction signature to simulate processing a transaction with a different signature
    // for the same message.
    tx.signatures[0] = Signature::default();

    // Ensure that message hash check works
    assert_eq!(
        bank.process_transaction(&tx),
        Err(TransactionError::AlreadyProcessed)
    );
}

/// Verifies that last ids and status cache are correctly referenced from parent
#[test]
fn test_bank_parent_already_processed() {
    let (genesis_config, mint_keypair) = create_genesis_config(sol_to_lamports(1.));
    let key1 = Keypair::new();
    let (parent, bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);
    let amount = genesis_config.rent.minimum_balance(0);

    let tx =
        system_transaction::transfer(&mint_keypair, &key1.pubkey(), amount, genesis_config.hash());
    assert_eq!(parent.process_transaction(&tx), Ok(()));
    let bank = new_from_parent_with_fork_next_slot(parent, bank_forks.as_ref());
    assert_eq!(
        bank.process_transaction(&tx),
        Err(TransactionError::AlreadyProcessed)
    );
}

/// Verifies that last ids and accounts are correctly referenced from parent
#[test]
fn test_bank_parent_account_spend() {
    let (genesis_config, mint_keypair) = create_genesis_config_no_tx_fee(sol_to_lamports(1.0));
    let key1 = Keypair::new();
    let key2 = Keypair::new();
    let (parent, bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);
    let amount = genesis_config.rent.minimum_balance(0);

    let tx =
        system_transaction::transfer(&mint_keypair, &key1.pubkey(), amount, genesis_config.hash());
    assert_eq!(parent.process_transaction(&tx), Ok(()));
    let bank = new_from_parent_with_fork_next_slot(parent.clone(), bank_forks.as_ref());
    let tx = system_transaction::transfer(&key1, &key2.pubkey(), amount, genesis_config.hash());
    assert_eq!(bank.process_transaction(&tx), Ok(()));
    assert_eq!(parent.get_signature_status(&tx.signatures[0]), None);
}

#[test_case(false; "accounts lt hash disabled")]
#[test_case(true; "accounts lt hash enabled")]
fn test_bank_hash_internal_state(is_accounts_lt_hash_enabled: bool) {
    let (genesis_config, mint_keypair) =
        create_genesis_config_no_tx_fee_no_rent(sol_to_lamports(1.));
    let (bank0, _bank_forks0) = Bank::new_with_bank_forks_for_tests(&genesis_config);
    let (bank1, bank_forks1) = Bank::new_with_bank_forks_for_tests(&genesis_config);
    bank0
        .rc
        .accounts
        .accounts_db
        .set_is_experimental_accumulator_hash_enabled(is_accounts_lt_hash_enabled);
    bank1
        .rc
        .accounts
        .accounts_db
        .set_is_experimental_accumulator_hash_enabled(is_accounts_lt_hash_enabled);
    assert_eq!(
        bank0.is_accounts_lt_hash_enabled(),
        is_accounts_lt_hash_enabled,
    );
    assert_eq!(
        bank1.is_accounts_lt_hash_enabled(),
        is_accounts_lt_hash_enabled,
    );

    let initial_state = bank0.hash_internal_state();
    assert_eq!(bank1.hash_internal_state(), initial_state);

    // Ensure calling hash_internal_state() again does *not* change the bank hash value
    assert_eq!(bank1.hash_internal_state(), initial_state);

    let amount = genesis_config.rent.minimum_balance(0);
    let pubkey = solana_sdk::pubkey::new_rand();
    bank0.transfer(amount, &mint_keypair, &pubkey).unwrap();
    bank0.freeze();
    assert_ne!(bank0.hash_internal_state(), initial_state);
    bank1.transfer(amount, &mint_keypair, &pubkey).unwrap();
    bank1.freeze();
    assert_eq!(bank0.hash_internal_state(), bank1.hash_internal_state());

    // Checkpointing should always result in a new state
    let bank2 = new_from_parent_with_fork_next_slot(bank1, &bank_forks1);
    assert_ne!(bank0.hash_internal_state(), bank2.hash_internal_state());

    let pubkey2 = solana_sdk::pubkey::new_rand();
    bank2.transfer(amount, &mint_keypair, &pubkey2).unwrap();
    bank2.squash();
    bank2.force_flush_accounts_cache();
    bank2.update_accounts_hash(CalcAccountsHashDataSource::Storages, false, false);
    assert!(bank2.verify_accounts_hash(None, VerifyAccountsHashConfig::default_for_test(), None,));
}

#[test_case(false; "accounts lt hash disabled")]
#[test_case(true; "accounts lt hash enabled")]
fn test_bank_hash_internal_state_verify(is_accounts_lt_hash_enabled: bool) {
    for pass in 0..4 {
        let (genesis_config, mint_keypair) =
            create_genesis_config_no_tx_fee_no_rent(sol_to_lamports(1.));
        let (bank0, bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);
        bank0
            .rc
            .accounts
            .accounts_db
            .set_is_experimental_accumulator_hash_enabled(is_accounts_lt_hash_enabled);
        assert_eq!(
            bank0.is_accounts_lt_hash_enabled(),
            is_accounts_lt_hash_enabled,
        );

        let amount = genesis_config.rent.minimum_balance(0);
        let pubkey = solana_sdk::pubkey::new_rand();
        bank0.transfer(amount, &mint_keypair, &pubkey).unwrap();

        let bank0_state = bank0.hash_internal_state();
        // Checkpointing should result in a new state while freezing the parent
        let bank2 = new_bank_from_parent_with_bank_forks(
            &bank_forks,
            bank0.clone(),
            &solana_sdk::pubkey::new_rand(),
            2,
        );
        assert_ne!(bank0_state, bank2.hash_internal_state());

        // Checkpointing should modify the checkpoint's state when freezed
        assert_ne!(bank0_state, bank0.hash_internal_state());

        // Checkpointing should never modify the checkpoint's state once frozen
        add_root_and_flush_write_cache(&bank0);
        let bank0_state = bank0.hash_internal_state();
        if pass == 0 {
            // we later modify bank 2, so this flush is destructive to the test
            bank2.freeze();
            add_root_and_flush_write_cache(&bank2);
            bank2.update_accounts_hash_for_tests();
            assert!(bank2.verify_accounts_hash(
                None,
                VerifyAccountsHashConfig::default_for_test(),
                None,
            ));
        }
        let bank3 = new_bank_from_parent_with_bank_forks(
            &bank_forks,
            bank0.clone(),
            &solana_sdk::pubkey::new_rand(),
            3,
        );
        assert_eq!(bank0_state, bank0.hash_internal_state());
        if pass == 0 {
            // this relies on us having set bank2's accounts hash in the pass==0 if above
            assert!(bank2.verify_accounts_hash(
                None,
                VerifyAccountsHashConfig::default_for_test(),
                None,
            ));
            continue;
        }
        if pass == 1 {
            // flushing slot 3 here causes us to mark it as a root. Marking it as a root
            // prevents us from marking slot 2 as a root later since slot 2 is < slot 3.
            // Doing so throws an assert. So, we can't flush 3 until 2 is flushed.
            bank3.freeze();
            add_root_and_flush_write_cache(&bank3);
            bank3.update_accounts_hash_for_tests();
            assert!(bank3.verify_accounts_hash(
                None,
                VerifyAccountsHashConfig::default_for_test(),
                None,
            ));
            continue;
        }

        let pubkey2 = solana_sdk::pubkey::new_rand();
        bank2.transfer(amount, &mint_keypair, &pubkey2).unwrap();
        bank2.freeze(); // <-- keep freeze() *outside* `if pass == 2 {}`
        if pass == 2 {
            add_root_and_flush_write_cache(&bank2);
            bank2.update_accounts_hash_for_tests();
            assert!(bank2.verify_accounts_hash(
                None,
                VerifyAccountsHashConfig::default_for_test(),
                None,
            ));

            if is_accounts_lt_hash_enabled {
                // Verifying the accounts lt hash is only intended to be called at startup, and
                // normally in the background.  Since here we're *not* at startup, and doing it
                // in the foreground, the verification uses the accounts index.  The test just
                // rooted bank2, and will root bank3 next; but they are on different forks,
                // which is not valid.  This causes the accounts index to see accounts from
                // bank2 and bank3, which causes verifying bank3's accounts lt hash to fail.
                // To workaround this "issue", we cannot root bank2 when the accounts lt hash
                // is enabled.
                continue;
            }
        }

        bank3.freeze();
        add_root_and_flush_write_cache(&bank3);
        bank3.update_accounts_hash_for_tests();
        assert!(bank3.verify_accounts_hash(
            None,
            VerifyAccountsHashConfig::default_for_test(),
            None,
        ));
    }
}

#[test]
#[should_panic(expected = "self.is_frozen()")]
fn test_verify_hash_unfrozen() {
    let bank = create_simple_test_bank(2_000);
    assert!(bank.verify_hash());
}

#[test]
fn test_verify_snapshot_bank() {
    solana_logger::setup();
    let pubkey = solana_sdk::pubkey::new_rand();
    let (genesis_config, mint_keypair) = create_genesis_config(sol_to_lamports(1.));
    let (bank, _bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);
    bank.transfer(
        genesis_config.rent.minimum_balance(0),
        &mint_keypair,
        &pubkey,
    )
    .unwrap();
    bank.freeze();
    add_root_and_flush_write_cache(&bank);
    bank.update_accounts_hash_for_tests();
    assert!(bank.verify_snapshot_bank(true, false, false, bank.slot(), None, None,));

    // tamper the bank after freeze!
    bank.increment_signature_count(1);
    assert!(!bank.verify_snapshot_bank(true, false, false, bank.slot(), None, None,));
}

// Test that two bank forks with the same accounts should not hash to the same value.
#[test]
fn test_bank_hash_internal_state_same_account_different_fork() {
    solana_logger::setup();
    let (genesis_config, mint_keypair) = create_genesis_config(sol_to_lamports(1.));
    let amount = genesis_config.rent.minimum_balance(0);
    let (bank0, bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);
    let initial_state = bank0.hash_internal_state();
    let bank1 = new_bank_from_parent_with_bank_forks(
        bank_forks.as_ref(),
        bank0.clone(),
        &Pubkey::default(),
        1,
    );
    assert_ne!(bank1.hash_internal_state(), initial_state);

    info!("transfer bank1");
    let pubkey = solana_sdk::pubkey::new_rand();
    bank1.transfer(amount, &mint_keypair, &pubkey).unwrap();
    assert_ne!(bank1.hash_internal_state(), initial_state);

    info!("transfer bank2");
    // bank2 should not hash the same as bank1
    let bank2 =
        new_bank_from_parent_with_bank_forks(bank_forks.as_ref(), bank0, &Pubkey::default(), 2);
    bank2.transfer(amount, &mint_keypair, &pubkey).unwrap();
    assert_ne!(bank2.hash_internal_state(), initial_state);
    assert_ne!(bank1.hash_internal_state(), bank2.hash_internal_state());
}

#[test]
fn test_hash_internal_state_genesis() {
    let bank0 = Bank::new_for_tests(&create_genesis_config(10).0);
    let bank1 = Bank::new_for_tests(&create_genesis_config(20).0);
    assert_ne!(bank0.hash_internal_state(), bank1.hash_internal_state());
}

// See that the order of two transfers does not affect the result
// of hash_internal_state
#[test]
fn test_hash_internal_state_order() {
    let (genesis_config, mint_keypair) = create_genesis_config(sol_to_lamports(1.));
    let amount = genesis_config.rent.minimum_balance(0);
    let (bank0, _bank_forks0) = Bank::new_with_bank_forks_for_tests(&genesis_config);
    let (bank1, _bank_forks1) = Bank::new_with_bank_forks_for_tests(&genesis_config);
    assert_eq!(bank0.hash_internal_state(), bank1.hash_internal_state());
    let key0 = solana_sdk::pubkey::new_rand();
    let key1 = solana_sdk::pubkey::new_rand();
    bank0.transfer(amount, &mint_keypair, &key0).unwrap();
    bank0.transfer(amount * 2, &mint_keypair, &key1).unwrap();

    bank1.transfer(amount * 2, &mint_keypair, &key1).unwrap();
    bank1.transfer(amount, &mint_keypair, &key0).unwrap();

    assert_eq!(bank0.hash_internal_state(), bank1.hash_internal_state());
}

#[test]
fn test_hash_internal_state_error() {
    solana_logger::setup();
    let (genesis_config, mint_keypair) = create_genesis_config(sol_to_lamports(1.));
    let amount = genesis_config.rent.minimum_balance(0);
    let (bank, _bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);
    let key0 = solana_sdk::pubkey::new_rand();
    bank.transfer(amount, &mint_keypair, &key0).unwrap();
    let orig = bank.hash_internal_state();

    // Transfer will error but still take a fee
    assert!(bank
        .transfer(sol_to_lamports(1.), &mint_keypair, &key0)
        .is_err());
    assert_ne!(orig, bank.hash_internal_state());

    let orig = bank.hash_internal_state();
    let empty_keypair = Keypair::new();
    assert!(bank.transfer(amount, &empty_keypair, &key0).is_err());
    assert_eq!(orig, bank.hash_internal_state());
}

#[test]
fn test_bank_hash_internal_state_squash() {
    let collector_id = Pubkey::default();
    let bank0 = Arc::new(Bank::new_for_tests(&create_genesis_config(10).0));
    let hash0 = bank0.hash_internal_state();
    // save hash0 because new_from_parent
    // updates sysvar entries

    let bank1 = Bank::new_from_parent(bank0, &collector_id, 1);

    // no delta in bank1, hashes should always update
    assert_ne!(hash0, bank1.hash_internal_state());

    // remove parent
    bank1.squash();
    assert!(bank1.parents().is_empty());
}

/// Verifies that last ids and accounts are correctly referenced from parent
#[test]
fn test_bank_squash() {
    solana_logger::setup();
    let (genesis_config, mint_keypair) = create_genesis_config_no_tx_fee(sol_to_lamports(2.));
    let key1 = Keypair::new();
    let key2 = Keypair::new();
    let (parent, bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);
    let amount = genesis_config.rent.minimum_balance(0);

    let tx_transfer_mint_to_1 =
        system_transaction::transfer(&mint_keypair, &key1.pubkey(), amount, genesis_config.hash());
    trace!("parent process tx ");
    assert_eq!(parent.process_transaction(&tx_transfer_mint_to_1), Ok(()));
    trace!("done parent process tx ");
    assert_eq!(parent.transaction_count(), 1);
    assert_eq!(parent.non_vote_transaction_count_since_restart(), 1);
    assert_eq!(
        parent.get_signature_status(&tx_transfer_mint_to_1.signatures[0]),
        Some(Ok(()))
    );

    trace!("new from parent");
    let bank = new_from_parent_with_fork_next_slot(parent.clone(), bank_forks.as_ref());
    trace!("done new from parent");
    assert_eq!(
        bank.get_signature_status(&tx_transfer_mint_to_1.signatures[0]),
        Some(Ok(()))
    );

    assert_eq!(bank.transaction_count(), parent.transaction_count());
    assert_eq!(
        bank.non_vote_transaction_count_since_restart(),
        parent.non_vote_transaction_count_since_restart()
    );
    let tx_transfer_1_to_2 =
        system_transaction::transfer(&key1, &key2.pubkey(), amount, genesis_config.hash());
    assert_eq!(bank.process_transaction(&tx_transfer_1_to_2), Ok(()));
    assert_eq!(bank.transaction_count(), 2);
    assert_eq!(bank.non_vote_transaction_count_since_restart(), 2);
    assert_eq!(parent.transaction_count(), 1);
    assert_eq!(parent.non_vote_transaction_count_since_restart(), 1);
    assert_eq!(
        parent.get_signature_status(&tx_transfer_1_to_2.signatures[0]),
        None
    );

    for _ in 0..3 {
        // first time these should match what happened above, assert that parents are ok
        assert_eq!(bank.get_balance(&key1.pubkey()), 0);
        assert_eq!(bank.get_account(&key1.pubkey()), None);
        assert_eq!(bank.get_balance(&key2.pubkey()), amount);
        trace!("start");
        assert_eq!(
            bank.get_signature_status(&tx_transfer_mint_to_1.signatures[0]),
            Some(Ok(()))
        );
        assert_eq!(
            bank.get_signature_status(&tx_transfer_1_to_2.signatures[0]),
            Some(Ok(()))
        );

        // works iteration 0, no-ops on iteration 1 and 2
        trace!("SQUASH");
        bank.squash();

        assert_eq!(parent.transaction_count(), 1);
        assert_eq!(parent.non_vote_transaction_count_since_restart(), 1);
        assert_eq!(bank.transaction_count(), 2);
        assert_eq!(bank.non_vote_transaction_count_since_restart(), 2);
    }
}

#[test]
fn test_bank_get_account_in_parent_after_squash() {
    let (genesis_config, mint_keypair) = create_genesis_config(sol_to_lamports(1.));
    let (parent, _bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);
    let amount = genesis_config.rent.minimum_balance(0);

    let key1 = Keypair::new();

    parent
        .transfer(amount, &mint_keypair, &key1.pubkey())
        .unwrap();
    assert_eq!(parent.get_balance(&key1.pubkey()), amount);
    let bank = new_from_parent(parent.clone());
    bank.squash();
    assert_eq!(parent.get_balance(&key1.pubkey()), amount);
}

#[test]
fn test_bank_get_account_in_parent_after_squash2() {
    solana_logger::setup();
    let (genesis_config, mint_keypair) = create_genesis_config(sol_to_lamports(1.));
    let (bank0, bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);
    let amount = genesis_config.rent.minimum_balance(0);

    let key1 = Keypair::new();

    bank0
        .transfer(amount, &mint_keypair, &key1.pubkey())
        .unwrap();
    assert_eq!(bank0.get_balance(&key1.pubkey()), amount);

    let bank1 = new_bank_from_parent_with_bank_forks(
        bank_forks.as_ref(),
        bank0.clone(),
        &Pubkey::default(),
        1,
    );
    bank1
        .transfer(3 * amount, &mint_keypair, &key1.pubkey())
        .unwrap();
    let bank2 = new_bank_from_parent_with_bank_forks(
        bank_forks.as_ref(),
        bank0.clone(),
        &Pubkey::default(),
        2,
    );
    bank2
        .transfer(2 * amount, &mint_keypair, &key1.pubkey())
        .unwrap();

    let bank3 = new_bank_from_parent_with_bank_forks(
        bank_forks.as_ref(),
        bank1.clone(),
        &Pubkey::default(),
        3,
    );
    bank1.squash();

    // This picks up the values from 1 which is the highest root:
    // TODO: if we need to access rooted banks older than this,
    // need to fix the lookup.
    assert_eq!(bank0.get_balance(&key1.pubkey()), 4 * amount);
    assert_eq!(bank3.get_balance(&key1.pubkey()), 4 * amount);
    assert_eq!(bank2.get_balance(&key1.pubkey()), 3 * amount);
    bank3.squash();
    assert_eq!(bank1.get_balance(&key1.pubkey()), 4 * amount);

    let bank4 = new_bank_from_parent_with_bank_forks(
        bank_forks.as_ref(),
        bank3.clone(),
        &Pubkey::default(),
        4,
    );
    bank4
        .transfer(4 * amount, &mint_keypair, &key1.pubkey())
        .unwrap();
    assert_eq!(bank4.get_balance(&key1.pubkey()), 8 * amount);
    assert_eq!(bank3.get_balance(&key1.pubkey()), 4 * amount);
    bank4.squash();
    let bank5 = new_bank_from_parent_with_bank_forks(
        bank_forks.as_ref(),
        bank4.clone(),
        &Pubkey::default(),
        5,
    );
    bank5.squash();
    let bank6 =
        new_bank_from_parent_with_bank_forks(bank_forks.as_ref(), bank5, &Pubkey::default(), 6);
    bank6.squash();

    // This picks up the values from 4 which is the highest root:
    // TODO: if we need to access rooted banks older than this,
    // need to fix the lookup.
    assert_eq!(bank3.get_balance(&key1.pubkey()), 8 * amount);
    assert_eq!(bank2.get_balance(&key1.pubkey()), 8 * amount);

    assert_eq!(bank4.get_balance(&key1.pubkey()), 8 * amount);
}

#[test]
fn test_bank_get_account_modified_since_parent_with_fixed_root() {
    let pubkey = solana_sdk::pubkey::new_rand();

    let (genesis_config, mint_keypair) = create_genesis_config(sol_to_lamports(1.));
    let amount = genesis_config.rent.minimum_balance(0);
    let (bank1, bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);
    bank1.transfer(amount, &mint_keypair, &pubkey).unwrap();
    let result = bank1.get_account_modified_since_parent_with_fixed_root(&pubkey);
    assert!(result.is_some());
    let (account, slot) = result.unwrap();
    assert_eq!(account.lamports(), amount);
    assert_eq!(slot, 0);

    let bank2 = new_bank_from_parent_with_bank_forks(
        bank_forks.as_ref(),
        bank1.clone(),
        &Pubkey::default(),
        1,
    );
    assert!(bank2
        .get_account_modified_since_parent_with_fixed_root(&pubkey)
        .is_none());
    bank2.transfer(2 * amount, &mint_keypair, &pubkey).unwrap();
    let result = bank1.get_account_modified_since_parent_with_fixed_root(&pubkey);
    assert!(result.is_some());
    let (account, slot) = result.unwrap();
    assert_eq!(account.lamports(), amount);
    assert_eq!(slot, 0);
    let result = bank2.get_account_modified_since_parent_with_fixed_root(&pubkey);
    assert!(result.is_some());
    let (account, slot) = result.unwrap();
    assert_eq!(account.lamports(), 3 * amount);
    assert_eq!(slot, 1);

    bank1.squash();

    let bank3 =
        new_bank_from_parent_with_bank_forks(bank_forks.as_ref(), bank2, &Pubkey::default(), 3);
    assert_eq!(
        None,
        bank3.get_account_modified_since_parent_with_fixed_root(&pubkey)
    );
}

#[test]
fn test_bank_update_sysvar_account() {
    solana_logger::setup();
    // flushing the write cache is destructive, so test has to restart each time we flush and want to do 'illegal' operations once flushed
    for pass in 0..5 {
        use sysvar::clock::Clock;

        // This pubkey was chosen empirically.
        // The test requires that the dummy clock id *not* be loaded for rent collection
        // (since rent collection will update the rent epoch, thus causing the
        // subsequent checks to fail spuriously).
        let dummy_clock_id = Pubkey::from_str_const("64jsX5hwtsjsKR7eNcNU4yhgwjuXoU9KR2MpnV47iXXz");
        let dummy_rent_epoch = 44;
        let (mut genesis_config, _mint_keypair) = create_genesis_config(500);

        let expected_previous_slot = 3;
        let mut expected_next_slot = expected_previous_slot + 1;

        // First, initialize the clock sysvar
        for feature_id in FeatureSet::default().inactive {
            activate_feature(&mut genesis_config, feature_id);
        }
        let bank1 = Arc::new(Bank::new_for_tests(&genesis_config));
        if pass == 0 {
            add_root_and_flush_write_cache(&bank1);
            assert_eq!(bank1.calculate_capitalization(true), bank1.capitalization());
            continue;
        }

        assert_capitalization_diff(
            &bank1,
            || {
                bank1.update_sysvar_account(&dummy_clock_id, |optional_account| {
                    assert!(optional_account.is_none());

                    let mut account = create_account(
                        &Clock {
                            slot: expected_previous_slot,
                            ..Clock::default()
                        },
                        bank1.inherit_specially_retained_account_fields(optional_account),
                    );
                    account.set_rent_epoch(dummy_rent_epoch);
                    account
                });
                let current_account = bank1.get_account(&dummy_clock_id).unwrap();
                assert_eq!(
                    expected_previous_slot,
                    from_account::<Clock, _>(&current_account).unwrap().slot
                );
                assert_eq!(dummy_rent_epoch, current_account.rent_epoch());
            },
            |old, new| {
                assert_eq!(
                    old + min_rent_exempt_balance_for_sysvars(&bank1, &[sysvar::clock::id()]),
                    new
                );
                pass == 1
            },
        );
        if pass == 1 {
            continue;
        }

        assert_capitalization_diff(
            &bank1,
            || {
                bank1.update_sysvar_account(&dummy_clock_id, |optional_account| {
                    assert!(optional_account.is_some());

                    create_account(
                        &Clock {
                            slot: expected_previous_slot,
                            ..Clock::default()
                        },
                        bank1.inherit_specially_retained_account_fields(optional_account),
                    )
                })
            },
            |old, new| {
                // creating new sysvar twice in a slot shouldn't increment capitalization twice
                assert_eq!(old, new);
                pass == 2
            },
        );
        if pass == 2 {
            continue;
        }

        // Updating should increment the clock's slot
        let bank2 = Arc::new(Bank::new_from_parent(bank1.clone(), &Pubkey::default(), 1));
        add_root_and_flush_write_cache(&bank1);
        assert_capitalization_diff(
            &bank2,
            || {
                bank2.update_sysvar_account(&dummy_clock_id, |optional_account| {
                    let slot = from_account::<Clock, _>(optional_account.as_ref().unwrap())
                        .unwrap()
                        .slot
                        + 1;

                    create_account(
                        &Clock {
                            slot,
                            ..Clock::default()
                        },
                        bank2.inherit_specially_retained_account_fields(optional_account),
                    )
                });
                let current_account = bank2.get_account(&dummy_clock_id).unwrap();
                assert_eq!(
                    expected_next_slot,
                    from_account::<Clock, _>(&current_account).unwrap().slot
                );
                assert_eq!(dummy_rent_epoch, current_account.rent_epoch());
            },
            |old, new| {
                // if existing, capitalization shouldn't change
                assert_eq!(old, new);
                pass == 3
            },
        );
        if pass == 3 {
            continue;
        }

        // Updating again should give bank2's sysvar to the closure not bank1's.
        // Thus, increment expected_next_slot accordingly
        expected_next_slot += 1;
        assert_capitalization_diff(
            &bank2,
            || {
                bank2.update_sysvar_account(&dummy_clock_id, |optional_account| {
                    let slot = from_account::<Clock, _>(optional_account.as_ref().unwrap())
                        .unwrap()
                        .slot
                        + 1;

                    create_account(
                        &Clock {
                            slot,
                            ..Clock::default()
                        },
                        bank2.inherit_specially_retained_account_fields(optional_account),
                    )
                });
                let current_account = bank2.get_account(&dummy_clock_id).unwrap();
                assert_eq!(
                    expected_next_slot,
                    from_account::<Clock, _>(&current_account).unwrap().slot
                );
            },
            |old, new| {
                // updating twice in a slot shouldn't increment capitalization twice
                assert_eq!(old, new);
                true
            },
        );
    }
}

#[test]
fn test_bank_epoch_vote_accounts() {
    let leader_pubkey = solana_sdk::pubkey::new_rand();
    let leader_lamports = 3;
    let mut genesis_config =
        create_genesis_config_with_leader(5, &leader_pubkey, leader_lamports).genesis_config;

    // set this up weird, forces future generation, odd mod(), etc.
    //  this says: "vote_accounts for epoch X should be generated at slot index 3 in epoch X-2...
    const SLOTS_PER_EPOCH: u64 = MINIMUM_SLOTS_PER_EPOCH;
    const LEADER_SCHEDULE_SLOT_OFFSET: u64 = SLOTS_PER_EPOCH * 3 - 3;
    // no warmup allows me to do the normal division stuff below
    genesis_config.epoch_schedule =
        EpochSchedule::custom(SLOTS_PER_EPOCH, LEADER_SCHEDULE_SLOT_OFFSET, false);

    let parent = Arc::new(Bank::new_for_tests(&genesis_config));
    let mut leader_vote_stake: Vec<_> = parent
        .epoch_vote_accounts(0)
        .map(|accounts| {
            accounts
                .iter()
                .filter_map(|(pubkey, (stake, account))| {
                    if account.node_pubkey() == &leader_pubkey {
                        Some((*pubkey, *stake))
                    } else {
                        None
                    }
                })
                .collect()
        })
        .unwrap();
    assert_eq!(leader_vote_stake.len(), 1);
    let (leader_vote_account, leader_stake) = leader_vote_stake.pop().unwrap();
    assert!(leader_stake > 0);

    let leader_stake = Stake {
        delegation: Delegation {
            stake: leader_lamports,
            activation_epoch: u64::MAX, // bootstrap
            ..Delegation::default()
        },
        ..Stake::default()
    };

    let mut epoch = 1;
    loop {
        if epoch > LEADER_SCHEDULE_SLOT_OFFSET / SLOTS_PER_EPOCH {
            break;
        }
        let vote_accounts = parent.epoch_vote_accounts(epoch);
        assert!(vote_accounts.is_some());

        // epoch_stakes are a snapshot at the leader_schedule_slot_offset boundary
        //   in the prior epoch (0 in this case)
        assert_eq!(
            leader_stake.stake(0, &StakeHistory::default(), None),
            vote_accounts.unwrap().get(&leader_vote_account).unwrap().0
        );

        epoch += 1;
    }

    // child crosses epoch boundary and is the first slot in the epoch
    let child = Bank::new_from_parent(
        parent.clone(),
        &leader_pubkey,
        SLOTS_PER_EPOCH - (LEADER_SCHEDULE_SLOT_OFFSET % SLOTS_PER_EPOCH),
    );

    assert!(child.epoch_vote_accounts(epoch).is_some());
    assert_eq!(
        leader_stake.stake(child.epoch(), &StakeHistory::default(), None),
        child
            .epoch_vote_accounts(epoch)
            .unwrap()
            .get(&leader_vote_account)
            .unwrap()
            .0
    );

    // child crosses epoch boundary but isn't the first slot in the epoch, still
    //  makes an epoch stakes snapshot at 1
    let child = Bank::new_from_parent(
        parent,
        &leader_pubkey,
        SLOTS_PER_EPOCH - (LEADER_SCHEDULE_SLOT_OFFSET % SLOTS_PER_EPOCH) + 1,
    );
    assert!(child.epoch_vote_accounts(epoch).is_some());
    assert_eq!(
        leader_stake.stake(child.epoch(), &StakeHistory::default(), None),
        child
            .epoch_vote_accounts(epoch)
            .unwrap()
            .get(&leader_vote_account)
            .unwrap()
            .0
    );
}

#[test]
fn test_zero_signatures() {
    solana_logger::setup();
    let (genesis_config, mint_keypair) = create_genesis_config(500);
    let mut bank = Bank::new_for_tests(&genesis_config);
    bank.fee_rate_governor.lamports_per_signature = 2;
    let key = solana_sdk::pubkey::new_rand();

    let mut transfer_instruction = system_instruction::transfer(&mint_keypair.pubkey(), &key, 0);
    transfer_instruction.accounts[0].is_signer = false;
    let message = Message::new(&[transfer_instruction], None);
    let tx = Transaction::new(&Vec::<&Keypair>::new(), message, bank.last_blockhash());

    assert_eq!(
        bank.process_transaction(&tx),
        Err(TransactionError::SanitizeFailure)
    );
    assert_eq!(bank.get_balance(&key), 0);
}

#[test]
fn test_bank_get_slots_in_epoch() {
    let (genesis_config, _) = create_genesis_config(500);

    let bank = Bank::new_for_tests(&genesis_config);

    assert_eq!(bank.get_slots_in_epoch(0), MINIMUM_SLOTS_PER_EPOCH);
    assert_eq!(bank.get_slots_in_epoch(2), (MINIMUM_SLOTS_PER_EPOCH * 4));
    assert_eq!(
        bank.get_slots_in_epoch(5000),
        genesis_config.epoch_schedule.slots_per_epoch
    );
}

#[test]
fn test_is_delta_true() {
    let (genesis_config, mint_keypair) = create_genesis_config(sol_to_lamports(1.0));
    let (bank, _bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);
    let key1 = Keypair::new();
    let tx_transfer_mint_to_1 = system_transaction::transfer(
        &mint_keypair,
        &key1.pubkey(),
        genesis_config.rent.minimum_balance(0),
        genesis_config.hash(),
    );
    assert_eq!(bank.process_transaction(&tx_transfer_mint_to_1), Ok(()));
    assert!(bank.is_delta.load(Relaxed));

    let bank1 = new_from_parent(bank.clone());
    let hash1 = bank1.hash_internal_state();
    assert!(!bank1.is_delta.load(Relaxed));
    assert_ne!(hash1, bank.hash());
    // ticks don't make a bank into a delta or change its state unless a block boundary is crossed
    bank1.register_default_tick_for_test();
    assert!(!bank1.is_delta.load(Relaxed));
    assert_eq!(bank1.hash_internal_state(), hash1);
}

#[test]
fn test_is_empty() {
    let (genesis_config, mint_keypair) = create_genesis_config(sol_to_lamports(1.0));
    let (bank0, _bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);
    let key1 = Keypair::new();

    // The zeroth bank is empty becasue there are no transactions
    assert!(bank0.is_empty());

    // Set is_delta to true, bank is no longer empty
    let tx_transfer_mint_to_1 = system_transaction::transfer(
        &mint_keypair,
        &key1.pubkey(),
        genesis_config.rent.minimum_balance(0),
        genesis_config.hash(),
    );
    assert_eq!(bank0.process_transaction(&tx_transfer_mint_to_1), Ok(()));
    assert!(!bank0.is_empty());
}

#[test]
fn test_bank_inherit_tx_count() {
    let (genesis_config, mint_keypair) = create_genesis_config(sol_to_lamports(1.0));
    let (bank0, bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);

    // Bank 1
    let bank1 = new_bank_from_parent_with_bank_forks(
        bank_forks.as_ref(),
        bank0.clone(),
        &solana_sdk::pubkey::new_rand(),
        1,
    );
    // Bank 2
    let bank2 = new_bank_from_parent_with_bank_forks(
        bank_forks.as_ref(),
        bank0.clone(),
        &solana_sdk::pubkey::new_rand(),
        2,
    );

    // transfer a token
    assert_eq!(
        bank1.process_transaction(&system_transaction::transfer(
            &mint_keypair,
            &Keypair::new().pubkey(),
            genesis_config.rent.minimum_balance(0),
            genesis_config.hash(),
        )),
        Ok(())
    );

    assert_eq!(bank0.transaction_count(), 0);
    assert_eq!(bank0.non_vote_transaction_count_since_restart(), 0);
    assert_eq!(bank2.transaction_count(), 0);
    assert_eq!(bank2.non_vote_transaction_count_since_restart(), 0);
    assert_eq!(bank1.transaction_count(), 1);
    assert_eq!(bank1.non_vote_transaction_count_since_restart(), 1);

    bank1.squash();

    assert_eq!(bank0.transaction_count(), 0);
    assert_eq!(bank0.non_vote_transaction_count_since_restart(), 0);
    assert_eq!(bank2.transaction_count(), 0);
    assert_eq!(bank2.non_vote_transaction_count_since_restart(), 0);
    assert_eq!(bank1.transaction_count(), 1);
    assert_eq!(bank1.non_vote_transaction_count_since_restart(), 1);

    let bank6 = new_bank_from_parent_with_bank_forks(
        bank_forks.as_ref(),
        bank1.clone(),
        &solana_sdk::pubkey::new_rand(),
        3,
    );
    assert_eq!(bank1.transaction_count(), 1);
    assert_eq!(bank1.non_vote_transaction_count_since_restart(), 1);
    assert_eq!(bank6.transaction_count(), 1);
    assert_eq!(bank6.non_vote_transaction_count_since_restart(), 1);

    bank6.squash();
    assert_eq!(bank6.transaction_count(), 1);
    assert_eq!(bank6.non_vote_transaction_count_since_restart(), 1);
}

#[test]
fn test_bank_inherit_fee_rate_governor() {
    let (mut genesis_config, _mint_keypair) = create_genesis_config(500);
    genesis_config
        .fee_rate_governor
        .target_lamports_per_signature = 123;

    let bank0 = Arc::new(Bank::new_for_tests(&genesis_config));
    let bank1 = Arc::new(new_from_parent(bank0.clone()));
    assert_eq!(
        bank0.fee_rate_governor.target_lamports_per_signature / 2,
        bank1
            .fee_rate_governor
            .create_fee_calculator()
            .lamports_per_signature
    );
}

#[test]
fn test_bank_vote_accounts() {
    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config_with_leader(500, &solana_sdk::pubkey::new_rand(), 1);
    let (bank, _bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);

    let vote_accounts = bank.vote_accounts();
    assert_eq!(vote_accounts.len(), 1); // bootstrap validator has
                                        // to have a vote account

    let vote_keypair = Keypair::new();
    let instructions = vote_instruction::create_account_with_config(
        &mint_keypair.pubkey(),
        &vote_keypair.pubkey(),
        &VoteInit {
            node_pubkey: mint_keypair.pubkey(),
            authorized_voter: vote_keypair.pubkey(),
            authorized_withdrawer: vote_keypair.pubkey(),
            commission: 0,
        },
        10,
        vote_instruction::CreateVoteAccountConfig {
            space: VoteStateVersions::vote_state_size_of(true) as u64,
            ..vote_instruction::CreateVoteAccountConfig::default()
        },
    );

    let message = Message::new(&instructions, Some(&mint_keypair.pubkey()));
    let transaction = Transaction::new(
        &[&mint_keypair, &vote_keypair],
        message,
        bank.last_blockhash(),
    );

    bank.process_transaction(&transaction).unwrap();

    let vote_accounts = bank.vote_accounts();

    assert_eq!(vote_accounts.len(), 2);

    assert!(vote_accounts.get(&vote_keypair.pubkey()).is_some());

    assert!(bank.withdraw(&vote_keypair.pubkey(), 10).is_ok());

    let vote_accounts = bank.vote_accounts();

    assert_eq!(vote_accounts.len(), 1);
}

#[test]
fn test_bank_cloned_stake_delegations() {
    let GenesisConfigInfo {
        mut genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config_with_leader(
        123_456_000_000_000,
        &solana_sdk::pubkey::new_rand(),
        123_000_000_000,
    );
    genesis_config.rent = Rent::default();
    let (bank, _bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);

    let stake_delegations = bank.stakes_cache.stakes().stake_delegations().clone();
    assert_eq!(stake_delegations.len(), 1); // bootstrap validator has
                                            // to have a stake delegation

    let (vote_balance, stake_balance) = {
        let rent = &bank.rent_collector().rent;
        let vote_rent_exempt_reserve = rent.minimum_balance(VoteState::size_of());
        let stake_rent_exempt_reserve = rent.minimum_balance(StakeStateV2::size_of());
        let minimum_delegation = solana_stake_program::get_minimum_delegation(&bank.feature_set);
        (
            vote_rent_exempt_reserve,
            stake_rent_exempt_reserve + minimum_delegation,
        )
    };

    let vote_keypair = Keypair::new();
    let mut instructions = vote_instruction::create_account_with_config(
        &mint_keypair.pubkey(),
        &vote_keypair.pubkey(),
        &VoteInit {
            node_pubkey: mint_keypair.pubkey(),
            authorized_voter: vote_keypair.pubkey(),
            authorized_withdrawer: vote_keypair.pubkey(),
            commission: 0,
        },
        vote_balance,
        vote_instruction::CreateVoteAccountConfig {
            space: VoteStateVersions::vote_state_size_of(true) as u64,
            ..vote_instruction::CreateVoteAccountConfig::default()
        },
    );

    let stake_keypair = Keypair::new();
    instructions.extend(stake_instruction::create_account_and_delegate_stake(
        &mint_keypair.pubkey(),
        &stake_keypair.pubkey(),
        &vote_keypair.pubkey(),
        &Authorized::auto(&stake_keypair.pubkey()),
        &Lockup::default(),
        stake_balance,
    ));

    let message = Message::new(&instructions, Some(&mint_keypair.pubkey()));
    let transaction = Transaction::new(
        &[&mint_keypair, &vote_keypair, &stake_keypair],
        message,
        bank.last_blockhash(),
    );

    bank.process_transaction(&transaction).unwrap();

    let stake_delegations = bank.stakes_cache.stakes().stake_delegations().clone();
    assert_eq!(stake_delegations.len(), 2);
    assert!(stake_delegations.get(&stake_keypair.pubkey()).is_some());
}

#[test]
fn test_is_delta_with_no_committables() {
    let (genesis_config, mint_keypair) = create_genesis_config(8000);
    let (bank, _bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);
    bank.is_delta.store(false, Relaxed);

    let keypair1 = Keypair::new();
    let keypair2 = Keypair::new();
    let fail_tx =
        system_transaction::transfer(&keypair1, &keypair2.pubkey(), 1, bank.last_blockhash());

    // Should fail with TransactionError::AccountNotFound, which means
    // the account which this tx operated on will not be committed. Thus
    // the bank is_delta should still be false
    assert_eq!(
        bank.process_transaction(&fail_tx),
        Err(TransactionError::AccountNotFound)
    );

    // Check the bank is_delta is still false
    assert!(!bank.is_delta.load(Relaxed));

    // Should fail with InstructionError, but InstructionErrors are committable,
    // so is_delta should be true
    assert_eq!(
        bank.transfer(10_001, &mint_keypair, &solana_sdk::pubkey::new_rand()),
        Err(TransactionError::InstructionError(
            0,
            SystemError::ResultWithNegativeLamports.into(),
        ))
    );

    assert!(bank.is_delta.load(Relaxed));
}

#[test]
fn test_bank_get_program_accounts() {
    let (genesis_config, mint_keypair) = create_genesis_config(500);
    let parent = Arc::new(Bank::new_for_tests(&genesis_config));
    parent.restore_old_behavior_for_fragile_tests();

    let genesis_accounts: Vec<_> = parent.get_all_accounts(false).unwrap();
    assert!(
        genesis_accounts
            .iter()
            .any(|(pubkey, _, _)| *pubkey == mint_keypair.pubkey()),
        "mint pubkey not found"
    );
    assert!(
        genesis_accounts
            .iter()
            .any(|(_, account, _)| solana_sdk::sysvar::check_id(account.owner())),
        "no sysvars found"
    );

    let bank0 = Arc::new(new_from_parent(parent));
    let pubkey0 = solana_sdk::pubkey::new_rand();
    let program_id = Pubkey::from([2; 32]);
    let account0 = AccountSharedData::new(1, 0, &program_id);
    bank0.store_account(&pubkey0, &account0);

    assert_eq!(
        bank0.get_program_accounts_modified_since_parent(&program_id),
        vec![(pubkey0, account0.clone())]
    );

    let bank1 = Arc::new(new_from_parent(bank0.clone()));
    bank1.squash();
    assert_eq!(
        bank0
            .get_program_accounts(&program_id, &ScanConfig::default(),)
            .unwrap(),
        vec![(pubkey0, account0.clone())]
    );
    assert_eq!(
        bank1
            .get_program_accounts(&program_id, &ScanConfig::default(),)
            .unwrap(),
        vec![(pubkey0, account0)]
    );
    assert_eq!(
        bank1.get_program_accounts_modified_since_parent(&program_id),
        vec![]
    );

    let bank2 = Arc::new(new_from_parent(bank1.clone()));
    let pubkey1 = solana_sdk::pubkey::new_rand();
    let account1 = AccountSharedData::new(3, 0, &program_id);
    bank2.store_account(&pubkey1, &account1);
    // Accounts with 0 lamports should be filtered out by Accounts::load_by_program()
    let pubkey2 = solana_sdk::pubkey::new_rand();
    let account2 = AccountSharedData::new(0, 0, &program_id);
    bank2.store_account(&pubkey2, &account2);

    let bank3 = Arc::new(new_from_parent(bank2));
    bank3.squash();
    assert_eq!(
        bank1
            .get_program_accounts(&program_id, &ScanConfig::default(),)
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        bank3
            .get_program_accounts(&program_id, &ScanConfig::default(),)
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn test_get_filtered_indexed_accounts_limit_exceeded() {
    let (genesis_config, _mint_keypair) = create_genesis_config(500);
    let mut account_indexes = AccountSecondaryIndexes::default();
    account_indexes.indexes.insert(AccountIndex::ProgramId);
    let bank_config = BankTestConfig {
        accounts_db_config: AccountsDbConfig {
            account_indexes: Some(account_indexes),
            ..ACCOUNTS_DB_CONFIG_FOR_TESTING
        },
    };
    let bank = Arc::new(Bank::new_with_config_for_tests(
        &genesis_config,
        bank_config,
    ));

    let address = Pubkey::new_unique();
    let program_id = Pubkey::new_unique();
    let limit = 100;
    let account = AccountSharedData::new(1, limit, &program_id);
    bank.store_account(&address, &account);

    assert!(bank
        .get_filtered_indexed_accounts(
            &IndexKey::ProgramId(program_id),
            |_| true,
            &ScanConfig::default(),
            Some(limit), // limit here will be exceeded, resulting in aborted scan
        )
        .is_err());
}

#[test]
fn test_get_filtered_indexed_accounts() {
    let (genesis_config, _mint_keypair) = create_genesis_config(500);
    let mut account_indexes = AccountSecondaryIndexes::default();
    account_indexes.indexes.insert(AccountIndex::ProgramId);
    let bank_config = BankTestConfig {
        accounts_db_config: AccountsDbConfig {
            account_indexes: Some(account_indexes),
            ..ACCOUNTS_DB_CONFIG_FOR_TESTING
        },
    };
    let bank = Arc::new(Bank::new_with_config_for_tests(
        &genesis_config,
        bank_config,
    ));

    let address = Pubkey::new_unique();
    let program_id = Pubkey::new_unique();
    let account = AccountSharedData::new(1, 0, &program_id);
    bank.store_account(&address, &account);

    let indexed_accounts = bank
        .get_filtered_indexed_accounts(
            &IndexKey::ProgramId(program_id),
            |_| true,
            &ScanConfig::default(),
            None,
        )
        .unwrap();
    assert_eq!(indexed_accounts.len(), 1);
    assert_eq!(indexed_accounts[0], (address, account));

    // Even though the account is re-stored in the bank (and the index) under a new program id,
    // it is still present in the index under the original program id as well. This
    // demonstrates the need for a redundant post-processing filter.
    let another_program_id = Pubkey::new_unique();
    let new_account = AccountSharedData::new(1, 0, &another_program_id);
    let bank = Arc::new(new_from_parent(bank));
    bank.store_account(&address, &new_account);
    let indexed_accounts = bank
        .get_filtered_indexed_accounts(
            &IndexKey::ProgramId(program_id),
            |_| true,
            &ScanConfig::default(),
            None,
        )
        .unwrap();
    assert_eq!(indexed_accounts.len(), 1);
    assert_eq!(indexed_accounts[0], (address, new_account.clone()));
    let indexed_accounts = bank
        .get_filtered_indexed_accounts(
            &IndexKey::ProgramId(another_program_id),
            |_| true,
            &ScanConfig::default(),
            None,
        )
        .unwrap();
    assert_eq!(indexed_accounts.len(), 1);
    assert_eq!(indexed_accounts[0], (address, new_account.clone()));

    // Post-processing filter
    let indexed_accounts = bank
        .get_filtered_indexed_accounts(
            &IndexKey::ProgramId(program_id),
            |account| account.owner() == &program_id,
            &ScanConfig::default(),
            None,
        )
        .unwrap();
    assert!(indexed_accounts.is_empty());
    let indexed_accounts = bank
        .get_filtered_indexed_accounts(
            &IndexKey::ProgramId(another_program_id),
            |account| account.owner() == &another_program_id,
            &ScanConfig::default(),
            None,
        )
        .unwrap();
    assert_eq!(indexed_accounts.len(), 1);
    assert_eq!(indexed_accounts[0], (address, new_account));
}

#[test]
fn test_status_cache_ancestors() {
    solana_logger::setup();
    let (parent, _bank_forks) = create_simple_test_arc_bank(500);
    let bank1 = Arc::new(new_from_parent(parent));
    let mut bank = bank1;
    for _ in 0..MAX_CACHE_ENTRIES * 2 {
        bank = Arc::new(new_from_parent(bank));
        bank.squash();
    }

    let bank = new_from_parent(bank);
    assert_eq!(
        bank.status_cache_ancestors(),
        (bank.slot() - MAX_CACHE_ENTRIES as u64..=bank.slot()).collect::<Vec<_>>()
    );
}

#[test]
fn test_add_builtin() {
    let (genesis_config, mint_keypair) = create_genesis_config_no_tx_fee_no_rent(500);
    let mut bank = Bank::new_for_tests(&genesis_config);

    fn mock_vote_program_id() -> Pubkey {
        Pubkey::from([42u8; 32])
    }
    declare_process_instruction!(MockBuiltin, 1, |invoke_context| {
        let transaction_context = &invoke_context.transaction_context;
        let instruction_context = transaction_context.get_current_instruction_context()?;
        let program_id = instruction_context.get_last_program_key(transaction_context)?;
        if mock_vote_program_id() != *program_id {
            return Err(InstructionError::IncorrectProgramId);
        }
        Err(InstructionError::Custom(42))
    });

    assert!(bank.get_account(&mock_vote_program_id()).is_none());
    bank.add_mockup_builtin(mock_vote_program_id(), MockBuiltin::vm);
    assert!(bank.get_account(&mock_vote_program_id()).is_some());

    let mock_account = Keypair::new();
    let mock_validator_identity = Keypair::new();
    let mut instructions = vote_instruction::create_account_with_config(
        &mint_keypair.pubkey(),
        &mock_account.pubkey(),
        &VoteInit {
            node_pubkey: mock_validator_identity.pubkey(),
            ..VoteInit::default()
        },
        1,
        vote_instruction::CreateVoteAccountConfig {
            space: VoteStateVersions::vote_state_size_of(true) as u64,
            ..vote_instruction::CreateVoteAccountConfig::default()
        },
    );
    instructions[1].program_id = mock_vote_program_id();

    let message = Message::new(&instructions, Some(&mint_keypair.pubkey()));
    let transaction = Transaction::new(
        &[&mint_keypair, &mock_account, &mock_validator_identity],
        message,
        bank.last_blockhash(),
    );

    let (bank, _bank_forks) = bank.wrap_with_bank_forks_for_tests();
    assert_eq!(
        bank.process_transaction(&transaction),
        Err(TransactionError::InstructionError(
            1,
            InstructionError::Custom(42)
        ))
    );
}

#[test]
fn test_add_duplicate_static_program() {
    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config_with_leader(500, &solana_sdk::pubkey::new_rand(), 0);
    let (bank, bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);

    declare_process_instruction!(MockBuiltin, 1, |_invoke_context| {
        Err(InstructionError::Custom(42))
    });

    let mock_account = Keypair::new();
    let mock_validator_identity = Keypair::new();
    let instructions = vote_instruction::create_account_with_config(
        &mint_keypair.pubkey(),
        &mock_account.pubkey(),
        &VoteInit {
            node_pubkey: mock_validator_identity.pubkey(),
            ..VoteInit::default()
        },
        1,
        vote_instruction::CreateVoteAccountConfig {
            space: VoteStateVersions::vote_state_size_of(true) as u64,
            ..vote_instruction::CreateVoteAccountConfig::default()
        },
    );

    let message = Message::new(&instructions, Some(&mint_keypair.pubkey()));
    let transaction = Transaction::new(
        &[&mint_keypair, &mock_account, &mock_validator_identity],
        message,
        bank.last_blockhash(),
    );

    let slot = bank.slot().saturating_add(1);
    let mut bank = Bank::new_from_parent(bank, &Pubkey::default(), slot);
    bank.add_mockup_builtin(solana_vote_program::id(), MockBuiltin::vm);
    let bank = bank_forks
        .write()
        .unwrap()
        .insert(bank)
        .clone_without_scheduler();

    let vote_loader_account = bank.get_account(&solana_vote_program::id()).unwrap();
    let new_vote_loader_account = bank.get_account(&solana_vote_program::id()).unwrap();
    // Vote loader account should not be updated since it was included in the genesis config.
    assert_eq!(vote_loader_account.data(), new_vote_loader_account.data());
    assert_eq!(
        bank.process_transaction(&transaction),
        Err(TransactionError::InstructionError(
            1,
            InstructionError::Custom(42)
        ))
    );
}

#[test]
fn test_add_instruction_processor_for_existing_unrelated_accounts() {
    for pass in 0..5 {
        let mut bank = create_simple_test_bank(500);

        declare_process_instruction!(MockBuiltin, 1, |_invoke_context| {
            Err(InstructionError::Custom(42))
        });

        // Non-builtin loader accounts can not be used for instruction processing
        {
            let stakes = bank.stakes_cache.stakes();
            assert!(stakes.vote_accounts().as_ref().is_empty());
        }
        assert!(bank.stakes_cache.stakes().stake_delegations().is_empty());
        if pass == 0 {
            add_root_and_flush_write_cache(&bank);
            assert_eq!(bank.calculate_capitalization(true), bank.capitalization());
            continue;
        }

        let ((vote_id, vote_account), (stake_id, stake_account)) =
            crate::stakes::tests::create_staked_node_accounts(1_0000);
        bank.capitalization
            .fetch_add(vote_account.lamports() + stake_account.lamports(), Relaxed);
        bank.store_account(&vote_id, &vote_account);
        bank.store_account(&stake_id, &stake_account);
        {
            let stakes = bank.stakes_cache.stakes();
            assert!(!stakes.vote_accounts().as_ref().is_empty());
        }
        assert!(!bank.stakes_cache.stakes().stake_delegations().is_empty());
        if pass == 1 {
            add_root_and_flush_write_cache(&bank);
            assert_eq!(bank.calculate_capitalization(true), bank.capitalization());
            continue;
        }

        bank.transaction_processor.add_builtin(
            &bank,
            vote_id,
            "mock_program1",
            ProgramCacheEntry::new_builtin(0, 0, MockBuiltin::vm),
        );
        bank.transaction_processor.add_builtin(
            &bank,
            stake_id,
            "mock_program2",
            ProgramCacheEntry::new_builtin(0, 0, MockBuiltin::vm),
        );
        {
            let stakes = bank.stakes_cache.stakes();
            assert!(stakes.vote_accounts().as_ref().is_empty());
        }
        assert!(bank.stakes_cache.stakes().stake_delegations().is_empty());
        if pass == 2 {
            add_root_and_flush_write_cache(&bank);
            assert_eq!(bank.calculate_capitalization(true), bank.capitalization());
            continue;
        }
        assert_eq!(
            "mock_program1",
            String::from_utf8_lossy(bank.get_account(&vote_id).unwrap_or_default().data())
        );
        assert_eq!(
            "mock_program2",
            String::from_utf8_lossy(bank.get_account(&stake_id).unwrap_or_default().data())
        );

        // Re-adding builtin programs should be no-op
        bank.update_accounts_hash_for_tests();
        let old_hash = bank.get_accounts_hash().unwrap();
        bank.add_mockup_builtin(vote_id, MockBuiltin::vm);
        bank.add_mockup_builtin(stake_id, MockBuiltin::vm);
        add_root_and_flush_write_cache(&bank);
        bank.update_accounts_hash_for_tests();
        let new_hash = bank.get_accounts_hash().unwrap();
        assert_eq!(old_hash, new_hash);
        {
            let stakes = bank.stakes_cache.stakes();
            assert!(stakes.vote_accounts().as_ref().is_empty());
        }
        assert!(bank.stakes_cache.stakes().stake_delegations().is_empty());
        assert_eq!(bank.calculate_capitalization(true), bank.capitalization());
        assert_eq!(
            "mock_program1",
            String::from_utf8_lossy(bank.get_account(&vote_id).unwrap_or_default().data())
        );
        assert_eq!(
            "mock_program2",
            String::from_utf8_lossy(bank.get_account(&stake_id).unwrap_or_default().data())
        );
    }
}

#[allow(deprecated)]
#[test]
fn test_recent_blockhashes_sysvar() {
    let (mut bank, _bank_forks) = create_simple_test_arc_bank(500);
    for i in 1..5 {
        let bhq_account = bank.get_account(&sysvar::recent_blockhashes::id()).unwrap();
        let recent_blockhashes =
            from_account::<sysvar::recent_blockhashes::RecentBlockhashes, _>(&bhq_account).unwrap();
        // Check length
        assert_eq!(recent_blockhashes.len(), i);
        let most_recent_hash = recent_blockhashes.iter().next().unwrap().blockhash;
        // Check order
        assert!(bank.is_hash_valid_for_age(&most_recent_hash, 0));
        goto_end_of_slot(bank.clone());
        bank = Arc::new(new_from_parent(bank));
    }
}

#[allow(deprecated)]
#[test]
fn test_blockhash_queue_sysvar_consistency() {
    let (bank, _bank_forks) = create_simple_test_arc_bank(100_000);
    goto_end_of_slot(bank.clone());

    let bhq_account = bank.get_account(&sysvar::recent_blockhashes::id()).unwrap();
    let recent_blockhashes =
        from_account::<sysvar::recent_blockhashes::RecentBlockhashes, _>(&bhq_account).unwrap();

    let sysvar_recent_blockhash = recent_blockhashes[0].blockhash;
    let bank_last_blockhash = bank.last_blockhash();
    assert_eq!(sysvar_recent_blockhash, bank_last_blockhash);
}

#[test]
fn test_hash_internal_state_unchanged() {
    let (genesis_config, _) = create_genesis_config(500);
    let bank0 = Arc::new(Bank::new_for_tests(&genesis_config));
    bank0.freeze();
    let bank0_hash = bank0.hash();
    let bank1 = Bank::new_from_parent(bank0, &Pubkey::default(), 1);
    bank1.freeze();
    let bank1_hash = bank1.hash();
    // Checkpointing should always result in a new state
    assert_ne!(bank0_hash, bank1_hash);
}

#[test]
fn test_hash_internal_state_unchanged_with_ticks() {
    let (genesis_config, _) = create_genesis_config(500);
    let bank = Arc::new(Bank::new_for_tests(&genesis_config));
    let bank1 = new_from_parent(bank);
    let hash1 = bank1.hash_internal_state();
    // ticks don't change its state even if a slot boundary is crossed
    // because blockhashes are only recorded at block boundaries
    for _ in 0..genesis_config.ticks_per_slot {
        assert_eq!(bank1.hash_internal_state(), hash1);
        bank1.register_default_tick_for_test();
    }
    assert_eq!(bank1.hash_internal_state(), hash1);
}

#[ignore]
#[test]
fn test_banks_leak() {
    fn add_lotsa_stake_accounts(genesis_config: &mut GenesisConfig) {
        const LOTSA: usize = 4_096;

        (0..LOTSA).for_each(|_| {
            let pubkey = solana_sdk::pubkey::new_rand();
            genesis_config.add_account(
                pubkey,
                stake_state::create_lockup_stake_account(
                    &Authorized::auto(&pubkey),
                    &Lockup::default(),
                    &Rent::default(),
                    50_000_000,
                ),
            );
        });
    }
    solana_logger::setup();
    let (mut genesis_config, _) = create_genesis_config(100_000_000_000_000);
    add_lotsa_stake_accounts(&mut genesis_config);
    let (mut bank, bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);
    let mut num_banks = 0;
    let pid = std::process::id();
    #[cfg(not(target_os = "linux"))]
    error!(
        "\nYou can run this to watch RAM:\n   while read -p 'banks: '; do echo $(( $(ps -o vsize= -p {})/$REPLY));done", pid
    );
    loop {
        num_banks += 1;
        bank = new_from_parent_with_fork_next_slot(bank, bank_forks.as_ref());
        if num_banks % 100 == 0 {
            #[cfg(target_os = "linux")]
            {
                let pages_consumed = std::fs::read_to_string(format!("/proc/{pid}/statm"))
                    .unwrap()
                    .split_whitespace()
                    .next()
                    .unwrap()
                    .parse::<usize>()
                    .unwrap();
                error!(
                    "at {} banks: {} mem or {}kB/bank",
                    num_banks,
                    pages_consumed * 4096,
                    (pages_consumed * 4) / num_banks
                );
            }
            #[cfg(not(target_os = "linux"))]
            {
                error!("{} banks, sleeping for 5 sec", num_banks);
                std::thread::sleep(Duration::from_secs(5));
            }
        }
    }
}

pub(in crate::bank) fn get_nonce_blockhash(bank: &Bank, nonce_pubkey: &Pubkey) -> Option<Hash> {
    let account = bank.get_account(nonce_pubkey)?;
    let nonce_data = get_nonce_data_from_account(&account)?;
    Some(nonce_data.blockhash())
}

pub(in crate::bank) fn get_nonce_data_from_account(
    account: &AccountSharedData,
) -> Option<nonce::state::Data> {
    let nonce_versions = StateMut::<nonce::state::Versions>::state(account).ok()?;
    if let nonce::State::Initialized(nonce_data) = nonce_versions.state() {
        Some(nonce_data.clone())
    } else {
        None
    }
}

fn nonce_setup(
    bank: &Arc<Bank>,
    mint_keypair: &Keypair,
    custodian_lamports: u64,
    nonce_lamports: u64,
    nonce_authority: Option<Pubkey>,
) -> Result<(Keypair, Keypair)> {
    let custodian_keypair = Keypair::new();
    let nonce_keypair = Keypair::new();
    /* Setup accounts */
    let mut setup_ixs = vec![system_instruction::transfer(
        &mint_keypair.pubkey(),
        &custodian_keypair.pubkey(),
        custodian_lamports,
    )];
    let nonce_authority = nonce_authority.unwrap_or_else(|| nonce_keypair.pubkey());
    setup_ixs.extend_from_slice(&system_instruction::create_nonce_account(
        &custodian_keypair.pubkey(),
        &nonce_keypair.pubkey(),
        &nonce_authority,
        nonce_lamports,
    ));
    let message = Message::new(&setup_ixs, Some(&mint_keypair.pubkey()));
    let setup_tx = Transaction::new(
        &[mint_keypair, &custodian_keypair, &nonce_keypair],
        message,
        bank.last_blockhash(),
    );
    bank.process_transaction(&setup_tx)?;
    Ok((custodian_keypair, nonce_keypair))
}

type NonceSetup = (Arc<Bank>, Keypair, Keypair, Keypair, Arc<RwLock<BankForks>>);

pub(in crate::bank) fn setup_nonce_with_bank<F>(
    supply_lamports: u64,
    mut genesis_cfg_fn: F,
    custodian_lamports: u64,
    nonce_lamports: u64,
    nonce_authority: Option<Pubkey>,
    feature_set: FeatureSet,
) -> Result<NonceSetup>
where
    F: FnMut(&mut GenesisConfig),
{
    let (mut genesis_config, mint_keypair) = create_genesis_config(supply_lamports);
    genesis_config.rent.lamports_per_byte_year = 0;
    genesis_cfg_fn(&mut genesis_config);
    let mut bank = Bank::new_for_tests(&genesis_config);
    bank.feature_set = Arc::new(feature_set);
    let (mut bank, bank_forks) = bank.wrap_with_bank_forks_for_tests();

    // Banks 0 and 1 have no fees, wait two blocks before
    // initializing our nonce accounts
    for _ in 0..2 {
        goto_end_of_slot(bank.clone());
        bank = new_from_parent_with_fork_next_slot(bank, bank_forks.as_ref());
    }

    let (custodian_keypair, nonce_keypair) = nonce_setup(
        &bank,
        &mint_keypair,
        custodian_lamports,
        nonce_lamports,
        nonce_authority,
    )?;

    // The setup nonce is not valid to be used until the next bank
    // so wait one more block
    goto_end_of_slot(bank.clone());
    bank = new_from_parent_with_fork_next_slot(bank, bank_forks.as_ref());

    Ok((
        bank,
        mint_keypair,
        custodian_keypair,
        nonce_keypair,
        bank_forks,
    ))
}

impl Bank {
    pub(in crate::bank) fn next_durable_nonce(&self) -> DurableNonce {
        let hash_queue = self.blockhash_queue.read().unwrap();
        let last_blockhash = hash_queue.last_hash();
        DurableNonce::from_blockhash(&last_blockhash)
    }
}

#[test]
fn test_assign_from_nonce_account_fail() {
    let (bank, _bank_forks) = create_simple_test_arc_bank(100_000_000);
    let nonce = Keypair::new();
    let nonce_account = AccountSharedData::new_data(
        42_424_242,
        &nonce::state::Versions::new(nonce::State::Initialized(nonce::state::Data::default())),
        &system_program::id(),
    )
    .unwrap();
    let blockhash = bank.last_blockhash();
    bank.store_account(&nonce.pubkey(), &nonce_account);

    let ix = system_instruction::assign(&nonce.pubkey(), &Pubkey::from([9u8; 32]));
    let message = Message::new(&[ix], Some(&nonce.pubkey()));
    let tx = Transaction::new(&[&nonce], message, blockhash);

    let expect = Err(TransactionError::InstructionError(
        0,
        InstructionError::ModifiedProgramId,
    ));
    assert_eq!(bank.process_transaction(&tx), expect);
}

#[test]
fn test_nonce_must_be_advanceable() {
    let mut bank = create_simple_test_bank(100_000_000);
    bank.feature_set = Arc::new(FeatureSet::all_enabled());
    let (bank, _bank_forks) = bank.wrap_with_bank_forks_for_tests();
    let nonce_keypair = Keypair::new();
    let nonce_authority = nonce_keypair.pubkey();
    let durable_nonce = DurableNonce::from_blockhash(&bank.last_blockhash());
    let nonce_account = AccountSharedData::new_data(
        42_424_242,
        &nonce::state::Versions::new(nonce::State::Initialized(nonce::state::Data::new(
            nonce_authority,
            durable_nonce,
            5000,
        ))),
        &system_program::id(),
    )
    .unwrap();
    bank.store_account(&nonce_keypair.pubkey(), &nonce_account);

    let ix = system_instruction::advance_nonce_account(&nonce_keypair.pubkey(), &nonce_authority);
    let message = Message::new(&[ix], Some(&nonce_keypair.pubkey()));
    let tx = Transaction::new(&[&nonce_keypair], message, *durable_nonce.as_hash());
    assert_eq!(
        bank.process_transaction(&tx),
        Err(TransactionError::BlockhashNotFound)
    );
}

#[test]
fn test_nonce_transaction() {
    let (mut bank, _mint_keypair, custodian_keypair, nonce_keypair, bank_forks) =
        setup_nonce_with_bank(
            10_000_000,
            |_| {},
            5_000_000,
            250_000,
            None,
            FeatureSet::all_enabled(),
        )
        .unwrap();
    let alice_keypair = Keypair::new();
    let alice_pubkey = alice_keypair.pubkey();
    let custodian_pubkey = custodian_keypair.pubkey();
    let nonce_pubkey = nonce_keypair.pubkey();

    assert_eq!(bank.get_balance(&custodian_pubkey), 4_750_000);
    assert_eq!(bank.get_balance(&nonce_pubkey), 250_000);

    /* Grab the hash stored in the nonce account */
    let nonce_hash = get_nonce_blockhash(&bank, &nonce_pubkey).unwrap();

    /* Kick nonce hash off the blockhash_queue */
    for _ in 0..MAX_RECENT_BLOCKHASHES + 1 {
        goto_end_of_slot(bank.clone());
        bank = new_from_parent_with_fork_next_slot(bank, bank_forks.as_ref());
    }

    /* Expect a non-Nonce transfer to fail */
    assert_eq!(
        bank.process_transaction(&system_transaction::transfer(
            &custodian_keypair,
            &alice_pubkey,
            100_000,
            nonce_hash
        ),),
        Err(TransactionError::BlockhashNotFound),
    );
    /* Check fee not charged */
    assert_eq!(bank.get_balance(&custodian_pubkey), 4_750_000);

    /* Nonce transfer */
    let nonce_tx = Transaction::new_signed_with_payer(
        &[
            system_instruction::advance_nonce_account(&nonce_pubkey, &nonce_pubkey),
            system_instruction::transfer(&custodian_pubkey, &alice_pubkey, 100_000),
        ],
        Some(&custodian_pubkey),
        &[&custodian_keypair, &nonce_keypair],
        nonce_hash,
    );
    assert_eq!(bank.process_transaction(&nonce_tx), Ok(()));

    /* Check balances */
    let mut recent_message = nonce_tx.message;
    recent_message.recent_blockhash = bank.last_blockhash();
    let mut expected_balance = 4_650_000
        - bank
            .get_fee_for_message(&new_sanitized_message(recent_message))
            .unwrap();
    assert_eq!(bank.get_balance(&custodian_pubkey), expected_balance);
    assert_eq!(bank.get_balance(&nonce_pubkey), 250_000);
    assert_eq!(bank.get_balance(&alice_pubkey), 100_000);

    /* Confirm stored nonce has advanced */
    let new_nonce = get_nonce_blockhash(&bank, &nonce_pubkey).unwrap();
    assert_ne!(nonce_hash, new_nonce);

    /* Nonce re-use fails */
    let nonce_tx = Transaction::new_signed_with_payer(
        &[
            system_instruction::advance_nonce_account(&nonce_pubkey, &nonce_pubkey),
            system_instruction::transfer(&custodian_pubkey, &alice_pubkey, 100_000),
        ],
        Some(&custodian_pubkey),
        &[&custodian_keypair, &nonce_keypair],
        nonce_hash,
    );
    assert_eq!(
        bank.process_transaction(&nonce_tx),
        Err(TransactionError::BlockhashNotFound)
    );
    /* Check fee not charged and nonce not advanced */
    assert_eq!(bank.get_balance(&custodian_pubkey), expected_balance);
    assert_eq!(
        new_nonce,
        get_nonce_blockhash(&bank, &nonce_pubkey).unwrap()
    );

    let nonce_hash = new_nonce;

    /* Kick nonce hash off the blockhash_queue */
    for _ in 0..MAX_RECENT_BLOCKHASHES + 1 {
        goto_end_of_slot(bank.clone());
        bank = new_from_parent_with_fork_next_slot(bank, bank_forks.as_ref());
    }

    let nonce_tx = Transaction::new_signed_with_payer(
        &[
            system_instruction::advance_nonce_account(&nonce_pubkey, &nonce_pubkey),
            system_instruction::transfer(&custodian_pubkey, &alice_pubkey, 100_000_000),
        ],
        Some(&custodian_pubkey),
        &[&custodian_keypair, &nonce_keypair],
        nonce_hash,
    );
    assert_eq!(
        bank.process_transaction(&nonce_tx),
        Err(TransactionError::InstructionError(
            1,
            system_instruction::SystemError::ResultWithNegativeLamports.into(),
        ))
    );
    /* Check fee charged and nonce has advanced */
    let mut recent_message = nonce_tx.message.clone();
    recent_message.recent_blockhash = bank.last_blockhash();
    expected_balance -= bank
        .get_fee_for_message(&new_sanitized_message(recent_message))
        .unwrap();
    assert_eq!(bank.get_balance(&custodian_pubkey), expected_balance);
    assert_ne!(
        nonce_hash,
        get_nonce_blockhash(&bank, &nonce_pubkey).unwrap()
    );
    /* Confirm replaying a TX that failed with InstructionError::* now
     * fails with TransactionError::BlockhashNotFound
     */
    assert_eq!(
        bank.process_transaction(&nonce_tx),
        Err(TransactionError::BlockhashNotFound),
    );
}

#[test]
fn test_nonce_transaction_with_tx_wide_caps() {
    let feature_set = FeatureSet::all_enabled();
    let (mut bank, _mint_keypair, custodian_keypair, nonce_keypair, bank_forks) =
        setup_nonce_with_bank(10_000_000, |_| {}, 5_000_000, 250_000, None, feature_set).unwrap();
    let alice_keypair = Keypair::new();
    let alice_pubkey = alice_keypair.pubkey();
    let custodian_pubkey = custodian_keypair.pubkey();
    let nonce_pubkey = nonce_keypair.pubkey();

    assert_eq!(bank.get_balance(&custodian_pubkey), 4_750_000);
    assert_eq!(bank.get_balance(&nonce_pubkey), 250_000);

    /* Grab the hash stored in the nonce account */
    let nonce_hash = get_nonce_blockhash(&bank, &nonce_pubkey).unwrap();

    /* Kick nonce hash off the blockhash_queue */
    for _ in 0..MAX_RECENT_BLOCKHASHES + 1 {
        goto_end_of_slot(bank.clone());
        bank = new_from_parent_with_fork_next_slot(bank, bank_forks.as_ref());
    }

    /* Expect a non-Nonce transfer to fail */
    assert_eq!(
        bank.process_transaction(&system_transaction::transfer(
            &custodian_keypair,
            &alice_pubkey,
            100_000,
            nonce_hash
        ),),
        Err(TransactionError::BlockhashNotFound),
    );
    /* Check fee not charged */
    assert_eq!(bank.get_balance(&custodian_pubkey), 4_750_000);

    /* Nonce transfer */
    let nonce_tx = Transaction::new_signed_with_payer(
        &[
            system_instruction::advance_nonce_account(&nonce_pubkey, &nonce_pubkey),
            system_instruction::transfer(&custodian_pubkey, &alice_pubkey, 100_000),
        ],
        Some(&custodian_pubkey),
        &[&custodian_keypair, &nonce_keypair],
        nonce_hash,
    );
    assert_eq!(bank.process_transaction(&nonce_tx), Ok(()));

    /* Check balances */
    let mut recent_message = nonce_tx.message;
    recent_message.recent_blockhash = bank.last_blockhash();
    let mut expected_balance = 4_650_000
        - bank
            .get_fee_for_message(&new_sanitized_message(recent_message))
            .unwrap();
    assert_eq!(bank.get_balance(&custodian_pubkey), expected_balance);
    assert_eq!(bank.get_balance(&nonce_pubkey), 250_000);
    assert_eq!(bank.get_balance(&alice_pubkey), 100_000);

    /* Confirm stored nonce has advanced */
    let new_nonce = get_nonce_blockhash(&bank, &nonce_pubkey).unwrap();
    assert_ne!(nonce_hash, new_nonce);

    /* Nonce re-use fails */
    let nonce_tx = Transaction::new_signed_with_payer(
        &[
            system_instruction::advance_nonce_account(&nonce_pubkey, &nonce_pubkey),
            system_instruction::transfer(&custodian_pubkey, &alice_pubkey, 100_000),
        ],
        Some(&custodian_pubkey),
        &[&custodian_keypair, &nonce_keypair],
        nonce_hash,
    );
    assert_eq!(
        bank.process_transaction(&nonce_tx),
        Err(TransactionError::BlockhashNotFound)
    );
    /* Check fee not charged and nonce not advanced */
    assert_eq!(bank.get_balance(&custodian_pubkey), expected_balance);
    assert_eq!(
        new_nonce,
        get_nonce_blockhash(&bank, &nonce_pubkey).unwrap()
    );

    let nonce_hash = new_nonce;

    /* Kick nonce hash off the blockhash_queue */
    for _ in 0..MAX_RECENT_BLOCKHASHES + 1 {
        goto_end_of_slot(bank.clone());
        bank = new_from_parent_with_fork_next_slot(bank, bank_forks.as_ref());
    }

    let nonce_tx = Transaction::new_signed_with_payer(
        &[
            system_instruction::advance_nonce_account(&nonce_pubkey, &nonce_pubkey),
            system_instruction::transfer(&custodian_pubkey, &alice_pubkey, 100_000_000),
        ],
        Some(&custodian_pubkey),
        &[&custodian_keypair, &nonce_keypair],
        nonce_hash,
    );
    assert_eq!(
        bank.process_transaction(&nonce_tx),
        Err(TransactionError::InstructionError(
            1,
            system_instruction::SystemError::ResultWithNegativeLamports.into(),
        ))
    );
    /* Check fee charged and nonce has advanced */
    let mut recent_message = nonce_tx.message.clone();
    recent_message.recent_blockhash = bank.last_blockhash();
    expected_balance -= bank
        .get_fee_for_message(&new_sanitized_message(recent_message))
        .unwrap();
    assert_eq!(bank.get_balance(&custodian_pubkey), expected_balance);
    assert_ne!(
        nonce_hash,
        get_nonce_blockhash(&bank, &nonce_pubkey).unwrap()
    );
    /* Confirm replaying a TX that failed with InstructionError::* now
     * fails with TransactionError::BlockhashNotFound
     */
    assert_eq!(
        bank.process_transaction(&nonce_tx),
        Err(TransactionError::BlockhashNotFound),
    );
}

#[test]
fn test_nonce_authority() {
    solana_logger::setup();
    let (mut bank, _mint_keypair, custodian_keypair, nonce_keypair, bank_forks) =
        setup_nonce_with_bank(
            10_000_000,
            |_| {},
            5_000_000,
            250_000,
            None,
            FeatureSet::all_enabled(),
        )
        .unwrap();
    let alice_keypair = Keypair::new();
    let alice_pubkey = alice_keypair.pubkey();
    let custodian_pubkey = custodian_keypair.pubkey();
    let nonce_pubkey = nonce_keypair.pubkey();
    let bad_nonce_authority_keypair = Keypair::new();
    let bad_nonce_authority = bad_nonce_authority_keypair.pubkey();
    let custodian_account = bank.get_account(&custodian_pubkey).unwrap();

    debug!("alice: {}", alice_pubkey);
    debug!("custodian: {}", custodian_pubkey);
    debug!("nonce: {}", nonce_pubkey);
    debug!("nonce account: {:?}", bank.get_account(&nonce_pubkey));
    debug!("cust: {:?}", custodian_account);
    let nonce_hash = get_nonce_blockhash(&bank, &nonce_pubkey).unwrap();

    for _ in 0..MAX_RECENT_BLOCKHASHES + 1 {
        goto_end_of_slot(bank.clone());
        bank = new_from_parent_with_fork_next_slot(bank, bank_forks.as_ref());
    }

    let nonce_tx = Transaction::new_signed_with_payer(
        &[
            system_instruction::advance_nonce_account(&nonce_pubkey, &bad_nonce_authority),
            system_instruction::transfer(&custodian_pubkey, &alice_pubkey, 42),
        ],
        Some(&custodian_pubkey),
        &[&custodian_keypair, &bad_nonce_authority_keypair],
        nonce_hash,
    );
    debug!("{:?}", nonce_tx);
    let initial_custodian_balance = custodian_account.lamports();
    assert_eq!(
        bank.process_transaction(&nonce_tx),
        Err(TransactionError::BlockhashNotFound),
    );
    /* Check fee was *not* charged and nonce has *not* advanced */
    let mut recent_message = nonce_tx.message;
    recent_message.recent_blockhash = bank.last_blockhash();
    assert_eq!(
        bank.get_balance(&custodian_pubkey),
        initial_custodian_balance
    );
    assert_eq!(
        nonce_hash,
        get_nonce_blockhash(&bank, &nonce_pubkey).unwrap()
    );
}

#[test]
fn test_nonce_payer() {
    solana_logger::setup();
    let nonce_starting_balance = 250_000;
    let (mut bank, _mint_keypair, custodian_keypair, nonce_keypair, bank_forks) =
        setup_nonce_with_bank(
            10_000_000,
            |_| {},
            5_000_000,
            nonce_starting_balance,
            None,
            FeatureSet::all_enabled(),
        )
        .unwrap();
    let alice_keypair = Keypair::new();
    let alice_pubkey = alice_keypair.pubkey();
    let custodian_pubkey = custodian_keypair.pubkey();
    let nonce_pubkey = nonce_keypair.pubkey();

    debug!("alice: {}", alice_pubkey);
    debug!("custodian: {}", custodian_pubkey);
    debug!("nonce: {}", nonce_pubkey);
    debug!("nonce account: {:?}", bank.get_account(&nonce_pubkey));
    debug!("cust: {:?}", bank.get_account(&custodian_pubkey));
    let nonce_hash = get_nonce_blockhash(&bank, &nonce_pubkey).unwrap();

    for _ in 0..MAX_RECENT_BLOCKHASHES + 1 {
        goto_end_of_slot(bank.clone());
        bank = new_from_parent_with_fork_next_slot(bank, bank_forks.as_ref());
    }

    let nonce_tx = Transaction::new_signed_with_payer(
        &[
            system_instruction::advance_nonce_account(&nonce_pubkey, &nonce_pubkey),
            system_instruction::transfer(&custodian_pubkey, &alice_pubkey, 100_000_000),
        ],
        Some(&nonce_pubkey),
        &[&custodian_keypair, &nonce_keypair],
        nonce_hash,
    );
    debug!("{:?}", nonce_tx);
    assert_eq!(
        bank.process_transaction(&nonce_tx),
        Err(TransactionError::InstructionError(
            1,
            system_instruction::SystemError::ResultWithNegativeLamports.into(),
        ))
    );
    /* Check fee charged and nonce has advanced */
    let mut recent_message = nonce_tx.message;
    recent_message.recent_blockhash = bank.last_blockhash();
    assert_eq!(
        bank.get_balance(&nonce_pubkey),
        nonce_starting_balance
            - bank
                .get_fee_for_message(&new_sanitized_message(recent_message))
                .unwrap()
    );
    assert_ne!(
        nonce_hash,
        get_nonce_blockhash(&bank, &nonce_pubkey).unwrap()
    );
}

#[test]
fn test_nonce_payer_tx_wide_cap() {
    solana_logger::setup();
    let nonce_starting_balance =
        250_000 + FeeStructure::default().compute_fee_bins.last().unwrap().fee;
    let feature_set = FeatureSet::all_enabled();
    let (mut bank, _mint_keypair, custodian_keypair, nonce_keypair, bank_forks) =
        setup_nonce_with_bank(
            10_000_000,
            |_| {},
            5_000_000,
            nonce_starting_balance,
            None,
            feature_set,
        )
        .unwrap();
    let alice_keypair = Keypair::new();
    let alice_pubkey = alice_keypair.pubkey();
    let custodian_pubkey = custodian_keypair.pubkey();
    let nonce_pubkey = nonce_keypair.pubkey();

    debug!("alice: {}", alice_pubkey);
    debug!("custodian: {}", custodian_pubkey);
    debug!("nonce: {}", nonce_pubkey);
    debug!("nonce account: {:?}", bank.get_account(&nonce_pubkey));
    debug!("cust: {:?}", bank.get_account(&custodian_pubkey));
    let nonce_hash = get_nonce_blockhash(&bank, &nonce_pubkey).unwrap();

    for _ in 0..MAX_RECENT_BLOCKHASHES + 1 {
        goto_end_of_slot(bank.clone());
        bank = new_from_parent_with_fork_next_slot(bank, bank_forks.as_ref());
    }

    let nonce_tx = Transaction::new_signed_with_payer(
        &[
            system_instruction::advance_nonce_account(&nonce_pubkey, &nonce_pubkey),
            system_instruction::transfer(&custodian_pubkey, &alice_pubkey, 100_000_000),
        ],
        Some(&nonce_pubkey),
        &[&custodian_keypair, &nonce_keypair],
        nonce_hash,
    );
    debug!("{:?}", nonce_tx);

    assert_eq!(
        bank.process_transaction(&nonce_tx),
        Err(TransactionError::InstructionError(
            1,
            system_instruction::SystemError::ResultWithNegativeLamports.into(),
        ))
    );
    /* Check fee charged and nonce has advanced */
    let mut recent_message = nonce_tx.message;
    recent_message.recent_blockhash = bank.last_blockhash();
    assert_eq!(
        bank.get_balance(&nonce_pubkey),
        nonce_starting_balance
            - bank
                .get_fee_for_message(&new_sanitized_message(recent_message))
                .unwrap()
    );
    assert_ne!(
        nonce_hash,
        get_nonce_blockhash(&bank, &nonce_pubkey).unwrap()
    );
}

#[test]
fn test_nonce_fee_calculator_updates() {
    let (mut genesis_config, mint_keypair) = create_genesis_config(1_000_000);
    genesis_config.rent.lamports_per_byte_year = 0;
    let mut bank = Bank::new_for_tests(&genesis_config);
    bank.feature_set = Arc::new(FeatureSet::all_enabled());
    let (mut bank, bank_forks) = bank.wrap_with_bank_forks_for_tests();

    // Deliberately use bank 0 to initialize nonce account, so that nonce account fee_calculator indicates 0 fees
    let (custodian_keypair, nonce_keypair) =
        nonce_setup(&bank, &mint_keypair, 500_000, 100_000, None).unwrap();
    let custodian_pubkey = custodian_keypair.pubkey();
    let nonce_pubkey = nonce_keypair.pubkey();

    // Grab the hash and fee_calculator stored in the nonce account
    let (stored_nonce_hash, stored_fee_calculator) = bank
        .get_account(&nonce_pubkey)
        .and_then(|acc| {
            let nonce_versions = StateMut::<nonce::state::Versions>::state(&acc);
            match nonce_versions.ok()?.state() {
                nonce::State::Initialized(ref data) => {
                    Some((data.blockhash(), data.fee_calculator))
                }
                _ => None,
            }
        })
        .unwrap();

    // Kick nonce hash off the blockhash_queue
    for _ in 0..MAX_RECENT_BLOCKHASHES + 1 {
        goto_end_of_slot(bank.clone());
        bank = new_from_parent_with_fork_next_slot(bank, bank_forks.as_ref());
    }

    // Nonce transfer
    let nonce_tx = Transaction::new_signed_with_payer(
        &[
            system_instruction::advance_nonce_account(&nonce_pubkey, &nonce_pubkey),
            system_instruction::transfer(
                &custodian_pubkey,
                &solana_sdk::pubkey::new_rand(),
                100_000,
            ),
        ],
        Some(&custodian_pubkey),
        &[&custodian_keypair, &nonce_keypair],
        stored_nonce_hash,
    );
    bank.process_transaction(&nonce_tx).unwrap();

    // Grab the new hash and fee_calculator; both should be updated
    let (nonce_hash, fee_calculator) = bank
        .get_account(&nonce_pubkey)
        .and_then(|acc| {
            let nonce_versions = StateMut::<nonce::state::Versions>::state(&acc);
            match nonce_versions.ok()?.state() {
                nonce::State::Initialized(ref data) => {
                    Some((data.blockhash(), data.fee_calculator))
                }
                _ => None,
            }
        })
        .unwrap();

    assert_ne!(stored_nonce_hash, nonce_hash);
    assert_ne!(stored_fee_calculator, fee_calculator);
}

#[test]
fn test_nonce_fee_calculator_updates_tx_wide_cap() {
    let (mut genesis_config, mint_keypair) = create_genesis_config(1_000_000);
    genesis_config.rent.lamports_per_byte_year = 0;
    let mut bank = Bank::new_for_tests(&genesis_config);
    bank.feature_set = Arc::new(FeatureSet::all_enabled());
    let (mut bank, bank_forks) = bank.wrap_with_bank_forks_for_tests();

    // Deliberately use bank 0 to initialize nonce account, so that nonce account fee_calculator indicates 0 fees
    let (custodian_keypair, nonce_keypair) =
        nonce_setup(&bank, &mint_keypair, 500_000, 100_000, None).unwrap();
    let custodian_pubkey = custodian_keypair.pubkey();
    let nonce_pubkey = nonce_keypair.pubkey();

    // Grab the hash and fee_calculator stored in the nonce account
    let (stored_nonce_hash, stored_fee_calculator) = bank
        .get_account(&nonce_pubkey)
        .and_then(|acc| {
            let nonce_versions = StateMut::<nonce::state::Versions>::state(&acc);
            match nonce_versions.ok()?.state() {
                nonce::State::Initialized(ref data) => {
                    Some((data.blockhash(), data.fee_calculator))
                }
                _ => None,
            }
        })
        .unwrap();

    // Kick nonce hash off the blockhash_queue
    for _ in 0..MAX_RECENT_BLOCKHASHES + 1 {
        goto_end_of_slot(bank.clone());
        bank = new_from_parent_with_fork_next_slot(bank, bank_forks.as_ref());
    }

    // Nonce transfer
    let nonce_tx = Transaction::new_signed_with_payer(
        &[
            system_instruction::advance_nonce_account(&nonce_pubkey, &nonce_pubkey),
            system_instruction::transfer(
                &custodian_pubkey,
                &solana_sdk::pubkey::new_rand(),
                100_000,
            ),
        ],
        Some(&custodian_pubkey),
        &[&custodian_keypair, &nonce_keypair],
        stored_nonce_hash,
    );
    bank.process_transaction(&nonce_tx).unwrap();

    // Grab the new hash and fee_calculator; both should be updated
    let (nonce_hash, fee_calculator) = bank
        .get_account(&nonce_pubkey)
        .and_then(|acc| {
            let nonce_versions = StateMut::<nonce::state::Versions>::state(&acc);
            match nonce_versions.ok()?.state() {
                nonce::State::Initialized(ref data) => {
                    Some((data.blockhash(), data.fee_calculator))
                }
                _ => None,
            }
        })
        .unwrap();

    assert_ne!(stored_nonce_hash, nonce_hash);
    assert_ne!(stored_fee_calculator, fee_calculator);
}

#[test]
fn test_check_ro_durable_nonce_fails() {
    let (mut bank, _mint_keypair, custodian_keypair, nonce_keypair, bank_forks) =
        setup_nonce_with_bank(
            10_000_000,
            |_| {},
            5_000_000,
            250_000,
            None,
            FeatureSet::all_enabled(),
        )
        .unwrap();
    let custodian_pubkey = custodian_keypair.pubkey();
    let nonce_pubkey = nonce_keypair.pubkey();

    let nonce_hash = get_nonce_blockhash(&bank, &nonce_pubkey).unwrap();
    let account_metas = vec![
        AccountMeta::new_readonly(nonce_pubkey, false),
        #[allow(deprecated)]
        AccountMeta::new_readonly(sysvar::recent_blockhashes::id(), false),
        AccountMeta::new_readonly(nonce_pubkey, true),
    ];
    let nonce_instruction = Instruction::new_with_bincode(
        system_program::id(),
        &system_instruction::SystemInstruction::AdvanceNonceAccount,
        account_metas,
    );
    let tx = Transaction::new_signed_with_payer(
        &[nonce_instruction],
        Some(&custodian_pubkey),
        &[&custodian_keypair, &nonce_keypair],
        nonce_hash,
    );
    // SanitizedMessage::get_durable_nonce returns None because nonce
    // account is not writable. Durable nonce and blockhash domains are
    // separate, so the recent_blockhash (== durable nonce) in the
    // transaction is not found in the hash queue.
    assert_eq!(
        bank.process_transaction(&tx),
        Err(TransactionError::BlockhashNotFound),
    );
    // Kick nonce hash off the blockhash_queue
    for _ in 0..MAX_RECENT_BLOCKHASHES + 1 {
        goto_end_of_slot(bank.clone());
        bank = new_from_parent_with_fork_next_slot(bank, bank_forks.as_ref())
    }
    // Caught by the runtime because it is a nonce transaction
    assert_eq!(
        bank.process_transaction(&tx),
        Err(TransactionError::BlockhashNotFound)
    );
    let (_, lamports_per_signature) = bank.last_blockhash_and_lamports_per_signature();
    assert_eq!(
        bank.check_load_and_advance_message_nonce_account(
            &new_sanitized_message(tx.message().clone()),
            &bank.next_durable_nonce(),
            lamports_per_signature,
        ),
        None
    );
}

#[test]
fn test_collect_balances() {
    let (parent, _bank_forks) = create_simple_test_arc_bank(500);
    let bank0 = Arc::new(new_from_parent(parent));

    let keypair = Keypair::new();
    let pubkey0 = solana_sdk::pubkey::new_rand();
    let pubkey1 = solana_sdk::pubkey::new_rand();
    let program_id = Pubkey::from([2; 32]);
    let keypair_account = AccountSharedData::new(8, 0, &program_id);
    let account0 = AccountSharedData::new(11, 0, &program_id);
    let program_account = AccountSharedData::new(1, 10, &Pubkey::default());
    bank0.store_account(&keypair.pubkey(), &keypair_account);
    bank0.store_account(&pubkey0, &account0);
    bank0.store_account(&program_id, &program_account);

    let instructions = vec![CompiledInstruction::new(1, &(), vec![0])];
    let tx0 = Transaction::new_with_compiled_instructions(
        &[&keypair],
        &[pubkey0],
        Hash::default(),
        vec![program_id],
        instructions,
    );
    let instructions = vec![CompiledInstruction::new(1, &(), vec![0])];
    let tx1 = Transaction::new_with_compiled_instructions(
        &[&keypair],
        &[pubkey1],
        Hash::default(),
        vec![program_id],
        instructions,
    );
    let txs = vec![tx0, tx1];
    let batch = bank0.prepare_batch_for_tests(txs.clone());
    let balances = bank0.collect_balances(&batch);
    assert_eq!(balances.len(), 2);
    assert_eq!(balances[0], vec![8, 11, 1]);
    assert_eq!(balances[1], vec![8, 0, 1]);

    let txs: Vec<_> = txs.into_iter().rev().collect();
    let batch = bank0.prepare_batch_for_tests(txs);
    let balances = bank0.collect_balances(&batch);
    assert_eq!(balances.len(), 2);
    assert_eq!(balances[0], vec![8, 0, 1]);
    assert_eq!(balances[1], vec![8, 11, 1]);
}

#[test]
fn test_pre_post_transaction_balances() {
    let (mut genesis_config, _mint_keypair) = create_genesis_config(500_000);
    let fee_rate_governor = FeeRateGovernor::new(5000, 0);
    genesis_config.fee_rate_governor = fee_rate_governor;
    let (parent, bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);
    let bank0 = new_from_parent_with_fork_next_slot(parent, bank_forks.as_ref());

    let keypair0 = Keypair::new();
    let keypair1 = Keypair::new();
    let pubkey0 = solana_sdk::pubkey::new_rand();
    let pubkey1 = solana_sdk::pubkey::new_rand();
    let pubkey2 = solana_sdk::pubkey::new_rand();
    let keypair0_account = AccountSharedData::new(908_000, 0, &Pubkey::default());
    let keypair1_account = AccountSharedData::new(909_000, 0, &Pubkey::default());
    let account0 = AccountSharedData::new(911_000, 0, &Pubkey::default());
    bank0.store_account(&keypair0.pubkey(), &keypair0_account);
    bank0.store_account(&keypair1.pubkey(), &keypair1_account);
    bank0.store_account(&pubkey0, &account0);

    let blockhash = bank0.last_blockhash();

    let tx0 = system_transaction::transfer(&keypair0, &pubkey0, 2_000, blockhash);
    let tx1 = system_transaction::transfer(&Keypair::new(), &pubkey1, 2_000, blockhash);
    let tx2 = system_transaction::transfer(&keypair1, &pubkey2, 912_000, blockhash);
    let txs = vec![tx0, tx1, tx2];

    let lock_result = bank0.prepare_batch_for_tests(txs);
    let (commit_results, transaction_balances_set) = bank0.load_execute_and_commit_transactions(
        &lock_result,
        MAX_PROCESSING_AGE,
        true,
        ExecutionRecordingConfig::new_single_setting(false),
        &mut ExecuteTimings::default(),
        None,
    );

    assert_eq!(transaction_balances_set.pre_balances.len(), 3);
    assert_eq!(transaction_balances_set.post_balances.len(), 3);

    assert!(commit_results[0].was_executed_successfully());
    assert_eq!(
        transaction_balances_set.pre_balances[0],
        vec![908_000, 911_000, 1]
    );
    assert_eq!(
        transaction_balances_set.post_balances[0],
        vec![901_000, 913_000, 1]
    );

    // Failed transactions still produce balance sets
    // This is a TransactionError - not possible to charge fees
    assert_matches!(commit_results[1], Err(TransactionError::AccountNotFound));
    assert_eq!(transaction_balances_set.pre_balances[1], vec![0, 0, 1]);
    assert_eq!(transaction_balances_set.post_balances[1], vec![0, 0, 1]);

    // Failed transactions still produce balance sets
    // This is an InstructionError - fees charged
    assert_eq!(
        commit_results[2].as_ref().unwrap().status,
        Err(TransactionError::InstructionError(
            0,
            InstructionError::Custom(1),
        )),
    );
    assert_eq!(
        transaction_balances_set.pre_balances[2],
        vec![909_000, 0, 1]
    );
    assert_eq!(
        transaction_balances_set.post_balances[2],
        vec![904_000, 0, 1]
    );
}

#[test]
fn test_transaction_with_duplicate_accounts_in_instruction() {
    let (genesis_config, mint_keypair) = create_genesis_config_no_tx_fee_no_rent(500);

    let mock_program_id = Pubkey::from([2u8; 32]);
    let (bank, _bank_forks) =
        Bank::new_with_mockup_builtin_for_tests(&genesis_config, mock_program_id, MockBuiltin::vm);

    declare_process_instruction!(MockBuiltin, 1, |invoke_context| {
        let transaction_context = &invoke_context.transaction_context;
        let instruction_context = transaction_context.get_current_instruction_context()?;
        let instruction_data = instruction_context.get_instruction_data();
        let lamports = u64::from_le_bytes(instruction_data.try_into().unwrap());
        instruction_context
            .try_borrow_instruction_account(transaction_context, 2)?
            .checked_sub_lamports(lamports)?;
        instruction_context
            .try_borrow_instruction_account(transaction_context, 1)?
            .checked_add_lamports(lamports)?;
        instruction_context
            .try_borrow_instruction_account(transaction_context, 0)?
            .checked_sub_lamports(lamports)?;
        instruction_context
            .try_borrow_instruction_account(transaction_context, 1)?
            .checked_add_lamports(lamports)?;
        Ok(())
    });

    let from_pubkey = solana_sdk::pubkey::new_rand();
    let to_pubkey = solana_sdk::pubkey::new_rand();
    let dup_pubkey = from_pubkey;
    let from_account = AccountSharedData::new(sol_to_lamports(100.), 1, &mock_program_id);
    let to_account = AccountSharedData::new(0, 1, &mock_program_id);
    bank.store_account(&from_pubkey, &from_account);
    bank.store_account(&to_pubkey, &to_account);

    let account_metas = vec![
        AccountMeta::new(from_pubkey, false),
        AccountMeta::new(to_pubkey, false),
        AccountMeta::new(dup_pubkey, false),
    ];
    let instruction =
        Instruction::new_with_bincode(mock_program_id, &sol_to_lamports(10.), account_metas);
    let tx = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&mint_keypair.pubkey()),
        &[&mint_keypair],
        bank.last_blockhash(),
    );

    let result = bank.process_transaction(&tx);
    assert_eq!(result, Ok(()));
    assert_eq!(bank.get_balance(&from_pubkey), sol_to_lamports(80.));
    assert_eq!(bank.get_balance(&to_pubkey), sol_to_lamports(20.));
}

#[test]
fn test_transaction_with_program_ids_passed_to_programs() {
    let (genesis_config, mint_keypair) = create_genesis_config_no_tx_fee_no_rent(500);

    let mock_program_id = Pubkey::from([2u8; 32]);
    let (bank, _bank_forks) =
        Bank::new_with_mockup_builtin_for_tests(&genesis_config, mock_program_id, MockBuiltin::vm);

    let from_pubkey = solana_sdk::pubkey::new_rand();
    let to_pubkey = solana_sdk::pubkey::new_rand();
    let dup_pubkey = from_pubkey;
    let from_account = AccountSharedData::new(100, 1, &mock_program_id);
    let to_account = AccountSharedData::new(0, 1, &mock_program_id);
    bank.store_account(&from_pubkey, &from_account);
    bank.store_account(&to_pubkey, &to_account);

    let account_metas = vec![
        AccountMeta::new(from_pubkey, false),
        AccountMeta::new(to_pubkey, false),
        AccountMeta::new(dup_pubkey, false),
        AccountMeta::new(mock_program_id, false),
    ];
    let instruction = Instruction::new_with_bincode(mock_program_id, &10, account_metas);
    let tx = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&mint_keypair.pubkey()),
        &[&mint_keypair],
        bank.last_blockhash(),
    );

    let result = bank.process_transaction(&tx);
    assert_eq!(result, Ok(()));
}

#[test]
fn test_account_ids_after_program_ids() {
    solana_logger::setup();
    let (genesis_config, mint_keypair) = create_genesis_config_no_tx_fee_no_rent(500);
    let (bank, bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);

    let from_pubkey = solana_sdk::pubkey::new_rand();
    let to_pubkey = solana_sdk::pubkey::new_rand();

    let account_metas = vec![
        AccountMeta::new(from_pubkey, false),
        AccountMeta::new(to_pubkey, false),
    ];

    let instruction = Instruction::new_with_bincode(solana_vote_program::id(), &10, account_metas);
    let mut tx = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&mint_keypair.pubkey()),
        &[&mint_keypair],
        bank.last_blockhash(),
    );

    tx.message.account_keys.push(solana_sdk::pubkey::new_rand());

    let slot = bank.slot().saturating_add(1);
    let mut bank = Bank::new_from_parent(bank, &Pubkey::default(), slot);
    bank.add_mockup_builtin(solana_vote_program::id(), MockBuiltin::vm);
    let bank = bank_forks
        .write()
        .unwrap()
        .insert(bank)
        .clone_without_scheduler();

    let result = bank.process_transaction(&tx);
    assert_eq!(result, Ok(()));
    let account = bank.get_account(&solana_vote_program::id()).unwrap();
    info!("account: {:?}", account);
    assert!(account.executable());
}

#[test]
fn test_incinerator() {
    let (genesis_config, mint_keypair) = create_genesis_config_no_tx_fee_no_rent(1_000_000_000_000);
    let (bank0, bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);

    // Move to the first normal slot so normal rent behaviour applies
    let bank = new_bank_from_parent_with_bank_forks(
        bank_forks.as_ref(),
        bank0,
        &Pubkey::default(),
        genesis_config.epoch_schedule.first_normal_slot,
    );
    let pre_capitalization = bank.capitalization();

    // Burn a non-rent exempt amount
    let burn_amount = bank.get_minimum_balance_for_rent_exemption(0) - 1;

    assert_eq!(bank.get_balance(&incinerator::id()), 0);
    bank.transfer(burn_amount, &mint_keypair, &incinerator::id())
        .unwrap();
    assert_eq!(bank.get_balance(&incinerator::id()), burn_amount);
    bank.freeze();
    assert_eq!(bank.get_balance(&incinerator::id()), 0);

    // Ensure that no rent was collected, and the entire burn amount was removed from bank
    // capitalization
    assert_eq!(bank.capitalization(), pre_capitalization - burn_amount);
}

#[test]
fn test_duplicate_account_key() {
    solana_logger::setup();
    let (genesis_config, mint_keypair) = create_genesis_config(500);
    let (bank, _bank_forks) = Bank::new_with_mockup_builtin_for_tests(
        &genesis_config,
        solana_vote_program::id(),
        MockBuiltin::vm,
    );

    let from_pubkey = solana_sdk::pubkey::new_rand();
    let to_pubkey = solana_sdk::pubkey::new_rand();

    let account_metas = vec![
        AccountMeta::new(from_pubkey, false),
        AccountMeta::new(to_pubkey, false),
    ];

    let instruction = Instruction::new_with_bincode(solana_vote_program::id(), &10, account_metas);
    let mut tx = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&mint_keypair.pubkey()),
        &[&mint_keypair],
        bank.last_blockhash(),
    );
    tx.message.account_keys.push(from_pubkey);

    let result = bank.process_transaction(&tx);
    assert_eq!(result, Err(TransactionError::AccountLoadedTwice));
}

#[test]
fn test_process_transaction_with_too_many_account_locks() {
    solana_logger::setup();
    let (genesis_config, mint_keypair) = create_genesis_config(500);
    let (bank, _bank_forks) = Bank::new_with_mockup_builtin_for_tests(
        &genesis_config,
        solana_vote_program::id(),
        MockBuiltin::vm,
    );

    let from_pubkey = solana_sdk::pubkey::new_rand();
    let to_pubkey = solana_sdk::pubkey::new_rand();

    let account_metas = vec![
        AccountMeta::new(from_pubkey, false),
        AccountMeta::new(to_pubkey, false),
    ];

    let instruction = Instruction::new_with_bincode(solana_vote_program::id(), &10, account_metas);
    let mut tx = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&mint_keypair.pubkey()),
        &[&mint_keypair],
        bank.last_blockhash(),
    );

    let transaction_account_lock_limit = bank.get_transaction_account_lock_limit();
    while tx.message.account_keys.len() <= transaction_account_lock_limit {
        tx.message.account_keys.push(solana_sdk::pubkey::new_rand());
    }

    let result = bank.process_transaction(&tx);
    assert_eq!(result, Err(TransactionError::TooManyAccountLocks));
}

#[test]
fn test_program_id_as_payer() {
    solana_logger::setup();
    let (genesis_config, mint_keypair) = create_genesis_config(500);
    let mut bank = Bank::new_for_tests(&genesis_config);

    let from_pubkey = solana_sdk::pubkey::new_rand();
    let to_pubkey = solana_sdk::pubkey::new_rand();

    let account_metas = vec![
        AccountMeta::new(from_pubkey, false),
        AccountMeta::new(to_pubkey, false),
    ];

    bank.add_mockup_builtin(solana_vote_program::id(), MockBuiltin::vm);

    let instruction = Instruction::new_with_bincode(solana_vote_program::id(), &10, account_metas);
    let mut tx = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&mint_keypair.pubkey()),
        &[&mint_keypair],
        bank.last_blockhash(),
    );

    info!(
        "mint: {} account keys: {:?}",
        mint_keypair.pubkey(),
        tx.message.account_keys
    );
    assert_eq!(tx.message.account_keys.len(), 4);
    tx.message.account_keys.clear();
    tx.message.account_keys.push(solana_vote_program::id());
    tx.message.account_keys.push(mint_keypair.pubkey());
    tx.message.account_keys.push(from_pubkey);
    tx.message.account_keys.push(to_pubkey);
    tx.message.instructions[0].program_id_index = 0;
    tx.message.instructions[0].accounts.clear();
    tx.message.instructions[0].accounts.push(2);
    tx.message.instructions[0].accounts.push(3);

    let result = bank.process_transaction(&tx);
    assert_eq!(result, Err(TransactionError::SanitizeFailure));
}

#[test]
fn test_ref_account_key_after_program_id() {
    let (genesis_config, mint_keypair) = create_genesis_config_no_tx_fee_no_rent(500);
    let (bank, bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);

    let from_pubkey = solana_sdk::pubkey::new_rand();
    let to_pubkey = solana_sdk::pubkey::new_rand();

    let account_metas = vec![
        AccountMeta::new(from_pubkey, false),
        AccountMeta::new(to_pubkey, false),
    ];

    let slot = bank.slot().saturating_add(1);
    let mut bank = Bank::new_from_parent(bank, &Pubkey::default(), slot);
    bank.add_mockup_builtin(solana_vote_program::id(), MockBuiltin::vm);
    let bank = bank_forks
        .write()
        .unwrap()
        .insert(bank)
        .clone_without_scheduler();

    let instruction = Instruction::new_with_bincode(solana_vote_program::id(), &10, account_metas);
    let mut tx = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&mint_keypair.pubkey()),
        &[&mint_keypair],
        bank.last_blockhash(),
    );

    tx.message.account_keys.push(solana_sdk::pubkey::new_rand());
    assert_eq!(tx.message.account_keys.len(), 5);
    tx.message.instructions[0].accounts.remove(0);
    tx.message.instructions[0].accounts.push(4);

    let result = bank.process_transaction(&tx);
    assert_eq!(result, Ok(()));
}

#[test]
fn test_fuzz_instructions() {
    solana_logger::setup();
    use rand::{thread_rng, Rng};
    let bank = create_simple_test_bank(1_000_000_000);

    let max_programs = 5;
    let program_keys: Vec<_> = (0..max_programs)
        .enumerate()
        .map(|i| {
            let key = solana_sdk::pubkey::new_rand();
            let name = format!("program{i:?}");
            bank.transaction_processor.add_builtin(
                &bank,
                key,
                name.as_str(),
                ProgramCacheEntry::new_builtin(0, 0, MockBuiltin::vm),
            );
            (key, name.as_bytes().to_vec())
        })
        .collect();
    let (bank, _bank_forks) = bank.wrap_with_bank_forks_for_tests();
    let max_keys = MAX_STATIC_ACCOUNTS_PER_PACKET;
    let keys: Vec<_> = (0..max_keys)
        .enumerate()
        .map(|_| {
            let key = solana_sdk::pubkey::new_rand();
            let balance = if thread_rng().gen_ratio(9, 10) {
                let lamports = if thread_rng().gen_ratio(1, 5) {
                    thread_rng().gen_range(0..10)
                } else {
                    thread_rng().gen_range(20..100)
                };
                let space = thread_rng().gen_range(0..10);
                let owner = Pubkey::default();
                let account = AccountSharedData::new(lamports, space, &owner);
                bank.store_account(&key, &account);
                lamports
            } else {
                0
            };
            (key, balance)
        })
        .collect();
    let mut results = HashMap::new();
    for _ in 0..2_000 {
        let num_keys = if thread_rng().gen_ratio(1, 5) {
            thread_rng().gen_range(0..max_keys)
        } else {
            thread_rng().gen_range(1..4)
        };
        let num_instructions = thread_rng().gen_range(0..max_keys - num_keys);

        let mut account_keys: Vec<_> = if thread_rng().gen_ratio(1, 5) {
            (0..num_keys)
                .map(|_| {
                    let idx = thread_rng().gen_range(0..keys.len());
                    keys[idx].0
                })
                .collect()
        } else {
            let mut inserted = HashSet::new();
            (0..num_keys)
                .map(|_| {
                    let mut idx;
                    loop {
                        idx = thread_rng().gen_range(0..keys.len());
                        if !inserted.contains(&idx) {
                            break;
                        }
                    }
                    inserted.insert(idx);
                    keys[idx].0
                })
                .collect()
        };

        let instructions: Vec<_> = if num_keys > 0 {
            (0..num_instructions)
                .map(|_| {
                    let num_accounts_to_pass = thread_rng().gen_range(0..num_keys);
                    let account_indexes = (0..num_accounts_to_pass)
                        .map(|_| thread_rng().gen_range(0..num_keys))
                        .collect();
                    let program_index: u8 = thread_rng().gen_range(0..num_keys);
                    if thread_rng().gen_ratio(4, 5) {
                        let programs_index = thread_rng().gen_range(0..program_keys.len());
                        account_keys[program_index as usize] = program_keys[programs_index].0;
                    }
                    CompiledInstruction::new(program_index, &10, account_indexes)
                })
                .collect()
        } else {
            vec![]
        };

        let account_keys_len = std::cmp::max(account_keys.len(), 2);
        let num_signatures = if thread_rng().gen_ratio(1, 5) {
            thread_rng().gen_range(0..account_keys_len + 10)
        } else {
            thread_rng().gen_range(1..account_keys_len)
        };

        let num_required_signatures = if thread_rng().gen_ratio(1, 5) {
            thread_rng().gen_range(0..account_keys_len + 10) as u8
        } else {
            thread_rng().gen_range(1..std::cmp::max(2, num_signatures)) as u8
        };
        let num_readonly_signed_accounts = if thread_rng().gen_ratio(1, 5) {
            thread_rng().gen_range(0..account_keys_len) as u8
        } else {
            let max = if num_required_signatures > 1 {
                num_required_signatures - 1
            } else {
                1
            };
            thread_rng().gen_range(0..max)
        };

        let num_readonly_unsigned_accounts = if thread_rng().gen_ratio(1, 5)
            || (num_required_signatures as usize) >= account_keys_len
        {
            thread_rng().gen_range(0..account_keys_len) as u8
        } else {
            thread_rng().gen_range(0..account_keys_len - num_required_signatures as usize) as u8
        };

        let header = MessageHeader {
            num_required_signatures,
            num_readonly_signed_accounts,
            num_readonly_unsigned_accounts,
        };
        let message = Message {
            header,
            account_keys,
            recent_blockhash: bank.last_blockhash(),
            instructions,
        };

        let tx = Transaction {
            signatures: vec![Signature::default(); num_signatures],
            message,
        };

        let result = bank.process_transaction(&tx);
        for (key, balance) in &keys {
            assert_eq!(bank.get_balance(key), *balance);
        }
        for (key, name) in &program_keys {
            let account = bank.get_account(key).unwrap();
            assert!(account.executable());
            assert_eq!(account.data(), name);
        }
        info!("result: {:?}", result);
        let result_key = format!("{result:?}");
        *results.entry(result_key).or_insert(0) += 1;
    }
    info!("results: {:?}", results);
}

#[test]
fn test_bank_hash_consistency() {
    solana_logger::setup();

    let account = AccountSharedData::new(1_000_000_000_000, 0, &system_program::id());
    assert_eq!(account.rent_epoch(), 0);
    let mut genesis_config = GenesisConfig::new(&[(Pubkey::from([42; 32]), account)], &[]);
    genesis_config.creation_time = 0;
    genesis_config.cluster_type = ClusterType::MainnetBeta;
    genesis_config.rent.burn_percent = 100;
    activate_feature(
        &mut genesis_config,
        solana_feature_set::set_exempt_rent_epoch_max::id(),
    );

    let mut bank = Arc::new(Bank::new_for_tests(&genesis_config));
    // Check a few slots, cross an epoch boundary
    assert_eq!(bank.get_slots_in_epoch(0), 32);
    loop {
        goto_end_of_slot(bank.clone());

        if bank.slot == 0 {
            assert_eq!(
                bank.hash().to_string(),
                "Hn2FoJuoFWXVFVnwcQ6peuT24mUPmhDtXHXVjKD7M4yP",
            );
        }

        if bank.slot == 32 {
            assert_eq!(
                bank.hash().to_string(),
                "7FPfwBut4b7bXtKPsobQS1cuFgF47SZHDb4teQcJRomv"
            );
        }
        if bank.slot == 64 {
            assert_eq!(
                bank.hash().to_string(),
                "28CWiEuA3izdt5xe4LyS4Q1DTALmYgrVctSTazFiPVcW"
            );
        }
        if bank.slot == 128 {
            assert_eq!(
                bank.hash().to_string(),
                "AdCmEvRXWKpvXb9fG6AFQhzGgB5ciAXnDajvaNK7YUg8"
            );
            break;
        }
        bank = Arc::new(new_from_parent(bank));
    }
}

#[ignore]
#[test]
fn test_same_program_id_uses_unique_executable_accounts() {
    declare_process_instruction!(MockBuiltin, 1, |invoke_context| {
        let transaction_context = &invoke_context.transaction_context;
        let instruction_context = transaction_context.get_current_instruction_context()?;
        instruction_context
            .try_borrow_program_account(transaction_context, 0)?
            .set_data_length(2)
    });

    let (genesis_config, mint_keypair) = create_genesis_config(50000);
    let program1_pubkey = solana_sdk::pubkey::new_rand();
    let (bank, _bank_forks) =
        Bank::new_with_mockup_builtin_for_tests(&genesis_config, program1_pubkey, MockBuiltin::vm);

    // Add a new program owned by the first
    let program2_pubkey = solana_sdk::pubkey::new_rand();
    let mut program2_account = AccountSharedData::new(1, 1, &program1_pubkey);
    program2_account.set_executable(true);
    bank.store_account(&program2_pubkey, &program2_account);

    let instruction = Instruction::new_with_bincode(program2_pubkey, &10, vec![]);
    let tx = Transaction::new_signed_with_payer(
        &[instruction.clone(), instruction],
        Some(&mint_keypair.pubkey()),
        &[&mint_keypair],
        bank.last_blockhash(),
    );
    assert!(bank.process_transaction(&tx).is_ok());
    assert_eq!(6, bank.get_account(&program1_pubkey).unwrap().data().len());
    assert_eq!(1, bank.get_account(&program2_pubkey).unwrap().data().len());
}

fn get_shrink_account_size() -> usize {
    let (genesis_config, _mint_keypair) = create_genesis_config(1_000_000_000);

    // Set root for bank 0, with caching disabled so we can get the size
    // of the storage for this slot
    let bank0 = Arc::new(Bank::new_with_config_for_tests(
        &genesis_config,
        BankTestConfig::default(),
    ));
    bank0.restore_old_behavior_for_fragile_tests();
    goto_end_of_slot(bank0.clone());
    bank0.freeze();
    bank0.squash();
    add_root_and_flush_write_cache(&bank0);

    let sizes = bank0
        .rc
        .accounts
        .accounts_db
        .sizes_of_accounts_in_storage_for_tests(0);

    // Create an account such that it takes DEFAULT_ACCOUNTS_SHRINK_RATIO of the total account space for
    // the slot, so when it gets pruned, the storage entry will become a shrink candidate.
    let bank0_total_size: usize = sizes.into_iter().sum();
    let pubkey0_size = (bank0_total_size as f64 / (1.0 - DEFAULT_ACCOUNTS_SHRINK_RATIO)).ceil();
    assert!(
        pubkey0_size / (pubkey0_size + bank0_total_size as f64) > DEFAULT_ACCOUNTS_SHRINK_RATIO
    );
    pubkey0_size as usize
}

#[test]
fn test_clean_nonrooted() {
    solana_logger::setup();

    let (genesis_config, _mint_keypair) = create_genesis_config(1_000_000_000);
    let pubkey0 = Pubkey::from([0; 32]);
    let pubkey1 = Pubkey::from([1; 32]);

    info!("pubkey0: {}", pubkey0);
    info!("pubkey1: {}", pubkey1);

    // Set root for bank 0, with caching enabled
    let bank0 = Arc::new(Bank::new_with_config_for_tests(
        &genesis_config,
        BankTestConfig::default(),
    ));

    let account_zero = AccountSharedData::new(0, 0, &Pubkey::new_unique());

    goto_end_of_slot(bank0.clone());
    bank0.freeze();
    bank0.squash();
    // Flush now so that accounts cache cleaning doesn't clean up bank 0 when later
    // slots add updates to the cache
    bank0.force_flush_accounts_cache();

    // Store some lamports in bank 1
    let some_lamports = 123;
    let bank1 = Arc::new(Bank::new_from_parent(bank0.clone(), &Pubkey::default(), 1));
    test_utils::deposit(&bank1, &pubkey0, some_lamports).unwrap();
    goto_end_of_slot(bank1.clone());
    bank1.freeze();
    bank1.flush_accounts_cache_slot_for_tests();

    bank1.print_accounts_stats();

    // Store some lamports for pubkey1 in bank 2, root bank 2
    // bank2's parent is bank0
    let bank2 = Arc::new(Bank::new_from_parent(bank0, &Pubkey::default(), 2));
    test_utils::deposit(&bank2, &pubkey1, some_lamports).unwrap();
    bank2.store_account(&pubkey0, &account_zero);
    goto_end_of_slot(bank2.clone());
    bank2.freeze();
    bank2.squash();
    bank2.force_flush_accounts_cache();

    bank2.print_accounts_stats();
    drop(bank1);

    // Clean accounts, which should add earlier slots to the shrink
    // candidate set
    bank2.clean_accounts_for_tests();

    let bank3 = Arc::new(Bank::new_from_parent(bank2, &Pubkey::default(), 3));
    test_utils::deposit(&bank3, &pubkey1, some_lamports + 1).unwrap();
    goto_end_of_slot(bank3.clone());
    bank3.freeze();
    bank3.squash();
    bank3.force_flush_accounts_cache();

    bank3.clean_accounts_for_tests();
    assert_eq!(
        bank3.rc.accounts.accounts_db.ref_count_for_pubkey(&pubkey0),
        2
    );
    assert!(bank3
        .rc
        .accounts
        .accounts_db
        .storage
        .get_slot_storage_entry(1)
        .is_none());

    bank3.print_accounts_stats();
}

#[test]
fn test_shrink_candidate_slots_cached() {
    solana_logger::setup();

    let (genesis_config, _mint_keypair) = create_genesis_config(1_000_000_000);
    let pubkey0 = solana_sdk::pubkey::new_rand();
    let pubkey1 = solana_sdk::pubkey::new_rand();
    let pubkey2 = solana_sdk::pubkey::new_rand();

    // Set root for bank 0, with caching enabled
    let bank0 = Arc::new(Bank::new_with_config_for_tests(
        &genesis_config,
        BankTestConfig::default(),
    ));
    bank0.restore_old_behavior_for_fragile_tests();

    let pubkey0_size = get_shrink_account_size();

    let account0 = AccountSharedData::new(1000, pubkey0_size, &Pubkey::new_unique());
    bank0.store_account(&pubkey0, &account0);

    goto_end_of_slot(bank0.clone());
    bank0.freeze();
    bank0.squash();
    // Flush now so that accounts cache cleaning doesn't clean up bank 0 when later
    // slots add updates to the cache
    bank0.force_flush_accounts_cache();

    // Store some lamports in bank 1
    let some_lamports = 123;
    let bank1 = Arc::new(new_from_parent(bank0));
    test_utils::deposit(&bank1, &pubkey1, some_lamports).unwrap();
    test_utils::deposit(&bank1, &pubkey2, some_lamports).unwrap();
    goto_end_of_slot(bank1.clone());
    bank1.freeze();
    bank1.squash();
    // Flush now so that accounts cache cleaning doesn't clean up bank 0 when later
    // slots add updates to the cache
    bank1.force_flush_accounts_cache();

    // Store some lamports for pubkey1 in bank 2, root bank 2
    let bank2 = Arc::new(new_from_parent(bank1));
    test_utils::deposit(&bank2, &pubkey1, some_lamports).unwrap();
    bank2.store_account(&pubkey0, &account0);
    goto_end_of_slot(bank2.clone());
    bank2.freeze();
    bank2.squash();
    bank2.force_flush_accounts_cache();

    // Clean accounts, which should add earlier slots to the shrink
    // candidate set
    bank2.clean_accounts_for_tests();

    // Slots 0 and 1 should be candidates for shrinking, but slot 2
    // shouldn't because none of its accounts are outdated by a later
    // root
    assert_eq!(bank2.shrink_candidate_slots(), 2);
    let alive_counts: Vec<usize> = (0..3)
        .map(|slot| {
            bank2
                .rc
                .accounts
                .accounts_db
                .alive_account_count_in_slot(slot)
        })
        .collect();

    // No more slots should be shrunk
    assert_eq!(bank2.shrink_candidate_slots(), 0);
    // alive_counts represents the count of alive accounts in the three slots 0,1,2
    assert_eq!(alive_counts, vec![15, 1, 6]);
}

#[test]
fn test_add_builtin_no_overwrite() {
    let slot = 123;
    let program_id = solana_sdk::pubkey::new_rand();

    let (parent_bank, _bank_forks) = create_simple_test_arc_bank(100_000);
    let mut bank = Arc::new(Bank::new_from_parent(parent_bank, &Pubkey::default(), slot));
    assert_eq!(bank.get_account_modified_slot(&program_id), None);

    Arc::get_mut(&mut bank)
        .unwrap()
        .add_mockup_builtin(program_id, MockBuiltin::vm);
    assert_eq!(bank.get_account_modified_slot(&program_id).unwrap().1, slot);

    let mut bank = Arc::new(new_from_parent(bank));
    Arc::get_mut(&mut bank)
        .unwrap()
        .add_mockup_builtin(program_id, MockBuiltin::vm);
    assert_eq!(bank.get_account_modified_slot(&program_id).unwrap().1, slot);
}

#[test]
fn test_add_builtin_loader_no_overwrite() {
    let slot = 123;
    let loader_id = solana_sdk::pubkey::new_rand();

    let (parent_bank, _bank_forks) = create_simple_test_arc_bank(100_000);
    let mut bank = Arc::new(Bank::new_from_parent(parent_bank, &Pubkey::default(), slot));
    assert_eq!(bank.get_account_modified_slot(&loader_id), None);

    Arc::get_mut(&mut bank)
        .unwrap()
        .add_mockup_builtin(loader_id, MockBuiltin::vm);
    assert_eq!(bank.get_account_modified_slot(&loader_id).unwrap().1, slot);

    let mut bank = Arc::new(new_from_parent(bank));
    Arc::get_mut(&mut bank)
        .unwrap()
        .add_mockup_builtin(loader_id, MockBuiltin::vm);
    assert_eq!(bank.get_account_modified_slot(&loader_id).unwrap().1, slot);
}

#[test]
fn test_add_builtin_account() {
    for pass in 0..5 {
        let (mut genesis_config, _mint_keypair) = create_genesis_config(100_000);
        activate_all_features(&mut genesis_config);

        let slot = 123;
        // The account at program_id will be created initially with just 1 lamport.
        // This is below the rent-exempt minimum.  If the program_id is in the
        // rent collection partition for the banks used in this test, then the
        // account will be rent-collected away.  That'll make the test fail.
        // So pick a pubkey that is guaranteed to *not* be part of rent collection.
        let program_id = Pubkey::new_from_array([0xFF; 32]);

        let bank = Arc::new(Bank::new_from_parent(
            Arc::new(Bank::new_for_tests(&genesis_config)),
            &Pubkey::default(),
            slot,
        ));
        add_root_and_flush_write_cache(&bank.parent().unwrap());
        assert_eq!(bank.get_account_modified_slot(&program_id), None);

        assert_capitalization_diff(
            &bank,
            || bank.add_builtin_account("mock_program", &program_id),
            |old, new| {
                assert_eq!(old + 1, new);
                pass == 0
            },
        );
        if pass == 0 {
            continue;
        }

        assert_eq!(bank.get_account_modified_slot(&program_id).unwrap().1, slot);

        let bank = Arc::new(new_from_parent(bank));
        add_root_and_flush_write_cache(&bank.parent().unwrap());
        assert_capitalization_diff(
            &bank,
            || bank.add_builtin_account("mock_program", &program_id),
            |old, new| {
                assert_eq!(old, new);
                pass == 1
            },
        );
        if pass == 1 {
            continue;
        }

        assert_eq!(bank.get_account_modified_slot(&program_id).unwrap().1, slot);

        let bank = Arc::new(new_from_parent(bank));
        add_root_and_flush_write_cache(&bank.parent().unwrap());
        // No builtin replacement should happen if the program id is already assigned to a
        // builtin.
        assert_capitalization_diff(
            &bank,
            || bank.add_builtin_account("mock_program v2", &program_id),
            |old, new| {
                assert_eq!(old, new);
                pass == 2
            },
        );
        if pass == 2 {
            continue;
        }

        // No replacement should have happened
        assert_eq!(bank.get_account_modified_slot(&program_id).unwrap().1, slot);
    }
}

/// useful to adapt tests written prior to introduction of the write cache
/// to use the write cache
fn add_root_and_flush_write_cache(bank: &Bank) {
    bank.rc.accounts.add_root(bank.slot());
    bank.flush_accounts_cache_slot_for_tests()
}

#[test]
fn test_add_builtin_account_inherited_cap_while_replacing() {
    for pass in 0..4 {
        let (genesis_config, mint_keypair) = create_genesis_config(100_000);
        let bank = Bank::new_for_tests(&genesis_config);
        let program_id = solana_sdk::pubkey::new_rand();

        bank.add_builtin_account("mock_program", &program_id);
        if pass == 0 {
            add_root_and_flush_write_cache(&bank);
            assert_eq!(bank.capitalization(), bank.calculate_capitalization(true));
            continue;
        }

        // someone mess with program_id's balance
        bank.withdraw(&mint_keypair.pubkey(), 10).unwrap();
        if pass == 1 {
            add_root_and_flush_write_cache(&bank);
            assert_ne!(bank.capitalization(), bank.calculate_capitalization(true));
            continue;
        }
        test_utils::deposit(&bank, &program_id, 10).unwrap();
        if pass == 2 {
            add_root_and_flush_write_cache(&bank);
            assert_eq!(bank.capitalization(), bank.calculate_capitalization(true));
            continue;
        }

        bank.add_builtin_account("mock_program v2", &program_id);
        add_root_and_flush_write_cache(&bank);
        assert_eq!(bank.capitalization(), bank.calculate_capitalization(true));
    }
}

#[test]
fn test_add_builtin_account_squatted_while_not_replacing() {
    for pass in 0..3 {
        let (genesis_config, mint_keypair) = create_genesis_config(100_000);
        let bank = Bank::new_for_tests(&genesis_config);
        let program_id = solana_sdk::pubkey::new_rand();

        // someone managed to squat at program_id!
        bank.withdraw(&mint_keypair.pubkey(), 10).unwrap();
        if pass == 0 {
            add_root_and_flush_write_cache(&bank);
            assert_ne!(bank.capitalization(), bank.calculate_capitalization(true));
            continue;
        }
        test_utils::deposit(&bank, &program_id, 10).unwrap();
        if pass == 1 {
            add_root_and_flush_write_cache(&bank);
            assert_eq!(bank.capitalization(), bank.calculate_capitalization(true));
            continue;
        }

        bank.add_builtin_account("mock_program", &program_id);
        add_root_and_flush_write_cache(&bank);
        assert_eq!(bank.capitalization(), bank.calculate_capitalization(true));
    }
}

#[test]
#[should_panic(
    expected = "Can't change frozen bank by adding not-existing new builtin \
                program (mock_program, CiXgo2KHKSDmDnV1F6B69eWFgNAPiSBjjYvfB4cvRNre). \
                Maybe, inconsistent program activation is detected on snapshot restore?"
)]
fn test_add_builtin_account_after_frozen() {
    let slot = 123;
    let program_id = Pubkey::from_str("CiXgo2KHKSDmDnV1F6B69eWFgNAPiSBjjYvfB4cvRNre").unwrap();

    let (parent_bank, _bank_forks) = create_simple_test_arc_bank(100_000);
    let bank = Bank::new_from_parent(parent_bank, &Pubkey::default(), slot);
    bank.freeze();

    bank.add_builtin_account("mock_program", &program_id);
}

#[test]
fn test_add_precompiled_account() {
    for pass in 0..2 {
        let (mut genesis_config, _mint_keypair) = create_genesis_config(100_000);
        activate_all_features(&mut genesis_config);

        let slot = 123;
        let program_id = solana_sdk::pubkey::new_rand();

        let bank = Arc::new(Bank::new_from_parent(
            Arc::new(Bank::new_for_tests(&genesis_config)),
            &Pubkey::default(),
            slot,
        ));
        add_root_and_flush_write_cache(&bank.parent().unwrap());
        assert_eq!(bank.get_account_modified_slot(&program_id), None);

        assert_capitalization_diff(
            &bank,
            || bank.add_precompiled_account(&program_id),
            |old, new| {
                assert_eq!(old + 1, new);
                pass == 0
            },
        );
        if pass == 0 {
            continue;
        }

        assert_eq!(bank.get_account_modified_slot(&program_id).unwrap().1, slot);

        let bank = Arc::new(new_from_parent(bank));
        add_root_and_flush_write_cache(&bank.parent().unwrap());
        assert_capitalization_diff(
            &bank,
            || bank.add_precompiled_account(&program_id),
            |old, new| {
                assert_eq!(old, new);
                true
            },
        );

        assert_eq!(bank.get_account_modified_slot(&program_id).unwrap().1, slot);
    }
}

#[test]
fn test_add_precompiled_account_inherited_cap_while_replacing() {
    // when we flush the cache, it has side effects, so we have to restart the test each time we flush the cache
    // and then want to continue modifying the bank
    for pass in 0..4 {
        let (genesis_config, mint_keypair) = create_genesis_config(100_000);
        let bank = Bank::new_for_tests(&genesis_config);
        let program_id = solana_sdk::pubkey::new_rand();

        bank.add_precompiled_account(&program_id);
        if pass == 0 {
            add_root_and_flush_write_cache(&bank);
            assert_eq!(bank.capitalization(), bank.calculate_capitalization(true));
            continue;
        }

        // someone mess with program_id's balance
        bank.withdraw(&mint_keypair.pubkey(), 10).unwrap();
        if pass == 1 {
            add_root_and_flush_write_cache(&bank);
            assert_ne!(bank.capitalization(), bank.calculate_capitalization(true));
            continue;
        }
        test_utils::deposit(&bank, &program_id, 10).unwrap();
        if pass == 2 {
            add_root_and_flush_write_cache(&bank);
            assert_eq!(bank.capitalization(), bank.calculate_capitalization(true));
            continue;
        }

        bank.add_precompiled_account(&program_id);
        add_root_and_flush_write_cache(&bank);
        assert_eq!(bank.capitalization(), bank.calculate_capitalization(true));
    }
}

#[test]
fn test_add_precompiled_account_squatted_while_not_replacing() {
    for pass in 0..3 {
        let (genesis_config, mint_keypair) = create_genesis_config(100_000);
        let bank = Bank::new_for_tests(&genesis_config);
        let program_id = solana_sdk::pubkey::new_rand();

        // someone managed to squat at program_id!
        bank.withdraw(&mint_keypair.pubkey(), 10).unwrap();
        if pass == 0 {
            add_root_and_flush_write_cache(&bank);

            assert_ne!(bank.capitalization(), bank.calculate_capitalization(true));
            continue;
        }
        test_utils::deposit(&bank, &program_id, 10).unwrap();
        if pass == 1 {
            add_root_and_flush_write_cache(&bank);
            assert_eq!(bank.capitalization(), bank.calculate_capitalization(true));
            continue;
        }

        bank.add_precompiled_account(&program_id);
        add_root_and_flush_write_cache(&bank);

        assert_eq!(bank.capitalization(), bank.calculate_capitalization(true));
    }
}

#[test]
#[should_panic(
    expected = "Can't change frozen bank by adding not-existing new precompiled \
                program (CiXgo2KHKSDmDnV1F6B69eWFgNAPiSBjjYvfB4cvRNre). \
                Maybe, inconsistent program activation is detected on snapshot restore?"
)]
fn test_add_precompiled_account_after_frozen() {
    let slot = 123;
    let program_id = Pubkey::from_str("CiXgo2KHKSDmDnV1F6B69eWFgNAPiSBjjYvfB4cvRNre").unwrap();

    let (parent_bank, _bank_forks) = create_simple_test_arc_bank(100_000);
    let bank = Bank::new_from_parent(parent_bank, &Pubkey::default(), slot);
    bank.freeze();

    bank.add_precompiled_account(&program_id);
}

#[test]
fn test_reconfigure_token2_native_mint() {
    solana_logger::setup();

    let genesis_config =
        create_genesis_config_with_leader(5, &solana_sdk::pubkey::new_rand(), 0).genesis_config;
    let bank = Arc::new(Bank::new_for_tests(&genesis_config));
    assert_eq!(bank.get_balance(&token::native_mint::id()), 1000000000);
    let native_mint_account = bank.get_account(&token::native_mint::id()).unwrap();
    assert_eq!(native_mint_account.data().len(), 82);
    assert_eq!(native_mint_account.owner(), &token::id());
}

#[test]
fn test_bank_load_program() {
    solana_logger::setup();

    let (genesis_config, mint_keypair) = create_genesis_config_no_tx_fee(1_000_000_000);
    let bank = Bank::new_for_tests(&genesis_config);
    let (bank, bank_forks) = bank.wrap_with_bank_forks_for_tests();
    goto_end_of_slot(bank.clone());
    let bank = new_bank_from_parent_with_bank_forks(&bank_forks, bank, &Pubkey::default(), 42);
    let bank = new_bank_from_parent_with_bank_forks(&bank_forks, bank, &Pubkey::default(), 50);

    let program_key = solana_sdk::pubkey::new_rand();
    let programdata_key = solana_sdk::pubkey::new_rand();

    let mut file = File::open("../programs/bpf_loader/test_elfs/out/noop_aligned.so").unwrap();
    let mut elf = Vec::new();
    file.read_to_end(&mut elf).unwrap();
    let mut program_account = AccountSharedData::new_data(
        40,
        &UpgradeableLoaderState::Program {
            programdata_address: programdata_key,
        },
        &bpf_loader_upgradeable::id(),
    )
    .unwrap();
    program_account.set_executable(true);
    program_account.set_rent_epoch(1);
    let programdata_data_offset = UpgradeableLoaderState::size_of_programdata_metadata();
    let mut programdata_account = AccountSharedData::new(
        40,
        programdata_data_offset + elf.len(),
        &bpf_loader_upgradeable::id(),
    );
    programdata_account
        .set_state(&UpgradeableLoaderState::ProgramData {
            slot: 42,
            upgrade_authority_address: None,
        })
        .unwrap();
    programdata_account.data_as_mut_slice()[programdata_data_offset..].copy_from_slice(&elf);
    programdata_account.set_rent_epoch(1);
    bank.store_account_and_update_capitalization(&program_key, &program_account);
    bank.store_account_and_update_capitalization(&programdata_key, &programdata_account);

    let instruction = Instruction::new_with_bytes(program_key, &[], Vec::new());
    let invocation_message = Message::new(&[instruction], Some(&mint_keypair.pubkey()));
    let binding = mint_keypair.insecure_clone();
    let transaction = Transaction::new(
        &[&binding],
        invocation_message.clone(),
        bank.last_blockhash(),
    );
    assert!(bank.process_transaction(&transaction).is_ok());

    {
        let program_cache = bank.transaction_processor.program_cache.read().unwrap();
        let [program] = program_cache.get_slot_versions_for_tests(&program_key) else {
            panic!();
        };
        assert_matches!(program.program, ProgramCacheEntryType::Loaded(_));
        assert_eq!(
            program.account_size,
            program_account.data().len() + programdata_account.data().len()
        );
    }
}

#[test]
fn test_bpf_loader_upgradeable_deploy_with_max_len() {
    let (genesis_config, mint_keypair) = create_genesis_config_no_tx_fee(1_000_000_000);
    let mut bank = Bank::new_for_tests(&genesis_config);
    bank.feature_set = Arc::new(FeatureSet::all_enabled());
    let (bank, bank_forks) = bank.wrap_with_bank_forks_for_tests();
    let mut bank_client = BankClient::new_shared(bank.clone());

    // Setup keypairs and addresses
    let payer_keypair = Keypair::new();
    let program_keypair = Keypair::new();
    let buffer_address = Pubkey::new_unique();
    let (programdata_address, _) = Pubkey::find_program_address(
        &[program_keypair.pubkey().as_ref()],
        &bpf_loader_upgradeable::id(),
    );
    let upgrade_authority_keypair = Keypair::new();

    // Test nonexistent program invocation
    let instruction = Instruction::new_with_bytes(program_keypair.pubkey(), &[], Vec::new());
    let invocation_message = Message::new(&[instruction], Some(&mint_keypair.pubkey()));
    let binding = mint_keypair.insecure_clone();
    let transaction = Transaction::new(
        &[&binding],
        invocation_message.clone(),
        bank.last_blockhash(),
    );
    assert_eq!(
        bank.process_transaction(&transaction),
        Err(TransactionError::ProgramAccountNotFound),
    );
    {
        // Make sure it is not in the cache because the account owner is not a loader
        let program_cache = bank.transaction_processor.program_cache.read().unwrap();
        let slot_versions = program_cache.get_slot_versions_for_tests(&program_keypair.pubkey());
        assert!(slot_versions.is_empty());
    }

    // Advance bank to get a new last blockhash so that when we retry invocation
    // after creating the program, the new transaction created below with the
    // same `invocation_message` as above doesn't return `AlreadyProcessed` when
    // processed.
    goto_end_of_slot(bank.clone());
    let bank = bank_client
        .advance_slot(1, bank_forks.as_ref(), &mint_keypair.pubkey())
        .unwrap();

    // Load program file
    let mut file = File::open("../programs/bpf_loader/test_elfs/out/noop_aligned.so")
        .expect("file open failed");
    let mut elf = Vec::new();
    file.read_to_end(&mut elf).unwrap();

    // Compute rent exempt balances
    let program_len = elf.len();
    let min_program_balance =
        bank.get_minimum_balance_for_rent_exemption(UpgradeableLoaderState::size_of_program());
    let min_buffer_balance = bank.get_minimum_balance_for_rent_exemption(
        UpgradeableLoaderState::size_of_buffer(program_len),
    );
    let min_programdata_balance = bank.get_minimum_balance_for_rent_exemption(
        UpgradeableLoaderState::size_of_programdata(program_len),
    );

    // Setup accounts
    let buffer_account = {
        let mut account = AccountSharedData::new(
            min_buffer_balance,
            UpgradeableLoaderState::size_of_buffer(elf.len()),
            &bpf_loader_upgradeable::id(),
        );
        account
            .set_state(&UpgradeableLoaderState::Buffer {
                authority_address: Some(upgrade_authority_keypair.pubkey()),
            })
            .unwrap();
        account
            .data_as_mut_slice()
            .get_mut(UpgradeableLoaderState::size_of_buffer_metadata()..)
            .unwrap()
            .copy_from_slice(&elf);
        account
    };
    let program_account = AccountSharedData::new(
        min_programdata_balance,
        UpgradeableLoaderState::size_of_program(),
        &bpf_loader_upgradeable::id(),
    );
    let programdata_account = AccountSharedData::new(
        1,
        UpgradeableLoaderState::size_of_programdata(elf.len()),
        &bpf_loader_upgradeable::id(),
    );

    // Test uninitialized program invocation
    bank.store_account(&program_keypair.pubkey(), &program_account);
    let transaction = Transaction::new(
        &[&binding],
        invocation_message.clone(),
        bank.last_blockhash(),
    );
    assert_eq!(
        bank.process_transaction(&transaction),
        Err(TransactionError::InstructionError(
            0,
            InstructionError::InvalidAccountData
        )),
    );
    {
        let program_cache = bank.transaction_processor.program_cache.read().unwrap();
        let slot_versions = program_cache.get_slot_versions_for_tests(&program_keypair.pubkey());
        assert_eq!(slot_versions.len(), 1);
        assert_eq!(slot_versions[0].deployment_slot, bank.slot());
        assert_eq!(slot_versions[0].effective_slot, bank.slot());
        assert!(matches!(
            slot_versions[0].program,
            ProgramCacheEntryType::Closed,
        ));
    }

    // Test buffer invocation
    bank.store_account(&buffer_address, &buffer_account);
    let instruction = Instruction::new_with_bytes(buffer_address, &[], Vec::new());
    let message = Message::new(&[instruction], Some(&mint_keypair.pubkey()));
    let transaction = Transaction::new(&[&binding], message, bank.last_blockhash());
    assert_eq!(
        bank.process_transaction(&transaction),
        Err(TransactionError::InstructionError(
            0,
            InstructionError::InvalidAccountData,
        )),
    );
    {
        let program_cache = bank.transaction_processor.program_cache.read().unwrap();
        let slot_versions = program_cache.get_slot_versions_for_tests(&buffer_address);
        assert_eq!(slot_versions.len(), 1);
        assert_eq!(slot_versions[0].deployment_slot, bank.slot());
        assert_eq!(slot_versions[0].effective_slot, bank.slot());
        assert!(matches!(
            slot_versions[0].program,
            ProgramCacheEntryType::Closed,
        ));
    }

    // Test successful deploy
    let payer_base_balance = LAMPORTS_PER_SOL;
    let deploy_fees = {
        let fee_calculator = genesis_config.fee_rate_governor.create_fee_calculator();
        3 * fee_calculator.lamports_per_signature
    };
    let min_payer_balance = min_program_balance
        .saturating_add(min_programdata_balance)
        .saturating_sub(min_buffer_balance.saturating_add(deploy_fees));
    bank.store_account(
        &payer_keypair.pubkey(),
        &AccountSharedData::new(
            payer_base_balance.saturating_add(min_payer_balance),
            0,
            &system_program::id(),
        ),
    );
    bank.store_account(&program_keypair.pubkey(), &AccountSharedData::default());
    bank.store_account(&programdata_address, &AccountSharedData::default());
    let message = Message::new(
        &bpf_loader_upgradeable::deploy_with_max_program_len(
            &payer_keypair.pubkey(),
            &program_keypair.pubkey(),
            &buffer_address,
            &upgrade_authority_keypair.pubkey(),
            min_program_balance,
            elf.len(),
        )
        .unwrap(),
        Some(&payer_keypair.pubkey()),
    );
    assert!(bank_client
        .send_and_confirm_message(
            &[&payer_keypair, &program_keypair, &upgrade_authority_keypair],
            message
        )
        .is_ok());
    assert_eq!(
        bank.get_balance(&payer_keypair.pubkey()),
        payer_base_balance
    );
    assert_eq!(bank.get_balance(&buffer_address), 0);
    assert_eq!(None, bank.get_account(&buffer_address));
    let post_program_account = bank.get_account(&program_keypair.pubkey()).unwrap();
    assert_eq!(post_program_account.lamports(), min_program_balance);
    assert_eq!(post_program_account.owner(), &bpf_loader_upgradeable::id());
    assert_eq!(
        post_program_account.data().len(),
        UpgradeableLoaderState::size_of_program()
    );
    let state: UpgradeableLoaderState = post_program_account.state().unwrap();
    assert_eq!(
        state,
        UpgradeableLoaderState::Program {
            programdata_address
        }
    );
    let post_programdata_account = bank.get_account(&programdata_address).unwrap();
    assert_eq!(post_programdata_account.lamports(), min_programdata_balance);
    assert_eq!(
        post_programdata_account.owner(),
        &bpf_loader_upgradeable::id()
    );
    let state: UpgradeableLoaderState = post_programdata_account.state().unwrap();
    assert_eq!(
        state,
        UpgradeableLoaderState::ProgramData {
            slot: bank_client.get_slot().unwrap(),
            upgrade_authority_address: Some(upgrade_authority_keypair.pubkey())
        }
    );
    for (i, byte) in post_programdata_account
        .data()
        .get(UpgradeableLoaderState::size_of_programdata_metadata()..)
        .unwrap()
        .iter()
        .enumerate()
    {
        assert_eq!(*elf.get(i).unwrap(), *byte);
    }

    // Advance the bank so that the program becomes effective
    goto_end_of_slot(bank.clone());
    let bank = bank_client
        .advance_slot(1, bank_forks.as_ref(), &mint_keypair.pubkey())
        .unwrap();

    // Invoke the deployed program
    let transaction = Transaction::new(&[&binding], invocation_message, bank.last_blockhash());
    assert!(bank.process_transaction(&transaction).is_ok());
    {
        let program_cache = bank.transaction_processor.program_cache.read().unwrap();
        let slot_versions = program_cache.get_slot_versions_for_tests(&program_keypair.pubkey());
        assert_eq!(slot_versions.len(), 2);
        assert_eq!(slot_versions[0].deployment_slot, bank.slot() - 1);
        assert_eq!(slot_versions[0].effective_slot, bank.slot() - 1);
        assert!(matches!(
            slot_versions[0].program,
            ProgramCacheEntryType::Closed,
        ));
        assert_eq!(slot_versions[1].deployment_slot, bank.slot() - 1);
        assert_eq!(slot_versions[1].effective_slot, bank.slot());
        assert!(matches!(
            slot_versions[1].program,
            ProgramCacheEntryType::Loaded(_),
        ));
    }

    // Test initialized program account
    bank.clear_signatures();
    bank.store_account(&buffer_address, &buffer_account);
    let bank = bank_client
        .advance_slot(1, bank_forks.as_ref(), &mint_keypair.pubkey())
        .unwrap();
    let message = Message::new(
        &[Instruction::new_with_bincode(
            bpf_loader_upgradeable::id(),
            &UpgradeableLoaderInstruction::DeployWithMaxDataLen {
                max_data_len: elf.len(),
            },
            vec![
                AccountMeta::new(mint_keypair.pubkey(), true),
                AccountMeta::new(programdata_address, false),
                AccountMeta::new(program_keypair.pubkey(), false),
                AccountMeta::new(buffer_address, false),
                AccountMeta::new_readonly(sysvar::rent::id(), false),
                AccountMeta::new_readonly(sysvar::clock::id(), false),
                AccountMeta::new_readonly(system_program::id(), false),
                AccountMeta::new_readonly(upgrade_authority_keypair.pubkey(), true),
            ],
        )],
        Some(&mint_keypair.pubkey()),
    );
    assert_eq!(
        TransactionError::InstructionError(0, InstructionError::AccountAlreadyInitialized),
        bank_client
            .send_and_confirm_message(&[&mint_keypair, &upgrade_authority_keypair], message)
            .unwrap_err()
            .unwrap()
    );

    // Test initialized ProgramData account
    bank.clear_signatures();
    bank.store_account(&buffer_address, &buffer_account);
    bank.store_account(&program_keypair.pubkey(), &AccountSharedData::default());
    let message = Message::new(
        &bpf_loader_upgradeable::deploy_with_max_program_len(
            &mint_keypair.pubkey(),
            &program_keypair.pubkey(),
            &buffer_address,
            &upgrade_authority_keypair.pubkey(),
            min_program_balance,
            elf.len(),
        )
        .unwrap(),
        Some(&mint_keypair.pubkey()),
    );
    assert_eq!(
        TransactionError::InstructionError(1, InstructionError::Custom(0)),
        bank_client
            .send_and_confirm_message(
                &[&mint_keypair, &program_keypair, &upgrade_authority_keypair],
                message
            )
            .unwrap_err()
            .unwrap()
    );

    // Test deploy no authority
    bank.clear_signatures();
    bank.store_account(&buffer_address, &buffer_account);
    bank.store_account(&program_keypair.pubkey(), &program_account);
    bank.store_account(&programdata_address, &programdata_account);
    let message = Message::new(
        &[Instruction::new_with_bincode(
            bpf_loader_upgradeable::id(),
            &UpgradeableLoaderInstruction::DeployWithMaxDataLen {
                max_data_len: elf.len(),
            },
            vec![
                AccountMeta::new(mint_keypair.pubkey(), true),
                AccountMeta::new(programdata_address, false),
                AccountMeta::new(program_keypair.pubkey(), false),
                AccountMeta::new(buffer_address, false),
                AccountMeta::new_readonly(sysvar::rent::id(), false),
                AccountMeta::new_readonly(sysvar::clock::id(), false),
                AccountMeta::new_readonly(system_program::id(), false),
            ],
        )],
        Some(&mint_keypair.pubkey()),
    );
    assert_eq!(
        TransactionError::InstructionError(0, InstructionError::NotEnoughAccountKeys),
        bank_client
            .send_and_confirm_message(&[&mint_keypair], message)
            .unwrap_err()
            .unwrap()
    );

    // Test deploy authority not a signer
    bank.clear_signatures();
    bank.store_account(&buffer_address, &buffer_account);
    bank.store_account(&program_keypair.pubkey(), &program_account);
    bank.store_account(&programdata_address, &programdata_account);
    let message = Message::new(
        &[Instruction::new_with_bincode(
            bpf_loader_upgradeable::id(),
            &UpgradeableLoaderInstruction::DeployWithMaxDataLen {
                max_data_len: elf.len(),
            },
            vec![
                AccountMeta::new(mint_keypair.pubkey(), true),
                AccountMeta::new(programdata_address, false),
                AccountMeta::new(program_keypair.pubkey(), false),
                AccountMeta::new(buffer_address, false),
                AccountMeta::new_readonly(sysvar::rent::id(), false),
                AccountMeta::new_readonly(sysvar::clock::id(), false),
                AccountMeta::new_readonly(system_program::id(), false),
                AccountMeta::new_readonly(upgrade_authority_keypair.pubkey(), false),
            ],
        )],
        Some(&mint_keypair.pubkey()),
    );
    assert_eq!(
        TransactionError::InstructionError(0, InstructionError::MissingRequiredSignature),
        bank_client
            .send_and_confirm_message(&[&mint_keypair], message)
            .unwrap_err()
            .unwrap()
    );

    // Test invalid Buffer account state
    bank.clear_signatures();
    bank.store_account(&buffer_address, &AccountSharedData::default());
    bank.store_account(&program_keypair.pubkey(), &AccountSharedData::default());
    bank.store_account(&programdata_address, &AccountSharedData::default());
    let message = Message::new(
        &bpf_loader_upgradeable::deploy_with_max_program_len(
            &mint_keypair.pubkey(),
            &program_keypair.pubkey(),
            &buffer_address,
            &upgrade_authority_keypair.pubkey(),
            min_program_balance,
            elf.len(),
        )
        .unwrap(),
        Some(&mint_keypair.pubkey()),
    );
    assert_eq!(
        TransactionError::InstructionError(1, InstructionError::InvalidAccountData),
        bank_client
            .send_and_confirm_message(
                &[&mint_keypair, &program_keypair, &upgrade_authority_keypair],
                message
            )
            .unwrap_err()
            .unwrap()
    );

    // Test program account not rent exempt
    bank.clear_signatures();
    bank.store_account(&buffer_address, &buffer_account);
    bank.store_account(&program_keypair.pubkey(), &AccountSharedData::default());
    bank.store_account(&programdata_address, &AccountSharedData::default());
    let message = Message::new(
        &bpf_loader_upgradeable::deploy_with_max_program_len(
            &mint_keypair.pubkey(),
            &program_keypair.pubkey(),
            &buffer_address,
            &upgrade_authority_keypair.pubkey(),
            min_program_balance.saturating_sub(1),
            elf.len(),
        )
        .unwrap(),
        Some(&mint_keypair.pubkey()),
    );
    assert_eq!(
        TransactionError::InstructionError(1, InstructionError::ExecutableAccountNotRentExempt),
        bank_client
            .send_and_confirm_message(
                &[&mint_keypair, &program_keypair, &upgrade_authority_keypair],
                message
            )
            .unwrap_err()
            .unwrap()
    );

    // Test program account not rent exempt because data is larger than needed
    bank.clear_signatures();
    bank.store_account(&buffer_address, &buffer_account);
    bank.store_account(&program_keypair.pubkey(), &AccountSharedData::default());
    bank.store_account(&programdata_address, &AccountSharedData::default());
    let mut instructions = bpf_loader_upgradeable::deploy_with_max_program_len(
        &mint_keypair.pubkey(),
        &program_keypair.pubkey(),
        &buffer_address,
        &upgrade_authority_keypair.pubkey(),
        min_program_balance,
        elf.len(),
    )
    .unwrap();
    *instructions.get_mut(0).unwrap() = system_instruction::create_account(
        &mint_keypair.pubkey(),
        &program_keypair.pubkey(),
        min_program_balance,
        (UpgradeableLoaderState::size_of_program() as u64).saturating_add(1),
        &bpf_loader_upgradeable::id(),
    );
    let message = Message::new(&instructions, Some(&mint_keypair.pubkey()));
    assert_eq!(
        TransactionError::InstructionError(1, InstructionError::ExecutableAccountNotRentExempt),
        bank_client
            .send_and_confirm_message(
                &[&mint_keypair, &program_keypair, &upgrade_authority_keypair],
                message
            )
            .unwrap_err()
            .unwrap()
    );

    // Test program account too small
    bank.clear_signatures();
    bank.store_account(&buffer_address, &buffer_account);
    bank.store_account(&program_keypair.pubkey(), &AccountSharedData::default());
    bank.store_account(&programdata_address, &AccountSharedData::default());
    let mut instructions = bpf_loader_upgradeable::deploy_with_max_program_len(
        &mint_keypair.pubkey(),
        &program_keypair.pubkey(),
        &buffer_address,
        &upgrade_authority_keypair.pubkey(),
        min_program_balance,
        elf.len(),
    )
    .unwrap();
    *instructions.get_mut(0).unwrap() = system_instruction::create_account(
        &mint_keypair.pubkey(),
        &program_keypair.pubkey(),
        min_program_balance,
        (UpgradeableLoaderState::size_of_program() as u64).saturating_sub(1),
        &bpf_loader_upgradeable::id(),
    );
    let message = Message::new(&instructions, Some(&mint_keypair.pubkey()));
    assert_eq!(
        TransactionError::InstructionError(1, InstructionError::AccountDataTooSmall),
        bank_client
            .send_and_confirm_message(
                &[&mint_keypair, &program_keypair, &upgrade_authority_keypair],
                message
            )
            .unwrap_err()
            .unwrap()
    );

    // Test Insufficient payer funds (need more funds to cover the
    // difference between buffer lamports and programdata lamports)
    bank.clear_signatures();
    bank.store_account(
        &mint_keypair.pubkey(),
        &AccountSharedData::new(
            deploy_fees.saturating_add(min_program_balance),
            0,
            &system_program::id(),
        ),
    );
    bank.store_account(&buffer_address, &buffer_account);
    bank.store_account(&program_keypair.pubkey(), &AccountSharedData::default());
    bank.store_account(&programdata_address, &AccountSharedData::default());
    let message = Message::new(
        &bpf_loader_upgradeable::deploy_with_max_program_len(
            &mint_keypair.pubkey(),
            &program_keypair.pubkey(),
            &buffer_address,
            &upgrade_authority_keypair.pubkey(),
            min_program_balance,
            elf.len(),
        )
        .unwrap(),
        Some(&mint_keypair.pubkey()),
    );
    assert_eq!(
        TransactionError::InstructionError(1, InstructionError::Custom(1)),
        bank_client
            .send_and_confirm_message(
                &[&mint_keypair, &program_keypair, &upgrade_authority_keypair],
                message
            )
            .unwrap_err()
            .unwrap()
    );
    bank.store_account(
        &mint_keypair.pubkey(),
        &AccountSharedData::new(1_000_000_000, 0, &system_program::id()),
    );

    // Test max_data_len
    bank.clear_signatures();
    bank.store_account(&buffer_address, &buffer_account);
    bank.store_account(&program_keypair.pubkey(), &AccountSharedData::default());
    bank.store_account(&programdata_address, &AccountSharedData::default());
    let message = Message::new(
        &bpf_loader_upgradeable::deploy_with_max_program_len(
            &mint_keypair.pubkey(),
            &program_keypair.pubkey(),
            &buffer_address,
            &upgrade_authority_keypair.pubkey(),
            min_program_balance,
            elf.len().saturating_sub(1),
        )
        .unwrap(),
        Some(&mint_keypair.pubkey()),
    );
    assert_eq!(
        TransactionError::InstructionError(1, InstructionError::AccountDataTooSmall),
        bank_client
            .send_and_confirm_message(
                &[&mint_keypair, &program_keypair, &upgrade_authority_keypair],
                message
            )
            .unwrap_err()
            .unwrap()
    );

    // Test max_data_len too large
    bank.clear_signatures();
    bank.store_account(
        &mint_keypair.pubkey(),
        &AccountSharedData::new(u64::MAX / 2, 0, &system_program::id()),
    );
    let mut modified_buffer_account = buffer_account.clone();
    modified_buffer_account.set_lamports(u64::MAX / 2);
    bank.store_account(&buffer_address, &modified_buffer_account);
    bank.store_account(&program_keypair.pubkey(), &AccountSharedData::default());
    bank.store_account(&programdata_address, &AccountSharedData::default());
    let message = Message::new(
        &bpf_loader_upgradeable::deploy_with_max_program_len(
            &mint_keypair.pubkey(),
            &program_keypair.pubkey(),
            &buffer_address,
            &upgrade_authority_keypair.pubkey(),
            min_program_balance,
            usize::MAX,
        )
        .unwrap(),
        Some(&mint_keypair.pubkey()),
    );
    assert_eq!(
        TransactionError::InstructionError(1, InstructionError::InvalidArgument),
        bank_client
            .send_and_confirm_message(
                &[&mint_keypair, &program_keypair, &upgrade_authority_keypair],
                message
            )
            .unwrap_err()
            .unwrap()
    );

    // Test not the system account
    bank.clear_signatures();
    bank.store_account(&buffer_address, &buffer_account);
    bank.store_account(&program_keypair.pubkey(), &AccountSharedData::default());
    bank.store_account(&programdata_address, &AccountSharedData::default());
    let mut instructions = bpf_loader_upgradeable::deploy_with_max_program_len(
        &mint_keypair.pubkey(),
        &program_keypair.pubkey(),
        &buffer_address,
        &upgrade_authority_keypair.pubkey(),
        min_program_balance,
        elf.len(),
    )
    .unwrap();
    *instructions
        .get_mut(1)
        .unwrap()
        .accounts
        .get_mut(6)
        .unwrap() = AccountMeta::new_readonly(Pubkey::new_unique(), false);
    let message = Message::new(&instructions, Some(&mint_keypair.pubkey()));
    assert_eq!(
        TransactionError::InstructionError(1, InstructionError::MissingAccount),
        bank_client
            .send_and_confirm_message(
                &[&mint_keypair, &program_keypair, &upgrade_authority_keypair],
                message
            )
            .unwrap_err()
            .unwrap()
    );

    fn truncate_data(account: &mut AccountSharedData, len: usize) {
        let mut data = account.data().to_vec();
        data.truncate(len);
        account.set_data(data);
    }

    // Test Bad ELF data
    bank.clear_signatures();
    let mut modified_buffer_account = buffer_account;
    truncate_data(
        &mut modified_buffer_account,
        UpgradeableLoaderState::size_of_buffer(1),
    );
    bank.store_account(&buffer_address, &modified_buffer_account);
    bank.store_account(&program_keypair.pubkey(), &AccountSharedData::default());
    bank.store_account(&programdata_address, &AccountSharedData::default());
    let message = Message::new(
        &bpf_loader_upgradeable::deploy_with_max_program_len(
            &mint_keypair.pubkey(),
            &program_keypair.pubkey(),
            &buffer_address,
            &upgrade_authority_keypair.pubkey(),
            min_program_balance,
            elf.len(),
        )
        .unwrap(),
        Some(&mint_keypair.pubkey()),
    );
    assert_eq!(
        TransactionError::InstructionError(1, InstructionError::InvalidAccountData),
        bank_client
            .send_and_confirm_message(
                &[&mint_keypair, &program_keypair, &upgrade_authority_keypair],
                message
            )
            .unwrap_err()
            .unwrap()
    );

    // Test small buffer account
    bank.clear_signatures();
    let mut modified_buffer_account = AccountSharedData::new(
        min_programdata_balance,
        UpgradeableLoaderState::size_of_buffer(elf.len()),
        &bpf_loader_upgradeable::id(),
    );
    modified_buffer_account
        .set_state(&UpgradeableLoaderState::Buffer {
            authority_address: Some(upgrade_authority_keypair.pubkey()),
        })
        .unwrap();
    modified_buffer_account
        .data_as_mut_slice()
        .get_mut(UpgradeableLoaderState::size_of_buffer_metadata()..)
        .unwrap()
        .copy_from_slice(&elf);
    truncate_data(&mut modified_buffer_account, 5);
    bank.store_account(&buffer_address, &modified_buffer_account);
    bank.store_account(&program_keypair.pubkey(), &AccountSharedData::default());
    bank.store_account(&programdata_address, &AccountSharedData::default());
    let message = Message::new(
        &bpf_loader_upgradeable::deploy_with_max_program_len(
            &mint_keypair.pubkey(),
            &program_keypair.pubkey(),
            &buffer_address,
            &upgrade_authority_keypair.pubkey(),
            min_program_balance,
            elf.len(),
        )
        .unwrap(),
        Some(&mint_keypair.pubkey()),
    );
    assert_eq!(
        TransactionError::InstructionError(1, InstructionError::InvalidAccountData),
        bank_client
            .send_and_confirm_message(
                &[&mint_keypair, &program_keypair, &upgrade_authority_keypair],
                message
            )
            .unwrap_err()
            .unwrap()
    );

    // Mismatched buffer and program authority
    bank.clear_signatures();
    let mut modified_buffer_account = AccountSharedData::new(
        min_programdata_balance,
        UpgradeableLoaderState::size_of_buffer(elf.len()),
        &bpf_loader_upgradeable::id(),
    );
    modified_buffer_account
        .set_state(&UpgradeableLoaderState::Buffer {
            authority_address: Some(buffer_address),
        })
        .unwrap();
    modified_buffer_account
        .data_as_mut_slice()
        .get_mut(UpgradeableLoaderState::size_of_buffer_metadata()..)
        .unwrap()
        .copy_from_slice(&elf);
    bank.store_account(&buffer_address, &modified_buffer_account);
    bank.store_account(&program_keypair.pubkey(), &AccountSharedData::default());
    bank.store_account(&programdata_address, &AccountSharedData::default());
    let message = Message::new(
        &bpf_loader_upgradeable::deploy_with_max_program_len(
            &mint_keypair.pubkey(),
            &program_keypair.pubkey(),
            &buffer_address,
            &upgrade_authority_keypair.pubkey(),
            min_program_balance,
            elf.len(),
        )
        .unwrap(),
        Some(&mint_keypair.pubkey()),
    );
    assert_eq!(
        TransactionError::InstructionError(1, InstructionError::IncorrectAuthority),
        bank_client
            .send_and_confirm_message(
                &[&mint_keypair, &program_keypair, &upgrade_authority_keypair],
                message
            )
            .unwrap_err()
            .unwrap()
    );

    // Deploy buffer with mismatched None authority
    bank.clear_signatures();
    let mut modified_buffer_account = AccountSharedData::new(
        min_programdata_balance,
        UpgradeableLoaderState::size_of_buffer(elf.len()),
        &bpf_loader_upgradeable::id(),
    );
    modified_buffer_account
        .set_state(&UpgradeableLoaderState::Buffer {
            authority_address: None,
        })
        .unwrap();
    modified_buffer_account
        .data_as_mut_slice()
        .get_mut(UpgradeableLoaderState::size_of_buffer_metadata()..)
        .unwrap()
        .copy_from_slice(&elf);
    bank.store_account(&buffer_address, &modified_buffer_account);
    bank.store_account(&program_keypair.pubkey(), &AccountSharedData::default());
    bank.store_account(&programdata_address, &AccountSharedData::default());
    let message = Message::new(
        &bpf_loader_upgradeable::deploy_with_max_program_len(
            &mint_keypair.pubkey(),
            &program_keypair.pubkey(),
            &buffer_address,
            &upgrade_authority_keypair.pubkey(),
            min_program_balance,
            elf.len(),
        )
        .unwrap(),
        Some(&mint_keypair.pubkey()),
    );
    assert_eq!(
        TransactionError::InstructionError(1, InstructionError::IncorrectAuthority),
        bank_client
            .send_and_confirm_message(
                &[&mint_keypair, &program_keypair, &upgrade_authority_keypair],
                message
            )
            .unwrap_err()
            .unwrap()
    );
}

#[test]
fn test_compute_active_feature_set() {
    let (bank0, _bank_forks) = create_simple_test_arc_bank(100_000);
    let mut bank = Bank::new_from_parent(bank0, &Pubkey::default(), 1);

    let test_feature = "TestFeature11111111111111111111111111111111"
        .parse::<Pubkey>()
        .unwrap();
    let mut feature_set = FeatureSet::default();
    feature_set.inactive.insert(test_feature);
    bank.feature_set = Arc::new(feature_set.clone());

    let (feature_set, new_activations) = bank.compute_active_feature_set(true);
    assert!(new_activations.is_empty());
    assert!(!feature_set.is_active(&test_feature));

    // Depositing into the `test_feature` account should do nothing
    test_utils::deposit(&bank, &test_feature, 42).unwrap();
    let (feature_set, new_activations) = bank.compute_active_feature_set(true);
    assert!(new_activations.is_empty());
    assert!(!feature_set.is_active(&test_feature));

    // Request `test_feature` activation
    let feature = Feature::default();
    assert_eq!(feature.activated_at, None);
    bank.store_account(&test_feature, &feature::create_account(&feature, 42));
    let feature = feature::from_account(&bank.get_account(&test_feature).expect("get_account"))
        .expect("from_account");
    assert_eq!(feature.activated_at, None);

    // Run `compute_active_feature_set` excluding pending activation
    let (feature_set, new_activations) = bank.compute_active_feature_set(false);
    assert!(new_activations.is_empty());
    assert!(!feature_set.is_active(&test_feature));

    // Run `compute_active_feature_set` including pending activation
    let (_feature_set, new_activations) = bank.compute_active_feature_set(true);
    assert_eq!(new_activations.len(), 1);
    assert!(new_activations.contains(&test_feature));

    // Actually activate the pending activation
    bank.apply_feature_activations(ApplyFeatureActivationsCaller::NewFromParent, true);
    let feature = feature::from_account(&bank.get_account(&test_feature).expect("get_account"))
        .expect("from_account");
    assert_eq!(feature.activated_at, Some(1));

    let (feature_set, new_activations) = bank.compute_active_feature_set(true);
    assert!(new_activations.is_empty());
    assert!(feature_set.is_active(&test_feature));
}

#[test]
fn test_reserved_account_keys() {
    let (bank0, _bank_forks) = create_simple_test_arc_bank(100_000);
    let mut bank = Bank::new_from_parent(bank0, &Pubkey::default(), 1);
    bank.feature_set = Arc::new(FeatureSet::default());

    assert_eq!(
        bank.get_reserved_account_keys().len(),
        20,
        "before activating the new feature, bank should already have active reserved keys"
    );

    // Activate `add_new_reserved_account_keys` feature
    bank.store_account(
        &feature_set::add_new_reserved_account_keys::id(),
        &feature::create_account(&Feature::default(), 42),
    );
    bank.apply_feature_activations(ApplyFeatureActivationsCaller::NewFromParent, true);

    assert_eq!(
        bank.get_reserved_account_keys().len(),
        30,
        "after activating the new feature, bank should have new active reserved keys"
    );
}

#[test]
fn test_program_replacement() {
    let mut bank = create_simple_test_bank(0);

    // Setup original program account
    let old_address = Pubkey::new_unique();
    let new_address = Pubkey::new_unique();
    bank.store_account_and_update_capitalization(
        &old_address,
        &AccountSharedData::from(Account {
            lamports: 100,
            ..Account::default()
        }),
    );
    assert_eq!(bank.get_balance(&old_address), 100);

    // Setup new program account
    let new_program_account = AccountSharedData::from(Account {
        lamports: 123,
        ..Account::default()
    });
    bank.store_account_and_update_capitalization(&new_address, &new_program_account);
    assert_eq!(bank.get_balance(&new_address), 123);

    let original_capitalization = bank.capitalization();

    bank.replace_program_account(&old_address, &new_address, "bank-apply_program_replacement");

    // New program account is now empty
    assert_eq!(bank.get_balance(&new_address), 0);

    // Old program account holds the new program account
    assert_eq!(bank.get_account(&old_address), Some(new_program_account));

    // Lamports in the old token account were burnt
    assert_eq!(bank.capitalization(), original_capitalization - 100);
}

fn min_rent_exempt_balance_for_sysvars(bank: &Bank, sysvar_ids: &[Pubkey]) -> u64 {
    sysvar_ids
        .iter()
        .map(|sysvar_id| {
            trace!("min_rent_excempt_balance_for_sysvars: {}", sysvar_id);
            bank.get_minimum_balance_for_rent_exemption(
                bank.get_account(sysvar_id).unwrap().data().len(),
            )
        })
        .sum()
}

#[test]
fn test_adjust_sysvar_balance_for_rent() {
    let bank = create_simple_test_bank(0);
    let mut smaller_sample_sysvar = AccountSharedData::new(1, 0, &Pubkey::default());
    assert_eq!(smaller_sample_sysvar.lamports(), 1);
    bank.adjust_sysvar_balance_for_rent(&mut smaller_sample_sysvar);
    assert_eq!(
        smaller_sample_sysvar.lamports(),
        bank.get_minimum_balance_for_rent_exemption(smaller_sample_sysvar.data().len()),
    );

    let mut bigger_sample_sysvar = AccountSharedData::new(
        1,
        smaller_sample_sysvar.data().len() + 1,
        &Pubkey::default(),
    );
    bank.adjust_sysvar_balance_for_rent(&mut bigger_sample_sysvar);
    assert!(smaller_sample_sysvar.lamports() < bigger_sample_sysvar.lamports());

    // excess lamports shouldn't be reduced by adjust_sysvar_balance_for_rent()
    let excess_lamports = smaller_sample_sysvar.lamports() + 999;
    smaller_sample_sysvar.set_lamports(excess_lamports);
    bank.adjust_sysvar_balance_for_rent(&mut smaller_sample_sysvar);
    assert_eq!(smaller_sample_sysvar.lamports(), excess_lamports);
}

#[test]
fn test_update_clock_timestamp() {
    let leader_pubkey = solana_sdk::pubkey::new_rand();
    let GenesisConfigInfo {
        genesis_config,
        voting_keypair,
        ..
    } = create_genesis_config_with_leader(5, &leader_pubkey, 3);
    let mut bank = Bank::new_for_tests(&genesis_config);
    // Advance past slot 0, which has special handling.
    bank = new_from_parent(Arc::new(bank));
    bank = new_from_parent(Arc::new(bank));
    assert_eq!(
        bank.clock().unix_timestamp,
        bank.unix_timestamp_from_genesis()
    );

    bank.update_clock(None);
    assert_eq!(
        bank.clock().unix_timestamp,
        bank.unix_timestamp_from_genesis()
    );

    update_vote_account_timestamp(
        BlockTimestamp {
            slot: bank.slot(),
            timestamp: bank.unix_timestamp_from_genesis() - 1,
        },
        &bank,
        &voting_keypair.pubkey(),
    );
    bank.update_clock(None);
    assert_eq!(
        bank.clock().unix_timestamp,
        bank.unix_timestamp_from_genesis()
    );

    update_vote_account_timestamp(
        BlockTimestamp {
            slot: bank.slot(),
            timestamp: bank.unix_timestamp_from_genesis(),
        },
        &bank,
        &voting_keypair.pubkey(),
    );
    bank.update_clock(None);
    assert_eq!(
        bank.clock().unix_timestamp,
        bank.unix_timestamp_from_genesis()
    );

    update_vote_account_timestamp(
        BlockTimestamp {
            slot: bank.slot(),
            timestamp: bank.unix_timestamp_from_genesis() + 1,
        },
        &bank,
        &voting_keypair.pubkey(),
    );
    bank.update_clock(None);
    assert_eq!(
        bank.clock().unix_timestamp,
        bank.unix_timestamp_from_genesis() + 1
    );

    // Timestamp cannot go backward from ancestor Bank to child
    bank = new_from_parent(Arc::new(bank));
    update_vote_account_timestamp(
        BlockTimestamp {
            slot: bank.slot(),
            timestamp: bank.unix_timestamp_from_genesis() - 1,
        },
        &bank,
        &voting_keypair.pubkey(),
    );
    bank.update_clock(None);
    assert_eq!(
        bank.clock().unix_timestamp,
        bank.unix_timestamp_from_genesis()
    );
}

fn poh_estimate_offset(bank: &Bank) -> Duration {
    let mut epoch_start_slot = bank.epoch_schedule.get_first_slot_in_epoch(bank.epoch());
    if epoch_start_slot == bank.slot() {
        epoch_start_slot = bank
            .epoch_schedule
            .get_first_slot_in_epoch(bank.epoch() - 1);
    }
    bank.slot().saturating_sub(epoch_start_slot) as u32
        * Duration::from_nanos(bank.ns_per_slot as u64)
}

#[test]
fn test_timestamp_slow() {
    fn max_allowable_delta_since_epoch(bank: &Bank, max_allowable_drift: u32) -> i64 {
        let poh_estimate_offset = poh_estimate_offset(bank);
        (poh_estimate_offset.as_secs()
            + (poh_estimate_offset * max_allowable_drift / 100).as_secs()) as i64
    }

    let leader_pubkey = solana_sdk::pubkey::new_rand();
    let GenesisConfigInfo {
        mut genesis_config,
        voting_keypair,
        ..
    } = create_genesis_config_with_leader(5, &leader_pubkey, 3);
    let slots_in_epoch = 32;
    genesis_config.epoch_schedule = EpochSchedule::new(slots_in_epoch);
    let mut bank = Bank::new_for_tests(&genesis_config);
    let slot_duration = Duration::from_nanos(bank.ns_per_slot as u64);

    let recent_timestamp: UnixTimestamp = bank.unix_timestamp_from_genesis();
    let additional_secs =
        ((slot_duration * MAX_ALLOWABLE_DRIFT_PERCENTAGE_SLOW_V2 * 32) / 100).as_secs() as i64 + 1; // Greater than max_allowable_drift_slow_v2 for full epoch
    update_vote_account_timestamp(
        BlockTimestamp {
            slot: bank.slot(),
            timestamp: recent_timestamp + additional_secs,
        },
        &bank,
        &voting_keypair.pubkey(),
    );

    // additional_secs greater than MAX_ALLOWABLE_DRIFT_PERCENTAGE_SLOW_V2 for an epoch
    // timestamp bounded to 150% deviation
    for _ in 0..31 {
        bank = new_from_parent(Arc::new(bank));
        assert_eq!(
            bank.clock().unix_timestamp,
            bank.clock().epoch_start_timestamp
                + max_allowable_delta_since_epoch(&bank, MAX_ALLOWABLE_DRIFT_PERCENTAGE_SLOW_V2),
        );
        assert_eq!(bank.clock().epoch_start_timestamp, recent_timestamp);
    }
}

#[test]
fn test_timestamp_fast() {
    fn max_allowable_delta_since_epoch(bank: &Bank, max_allowable_drift: u32) -> i64 {
        let poh_estimate_offset = poh_estimate_offset(bank);
        (poh_estimate_offset.as_secs()
            - (poh_estimate_offset * max_allowable_drift / 100).as_secs()) as i64
    }

    let leader_pubkey = solana_sdk::pubkey::new_rand();
    let GenesisConfigInfo {
        mut genesis_config,
        voting_keypair,
        ..
    } = create_genesis_config_with_leader(5, &leader_pubkey, 3);
    let slots_in_epoch = 32;
    genesis_config.epoch_schedule = EpochSchedule::new(slots_in_epoch);
    let mut bank = Bank::new_for_tests(&genesis_config);

    let recent_timestamp: UnixTimestamp = bank.unix_timestamp_from_genesis();
    let additional_secs = 5; // Greater than MAX_ALLOWABLE_DRIFT_PERCENTAGE_FAST for full epoch
    update_vote_account_timestamp(
        BlockTimestamp {
            slot: bank.slot(),
            timestamp: recent_timestamp - additional_secs,
        },
        &bank,
        &voting_keypair.pubkey(),
    );

    // additional_secs greater than MAX_ALLOWABLE_DRIFT_PERCENTAGE_FAST for an epoch
    // timestamp bounded to 25% deviation
    for _ in 0..31 {
        bank = new_from_parent(Arc::new(bank));
        assert_eq!(
            bank.clock().unix_timestamp,
            bank.clock().epoch_start_timestamp
                + max_allowable_delta_since_epoch(&bank, MAX_ALLOWABLE_DRIFT_PERCENTAGE_FAST),
        );
        assert_eq!(bank.clock().epoch_start_timestamp, recent_timestamp);
    }
}

#[test]
fn test_program_is_native_loader() {
    let (genesis_config, mint_keypair) = create_genesis_config(50000);
    let (bank, _bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);

    let tx = Transaction::new_signed_with_payer(
        &[Instruction::new_with_bincode(
            native_loader::id(),
            &(),
            vec![],
        )],
        Some(&mint_keypair.pubkey()),
        &[&mint_keypair],
        bank.last_blockhash(),
    );
    assert_eq!(
        bank.process_transaction(&tx),
        Err(TransactionError::InstructionError(
            0,
            InstructionError::UnsupportedProgramId
        ))
    );
}

#[test]
fn test_debug_bank() {
    let (genesis_config, _mint_keypair) = create_genesis_config(50000);
    let mut bank = Bank::new_for_tests(&genesis_config);
    bank.finish_init(&genesis_config, None, false);
    let debug = format!("{bank:#?}");
    assert!(!debug.is_empty());
}

#[derive(Debug)]
enum AcceptableScanResults {
    DroppedSlotError,
    NoFailure,
    Both,
}

fn test_store_scan_consistency<F>(
    update_f: F,
    drop_callback: Option<Box<dyn DropCallback + Send + Sync>>,
    acceptable_scan_results: AcceptableScanResults,
) where
    F: Fn(
            Arc<Bank>,
            crossbeam_channel::Sender<Arc<Bank>>,
            crossbeam_channel::Receiver<BankId>,
            Arc<HashSet<Pubkey>>,
            Pubkey,
            u64,
        ) + std::marker::Send
        + 'static,
{
    solana_logger::setup();
    // Set up initial bank
    let mut genesis_config =
        create_genesis_config_with_leader(10, &solana_sdk::pubkey::new_rand(), 374_999_998_287_840)
            .genesis_config;
    genesis_config.rent = Rent::free();
    let bank0 = Arc::new(Bank::new_with_config_for_tests(
        &genesis_config,
        BankTestConfig::default(),
    ));
    bank0.set_callback(drop_callback);

    // Set up pubkeys to write to
    let total_pubkeys = ITER_BATCH_SIZE * 10;
    let total_pubkeys_to_modify = 10;
    let all_pubkeys: Vec<Pubkey> = std::iter::repeat_with(solana_sdk::pubkey::new_rand)
        .take(total_pubkeys)
        .collect();
    let program_id = system_program::id();
    let starting_lamports = 1;
    let starting_account = AccountSharedData::new(starting_lamports, 0, &program_id);

    // Write accounts to the store
    for key in &all_pubkeys {
        bank0.store_account(key, &starting_account);
    }

    // Set aside a subset of accounts to modify
    let pubkeys_to_modify: Arc<HashSet<Pubkey>> = Arc::new(
        all_pubkeys
            .into_iter()
            .take(total_pubkeys_to_modify)
            .collect(),
    );
    let exit = Arc::new(AtomicBool::new(false));

    // Thread that runs scan and constantly checks for
    // consistency
    let pubkeys_to_modify_ = pubkeys_to_modify.clone();

    // Channel over which the bank to scan is sent
    let (bank_to_scan_sender, bank_to_scan_receiver): (
        crossbeam_channel::Sender<Arc<Bank>>,
        crossbeam_channel::Receiver<Arc<Bank>>,
    ) = bounded(1);

    let (scan_finished_sender, scan_finished_receiver): (
        crossbeam_channel::Sender<BankId>,
        crossbeam_channel::Receiver<BankId>,
    ) = unbounded();
    let num_banks_scanned = Arc::new(AtomicU64::new(0));
    let scan_thread = {
        let exit = exit.clone();
        let num_banks_scanned = num_banks_scanned.clone();
        Builder::new()
            .name("scan".to_string())
            .spawn(move || {
                loop {
                    info!("starting scan iteration");
                    if exit.load(Relaxed) {
                        info!("scan exiting");
                        return;
                    }
                    if let Ok(bank_to_scan) =
                        bank_to_scan_receiver.recv_timeout(Duration::from_millis(10))
                    {
                        info!("scanning program accounts for slot {}", bank_to_scan.slot());
                        let accounts_result =
                            bank_to_scan.get_program_accounts(&program_id, &ScanConfig::default());
                        let _ = scan_finished_sender.send(bank_to_scan.bank_id());
                        num_banks_scanned.fetch_add(1, Relaxed);
                        match (&acceptable_scan_results, accounts_result.is_err()) {
                            (AcceptableScanResults::DroppedSlotError, _)
                            | (AcceptableScanResults::Both, true) => {
                                assert_eq!(
                                    accounts_result,
                                    Err(ScanError::SlotRemoved {
                                        slot: bank_to_scan.slot(),
                                        bank_id: bank_to_scan.bank_id()
                                    })
                                );
                            }
                            (AcceptableScanResults::NoFailure, _)
                            | (AcceptableScanResults::Both, false) => {
                                assert!(accounts_result.is_ok())
                            }
                        }

                        // Should never see empty accounts because no slot ever deleted
                        // any of the original accounts, and the scan should reflect the
                        // account state at some frozen slot `X` (no partial updates).
                        if let Ok(accounts) = accounts_result {
                            assert!(!accounts.is_empty());
                            let mut expected_lamports = None;
                            let mut target_accounts_found = HashSet::new();
                            for (pubkey, account) in accounts {
                                let account_balance = account.lamports();
                                if pubkeys_to_modify_.contains(&pubkey) {
                                    target_accounts_found.insert(pubkey);
                                    if let Some(expected_lamports) = expected_lamports {
                                        assert_eq!(account_balance, expected_lamports);
                                    } else {
                                        // All pubkeys in the specified set should have the same balance
                                        expected_lamports = Some(account_balance);
                                    }
                                }
                            }

                            // Should've found all the accounts, i.e. no partial cleans should
                            // be detected
                            assert_eq!(target_accounts_found.len(), total_pubkeys_to_modify);
                        }
                    }
                }
            })
            .unwrap()
    };

    // Thread that constantly updates the accounts, sets
    // roots, and cleans
    let update_thread = Builder::new()
        .name("update".to_string())
        .spawn(move || {
            update_f(
                bank0,
                bank_to_scan_sender,
                scan_finished_receiver,
                pubkeys_to_modify,
                program_id,
                starting_lamports,
            );
        })
        .unwrap();

    // Let threads run for a while, check the scans didn't see any mixed slots
    let min_expected_number_of_scans = 5;
    std::thread::sleep(Duration::new(5, 0));
    // This can be reduced when you are running this test locally to deal with hangs
    // But, if it is too low, the ci fails intermittently.
    let mut remaining_loops = 2000;
    loop {
        if num_banks_scanned.load(Relaxed) > min_expected_number_of_scans {
            break;
        } else {
            std::thread::sleep(Duration::from_millis(100));
        }
        remaining_loops -= 1;
        if remaining_loops == 0 {
            break; // just quit and try to get the thread result (panic, etc.)
        }
    }
    exit.store(true, Relaxed);
    scan_thread.join().unwrap();
    update_thread.join().unwrap();
    assert!(remaining_loops > 0, "test timed out");
}

#[test]
fn test_store_scan_consistency_unrooted() {
    let (pruned_banks_sender, pruned_banks_receiver) = unbounded();
    let pruned_banks_request_handler = PrunedBanksRequestHandler {
        pruned_banks_receiver,
    };
    test_store_scan_consistency(
        move |bank0,
              bank_to_scan_sender,
              _scan_finished_receiver,
              pubkeys_to_modify,
              program_id,
              starting_lamports| {
            let mut current_major_fork_bank = bank0;
            loop {
                let mut current_minor_fork_bank = current_major_fork_bank.clone();
                let num_new_banks = 2;
                let lamports = current_minor_fork_bank.slot() + starting_lamports + 1;
                // Modify banks on the two banks on the minor fork
                for pubkeys_to_modify in &pubkeys_to_modify
                    .iter()
                    .chunks(pubkeys_to_modify.len() / num_new_banks)
                {
                    let slot = current_minor_fork_bank.slot() + 2;
                    current_minor_fork_bank = Arc::new(Bank::new_from_parent(
                        current_minor_fork_bank,
                        &solana_sdk::pubkey::new_rand(),
                        slot,
                    ));
                    let account = AccountSharedData::new(lamports, 0, &program_id);
                    // Write partial updates to each of the banks in the minor fork so if any of them
                    // get cleaned up, there will be keys with the wrong account value/missing.
                    for key in pubkeys_to_modify {
                        current_minor_fork_bank.store_account(key, &account);
                    }
                    current_minor_fork_bank.freeze();
                }

                // All the parent banks made in this iteration of the loop
                // are currently discoverable, previous parents should have
                // been squashed
                assert_eq!(
                    current_minor_fork_bank.clone().parents_inclusive().len(),
                    num_new_banks + 1,
                );

                // `next_major_bank` needs to be sandwiched between the minor fork banks
                // That way, after the squash(), the minor fork has the potential to see a
                // *partial* clean of the banks < `next_major_bank`.
                current_major_fork_bank = Arc::new(Bank::new_from_parent(
                    current_major_fork_bank,
                    &solana_sdk::pubkey::new_rand(),
                    current_minor_fork_bank.slot() - 1,
                ));
                let lamports = current_major_fork_bank.slot() + starting_lamports + 1;
                let account = AccountSharedData::new(lamports, 0, &program_id);
                for key in pubkeys_to_modify.iter() {
                    // Store rooted updates to these pubkeys such that the minor
                    // fork updates to the same keys will be deleted by clean
                    current_major_fork_bank.store_account(key, &account);
                }

                // Send the last new bank to the scan thread to perform the scan.
                // Meanwhile this thread will continually set roots on a separate fork
                // and squash/clean, purging the account entries from the minor forks
                /*
                            bank 0
                        /         \
                minor bank 1       \
                    /         current_major_fork_bank
                minor bank 2

                */
                // The capacity of the channel is 1 so that this thread will wait for the scan to finish before starting
                // the next iteration, allowing the scan to stay in sync with these updates
                // such that every scan will see this interruption.
                if bank_to_scan_sender.send(current_minor_fork_bank).is_err() {
                    // Channel was disconnected, exit
                    return;
                }
                current_major_fork_bank.freeze();
                current_major_fork_bank.squash();
                // Try to get cache flush/clean to overlap with the scan
                current_major_fork_bank.force_flush_accounts_cache();
                current_major_fork_bank.clean_accounts_for_tests();
                // Move purge here so that Bank::drop()->purge_slots() doesn't race
                // with clean. Simulates the call from AccountsBackgroundService
                pruned_banks_request_handler.handle_request(&current_major_fork_bank);
            }
        },
        Some(Box::new(SendDroppedBankCallback::new(pruned_banks_sender))),
        AcceptableScanResults::NoFailure,
    )
}

#[test]
fn test_store_scan_consistency_root() {
    test_store_scan_consistency(
        |bank0,
         bank_to_scan_sender,
         _scan_finished_receiver,
         pubkeys_to_modify,
         program_id,
         starting_lamports| {
            let mut current_bank = bank0.clone();
            let mut prev_bank = bank0;
            loop {
                let lamports_this_round = current_bank.slot() + starting_lamports + 1;
                let account = AccountSharedData::new(lamports_this_round, 0, &program_id);
                for key in pubkeys_to_modify.iter() {
                    current_bank.store_account(key, &account);
                }
                current_bank.freeze();
                // Send the previous bank to the scan thread to perform the scan.
                // Meanwhile this thread will squash and update roots immediately after
                // so the roots will update while scanning.
                //
                // The capacity of the channel is 1 so that this thread will wait for the scan to finish before starting
                // the next iteration, allowing the scan to stay in sync with these updates
                // such that every scan will see this interruption.
                if bank_to_scan_sender.send(prev_bank).is_err() {
                    // Channel was disconnected, exit
                    return;
                }
                current_bank.squash();
                if current_bank.slot() % 2 == 0 {
                    current_bank.force_flush_accounts_cache();
                    current_bank.clean_accounts();
                }
                prev_bank = current_bank.clone();
                let slot = current_bank.slot() + 1;
                current_bank = Arc::new(Bank::new_from_parent(
                    current_bank,
                    &solana_sdk::pubkey::new_rand(),
                    slot,
                ));
            }
        },
        None,
        AcceptableScanResults::NoFailure,
    );
}

fn setup_banks_on_fork_to_remove(
    bank0: Arc<Bank>,
    pubkeys_to_modify: Arc<HashSet<Pubkey>>,
    program_id: &Pubkey,
    starting_lamports: u64,
    num_banks_on_fork: usize,
    step_size: usize,
) -> (Arc<Bank>, Vec<(Slot, BankId)>, Ancestors) {
    // Need at least 2 keys to create inconsistency in account balances when deleting
    // slots
    assert!(pubkeys_to_modify.len() > 1);

    // Tracks the bank at the tip of the to be created fork
    let mut bank_at_fork_tip = bank0;

    // All the slots on the fork except slot 0
    let mut slots_on_fork = Vec::with_capacity(num_banks_on_fork);

    // All accounts in each set of `step_size` slots will have the same account balances.
    // The account balances of the accounts changes every `step_size` banks. Thus if you
    // delete any one of the latest `step_size` slots, then you will see varying account
    // balances when loading the accounts.
    assert!(num_banks_on_fork >= 2);
    assert!(step_size >= 2);
    let pubkeys_to_modify: Vec<Pubkey> = pubkeys_to_modify.iter().cloned().collect();
    let pubkeys_to_modify_per_slot = (pubkeys_to_modify.len() / step_size).max(1);
    for _ in (0..num_banks_on_fork).step_by(step_size) {
        let mut lamports_this_round = 0;
        for i in 0..step_size {
            let slot = bank_at_fork_tip.slot() + 1;
            bank_at_fork_tip = Arc::new(Bank::new_from_parent(
                bank_at_fork_tip,
                &solana_sdk::pubkey::new_rand(),
                slot,
            ));
            if lamports_this_round == 0 {
                lamports_this_round = bank_at_fork_tip.bank_id() + starting_lamports + 1;
            }
            let pubkey_to_modify_starting_index = i * pubkeys_to_modify_per_slot;
            let account = AccountSharedData::new(lamports_this_round, 0, program_id);
            for pubkey_index_to_modify in pubkey_to_modify_starting_index
                ..pubkey_to_modify_starting_index + pubkeys_to_modify_per_slot
            {
                let key = pubkeys_to_modify[pubkey_index_to_modify % pubkeys_to_modify.len()];
                bank_at_fork_tip.store_account(&key, &account);
            }
            bank_at_fork_tip.freeze();
            slots_on_fork.push((bank_at_fork_tip.slot(), bank_at_fork_tip.bank_id()));
        }
    }

    let ancestors: Vec<(Slot, usize)> = slots_on_fork.iter().map(|(s, _)| (*s, 0)).collect();
    let ancestors = Ancestors::from(ancestors);

    (bank_at_fork_tip, slots_on_fork, ancestors)
}

#[test]
fn test_remove_unrooted_before_scan() {
    test_store_scan_consistency(
        |bank0,
         bank_to_scan_sender,
         scan_finished_receiver,
         pubkeys_to_modify,
         program_id,
         starting_lamports| {
            loop {
                let (bank_at_fork_tip, slots_on_fork, ancestors) = setup_banks_on_fork_to_remove(
                    bank0.clone(),
                    pubkeys_to_modify.clone(),
                    &program_id,
                    starting_lamports,
                    10,
                    2,
                );
                // Test removing the slot before the scan starts, should cause
                // SlotRemoved error every time
                for k in pubkeys_to_modify.iter() {
                    assert!(bank_at_fork_tip.load_slow(&ancestors, k).is_some());
                }
                bank_at_fork_tip.remove_unrooted_slots(&slots_on_fork);

                // Accounts on this fork should not be found after removal
                for k in pubkeys_to_modify.iter() {
                    assert!(bank_at_fork_tip.load_slow(&ancestors, k).is_none());
                }
                if bank_to_scan_sender.send(bank_at_fork_tip.clone()).is_err() {
                    return;
                }

                // Wait for scan to finish before starting next iteration
                let finished_scan_bank_id = scan_finished_receiver.recv();
                if finished_scan_bank_id.is_err() {
                    return;
                }
                assert_eq!(finished_scan_bank_id.unwrap(), bank_at_fork_tip.bank_id());
            }
        },
        None,
        // Test removing the slot before the scan starts, should error every time
        AcceptableScanResults::DroppedSlotError,
    );
}

#[test]
fn test_remove_unrooted_scan_then_recreate_same_slot_before_scan() {
    test_store_scan_consistency(
        |bank0,
         bank_to_scan_sender,
         scan_finished_receiver,
         pubkeys_to_modify,
         program_id,
         starting_lamports| {
            let mut prev_bank = bank0.clone();
            loop {
                let start = Instant::now();
                let (bank_at_fork_tip, slots_on_fork, ancestors) = setup_banks_on_fork_to_remove(
                    bank0.clone(),
                    pubkeys_to_modify.clone(),
                    &program_id,
                    starting_lamports,
                    10,
                    2,
                );
                info!("setting up banks elapsed: {}", start.elapsed().as_millis());
                // Remove the fork. Then we'll recreate the slots and only after we've
                // recreated the slots, do we send this old bank for scanning.
                // Skip scanning bank 0 on first iteration of loop, since those accounts
                // aren't being removed
                if prev_bank.slot() != 0 {
                    info!(
                        "sending bank with slot: {:?}, elapsed: {}",
                        prev_bank.slot(),
                        start.elapsed().as_millis()
                    );
                    // Although we dumped the slots last iteration via `remove_unrooted_slots()`,
                    // we've recreated those slots this iteration, so they should be findable
                    // again
                    for k in pubkeys_to_modify.iter() {
                        assert!(bank_at_fork_tip.load_slow(&ancestors, k).is_some());
                    }

                    // Now after we've recreated the slots removed in the previous loop
                    // iteration, send the previous bank, should fail even though the
                    // same slots were recreated
                    if bank_to_scan_sender.send(prev_bank.clone()).is_err() {
                        return;
                    }

                    let finished_scan_bank_id = scan_finished_receiver.recv();
                    if finished_scan_bank_id.is_err() {
                        return;
                    }
                    // Wait for scan to finish before starting next iteration
                    assert_eq!(finished_scan_bank_id.unwrap(), prev_bank.bank_id());
                }
                bank_at_fork_tip.remove_unrooted_slots(&slots_on_fork);
                prev_bank = bank_at_fork_tip;
            }
        },
        None,
        // Test removing the slot before the scan starts, should error every time
        AcceptableScanResults::DroppedSlotError,
    );
}

#[test]
fn test_remove_unrooted_scan_interleaved_with_remove_unrooted_slots() {
    test_store_scan_consistency(
        |bank0,
         bank_to_scan_sender,
         scan_finished_receiver,
         pubkeys_to_modify,
         program_id,
         starting_lamports| {
            loop {
                let step_size = 2;
                let (bank_at_fork_tip, slots_on_fork, ancestors) = setup_banks_on_fork_to_remove(
                    bank0.clone(),
                    pubkeys_to_modify.clone(),
                    &program_id,
                    starting_lamports,
                    10,
                    step_size,
                );
                // Although we dumped the slots last iteration via `remove_unrooted_slots()`,
                // we've recreated those slots this iteration, so they should be findable
                // again
                for k in pubkeys_to_modify.iter() {
                    assert!(bank_at_fork_tip.load_slow(&ancestors, k).is_some());
                }

                // Now after we've recreated the slots removed in the previous loop
                // iteration, send the previous bank, should fail even though the
                // same slots were recreated
                if bank_to_scan_sender.send(bank_at_fork_tip.clone()).is_err() {
                    return;
                }

                // Remove 1 < `step_size` of the *latest* slots while the scan is happening.
                // This should create inconsistency between the account balances of accounts
                // stored in that slot, and the accounts stored in earlier slots
                let slot_to_remove = *slots_on_fork.last().unwrap();
                bank_at_fork_tip.remove_unrooted_slots(&[slot_to_remove]);

                // Wait for scan to finish before starting next iteration
                let finished_scan_bank_id = scan_finished_receiver.recv();
                if finished_scan_bank_id.is_err() {
                    return;
                }
                assert_eq!(finished_scan_bank_id.unwrap(), bank_at_fork_tip.bank_id());

                // Remove the rest of the slots before the next iteration
                for (slot, bank_id) in slots_on_fork {
                    bank_at_fork_tip.remove_unrooted_slots(&[(slot, bank_id)]);
                }
            }
        },
        None,
        // Test removing the slot before the scan starts, should error every time
        AcceptableScanResults::Both,
    );
}

#[test]
fn test_get_inflation_start_slot_devnet_testnet() {
    let GenesisConfigInfo {
        mut genesis_config, ..
    } = create_genesis_config_with_leader(42, &solana_sdk::pubkey::new_rand(), 42);
    genesis_config
        .accounts
        .remove(&feature_set::pico_inflation::id())
        .unwrap();
    genesis_config
        .accounts
        .remove(&feature_set::full_inflation::devnet_and_testnet::id())
        .unwrap();
    for pair in feature_set::FULL_INFLATION_FEATURE_PAIRS.iter() {
        genesis_config.accounts.remove(&pair.vote_id).unwrap();
        genesis_config.accounts.remove(&pair.enable_id).unwrap();
    }

    let bank = Bank::new_for_tests(&genesis_config);

    // Advance slot
    let mut bank = new_from_parent(Arc::new(bank));
    bank = new_from_parent(Arc::new(bank));
    assert_eq!(bank.get_inflation_start_slot(), 0);
    assert_eq!(bank.slot(), 2);

    // Request `pico_inflation` activation
    bank.store_account(
        &feature_set::pico_inflation::id(),
        &feature::create_account(
            &Feature {
                activated_at: Some(1),
            },
            42,
        ),
    );
    bank.feature_set = Arc::new(bank.compute_active_feature_set(true).0);
    assert_eq!(bank.get_inflation_start_slot(), 1);

    // Advance slot
    bank = new_from_parent(Arc::new(bank));
    assert_eq!(bank.slot(), 3);

    // Request `full_inflation::devnet_and_testnet` activation,
    // which takes priority over pico_inflation
    bank.store_account(
        &feature_set::full_inflation::devnet_and_testnet::id(),
        &feature::create_account(
            &Feature {
                activated_at: Some(2),
            },
            42,
        ),
    );
    bank.feature_set = Arc::new(bank.compute_active_feature_set(true).0);
    assert_eq!(bank.get_inflation_start_slot(), 2);

    // Request `full_inflation::mainnet::certusone` activation,
    // which should have no effect on `get_inflation_start_slot`
    bank.store_account(
        &feature_set::full_inflation::mainnet::certusone::vote::id(),
        &feature::create_account(
            &Feature {
                activated_at: Some(3),
            },
            42,
        ),
    );
    bank.store_account(
        &feature_set::full_inflation::mainnet::certusone::enable::id(),
        &feature::create_account(
            &Feature {
                activated_at: Some(3),
            },
            42,
        ),
    );
    bank.feature_set = Arc::new(bank.compute_active_feature_set(true).0);
    assert_eq!(bank.get_inflation_start_slot(), 2);
}

#[test]
fn test_get_inflation_start_slot_mainnet() {
    let GenesisConfigInfo {
        mut genesis_config, ..
    } = create_genesis_config_with_leader(42, &solana_sdk::pubkey::new_rand(), 42);
    genesis_config
        .accounts
        .remove(&feature_set::pico_inflation::id())
        .unwrap();
    genesis_config
        .accounts
        .remove(&feature_set::full_inflation::devnet_and_testnet::id())
        .unwrap();
    for pair in feature_set::FULL_INFLATION_FEATURE_PAIRS.iter() {
        genesis_config.accounts.remove(&pair.vote_id).unwrap();
        genesis_config.accounts.remove(&pair.enable_id).unwrap();
    }

    let bank = Bank::new_for_tests(&genesis_config);

    // Advance slot
    let mut bank = new_from_parent(Arc::new(bank));
    bank = new_from_parent(Arc::new(bank));
    assert_eq!(bank.get_inflation_start_slot(), 0);
    assert_eq!(bank.slot(), 2);

    // Request `pico_inflation` activation
    bank.store_account(
        &feature_set::pico_inflation::id(),
        &feature::create_account(
            &Feature {
                activated_at: Some(1),
            },
            42,
        ),
    );
    bank.feature_set = Arc::new(bank.compute_active_feature_set(true).0);
    assert_eq!(bank.get_inflation_start_slot(), 1);

    // Advance slot
    bank = new_from_parent(Arc::new(bank));
    assert_eq!(bank.slot(), 3);

    // Request `full_inflation::mainnet::certusone` activation,
    // which takes priority over pico_inflation
    bank.store_account(
        &feature_set::full_inflation::mainnet::certusone::vote::id(),
        &feature::create_account(
            &Feature {
                activated_at: Some(2),
            },
            42,
        ),
    );
    bank.store_account(
        &feature_set::full_inflation::mainnet::certusone::enable::id(),
        &feature::create_account(
            &Feature {
                activated_at: Some(2),
            },
            42,
        ),
    );
    bank.feature_set = Arc::new(bank.compute_active_feature_set(true).0);
    assert_eq!(bank.get_inflation_start_slot(), 2);

    // Advance slot
    bank = new_from_parent(Arc::new(bank));
    assert_eq!(bank.slot(), 4);

    // Request `full_inflation::devnet_and_testnet` activation,
    // which should have no effect on `get_inflation_start_slot`
    bank.store_account(
        &feature_set::full_inflation::devnet_and_testnet::id(),
        &feature::create_account(
            &Feature {
                activated_at: Some(bank.slot()),
            },
            42,
        ),
    );
    bank.feature_set = Arc::new(bank.compute_active_feature_set(true).0);
    assert_eq!(bank.get_inflation_start_slot(), 2);
}

#[test]
fn test_get_inflation_num_slots_with_activations() {
    let GenesisConfigInfo {
        mut genesis_config, ..
    } = create_genesis_config_with_leader(42, &solana_sdk::pubkey::new_rand(), 42);
    let slots_per_epoch = 32;
    genesis_config.epoch_schedule = EpochSchedule::new(slots_per_epoch);
    genesis_config
        .accounts
        .remove(&feature_set::pico_inflation::id())
        .unwrap();
    genesis_config
        .accounts
        .remove(&feature_set::full_inflation::devnet_and_testnet::id())
        .unwrap();
    for pair in feature_set::FULL_INFLATION_FEATURE_PAIRS.iter() {
        genesis_config.accounts.remove(&pair.vote_id).unwrap();
        genesis_config.accounts.remove(&pair.enable_id).unwrap();
    }

    let mut bank = Bank::new_for_tests(&genesis_config);
    assert_eq!(bank.get_inflation_num_slots(), 0);
    for _ in 0..2 * slots_per_epoch {
        bank = new_from_parent(Arc::new(bank));
    }
    assert_eq!(bank.get_inflation_num_slots(), 2 * slots_per_epoch);

    // Activate pico_inflation
    let pico_inflation_activation_slot = bank.slot();
    bank.store_account(
        &feature_set::pico_inflation::id(),
        &feature::create_account(
            &Feature {
                activated_at: Some(pico_inflation_activation_slot),
            },
            42,
        ),
    );
    bank.feature_set = Arc::new(bank.compute_active_feature_set(true).0);
    assert_eq!(bank.get_inflation_num_slots(), slots_per_epoch);
    for _ in 0..slots_per_epoch {
        bank = new_from_parent(Arc::new(bank));
    }
    assert_eq!(bank.get_inflation_num_slots(), 2 * slots_per_epoch);

    // Activate full_inflation::devnet_and_testnet
    let full_inflation_activation_slot = bank.slot();
    bank.store_account(
        &feature_set::full_inflation::devnet_and_testnet::id(),
        &feature::create_account(
            &Feature {
                activated_at: Some(full_inflation_activation_slot),
            },
            42,
        ),
    );
    bank.feature_set = Arc::new(bank.compute_active_feature_set(true).0);
    assert_eq!(bank.get_inflation_num_slots(), slots_per_epoch);
    for _ in 0..slots_per_epoch {
        bank = new_from_parent(Arc::new(bank));
    }
    assert_eq!(bank.get_inflation_num_slots(), 2 * slots_per_epoch);
}

#[test]
fn test_get_inflation_num_slots_already_activated() {
    let GenesisConfigInfo {
        mut genesis_config, ..
    } = create_genesis_config_with_leader(42, &solana_sdk::pubkey::new_rand(), 42);
    let slots_per_epoch = 32;
    genesis_config.epoch_schedule = EpochSchedule::new(slots_per_epoch);
    let mut bank = Bank::new_for_tests(&genesis_config);
    assert_eq!(bank.get_inflation_num_slots(), 0);
    for _ in 0..slots_per_epoch {
        bank = new_from_parent(Arc::new(bank));
    }
    assert_eq!(bank.get_inflation_num_slots(), slots_per_epoch);
    for _ in 0..slots_per_epoch {
        bank = new_from_parent(Arc::new(bank));
    }
    assert_eq!(bank.get_inflation_num_slots(), 2 * slots_per_epoch);
}

#[test]
fn test_stake_vote_account_validity() {
    let thread_pool = ThreadPoolBuilder::new().num_threads(1).build().unwrap();
    // TODO: stakes cache should be hardened for the case when the account
    // owner is changed from vote/stake program to something else. see:
    // https://github.com/solana-labs/solana/pull/24200#discussion_r849935444
    check_stake_vote_account_validity(
        false, // check owner change
        |bank: &Bank| bank._load_vote_and_stake_accounts(&thread_pool, null_tracer()),
    );
}

#[test]
fn test_epoch_schedule_from_genesis_config() {
    let validator_vote_keypairs0 = ValidatorVoteKeypairs::new_rand();
    let validator_vote_keypairs1 = ValidatorVoteKeypairs::new_rand();
    let validator_keypairs = vec![&validator_vote_keypairs0, &validator_vote_keypairs1];
    let GenesisConfigInfo {
        mut genesis_config, ..
    } = create_genesis_config_with_vote_accounts(
        1_000_000_000,
        &validator_keypairs,
        vec![LAMPORTS_PER_SOL; 2],
    );

    genesis_config.epoch_schedule = EpochSchedule::custom(8192, 100, true);

    let bank = Arc::new(Bank::new_with_paths(
        &genesis_config,
        Arc::<RuntimeConfig>::default(),
        Vec::new(),
        None,
        None,
        false,
        Some(ACCOUNTS_DB_CONFIG_FOR_TESTING),
        None,
        None,
        Arc::default(),
        None,
        None,
    ));

    assert_eq!(bank.epoch_schedule(), &genesis_config.epoch_schedule);
}

fn check_stake_vote_account_validity<F>(check_owner_change: bool, load_vote_and_stake_accounts: F)
where
    F: Fn(&Bank) -> LoadVoteAndStakeAccountsResult,
{
    let validator_vote_keypairs0 = ValidatorVoteKeypairs::new_rand();
    let validator_vote_keypairs1 = ValidatorVoteKeypairs::new_rand();
    let validator_keypairs = vec![&validator_vote_keypairs0, &validator_vote_keypairs1];
    let GenesisConfigInfo { genesis_config, .. } = create_genesis_config_with_vote_accounts(
        1_000_000_000,
        &validator_keypairs,
        vec![LAMPORTS_PER_SOL; 2],
    );
    let bank = Arc::new(Bank::new_with_paths(
        &genesis_config,
        Arc::<RuntimeConfig>::default(),
        Vec::new(),
        None,
        None,
        false,
        Some(ACCOUNTS_DB_CONFIG_FOR_TESTING),
        None,
        None,
        Arc::default(),
        None,
        None,
    ));
    let vote_and_stake_accounts =
        load_vote_and_stake_accounts(&bank).vote_with_stake_delegations_map;
    assert_eq!(vote_and_stake_accounts.len(), 2);

    let mut vote_account = bank
        .get_account(&validator_vote_keypairs0.vote_keypair.pubkey())
        .unwrap_or_default();
    let original_lamports = vote_account.lamports();
    vote_account.set_lamports(0);
    // Simulate vote account removal via full withdrawal
    bank.store_account(
        &validator_vote_keypairs0.vote_keypair.pubkey(),
        &vote_account,
    );

    // Modify staked vote account owner; a vote account owned by another program could be
    // freely modified with malicious data
    let bogus_vote_program = Pubkey::new_unique();
    vote_account.set_lamports(original_lamports);
    vote_account.set_owner(bogus_vote_program);
    bank.store_account(
        &validator_vote_keypairs0.vote_keypair.pubkey(),
        &vote_account,
    );

    assert_eq!(bank.vote_accounts().len(), 1);

    // Modify stake account owner; a stake account owned by another program could be freely
    // modified with malicious data
    let bogus_stake_program = Pubkey::new_unique();
    let mut stake_account = bank
        .get_account(&validator_vote_keypairs1.stake_keypair.pubkey())
        .unwrap_or_default();
    stake_account.set_owner(bogus_stake_program);
    bank.store_account(
        &validator_vote_keypairs1.stake_keypair.pubkey(),
        &stake_account,
    );

    // Accounts must be valid stake and vote accounts
    let vote_and_stake_accounts =
        load_vote_and_stake_accounts(&bank).vote_with_stake_delegations_map;
    assert_eq!(
        vote_and_stake_accounts.len(),
        usize::from(!check_owner_change)
    );
}

#[test]
fn test_vote_epoch_panic() {
    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config_with_leader(
        1_000_000_000_000_000,
        &Pubkey::new_unique(),
        bootstrap_validator_stake_lamports(),
    );
    let (bank, bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);

    let vote_keypair = keypair_from_seed(&[1u8; 32]).unwrap();
    let stake_keypair = keypair_from_seed(&[2u8; 32]).unwrap();

    let mut setup_ixs = Vec::new();
    setup_ixs.extend(vote_instruction::create_account_with_config(
        &mint_keypair.pubkey(),
        &vote_keypair.pubkey(),
        &VoteInit {
            node_pubkey: mint_keypair.pubkey(),
            authorized_voter: vote_keypair.pubkey(),
            authorized_withdrawer: mint_keypair.pubkey(),
            commission: 0,
        },
        1_000_000_000,
        vote_instruction::CreateVoteAccountConfig {
            space: VoteStateVersions::vote_state_size_of(true) as u64,
            ..vote_instruction::CreateVoteAccountConfig::default()
        },
    ));
    setup_ixs.extend(stake_instruction::create_account_and_delegate_stake(
        &mint_keypair.pubkey(),
        &stake_keypair.pubkey(),
        &vote_keypair.pubkey(),
        &Authorized::auto(&mint_keypair.pubkey()),
        &Lockup::default(),
        1_000_000_000_000,
    ));
    setup_ixs.push(vote_instruction::withdraw(
        &vote_keypair.pubkey(),
        &mint_keypair.pubkey(),
        1_000_000_000,
        &mint_keypair.pubkey(),
    ));
    setup_ixs.push(system_instruction::transfer(
        &mint_keypair.pubkey(),
        &vote_keypair.pubkey(),
        1_000_000_000,
    ));

    let result = bank.process_transaction(&Transaction::new(
        &[&mint_keypair, &vote_keypair, &stake_keypair],
        Message::new(&setup_ixs, Some(&mint_keypair.pubkey())),
        bank.last_blockhash(),
    ));
    assert!(result.is_ok());

    let _bank = new_bank_from_parent_with_bank_forks(
        bank_forks.as_ref(),
        bank,
        &mint_keypair.pubkey(),
        genesis_config.epoch_schedule.get_first_slot_in_epoch(1),
    );
}

#[test]
fn test_tx_log_order() {
    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config_with_leader(
        1_000_000_000_000_000,
        &Pubkey::new_unique(),
        bootstrap_validator_stake_lamports(),
    );
    let (bank, _bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);
    *bank.transaction_log_collector_config.write().unwrap() = TransactionLogCollectorConfig {
        mentioned_addresses: HashSet::new(),
        filter: TransactionLogCollectorFilter::All,
    };
    let blockhash = bank.last_blockhash();

    let sender0 = Keypair::new();
    let sender1 = Keypair::new();
    bank.transfer(100, &mint_keypair, &sender0.pubkey())
        .unwrap();
    bank.transfer(100, &mint_keypair, &sender1.pubkey())
        .unwrap();

    let recipient0 = Pubkey::new_unique();
    let recipient1 = Pubkey::new_unique();
    let tx0 = system_transaction::transfer(&sender0, &recipient0, 10, blockhash);
    let success_sig = tx0.signatures[0];
    let tx1 = system_transaction::transfer(&sender1, &recipient1, 110, blockhash); // Should produce insufficient funds log
    let failure_sig = tx1.signatures[0];
    let tx2 = system_transaction::transfer(&sender0, &recipient0, 1, blockhash);
    let txs = vec![tx0, tx1, tx2];
    let batch = bank.prepare_batch_for_tests(txs);

    let commit_results = bank
        .load_execute_and_commit_transactions(
            &batch,
            MAX_PROCESSING_AGE,
            false,
            ExecutionRecordingConfig {
                enable_cpi_recording: false,
                enable_log_recording: true,
                enable_return_data_recording: false,
            },
            &mut ExecuteTimings::default(),
            None,
        )
        .0;

    assert_eq!(commit_results.len(), 3);

    assert!(commit_results[0].is_ok());
    assert!(commit_results[0]
        .as_ref()
        .unwrap()
        .log_messages
        .as_ref()
        .unwrap()[1]
        .contains(&"success".to_string()));
    assert!(commit_results[1].is_ok());
    assert!(commit_results[1]
        .as_ref()
        .unwrap()
        .log_messages
        .as_ref()
        .unwrap()[2]
        .contains(&"failed".to_string()));
    assert!(commit_results[2].is_err());

    let stored_logs = &bank.transaction_log_collector.read().unwrap().logs;
    let success_log_info = stored_logs
        .iter()
        .find(|transaction_log_info| transaction_log_info.signature == success_sig)
        .unwrap();
    assert!(success_log_info.result.is_ok());
    let success_log = success_log_info.log_messages.clone().pop().unwrap();
    assert!(success_log.contains(&"success".to_string()));
    let failure_log_info = stored_logs
        .iter()
        .find(|transaction_log_info| transaction_log_info.signature == failure_sig)
        .unwrap();
    assert!(failure_log_info.result.is_err());
    let failure_log = failure_log_info.log_messages.clone().pop().unwrap();
    assert!(failure_log.contains(&"failed".to_string()));
}

#[test]
fn test_tx_return_data() {
    solana_logger::setup();
    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config_with_leader(
        1_000_000_000_000_000,
        &Pubkey::new_unique(),
        bootstrap_validator_stake_lamports(),
    );
    let mock_program_id = Pubkey::from([2u8; 32]);
    let (bank, _bank_forks) =
        Bank::new_with_mockup_builtin_for_tests(&genesis_config, mock_program_id, MockBuiltin::vm);

    declare_process_instruction!(MockBuiltin, 1, |invoke_context| {
        let mock_program_id = Pubkey::from([2u8; 32]);
        let transaction_context = &mut invoke_context.transaction_context;
        let instruction_context = transaction_context.get_current_instruction_context()?;
        let instruction_data = instruction_context.get_instruction_data();
        let mut return_data = [0u8; MAX_RETURN_DATA];
        if !instruction_data.is_empty() {
            let index = usize::from_le_bytes(instruction_data.try_into().unwrap());
            return_data[index / 2] = 1;
            transaction_context
                .set_return_data(mock_program_id, return_data[..index + 1].to_vec())
                .unwrap();
        }
        Ok(())
    });

    let blockhash = bank.last_blockhash();

    for index in [
        None,
        Some(0),
        Some(MAX_RETURN_DATA / 2),
        Some(MAX_RETURN_DATA - 1),
    ] {
        let data = if let Some(index) = index {
            usize::to_le_bytes(index).to_vec()
        } else {
            Vec::new()
        };
        let txs = vec![Transaction::new_signed_with_payer(
            &[Instruction {
                program_id: mock_program_id,
                data,
                accounts: vec![AccountMeta::new(Pubkey::new_unique(), false)],
            }],
            Some(&mint_keypair.pubkey()),
            &[&mint_keypair],
            blockhash,
        )];
        let batch = bank.prepare_batch_for_tests(txs);
        let commit_results = bank
            .load_execute_and_commit_transactions(
                &batch,
                MAX_PROCESSING_AGE,
                false,
                ExecutionRecordingConfig {
                    enable_cpi_recording: false,
                    enable_log_recording: false,
                    enable_return_data_recording: true,
                },
                &mut ExecuteTimings::default(),
                None,
            )
            .0;
        let return_data = commit_results[0].as_ref().unwrap().return_data.clone();
        if let Some(index) = index {
            let return_data = return_data.unwrap();
            assert_eq!(return_data.program_id, mock_program_id);
            let mut expected_data = vec![0u8; index + 1];
            // include some trailing zeros
            expected_data[index / 2] = 1;
            assert_eq!(return_data.data, expected_data);
        } else {
            assert!(return_data.is_none());
        }
    }
}

#[test]
fn test_load_and_execute_commit_transactions_rent_debits() {
    let (mut genesis_config, mint_keypair) = create_genesis_config(sol_to_lamports(1.));
    genesis_config.rent = Rent::default();
    let (bank, _bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);
    let bank = Bank::new_from_parent(
        bank,
        &Pubkey::new_unique(),
        genesis_config.epoch_schedule.get_first_slot_in_epoch(1),
    );
    let amount = genesis_config.rent.minimum_balance(0);

    // Make sure that rent debits are tracked for successful transactions
    {
        let alice = Keypair::new();
        test_utils::deposit(&bank, &alice.pubkey(), amount - 1).unwrap();
        let tx = system_transaction::transfer(
            &mint_keypair,
            &alice.pubkey(),
            amount,
            genesis_config.hash(),
        );

        let batch = bank.prepare_batch_for_tests(vec![tx]);
        let commit_result = bank
            .load_execute_and_commit_transactions(
                &batch,
                MAX_PROCESSING_AGE,
                false,
                ExecutionRecordingConfig::new_single_setting(false),
                &mut ExecuteTimings::default(),
                None,
            )
            .0
            .remove(0);
        assert!(commit_result.is_ok());
        assert!(commit_result.was_executed_successfully());
        assert!(!commit_result.ok().unwrap().rent_debits.is_empty());
    }

    // Make sure that rent debits are ignored for failed transactions
    {
        let bob = Keypair::new();
        test_utils::deposit(&bank, &bob.pubkey(), amount - 1).unwrap();
        let tx =
            system_transaction::transfer(&mint_keypair, &bob.pubkey(), 1, genesis_config.hash());

        let batch = bank.prepare_batch_for_tests(vec![tx]);
        let commit_result = bank
            .load_execute_and_commit_transactions(
                &batch,
                MAX_PROCESSING_AGE,
                false,
                ExecutionRecordingConfig::new_single_setting(false),
                &mut ExecuteTimings::default(),
                None,
            )
            .0
            .remove(0);
        assert!(commit_result.is_ok());
        assert!(!commit_result.was_executed_successfully());
        assert!(commit_result.ok().unwrap().rent_debits.is_empty());
    }
}

#[test]
fn test_get_largest_accounts() {
    let GenesisConfigInfo { genesis_config, .. } =
        create_genesis_config_with_leader(42, &solana_sdk::pubkey::new_rand(), 42);
    let bank = Bank::new_for_tests(&genesis_config);

    let pubkeys: Vec<_> = (0..5).map(|_| Pubkey::new_unique()).collect();
    let pubkeys_hashset: HashSet<_> = pubkeys.iter().cloned().collect();

    let pubkeys_balances: Vec<_> = pubkeys
        .iter()
        .cloned()
        .zip(vec![
            sol_to_lamports(2.0),
            sol_to_lamports(3.0),
            sol_to_lamports(3.0),
            sol_to_lamports(4.0),
            sol_to_lamports(5.0),
        ])
        .collect();

    // Initialize accounts; all have larger SOL balances than current Bank built-ins
    let account0 = AccountSharedData::new(pubkeys_balances[0].1, 0, &Pubkey::default());
    bank.store_account(&pubkeys_balances[0].0, &account0);
    let account1 = AccountSharedData::new(pubkeys_balances[1].1, 0, &Pubkey::default());
    bank.store_account(&pubkeys_balances[1].0, &account1);
    let account2 = AccountSharedData::new(pubkeys_balances[2].1, 0, &Pubkey::default());
    bank.store_account(&pubkeys_balances[2].0, &account2);
    let account3 = AccountSharedData::new(pubkeys_balances[3].1, 0, &Pubkey::default());
    bank.store_account(&pubkeys_balances[3].0, &account3);
    let account4 = AccountSharedData::new(pubkeys_balances[4].1, 0, &Pubkey::default());
    bank.store_account(&pubkeys_balances[4].0, &account4);

    // Create HashSet to exclude an account
    let exclude4: HashSet<_> = pubkeys[4..].iter().cloned().collect();

    let mut sorted_accounts = pubkeys_balances.clone();
    sorted_accounts.sort_by(|a, b| a.1.cmp(&b.1).reverse());

    // Return only one largest account
    assert_eq!(
        bank.get_largest_accounts(1, &pubkeys_hashset, AccountAddressFilter::Include, false)
            .unwrap(),
        vec![(pubkeys[4], sol_to_lamports(5.0))]
    );
    assert_eq!(
        bank.get_largest_accounts(1, &HashSet::new(), AccountAddressFilter::Exclude, false)
            .unwrap(),
        vec![(pubkeys[4], sol_to_lamports(5.0))]
    );
    assert_eq!(
        bank.get_largest_accounts(1, &exclude4, AccountAddressFilter::Exclude, false)
            .unwrap(),
        vec![(pubkeys[3], sol_to_lamports(4.0))]
    );

    // Return all added accounts
    let results = bank
        .get_largest_accounts(10, &pubkeys_hashset, AccountAddressFilter::Include, false)
        .unwrap();
    assert_eq!(results.len(), sorted_accounts.len());
    for pubkey_balance in sorted_accounts.iter() {
        assert!(results.contains(pubkey_balance));
    }
    let mut sorted_results = results.clone();
    sorted_results.sort_by(|a, b| a.1.cmp(&b.1).reverse());
    assert_eq!(sorted_results, results);

    let expected_accounts = sorted_accounts[1..].to_vec();
    let results = bank
        .get_largest_accounts(10, &exclude4, AccountAddressFilter::Exclude, false)
        .unwrap();
    // results include 5 Bank builtins
    assert_eq!(results.len(), 10);
    for pubkey_balance in expected_accounts.iter() {
        assert!(results.contains(pubkey_balance));
    }
    let mut sorted_results = results.clone();
    sorted_results.sort_by(|a, b| a.1.cmp(&b.1).reverse());
    assert_eq!(sorted_results, results);

    // Return 3 added accounts
    let expected_accounts = sorted_accounts[0..4].to_vec();
    let results = bank
        .get_largest_accounts(4, &pubkeys_hashset, AccountAddressFilter::Include, false)
        .unwrap();
    assert_eq!(results.len(), expected_accounts.len());
    for pubkey_balance in expected_accounts.iter() {
        assert!(results.contains(pubkey_balance));
    }

    let expected_accounts = expected_accounts[1..4].to_vec();
    let results = bank
        .get_largest_accounts(3, &exclude4, AccountAddressFilter::Exclude, false)
        .unwrap();
    assert_eq!(results.len(), expected_accounts.len());
    for pubkey_balance in expected_accounts.iter() {
        assert!(results.contains(pubkey_balance));
    }

    // Exclude more, and non-sequential, accounts
    let exclude: HashSet<_> = [pubkeys[0], pubkeys[2], pubkeys[4]]
        .iter()
        .cloned()
        .collect();
    assert_eq!(
        bank.get_largest_accounts(2, &exclude, AccountAddressFilter::Exclude, false)
            .unwrap(),
        vec![pubkeys_balances[3], pubkeys_balances[1]]
    );
}

#[test]
fn test_transfer_sysvar() {
    solana_logger::setup();
    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config_with_leader(
        1_000_000_000_000_000,
        &Pubkey::new_unique(),
        bootstrap_validator_stake_lamports(),
    );
    let program_id = solana_sdk::pubkey::new_rand();

    let (bank, _bank_forks) =
        Bank::new_with_mockup_builtin_for_tests(&genesis_config, program_id, MockBuiltin::vm);

    declare_process_instruction!(MockBuiltin, 1, |invoke_context| {
        let transaction_context = &invoke_context.transaction_context;
        let instruction_context = transaction_context.get_current_instruction_context()?;
        instruction_context
            .try_borrow_instruction_account(transaction_context, 1)?
            .set_data(vec![0; 40])?;
        Ok(())
    });

    let blockhash = bank.last_blockhash();
    #[allow(deprecated)]
    let blockhash_sysvar = sysvar::clock::id();
    #[allow(deprecated)]
    let orig_lamports = bank.get_account(&sysvar::clock::id()).unwrap().lamports();
    let tx = system_transaction::transfer(&mint_keypair, &blockhash_sysvar, 10, blockhash);
    assert_eq!(
        bank.process_transaction(&tx),
        Err(TransactionError::InstructionError(
            0,
            InstructionError::ReadonlyLamportChange
        ))
    );
    assert_eq!(
        bank.get_account(&sysvar::clock::id()).unwrap().lamports(),
        orig_lamports
    );

    let accounts = vec![
        AccountMeta::new(mint_keypair.pubkey(), true),
        AccountMeta::new(blockhash_sysvar, false),
    ];
    let ix = Instruction::new_with_bincode(program_id, &0, accounts);
    let message = Message::new(&[ix], Some(&mint_keypair.pubkey()));
    let tx = Transaction::new(&[&mint_keypair], message, blockhash);
    assert_eq!(
        bank.process_transaction(&tx),
        Err(TransactionError::InstructionError(
            0,
            InstructionError::ReadonlyDataModified
        ))
    );
}

#[test]
fn test_clean_dropped_unrooted_frozen_banks() {
    solana_logger::setup();
    do_test_clean_dropped_unrooted_banks(FreezeBank1::Yes);
}

#[test]
fn test_clean_dropped_unrooted_unfrozen_banks() {
    solana_logger::setup();
    do_test_clean_dropped_unrooted_banks(FreezeBank1::No);
}

/// A simple enum to toggle freezing Bank1 or not.  Used in the clean_dropped_unrooted tests.
enum FreezeBank1 {
    No,
    Yes,
}

fn do_test_clean_dropped_unrooted_banks(freeze_bank1: FreezeBank1) {
    //! Test that dropped unrooted banks are cleaned up properly
    //!
    //! slot 0:       bank0 (rooted)
    //!               /   \
    //! slot 1:      /   bank1 (unrooted and dropped)
    //!             /
    //! slot 2:  bank2 (rooted)
    //!
    //! In the scenario above, when `clean_accounts()` is called on bank2, the keys that exist
    //! _only_ in bank1 should be cleaned up, since those keys are unreachable.
    //!
    //! The following scenarios are tested:
    //!
    //! 1. A key is written _only_ in an unrooted bank (key1)
    //!     - In this case, key1 should be cleaned up
    //! 2. A key is written in both an unrooted _and_ rooted bank (key3)
    //!     - In this case, key3's ref-count should be decremented correctly
    //! 3. A key with zero lamports is _only_ in an unrooted bank (key4)
    //!     - In this case, key4 should be cleaned up
    //! 4. A key with zero lamports is in both an unrooted _and_ rooted bank (key5)
    //!     - In this case, key5's ref-count should be decremented correctly

    let (genesis_config, mint_keypair) = create_genesis_config(sol_to_lamports(1.));
    let (bank0, bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);
    let amount = genesis_config.rent.minimum_balance(0);

    let collector = Pubkey::new_unique();
    let owner = Pubkey::new_unique();

    let key1 = Keypair::new(); // only touched in bank1
    let key2 = Keypair::new(); // only touched in bank2
    let key3 = Keypair::new(); // touched in both bank1 and bank2
    let key4 = Keypair::new(); // in only bank1, and has zero lamports
    let key5 = Keypair::new(); // in both bank1 and bank2, and has zero lamports
    bank0
        .transfer(amount, &mint_keypair, &key2.pubkey())
        .unwrap();
    bank0.freeze();

    let slot = 1;
    let bank1 =
        new_bank_from_parent_with_bank_forks(bank_forks.as_ref(), bank0.clone(), &collector, slot);
    add_root_and_flush_write_cache(&bank0);
    bank1
        .transfer(amount, &mint_keypair, &key1.pubkey())
        .unwrap();
    bank1.store_account(&key4.pubkey(), &AccountSharedData::new(0, 0, &owner));
    bank1.store_account(&key5.pubkey(), &AccountSharedData::new(0, 0, &owner));

    if let FreezeBank1::Yes = freeze_bank1 {
        bank1.freeze();
    }

    let slot = slot + 1;
    let bank2 = new_bank_from_parent_with_bank_forks(bank_forks.as_ref(), bank0, &collector, slot);
    bank2
        .transfer(amount * 2, &mint_keypair, &key2.pubkey())
        .unwrap();
    bank2
        .transfer(amount, &mint_keypair, &key3.pubkey())
        .unwrap();
    bank2.store_account(&key5.pubkey(), &AccountSharedData::new(0, 0, &owner));

    bank2.freeze(); // the freeze here is not strictly necessary, but more for illustration
    bank2.squash();
    add_root_and_flush_write_cache(&bank2);

    bank_forks.write().unwrap().remove(1);
    drop(bank1);
    bank2.clean_accounts_for_tests();

    let expected_ref_count_for_cleaned_up_keys = 0;
    let expected_ref_count_for_keys_in_both_slot1_and_slot2 = 1;

    assert_eq!(
        bank2
            .rc
            .accounts
            .accounts_db
            .accounts_index
            .ref_count_from_storage(&key1.pubkey()),
        expected_ref_count_for_cleaned_up_keys
    );
    assert_ne!(
        bank2
            .rc
            .accounts
            .accounts_db
            .accounts_index
            .ref_count_from_storage(&key3.pubkey()),
        expected_ref_count_for_cleaned_up_keys
    );
    assert_eq!(
        bank2
            .rc
            .accounts
            .accounts_db
            .accounts_index
            .ref_count_from_storage(&key4.pubkey()),
        expected_ref_count_for_cleaned_up_keys
    );
    assert_eq!(
        bank2
            .rc
            .accounts
            .accounts_db
            .accounts_index
            .ref_count_from_storage(&key5.pubkey()),
        expected_ref_count_for_keys_in_both_slot1_and_slot2,
    );

    assert_eq!(
        bank2.rc.accounts.accounts_db.alive_account_count_in_slot(1),
        0
    );
}

#[test]
fn test_rent_debits() {
    let mut rent_debits = RentDebits::default();

    // No entry for 0 rewards
    rent_debits.insert(&Pubkey::new_unique(), 0, 0);
    assert_eq!(rent_debits.len(), 0);

    // Some that actually work
    rent_debits.insert(&Pubkey::new_unique(), 1, 0);
    assert_eq!(rent_debits.len(), 1);
    rent_debits.insert(&Pubkey::new_unique(), i64::MAX as u64, 0);
    assert_eq!(rent_debits.len(), 2);
}

#[test]
fn test_compute_budget_program_noop() {
    solana_logger::setup();
    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config_with_leader(
        1_000_000_000_000_000,
        &Pubkey::new_unique(),
        bootstrap_validator_stake_lamports(),
    );
    let program_id = solana_sdk::pubkey::new_rand();
    let (bank, _bank_forks) =
        Bank::new_with_mockup_builtin_for_tests(&genesis_config, program_id, MockBuiltin::vm);

    declare_process_instruction!(MockBuiltin, 1, |invoke_context| {
        let compute_budget = invoke_context.get_compute_budget();
        assert_eq!(
            *compute_budget,
            ComputeBudget {
                compute_unit_limit: u64::from(
                    compute_budget_limits::DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT
                ),
                heap_size: 48 * 1024,
                ..ComputeBudget::default()
            }
        );
        Ok(())
    });

    let message = Message::new(
        &[
            ComputeBudgetInstruction::set_compute_unit_limit(
                compute_budget_limits::DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT,
            ),
            ComputeBudgetInstruction::request_heap_frame(48 * 1024),
            Instruction::new_with_bincode(program_id, &0, vec![]),
        ],
        Some(&mint_keypair.pubkey()),
    );
    let tx = Transaction::new(&[&mint_keypair], message, bank.last_blockhash());
    bank.process_transaction(&tx).unwrap();
}

#[test]
fn test_compute_request_instruction() {
    solana_logger::setup();
    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config_with_leader(
        1_000_000_000_000_000,
        &Pubkey::new_unique(),
        bootstrap_validator_stake_lamports(),
    );
    let program_id = solana_sdk::pubkey::new_rand();
    let (bank, _bank_forks) =
        Bank::new_with_mockup_builtin_for_tests(&genesis_config, program_id, MockBuiltin::vm);

    declare_process_instruction!(MockBuiltin, 1, |invoke_context| {
        let compute_budget = invoke_context.get_compute_budget();
        assert_eq!(
            *compute_budget,
            ComputeBudget {
                compute_unit_limit: u64::from(
                    compute_budget_limits::DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT
                ),
                heap_size: 48 * 1024,
                ..ComputeBudget::default()
            }
        );
        Ok(())
    });

    let message = Message::new(
        &[
            ComputeBudgetInstruction::set_compute_unit_limit(
                compute_budget_limits::DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT,
            ),
            ComputeBudgetInstruction::request_heap_frame(48 * 1024),
            Instruction::new_with_bincode(program_id, &0, vec![]),
        ],
        Some(&mint_keypair.pubkey()),
    );
    let tx = Transaction::new(&[&mint_keypair], message, bank.last_blockhash());
    bank.process_transaction(&tx).unwrap();
}

#[test]
fn test_failed_compute_request_instruction() {
    solana_logger::setup();
    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config_with_leader(
        1_000_000_000_000_000,
        &Pubkey::new_unique(),
        bootstrap_validator_stake_lamports(),
    );

    let program_id = solana_sdk::pubkey::new_rand();
    let (bank, _bank_forks) =
        Bank::new_with_mockup_builtin_for_tests(&genesis_config, program_id, MockBuiltin::vm);

    let payer0_keypair = Keypair::new();
    let payer1_keypair = Keypair::new();
    bank.transfer(10, &mint_keypair, &payer0_keypair.pubkey())
        .unwrap();
    bank.transfer(10, &mint_keypair, &payer1_keypair.pubkey())
        .unwrap();

    declare_process_instruction!(MockBuiltin, 1, |invoke_context| {
        let compute_budget = invoke_context.get_compute_budget();
        assert_eq!(
            *compute_budget,
            ComputeBudget {
                compute_unit_limit: u64::from(
                    compute_budget_limits::DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT
                ),
                heap_size: 48 * 1024,
                ..ComputeBudget::default()
            }
        );
        Ok(())
    });

    // This message will not be executed because the compute budget request is invalid
    let message0 = Message::new(
        &[
            ComputeBudgetInstruction::request_heap_frame(1),
            Instruction::new_with_bincode(program_id, &0, vec![]),
        ],
        Some(&payer0_keypair.pubkey()),
    );
    // This message will be processed successfully
    let message1 = Message::new(
        &[
            ComputeBudgetInstruction::set_compute_unit_limit(1),
            ComputeBudgetInstruction::request_heap_frame(48 * 1024),
            Instruction::new_with_bincode(program_id, &0, vec![]),
        ],
        Some(&payer1_keypair.pubkey()),
    );
    let txs = vec![
        Transaction::new(&[&payer0_keypair], message0, bank.last_blockhash()),
        Transaction::new(&[&payer1_keypair], message1, bank.last_blockhash()),
    ];
    let results = bank.process_transactions(txs.iter());

    assert_eq!(
        results[0],
        Err(TransactionError::InstructionError(
            0,
            InstructionError::InvalidInstructionData
        ))
    );
    assert_eq!(results[1], Ok(()));
    // two transfers and the mock program
    assert_eq!(bank.signature_count(), 3);
}

#[test]
fn test_verify_and_hash_transaction_sig_len() {
    let GenesisConfigInfo {
        mut genesis_config, ..
    } = create_genesis_config_with_leader(42, &solana_sdk::pubkey::new_rand(), 42);

    // activate all features
    activate_all_features(&mut genesis_config);
    let bank = Bank::new_for_tests(&genesis_config);

    let recent_blockhash = Hash::new_unique();
    let from_keypair = Keypair::new();
    let to_keypair = Keypair::new();
    let from_pubkey = from_keypair.pubkey();
    let to_pubkey = to_keypair.pubkey();

    enum TestCase {
        AddSignature,
        RemoveSignature,
    }

    let make_transaction = |case: TestCase| {
        let message = Message::new(
            &[system_instruction::transfer(&from_pubkey, &to_pubkey, 1)],
            Some(&from_pubkey),
        );
        let mut tx = Transaction::new(&[&from_keypair], message, recent_blockhash);
        assert_eq!(tx.message.header.num_required_signatures, 1);
        match case {
            TestCase::AddSignature => {
                let signature = to_keypair.sign_message(&tx.message.serialize());
                tx.signatures.push(signature);
            }
            TestCase::RemoveSignature => {
                tx.signatures.remove(0);
            }
        }
        tx
    };

    // Too few signatures: Sanitization failure
    {
        let tx = make_transaction(TestCase::RemoveSignature);
        assert_eq!(
            bank.verify_transaction(tx.into(), TransactionVerificationMode::FullVerification),
            Err(TransactionError::SanitizeFailure),
        );
    }
    // Too many signatures: Sanitization failure
    {
        let tx = make_transaction(TestCase::AddSignature);
        assert_eq!(
            bank.verify_transaction(tx.into(), TransactionVerificationMode::FullVerification),
            Err(TransactionError::SanitizeFailure),
        );
    }
}

#[test]
fn test_verify_transactions_packet_data_size() {
    let GenesisConfigInfo { genesis_config, .. } =
        create_genesis_config_with_leader(42, &solana_sdk::pubkey::new_rand(), 42);
    let bank = Bank::new_for_tests(&genesis_config);

    let recent_blockhash = Hash::new_unique();
    let keypair = Keypair::new();
    let pubkey = keypair.pubkey();
    let make_transaction = |size| {
        let ixs: Vec<_> = std::iter::repeat_with(|| {
            system_instruction::transfer(&pubkey, &Pubkey::new_unique(), 1)
        })
        .take(size)
        .collect();
        let message = Message::new(&ixs[..], Some(&pubkey));
        Transaction::new(&[&keypair], message, recent_blockhash)
    };
    // Small transaction.
    {
        let tx = make_transaction(5);
        assert!(bincode::serialized_size(&tx).unwrap() <= PACKET_DATA_SIZE as u64);
        assert!(bank
            .verify_transaction(tx.into(), TransactionVerificationMode::FullVerification)
            .is_ok(),);
    }
    // Big transaction.
    {
        let tx = make_transaction(25);
        assert!(bincode::serialized_size(&tx).unwrap() > PACKET_DATA_SIZE as u64);
        assert_eq!(
            bank.verify_transaction(tx.into(), TransactionVerificationMode::FullVerification),
            Err(TransactionError::SanitizeFailure),
        );
    }
    // Assert that verify fails as soon as serialized
    // size exceeds packet data size.
    for size in 1..30 {
        let tx = make_transaction(size);
        assert_eq!(
            bincode::serialized_size(&tx).unwrap() <= PACKET_DATA_SIZE as u64,
            bank.verify_transaction(tx.into(), TransactionVerificationMode::FullVerification)
                .is_ok(),
        );
    }
}

#[test]
fn test_check_reserved_keys() {
    let (genesis_config, _mint_keypair) = create_genesis_config(1);
    let bank = Bank::new_for_tests(&genesis_config);
    let mut bank = Bank::new_from_parent(Arc::new(bank), &Pubkey::new_unique(), 1);

    let transaction =
        SanitizedTransaction::from_transaction_for_tests(system_transaction::transfer(
            &Keypair::new(),
            &Pubkey::new_unique(),
            1,
            genesis_config.hash(),
        ));

    assert_eq!(bank.check_reserved_keys(&transaction), Ok(()));

    Arc::make_mut(&mut bank.reserved_account_keys)
        .active
        .insert(transaction.account_keys()[1]);
    assert_eq!(
        bank.check_reserved_keys(&transaction),
        Err(TransactionError::ResanitizationNeeded)
    );
}

#[test]
fn test_call_precomiled_program() {
    let GenesisConfigInfo {
        mut genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config_with_leader(42, &Pubkey::new_unique(), 42);
    activate_all_features(&mut genesis_config);
    let (bank, _bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);

    // libsecp256k1
    // Since libsecp256k1 is still using the old version of rand, this test
    // copies the `random` implementation at:
    // https://docs.rs/libsecp256k1/latest/src/libsecp256k1/lib.rs.html#430
    let secp_privkey = {
        use rand::RngCore;
        let mut rng = rand::thread_rng();
        loop {
            let mut ret = [0u8; libsecp256k1::util::SECRET_KEY_SIZE];
            rng.fill_bytes(&mut ret);
            if let Ok(key) = libsecp256k1::SecretKey::parse(&ret) {
                break key;
            }
        }
    };
    let message_arr = b"hello";
    let instruction =
        solana_sdk::secp256k1_instruction::new_secp256k1_instruction(&secp_privkey, message_arr);
    let tx = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&mint_keypair.pubkey()),
        &[&mint_keypair],
        bank.last_blockhash(),
    );
    // calling the program should be successful when called from the bank
    // even if the program itself is not called
    bank.process_transaction(&tx).unwrap();

    // ed25519
    // Since ed25519_dalek is still using the old version of rand, this test
    // copies the `generate` implementation at:
    // https://docs.rs/ed25519-dalek/1.0.1/src/ed25519_dalek/secret.rs.html#167
    let privkey = {
        use rand::RngCore;
        let mut rng = rand::thread_rng();
        let mut seed = [0u8; ed25519_dalek::SECRET_KEY_LENGTH];
        rng.fill_bytes(&mut seed);
        let secret =
            ed25519_dalek::SecretKey::from_bytes(&seed[..ed25519_dalek::SECRET_KEY_LENGTH])
                .unwrap();
        let public = ed25519_dalek::PublicKey::from(&secret);
        ed25519_dalek::Keypair { secret, public }
    };
    let message_arr = b"hello";
    let instruction =
        solana_sdk::ed25519_instruction::new_ed25519_instruction(&privkey, message_arr);
    let tx = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&mint_keypair.pubkey()),
        &[&mint_keypair],
        bank.last_blockhash(),
    );
    // calling the program should be successful when called from the bank
    // even if the program itself is not called
    bank.process_transaction(&tx).unwrap();
}

fn calculate_test_fee(
    message: &impl SVMMessage,
    lamports_per_signature: u64,
    fee_structure: &FeeStructure,
) -> u64 {
    let fee_budget_limits = FeeBudgetLimits::from(
        process_compute_budget_instructions(message.program_instructions_iter())
            .unwrap_or_default(),
    );
    solana_fee::calculate_fee(
        message,
        lamports_per_signature == 0,
        fee_structure.lamports_per_signature,
        fee_budget_limits.prioritization_fee,
        true,
    )
}

#[test]
fn test_calculate_fee() {
    // Default: no fee.
    let message = new_sanitized_message(Message::new(&[], Some(&Pubkey::new_unique())));
    assert_eq!(
        calculate_test_fee(
            &message,
            0,
            &FeeStructure {
                lamports_per_signature: 0,
                ..FeeStructure::default()
            },
        ),
        0
    );

    // One signature, a fee.
    assert_eq!(
        calculate_test_fee(
            &message,
            1,
            &FeeStructure {
                lamports_per_signature: 1,
                ..FeeStructure::default()
            },
        ),
        1
    );

    // Two signatures, double the fee.
    let key0 = Pubkey::new_unique();
    let key1 = Pubkey::new_unique();
    let ix0 = system_instruction::transfer(&key0, &key1, 1);
    let ix1 = system_instruction::transfer(&key1, &key0, 1);
    let message = new_sanitized_message(Message::new(&[ix0, ix1], Some(&key0)));
    assert_eq!(
        calculate_test_fee(
            &message,
            2,
            &FeeStructure {
                lamports_per_signature: 2,
                ..FeeStructure::default()
            },
        ),
        4
    );
}

#[test]
fn test_calculate_fee_compute_units() {
    let fee_structure = FeeStructure {
        lamports_per_signature: 1,
        ..FeeStructure::default()
    };
    let max_fee = fee_structure.compute_fee_bins.last().unwrap().fee;
    let lamports_per_signature = fee_structure.lamports_per_signature;

    // One signature, no unit request

    let message = new_sanitized_message(Message::new(&[], Some(&Pubkey::new_unique())));
    assert_eq!(
        calculate_test_fee(&message, 1, &fee_structure,),
        max_fee + lamports_per_signature
    );

    // Three signatures, two instructions, no unit request

    let ix0 = system_instruction::transfer(&Pubkey::new_unique(), &Pubkey::new_unique(), 1);
    let ix1 = system_instruction::transfer(&Pubkey::new_unique(), &Pubkey::new_unique(), 1);
    let message = new_sanitized_message(Message::new(&[ix0, ix1], Some(&Pubkey::new_unique())));
    assert_eq!(
        calculate_test_fee(&message, 1, &fee_structure,),
        max_fee + 3 * lamports_per_signature
    );

    // Explicit fee schedule

    for requested_compute_units in [
        0,
        5_000,
        10_000,
        100_000,
        300_000,
        500_000,
        700_000,
        900_000,
        1_100_000,
        1_300_000,
        MAX_COMPUTE_UNIT_LIMIT,
    ] {
        const PRIORITIZATION_FEE_RATE: u64 = 42;
        let prioritization_fee_details = PrioritizationFeeDetails::new(
            PrioritizationFeeType::ComputeUnitPrice(PRIORITIZATION_FEE_RATE),
            requested_compute_units as u64,
        );
        let message = new_sanitized_message(Message::new(
            &[
                ComputeBudgetInstruction::set_compute_unit_limit(requested_compute_units),
                ComputeBudgetInstruction::set_compute_unit_price(PRIORITIZATION_FEE_RATE),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
            ],
            Some(&Pubkey::new_unique()),
        ));
        let fee = calculate_test_fee(&message, 1, &fee_structure);
        assert_eq!(
            fee,
            lamports_per_signature + prioritization_fee_details.get_fee()
        );
    }
}

#[test]
fn test_calculate_prioritization_fee() {
    let fee_structure = FeeStructure {
        lamports_per_signature: 1,
        ..FeeStructure::default()
    };

    let request_units = 1_000_000_u32;
    let request_unit_price = 2_000_000_000_u64;
    let prioritization_fee_details = PrioritizationFeeDetails::new(
        PrioritizationFeeType::ComputeUnitPrice(request_unit_price),
        request_units as u64,
    );
    let prioritization_fee = prioritization_fee_details.get_fee();

    let message = new_sanitized_message(Message::new(
        &[
            ComputeBudgetInstruction::set_compute_unit_limit(request_units),
            ComputeBudgetInstruction::set_compute_unit_price(request_unit_price),
        ],
        Some(&Pubkey::new_unique()),
    ));

    let fee = calculate_test_fee(
        &message,
        fee_structure.lamports_per_signature,
        &fee_structure,
    );
    assert_eq!(
        fee,
        fee_structure.lamports_per_signature + prioritization_fee
    );
}

#[test]
fn test_calculate_fee_secp256k1() {
    let fee_structure = FeeStructure {
        lamports_per_signature: 1,
        ..FeeStructure::default()
    };
    let key0 = Pubkey::new_unique();
    let key1 = Pubkey::new_unique();
    let ix0 = system_instruction::transfer(&key0, &key1, 1);

    let mut secp_instruction1 = Instruction {
        program_id: secp256k1_program::id(),
        accounts: vec![],
        data: vec![],
    };
    let mut secp_instruction2 = Instruction {
        program_id: secp256k1_program::id(),
        accounts: vec![],
        data: vec![1],
    };

    let message = new_sanitized_message(Message::new(
        &[
            ix0.clone(),
            secp_instruction1.clone(),
            secp_instruction2.clone(),
        ],
        Some(&key0),
    ));
    assert_eq!(calculate_test_fee(&message, 1, &fee_structure,), 2);

    secp_instruction1.data = vec![0];
    secp_instruction2.data = vec![10];
    let message = new_sanitized_message(Message::new(
        &[ix0, secp_instruction1, secp_instruction2],
        Some(&key0),
    ));
    assert_eq!(calculate_test_fee(&message, 1, &fee_structure,), 11);
}

#[test]
fn test_an_empty_instruction_without_program() {
    let (genesis_config, mint_keypair) = create_genesis_config_no_tx_fee_no_rent(1);
    let destination = solana_sdk::pubkey::new_rand();
    let mut ix = system_instruction::transfer(&mint_keypair.pubkey(), &destination, 0);
    ix.program_id = native_loader::id(); // Empty executable account chain
    let message = Message::new(&[ix], Some(&mint_keypair.pubkey()));
    let tx = Transaction::new(&[&mint_keypair], message, genesis_config.hash());

    let (bank, _bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);
    assert_eq!(
        bank.process_transaction(&tx).unwrap_err(),
        TransactionError::InstructionError(0, InstructionError::UnsupportedProgramId),
    );
}

#[test]
fn test_transaction_log_collector_get_logs_for_address() {
    let address = Pubkey::new_unique();
    let mut mentioned_address_map = HashMap::new();
    mentioned_address_map.insert(address, vec![0]);
    let transaction_log_collector = TransactionLogCollector {
        mentioned_address_map,
        ..TransactionLogCollector::default()
    };
    assert_eq!(
        transaction_log_collector.get_logs_for_address(Some(&address)),
        Some(Vec::<TransactionLogInfo>::new()),
    );
}

/// Test processing a good transaction correctly modifies the accounts data size
#[test]
fn test_accounts_data_size_with_good_transaction() {
    const ACCOUNT_SIZE: u64 = MAX_PERMITTED_DATA_LENGTH;
    let (genesis_config, mint_keypair) = create_genesis_config(1_000 * LAMPORTS_PER_SOL);
    let bank = Bank::new_for_tests(&genesis_config);
    let (bank, _bank_forks) = bank.wrap_with_bank_forks_for_tests();
    let transaction = system_transaction::create_account(
        &mint_keypair,
        &Keypair::new(),
        bank.last_blockhash(),
        genesis_config
            .rent
            .minimum_balance(ACCOUNT_SIZE.try_into().unwrap()),
        ACCOUNT_SIZE,
        &solana_sdk::system_program::id(),
    );

    let accounts_data_size_before = bank.load_accounts_data_size();
    let accounts_data_size_delta_before = bank.load_accounts_data_size_delta();
    let accounts_data_size_delta_on_chain_before = bank.load_accounts_data_size_delta_on_chain();
    let result = bank.process_transaction(&transaction);
    let accounts_data_size_after = bank.load_accounts_data_size();
    let accounts_data_size_delta_after = bank.load_accounts_data_size_delta();
    let accounts_data_size_delta_on_chain_after = bank.load_accounts_data_size_delta_on_chain();

    assert!(result.is_ok());
    assert_eq!(
        accounts_data_size_after - accounts_data_size_before,
        ACCOUNT_SIZE,
    );
    assert_eq!(
        accounts_data_size_delta_after - accounts_data_size_delta_before,
        ACCOUNT_SIZE as i64,
    );
    assert_eq!(
        accounts_data_size_delta_on_chain_after - accounts_data_size_delta_on_chain_before,
        ACCOUNT_SIZE as i64,
    );
}

/// Test processing a bad transaction correctly modifies the accounts data size
#[test]
fn test_accounts_data_size_with_bad_transaction() {
    const ACCOUNT_SIZE: u64 = MAX_PERMITTED_DATA_LENGTH;
    let (genesis_config, _mint_keypair) = create_genesis_config(1_000 * LAMPORTS_PER_SOL);
    let bank = Bank::new_for_tests(&genesis_config);
    let (bank, _bank_forks) = bank.wrap_with_bank_forks_for_tests();
    let transaction = system_transaction::create_account(
        &Keypair::new(),
        &Keypair::new(),
        bank.last_blockhash(),
        LAMPORTS_PER_SOL,
        ACCOUNT_SIZE,
        &solana_sdk::system_program::id(),
    );

    let accounts_data_size_before = bank.load_accounts_data_size();
    let accounts_data_size_delta_before = bank.load_accounts_data_size_delta();
    let accounts_data_size_delta_on_chain_before = bank.load_accounts_data_size_delta_on_chain();
    let result = bank.process_transaction(&transaction);
    let accounts_data_size_after = bank.load_accounts_data_size();
    let accounts_data_size_delta_after = bank.load_accounts_data_size_delta();
    let accounts_data_size_delta_on_chain_after = bank.load_accounts_data_size_delta_on_chain();

    assert!(result.is_err());
    assert_eq!(accounts_data_size_after, accounts_data_size_before,);
    assert_eq!(
        accounts_data_size_delta_after,
        accounts_data_size_delta_before,
    );
    assert_eq!(
        accounts_data_size_delta_on_chain_after,
        accounts_data_size_delta_on_chain_before,
    );
}

#[derive(Serialize, Deserialize)]
enum MockTransferInstruction {
    Transfer(u64),
}

declare_process_instruction!(MockTransferBuiltin, 1, |invoke_context| {
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;
    let instruction_data = instruction_context.get_instruction_data();
    if let Ok(instruction) = bincode::deserialize(instruction_data) {
        match instruction {
            MockTransferInstruction::Transfer(amount) => {
                instruction_context
                    .try_borrow_instruction_account(transaction_context, 1)?
                    .checked_sub_lamports(amount)?;
                instruction_context
                    .try_borrow_instruction_account(transaction_context, 2)?
                    .checked_add_lamports(amount)?;
                Ok(())
            }
        }
    } else {
        Err(InstructionError::InvalidInstructionData)
    }
});

fn create_mock_transfer(
    payer: &Keypair,
    from: &Keypair,
    to: &Keypair,
    amount: u64,
    mock_program_id: Pubkey,
    recent_blockhash: Hash,
) -> Transaction {
    let account_metas = vec![
        AccountMeta::new(payer.pubkey(), true),
        AccountMeta::new(from.pubkey(), true),
        AccountMeta::new(to.pubkey(), true),
    ];
    let transfer_instruction = Instruction::new_with_bincode(
        mock_program_id,
        &MockTransferInstruction::Transfer(amount),
        account_metas,
    );
    Transaction::new_signed_with_payer(
        &[transfer_instruction],
        Some(&payer.pubkey()),
        &[payer, from, to],
        recent_blockhash,
    )
}

#[test]
fn test_invalid_rent_state_changes_existing_accounts() {
    let GenesisConfigInfo {
        mut genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config_with_leader(sol_to_lamports(100.), &Pubkey::new_unique(), 42);
    genesis_config.rent = Rent::default();

    let mock_program_id = Pubkey::new_unique();
    let account_data_size = 100;
    let rent_exempt_minimum = genesis_config.rent.minimum_balance(account_data_size);

    // Create legacy accounts of various kinds
    let rent_paying_account = Keypair::new();
    genesis_config.accounts.insert(
        rent_paying_account.pubkey(),
        Account::new_rent_epoch(
            rent_exempt_minimum - 1,
            account_data_size,
            &mock_program_id,
            INITIAL_RENT_EPOCH + 1,
        ),
    );
    let rent_exempt_account = Keypair::new();
    genesis_config.accounts.insert(
        rent_exempt_account.pubkey(),
        Account::new_rent_epoch(
            rent_exempt_minimum,
            account_data_size,
            &mock_program_id,
            INITIAL_RENT_EPOCH + 1,
        ),
    );

    let (bank, _bank_forks) = Bank::new_with_mockup_builtin_for_tests(
        &genesis_config,
        mock_program_id,
        MockTransferBuiltin::vm,
    );
    let recent_blockhash = bank.last_blockhash();

    let check_account_is_rent_exempt = |pubkey: &Pubkey| -> bool {
        let account = bank.get_account(pubkey).unwrap();
        Rent::default().is_exempt(account.lamports(), account.data().len())
    };

    // RentPaying account can be left as Uninitialized, in other RentPaying states, or RentExempt
    let tx = create_mock_transfer(
        &mint_keypair,        // payer
        &rent_paying_account, // from
        &mint_keypair,        // to
        1,
        mock_program_id,
        recent_blockhash,
    );
    let result = bank.process_transaction(&tx);
    assert!(result.is_ok());
    assert!(!check_account_is_rent_exempt(&rent_paying_account.pubkey()));
    let tx = create_mock_transfer(
        &mint_keypair,        // payer
        &rent_paying_account, // from
        &mint_keypair,        // to
        rent_exempt_minimum - 2,
        mock_program_id,
        recent_blockhash,
    );
    let result = bank.process_transaction(&tx);
    assert!(result.is_ok());
    assert!(bank.get_account(&rent_paying_account.pubkey()).is_none());

    bank.store_account(
        // restore program-owned account
        &rent_paying_account.pubkey(),
        &AccountSharedData::new(rent_exempt_minimum - 1, account_data_size, &mock_program_id),
    );
    let result = bank.transfer(1, &mint_keypair, &rent_paying_account.pubkey());
    assert!(result.is_ok());
    assert!(check_account_is_rent_exempt(&rent_paying_account.pubkey()));

    // RentExempt account can only remain RentExempt or be Uninitialized
    let tx = create_mock_transfer(
        &mint_keypair,        // payer
        &rent_exempt_account, // from
        &mint_keypair,        // to
        1,
        mock_program_id,
        recent_blockhash,
    );
    let result = bank.process_transaction(&tx);
    assert!(result.is_err());
    assert!(check_account_is_rent_exempt(&rent_exempt_account.pubkey()));
    let result = bank.transfer(1, &mint_keypair, &rent_exempt_account.pubkey());
    assert!(result.is_ok());
    assert!(check_account_is_rent_exempt(&rent_exempt_account.pubkey()));
    let tx = create_mock_transfer(
        &mint_keypair,        // payer
        &rent_exempt_account, // from
        &mint_keypair,        // to
        rent_exempt_minimum + 1,
        mock_program_id,
        recent_blockhash,
    );
    let result = bank.process_transaction(&tx);
    assert!(result.is_ok());
    assert!(bank.get_account(&rent_exempt_account.pubkey()).is_none());
}

#[test]
fn test_invalid_rent_state_changes_new_accounts() {
    let GenesisConfigInfo {
        mut genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config_with_leader(sol_to_lamports(100.), &Pubkey::new_unique(), 42);
    genesis_config.rent = Rent::default();

    let mock_program_id = Pubkey::new_unique();
    let account_data_size = 100;
    let rent_exempt_minimum = genesis_config.rent.minimum_balance(account_data_size);

    let (bank, _bank_forks) = Bank::new_with_mockup_builtin_for_tests(
        &genesis_config,
        mock_program_id,
        MockTransferBuiltin::vm,
    );
    let recent_blockhash = bank.last_blockhash();

    let check_account_is_rent_exempt = |pubkey: &Pubkey| -> bool {
        let account = bank.get_account(pubkey).unwrap();
        Rent::default().is_exempt(account.lamports(), account.data().len())
    };

    // Try to create RentPaying account
    let rent_paying_account = Keypair::new();
    let tx = system_transaction::create_account(
        &mint_keypair,
        &rent_paying_account,
        recent_blockhash,
        rent_exempt_minimum - 1,
        account_data_size as u64,
        &mock_program_id,
    );
    let result = bank.process_transaction(&tx);
    assert!(result.is_err());
    assert!(bank.get_account(&rent_paying_account.pubkey()).is_none());

    // Try to create RentExempt account
    let rent_exempt_account = Keypair::new();
    let tx = system_transaction::create_account(
        &mint_keypair,
        &rent_exempt_account,
        recent_blockhash,
        rent_exempt_minimum,
        account_data_size as u64,
        &mock_program_id,
    );
    let result = bank.process_transaction(&tx);
    assert!(result.is_ok());
    assert!(check_account_is_rent_exempt(&rent_exempt_account.pubkey()));
}

#[test]
fn test_drained_created_account() {
    let GenesisConfigInfo {
        mut genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config_with_leader(sol_to_lamports(100.), &Pubkey::new_unique(), 42);
    genesis_config.rent = Rent::default();
    activate_all_features(&mut genesis_config);

    let mock_program_id = Pubkey::new_unique();
    // small enough to not pay rent, thus bypassing the data clearing rent
    // mechanism
    let data_size_no_rent = 100;
    // large enough to pay rent, will have data cleared
    let data_size_rent = 10000;
    let lamports_to_transfer = 100;

    // Create legacy accounts of various kinds
    let created_keypair = Keypair::new();

    let (bank, _bank_forks) = Bank::new_with_mockup_builtin_for_tests(
        &genesis_config,
        mock_program_id,
        MockTransferBuiltin::vm,
    );
    let recent_blockhash = bank.last_blockhash();

    // Create and drain a small data size account
    let create_instruction = system_instruction::create_account(
        &mint_keypair.pubkey(),
        &created_keypair.pubkey(),
        lamports_to_transfer,
        data_size_no_rent,
        &mock_program_id,
    );
    let account_metas = vec![
        AccountMeta::new(mint_keypair.pubkey(), true),
        AccountMeta::new(created_keypair.pubkey(), true),
        AccountMeta::new(mint_keypair.pubkey(), false),
    ];
    let transfer_from_instruction = Instruction::new_with_bincode(
        mock_program_id,
        &MockTransferInstruction::Transfer(lamports_to_transfer),
        account_metas,
    );
    let tx = Transaction::new_signed_with_payer(
        &[create_instruction, transfer_from_instruction],
        Some(&mint_keypair.pubkey()),
        &[&mint_keypair, &created_keypair],
        recent_blockhash,
    );

    let result = bank.process_transaction(&tx);
    assert!(result.is_ok());
    // account data is not stored because of zero balance even though its
    // data wasn't cleared
    assert!(bank.get_account(&created_keypair.pubkey()).is_none());

    // Create and drain a large data size account
    let create_instruction = system_instruction::create_account(
        &mint_keypair.pubkey(),
        &created_keypair.pubkey(),
        lamports_to_transfer,
        data_size_rent,
        &mock_program_id,
    );
    let account_metas = vec![
        AccountMeta::new(mint_keypair.pubkey(), true),
        AccountMeta::new(created_keypair.pubkey(), true),
        AccountMeta::new(mint_keypair.pubkey(), false),
    ];
    let transfer_from_instruction = Instruction::new_with_bincode(
        mock_program_id,
        &MockTransferInstruction::Transfer(lamports_to_transfer),
        account_metas,
    );
    let tx = Transaction::new_signed_with_payer(
        &[create_instruction, transfer_from_instruction],
        Some(&mint_keypair.pubkey()),
        &[&mint_keypair, &created_keypair],
        recent_blockhash,
    );

    let result = bank.process_transaction(&tx);
    assert!(result.is_ok());
    // account data is not stored because of zero balance
    assert!(bank.get_account(&created_keypair.pubkey()).is_none());
}

#[test]
fn test_rent_state_changes_sysvars() {
    let GenesisConfigInfo {
        mut genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config_with_leader(sol_to_lamports(100.), &Pubkey::new_unique(), 42);
    genesis_config.rent = Rent::default();

    let validator_pubkey = solana_sdk::pubkey::new_rand();
    let validator_stake_lamports = sol_to_lamports(1.);
    let validator_staking_keypair = Keypair::new();
    let validator_voting_keypair = Keypair::new();

    let validator_vote_account = vote_state::create_account(
        &validator_voting_keypair.pubkey(),
        &validator_pubkey,
        0,
        validator_stake_lamports,
    );

    let validator_stake_account = stake_state::create_account(
        &validator_staking_keypair.pubkey(),
        &validator_voting_keypair.pubkey(),
        &validator_vote_account,
        &genesis_config.rent,
        validator_stake_lamports,
    );

    genesis_config.accounts.insert(
        validator_pubkey,
        Account::new(
            genesis_config.rent.minimum_balance(0),
            0,
            &system_program::id(),
        ),
    );
    genesis_config.accounts.insert(
        validator_staking_keypair.pubkey(),
        Account::from(validator_stake_account),
    );
    genesis_config.accounts.insert(
        validator_voting_keypair.pubkey(),
        Account::from(validator_vote_account),
    );

    let (bank, _bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);

    // Ensure transactions with sysvars succeed, even though sysvars appear RentPaying by balance
    let tx = Transaction::new_signed_with_payer(
        &[stake_instruction::deactivate_stake(
            &validator_staking_keypair.pubkey(),
            &validator_staking_keypair.pubkey(),
        )],
        Some(&mint_keypair.pubkey()),
        &[&mint_keypair, &validator_staking_keypair],
        bank.last_blockhash(),
    );
    let result = bank.process_transaction(&tx);
    assert!(result.is_ok());
}

#[test]
fn test_invalid_rent_state_changes_fee_payer() {
    let GenesisConfigInfo {
        mut genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config_with_leader(sol_to_lamports(100.), &Pubkey::new_unique(), 42);
    genesis_config.rent = Rent::default();
    genesis_config.fee_rate_governor = FeeRateGovernor::new(
        solana_sdk::fee_calculator::DEFAULT_TARGET_LAMPORTS_PER_SIGNATURE,
        solana_sdk::fee_calculator::DEFAULT_TARGET_SIGNATURES_PER_SLOT,
    );
    let rent_exempt_minimum = genesis_config.rent.minimum_balance(0);

    // Create legacy rent-paying System account
    let rent_paying_fee_payer = Keypair::new();
    genesis_config.accounts.insert(
        rent_paying_fee_payer.pubkey(),
        Account::new(rent_exempt_minimum - 1, 0, &system_program::id()),
    );
    // Create RentExempt recipient account
    let recipient = Pubkey::new_unique();
    genesis_config.accounts.insert(
        recipient,
        Account::new(rent_exempt_minimum, 0, &system_program::id()),
    );

    let (bank, _bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);
    let recent_blockhash = bank.last_blockhash();

    let check_account_is_rent_exempt = |pubkey: &Pubkey| -> bool {
        let account = bank.get_account(pubkey).unwrap();
        Rent::default().is_exempt(account.lamports(), account.data().len())
    };

    // Create just-rent-exempt fee-payer
    let rent_exempt_fee_payer = Keypair::new();
    bank.transfer(
        rent_exempt_minimum,
        &mint_keypair,
        &rent_exempt_fee_payer.pubkey(),
    )
    .unwrap();

    // Dummy message to determine fee amount
    let dummy_message = new_sanitized_message(Message::new_with_blockhash(
        &[system_instruction::transfer(
            &rent_exempt_fee_payer.pubkey(),
            &recipient,
            sol_to_lamports(1.),
        )],
        Some(&rent_exempt_fee_payer.pubkey()),
        &recent_blockhash,
    ));
    let fee = bank.get_fee_for_message(&dummy_message).unwrap();

    // RentPaying fee-payer can remain RentPaying
    let tx = Transaction::new(
        &[&rent_paying_fee_payer, &mint_keypair],
        Message::new(
            &[system_instruction::transfer(
                &mint_keypair.pubkey(),
                &recipient,
                rent_exempt_minimum,
            )],
            Some(&rent_paying_fee_payer.pubkey()),
        ),
        recent_blockhash,
    );
    let result = bank.process_transaction(&tx);
    assert!(result.is_ok());
    assert!(!check_account_is_rent_exempt(
        &rent_paying_fee_payer.pubkey()
    ));

    // RentPaying fee-payer can remain RentPaying on failed executed tx
    let sender = Keypair::new();
    let fee_payer_balance = bank.get_balance(&rent_paying_fee_payer.pubkey());
    let tx = Transaction::new(
        &[&rent_paying_fee_payer, &sender],
        Message::new(
            &[system_instruction::transfer(
                &sender.pubkey(),
                &recipient,
                rent_exempt_minimum,
            )],
            Some(&rent_paying_fee_payer.pubkey()),
        ),
        recent_blockhash,
    );
    let result = bank.process_transaction(&tx);
    assert_eq!(
        result.unwrap_err(),
        TransactionError::InstructionError(0, InstructionError::Custom(1))
    );
    assert_ne!(
        fee_payer_balance,
        bank.get_balance(&rent_paying_fee_payer.pubkey())
    );
    assert!(!check_account_is_rent_exempt(
        &rent_paying_fee_payer.pubkey()
    ));

    // RentPaying fee-payer can be emptied with fee and transaction
    let tx = Transaction::new(
        &[&rent_paying_fee_payer],
        Message::new(
            &[system_instruction::transfer(
                &rent_paying_fee_payer.pubkey(),
                &recipient,
                bank.get_balance(&rent_paying_fee_payer.pubkey()) - fee,
            )],
            Some(&rent_paying_fee_payer.pubkey()),
        ),
        recent_blockhash,
    );
    let result = bank.process_transaction(&tx);
    assert!(result.is_ok());
    assert_eq!(0, bank.get_balance(&rent_paying_fee_payer.pubkey()));

    // RentExempt fee-payer cannot become RentPaying from transaction fee
    let tx = Transaction::new(
        &[&rent_exempt_fee_payer, &mint_keypair],
        Message::new(
            &[system_instruction::transfer(
                &mint_keypair.pubkey(),
                &recipient,
                rent_exempt_minimum,
            )],
            Some(&rent_exempt_fee_payer.pubkey()),
        ),
        recent_blockhash,
    );
    let result = bank.process_transaction(&tx);
    assert_eq!(
        result.unwrap_err(),
        TransactionError::InsufficientFundsForRent { account_index: 0 }
    );
    assert!(check_account_is_rent_exempt(
        &rent_exempt_fee_payer.pubkey()
    ));

    // RentExempt fee-payer cannot become RentPaying via failed executed tx
    let tx = Transaction::new(
        &[&rent_exempt_fee_payer, &sender],
        Message::new(
            &[system_instruction::transfer(
                &sender.pubkey(),
                &recipient,
                rent_exempt_minimum,
            )],
            Some(&rent_exempt_fee_payer.pubkey()),
        ),
        recent_blockhash,
    );
    let result = bank.process_transaction(&tx);
    assert_eq!(
        result.unwrap_err(),
        TransactionError::InsufficientFundsForRent { account_index: 0 }
    );
    assert!(check_account_is_rent_exempt(
        &rent_exempt_fee_payer.pubkey()
    ));

    // For good measure, show that a RentExempt fee-payer that is also debited by a transaction
    // cannot become RentPaying by that debit, but can still be charged for the fee
    bank.transfer(fee, &mint_keypair, &rent_exempt_fee_payer.pubkey())
        .unwrap();
    let fee_payer_balance = bank.get_balance(&rent_exempt_fee_payer.pubkey());
    assert_eq!(fee_payer_balance, rent_exempt_minimum + fee);
    let tx = Transaction::new(
        &[&rent_exempt_fee_payer],
        Message::new(
            &[system_instruction::transfer(
                &rent_exempt_fee_payer.pubkey(),
                &recipient,
                fee,
            )],
            Some(&rent_exempt_fee_payer.pubkey()),
        ),
        recent_blockhash,
    );
    let result = bank.process_transaction(&tx);
    assert_eq!(
        result.unwrap_err(),
        TransactionError::InsufficientFundsForRent { account_index: 0 }
    );
    assert_eq!(
        fee_payer_balance - fee,
        bank.get_balance(&rent_exempt_fee_payer.pubkey())
    );
    assert!(check_account_is_rent_exempt(
        &rent_exempt_fee_payer.pubkey()
    ));

    // Also show that a RentExempt fee-payer can be completely emptied via fee and transaction
    bank.transfer(fee + 1, &mint_keypair, &rent_exempt_fee_payer.pubkey())
        .unwrap();
    assert!(bank.get_balance(&rent_exempt_fee_payer.pubkey()) > rent_exempt_minimum + fee);
    let tx = Transaction::new(
        &[&rent_exempt_fee_payer],
        Message::new(
            &[system_instruction::transfer(
                &rent_exempt_fee_payer.pubkey(),
                &recipient,
                bank.get_balance(&rent_exempt_fee_payer.pubkey()) - fee,
            )],
            Some(&rent_exempt_fee_payer.pubkey()),
        ),
        recent_blockhash,
    );
    let result = bank.process_transaction(&tx);
    assert!(result.is_ok());
    assert_eq!(0, bank.get_balance(&rent_exempt_fee_payer.pubkey()));

    // ... but not if the fee alone would make it RentPaying
    bank.transfer(
        rent_exempt_minimum + 1,
        &mint_keypair,
        &rent_exempt_fee_payer.pubkey(),
    )
    .unwrap();
    assert!(bank.get_balance(&rent_exempt_fee_payer.pubkey()) < rent_exempt_minimum + fee);
    let tx = Transaction::new(
        &[&rent_exempt_fee_payer],
        Message::new(
            &[system_instruction::transfer(
                &rent_exempt_fee_payer.pubkey(),
                &recipient,
                bank.get_balance(&rent_exempt_fee_payer.pubkey()) - fee,
            )],
            Some(&rent_exempt_fee_payer.pubkey()),
        ),
        recent_blockhash,
    );
    let result = bank.process_transaction(&tx);
    assert_eq!(
        result.unwrap_err(),
        TransactionError::InsufficientFundsForRent { account_index: 0 }
    );
    assert!(check_account_is_rent_exempt(
        &rent_exempt_fee_payer.pubkey()
    ));
}

// Ensure System transfers of any size can be made to the incinerator
#[test]
fn test_rent_state_incinerator() {
    let GenesisConfigInfo {
        mut genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config_with_leader(sol_to_lamports(100.), &Pubkey::new_unique(), 42);
    genesis_config.rent = Rent::default();
    let rent_exempt_minimum = genesis_config.rent.minimum_balance(0);

    let (bank, _bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);

    for amount in [rent_exempt_minimum - 1, rent_exempt_minimum] {
        bank.transfer(amount, &mint_keypair, &solana_sdk::incinerator::id())
            .unwrap();
    }
}

#[test]
fn test_update_accounts_data_size() {
    // Test: Subtraction saturates at 0
    {
        let bank = create_simple_test_bank(100);
        let initial_data_size = bank.load_accounts_data_size() as i64;
        let data_size = 567;
        bank.accounts_data_size_delta_on_chain
            .store(data_size, Release);
        bank.update_accounts_data_size_delta_on_chain(
            (initial_data_size + data_size + 1).saturating_neg(),
        );
        assert_eq!(bank.load_accounts_data_size(), 0);
    }

    // Test: Addition saturates at u64::MAX
    {
        let mut bank = create_simple_test_bank(100);
        let data_size_remaining = 567;
        bank.accounts_data_size_initial = u64::MAX - data_size_remaining;
        bank.accounts_data_size_delta_off_chain
            .store((data_size_remaining + 1) as i64, Release);
        assert_eq!(bank.load_accounts_data_size(), u64::MAX);
    }

    // Test: Updates work as expected
    {
        // Set the accounts data size to be in the middle, then perform a bunch of small
        // updates, checking the results after each one.
        let mut bank = create_simple_test_bank(100);
        bank.accounts_data_size_initial = u32::MAX as u64;
        let mut rng = rand::thread_rng();
        for _ in 0..100 {
            let initial = bank.load_accounts_data_size() as i64;
            let delta1 = rng.gen_range(-500..500);
            bank.update_accounts_data_size_delta_on_chain(delta1);
            let delta2 = rng.gen_range(-500..500);
            bank.update_accounts_data_size_delta_off_chain(delta2);
            assert_eq!(
                bank.load_accounts_data_size() as i64,
                initial.saturating_add(delta1).saturating_add(delta2),
            );
        }
    }
}

#[derive(Serialize, Deserialize)]
enum MockReallocInstruction {
    Realloc(usize, u64, Pubkey),
}

declare_process_instruction!(MockReallocBuiltin, 1, |invoke_context| {
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;
    let instruction_data = instruction_context.get_instruction_data();
    if let Ok(instruction) = bincode::deserialize(instruction_data) {
        match instruction {
            MockReallocInstruction::Realloc(new_size, new_balance, _) => {
                // Set data length
                instruction_context
                    .try_borrow_instruction_account(transaction_context, 1)?
                    .set_data_length(new_size)?;

                // set balance
                let current_balance = instruction_context
                    .try_borrow_instruction_account(transaction_context, 1)?
                    .get_lamports();
                let diff_balance = (new_balance as i64).saturating_sub(current_balance as i64);
                let amount = diff_balance.unsigned_abs();
                if diff_balance.is_positive() {
                    instruction_context
                        .try_borrow_instruction_account(transaction_context, 0)?
                        .checked_sub_lamports(amount)?;
                    instruction_context
                        .try_borrow_instruction_account(transaction_context, 1)?
                        .set_lamports(new_balance)?;
                } else {
                    instruction_context
                        .try_borrow_instruction_account(transaction_context, 0)?
                        .checked_add_lamports(amount)?;
                    instruction_context
                        .try_borrow_instruction_account(transaction_context, 1)?
                        .set_lamports(new_balance)?;
                }
                Ok(())
            }
        }
    } else {
        Err(InstructionError::InvalidInstructionData)
    }
});

fn create_mock_realloc_tx(
    payer: &Keypair,
    funder: &Keypair,
    reallocd: &Pubkey,
    new_size: usize,
    new_balance: u64,
    mock_program_id: Pubkey,
    recent_blockhash: Hash,
) -> Transaction {
    let account_metas = vec![
        AccountMeta::new(funder.pubkey(), false),
        AccountMeta::new(*reallocd, false),
    ];
    let instruction = Instruction::new_with_bincode(
        mock_program_id,
        &MockReallocInstruction::Realloc(new_size, new_balance, Pubkey::new_unique()),
        account_metas,
    );
    Transaction::new_signed_with_payer(
        &[instruction],
        Some(&payer.pubkey()),
        &[payer],
        recent_blockhash,
    )
}

#[test]
fn test_resize_and_rent() {
    let GenesisConfigInfo {
        mut genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config_with_leader(1_000_000_000, &Pubkey::new_unique(), 42);
    genesis_config.rent = Rent::default();
    activate_all_features(&mut genesis_config);

    let mock_program_id = Pubkey::new_unique();
    let (bank, _bank_forks) = Bank::new_with_mockup_builtin_for_tests(
        &genesis_config,
        mock_program_id,
        MockReallocBuiltin::vm,
    );

    let recent_blockhash = bank.last_blockhash();

    let account_data_size_small = 1024;
    let rent_exempt_minimum_small = genesis_config.rent.minimum_balance(account_data_size_small);
    let account_data_size_large = 2048;
    let rent_exempt_minimum_large = genesis_config.rent.minimum_balance(account_data_size_large);

    let funding_keypair = Keypair::new();
    bank.store_account(
        &funding_keypair.pubkey(),
        &AccountSharedData::new(1_000_000_000, 0, &mock_program_id),
    );

    let rent_paying_pubkey = solana_sdk::pubkey::new_rand();
    let mut rent_paying_account = AccountSharedData::new(
        rent_exempt_minimum_small - 1,
        account_data_size_small,
        &mock_program_id,
    );
    rent_paying_account.set_rent_epoch(1);

    // restore program-owned account
    bank.store_account(&rent_paying_pubkey, &rent_paying_account);

    // rent paying, realloc larger, fail because not rent exempt
    let tx = create_mock_realloc_tx(
        &mint_keypair,
        &funding_keypair,
        &rent_paying_pubkey,
        account_data_size_large,
        rent_exempt_minimum_small - 1,
        mock_program_id,
        recent_blockhash,
    );
    let expected_err = {
        let account_index = tx
            .message
            .account_keys
            .iter()
            .position(|key| key == &rent_paying_pubkey)
            .unwrap() as u8;
        TransactionError::InsufficientFundsForRent { account_index }
    };
    assert_eq!(bank.process_transaction(&tx).unwrap_err(), expected_err);
    assert_eq!(
        rent_exempt_minimum_small - 1,
        bank.get_account(&rent_paying_pubkey).unwrap().lamports()
    );

    // rent paying, realloc larger and rent exempt
    let tx = create_mock_realloc_tx(
        &mint_keypair,
        &funding_keypair,
        &rent_paying_pubkey,
        account_data_size_large,
        rent_exempt_minimum_large,
        mock_program_id,
        recent_blockhash,
    );
    let result = bank.process_transaction(&tx);
    assert!(result.is_ok());
    assert_eq!(
        rent_exempt_minimum_large,
        bank.get_account(&rent_paying_pubkey).unwrap().lamports()
    );

    // rent exempt, realloc small, fail because not rent exempt
    let tx = create_mock_realloc_tx(
        &mint_keypair,
        &funding_keypair,
        &rent_paying_pubkey,
        account_data_size_small,
        rent_exempt_minimum_small - 1,
        mock_program_id,
        recent_blockhash,
    );
    let expected_err = {
        let account_index = tx
            .message
            .account_keys
            .iter()
            .position(|key| key == &rent_paying_pubkey)
            .unwrap() as u8;
        TransactionError::InsufficientFundsForRent { account_index }
    };
    assert_eq!(bank.process_transaction(&tx).unwrap_err(), expected_err);
    assert_eq!(
        rent_exempt_minimum_large,
        bank.get_account(&rent_paying_pubkey).unwrap().lamports()
    );

    // rent exempt, realloc smaller and rent exempt
    let tx = create_mock_realloc_tx(
        &mint_keypair,
        &funding_keypair,
        &rent_paying_pubkey,
        account_data_size_small,
        rent_exempt_minimum_small,
        mock_program_id,
        recent_blockhash,
    );
    let result = bank.process_transaction(&tx);
    assert!(result.is_ok());
    assert_eq!(
        rent_exempt_minimum_small,
        bank.get_account(&rent_paying_pubkey).unwrap().lamports()
    );

    // rent exempt, realloc large, fail because not rent exempt
    let tx = create_mock_realloc_tx(
        &mint_keypair,
        &funding_keypair,
        &rent_paying_pubkey,
        account_data_size_large,
        rent_exempt_minimum_large - 1,
        mock_program_id,
        recent_blockhash,
    );
    let expected_err = {
        let account_index = tx
            .message
            .account_keys
            .iter()
            .position(|key| key == &rent_paying_pubkey)
            .unwrap() as u8;
        TransactionError::InsufficientFundsForRent { account_index }
    };
    assert_eq!(bank.process_transaction(&tx).unwrap_err(), expected_err);
    assert_eq!(
        rent_exempt_minimum_small,
        bank.get_account(&rent_paying_pubkey).unwrap().lamports()
    );

    // rent exempt, realloc large and rent exempt
    let tx = create_mock_realloc_tx(
        &mint_keypair,
        &funding_keypair,
        &rent_paying_pubkey,
        account_data_size_large,
        rent_exempt_minimum_large,
        mock_program_id,
        recent_blockhash,
    );
    let result = bank.process_transaction(&tx);
    assert!(result.is_ok());
    assert_eq!(
        rent_exempt_minimum_large,
        bank.get_account(&rent_paying_pubkey).unwrap().lamports()
    );

    let created_keypair = Keypair::new();

    // create account, not rent exempt
    let tx = system_transaction::create_account(
        &mint_keypair,
        &created_keypair,
        recent_blockhash,
        rent_exempt_minimum_small - 1,
        account_data_size_small as u64,
        &system_program::id(),
    );
    let expected_err = {
        let account_index = tx
            .message
            .account_keys
            .iter()
            .position(|key| key == &created_keypair.pubkey())
            .unwrap() as u8;
        TransactionError::InsufficientFundsForRent { account_index }
    };
    assert_eq!(bank.process_transaction(&tx).unwrap_err(), expected_err);

    // create account, rent exempt
    let tx = system_transaction::create_account(
        &mint_keypair,
        &created_keypair,
        recent_blockhash,
        rent_exempt_minimum_small,
        account_data_size_small as u64,
        &system_program::id(),
    );
    let result = bank.process_transaction(&tx);
    assert!(result.is_ok());
    assert_eq!(
        rent_exempt_minimum_small,
        bank.get_account(&created_keypair.pubkey())
            .unwrap()
            .lamports()
    );

    let created_keypair = Keypair::new();
    // create account, no data
    let tx = system_transaction::create_account(
        &mint_keypair,
        &created_keypair,
        recent_blockhash,
        rent_exempt_minimum_small - 1,
        0,
        &system_program::id(),
    );
    let result = bank.process_transaction(&tx);
    assert!(result.is_ok());
    assert_eq!(
        rent_exempt_minimum_small - 1,
        bank.get_account(&created_keypair.pubkey())
            .unwrap()
            .lamports()
    );

    // alloc but not rent exempt
    let tx = system_transaction::allocate(
        &mint_keypair,
        &created_keypair,
        recent_blockhash,
        (account_data_size_small + 1) as u64,
    );
    let expected_err = {
        let account_index = tx
            .message
            .account_keys
            .iter()
            .position(|key| key == &created_keypair.pubkey())
            .unwrap() as u8;
        TransactionError::InsufficientFundsForRent { account_index }
    };
    assert_eq!(bank.process_transaction(&tx).unwrap_err(), expected_err);

    // bring balance of account up to rent exemption
    let tx = system_transaction::transfer(
        &mint_keypair,
        &created_keypair.pubkey(),
        1,
        recent_blockhash,
    );
    let result = bank.process_transaction(&tx);
    assert!(result.is_ok());
    assert_eq!(
        rent_exempt_minimum_small,
        bank.get_account(&created_keypair.pubkey())
            .unwrap()
            .lamports()
    );

    // allocate as rent exempt
    let tx = system_transaction::allocate(
        &mint_keypair,
        &created_keypair,
        recent_blockhash,
        account_data_size_small as u64,
    );
    let result = bank.process_transaction(&tx);
    assert!(result.is_ok());
    assert_eq!(
        rent_exempt_minimum_small,
        bank.get_account(&created_keypair.pubkey())
            .unwrap()
            .lamports()
    );
}

/// Ensure that accounts data size is updated correctly on resize transactions
#[test]
fn test_accounts_data_size_and_resize_transactions() {
    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = genesis_utils::create_genesis_config(100 * LAMPORTS_PER_SOL);
    let mock_program_id = Pubkey::new_unique();
    let (bank, _bank_forks) = Bank::new_with_mockup_builtin_for_tests(
        &genesis_config,
        mock_program_id,
        MockReallocBuiltin::vm,
    );

    let recent_blockhash = bank.last_blockhash();

    let funding_keypair = Keypair::new();
    bank.store_account(
        &funding_keypair.pubkey(),
        &AccountSharedData::new(10 * LAMPORTS_PER_SOL, 0, &mock_program_id),
    );

    let mut rng = rand::thread_rng();

    // Test case: Grow account
    {
        let account_pubkey = Pubkey::new_unique();
        let account_balance = LAMPORTS_PER_SOL;
        let account_size =
            rng.gen_range(1..MAX_PERMITTED_DATA_LENGTH as usize - MAX_PERMITTED_DATA_INCREASE);
        let account_data = AccountSharedData::new(account_balance, account_size, &mock_program_id);
        bank.store_account(&account_pubkey, &account_data);

        let accounts_data_size_before = bank.load_accounts_data_size();
        let account_grow_size = rng.gen_range(1..MAX_PERMITTED_DATA_INCREASE);
        let transaction = create_mock_realloc_tx(
            &mint_keypair,
            &funding_keypair,
            &account_pubkey,
            account_size + account_grow_size,
            account_balance,
            mock_program_id,
            recent_blockhash,
        );
        let result = bank.process_transaction(&transaction);
        assert!(result.is_ok());
        let accounts_data_size_after = bank.load_accounts_data_size();
        assert_eq!(
            accounts_data_size_after,
            accounts_data_size_before.saturating_add(account_grow_size as u64),
        );
    }

    // Test case: Shrink account
    {
        let account_pubkey = Pubkey::new_unique();
        let account_balance = LAMPORTS_PER_SOL;
        let account_size =
            rng.gen_range(MAX_PERMITTED_DATA_LENGTH / 2..MAX_PERMITTED_DATA_LENGTH) as usize;
        let account_data = AccountSharedData::new(account_balance, account_size, &mock_program_id);
        bank.store_account(&account_pubkey, &account_data);

        let accounts_data_size_before = bank.load_accounts_data_size();
        let account_shrink_size = rng.gen_range(1..account_size);
        let transaction = create_mock_realloc_tx(
            &mint_keypair,
            &funding_keypair,
            &account_pubkey,
            account_size - account_shrink_size,
            account_balance,
            mock_program_id,
            recent_blockhash,
        );
        let result = bank.process_transaction(&transaction);
        assert!(result.is_ok());
        let accounts_data_size_after = bank.load_accounts_data_size();
        assert_eq!(
            accounts_data_size_after,
            accounts_data_size_before.saturating_sub(account_shrink_size as u64),
        );
    }
}

#[test]
fn test_get_rent_paying_pubkeys() {
    let lamports = 1;
    let bank = create_simple_test_bank(lamports);

    let n = 432_000;
    assert!(bank.get_rent_paying_pubkeys(&(0, 1, n)).is_none());
    assert!(bank.get_rent_paying_pubkeys(&(0, 2, n)).is_none());
    assert!(bank.get_rent_paying_pubkeys(&(0, 0, n)).is_none());

    let pk1 = Pubkey::from([2; 32]);
    let pk2 = Pubkey::from([3; 32]);
    let index1 = accounts_partition::partition_from_pubkey(&pk1, n);
    let index2 = accounts_partition::partition_from_pubkey(&pk2, n);
    assert!(index1 > 0, "{}", index1);
    assert!(index2 > index1, "{index2}, {index1}");

    let epoch_schedule = EpochSchedule::custom(n, 0, false);

    let mut rent_paying_accounts_by_partition = RentPayingAccountsByPartition::new(&epoch_schedule);
    rent_paying_accounts_by_partition.add_account(&pk1);
    rent_paying_accounts_by_partition.add_account(&pk2);

    bank.rc
        .accounts
        .accounts_db
        .accounts_index
        .rent_paying_accounts_by_partition
        .set(rent_paying_accounts_by_partition)
        .unwrap();

    assert_eq!(
        bank.get_rent_paying_pubkeys(&(0, 1, n)),
        Some(HashSet::default())
    );
    assert_eq!(
        bank.get_rent_paying_pubkeys(&(0, 2, n)),
        Some(HashSet::default())
    );
    assert_eq!(
        bank.get_rent_paying_pubkeys(&(index1.saturating_sub(1), index1, n)),
        Some(HashSet::from([pk1]))
    );
    assert_eq!(
        bank.get_rent_paying_pubkeys(&(index2.saturating_sub(1), index2, n)),
        Some(HashSet::from([pk2]))
    );
    assert_eq!(
        bank.get_rent_paying_pubkeys(&(index1.saturating_sub(1), index2, n)),
        Some(HashSet::from([pk2, pk1]))
    );
    assert_eq!(
        bank.get_rent_paying_pubkeys(&(0, 0, n)),
        Some(HashSet::default())
    );
}

/// Ensure that accounts data size is updated correctly by rent collection
#[test_case(true; "enable rent fees collection")]
#[test_case(false; "disable rent fees collection")]
fn test_accounts_data_size_and_rent_collection(should_collect_rent: bool) {
    let GenesisConfigInfo {
        mut genesis_config, ..
    } = genesis_utils::create_genesis_config(100 * LAMPORTS_PER_SOL);
    genesis_config.rent = Rent::default();
    if should_collect_rent {
        genesis_config
            .accounts
            .remove(&solana_feature_set::disable_rent_fees_collection::id());
    }

    let bank = Arc::new(Bank::new_for_tests(&genesis_config));

    let slot = bank.slot() + bank.slot_count_per_normal_epoch();
    let bank = Arc::new(Bank::new_from_parent(bank, &Pubkey::default(), slot));

    // make another bank so that any reclaimed accounts from the previous bank do not impact
    // this test
    let slot = bank.slot() + bank.slot_count_per_normal_epoch();
    let bank = Arc::new(Bank::new_from_parent(bank, &Pubkey::default(), slot));

    // Store an account into the bank that is rent-paying and has data
    let data_size = 123;
    let mut account = AccountSharedData::new(1, data_size, &Pubkey::default());
    let keypair = Keypair::new();
    bank.store_account(&keypair.pubkey(), &account);

    // Ensure if we collect rent from the account that it will be reclaimed
    {
        let info = bank
            .rent_collector
            .collect_from_existing_account(&keypair.pubkey(), &mut account);
        assert_eq!(info.account_data_len_reclaimed, data_size as u64);
    }

    // Collect rent for real
    assert_eq!(should_collect_rent, bank.should_collect_rent());
    let accounts_data_size_delta_before_collecting_rent = bank.load_accounts_data_size_delta();
    bank.collect_rent_eagerly();
    let accounts_data_size_delta_after_collecting_rent = bank.load_accounts_data_size_delta();

    let accounts_data_size_delta_delta = accounts_data_size_delta_after_collecting_rent
        - accounts_data_size_delta_before_collecting_rent;
    assert!(!should_collect_rent || accounts_data_size_delta_delta < 0);
    let reclaimed_data_size = accounts_data_size_delta_delta.saturating_neg() as usize;

    // Ensure the account is reclaimed by rent collection
    assert!(!should_collect_rent || reclaimed_data_size == data_size);
}

#[test]
fn test_accounts_data_size_with_default_bank() {
    let bank = Bank::default_for_tests();
    assert_eq!(
        bank.load_accounts_data_size() as usize,
        bank.get_total_accounts_stats().unwrap().data_len
    );
}

#[test]
fn test_accounts_data_size_from_genesis() {
    let GenesisConfigInfo {
        mut genesis_config,
        mint_keypair,
        ..
    } = genesis_utils::create_genesis_config_with_leader(
        1_000_000 * LAMPORTS_PER_SOL,
        &Pubkey::new_unique(),
        100 * LAMPORTS_PER_SOL,
    );
    genesis_config.rent = Rent::default();
    genesis_config.ticks_per_slot = 3;

    let (mut bank, bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);
    assert_eq!(
        bank.load_accounts_data_size() as usize,
        bank.get_total_accounts_stats().unwrap().data_len
    );

    // Create accounts over a number of banks and ensure the accounts data size remains correct
    for _ in 0..10 {
        let slot = bank.slot() + 1;
        bank = new_bank_from_parent_with_bank_forks(
            bank_forks.as_ref(),
            bank,
            &Pubkey::default(),
            slot,
        );

        // Store an account into the bank that is rent-exempt and has data
        let data_size = rand::thread_rng().gen_range(3333..4444);
        let transaction = system_transaction::create_account(
            &mint_keypair,
            &Keypair::new(),
            bank.last_blockhash(),
            genesis_config.rent.minimum_balance(data_size),
            data_size as u64,
            &solana_sdk::system_program::id(),
        );
        bank.process_transaction(&transaction).unwrap();
        bank.fill_bank_with_ticks_for_tests();

        assert_eq!(
            bank.load_accounts_data_size() as usize,
            bank.get_total_accounts_stats().unwrap().data_len,
        );
    }
}

/// Ensures that if a transaction exceeds the maximum allowed accounts data allocation size:
/// 1. The transaction fails
/// 2. The bank's accounts_data_size is unmodified
#[test]
fn test_cap_accounts_data_allocations_per_transaction() {
    const NUM_MAX_SIZE_ALLOCATIONS_PER_TRANSACTION: usize =
        MAX_PERMITTED_ACCOUNTS_DATA_ALLOCATIONS_PER_TRANSACTION as usize
            / MAX_PERMITTED_DATA_LENGTH as usize;

    let (genesis_config, mint_keypair) = create_genesis_config(1_000_000 * LAMPORTS_PER_SOL);
    let (bank, _bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);

    let mut instructions = Vec::new();
    let mut keypairs = vec![mint_keypair.insecure_clone()];
    for _ in 0..=NUM_MAX_SIZE_ALLOCATIONS_PER_TRANSACTION {
        let keypair = Keypair::new();
        let instruction = system_instruction::create_account(
            &mint_keypair.pubkey(),
            &keypair.pubkey(),
            bank.rent_collector()
                .rent
                .minimum_balance(MAX_PERMITTED_DATA_LENGTH as usize),
            MAX_PERMITTED_DATA_LENGTH,
            &solana_sdk::system_program::id(),
        );
        keypairs.push(keypair);
        instructions.push(instruction);
    }
    let message = Message::new(&instructions, Some(&mint_keypair.pubkey()));
    let signers: Vec<_> = keypairs.iter().collect();
    let transaction = Transaction::new(&signers, message, bank.last_blockhash());

    let accounts_data_size_before = bank.load_accounts_data_size();
    let result = bank.process_transaction(&transaction);
    let accounts_data_size_after = bank.load_accounts_data_size();

    assert_eq!(accounts_data_size_before, accounts_data_size_after);
    assert_eq!(
        result,
        Err(TransactionError::InstructionError(
            NUM_MAX_SIZE_ALLOCATIONS_PER_TRANSACTION as u8,
            solana_sdk::instruction::InstructionError::MaxAccountsDataAllocationsExceeded,
        )),
    );
}

#[test]
fn test_feature_activation_idempotent() {
    let mut genesis_config = GenesisConfig::default();
    const HASHES_PER_TICK_START: u64 = 3;
    genesis_config.poh_config.hashes_per_tick = Some(HASHES_PER_TICK_START);

    let mut bank = Bank::new_for_tests(&genesis_config);
    assert_eq!(bank.hashes_per_tick, Some(HASHES_PER_TICK_START));

    // Don't activate feature
    bank.apply_feature_activations(ApplyFeatureActivationsCaller::NewFromParent, false);
    assert_eq!(bank.hashes_per_tick, Some(HASHES_PER_TICK_START));

    // Activate feature
    let feature_account_balance =
        std::cmp::max(genesis_config.rent.minimum_balance(Feature::size_of()), 1);
    bank.store_account(
        &feature_set::update_hashes_per_tick::id(),
        &feature::create_account(&Feature { activated_at: None }, feature_account_balance),
    );
    bank.apply_feature_activations(ApplyFeatureActivationsCaller::NewFromParent, false);
    assert_eq!(bank.hashes_per_tick, Some(DEFAULT_HASHES_PER_TICK));

    // Activate feature "again"
    bank.apply_feature_activations(ApplyFeatureActivationsCaller::NewFromParent, false);
    assert_eq!(bank.hashes_per_tick, Some(DEFAULT_HASHES_PER_TICK));
}

#[test]
fn test_feature_hashes_per_tick() {
    let mut genesis_config = GenesisConfig::default();
    const HASHES_PER_TICK_START: u64 = 3;
    genesis_config.poh_config.hashes_per_tick = Some(HASHES_PER_TICK_START);

    let mut bank = Bank::new_for_tests(&genesis_config);
    assert_eq!(bank.hashes_per_tick, Some(HASHES_PER_TICK_START));

    // Don't activate feature
    bank.apply_feature_activations(ApplyFeatureActivationsCaller::NewFromParent, false);
    assert_eq!(bank.hashes_per_tick, Some(HASHES_PER_TICK_START));

    // Activate feature
    let feature_account_balance =
        std::cmp::max(genesis_config.rent.minimum_balance(Feature::size_of()), 1);
    bank.store_account(
        &feature_set::update_hashes_per_tick::id(),
        &feature::create_account(&Feature { activated_at: None }, feature_account_balance),
    );
    bank.apply_feature_activations(ApplyFeatureActivationsCaller::NewFromParent, false);
    assert_eq!(bank.hashes_per_tick, Some(DEFAULT_HASHES_PER_TICK));

    // Activate feature
    let feature_account_balance =
        std::cmp::max(genesis_config.rent.minimum_balance(Feature::size_of()), 1);
    bank.store_account(
        &feature_set::update_hashes_per_tick2::id(),
        &feature::create_account(&Feature { activated_at: None }, feature_account_balance),
    );
    bank.apply_feature_activations(ApplyFeatureActivationsCaller::NewFromParent, false);
    assert_eq!(bank.hashes_per_tick, Some(UPDATED_HASHES_PER_TICK2));

    // Activate feature
    let feature_account_balance =
        std::cmp::max(genesis_config.rent.minimum_balance(Feature::size_of()), 1);
    bank.store_account(
        &feature_set::update_hashes_per_tick3::id(),
        &feature::create_account(&Feature { activated_at: None }, feature_account_balance),
    );
    bank.apply_feature_activations(ApplyFeatureActivationsCaller::NewFromParent, false);
    assert_eq!(bank.hashes_per_tick, Some(UPDATED_HASHES_PER_TICK3));

    // Activate feature
    let feature_account_balance =
        std::cmp::max(genesis_config.rent.minimum_balance(Feature::size_of()), 1);
    bank.store_account(
        &feature_set::update_hashes_per_tick4::id(),
        &feature::create_account(&Feature { activated_at: None }, feature_account_balance),
    );
    bank.apply_feature_activations(ApplyFeatureActivationsCaller::NewFromParent, false);
    assert_eq!(bank.hashes_per_tick, Some(UPDATED_HASHES_PER_TICK4));

    // Activate feature
    let feature_account_balance =
        std::cmp::max(genesis_config.rent.minimum_balance(Feature::size_of()), 1);
    bank.store_account(
        &feature_set::update_hashes_per_tick5::id(),
        &feature::create_account(&Feature { activated_at: None }, feature_account_balance),
    );
    bank.apply_feature_activations(ApplyFeatureActivationsCaller::NewFromParent, false);
    assert_eq!(bank.hashes_per_tick, Some(UPDATED_HASHES_PER_TICK5));

    // Activate feature
    let feature_account_balance =
        std::cmp::max(genesis_config.rent.minimum_balance(Feature::size_of()), 1);
    bank.store_account(
        &feature_set::update_hashes_per_tick6::id(),
        &feature::create_account(&Feature { activated_at: None }, feature_account_balance),
    );
    bank.apply_feature_activations(ApplyFeatureActivationsCaller::NewFromParent, false);
    assert_eq!(bank.hashes_per_tick, Some(UPDATED_HASHES_PER_TICK6));
}

#[test]
fn test_calculate_fee_with_congestion_multiplier() {
    let lamports_scale: u64 = 5;
    let base_lamports_per_signature: u64 = 5_000;
    let cheap_lamports_per_signature: u64 = base_lamports_per_signature / lamports_scale;
    let expensive_lamports_per_signature: u64 = base_lamports_per_signature * lamports_scale;
    let signature_count: u64 = 2;
    let signature_fee: u64 = 10;
    let fee_structure = FeeStructure {
        lamports_per_signature: signature_fee,
        ..FeeStructure::default()
    };

    // Two signatures, double the fee.
    let key0 = Pubkey::new_unique();
    let key1 = Pubkey::new_unique();
    let ix0 = system_instruction::transfer(&key0, &key1, 1);
    let ix1 = system_instruction::transfer(&key1, &key0, 1);
    let message = new_sanitized_message(Message::new(&[ix0, ix1], Some(&key0)));

    // assert when lamports_per_signature is less than BASE_LAMPORTS, turnning on/off
    // congestion_multiplier has no effect on fee.
    assert_eq!(
        calculate_test_fee(&message, cheap_lamports_per_signature, &fee_structure),
        signature_fee * signature_count
    );

    // assert when lamports_per_signature is more than BASE_LAMPORTS, turnning on/off
    // congestion_multiplier will change calculated fee.
    assert_eq!(
        calculate_test_fee(&message, expensive_lamports_per_signature, &fee_structure,),
        signature_fee * signature_count
    );
}

#[test]
fn test_calculate_fee_with_request_heap_frame_flag() {
    let key0 = Pubkey::new_unique();
    let key1 = Pubkey::new_unique();
    let lamports_per_signature: u64 = 5_000;
    let signature_fee: u64 = 10;
    let request_cu: u64 = 1;
    let lamports_per_cu: u64 = 5;
    let fee_structure = FeeStructure {
        lamports_per_signature: signature_fee,
        ..FeeStructure::default()
    };
    let message = new_sanitized_message(Message::new(
        &[
            system_instruction::transfer(&key0, &key1, 1),
            ComputeBudgetInstruction::set_compute_unit_limit(request_cu as u32),
            ComputeBudgetInstruction::request_heap_frame(40 * 1024),
            ComputeBudgetInstruction::set_compute_unit_price(lamports_per_cu * 1_000_000),
        ],
        Some(&key0),
    ));

    // assert when request_heap_frame is presented in tx, prioritization fee will be counted
    // into transaction fee
    assert_eq!(
        calculate_test_fee(&message, lamports_per_signature, &fee_structure),
        signature_fee + request_cu * lamports_per_cu
    );
}

#[test]
fn test_is_in_slot_hashes_history() {
    use solana_sdk::slot_hashes::MAX_ENTRIES;

    let (bank0, _bank_forks) = create_simple_test_arc_bank(1);
    assert!(!bank0.is_in_slot_hashes_history(&0));
    assert!(!bank0.is_in_slot_hashes_history(&1));
    let mut last_bank = bank0;
    for _ in 0..MAX_ENTRIES {
        let new_bank = Arc::new(new_from_parent(last_bank));
        assert!(new_bank.is_in_slot_hashes_history(&0));
        last_bank = new_bank;
    }
    let new_bank = Arc::new(new_from_parent(last_bank));
    assert!(!new_bank.is_in_slot_hashes_history(&0));
}

#[test]
fn test_feature_activation_loaded_programs_cache_preparation_phase() {
    solana_logger::setup();

    // Bank Setup
    let (mut genesis_config, mint_keypair) = create_genesis_config(1_000_000 * LAMPORTS_PER_SOL);
    genesis_config
        .accounts
        .remove(&feature_set::disable_sbpf_v1_execution::id());
    genesis_config
        .accounts
        .remove(&feature_set::reenable_sbpf_v1_execution::id());
    let (root_bank, bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);

    // Program Setup
    let program_keypair = Keypair::new();
    let program_data = include_bytes!("../../../programs/bpf_loader/test_elfs/out/noop_aligned.so");
    let program_account = AccountSharedData::from(Account {
        lamports: Rent::default().minimum_balance(program_data.len()).min(1),
        data: program_data.to_vec(),
        owner: bpf_loader::id(),
        executable: true,
        rent_epoch: 0,
    });
    root_bank.store_account(&program_keypair.pubkey(), &program_account);

    // Compose message using the desired program.
    let instruction = Instruction::new_with_bytes(program_keypair.pubkey(), &[], Vec::new());
    let message = Message::new(&[instruction], Some(&mint_keypair.pubkey()));
    let binding = mint_keypair.insecure_clone();
    let signers = vec![&binding];

    // Advance the bank so that the program becomes effective.
    goto_end_of_slot(root_bank.clone());
    let bank = new_from_parent_with_fork_next_slot(root_bank, bank_forks.as_ref());

    // Load the program with the old environment.
    let transaction = Transaction::new(&signers, message.clone(), bank.last_blockhash());
    let result_without_feature_enabled = bank.process_transaction(&transaction);
    assert_eq!(result_without_feature_enabled, Ok(()));

    // Schedule feature activation to trigger a change of environment at the epoch boundary.
    let feature_account_balance =
        std::cmp::max(genesis_config.rent.minimum_balance(Feature::size_of()), 1);
    bank.store_account(
        &feature_set::disable_sbpf_v1_execution::id(),
        &feature::create_account(&Feature { activated_at: None }, feature_account_balance),
    );

    // Advance the bank to middle of epoch to start the recompilation phase.
    goto_end_of_slot(bank.clone());
    let bank = new_bank_from_parent_with_bank_forks(&bank_forks, bank, &Pubkey::default(), 16);
    let current_env = bank
        .transaction_processor
        .program_cache
        .read()
        .unwrap()
        .get_environments_for_epoch(0)
        .program_runtime_v1;
    let upcoming_env = bank
        .transaction_processor
        .program_cache
        .read()
        .unwrap()
        .get_environments_for_epoch(1)
        .program_runtime_v1;

    // Advance the bank to recompile the program.
    {
        let program_cache = bank.transaction_processor.program_cache.read().unwrap();
        let slot_versions = program_cache.get_slot_versions_for_tests(&program_keypair.pubkey());
        assert_eq!(slot_versions.len(), 1);
        assert!(Arc::ptr_eq(
            slot_versions[0].program.get_environment().unwrap(),
            &current_env
        ));
    }
    goto_end_of_slot(bank.clone());
    let bank = new_from_parent_with_fork_next_slot(bank, bank_forks.as_ref());
    {
        let program_cache = bank.transaction_processor.program_cache.write().unwrap();
        let slot_versions = program_cache.get_slot_versions_for_tests(&program_keypair.pubkey());
        assert_eq!(slot_versions.len(), 2);
        assert!(Arc::ptr_eq(
            slot_versions[0].program.get_environment().unwrap(),
            &upcoming_env
        ));
        assert!(Arc::ptr_eq(
            slot_versions[1].program.get_environment().unwrap(),
            &current_env
        ));
    }

    // Advance the bank to cross the epoch boundary and activate the feature.
    goto_end_of_slot(bank.clone());
    let bank = new_bank_from_parent_with_bank_forks(&bank_forks, bank, &Pubkey::default(), 33);

    // Load the program with the new environment.
    let transaction = Transaction::new(&signers, message, bank.last_blockhash());
    let result_with_feature_enabled = bank.process_transaction(&transaction);
    assert_eq!(
        result_with_feature_enabled,
        Err(TransactionError::InstructionError(
            0,
            InstructionError::InvalidAccountData
        ))
    );
}

#[test]
fn test_feature_activation_loaded_programs_epoch_transition() {
    solana_logger::setup();

    // Bank Setup
    let (mut genesis_config, mint_keypair) = create_genesis_config(1_000_000 * LAMPORTS_PER_SOL);
    genesis_config
        .accounts
        .remove(&feature_set::disable_fees_sysvar::id());
    genesis_config
        .accounts
        .remove(&feature_set::reenable_sbpf_v1_execution::id());
    let (root_bank, bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);

    // Program Setup
    let program_keypair = Keypair::new();
    let program_data = include_bytes!("../../../programs/bpf_loader/test_elfs/out/noop_aligned.so");
    let program_account = AccountSharedData::from(Account {
        lamports: Rent::default().minimum_balance(program_data.len()).min(1),
        data: program_data.to_vec(),
        owner: bpf_loader::id(),
        executable: true,
        rent_epoch: 0,
    });
    root_bank.store_account(&program_keypair.pubkey(), &program_account);

    // Compose message using the desired program.
    let instruction = Instruction::new_with_bytes(program_keypair.pubkey(), &[], Vec::new());
    let message = Message::new(&[instruction], Some(&mint_keypair.pubkey()));
    let binding = mint_keypair.insecure_clone();
    let signers = vec![&binding];

    // Advance the bank so that the program becomes effective.
    goto_end_of_slot(root_bank.clone());
    let bank = new_from_parent_with_fork_next_slot(root_bank, bank_forks.as_ref());

    // Load the program with the old environment.
    let transaction = Transaction::new(&signers, message.clone(), bank.last_blockhash());
    assert!(bank.process_transaction(&transaction).is_ok());

    // Schedule feature activation to trigger a change of environment at the epoch boundary.
    let feature_account_balance =
        std::cmp::max(genesis_config.rent.minimum_balance(Feature::size_of()), 1);
    bank.store_account(
        &feature_set::disable_fees_sysvar::id(),
        &feature::create_account(&Feature { activated_at: None }, feature_account_balance),
    );

    // Advance the bank to cross the epoch boundary and activate the feature.
    goto_end_of_slot(bank.clone());
    let bank = new_bank_from_parent_with_bank_forks(&bank_forks, bank, &Pubkey::default(), 33);

    // Load the program with the new environment.
    let transaction = Transaction::new(&signers, message.clone(), bank.last_blockhash());
    assert!(bank.process_transaction(&transaction).is_ok());

    {
        // Prune for rerooting and thus finishing the recompilation phase.
        let mut program_cache = bank.transaction_processor.program_cache.write().unwrap();
        program_cache.prune(bank.slot(), bank.epoch());

        // Unload all (which is only the entry with the new environment)
        program_cache.sort_and_unload(percentage::Percentage::from(0));
    }

    // Reload the unloaded program with the new environment.
    goto_end_of_slot(bank.clone());
    let bank = new_from_parent_with_fork_next_slot(bank, bank_forks.as_ref());
    let transaction = Transaction::new(&signers, message, bank.last_blockhash());
    assert!(bank.process_transaction(&transaction).is_ok());
}

#[test]
fn test_bank_verify_accounts_hash_with_base() {
    let GenesisConfigInfo {
        mut genesis_config,
        mint_keypair: mint,
        ..
    } = genesis_utils::create_genesis_config_with_leader(
        1_000_000 * LAMPORTS_PER_SOL,
        &Pubkey::new_unique(),
        100 * LAMPORTS_PER_SOL,
    );
    genesis_config.rent = Rent::default();
    genesis_config.ticks_per_slot = 3;

    let do_transfers = |bank: &Bank| {
        let key1 = Keypair::new(); // lamports from mint
        let key2 = Keypair::new(); // will end with ZERO lamports
        let key3 = Keypair::new(); // lamports from key2

        let amount = 123_456_789;
        let fee = {
            let blockhash = bank.last_blockhash();
            let transaction = SanitizedTransaction::from_transaction_for_tests(
                system_transaction::transfer(&key2, &key3.pubkey(), amount, blockhash),
            );
            bank.get_fee_for_message(transaction.message()).unwrap()
        };
        bank.transfer(amount + fee, &mint, &key1.pubkey()).unwrap();
        bank.transfer(amount + fee, &mint, &key2.pubkey()).unwrap();
        bank.transfer(amount + fee, &key2, &key3.pubkey()).unwrap();
        assert_eq!(bank.get_balance(&key2.pubkey()), 0);

        bank.fill_bank_with_ticks_for_tests();
    };

    let (mut bank, bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);

    // make some banks, do some transactions, ensure there's some zero-lamport accounts
    for _ in 0..2 {
        let slot = bank.slot() + 1;
        bank = new_bank_from_parent_with_bank_forks(
            bank_forks.as_ref(),
            bank,
            &Pubkey::new_unique(),
            slot,
        );
        do_transfers(&bank);
    }

    // update the base accounts hash
    bank.squash();
    bank.force_flush_accounts_cache();
    bank.update_accounts_hash(CalcAccountsHashDataSource::Storages, false, false);
    let base_slot = bank.slot();
    let base_capitalization = bank.capitalization();

    // make more banks, do more transactions, ensure there's more zero-lamport accounts
    for _ in 0..2 {
        let slot = bank.slot() + 1;
        bank = new_bank_from_parent_with_bank_forks(
            bank_forks.as_ref(),
            bank,
            &Pubkey::new_unique(),
            slot,
        );
        do_transfers(&bank);
    }

    // update the incremental accounts hash
    bank.squash();
    bank.force_flush_accounts_cache();
    bank.update_incremental_accounts_hash(base_slot);

    // ensure the accounts hash verifies
    assert!(bank.verify_accounts_hash(
        Some((base_slot, base_capitalization)),
        VerifyAccountsHashConfig {
            test_hash_calculation: false,
            ..VerifyAccountsHashConfig::default_for_test()
        },
        None,
    ));
}

#[test]
fn test_squash_timing_add_assign() {
    let mut t0 = SquashTiming::default();

    let t1 = SquashTiming {
        squash_accounts_ms: 1,
        squash_accounts_cache_ms: 2,
        squash_accounts_index_ms: 3,
        squash_accounts_store_ms: 4,
        squash_cache_ms: 5,
    };

    let expected = SquashTiming {
        squash_accounts_ms: 2,
        squash_accounts_cache_ms: 2 * 2,
        squash_accounts_index_ms: 3 * 2,
        squash_accounts_store_ms: 4 * 2,
        squash_cache_ms: 5 * 2,
    };

    t0 += t1;
    t0 += t1;

    assert!(t0 == expected);
}

#[test]
fn test_system_instruction_allocate() {
    let (genesis_config, mint_keypair) = create_genesis_config_no_tx_fee(sol_to_lamports(1.0));
    let (bank, _bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);
    let bank_client = BankClient::new_shared(bank);
    let data_len = 2;
    let amount = genesis_config.rent.minimum_balance(data_len);

    let alice_keypair = Keypair::new();
    let alice_pubkey = alice_keypair.pubkey();
    let seed = "seed";
    let owner = Pubkey::new_unique();
    let alice_with_seed = Pubkey::create_with_seed(&alice_pubkey, seed, &owner).unwrap();

    bank_client
        .transfer_and_confirm(amount, &mint_keypair, &alice_pubkey)
        .unwrap();

    let allocate_with_seed = Message::new(
        &[system_instruction::allocate_with_seed(
            &alice_with_seed,
            &alice_pubkey,
            seed,
            data_len as u64,
            &owner,
        )],
        Some(&alice_pubkey),
    );

    assert!(bank_client
        .send_and_confirm_message(&[&alice_keypair], allocate_with_seed)
        .is_ok());

    let allocate = system_instruction::allocate(&alice_pubkey, data_len as u64);

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

    let mint_lamports = sol_to_lamports(1.0);
    let len1 = 123;
    let len2 = 456;

    // create initial bank and fund the alice account
    let (genesis_config, mint_keypair) = create_genesis_config_no_tx_fee_no_rent(mint_lamports);
    let (bank, bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);
    let bank_client = BankClient::new_shared(bank.clone());
    bank_client
        .transfer_and_confirm(mint_lamports, &mint_keypair, &alice_pubkey)
        .unwrap();

    // create and freeze a bank a few epochs in the future to trigger rent
    // collection to visit (and rewrite) all accounts
    let bank = new_from_parent_next_epoch(bank, &bank_forks, 2);
    bank.freeze(); // trigger rent collection

    // create zero-lamports account to be cleaned
    let account = AccountSharedData::new(0, len1, &program);
    let slot = bank.slot() + 1;
    let bank = new_bank_from_parent_with_bank_forks(bank_forks.as_ref(), bank, &collector, slot);
    bank.store_account(&bob_pubkey, &account);

    // transfer some to bogus pubkey just to make previous bank (=slot) really cleanable
    let slot = bank.slot() + 1;
    let bank = new_bank_from_parent_with_bank_forks(bank_forks.as_ref(), bank, &collector, slot);
    let bank_client = BankClient::new_shared(bank.clone());
    bank_client
        .transfer_and_confirm(
            genesis_config.rent.minimum_balance(0),
            &alice_keypair,
            &Pubkey::new_unique(),
        )
        .unwrap();

    // super fun time; callback chooses to .clean_accounts() or not
    let slot = bank.slot() + 1;
    let bank = new_bank_from_parent_with_bank_forks(bank_forks.as_ref(), bank, &collector, slot);
    callback(&bank);

    // create a normal account at the same pubkey as the zero-lamports account
    let lamports = genesis_config.rent.minimum_balance(len2);
    let slot = bank.slot() + 1;
    let bank = new_bank_from_parent_with_bank_forks(bank_forks.as_ref(), bank, &collector, slot);
    let bank_client = BankClient::new_shared(bank);
    let ix = system_instruction::create_account(
        &alice_pubkey,
        &bob_pubkey,
        lamports,
        len2 as u64,
        &program,
    );
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
        assert_eq!(5, bank.get_snapshot_storages(None).len());
        bank.clean_accounts();
        assert_eq!(4, bank.get_snapshot_storages(None).len());
    });
}

#[test]
fn test_create_zero_lamport_without_clean() {
    with_create_zero_lamport(|_| {
        // just do nothing; this should behave identically with test_create_zero_lamport_with_clean
    });
}

#[test]
fn test_system_instruction_assign_with_seed() {
    let (genesis_config, mint_keypair) = create_genesis_config_no_tx_fee(sol_to_lamports(1.0));
    let (bank, _bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);
    let bank_client = BankClient::new_shared(bank);

    let alice_keypair = Keypair::new();
    let alice_pubkey = alice_keypair.pubkey();
    let seed = "seed";
    let owner = Pubkey::new_unique();
    let alice_with_seed = Pubkey::create_with_seed(&alice_pubkey, seed, &owner).unwrap();

    bank_client
        .transfer_and_confirm(
            genesis_config.rent.minimum_balance(0),
            &mint_keypair,
            &alice_pubkey,
        )
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
fn test_system_instruction_unsigned_transaction() {
    let (genesis_config, alice_keypair) = create_genesis_config_no_tx_fee(sol_to_lamports(1.0));
    let alice_pubkey = alice_keypair.pubkey();
    let mallory_keypair = Keypair::new();
    let mallory_pubkey = mallory_keypair.pubkey();
    let amount = genesis_config.rent.minimum_balance(0);

    // Fund to account to bypass AccountNotFound error
    let (bank, _bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);
    let bank_client = BankClient::new_shared(bank);
    bank_client
        .transfer_and_confirm(amount, &alice_keypair, &mallory_pubkey)
        .unwrap();

    // Erroneously sign transaction with recipient account key
    // No signature case is tested by bank `test_zero_signatures()`
    let account_metas = vec![
        AccountMeta::new(alice_pubkey, false),
        AccountMeta::new(mallory_pubkey, true),
    ];
    let malicious_instruction = Instruction::new_with_bincode(
        system_program::id(),
        &system_instruction::SystemInstruction::Transfer { lamports: amount },
        account_metas,
    );
    assert_eq!(
        bank_client
            .send_and_confirm_instruction(&mallory_keypair, malicious_instruction)
            .unwrap_err()
            .unwrap(),
        TransactionError::InstructionError(0, InstructionError::MissingRequiredSignature)
    );
    assert_eq!(
        bank_client.get_balance(&alice_pubkey).unwrap(),
        sol_to_lamports(1.0) - amount
    );
    assert_eq!(bank_client.get_balance(&mallory_pubkey).unwrap(), amount);
}

#[test]
fn test_calc_vote_accounts_to_store_empty() {
    let vote_account_rewards = DashMap::default();
    let result = Bank::calc_vote_accounts_to_store(vote_account_rewards);
    assert_eq!(result.rewards.len(), result.accounts_to_store.len());
    assert!(result.rewards.is_empty());
}

#[test]
fn test_calc_vote_accounts_to_store_overflow() {
    let vote_account_rewards = DashMap::default();
    let pubkey = solana_sdk::pubkey::new_rand();
    let mut vote_account = AccountSharedData::default();
    vote_account.set_lamports(u64::MAX);
    vote_account_rewards.insert(
        pubkey,
        VoteReward {
            vote_account,
            commission: 0,
            vote_rewards: 1, // enough to overflow
            vote_needs_store: false,
        },
    );
    let result = Bank::calc_vote_accounts_to_store(vote_account_rewards);
    assert_eq!(result.rewards.len(), result.accounts_to_store.len());
    assert!(result.rewards.is_empty());
}

#[test]
fn test_calc_vote_accounts_to_store_three() {
    let vote_account_rewards = DashMap::default();
    let pubkey = solana_sdk::pubkey::new_rand();
    let pubkey2 = solana_sdk::pubkey::new_rand();
    let pubkey3 = solana_sdk::pubkey::new_rand();
    let mut vote_account = AccountSharedData::default();
    vote_account.set_lamports(u64::MAX);
    vote_account_rewards.insert(
        pubkey,
        VoteReward {
            vote_account: vote_account.clone(),
            commission: 0,
            vote_rewards: 0,
            vote_needs_store: false, // don't store
        },
    );
    vote_account_rewards.insert(
        pubkey2,
        VoteReward {
            vote_account: vote_account.clone(),
            commission: 0,
            vote_rewards: 0,
            vote_needs_store: true, // this one needs storing
        },
    );
    vote_account_rewards.insert(
        pubkey3,
        VoteReward {
            vote_account: vote_account.clone(),
            commission: 0,
            vote_rewards: 0,
            vote_needs_store: false, // don't store
        },
    );
    let result = Bank::calc_vote_accounts_to_store(vote_account_rewards);
    assert_eq!(result.rewards.len(), result.accounts_to_store.len());
    assert_eq!(result.rewards.len(), 3);
    result.rewards.iter().enumerate().for_each(|(i, (k, _))| {
        // pubkey2 is some(account), others should be none
        if k == &pubkey2 {
            assert!(accounts_equal(
                result.accounts_to_store[i].as_ref().unwrap(),
                &vote_account
            ));
        } else {
            assert!(result.accounts_to_store[i].is_none());
        }
    });
}

#[test]
fn test_calc_vote_accounts_to_store_normal() {
    let pubkey = solana_sdk::pubkey::new_rand();
    for commission in 0..2 {
        for vote_rewards in 0..2 {
            for vote_needs_store in [false, true] {
                let vote_account_rewards = DashMap::default();
                let mut vote_account = AccountSharedData::default();
                vote_account.set_lamports(1);
                vote_account_rewards.insert(
                    pubkey,
                    VoteReward {
                        vote_account: vote_account.clone(),
                        commission,
                        vote_rewards,
                        vote_needs_store,
                    },
                );
                let result = Bank::calc_vote_accounts_to_store(vote_account_rewards);
                assert_eq!(result.rewards.len(), result.accounts_to_store.len());
                assert_eq!(result.rewards.len(), 1);
                let rewards = &result.rewards[0];
                let account = &result.accounts_to_store[0];
                _ = vote_account.checked_add_lamports(vote_rewards);
                if vote_needs_store {
                    assert!(accounts_equal(account.as_ref().unwrap(), &vote_account));
                } else {
                    assert!(account.is_none());
                }
                assert_eq!(
                    rewards.1,
                    RewardInfo {
                        reward_type: RewardType::Voting,
                        lamports: vote_rewards as i64,
                        post_balance: vote_account.lamports(),
                        commission: Some(commission),
                    }
                );
                assert_eq!(rewards.0, pubkey);
            }
        }
    }
}

#[test]
fn test_register_hard_fork() {
    fn get_hard_forks(bank: &Bank) -> Vec<Slot> {
        bank.hard_forks().iter().map(|(slot, _)| *slot).collect()
    }

    let (genesis_config, _mint_keypair) = create_genesis_config(10);
    let bank0 = Arc::new(Bank::new_for_tests(&genesis_config));

    let bank7 = Bank::new_from_parent(bank0.clone(), &Pubkey::default(), 7);
    bank7.register_hard_fork(6);
    bank7.register_hard_fork(7);
    bank7.register_hard_fork(8);
    // Bank7 will reject slot 6 since it is older, but allow the other two hard forks
    assert_eq!(get_hard_forks(&bank7), vec![7, 8]);

    let bank9 = Bank::new_from_parent(bank0, &Pubkey::default(), 9);
    bank9.freeze();
    bank9.register_hard_fork(9);
    bank9.register_hard_fork(10);
    // Bank9 will reject slot 9 since it has already been frozen
    assert_eq!(get_hard_forks(&bank9), vec![7, 8, 10]);
}

#[test]
fn test_last_restart_slot() {
    fn last_restart_slot_dirty(bank: &Bank) -> bool {
        let (dirty_accounts, _, _) = bank
            .rc
            .accounts
            .accounts_db
            .get_pubkey_hash_for_slot(bank.slot());
        let dirty_accounts: HashSet<_> = dirty_accounts
            .into_iter()
            .map(|(pubkey, _hash)| pubkey)
            .collect();
        dirty_accounts.contains(&sysvar::last_restart_slot::id())
    }

    fn get_last_restart_slot(bank: &Bank) -> Option<Slot> {
        bank.get_account(&sysvar::last_restart_slot::id())
            .and_then(|account| {
                let lrs: Option<LastRestartSlot> = from_account(&account);
                lrs
            })
            .map(|account| account.last_restart_slot)
    }

    let mint_lamports = 100;
    let validator_stake_lamports = 10;
    let leader_pubkey = Pubkey::default();
    let GenesisConfigInfo {
        mut genesis_config, ..
    } = create_genesis_config_with_leader(mint_lamports, &leader_pubkey, validator_stake_lamports);
    // Remove last restart slot account so we can simluate its' activation
    genesis_config
        .accounts
        .remove(&feature_set::last_restart_slot_sysvar::id())
        .unwrap();

    let bank0 = Arc::new(Bank::new_for_tests(&genesis_config));
    // Register a hard fork in the future so last restart slot will update
    bank0.register_hard_fork(6);
    assert!(!last_restart_slot_dirty(&bank0));
    assert_eq!(get_last_restart_slot(&bank0), None);

    let mut bank1 = Arc::new(Bank::new_from_parent(bank0, &Pubkey::default(), 1));
    assert!(!last_restart_slot_dirty(&bank1));
    assert_eq!(get_last_restart_slot(&bank1), None);

    // Activate the feature in slot 1, it will get initialized in slot 1's children
    Arc::get_mut(&mut bank1)
        .unwrap()
        .activate_feature(&feature_set::last_restart_slot_sysvar::id());
    let bank2 = Arc::new(Bank::new_from_parent(bank1.clone(), &Pubkey::default(), 2));
    assert!(last_restart_slot_dirty(&bank2));
    assert_eq!(get_last_restart_slot(&bank2), Some(0));
    let bank3 = Arc::new(Bank::new_from_parent(bank1, &Pubkey::default(), 3));
    assert!(last_restart_slot_dirty(&bank3));
    assert_eq!(get_last_restart_slot(&bank3), Some(0));

    // Not dirty in children where the last restart slot has not changed
    let bank4 = Arc::new(Bank::new_from_parent(bank2, &Pubkey::default(), 4));
    assert!(!last_restart_slot_dirty(&bank4));
    assert_eq!(get_last_restart_slot(&bank4), Some(0));
    let bank5 = Arc::new(Bank::new_from_parent(bank3, &Pubkey::default(), 5));
    assert!(!last_restart_slot_dirty(&bank5));
    assert_eq!(get_last_restart_slot(&bank5), Some(0));

    // Last restart slot has now changed so it will be dirty again
    let bank6 = Arc::new(Bank::new_from_parent(bank4, &Pubkey::default(), 6));
    assert!(last_restart_slot_dirty(&bank6));
    assert_eq!(get_last_restart_slot(&bank6), Some(6));

    // Last restart will not change for a hard fork that has not occurred yet
    bank6.register_hard_fork(10);
    let bank7 = Arc::new(Bank::new_from_parent(bank6, &Pubkey::default(), 7));
    assert!(!last_restart_slot_dirty(&bank7));
    assert_eq!(get_last_restart_slot(&bank7), Some(6));
}

/// Test that rehashing works with skipped rewrites
///
/// Since `bank_to_xxx_snapshot_archive()` calls `Bank::rehash()`, we must ensure that rehashing
/// works properly when also using `test_skip_rewrites_but_include_in_bank_hash`.
#[test]
fn test_rehash_with_skipped_rewrites() {
    let accounts_db_config = AccountsDbConfig {
        test_skip_rewrites_but_include_in_bank_hash: true,
        ..ACCOUNTS_DB_CONFIG_FOR_TESTING
    };
    let bank = Arc::new(Bank::new_with_paths(
        &GenesisConfig::default(),
        Arc::new(RuntimeConfig::default()),
        Vec::default(),
        None,
        None,
        false,
        Some(accounts_db_config),
        None,
        Some(Pubkey::new_unique()),
        Arc::new(AtomicBool::new(false)),
        None,
        None,
    ));
    // This test is only meaningful while the bank hash contains rewrites.
    // Once this feature is enabled, it may be possible to remove this test entirely.
    assert!(!bank.bank_hash_skips_rent_rewrites());

    // Store an account *in this bank* that will be checked for rent collection *in the next bank*
    let pubkey = {
        let rent_collection_partition = bank
            .variable_cycle_partitions_between_slots(bank.slot(), bank.slot() + 1)
            .last()
            .copied()
            .unwrap();
        let pubkey_range =
            accounts_partition::pubkey_range_from_partition(rent_collection_partition);
        *pubkey_range.end()
    };
    let mut account = AccountSharedData::new(123_456_789, 0, &Pubkey::default());
    // The account's rent epoch must be set to EXEMPT
    // in order for its rewrite to be skipped by rent collection.
    account.set_rent_epoch(RENT_EXEMPT_RENT_EPOCH);
    bank.store_account_and_update_capitalization(&pubkey, &account);

    // Create a new bank that will do rent collection on the account stored in the previous slot
    let bank = Arc::new(Bank::new_from_parent(
        bank.clone(),
        &Pubkey::new_unique(),
        bank.slot() + 1,
    ));

    // Freeze the bank to trigger rent collection and hash calculation
    bank.freeze();

    // Ensure the bank hash is the same before and after rehashing
    let bank_hash = bank.hash();
    bank.rehash();
    let bank_rehash = bank.hash();
    assert_eq!(bank_rehash, bank_hash);
}

/// Test that skipped_rewrites are properly rebuilt when booting from a snapshot
/// that was generated by a node skipping rewrites.
#[test]
fn test_rebuild_skipped_rewrites() {
    let genesis_config = GenesisConfig::default();
    let accounts_db_config = AccountsDbConfig {
        test_skip_rewrites_but_include_in_bank_hash: true,
        ..ACCOUNTS_DB_CONFIG_FOR_TESTING
    };
    let bank = Arc::new(Bank::new_with_paths(
        &genesis_config,
        Arc::new(RuntimeConfig::default()),
        Vec::default(),
        None,
        None,
        false,
        Some(accounts_db_config.clone()),
        None,
        Some(Pubkey::new_unique()),
        Arc::new(AtomicBool::new(false)),
        None,
        None,
    ));
    // This test is only meaningful while the bank hash contains rewrites.
    // Once this feature is enabled, it may be possible to remove this test entirely.
    assert!(!bank.bank_hash_skips_rent_rewrites());

    // Store an account *in this bank* that will be checked for rent collection *in the next bank*
    let pubkey = {
        let rent_collection_partition = bank
            .variable_cycle_partitions_between_slots(bank.slot(), bank.slot() + 1)
            .last()
            .copied()
            .unwrap();
        let pubkey_range =
            accounts_partition::pubkey_range_from_partition(rent_collection_partition);
        *pubkey_range.end()
    };
    let mut account = AccountSharedData::new(123_456_789, 0, &Pubkey::default());
    // The account's rent epoch must be set to EXEMPT
    // in order for its rewrite to be skipped by rent collection.
    account.set_rent_epoch(RENT_EXEMPT_RENT_EPOCH);
    bank.store_account_and_update_capitalization(&pubkey, &account);

    // Create a new bank that will do rent collection on the account stored in the previous slot
    let bank = Arc::new(Bank::new_from_parent(
        bank.clone(),
        &Pubkey::new_unique(),
        bank.slot() + 1,
    ));

    // This fn is called within freeze(), but freeze() *consumes* Self::skipped_rewrites!
    // For testing, we want to know what's in the skipped rewrites, so we perform
    // rent collection manually.
    bank.collect_rent_eagerly();
    let actual_skipped_rewrites = bank.skipped_rewrites.lock().unwrap().clone();
    // Ensure skipped rewrites now includes the account we stored above
    assert!(actual_skipped_rewrites.contains_key(&pubkey));
    // Ensure the calculated skipped rewrites match the actual ones
    let calculated_skipped_rewrites = bank.calculate_skipped_rewrites();
    assert_eq!(calculated_skipped_rewrites, actual_skipped_rewrites);

    // required in order to snapshot the bank
    bank.fill_bank_with_ticks_for_tests();

    // Now take a snapshot!
    let (_tmp_dir, accounts_dir) = snapshot_utils::create_tmp_accounts_dir_for_tests();
    let bank_snapshots_dir = TempDir::new().unwrap();
    let full_snapshot_archives_dir = TempDir::new().unwrap();
    let incremental_snapshot_archives_dir = TempDir::new().unwrap();
    let full_snapshot_archive = snapshot_bank_utils::bank_to_full_snapshot_archive(
        bank_snapshots_dir.path(),
        &bank,
        None,
        full_snapshot_archives_dir.path(),
        incremental_snapshot_archives_dir.path(),
        snapshot_utils::ArchiveFormat::Tar,
    )
    .unwrap();

    // Rebuild the bank and ensure it passes verification
    let (snapshot_bank, _) = snapshot_bank_utils::bank_from_snapshot_archives(
        &[accounts_dir],
        bank_snapshots_dir.path(),
        &full_snapshot_archive,
        None,
        &genesis_config,
        &RuntimeConfig::default(),
        None,
        None,
        None,
        false,
        false,
        false,
        false,
        Some(ACCOUNTS_DB_CONFIG_FOR_TESTING),
        None,
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    snapshot_bank.wait_for_initial_accounts_hash_verification_completed_for_tests();
    assert_eq!(bank.as_ref(), &snapshot_bank);

    // Ensure the snapshot bank's skipped rewrites match the original bank's
    let snapshot_skipped_rewrites = snapshot_bank.calculate_skipped_rewrites();
    assert_eq!(snapshot_skipped_rewrites, actual_skipped_rewrites);
}

/// Test that getting accounts for BankHashDetails works with skipped rewrites
#[test_case(true; "skip rewrites")]
#[test_case(false; "do rewrites")]
fn test_get_accounts_for_bank_hash_details(skip_rewrites: bool) {
    let genesis_config = GenesisConfig::default();
    let accounts_db_config = AccountsDbConfig {
        test_skip_rewrites_but_include_in_bank_hash: skip_rewrites,
        ..ACCOUNTS_DB_CONFIG_FOR_TESTING
    };
    let bank = Arc::new(Bank::new_with_paths(
        &genesis_config,
        Arc::new(RuntimeConfig::default()),
        Vec::default(),
        None,
        None,
        false,
        Some(accounts_db_config.clone()),
        None,
        Some(Pubkey::new_unique()),
        Arc::new(AtomicBool::new(false)),
        None,
        None,
    ));
    // This test is only meaningful while the bank hash contains rewrites.
    // Once this feature is enabled, it may be possible to remove this test entirely.
    assert!(!bank.bank_hash_skips_rent_rewrites());

    // Store an account *in this bank* that will be checked for rent collection *in the next bank*
    let pubkey = {
        let rent_collection_partition = bank
            .variable_cycle_partitions_between_slots(bank.slot(), bank.slot() + 1)
            .last()
            .copied()
            .unwrap();
        let pubkey_range =
            accounts_partition::pubkey_range_from_partition(rent_collection_partition);
        *pubkey_range.end()
    };
    let mut account = AccountSharedData::new(123_456_789, 0, &Pubkey::default());
    // The account's rent epoch must be set to EXEMPT
    // in order for its rewrite to be skipped by rent collection.
    account.set_rent_epoch(RENT_EXEMPT_RENT_EPOCH);
    bank.store_account_and_update_capitalization(&pubkey, &account);

    // Create a new bank that will do rent collection on the account stored in the previous slot
    let bank = Bank::new_from_parent(bank.clone(), &Pubkey::new_unique(), bank.slot() + 1);

    // Freeze the bank to do rent collection and calculate the accounts delta hash
    bank.freeze();

    // Ensure that the accounts returned by `get_accounts_for_bank_hash_details()` produces the
    // same AccountsDeltaHash as the actual value stored in the Bank.
    let calculated_accounts_delta_hash = {
        let accounts = bank.get_accounts_for_bank_hash_details();
        let hashes = accounts
            .into_iter()
            .map(|account| (account.pubkey, account.hash))
            .collect();
        AccountsDeltaHash(AccountsHasher::accumulate_account_hashes(hashes))
    };
    let actual_accounts_delta_hash = bank
        .rc
        .accounts
        .accounts_db
        .get_accounts_delta_hash(bank.slot())
        .unwrap();
    assert_eq!(calculated_accounts_delta_hash, actual_accounts_delta_hash);
}

/// Test that simulations report the compute units of failed transactions
#[test]
fn test_failed_simulation_compute_units() {
    let (genesis_config, mint_keypair) = create_genesis_config(LAMPORTS_PER_SOL);
    let program_id = Pubkey::new_unique();
    let (bank, _bank_forks) =
        Bank::new_with_mockup_builtin_for_tests(&genesis_config, program_id, MockBuiltin::vm);

    const TEST_UNITS: u64 = 10_000;
    const MOCK_BUILTIN_UNITS: u64 = 1;
    let expected_consumed_units = TEST_UNITS + MOCK_BUILTIN_UNITS;
    declare_process_instruction!(MockBuiltin, MOCK_BUILTIN_UNITS, |invoke_context| {
        invoke_context.consume_checked(TEST_UNITS).unwrap();
        Err(InstructionError::InvalidInstructionData)
    });

    let message = Message::new(
        &[Instruction::new_with_bincode(program_id, &0, vec![])],
        Some(&mint_keypair.pubkey()),
    );
    let transaction = Transaction::new(&[&mint_keypair], message, bank.last_blockhash());

    bank.freeze();
    let sanitized = SanitizedTransaction::from_transaction_for_tests(transaction);
    let simulation = bank.simulate_transaction(&sanitized, false);
    assert_eq!(expected_consumed_units, simulation.units_consumed);
}

/// Test that simulations report the load error of fees-only transactions
#[test]
fn test_failed_simulation_load_error() {
    let (genesis_config, mint_keypair) = create_genesis_config(LAMPORTS_PER_SOL);
    let bank = Bank::new_for_tests(&genesis_config);
    let (bank, _bank_forks) = bank.wrap_with_bank_forks_for_tests();
    let missing_program_id = Pubkey::new_unique();
    let message = Message::new(
        &[Instruction::new_with_bincode(
            missing_program_id,
            &0,
            vec![],
        )],
        Some(&mint_keypair.pubkey()),
    );
    let transaction = Transaction::new(&[&mint_keypair], message, bank.last_blockhash());

    bank.freeze();
    let sanitized = SanitizedTransaction::from_transaction_for_tests(transaction);
    let simulation = bank.simulate_transaction(&sanitized, false);
    assert_eq!(
        simulation,
        TransactionSimulationResult {
            result: Err(TransactionError::ProgramAccountNotFound),
            logs: vec![],
            post_simulation_accounts: vec![],
            units_consumed: 0,
            return_data: None,
            inner_instructions: None,
        }
    );
}

#[test]
fn test_filter_program_errors_and_collect_fee_details() {
    // TX  | PROCESSING RESULT           | COLLECT            | COLLECT
    //     |                             | (TX_FEE, PRIO_FEE) | RESULT
    // ---------------------------------------------------------------------------------
    // tx1 | not processed               | (0    , 0)         | Original Err
    // tx2 | processed but not executed  | (5_000, 1_000)     | Ok
    // tx3 | executed has error          | (5_000, 1_000)     | Ok
    // tx4 | executed and no error       | (5_000, 1_000)     | Ok
    //
    let initial_payer_balance = 7_000;
    let tx_fee = 5000;
    let priority_fee = 1000;
    let fee_details = FeeDetails::new(tx_fee, priority_fee, false);
    let expected_collected_fee_details = CollectorFeeDetails {
        transaction_fee: 3 * tx_fee,
        priority_fee: 3 * priority_fee,
    };

    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config_with_leader(initial_payer_balance, &Pubkey::new_unique(), 3);
    let bank = Bank::new_for_tests(&genesis_config);

    let results = vec![
        Err(TransactionError::AccountNotFound),
        Ok(ProcessedTransaction::FeesOnly(Box::new(
            FeesOnlyTransaction {
                load_error: TransactionError::InvalidProgramForExecution,
                rollback_accounts: RollbackAccounts::default(),
                fee_details,
            },
        ))),
        new_executed_processing_result(
            Err(TransactionError::InstructionError(
                0,
                SystemError::ResultWithNegativeLamports.into(),
            )),
            fee_details,
        ),
        new_executed_processing_result(Ok(()), fee_details),
    ];

    bank.filter_program_errors_and_collect_fee_details(&results);

    assert_eq!(
        expected_collected_fee_details,
        *bank.collector_fee_details.read().unwrap()
    );
    assert_eq!(
        initial_payer_balance,
        bank.get_balance(&mint_keypair.pubkey())
    );
}

#[test]
fn test_deploy_last_epoch_slot() {
    solana_logger::setup();

    // Bank Setup
    let (mut genesis_config, mint_keypair) = create_genesis_config(1_000_000 * LAMPORTS_PER_SOL);
    genesis_config
        .accounts
        .remove(&feature_set::disable_fees_sysvar::id());
    let mut bank = Bank::new_for_tests(&genesis_config);
    bank.activate_feature(&feature_set::disable_fees_sysvar::id());

    // go to the last slot in the epoch
    let (bank, bank_forks) = bank.wrap_with_bank_forks_for_tests();
    let slots_in_epoch = bank.epoch_schedule().get_slots_in_epoch(0);
    let bank = new_bank_from_parent_with_bank_forks(
        &bank_forks,
        bank,
        &Pubkey::default(),
        slots_in_epoch - 1,
    );
    eprintln!("now at slot {} epoch {}", bank.slot(), bank.epoch());

    // deploy a program
    let payer_keypair = Keypair::new();
    let program_keypair = Keypair::new();
    let mut file = File::open("../programs/bpf_loader/test_elfs/out/noop_aligned.so").unwrap();
    let mut elf = Vec::new();
    file.read_to_end(&mut elf).unwrap();
    let min_program_balance =
        bank.get_minimum_balance_for_rent_exemption(UpgradeableLoaderState::size_of_program());
    let min_buffer_balance = bank
        .get_minimum_balance_for_rent_exemption(UpgradeableLoaderState::size_of_buffer(elf.len()));
    let min_programdata_balance = bank.get_minimum_balance_for_rent_exemption(
        UpgradeableLoaderState::size_of_programdata(elf.len()),
    );
    let buffer_address = Pubkey::new_unique();
    let (programdata_address, _) = Pubkey::find_program_address(
        &[program_keypair.pubkey().as_ref()],
        &bpf_loader_upgradeable::id(),
    );
    let upgrade_authority_keypair = Keypair::new();

    let buffer_account = {
        let mut account = AccountSharedData::new(
            min_buffer_balance,
            UpgradeableLoaderState::size_of_buffer(elf.len()),
            &bpf_loader_upgradeable::id(),
        );
        account
            .set_state(&UpgradeableLoaderState::Buffer {
                authority_address: Some(upgrade_authority_keypair.pubkey()),
            })
            .unwrap();
        account
            .data_as_mut_slice()
            .get_mut(UpgradeableLoaderState::size_of_buffer_metadata()..)
            .unwrap()
            .copy_from_slice(&elf);
        account
    };

    let payer_base_balance = LAMPORTS_PER_SOL;
    let deploy_fees = {
        let fee_calculator = genesis_config.fee_rate_governor.create_fee_calculator();
        3 * fee_calculator.lamports_per_signature
    };
    let min_payer_balance = min_program_balance
        .saturating_add(min_programdata_balance)
        .saturating_sub(min_buffer_balance.saturating_add(deploy_fees));
    bank.store_account(
        &payer_keypair.pubkey(),
        &AccountSharedData::new(
            payer_base_balance.saturating_add(min_payer_balance),
            0,
            &system_program::id(),
        ),
    );
    bank.store_account(&buffer_address, &buffer_account);
    bank.store_account(&program_keypair.pubkey(), &AccountSharedData::default());
    bank.store_account(&programdata_address, &AccountSharedData::default());
    let message = Message::new(
        &bpf_loader_upgradeable::deploy_with_max_program_len(
            &payer_keypair.pubkey(),
            &program_keypair.pubkey(),
            &buffer_address,
            &upgrade_authority_keypair.pubkey(),
            min_program_balance,
            elf.len(),
        )
        .unwrap(),
        Some(&payer_keypair.pubkey()),
    );
    let signers = &[&payer_keypair, &program_keypair, &upgrade_authority_keypair];
    let transaction = Transaction::new(signers, message.clone(), bank.last_blockhash());
    let ret = bank.process_transaction(&transaction);
    assert!(ret.is_ok(), "ret: {:?}", ret);
    goto_end_of_slot(bank.clone());

    // go to the first slot in the new epoch
    let bank =
        new_bank_from_parent_with_bank_forks(&bank_forks, bank, &Pubkey::default(), slots_in_epoch);
    eprintln!("now at slot {} epoch {}", bank.slot(), bank.epoch());

    let instruction = Instruction::new_with_bytes(program_keypair.pubkey(), &[], Vec::new());
    let message = Message::new(&[instruction], Some(&mint_keypair.pubkey()));
    let binding = mint_keypair.insecure_clone();
    let signers = vec![&binding];
    let transaction = Transaction::new(&signers, message, bank.last_blockhash());
    let result_with_feature_enabled = bank.process_transaction(&transaction);
    assert_eq!(result_with_feature_enabled, Ok(()));
}

#[test]
fn test_blockhash_last_valid_block_height() {
    let genesis_config = GenesisConfig::default();
    let mut bank = Arc::new(Bank::new_for_tests(&genesis_config));

    // valid until MAX_PROCESSING_AGE
    let last_blockhash = bank.last_blockhash();
    for i in 1..=MAX_PROCESSING_AGE as u64 {
        goto_end_of_slot(bank.clone());
        bank = Arc::new(new_from_parent(bank));
        assert_eq!(i, bank.block_height);

        let last_valid_block_height = bank
            .get_blockhash_last_valid_block_height(&last_blockhash)
            .unwrap();
        assert_eq!(
            last_valid_block_height,
            bank.block_height + MAX_PROCESSING_AGE as u64 - i
        );
        assert!(bank.is_blockhash_valid(&last_blockhash));
    }

    // Make sure it stays in the queue until `MAX_RECENT_BLOCKHASHES`
    for i in MAX_PROCESSING_AGE + 1..=MAX_RECENT_BLOCKHASHES {
        goto_end_of_slot(bank.clone());
        bank = Arc::new(new_from_parent(bank));
        assert_eq!(bank.block_height, i as u64);

        // the blockhash is outside of the processing age, but still present in the queue,
        // so the call to get the block height still works even though it's in the past
        let last_valid_block_height = bank
            .get_blockhash_last_valid_block_height(&last_blockhash)
            .unwrap();
        assert_eq!(last_valid_block_height, MAX_PROCESSING_AGE as u64);

        // But it isn't valid for processing
        assert!(!bank.is_blockhash_valid(&last_blockhash));
    }

    // one past MAX_RECENT_BLOCKHASHES is no longer present at all
    goto_end_of_slot(bank.clone());
    bank = Arc::new(new_from_parent(bank));
    assert_eq!(
        bank.get_blockhash_last_valid_block_height(&last_blockhash),
        None
    );
    assert!(!bank.is_blockhash_valid(&last_blockhash));
}

#[test]
fn test_bank_epoch_stakes() {
    solana_logger::setup();
    let num_of_nodes: u64 = 30;
    let stakes = (1..num_of_nodes.checked_add(1).expect("Shouldn't be big")).collect::<Vec<_>>();
    let voting_keypairs = stakes
        .iter()
        .map(|_| ValidatorVoteKeypairs::new_rand())
        .collect::<Vec<_>>();
    let total_stake = stakes.iter().sum();
    let GenesisConfigInfo { genesis_config, .. } =
        create_genesis_config_with_vote_accounts(1_000_000_000, &voting_keypairs, stakes.clone());

    let bank0 = Arc::new(Bank::new_for_tests(&genesis_config));
    let mut bank1 = Bank::new_from_parent(
        bank0.clone(),
        &Pubkey::default(),
        bank0.get_slots_in_epoch(0) + 1,
    );

    let initial_epochs = bank0.epoch_stake_keys();
    assert_eq!(initial_epochs, vec![0, 1]);

    // Bank 0:

    // First query with bank's epoch. As noted by the above check, the epoch
    // stakes cache actually contains values for the _leader schedule_ epoch
    // (N + 1). Therefore, we should be able to query both.
    assert_eq!(bank0.epoch(), 0);
    assert_eq!(bank0.epoch_total_stake(0), Some(total_stake));
    assert_eq!(bank0.epoch_node_id_to_stake(0, &Pubkey::new_unique()), None);
    for (i, keypair) in voting_keypairs.iter().enumerate() {
        assert_eq!(
            bank0.epoch_node_id_to_stake(0, &keypair.node_keypair.pubkey()),
            Some(stakes[i])
        );
    }

    // Now query for epoch 1 on bank 0.
    assert_eq!(bank0.epoch().saturating_add(1), 1);
    assert_eq!(bank0.epoch_total_stake(1), Some(total_stake));
    assert_eq!(bank0.epoch_node_id_to_stake(1, &Pubkey::new_unique()), None);
    for (i, keypair) in voting_keypairs.iter().enumerate() {
        assert_eq!(
            bank0.epoch_node_id_to_stake(1, &keypair.node_keypair.pubkey()),
            Some(stakes[i])
        );
    }

    // Note using bank's `current_epoch_stake_*` methods should return the
    // same values.
    assert_eq!(bank0.get_current_epoch_total_stake(), total_stake);
    assert_eq!(
        bank0.get_current_epoch_total_stake(),
        bank0.epoch_total_stake(1).unwrap(),
    );
    assert_eq!(
        bank0.get_current_epoch_vote_accounts().len(),
        voting_keypairs.len()
    );
    assert_eq!(
        bank0.epoch_vote_accounts(1).unwrap(),
        bank0.get_current_epoch_vote_accounts(),
    );

    // Bank 1:

    // Run the same exercise. First query the bank's epoch.
    assert_eq!(bank1.epoch(), 1);
    assert_eq!(bank1.epoch_total_stake(1), Some(total_stake));
    assert_eq!(bank1.epoch_node_id_to_stake(1, &Pubkey::new_unique()), None);
    for (i, keypair) in voting_keypairs.iter().enumerate() {
        assert_eq!(
            bank1.epoch_node_id_to_stake(1, &keypair.node_keypair.pubkey()),
            Some(stakes[i])
        );
    }

    // Now query for epoch 2 on bank 1.
    assert_eq!(bank1.epoch().saturating_add(1), 2);
    assert_eq!(bank1.epoch_total_stake(2), Some(total_stake));
    assert_eq!(bank1.epoch_node_id_to_stake(2, &Pubkey::new_unique()), None);
    for (i, keypair) in voting_keypairs.iter().enumerate() {
        assert_eq!(
            bank1.epoch_node_id_to_stake(2, &keypair.node_keypair.pubkey()),
            Some(stakes[i])
        );
    }

    // Again, using bank's `current_epoch_stake_*` methods should return the
    // same values.
    assert_eq!(bank1.get_current_epoch_total_stake(), total_stake);
    assert_eq!(
        bank1.get_current_epoch_total_stake(),
        bank1.epoch_total_stake(2).unwrap(),
    );
    assert_eq!(
        bank1.get_current_epoch_vote_accounts().len(),
        voting_keypairs.len()
    );
    assert_eq!(
        bank1.epoch_vote_accounts(2).unwrap(),
        bank1.get_current_epoch_vote_accounts(),
    );

    // Setup new epoch stakes on Bank 1 for both leader schedule epochs.
    let make_new_epoch_stakes = |stake_coefficient: u64| {
        EpochStakes::new_for_tests(
            voting_keypairs
                .iter()
                .map(|keypair| {
                    let node_id = keypair.node_keypair.pubkey();
                    let authorized_voter = keypair.vote_keypair.pubkey();
                    let vote_account = VoteAccount::try_from(create_account_with_authorized(
                        &node_id,
                        &authorized_voter,
                        &node_id,
                        0,
                        100,
                    ))
                    .unwrap();
                    (authorized_voter, (stake_coefficient, vote_account))
                })
                .collect::<HashMap<_, _>>(),
            1,
        )
    };
    let stake_coefficient_epoch_1 = 100;
    let stake_coefficient_epoch_2 = 500;
    bank1.set_epoch_stakes_for_test(1, make_new_epoch_stakes(stake_coefficient_epoch_1));
    bank1.set_epoch_stakes_for_test(2, make_new_epoch_stakes(stake_coefficient_epoch_2));

    // Run the exercise again with the new stake. Now epoch 1 should have the
    // stake added for epoch 1 (`stake_coefficient_epoch_1`).
    assert_eq!(
        bank1.epoch_total_stake(1),
        Some(stake_coefficient_epoch_1 * num_of_nodes)
    );
    assert_eq!(bank1.epoch_node_id_to_stake(1, &Pubkey::new_unique()), None);
    for keypair in voting_keypairs.iter() {
        assert_eq!(
            bank1.epoch_node_id_to_stake(1, &keypair.node_keypair.pubkey()),
            Some(stake_coefficient_epoch_1)
        );
    }

    // Now query for epoch 2 on bank 1. Epoch 2 should have the stake added for
    // epoch 2 (`stake_coefficient_epoch_2`).
    assert_eq!(
        bank1.epoch_total_stake(2),
        Some(stake_coefficient_epoch_2 * num_of_nodes)
    );
    assert_eq!(bank1.epoch_node_id_to_stake(2, &Pubkey::new_unique()), None);
    for keypair in voting_keypairs.iter() {
        assert_eq!(
            bank1.epoch_node_id_to_stake(2, &keypair.node_keypair.pubkey()),
            Some(stake_coefficient_epoch_2)
        );
    }
}

#[test]
fn test_rehash_good() {
    let ten_sol = 10 * LAMPORTS_PER_SOL;
    let (genesis_config, _mint) = create_genesis_config(ten_sol);
    let bank = Bank::new_for_tests(&genesis_config);

    let lamports = 123_456_789;
    let account = AccountSharedData::new(lamports, 0, &Pubkey::default());
    let pubkey = Pubkey::new_unique();
    bank.store_account_and_update_capitalization(&pubkey, &account);

    // freeze the bank to trigger hash calculation
    bank.freeze();

    // ensure the bank hash is the same before and after rehashing
    let prev_bank_hash = bank.hash();
    bank.rehash();
    let post_bank_hash = bank.hash();
    assert_eq!(post_bank_hash, prev_bank_hash);
}

#[test]
#[should_panic(expected = "rehashing is not allowed to change the account state")]
fn test_rehash_bad() {
    let ten_sol = 10 * LAMPORTS_PER_SOL;
    let (genesis_config, _mint) = create_genesis_config(ten_sol);
    let bank = Bank::new_for_tests(&genesis_config);

    let mut account = AccountSharedData::new(ten_sol, 0, &Pubkey::default());
    let pubkey = Pubkey::new_unique();
    bank.store_account_and_update_capitalization(&pubkey, &account);

    // freeze the bank to trigger hash calculation
    bank.freeze();

    // change an account, which will cause rehashing to panic
    account.checked_add_lamports(ten_sol).unwrap();
    bank.rc
        .accounts
        .store_accounts_cached((bank.slot(), [(&pubkey, &account)].as_slice()));

    // let the show begin
    bank.rehash();
}
