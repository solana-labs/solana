use {
    crate::{
        mock_bank::{MockBankCallback, MockForkGraph},
        proto::InstrFixture,
        transaction_builder::SanitizedTransactionBuilder,
    },
    lazy_static::lazy_static,
    prost::Message,
    solana_bpf_loader_program::syscalls::create_program_runtime_environment_v1,
    solana_compute_budget::compute_budget::ComputeBudget,
    solana_program_runtime::{
        loaded_programs::{ProgramCache, ProgramCacheEntry, ProgramRuntimeEnvironments},
        solana_rbpf::{
            program::{BuiltinProgram, FunctionRegistry},
            vm::Config,
        },
        timings::ExecuteTimings,
    },
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount, WritableAccount},
        bpf_loader_upgradeable,
        epoch_schedule::EpochSchedule,
        feature_set::{FeatureSet, FEATURE_NAMES},
        hash::Hash,
        instruction::AccountMeta,
        pubkey::Pubkey,
        signature::Signature,
    },
    solana_svm::{
        account_loader::CheckedTransactionDetails,
        runtime_config::RuntimeConfig,
        transaction_error_metrics::TransactionErrorMetrics,
        transaction_processor::{
            ExecutionRecordingConfig, TransactionBatchProcessor, TransactionProcessingConfig,
        },
    },
    std::{
        collections::{HashMap, HashSet},
        env,
        ffi::OsString,
        fs::{self, File},
        io::Read,
        path::PathBuf,
        process::Command,
        sync::{Arc, RwLock},
    },
};

mod proto {
    include!(concat!(env!("OUT_DIR"), "/org.solana.sealevel.v1.rs"));
}
mod mock_bank;
mod transaction_builder;

const fn feature_u64(feature: &Pubkey) -> u64 {
    let feature_id = feature.to_bytes();
    feature_id[0] as u64
        | (feature_id[1] as u64) << 8
        | (feature_id[2] as u64) << 16
        | (feature_id[3] as u64) << 24
        | (feature_id[4] as u64) << 32
        | (feature_id[5] as u64) << 40
        | (feature_id[6] as u64) << 48
        | (feature_id[7] as u64) << 56
}

lazy_static! {
    static ref INDEXED_FEATURES: HashMap<u64, Pubkey> = {
        FEATURE_NAMES
            .iter()
            .map(|(pubkey, _)| (feature_u64(pubkey), *pubkey))
            .collect()
    };
}

fn setup() -> PathBuf {
    let mut dir = env::current_dir().unwrap();
    dir.push("test-vectors");
    if !dir.exists() {
        std::println!("Cloning test-vectors ...");
        Command::new("git")
            .args([
                "clone",
                "https://github.com/firedancer-io/test-vectors",
                dir.as_os_str().to_str().unwrap(),
            ])
            .status()
            .expect("Failed to download test-vectors");

        std::println!("Checking out commit e94f22066cb2e98f8fe5f05a6797b8111ad78265");
        Command::new("git")
            .current_dir(&dir)
            .args(["checkout", "e94f22066cb2e98f8fe5f05a6797b8111ad78265"])
            .status()
            .expect("Failed to checkout to proper test-vector commit");

        std::println!("Setup done!");
    }

    dir
}

fn cleanup() {
    let mut dir = env::current_dir().unwrap();
    dir.push("test-vectors");

    if dir.exists() {
        fs::remove_dir_all(dir).expect("Failed to delete test-vectors repository");
    }
}

#[test]
fn execute_fixtures() {
    let mut base_dir = setup();
    base_dir.push("instr");
    base_dir.push("fixtures");

    // bpf-loader tests
    base_dir.push("bpf-loader");
    for path in std::fs::read_dir(base_dir).unwrap() {
        let filename = path.as_ref().unwrap().file_name();
        let mut file = File::open(path.as_ref().unwrap().path()).expect("file not found");
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer).expect("Failed to read file");

        let fixture = proto::InstrFixture::decode(buffer.as_slice()).unwrap();
        run_fixture(fixture, filename);
    }

    cleanup();
}

fn run_fixture(fixture: InstrFixture, filename: OsString) {
    let input = fixture.input.unwrap();
    let output = fixture.output.unwrap();

    let mut transaction_builder = SanitizedTransactionBuilder::default();
    let program_id = Pubkey::new_from_array(input.program_id.try_into().unwrap());
    let mut accounts: Vec<AccountMeta> = Vec::with_capacity(input.instr_accounts.len());
    let mut signatures: HashMap<Pubkey, Signature> =
        HashMap::with_capacity(input.instr_accounts.len());

    for item in input.instr_accounts {
        let pubkey = Pubkey::new_from_array(
            input.accounts[item.index as usize]
                .address
                .clone()
                .try_into()
                .unwrap(),
        );
        accounts.push(AccountMeta {
            pubkey,
            is_signer: item.is_signer,
            is_writable: item.is_writable,
        });

        if item.is_signer {
            signatures.insert(pubkey, Signature::new_unique());
        }
    }

    transaction_builder.create_instruction(program_id, accounts, signatures, input.data);

    let mut feature_set = FeatureSet::default();
    if let Some(features) = &input.epoch_context.as_ref().unwrap().features {
        for id in &features.features {
            if let Some(pubkey) = INDEXED_FEATURES.get(id) {
                feature_set.activate(pubkey, 0);
            }
        }
    }

    let fee_payer = Pubkey::new_unique();
    let Ok(transaction) =
        transaction_builder.build(Hash::default(), (fee_payer, Signature::new_unique()), false)
    else {
        // If we can't build a sanitized transaction,
        // the output must be a failed instruction as well
        assert_ne!(output.result, 0);
        return;
    };

    let transactions = vec![transaction];
    let mut transaction_check = vec![Ok(CheckedTransactionDetails {
        nonce: None,
        lamports_per_signature: 30,
    })];

    let mut mock_bank = MockBankCallback::default();
    {
        let mut account_data_map = mock_bank.account_shared_data.borrow_mut();
        for item in input.accounts {
            let pubkey = Pubkey::new_from_array(item.address.try_into().unwrap());
            let mut account_data = AccountSharedData::default();
            account_data.set_lamports(item.lamports);
            account_data.set_data(item.data);
            account_data.set_owner(Pubkey::new_from_array(item.owner.try_into().unwrap()));
            account_data.set_executable(item.executable);
            account_data.set_rent_epoch(item.rent_epoch);

            account_data_map.insert(pubkey, account_data);
        }
        let mut account_data = AccountSharedData::default();
        account_data.set_lamports(800000);
        account_data_map.insert(fee_payer, account_data);
    }

    let compute_budget = ComputeBudget {
        compute_unit_limit: input.cu_avail,
        ..ComputeBudget::default()
    };

    let v1_environment =
        create_program_runtime_environment_v1(&feature_set, &compute_budget, false, false).unwrap();

    let mut program_cache = ProgramCache::<MockForkGraph>::new(0, 20);
    program_cache.environments = ProgramRuntimeEnvironments {
        program_runtime_v1: Arc::new(v1_environment),
        program_runtime_v2: Arc::new(BuiltinProgram::new_loader(
            Config::default(),
            FunctionRegistry::default(),
        )),
    };
    program_cache.fork_graph = Some(Arc::new(RwLock::new(MockForkGraph {})));

    let program_cache = Arc::new(RwLock::new(program_cache));
    mock_bank.override_feature_set(feature_set);
    let batch_processor = TransactionBatchProcessor::<MockForkGraph>::new(
        42,
        2,
        EpochSchedule::default(),
        Arc::new(RuntimeConfig::default()),
        program_cache.clone(),
        HashSet::new(),
    );

    batch_processor.fill_missing_sysvar_cache_entries(&mock_bank);
    register_builtins(&batch_processor, &mock_bank);

    let mut error_counter = TransactionErrorMetrics::default();
    let recording_config = ExecutionRecordingConfig {
        enable_log_recording: true,
        enable_return_data_recording: true,
        enable_cpi_recording: false,
    };
    let processor_config = TransactionProcessingConfig {
        account_overrides: None,
        log_messages_bytes_limit: None,
        limit_to_load_programs: true,
        recording_config,
    };
    let mut timings = ExecuteTimings::default();

    let result = batch_processor.load_and_execute_sanitized_transactions(
        &mock_bank,
        &transactions,
        transaction_check.as_mut_slice(),
        &mut error_counter,
        &mut timings,
        &processor_config,
    );

    // Assert that the transaction has worked without errors.
    if !result.execution_results[0].was_executed()
        || result.execution_results[0]
            .details()
            .unwrap()
            .status
            .is_err()
    {
        assert_ne!(
            output.result, 0,
            "Transaction was not successful, but should have been: file {:?}",
            filename
        );
        return;
    }

    // Check modified accounts
    let idx_map: HashMap<Pubkey, usize> = result.loaded_transactions[0]
        .as_ref()
        .unwrap()
        .accounts
        .iter()
        .enumerate()
        .map(|(idx, item)| (item.0, idx))
        .collect();

    for item in &output.modified_accounts {
        let pubkey = Pubkey::new_from_array(item.address.clone().try_into().unwrap());
        let index = *idx_map
            .get(&pubkey)
            .expect("Account not in expected results");
        let received_data = &result.loaded_transactions[0].as_ref().unwrap().accounts[index].1;

        assert_eq!(
            received_data.lamports(),
            item.lamports,
            "Lamports differ in case: {:?}",
            filename
        );
        assert_eq!(
            received_data.data(),
            item.data.as_slice(),
            "Account data differs in case: {:?}",
            filename
        );
        assert_eq!(
            received_data.owner(),
            &Pubkey::new_from_array(item.owner.clone().try_into().unwrap()),
            "Account owner differs in case: {:?}",
            filename
        );
        assert_eq!(
            received_data.executable(),
            item.executable,
            "Executable boolean differs in case: {:?}",
            filename
        );

        // u64::MAX means we are not considering the epoch
        if item.rent_epoch != u64::MAX && received_data.rent_epoch() != u64::MAX {
            assert_eq!(
                received_data.rent_epoch(),
                item.rent_epoch,
                "Rent epoch differs in case: {:?}",
                filename
            );
        }
    }

    let execution_details = result.execution_results[0].details().unwrap();
    assert_eq!(
        execution_details.executed_units,
        input.cu_avail.saturating_sub(output.cu_avail),
        "Execution units differs in case: {:?}",
        filename
    );

    if let Some(return_data) = &execution_details.return_data {
        assert_eq!(return_data.data, output.return_data);
    } else {
        assert_eq!(output.return_data.len(), 0);
    }
}

fn register_builtins(
    batch_processor: &TransactionBatchProcessor<MockForkGraph>,
    mock_bank: &MockBankCallback,
) {
    let bpf_loader = "solana_bpf_loader_upgradeable_program";
    batch_processor.add_builtin(
        mock_bank,
        bpf_loader_upgradeable::id(),
        bpf_loader,
        ProgramCacheEntry::new_builtin(
            0,
            bpf_loader.len(),
            solana_bpf_loader_program::Entrypoint::vm,
        ),
    );
}
