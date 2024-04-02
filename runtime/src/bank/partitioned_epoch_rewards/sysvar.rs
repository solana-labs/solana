use {
    super::Bank,
    log::info,
    solana_sdk::{
        account::{
            create_account_shared_data_with_fields as create_account, from_account, ReadableAccount,
        },
        sysvar,
    },
};

impl Bank {
    /// Helper fn to log epoch_rewards sysvar
    fn log_epoch_rewards_sysvar(&self, prefix: &str) {
        if let Some(account) = self.get_account(&sysvar::epoch_rewards::id()) {
            let epoch_rewards: sysvar::epoch_rewards::EpochRewards =
                from_account(&account).unwrap();
            info!(
                "{prefix} epoch_rewards sysvar: {:?}",
                (account.lamports(), epoch_rewards)
            );
        } else {
            info!("{prefix} epoch_rewards sysvar: none");
        }
    }

    /// Create EpochRewards sysvar with calculated rewards
    pub(in crate::bank) fn create_epoch_rewards_sysvar(
        &self,
        total_rewards: u64,
        distributed_rewards: u64,
        distribution_starting_block_height: u64,
    ) {
        assert!(self.is_partitioned_rewards_code_enabled());

        let epoch_rewards = sysvar::epoch_rewards::EpochRewards {
            total_rewards,
            distributed_rewards,
            distribution_starting_block_height,
            active: true,
            ..sysvar::epoch_rewards::EpochRewards::default()
        };

        self.update_sysvar_account(&sysvar::epoch_rewards::id(), |account| {
            let mut inherited_account_fields =
                self.inherit_specially_retained_account_fields(account);

            assert!(total_rewards >= distributed_rewards);
            // set the account lamports to the undistributed rewards
            inherited_account_fields.0 = total_rewards - distributed_rewards;
            create_account(&epoch_rewards, inherited_account_fields)
        });

        self.log_epoch_rewards_sysvar("create");
    }

    /// Update EpochRewards sysvar with distributed rewards
    pub(in crate::bank) fn update_epoch_rewards_sysvar(&self, distributed: u64) {
        assert!(self.is_partitioned_rewards_code_enabled());

        let mut epoch_rewards: sysvar::epoch_rewards::EpochRewards =
            from_account(&self.get_account(&sysvar::epoch_rewards::id()).unwrap()).unwrap();
        epoch_rewards.distribute(distributed);

        self.update_sysvar_account(&sysvar::epoch_rewards::id(), |account| {
            let mut inherited_account_fields =
                self.inherit_specially_retained_account_fields(account);

            let lamports = inherited_account_fields.0;
            assert!(lamports >= distributed);
            inherited_account_fields.0 = lamports - distributed;
            create_account(&epoch_rewards, inherited_account_fields)
        });

        self.log_epoch_rewards_sysvar("update");
    }

    pub(in crate::bank) fn destroy_epoch_rewards_sysvar(&self) {
        if let Some(account) = self.get_account(&sysvar::epoch_rewards::id()) {
            if account.lamports() > 0 {
                info!(
                    "burning {} extra lamports in EpochRewards sysvar account at slot {}",
                    account.lamports(),
                    self.slot()
                );
                self.log_epoch_rewards_sysvar("burn");
                self.burn_and_purge_account(&sysvar::epoch_rewards::id(), account);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::bank::tests::create_genesis_config,
        solana_sdk::{
            epoch_schedule::EpochSchedule, feature_set, hash::Hash, native_token::LAMPORTS_PER_SOL,
        },
    };

    /// Test `EpochRewards` sysvar creation, distribution, and burning.
    /// This test covers the following epoch_rewards_sysvar bank member functions, i.e.
    /// `create_epoch_rewards_sysvar`, `update_epoch_rewards_sysvar`, `burn_and_purge_account`.
    #[test]
    fn test_epoch_rewards_sysvar() {
        let (mut genesis_config, _mint_keypair) =
            create_genesis_config(1_000_000 * LAMPORTS_PER_SOL);
        genesis_config.epoch_schedule = EpochSchedule::custom(432000, 432000, false);
        let mut bank = Bank::new_for_tests(&genesis_config);
        bank.activate_feature(&feature_set::enable_partitioned_epoch_reward::id());

        let total_rewards = 1_000_000_000; // a large rewards so that the sysvar account is rent-exempted.

        // create epoch rewards sysvar
        let expected_epoch_rewards = sysvar::epoch_rewards::EpochRewards {
            distribution_starting_block_height: 42,
            num_partitions: 0,
            parent_blockhash: Hash::default(),
            total_points: 0,
            total_rewards,
            distributed_rewards: 10,
            active: true,
        };

        bank.create_epoch_rewards_sysvar(total_rewards, 10, 42);
        let account = bank.get_account(&sysvar::epoch_rewards::id()).unwrap();
        assert_eq!(account.lamports(), total_rewards - 10);
        let epoch_rewards: sysvar::epoch_rewards::EpochRewards = from_account(&account).unwrap();
        assert_eq!(epoch_rewards, expected_epoch_rewards);

        // make a distribution from epoch rewards sysvar
        bank.update_epoch_rewards_sysvar(10);
        let account = bank.get_account(&sysvar::epoch_rewards::id()).unwrap();
        assert_eq!(account.lamports(), total_rewards - 20);
        let epoch_rewards: sysvar::epoch_rewards::EpochRewards = from_account(&account).unwrap();
        let expected_epoch_rewards = sysvar::epoch_rewards::EpochRewards {
            distribution_starting_block_height: 42,
            num_partitions: 0,
            parent_blockhash: Hash::default(),
            total_points: 0,
            total_rewards,
            distributed_rewards: 20,
            active: true,
        };
        assert_eq!(epoch_rewards, expected_epoch_rewards);

        // burn epoch rewards sysvar
        bank.burn_and_purge_account(&sysvar::epoch_rewards::id(), account);
        let account = bank.get_account(&sysvar::epoch_rewards::id());
        assert!(account.is_none());
    }
}
