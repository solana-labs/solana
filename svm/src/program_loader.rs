use {
    crate::transaction_processing_callback::TransactionProcessingCallback,
    solana_program_runtime::{
        loaded_programs::{
            ForkGraph, LoadProgramMetrics, ProgramCache, ProgramCacheEntry, ProgramCacheEntryOwner,
            ProgramCacheEntryType, ProgramRuntimeEnvironment, DELAY_VISIBILITY_SLOT_OFFSET,
        },
        timings::ExecuteDetailsTimings,
    },
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount},
        account_utils::StateMut,
        bpf_loader, bpf_loader_deprecated,
        bpf_loader_upgradeable::UpgradeableLoaderState,
        clock::{Epoch, Slot},
        epoch_schedule::EpochSchedule,
        instruction::InstructionError,
        loader_v4::{self, LoaderV4State, LoaderV4Status},
        pubkey::Pubkey,
    },
    std::sync::Arc,
};

#[derive(Debug)]
pub(crate) enum ProgramAccountLoadResult {
    InvalidAccountData(ProgramCacheEntryOwner),
    ProgramOfLoaderV1(AccountSharedData),
    ProgramOfLoaderV2(AccountSharedData),
    ProgramOfLoaderV3(AccountSharedData, AccountSharedData, Slot),
    ProgramOfLoaderV4(AccountSharedData, Slot),
}

pub(crate) fn load_program_from_bytes(
    load_program_metrics: &mut LoadProgramMetrics,
    programdata: &[u8],
    loader_key: &Pubkey,
    account_size: usize,
    deployment_slot: Slot,
    program_runtime_environment: ProgramRuntimeEnvironment,
    reloading: bool,
) -> std::result::Result<ProgramCacheEntry, Box<dyn std::error::Error>> {
    if reloading {
        // Safety: this is safe because the program is being reloaded in the cache.
        unsafe {
            ProgramCacheEntry::reload(
                loader_key,
                program_runtime_environment.clone(),
                deployment_slot,
                deployment_slot.saturating_add(DELAY_VISIBILITY_SLOT_OFFSET),
                programdata,
                account_size,
                load_program_metrics,
            )
        }
    } else {
        ProgramCacheEntry::new(
            loader_key,
            program_runtime_environment.clone(),
            deployment_slot,
            deployment_slot.saturating_add(DELAY_VISIBILITY_SLOT_OFFSET),
            programdata,
            account_size,
            load_program_metrics,
        )
    }
}

pub(crate) fn load_program_accounts<CB: TransactionProcessingCallback>(
    callbacks: &CB,
    pubkey: &Pubkey,
) -> Option<ProgramAccountLoadResult> {
    let program_account = callbacks.get_account_shared_data(pubkey)?;

    if loader_v4::check_id(program_account.owner()) {
        return Some(
            solana_loader_v4_program::get_state(program_account.data())
                .ok()
                .and_then(|state| {
                    (!matches!(state.status, LoaderV4Status::Retracted)).then_some(state.slot)
                })
                .map(|slot| ProgramAccountLoadResult::ProgramOfLoaderV4(program_account, slot))
                .unwrap_or(ProgramAccountLoadResult::InvalidAccountData(
                    ProgramCacheEntryOwner::LoaderV4,
                )),
        );
    }

    if bpf_loader_deprecated::check_id(program_account.owner()) {
        return Some(ProgramAccountLoadResult::ProgramOfLoaderV1(program_account));
    }

    if bpf_loader::check_id(program_account.owner()) {
        return Some(ProgramAccountLoadResult::ProgramOfLoaderV2(program_account));
    }

    if let Ok(UpgradeableLoaderState::Program {
        programdata_address,
    }) = program_account.state()
    {
        if let Some(programdata_account) = callbacks.get_account_shared_data(&programdata_address) {
            if let Ok(UpgradeableLoaderState::ProgramData {
                slot,
                upgrade_authority_address: _,
            }) = programdata_account.state()
            {
                return Some(ProgramAccountLoadResult::ProgramOfLoaderV3(
                    program_account,
                    programdata_account,
                    slot,
                ));
            }
        }
    }
    Some(ProgramAccountLoadResult::InvalidAccountData(
        ProgramCacheEntryOwner::LoaderV3,
    ))
}

/// Loads the program with the given pubkey.
///
/// If the account doesn't exist it returns `None`. If the account does exist, it must be a program
/// account (belong to one of the program loaders). Returns `Some(InvalidAccountData)` if the program
/// account is `Closed`, contains invalid data or any of the programdata accounts are invalid.
pub fn load_program_with_pubkey<CB: TransactionProcessingCallback, FG: ForkGraph>(
    callbacks: &CB,
    program_cache: &ProgramCache<FG>,
    pubkey: &Pubkey,
    slot: Slot,
    effective_epoch: Epoch,
    _epoch_schedule: &EpochSchedule,
    reload: bool,
) -> Option<Arc<ProgramCacheEntry>> {
    let environments = program_cache.get_environments_for_epoch(effective_epoch);
    let mut load_program_metrics = LoadProgramMetrics {
        program_id: pubkey.to_string(),
        ..LoadProgramMetrics::default()
    };

    let loaded_program = match load_program_accounts(callbacks, pubkey)? {
        ProgramAccountLoadResult::InvalidAccountData(owner) => Ok(
            ProgramCacheEntry::new_tombstone(slot, owner, ProgramCacheEntryType::Closed),
        ),

        ProgramAccountLoadResult::ProgramOfLoaderV1(program_account) => load_program_from_bytes(
            &mut load_program_metrics,
            program_account.data(),
            program_account.owner(),
            program_account.data().len(),
            0,
            environments.program_runtime_v1.clone(),
            reload,
        )
        .map_err(|_| (0, ProgramCacheEntryOwner::LoaderV1)),

        ProgramAccountLoadResult::ProgramOfLoaderV2(program_account) => load_program_from_bytes(
            &mut load_program_metrics,
            program_account.data(),
            program_account.owner(),
            program_account.data().len(),
            0,
            environments.program_runtime_v1.clone(),
            reload,
        )
        .map_err(|_| (0, ProgramCacheEntryOwner::LoaderV2)),

        ProgramAccountLoadResult::ProgramOfLoaderV3(program_account, programdata_account, slot) => {
            programdata_account
                .data()
                .get(UpgradeableLoaderState::size_of_programdata_metadata()..)
                .ok_or(Box::new(InstructionError::InvalidAccountData).into())
                .and_then(|programdata| {
                    load_program_from_bytes(
                        &mut load_program_metrics,
                        programdata,
                        program_account.owner(),
                        program_account
                            .data()
                            .len()
                            .saturating_add(programdata_account.data().len()),
                        slot,
                        environments.program_runtime_v1.clone(),
                        reload,
                    )
                })
                .map_err(|_| (slot, ProgramCacheEntryOwner::LoaderV3))
        }

        ProgramAccountLoadResult::ProgramOfLoaderV4(program_account, slot) => program_account
            .data()
            .get(LoaderV4State::program_data_offset()..)
            .ok_or(Box::new(InstructionError::InvalidAccountData).into())
            .and_then(|elf_bytes| {
                load_program_from_bytes(
                    &mut load_program_metrics,
                    elf_bytes,
                    &loader_v4::id(),
                    program_account.data().len(),
                    slot,
                    environments.program_runtime_v2.clone(),
                    reload,
                )
            })
            .map_err(|_| (slot, ProgramCacheEntryOwner::LoaderV4)),
    }
    .unwrap_or_else(|(slot, owner)| {
        let env = if let ProgramCacheEntryOwner::LoaderV4 = &owner {
            environments.program_runtime_v2.clone()
        } else {
            environments.program_runtime_v1.clone()
        };
        ProgramCacheEntry::new_tombstone(
            slot,
            owner,
            ProgramCacheEntryType::FailedVerification(env),
        )
    });

    let mut timings = ExecuteDetailsTimings::default();
    load_program_metrics.submit_datapoint(&mut timings);
    loaded_program.update_access_slot(slot);
    Some(Arc::new(loaded_program))
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::transaction_processor::TransactionBatchProcessor,
        solana_program_runtime::{
            loaded_programs::{BlockRelation, ProgramRuntimeEnvironments},
            solana_rbpf::program::BuiltinProgram,
        },
        solana_sdk::{
            account::WritableAccount, bpf_loader, bpf_loader_upgradeable, feature_set::FeatureSet,
            hash::Hash, rent_collector::RentCollector,
        },
        std::{
            cell::RefCell,
            collections::HashMap,
            env,
            fs::{self, File},
            io::Read,
        },
    };

    struct TestForkGraph {}

    impl ForkGraph for TestForkGraph {
        fn relationship(&self, _a: Slot, _b: Slot) -> BlockRelation {
            BlockRelation::Unknown
        }
    }

    #[derive(Default, Clone)]
    pub struct MockBankCallback {
        rent_collector: RentCollector,
        feature_set: Arc<FeatureSet>,
        pub account_shared_data: RefCell<HashMap<Pubkey, AccountSharedData>>,
    }

    impl TransactionProcessingCallback for MockBankCallback {
        fn account_matches_owners(&self, account: &Pubkey, owners: &[Pubkey]) -> Option<usize> {
            if let Some(data) = self.account_shared_data.borrow().get(account) {
                if data.lamports() == 0 {
                    None
                } else {
                    owners.iter().position(|entry| data.owner() == entry)
                }
            } else {
                None
            }
        }

        fn get_account_shared_data(&self, pubkey: &Pubkey) -> Option<AccountSharedData> {
            self.account_shared_data.borrow().get(pubkey).cloned()
        }

        fn get_last_blockhash_and_lamports_per_signature(&self) -> (Hash, u64) {
            (Hash::new_unique(), 2)
        }

        fn get_rent_collector(&self) -> &RentCollector {
            &self.rent_collector
        }

        fn get_feature_set(&self) -> Arc<FeatureSet> {
            self.feature_set.clone()
        }

        fn add_builtin_account(&self, name: &str, program_id: &Pubkey) {
            let mut account_data = AccountSharedData::default();
            account_data.set_data(name.as_bytes().to_vec());
            self.account_shared_data
                .borrow_mut()
                .insert(*program_id, account_data);
        }
    }

    #[test]
    fn test_load_program_accounts_account_not_found() {
        let mock_bank = MockBankCallback::default();
        let key = Pubkey::new_unique();

        let result = load_program_accounts(&mock_bank, &key);
        assert!(result.is_none());

        let mut account_data = AccountSharedData::default();
        account_data.set_owner(bpf_loader_upgradeable::id());
        let state = UpgradeableLoaderState::Program {
            programdata_address: Pubkey::new_unique(),
        };
        account_data.set_data(bincode::serialize(&state).unwrap());
        mock_bank
            .account_shared_data
            .borrow_mut()
            .insert(key, account_data.clone());

        let result = load_program_accounts(&mock_bank, &key);
        assert!(matches!(
            result,
            Some(ProgramAccountLoadResult::InvalidAccountData(_))
        ));

        account_data.set_data(Vec::new());
        mock_bank
            .account_shared_data
            .borrow_mut()
            .insert(key, account_data);

        let result = load_program_accounts(&mock_bank, &key);

        assert!(matches!(
            result,
            Some(ProgramAccountLoadResult::InvalidAccountData(_))
        ));
    }

    #[test]
    fn test_load_program_accounts_loader_v4() {
        let key = Pubkey::new_unique();
        let mock_bank = MockBankCallback::default();
        let mut account_data = AccountSharedData::default();
        account_data.set_owner(loader_v4::id());
        mock_bank
            .account_shared_data
            .borrow_mut()
            .insert(key, account_data.clone());

        let result = load_program_accounts(&mock_bank, &key);
        assert!(matches!(
            result,
            Some(ProgramAccountLoadResult::InvalidAccountData(_))
        ));

        account_data.set_data(vec![0; 64]);
        mock_bank
            .account_shared_data
            .borrow_mut()
            .insert(key, account_data.clone());
        let result = load_program_accounts(&mock_bank, &key);
        assert!(matches!(
            result,
            Some(ProgramAccountLoadResult::InvalidAccountData(_))
        ));

        let loader_data = LoaderV4State {
            slot: 25,
            authority_address: Pubkey::new_unique(),
            status: LoaderV4Status::Deployed,
        };
        let encoded = unsafe {
            std::mem::transmute::<&LoaderV4State, &[u8; LoaderV4State::program_data_offset()]>(
                &loader_data,
            )
        };
        account_data.set_data(encoded.to_vec());
        mock_bank
            .account_shared_data
            .borrow_mut()
            .insert(key, account_data.clone());

        let result = load_program_accounts(&mock_bank, &key);

        match result {
            Some(ProgramAccountLoadResult::ProgramOfLoaderV4(data, slot)) => {
                assert_eq!(data, account_data);
                assert_eq!(slot, 25);
            }

            _ => panic!("Invalid result"),
        }
    }

    #[test]
    fn test_load_program_accounts_loader_v1_or_v2() {
        let key = Pubkey::new_unique();
        let mock_bank = MockBankCallback::default();
        let mut account_data = AccountSharedData::default();
        account_data.set_owner(bpf_loader::id());
        mock_bank
            .account_shared_data
            .borrow_mut()
            .insert(key, account_data.clone());

        let result = load_program_accounts(&mock_bank, &key);
        match result {
            Some(ProgramAccountLoadResult::ProgramOfLoaderV1(data))
            | Some(ProgramAccountLoadResult::ProgramOfLoaderV2(data)) => {
                assert_eq!(data, account_data);
            }
            _ => panic!("Invalid result"),
        }
    }

    #[test]
    fn test_load_program_accounts_success() {
        let key1 = Pubkey::new_unique();
        let key2 = Pubkey::new_unique();
        let mock_bank = MockBankCallback::default();

        let mut account_data = AccountSharedData::default();
        account_data.set_owner(bpf_loader_upgradeable::id());

        let state = UpgradeableLoaderState::Program {
            programdata_address: key2,
        };
        account_data.set_data(bincode::serialize(&state).unwrap());
        mock_bank
            .account_shared_data
            .borrow_mut()
            .insert(key1, account_data.clone());

        let state = UpgradeableLoaderState::ProgramData {
            slot: 25,
            upgrade_authority_address: None,
        };
        let mut account_data2 = AccountSharedData::default();
        account_data2.set_data(bincode::serialize(&state).unwrap());
        mock_bank
            .account_shared_data
            .borrow_mut()
            .insert(key2, account_data2.clone());

        let result = load_program_accounts(&mock_bank, &key1);

        match result {
            Some(ProgramAccountLoadResult::ProgramOfLoaderV3(data1, data2, slot)) => {
                assert_eq!(data1, account_data);
                assert_eq!(data2, account_data2);
                assert_eq!(slot, 25);
            }

            _ => panic!("Invalid result"),
        }
    }

    fn load_test_program() -> Vec<u8> {
        let mut dir = env::current_dir().unwrap();
        dir.push("tests");
        dir.push("example-programs");
        dir.push("hello-solana");
        dir.push("hello_solana_program.so");
        let mut file = File::open(dir.clone()).expect("file not found");
        let metadata = fs::metadata(dir).expect("Unable to read metadata");
        let mut buffer = vec![0; metadata.len() as usize];
        file.read_exact(&mut buffer).expect("Buffer overflow");
        buffer
    }

    #[test]
    fn test_load_program_from_bytes() {
        let buffer = load_test_program();

        let mut metrics = LoadProgramMetrics::default();
        let loader = bpf_loader_upgradeable::id();
        let size = buffer.len();
        let slot = 2;
        let environment = ProgramRuntimeEnvironment::new(BuiltinProgram::new_mock());

        let result = load_program_from_bytes(
            &mut metrics,
            &buffer,
            &loader,
            size,
            slot,
            environment.clone(),
            false,
        );

        assert!(result.is_ok());

        let result = load_program_from_bytes(
            &mut metrics,
            &buffer,
            &loader,
            size,
            slot,
            environment,
            true,
        );

        assert!(result.is_ok());
    }

    #[test]
    fn test_load_program_not_found() {
        let mock_bank = MockBankCallback::default();
        let key = Pubkey::new_unique();
        let batch_processor = TransactionBatchProcessor::<TestForkGraph>::default();
        let program_cache = batch_processor.program_cache.read().unwrap();

        let result = load_program_with_pubkey(
            &mock_bank,
            &program_cache,
            &key,
            500,
            50,
            &batch_processor.epoch_schedule,
            false,
        );
        assert!(result.is_none());
    }

    #[test]
    fn test_load_program_invalid_account_data() {
        let key = Pubkey::new_unique();
        let mock_bank = MockBankCallback::default();
        let mut account_data = AccountSharedData::default();
        account_data.set_owner(loader_v4::id());
        let batch_processor = TransactionBatchProcessor::<TestForkGraph>::default();
        mock_bank
            .account_shared_data
            .borrow_mut()
            .insert(key, account_data.clone());

        let program_cache = batch_processor.program_cache.read().unwrap();

        let result = load_program_with_pubkey(
            &mock_bank,
            &program_cache,
            &key,
            0, // Slot 0
            20,
            &batch_processor.epoch_schedule,
            false,
        );

        let loaded_program = ProgramCacheEntry::new_tombstone(
            0, // Slot 0
            ProgramCacheEntryOwner::LoaderV4,
            ProgramCacheEntryType::FailedVerification(
                batch_processor
                    .program_cache
                    .read()
                    .unwrap()
                    .get_environments_for_epoch(20)
                    .clone()
                    .program_runtime_v1,
            ),
        );
        assert_eq!(result.unwrap(), Arc::new(loaded_program));
    }

    #[test]
    fn test_load_program_program_loader_v1_or_v2() {
        let key = Pubkey::new_unique();
        let mock_bank = MockBankCallback::default();
        let mut account_data = AccountSharedData::default();
        account_data.set_owner(bpf_loader::id());
        let batch_processor = TransactionBatchProcessor::<TestForkGraph>::default();
        mock_bank
            .account_shared_data
            .borrow_mut()
            .insert(key, account_data.clone());

        let program_cache = batch_processor.program_cache.read().unwrap();

        // This should return an error
        let result = load_program_with_pubkey(
            &mock_bank,
            &program_cache,
            &key,
            200,
            20,
            &batch_processor.epoch_schedule,
            false,
        );
        let loaded_program = ProgramCacheEntry::new_tombstone(
            0,
            ProgramCacheEntryOwner::LoaderV2,
            ProgramCacheEntryType::FailedVerification(
                batch_processor
                    .program_cache
                    .read()
                    .unwrap()
                    .get_environments_for_epoch(20)
                    .clone()
                    .program_runtime_v1,
            ),
        );
        assert_eq!(result.unwrap(), Arc::new(loaded_program));

        let buffer = load_test_program();
        account_data.set_data(buffer);

        mock_bank
            .account_shared_data
            .borrow_mut()
            .insert(key, account_data.clone());

        let result = load_program_with_pubkey(
            &mock_bank,
            &program_cache,
            &key,
            200,
            20,
            &batch_processor.epoch_schedule,
            false,
        );

        let environments = ProgramRuntimeEnvironments::default();
        let expected = load_program_from_bytes(
            &mut LoadProgramMetrics::default(),
            account_data.data(),
            account_data.owner(),
            account_data.data().len(),
            0,
            environments.program_runtime_v1.clone(),
            false,
        );

        assert_eq!(result.unwrap(), Arc::new(expected.unwrap()));
    }

    #[test]
    fn test_load_program_program_loader_v3() {
        let key1 = Pubkey::new_unique();
        let key2 = Pubkey::new_unique();
        let mock_bank = MockBankCallback::default();
        let batch_processor = TransactionBatchProcessor::<TestForkGraph>::default();
        let program_cache = batch_processor.program_cache.read().unwrap();

        let mut account_data = AccountSharedData::default();
        account_data.set_owner(bpf_loader_upgradeable::id());

        let state = UpgradeableLoaderState::Program {
            programdata_address: key2,
        };
        account_data.set_data(bincode::serialize(&state).unwrap());
        mock_bank
            .account_shared_data
            .borrow_mut()
            .insert(key1, account_data.clone());

        let state = UpgradeableLoaderState::ProgramData {
            slot: 0,
            upgrade_authority_address: None,
        };
        let mut account_data2 = AccountSharedData::default();
        account_data2.set_data(bincode::serialize(&state).unwrap());
        mock_bank
            .account_shared_data
            .borrow_mut()
            .insert(key2, account_data2.clone());

        // This should return an error
        let result = load_program_with_pubkey(
            &mock_bank,
            &program_cache,
            &key1,
            0,
            0,
            &batch_processor.epoch_schedule,
            false,
        );
        let loaded_program = ProgramCacheEntry::new_tombstone(
            0,
            ProgramCacheEntryOwner::LoaderV3,
            ProgramCacheEntryType::FailedVerification(
                batch_processor
                    .program_cache
                    .read()
                    .unwrap()
                    .get_environments_for_epoch(0)
                    .clone()
                    .program_runtime_v1,
            ),
        );
        assert_eq!(result.unwrap(), Arc::new(loaded_program));

        let mut buffer = load_test_program();
        let mut header = bincode::serialize(&state).unwrap();
        let mut complement = vec![
            0;
            std::cmp::max(
                0,
                UpgradeableLoaderState::size_of_programdata_metadata() - header.len()
            )
        ];
        header.append(&mut complement);
        header.append(&mut buffer);
        account_data.set_data(header);

        mock_bank
            .account_shared_data
            .borrow_mut()
            .insert(key2, account_data.clone());

        let result = load_program_with_pubkey(
            &mock_bank,
            &program_cache,
            &key1,
            200,
            20,
            &batch_processor.epoch_schedule,
            false,
        );

        let data = account_data.data();
        account_data
            .set_data(data[UpgradeableLoaderState::size_of_programdata_metadata()..].to_vec());

        let environments = ProgramRuntimeEnvironments::default();
        let expected = load_program_from_bytes(
            &mut LoadProgramMetrics::default(),
            account_data.data(),
            account_data.owner(),
            account_data.data().len(),
            0,
            environments.program_runtime_v1.clone(),
            false,
        );
        assert_eq!(result.unwrap(), Arc::new(expected.unwrap()));
    }

    #[test]
    fn test_load_program_of_loader_v4() {
        let key = Pubkey::new_unique();
        let mock_bank = MockBankCallback::default();
        let mut account_data = AccountSharedData::default();
        account_data.set_owner(loader_v4::id());
        let batch_processor = TransactionBatchProcessor::<TestForkGraph>::default();
        let program_cache = batch_processor.program_cache.read().unwrap();

        let loader_data = LoaderV4State {
            slot: 0,
            authority_address: Pubkey::new_unique(),
            status: LoaderV4Status::Deployed,
        };
        let encoded = unsafe {
            std::mem::transmute::<&LoaderV4State, &[u8; LoaderV4State::program_data_offset()]>(
                &loader_data,
            )
        };
        account_data.set_data(encoded.to_vec());
        mock_bank
            .account_shared_data
            .borrow_mut()
            .insert(key, account_data.clone());

        let result = load_program_with_pubkey(
            &mock_bank,
            &program_cache,
            &key,
            0,
            0,
            &batch_processor.epoch_schedule,
            false,
        );
        let loaded_program = ProgramCacheEntry::new_tombstone(
            0,
            ProgramCacheEntryOwner::LoaderV4,
            ProgramCacheEntryType::FailedVerification(
                batch_processor
                    .program_cache
                    .read()
                    .unwrap()
                    .get_environments_for_epoch(0)
                    .clone()
                    .program_runtime_v1,
            ),
        );
        assert_eq!(result.unwrap(), Arc::new(loaded_program));

        let mut header = account_data.data().to_vec();
        let mut complement =
            vec![0; std::cmp::max(0, LoaderV4State::program_data_offset() - header.len())];
        header.append(&mut complement);

        let mut buffer = load_test_program();
        header.append(&mut buffer);

        account_data.set_data(header);
        mock_bank
            .account_shared_data
            .borrow_mut()
            .insert(key, account_data.clone());

        let result = load_program_with_pubkey(
            &mock_bank,
            &program_cache,
            &key,
            200,
            20,
            &batch_processor.epoch_schedule,
            false,
        );

        let data = account_data.data()[LoaderV4State::program_data_offset()..].to_vec();
        account_data.set_data(data);
        mock_bank
            .account_shared_data
            .borrow_mut()
            .insert(key, account_data.clone());

        let environments = ProgramRuntimeEnvironments::default();
        let expected = load_program_from_bytes(
            &mut LoadProgramMetrics::default(),
            account_data.data(),
            account_data.owner(),
            account_data.data().len(),
            0,
            environments.program_runtime_v1.clone(),
            false,
        );
        assert_eq!(result.unwrap(), Arc::new(expected.unwrap()));
    }

    #[test]
    fn test_load_program_environment() {
        let key = Pubkey::new_unique();
        let mock_bank = MockBankCallback::default();
        let mut account_data = AccountSharedData::default();
        account_data.set_owner(bpf_loader::id());
        let batch_processor = TransactionBatchProcessor::<TestForkGraph>::default();

        let upcoming_environments = ProgramRuntimeEnvironments::default();
        let current_environments = {
            let mut program_cache = batch_processor.program_cache.write().unwrap();
            program_cache.upcoming_environments = Some(upcoming_environments.clone());
            program_cache.environments.clone()
        };
        mock_bank
            .account_shared_data
            .borrow_mut()
            .insert(key, account_data.clone());

        let program_cache = batch_processor.program_cache.read().unwrap();

        for is_upcoming_env in [false, true] {
            let result = load_program_with_pubkey(
                &mock_bank,
                &program_cache,
                &key,
                200,
                is_upcoming_env as u64,
                &batch_processor.epoch_schedule,
                false,
            )
            .unwrap();
            assert_ne!(
                is_upcoming_env,
                Arc::ptr_eq(
                    result.program.get_environment().unwrap(),
                    &current_environments.program_runtime_v1,
                )
            );
            assert_eq!(
                is_upcoming_env,
                Arc::ptr_eq(
                    result.program.get_environment().unwrap(),
                    &upcoming_environments.program_runtime_v1,
                )
            );
        }
    }
}
