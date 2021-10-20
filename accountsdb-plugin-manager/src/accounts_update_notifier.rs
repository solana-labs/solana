use solana_sdk::transaction::TransactionError;

/// Module responsible for notifying plugins of account updates
use {
    crate::accountsdb_plugin_manager::AccountsDbPluginManager,
    log::*,
    solana_accountsdb_plugin_interface::accountsdb_plugin_interface::{
        ReplicaAccountInfo, ReplicaAccountInfoVersions, ReplicaTransactionLogInfo,
        ReplicaTranscaionLogInfoVersions, SlotStatus,
    },
    solana_measure::measure::Measure,
    solana_metrics::*,
    solana_runtime::{
        accounts_update_notifier_interface::AccountsUpdateNotifierInterface,
        append_vec::{StoredAccountMeta, StoredMeta},
        bank::TransactionLogInfo,
    },
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount},
        clock::Slot,
    },
    std::sync::{Arc, RwLock},
};
#[derive(Debug)]
pub(crate) struct AccountsUpdateNotifierImpl {
    plugin_manager: Arc<RwLock<AccountsDbPluginManager>>,
}

impl AccountsUpdateNotifierInterface for AccountsUpdateNotifierImpl {
    fn notify_account_update(&self, slot: Slot, meta: &StoredMeta, account: &AccountSharedData) {
        if let Some(account_info) = self.accountinfo_from_shared_account_data(meta, account) {
            self.notify_plugins_of_account_update(account_info, slot, false);
        }
    }

    fn notify_account_restore_from_snapshot(&self, slot: Slot, account: &StoredAccountMeta) {
        let mut measure_all = Measure::start("accountsdb-plugin-notify-account-restore-all");
        let mut measure_copy = Measure::start("accountsdb-plugin-copy-stored-account-info");

        let account = self.accountinfo_from_stored_account_meta(account);
        measure_copy.stop();

        inc_new_counter_debug!(
            "accountsdb-plugin-copy-stored-account-info-us",
            measure_copy.as_us() as usize,
            100000,
            100000
        );

        if let Some(account_info) = account {
            self.notify_plugins_of_account_update(account_info, slot, true);
        }
        measure_all.stop();

        inc_new_counter_debug!(
            "accountsdb-plugin-notify-account-restore-all-us",
            measure_all.as_us() as usize,
            100000,
            100000
        );
    }

    fn notify_end_of_restore_from_snapshot(&self) {
        let mut plugin_manager = self.plugin_manager.write().unwrap();
        if plugin_manager.plugins.is_empty() {
            return;
        }

        for plugin in plugin_manager.plugins.iter_mut() {
            let mut measure = Measure::start("accountsdb-plugin-end-of-restore-from-snapshot");
            match plugin.notify_end_of_startup() {
                Err(err) => {
                    error!(
                        "Failed to notify the end of restore from snapshot, error: {} to plugin {}",
                        err,
                        plugin.name()
                    )
                }
                Ok(_) => {
                    trace!(
                        "Successfully notified the end of restore from snapshot to plugin {}",
                        plugin.name()
                    );
                }
            }
            measure.stop();
            inc_new_counter_debug!(
                "accountsdb-plugin-end-of-restore-from-snapshot",
                measure.as_us() as usize
            );
        }
    }

    fn notify_slot_confirmed(&self, slot: Slot, parent: Option<Slot>) {
        self.notify_slot_status(slot, parent, SlotStatus::Confirmed);
    }

    fn notify_slot_processed(&self, slot: Slot, parent: Option<Slot>) {
        self.notify_slot_status(slot, parent, SlotStatus::Processed);
    }

    fn notify_slot_rooted(&self, slot: Slot, parent: Option<Slot>) {
        self.notify_slot_status(slot, parent, SlotStatus::Rooted);
    }

    fn notify_transaction_log_info(&self, transaction_log_info: &TransactionLogInfo, slot: Slot) {
        self.notify_plugins_of_transaction_log_info(transaction_log_info, slot);
    }
}

fn get_transaction_status(result: &Result<(), TransactionError>) -> Option<String> {
    if result.is_ok() {
        return None;
    }

    let err = match result.as_ref().err().unwrap() {
        TransactionError::AccountInUse => "AccountInUse",
        TransactionError::AccountLoadedTwice => "AccountLoadedTwice",
        TransactionError::AccountNotFound => "AccountNotFound",
        TransactionError::ProgramAccountNotFound => "ProgramAccountNotFound",
        TransactionError::InsufficientFundsForFee => "InsufficientFundsForFee",
        TransactionError::InvalidAccountForFee => "InvalidAccountForFee",
        TransactionError::AlreadyProcessed => "AlreadyProcessed",
        TransactionError::BlockhashNotFound => "BlockhashNotFound",
        TransactionError::InstructionError(idx, error) => {
            return Some(format!("InstructionError: idx ({}), error: {}", idx, error));
        }
        TransactionError::CallChainTooDeep => "CallChainTooDeep",
        TransactionError::MissingSignatureForFee => "MissingSignatureForFee",
        TransactionError::InvalidAccountIndex => "InvalidAccountIndex",
        TransactionError::SignatureFailure => "SignatureFailure",
        TransactionError::InvalidProgramForExecution => "InvalidProgramForExecution",
        TransactionError::SanitizeFailure => "SanitizeFailure",
        TransactionError::ClusterMaintenance => "ClusterMaintenance",
        TransactionError::AccountBorrowOutstanding => "AccountBorrowOutstanding",
        TransactionError::WouldExceedMaxBlockCostLimit => "WouldExceedMaxBlockCostLimit",
        TransactionError::UnsupportedVersion => "UnsupportedVersion",
        TransactionError::InvalidWritableAccount => "InvalidWritableAccount",
    };

    Some(err.to_string())
}

impl AccountsUpdateNotifierImpl {
    pub fn new(plugin_manager: Arc<RwLock<AccountsDbPluginManager>>) -> Self {
        AccountsUpdateNotifierImpl { plugin_manager }
    }

    fn accountinfo_from_shared_account_data<'a>(
        &self,
        meta: &'a StoredMeta,
        account: &'a AccountSharedData,
    ) -> Option<ReplicaAccountInfo<'a>> {
        Some(ReplicaAccountInfo {
            pubkey: meta.pubkey.as_ref(),
            lamports: account.lamports(),
            owner: account.owner().as_ref(),
            executable: account.executable(),
            rent_epoch: account.rent_epoch(),
            data: account.data(),
            write_version: meta.write_version,
        })
    }

    fn accountinfo_from_stored_account_meta<'a>(
        &self,
        stored_account_meta: &'a StoredAccountMeta,
    ) -> Option<ReplicaAccountInfo<'a>> {
        Some(ReplicaAccountInfo {
            pubkey: stored_account_meta.meta.pubkey.as_ref(),
            lamports: stored_account_meta.account_meta.lamports,
            owner: stored_account_meta.account_meta.owner.as_ref(),
            executable: stored_account_meta.account_meta.executable,
            rent_epoch: stored_account_meta.account_meta.rent_epoch,
            data: stored_account_meta.data,
            write_version: stored_account_meta.meta.write_version,
        })
    }

    fn build_replica_transaction_log_info<'a>(
        transaction_log_info: &'a TransactionLogInfo,
    ) -> ReplicaTransactionLogInfo<'a> {
        ReplicaTransactionLogInfo {
            signature: transaction_log_info.signature.as_ref(),
            result: get_transaction_status(&transaction_log_info.result),
            is_vote: transaction_log_info.is_vote,
            log_messages: &transaction_log_info.log_messages,
        }
    }

    fn notify_plugins_of_account_update(
        &self,
        account: ReplicaAccountInfo,
        slot: Slot,
        is_startup: bool,
    ) {
        let mut measure2 = Measure::start("accountsdb-plugin-notify_plugins_of_account_update");
        let mut plugin_manager = self.plugin_manager.write().unwrap();

        if plugin_manager.plugins.is_empty() {
            return;
        }
        for plugin in plugin_manager.plugins.iter_mut() {
            let mut measure = Measure::start("accountsdb-plugin-update-account");
            match plugin.update_account(
                ReplicaAccountInfoVersions::V0_0_1(&account),
                slot,
                is_startup,
            ) {
                Err(err) => {
                    error!(
                        "Failed to update account {} at slot {}, error: {} to plugin {}",
                        bs58::encode(account.pubkey).into_string(),
                        slot,
                        err,
                        plugin.name()
                    )
                }
                Ok(_) => {
                    trace!(
                        "Successfully updated account {} at slot {} to plugin {}",
                        bs58::encode(account.pubkey).into_string(),
                        slot,
                        plugin.name()
                    );
                }
            }
            measure.stop();
            inc_new_counter_debug!(
                "accountsdb-plugin-update-account-us",
                measure.as_us() as usize,
                100000,
                100000
            );
        }
        measure2.stop();
        inc_new_counter_debug!(
            "accountsdb-plugin-notify_plugins_of_account_update-us",
            measure2.as_us() as usize,
            100000,
            100000
        );
    }

    pub fn notify_slot_status(&self, slot: Slot, parent: Option<Slot>, slot_status: SlotStatus) {
        let mut plugin_manager = self.plugin_manager.write().unwrap();
        if plugin_manager.plugins.is_empty() {
            return;
        }

        for plugin in plugin_manager.plugins.iter_mut() {
            let mut measure = Measure::start("accountsdb-plugin-update-slot");
            match plugin.update_slot_status(slot, parent, slot_status.clone()) {
                Err(err) => {
                    error!(
                        "Failed to update slot status at slot {}, error: {} to plugin {}",
                        slot,
                        err,
                        plugin.name()
                    )
                }
                Ok(_) => {
                    trace!(
                        "Successfully updated slot status at slot {} to plugin {}",
                        slot,
                        plugin.name()
                    );
                }
            }
            measure.stop();
            inc_new_counter_debug!(
                "accountsdb-plugin-update-slot-us",
                measure.as_us() as usize,
                1000,
                1000
            );
        }
    }

    fn notify_plugins_of_transaction_log_info(
        &self,
        transaction_log_info: &TransactionLogInfo,
        slot: Slot,
    ) {
        let mut measure =
            Measure::start("accountsdb-plugin-notify_plugins_of_transaction_log_info");
        let mut plugin_manager = self.plugin_manager.write().unwrap();

        if plugin_manager.plugins.is_empty() {
            return;
        }

        let transaction_log_info = Self::build_replica_transaction_log_info(transaction_log_info);
        for plugin in plugin_manager.plugins.iter_mut() {
            match plugin.notify_transaction(
                ReplicaTranscaionLogInfoVersions::V0_0_1(&transaction_log_info),
                slot,
            ) {
                Err(err) => {
                    error!(
                        "Failed to notify transaction, error: {} to plugin {}",
                        err,
                        plugin.name()
                    )
                }
                Ok(_) => {
                    trace!(
                        "Successfully notified transaction to plugin {}",
                        plugin.name()
                    );
                }
            }
        }
        measure.stop();
        inc_new_counter_debug!(
            "accountsdb-plugin-notify_plugins_of_transaction_log_info-us",
            measure.as_us() as usize,
            10000,
            10000
        );
    }
}
