// Long-running storage_stage tests

#[cfg(test)]
mod tests {
    use log::*;
    use solana_core::genesis_utils::{create_genesis_config, GenesisConfigInfo};
    use solana_core::storage_stage::{test_cluster_info, SLOTS_PER_TURN_TEST};
    use solana_core::storage_stage::{StorageStage, StorageState};
    use solana_ledger::bank_forks::BankForks;
    use solana_ledger::blocktree_processor;
    use solana_ledger::entry;
    use solana_ledger::{blocktree::Blocktree, create_new_tmp_ledger};
    use solana_runtime::bank::Bank;
    use solana_sdk::clock::DEFAULT_TICKS_PER_SLOT;
    use solana_sdk::hash::Hash;
    use solana_sdk::message::Message;
    use solana_sdk::pubkey::Pubkey;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::transaction::Transaction;
    use solana_storage_api::storage_instruction;
    use solana_storage_api::storage_instruction::StorageAccountType;
    use std::fs::remove_dir_all;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::channel;
    use std::sync::{Arc, RwLock};
    use std::thread::sleep;
    use std::time::Duration;

    #[test]
    fn test_storage_stage_process_account_proofs() {
        solana_logger::setup();
        let keypair = Arc::new(Keypair::new());
        let storage_keypair = Arc::new(Keypair::new());
        let archiver_keypair = Arc::new(Keypair::new());
        let exit = Arc::new(AtomicBool::new(false));

        let GenesisConfigInfo {
            mut genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(1000);
        genesis_config
            .native_instruction_processors
            .push(solana_storage_program::solana_storage_program!());
        let (ledger_path, _blockhash) = create_new_tmp_ledger!(&genesis_config);

        let blocktree = Arc::new(Blocktree::open(&ledger_path).unwrap());

        let bank = Bank::new(&genesis_config);
        let bank = Arc::new(bank);
        let bank_forks = Arc::new(RwLock::new(BankForks::new_from_banks(
            &[bank.clone()],
            vec![0],
        )));
        let cluster_info = test_cluster_info(&keypair.pubkey());

        let (bank_sender, bank_receiver) = channel();
        let storage_state = StorageState::new(
            &bank.last_blockhash(),
            SLOTS_PER_TURN_TEST,
            bank.slots_per_segment(),
        );
        let storage_stage = StorageStage::new(
            &storage_state,
            bank_receiver,
            Some(blocktree.clone()),
            &keypair,
            &storage_keypair,
            &exit.clone(),
            &bank_forks,
            &cluster_info,
        );
        bank_sender.send(vec![bank.clone()]).unwrap();

        // create accounts
        let bank = Arc::new(Bank::new_from_parent(&bank, &keypair.pubkey(), 1));
        let account_ix = storage_instruction::create_storage_account(
            &mint_keypair.pubkey(),
            &Pubkey::new_rand(),
            &archiver_keypair.pubkey(),
            1,
            StorageAccountType::Archiver,
        );
        let account_tx = Transaction::new_signed_instructions(
            &[&mint_keypair, &archiver_keypair],
            account_ix,
            bank.last_blockhash(),
        );
        bank.process_transaction(&account_tx).expect("create");

        bank_sender.send(vec![bank.clone()]).unwrap();

        let mut reference_keys;
        {
            let keys = &storage_state.state.read().unwrap().storage_keys;
            reference_keys = vec![0; keys.len()];
            reference_keys.copy_from_slice(keys);
        }

        let keypair = Keypair::new();

        let mining_proof_ix = storage_instruction::mining_proof(
            &archiver_keypair.pubkey(),
            Hash::default(),
            0,
            keypair.sign_message(b"test"),
            bank.last_blockhash(),
        );

        let next_bank = Arc::new(Bank::new_from_parent(&bank, &keypair.pubkey(), 2));
        //register ticks so the program reports a different segment
        blocktree_processor::process_entries(
            &next_bank,
            &entry::create_ticks(
                DEFAULT_TICKS_PER_SLOT * next_bank.slots_per_segment() + 1,
                0,
                bank.last_blockhash(),
            ),
            true,
        )
        .unwrap();
        let message = Message::new_with_payer(vec![mining_proof_ix], Some(&mint_keypair.pubkey()));
        let mining_proof_tx = Transaction::new(
            &[&mint_keypair, archiver_keypair.as_ref()],
            message,
            next_bank.last_blockhash(),
        );
        next_bank
            .process_transaction(&mining_proof_tx)
            .expect("process txs");
        bank_sender.send(vec![next_bank]).unwrap();

        for _ in 0..5 {
            {
                let keys = &storage_state.state.read().unwrap().storage_keys;
                if keys[..] != *reference_keys.as_slice() {
                    break;
                }
            }

            sleep(Duration::new(1, 0));
        }

        debug!("joining..?");
        exit.store(true, Ordering::Relaxed);
        storage_stage.join().unwrap();

        {
            let keys = &storage_state.state.read().unwrap().storage_keys;
            assert_ne!(keys[..], *reference_keys);
        }

        remove_dir_all(ledger_path).unwrap();
    }

    #[test]
    fn test_storage_stage_process_banks() {
        solana_logger::setup();
        let keypair = Arc::new(Keypair::new());
        let storage_keypair = Arc::new(Keypair::new());
        let exit = Arc::new(AtomicBool::new(false));

        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(1000);
        let (ledger_path, _blockhash) = create_new_tmp_ledger!(&genesis_config);

        let blocktree = Arc::new(Blocktree::open(&ledger_path).unwrap());
        let slot = 1;
        let bank = Arc::new(Bank::new(&genesis_config));
        let bank_forks = Arc::new(RwLock::new(BankForks::new_from_banks(
            &[bank.clone()],
            vec![0],
        )));

        let cluster_info = test_cluster_info(&keypair.pubkey());
        let (bank_sender, bank_receiver) = channel();
        let storage_state = StorageState::new(
            &bank.last_blockhash(),
            SLOTS_PER_TURN_TEST,
            bank.slots_per_segment(),
        );
        let storage_stage = StorageStage::new(
            &storage_state,
            bank_receiver,
            Some(blocktree.clone()),
            &keypair,
            &storage_keypair,
            &exit.clone(),
            &bank_forks,
            &cluster_info,
        );
        bank_sender.send(vec![bank.clone()]).unwrap();

        let keypair = Keypair::new();
        let hash = Hash::default();
        let signature = keypair.sign_message(&hash.as_ref());

        let mut result = storage_state.get_mining_result(&signature);

        assert_eq!(result, Hash::default());

        let mut last_bank = bank;
        let rooted_banks = (slot..slot + last_bank.slots_per_segment() + 1)
            .map(|i| {
                let bank = Arc::new(Bank::new_from_parent(&last_bank, &keypair.pubkey(), i));
                blocktree_processor::process_entries(
                    &bank,
                    &entry::create_ticks(64, 0, bank.last_blockhash()),
                    true,
                )
                .expect("failed process entries");
                last_bank = bank;
                last_bank.clone()
            })
            .collect::<Vec<_>>();
        bank_sender.send(rooted_banks).unwrap();

        if solana_perf::perf_libs::api().is_some() {
            for _ in 0..5 {
                result = storage_state.get_mining_result(&signature);
                if result != Hash::default() {
                    info!("found result = {:?} sleeping..", result);
                    break;
                }
                info!("result = {:?} sleeping..", result);
                sleep(Duration::new(1, 0));
            }
        }

        info!("joining..?");
        exit.store(true, Ordering::Relaxed);
        storage_stage.join().unwrap();

        if solana_perf::perf_libs::api().is_some() {
            assert_ne!(result, Hash::default());
        } else {
            assert_eq!(result, Hash::default());
        }

        remove_dir_all(ledger_path).unwrap();
    }
}
