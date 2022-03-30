use {super::Bank, solana_program_runtime::sysvar_cache::SysvarCache};

impl Bank {
    pub(crate) fn fill_missing_sysvar_cache_entries(&self) {
        let mut sysvar_cache = self.sysvar_cache.write().unwrap();
        sysvar_cache.fill_missing_entries(|pubkey| self.get_account_with_fixed_root(pubkey));
    }

    pub(crate) fn reset_sysvar_cache(&self) {
        let mut sysvar_cache = self.sysvar_cache.write().unwrap();
        sysvar_cache.reset();
    }

    pub fn get_sysvar_cache_for_tests(&self) -> SysvarCache {
        self.sysvar_cache.read().unwrap().clone()
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{bank::ErrorCounters, genesis_utils::activate_all_features},
        solana_sdk::{
            account::ReadableAccount,
            genesis_config::create_genesis_config,
            instruction::{AccountMeta, Instruction},
            pubkey::Pubkey,
            signature::Signer,
            signer::keypair::Keypair,
            system_program, sysvar,
            transaction::{SanitizedTransaction, Transaction},
        },
        std::sync::Arc,
    };

    #[test]
    #[allow(deprecated)]
    fn test_sysvar_cache_initialization() {
        let (genesis_config, _mint_keypair) = create_genesis_config(100_000);
        let bank0 = Arc::new(Bank::new_for_tests(&genesis_config));

        let bank0_sysvar_cache = bank0.sysvar_cache.read().unwrap();
        let bank0_cached_clock = bank0_sysvar_cache.get_clock();
        let bank0_cached_epoch_schedule = bank0_sysvar_cache.get_epoch_schedule();
        let bank0_cached_fees = bank0_sysvar_cache.get_fees();
        let bank0_cached_rent = bank0_sysvar_cache.get_rent();

        assert!(bank0_cached_clock.is_ok());
        assert!(bank0_cached_epoch_schedule.is_ok());
        assert!(bank0_cached_fees.is_ok());
        assert!(bank0_cached_rent.is_ok());
        assert!(bank0_sysvar_cache.get_slot_hashes().is_err());

        let bank1 = Arc::new(Bank::new_from_parent(
            &bank0,
            &Pubkey::default(),
            bank0.slot() + 1,
        ));

        let bank1_sysvar_cache = bank1.sysvar_cache.read().unwrap();
        let bank1_cached_clock = bank1_sysvar_cache.get_clock();
        let bank1_cached_epoch_schedule = bank1_sysvar_cache.get_epoch_schedule();
        let bank1_cached_fees = bank1_sysvar_cache.get_fees();
        let bank1_cached_rent = bank1_sysvar_cache.get_rent();

        assert!(bank1_cached_clock.is_ok());
        assert!(bank1_cached_epoch_schedule.is_ok());
        assert!(bank1_cached_fees.is_ok());
        assert!(bank1_cached_rent.is_ok());
        assert!(bank1_sysvar_cache.get_slot_hashes().is_ok());

        assert_ne!(bank0_cached_clock, bank1_cached_clock);
        assert_eq!(bank0_cached_epoch_schedule, bank1_cached_epoch_schedule);
        assert_ne!(bank0_cached_fees, bank1_cached_fees);
        assert_eq!(bank0_cached_rent, bank1_cached_rent);

        let bank2 = Bank::new_from_parent(&bank1, &Pubkey::default(), bank1.slot() + 1);

        let bank2_sysvar_cache = bank2.sysvar_cache.read().unwrap();
        let bank2_cached_clock = bank2_sysvar_cache.get_clock();
        let bank2_cached_epoch_schedule = bank2_sysvar_cache.get_epoch_schedule();
        let bank2_cached_fees = bank2_sysvar_cache.get_fees();
        let bank2_cached_rent = bank2_sysvar_cache.get_rent();

        assert!(bank2_cached_clock.is_ok());
        assert!(bank2_cached_epoch_schedule.is_ok());
        assert!(bank2_cached_fees.is_ok());
        assert!(bank2_cached_rent.is_ok());
        assert!(bank2_sysvar_cache.get_slot_hashes().is_ok());

        assert_ne!(bank1_cached_clock, bank2_cached_clock);
        assert_eq!(bank1_cached_epoch_schedule, bank2_cached_epoch_schedule);
        assert_eq!(bank1_cached_fees, bank2_cached_fees);
        assert_eq!(bank1_cached_rent, bank2_cached_rent);
        assert_ne!(
            bank1_sysvar_cache.get_slot_hashes(),
            bank2_sysvar_cache.get_slot_hashes(),
        );
    }

    #[test]
    #[allow(deprecated)]
    fn test_reset_and_fill_sysvar_cache() {
        let (genesis_config, _mint_keypair) = create_genesis_config(100_000);
        let bank0 = Arc::new(Bank::new_for_tests(&genesis_config));
        let bank1 = Bank::new_from_parent(&bank0, &Pubkey::default(), bank0.slot() + 1);

        let bank1_sysvar_cache = bank1.sysvar_cache.read().unwrap();
        let bank1_cached_clock = bank1_sysvar_cache.get_clock();
        let bank1_cached_epoch_schedule = bank1_sysvar_cache.get_epoch_schedule();
        let bank1_cached_fees = bank1_sysvar_cache.get_fees();
        let bank1_cached_rent = bank1_sysvar_cache.get_rent();
        let bank1_cached_slot_hashes = bank1_sysvar_cache.get_slot_hashes();

        assert!(bank1_cached_clock.is_ok());
        assert!(bank1_cached_epoch_schedule.is_ok());
        assert!(bank1_cached_fees.is_ok());
        assert!(bank1_cached_rent.is_ok());
        assert!(bank1_cached_slot_hashes.is_ok());

        drop(bank1_sysvar_cache);
        bank1.reset_sysvar_cache();

        let bank1_sysvar_cache = bank1.sysvar_cache.read().unwrap();
        assert!(bank1_sysvar_cache.get_clock().is_err());
        assert!(bank1_sysvar_cache.get_epoch_schedule().is_err());
        assert!(bank1_sysvar_cache.get_fees().is_err());
        assert!(bank1_sysvar_cache.get_rent().is_err());
        assert!(bank1_sysvar_cache.get_slot_hashes().is_err());

        drop(bank1_sysvar_cache);
        bank1.fill_missing_sysvar_cache_entries();

        let bank1_sysvar_cache = bank1.sysvar_cache.read().unwrap();
        assert_eq!(bank1_sysvar_cache.get_clock(), bank1_cached_clock);
        assert_eq!(
            bank1_sysvar_cache.get_epoch_schedule(),
            bank1_cached_epoch_schedule
        );
        assert_eq!(bank1_sysvar_cache.get_fees(), bank1_cached_fees);
        assert_eq!(bank1_sysvar_cache.get_rent(), bank1_cached_rent);
        assert_eq!(
            bank1_sysvar_cache.get_slot_hashes(),
            bank1_cached_slot_hashes
        );
    }

    fn check_transaction_loaded_sysvars_match(bank: &Bank, payer: &Keypair) {
        let program_id = system_program::id();
        for sysvar_id in sysvar::ALL_IDS.iter() {
            let instruction = Instruction {
                program_id,
                accounts: vec![AccountMeta::new(*sysvar_id, false)],
                data: vec![],
            };
            let tx = Transaction::new_signed_with_payer(
                &[instruction],
                Some(&payer.pubkey()),
                &[payer],
                bank.blockhash_queue.read().unwrap().last_hash(),
            );
            let tx = SanitizedTransaction::from_transaction_for_tests(tx);
            let mut error_counters = ErrorCounters::default();
            let loaded_transactions = bank.rc.accounts.load_accounts(
                &bank.ancestors,
                &[tx],
                vec![(Ok(()), None)],
                &bank.blockhash_queue.read().unwrap(),
                &mut error_counters,
                &bank.rent_collector,
                &bank.feature_set,
                &bank.fee_structure,
                &bank.sysvar_cache.read().unwrap(),
            );
            assert_eq!(loaded_transactions.len(), 1);
            let transaction = &loaded_transactions[0].0.as_ref().unwrap();
            let (loaded_sysvar_id, loaded_sysvar_account) = &transaction.accounts[1];
            assert_eq!(loaded_sysvar_id, sysvar_id);
            let bank_sysvar_account = bank.get_account_with_fixed_root(sysvar_id);

            // rewards are deprecated and not available in the cache
            if *sysvar_id == sysvar::rewards::id() {
                assert_eq!(*loaded_sysvar_account.owner(), system_program::id());
                assert!(bank_sysvar_account.is_none());
            } else {
                assert_eq!(*loaded_sysvar_account.owner(), sysvar::id());
                // instructions can't be loaded as a normal account
                if *sysvar_id == sysvar::instructions::id() {
                    assert!(bank_sysvar_account.is_none());
                } else {
                    assert_eq!(&bank_sysvar_account.unwrap(), loaded_sysvar_account);
                }
            }
        }
    }

    #[test]
    fn test_load_sysvar_from_filled_cache() {
        let (mut genesis_config, mint_keypair) = create_genesis_config(100_000);
        activate_all_features(&mut genesis_config);
        let bank = Arc::new(Bank::new_for_tests(&genesis_config));
        let bank = Arc::new(Bank::new_from_parent(
            &bank,
            &Pubkey::default(),
            bank.slot() + 1,
        ));
        check_transaction_loaded_sysvars_match(&bank, &mint_keypair);
    }

    #[test]
    fn sanity_test_load_sysvar_from_empty_cache() {
        let (mut genesis_config, mint_keypair) = create_genesis_config(100_000);
        activate_all_features(&mut genesis_config);
        let bank = Arc::new(Bank::new_for_tests(&genesis_config));
        let bank = Arc::new(Bank::new_from_parent(
            &bank,
            &Pubkey::default(),
            bank.slot() + 1,
        ));
        bank.reset_sysvar_cache();
        check_transaction_loaded_sysvars_match(&bank, &mint_keypair);
    }
}
