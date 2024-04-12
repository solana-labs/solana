use {
    super::Bank,
    log::info,
    solana_sdk::{
        account::{create_account_shared_data_with_fields as create_account, from_account},
        sysvar,
    },
};

impl Bank {
    /// Helper fn to log epoch_rewards sysvar
    fn log_epoch_rewards_sysvar(&self, prefix: &str) {
        if let Some(account) = self.get_account(&sysvar::epoch_rewards::id()) {
            let epoch_rewards: sysvar::epoch_rewards::EpochRewards =
                from_account(&account).unwrap();
            info!("{prefix} epoch_rewards sysvar: {:?}", epoch_rewards);
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

        assert!(total_rewards >= distributed_rewards);

        let epoch_rewards = sysvar::epoch_rewards::EpochRewards {
            total_rewards,
            distributed_rewards,
            distribution_starting_block_height,
            active: true,
            ..sysvar::epoch_rewards::EpochRewards::default()
        };

        self.update_sysvar_account(&sysvar::epoch_rewards::id(), |account| {
            create_account(
                &epoch_rewards,
                self.inherit_specially_retained_account_fields(account),
            )
        });

        self.log_epoch_rewards_sysvar("create");
    }

    /// Update EpochRewards sysvar with distributed rewards
    pub(in crate::bank::partitioned_epoch_rewards) fn update_epoch_rewards_sysvar(
        &self,
        distributed: u64,
    ) {
        assert!(self.is_partitioned_rewards_code_enabled());

        let mut epoch_rewards = self.get_epoch_rewards_sysvar();
        assert!(epoch_rewards.active);

        epoch_rewards.distribute(distributed);

        self.update_sysvar_account(&sysvar::epoch_rewards::id(), |account| {
            create_account(
                &epoch_rewards,
                self.inherit_specially_retained_account_fields(account),
            )
        });

        self.log_epoch_rewards_sysvar("update");
    }

    /// Update EpochRewards sysvar with distributed rewards
    pub(in crate::bank::partitioned_epoch_rewards) fn set_epoch_rewards_sysvar_to_inactive(&self) {
        let mut epoch_rewards = self.get_epoch_rewards_sysvar();
        assert_eq!(
            epoch_rewards.distributed_rewards,
            epoch_rewards.total_rewards
        );
        epoch_rewards.active = false;

        self.update_sysvar_account(&sysvar::epoch_rewards::id(), |account| {
            create_account(
                &epoch_rewards,
                self.inherit_specially_retained_account_fields(account),
            )
        });

        self.log_epoch_rewards_sysvar("set_inactive");
    }

    /// Get EpochRewards sysvar. Returns EpochRewards::default() if sysvar
    /// account cannot be found or cannot be deserialized.
    pub(in crate::bank::partitioned_epoch_rewards) fn get_epoch_rewards_sysvar(
        &self,
    ) -> sysvar::epoch_rewards::EpochRewards {
        from_account(
            &self
                .get_account(&sysvar::epoch_rewards::id())
                .unwrap_or_default(),
        )
        .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::bank::tests::create_genesis_config,
        solana_sdk::{
            account::ReadableAccount, epoch_schedule::EpochSchedule, feature_set, hash::Hash,
            native_token::LAMPORTS_PER_SOL,
        },
    };

    /// Test `EpochRewards` sysvar creation, distribution, and burning.
    /// This test covers the following epoch_rewards_sysvar bank member functions, i.e.
    /// `create_epoch_rewards_sysvar`, `update_epoch_rewards_sysvar`, `test_epoch_rewards_sysvar`.
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

        let epoch_rewards = bank.get_epoch_rewards_sysvar();
        assert_eq!(
            epoch_rewards,
            sysvar::epoch_rewards::EpochRewards::default()
        );

        bank.create_epoch_rewards_sysvar(total_rewards, 10, 42);
        let account = bank.get_account(&sysvar::epoch_rewards::id()).unwrap();
        let expected_balance = bank.get_minimum_balance_for_rent_exemption(account.data().len());
        // Expected balance is the sysvar rent-exempt balance
        assert_eq!(account.lamports(), expected_balance);
        let epoch_rewards: sysvar::epoch_rewards::EpochRewards = from_account(&account).unwrap();
        assert_eq!(epoch_rewards, expected_epoch_rewards);

        let epoch_rewards = bank.get_epoch_rewards_sysvar();
        assert_eq!(epoch_rewards, expected_epoch_rewards);

        // make a distribution from epoch rewards sysvar
        bank.update_epoch_rewards_sysvar(10);
        let account = bank.get_account(&sysvar::epoch_rewards::id()).unwrap();
        // Balance should not change
        assert_eq!(account.lamports(), expected_balance);
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
    }
}
