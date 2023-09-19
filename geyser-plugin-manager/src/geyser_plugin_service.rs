use {
    crate::{
        accounts_update_notifier::AccountsUpdateNotifierImpl,
        block_metadata_notifier::BlockMetadataNotifierImpl,
        block_metadata_notifier_interface::BlockMetadataNotifierLock,
        entry_notifier::EntryNotifierImpl,
        geyser_plugin_manager::{GeyserPluginManager, GeyserPluginManagerRequest},
        slot_status_notifier::SlotStatusNotifierImpl,
        slot_status_observer::SlotStatusObserver,
        transaction_notifier::TransactionNotifierImpl,
    },
    crossbeam_channel::Receiver,
    log::*,
    solana_accounts_db::accounts_update_notifier_interface::AccountsUpdateNotifier,
    solana_ledger::entry_notifier_interface::EntryNotifierLock,
    solana_rpc::{
        optimistically_confirmed_bank_tracker::SlotNotification,
        transaction_notifier_interface::TransactionNotifierLock,
    },
    std::{
        path::{Path, PathBuf},
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, RwLock,
        },
        thread,
        time::Duration,
    },
    thiserror::Error,
};

/// The service managing the Geyser plugin workflow.
pub struct GeyserPluginService {
    slot_status_observer: Option<SlotStatusObserver>,
    plugin_manager: Arc<RwLock<GeyserPluginManager>>,
    accounts_update_notifier: Option<AccountsUpdateNotifier>,
    transaction_notifier: Option<TransactionNotifierLock>,
    entry_notifier: Option<EntryNotifierLock>,
    block_metadata_notifier: Option<BlockMetadataNotifierLock>,
}

impl GeyserPluginService {
    /// Creates and returns the GeyserPluginService.
    /// # Arguments
    /// * `confirmed_bank_receiver` - The receiver for confirmed bank notification
    /// * `geyser_plugin_config_file` - The config file path for the plugin. The
    ///    config file controls the plugin responsible
    ///    for transporting the data to external data stores. It is defined in JSON format.
    ///    The `libpath` field should be pointed to the full path of the dynamic shared library
    ///    (.so file) to be loaded. The shared library must implement the `GeyserPlugin`
    ///    trait. And the shared library shall export a `C` function `_create_plugin` which
    ///    shall create the implementation of `GeyserPlugin` and returns to the caller.
    ///    The rest of the JSON fields' definition is up to to the concrete plugin implementation
    ///    It is usually used to configure the connection information for the external data store.
    pub fn new(
        confirmed_bank_receiver: Receiver<SlotNotification>,
        geyser_plugin_config_files: &[PathBuf],
    ) -> Result<Self, GeyserPluginServiceError> {
        Self::new_with_receiver(confirmed_bank_receiver, geyser_plugin_config_files, None)
    }

    pub fn new_with_receiver(
        confirmed_bank_receiver: Receiver<SlotNotification>,
        geyser_plugin_config_files: &[PathBuf],
        rpc_to_plugin_manager_receiver_and_exit: Option<(
            Receiver<GeyserPluginManagerRequest>,
            Arc<AtomicBool>,
        )>,
    ) -> Result<Self, GeyserPluginServiceError> {
        info!(
            "Starting GeyserPluginService from config files: {:?}",
            geyser_plugin_config_files
        );
        let mut plugin_manager = GeyserPluginManager::new();

        for geyser_plugin_config_file in geyser_plugin_config_files {
            Self::load_plugin(&mut plugin_manager, geyser_plugin_config_file)?;
        }

        let account_data_notifications_enabled =
            plugin_manager.account_data_notifications_enabled();
        let transaction_notifications_enabled = plugin_manager.transaction_notifications_enabled();
        let entry_notifications_enabled = plugin_manager.entry_notifications_enabled();
        let plugin_manager = Arc::new(RwLock::new(plugin_manager));

        let accounts_update_notifier: Option<AccountsUpdateNotifier> =
            if account_data_notifications_enabled {
                let accounts_update_notifier =
                    AccountsUpdateNotifierImpl::new(plugin_manager.clone());
                Some(Arc::new(RwLock::new(accounts_update_notifier)))
            } else {
                None
            };

        let transaction_notifier: Option<TransactionNotifierLock> =
            if transaction_notifications_enabled {
                let transaction_notifier = TransactionNotifierImpl::new(plugin_manager.clone());
                Some(Arc::new(RwLock::new(transaction_notifier)))
            } else {
                None
            };

        let entry_notifier: Option<EntryNotifierLock> = if entry_notifications_enabled {
            let entry_notifier = EntryNotifierImpl::new(plugin_manager.clone());
            Some(Arc::new(RwLock::new(entry_notifier)))
        } else {
            None
        };

        let (slot_status_observer, block_metadata_notifier): (
            Option<SlotStatusObserver>,
            Option<BlockMetadataNotifierLock>,
        ) = if account_data_notifications_enabled
            || transaction_notifications_enabled
            || entry_notifications_enabled
        {
            let slot_status_notifier = SlotStatusNotifierImpl::new(plugin_manager.clone());
            let slot_status_notifier = Arc::new(RwLock::new(slot_status_notifier));
            (
                Some(SlotStatusObserver::new(
                    confirmed_bank_receiver,
                    slot_status_notifier,
                )),
                Some(Arc::new(RwLock::new(BlockMetadataNotifierImpl::new(
                    plugin_manager.clone(),
                )))),
            )
        } else {
            (None, None)
        };

        // Initialize plugin manager rpc handler thread if needed
        if let Some((request_receiver, exit)) = rpc_to_plugin_manager_receiver_and_exit {
            let plugin_manager = plugin_manager.clone();
            Self::start_manager_rpc_handler(plugin_manager, request_receiver, exit)
        };

        info!("Started GeyserPluginService");
        Ok(GeyserPluginService {
            slot_status_observer,
            plugin_manager,
            accounts_update_notifier,
            transaction_notifier,
            entry_notifier,
            block_metadata_notifier,
        })
    }

    fn load_plugin(
        plugin_manager: &mut GeyserPluginManager,
        geyser_plugin_config_file: &Path,
    ) -> Result<(), GeyserPluginServiceError> {
        plugin_manager
            .load_plugin(geyser_plugin_config_file)
            .map_err(|e| GeyserPluginServiceError::FailedToLoadPlugin(e.into()))?;
        Ok(())
    }

    pub fn get_accounts_update_notifier(&self) -> Option<AccountsUpdateNotifier> {
        self.accounts_update_notifier.clone()
    }

    pub fn get_transaction_notifier(&self) -> Option<TransactionNotifierLock> {
        self.transaction_notifier.clone()
    }

    pub fn get_entry_notifier(&self) -> Option<EntryNotifierLock> {
        self.entry_notifier.clone()
    }

    pub fn get_block_metadata_notifier(&self) -> Option<BlockMetadataNotifierLock> {
        self.block_metadata_notifier.clone()
    }

    pub fn join(self) -> thread::Result<()> {
        if let Some(mut slot_status_observer) = self.slot_status_observer {
            slot_status_observer.join()?;
        }
        self.plugin_manager.write().unwrap().unload();
        Ok(())
    }

    fn start_manager_rpc_handler(
        plugin_manager: Arc<RwLock<GeyserPluginManager>>,
        request_receiver: Receiver<GeyserPluginManagerRequest>,
        exit: Arc<AtomicBool>,
    ) {
        thread::Builder::new()
            .name("SolGeyserPluginRpc".to_string())
            .spawn(move || loop {
                if let Ok(request) = request_receiver.recv_timeout(Duration::from_secs(5)) {
                    match request {
                        GeyserPluginManagerRequest::ListPlugins { response_sender } => {
                            let plugin_list = plugin_manager.read().unwrap().list_plugins();
                            response_sender
                                .send(plugin_list)
                                .expect("Admin rpc service will be waiting for response");
                        }

                        GeyserPluginManagerRequest::ReloadPlugin {
                            ref name,
                            ref config_file,
                            response_sender,
                        } => {
                            let reload_result = plugin_manager
                                .write()
                                .unwrap()
                                .reload_plugin(name, config_file);
                            response_sender
                                .send(reload_result)
                                .expect("Admin rpc service will be waiting for response");
                        }

                        GeyserPluginManagerRequest::LoadPlugin {
                            ref config_file,
                            response_sender,
                        } => {
                            let load_result =
                                plugin_manager.write().unwrap().load_plugin(config_file);
                            response_sender
                                .send(load_result)
                                .expect("Admin rpc service will be waiting for response");
                        }

                        GeyserPluginManagerRequest::UnloadPlugin {
                            ref name,
                            response_sender,
                        } => {
                            let unload_result = plugin_manager.write().unwrap().unload_plugin(name);
                            response_sender
                                .send(unload_result)
                                .expect("Admin rpc service will be waiting for response");
                        }
                    }
                }

                if exit.load(Ordering::Relaxed) {
                    break;
                }
            })
            .unwrap();
    }
}

#[derive(Error, Debug)]
pub enum GeyserPluginServiceError {
    #[error("Failed to load a geyser plugin")]
    FailedToLoadPlugin(#[from] Box<dyn std::error::Error>),
}
