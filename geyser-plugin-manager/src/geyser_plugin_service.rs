use {
    crate::{
        accounts_update_notifier::AccountsUpdateNotifierImpl,
        block_metadata_notifier::BlockMetadataNotifierImpl,
        block_metadata_notifier_interface::BlockMetadataNotifierLock,
        geyser_plugin_manager::GeyserPluginManager, slot_status_notifier::SlotStatusNotifierImpl,
        slot_status_observer::SlotStatusObserver, transaction_notifier::TransactionNotifierImpl,
    },
    crossbeam_channel::Receiver,
    log::*,
    solana_rpc::{
        optimistically_confirmed_bank_tracker::BankNotification,
        transaction_notifier_interface::TransactionNotifierLock,
    },
    solana_runtime::accounts_update_notifier_interface::AccountsUpdateNotifier,
    std::{
        fs::File,
        io::Read,
        path::{Path, PathBuf},
        sync::{Arc, RwLock},
        thread,
    },
    thiserror::Error,
};

#[derive(Error, Debug)]
pub enum GeyserPluginServiceError {
    #[error("Cannot open the the plugin config file")]
    CannotOpenConfigFile(String),

    #[error("Cannot read the the plugin config file")]
    CannotReadConfigFile(String),

    #[error("The config file is not in a valid Json format")]
    InvalidConfigFileFormat(String),

    #[error("Plugin library path is not specified in the config file")]
    LibPathNotSet,

    #[error("Invalid plugin path")]
    InvalidPluginPath,

    #[error("Cannot load plugin shared library")]
    PluginLoadError(String),
}

/// The service managing the Geyser plugin workflow.
pub struct GeyserPluginService {
    slot_status_observer: Option<SlotStatusObserver>,
    plugin_manager: Arc<RwLock<GeyserPluginManager>>,
    accounts_update_notifier: Option<AccountsUpdateNotifier>,
    transaction_notifier: Option<TransactionNotifierLock>,
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
        confirmed_bank_receiver: Receiver<BankNotification>,
        geyser_plugin_config_files: &[PathBuf],
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

        let (slot_status_observer, block_metadata_notifier): (
            Option<SlotStatusObserver>,
            Option<BlockMetadataNotifierLock>,
        ) = if account_data_notifications_enabled || transaction_notifications_enabled {
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

        info!("Started GeyserPluginService");
        Ok(GeyserPluginService {
            slot_status_observer,
            plugin_manager,
            accounts_update_notifier,
            transaction_notifier,
            block_metadata_notifier,
        })
    }

    fn load_plugin(
        plugin_manager: &mut GeyserPluginManager,
        geyser_plugin_config_file: &Path,
    ) -> Result<(), GeyserPluginServiceError> {
        let mut file = match File::open(geyser_plugin_config_file) {
            Ok(file) => file,
            Err(err) => {
                return Err(GeyserPluginServiceError::CannotOpenConfigFile(format!(
                    "Failed to open the plugin config file {geyser_plugin_config_file:?}, error: {err:?}"
                )));
            }
        };

        let mut contents = String::new();
        if let Err(err) = file.read_to_string(&mut contents) {
            return Err(GeyserPluginServiceError::CannotReadConfigFile(format!(
                "Failed to read the plugin config file {geyser_plugin_config_file:?}, error: {err:?}"
            )));
        }

        let result: serde_json::Value = match json5::from_str(&contents) {
            Ok(value) => value,
            Err(err) => {
                return Err(GeyserPluginServiceError::InvalidConfigFileFormat(format!(
                    "The config file {geyser_plugin_config_file:?} is not in a valid Json5 format, error: {err:?}"
                )));
            }
        };

        let libpath = result["libpath"]
            .as_str()
            .ok_or(GeyserPluginServiceError::LibPathNotSet)?;
        let mut libpath = PathBuf::from(libpath);
        if libpath.is_relative() {
            let config_dir = geyser_plugin_config_file.parent().ok_or_else(|| {
                GeyserPluginServiceError::CannotOpenConfigFile(format!(
                    "Failed to resolve parent of {geyser_plugin_config_file:?}",
                ))
            })?;
            libpath = config_dir.join(libpath);
        }

        let config_file = geyser_plugin_config_file
            .as_os_str()
            .to_str()
            .ok_or(GeyserPluginServiceError::InvalidPluginPath)?;

        unsafe {
            let result = plugin_manager.load_plugin(libpath.to_str().unwrap(), config_file);
            if let Err(err) = result {
                let msg = format!("Failed to load the plugin library: {libpath:?}, error: {err:?}");
                return Err(GeyserPluginServiceError::PluginLoadError(msg));
            }
        }
        Ok(())
    }

    pub fn get_accounts_update_notifier(&self) -> Option<AccountsUpdateNotifier> {
        self.accounts_update_notifier.clone()
    }

    pub fn get_transaction_notifier(&self) -> Option<TransactionNotifierLock> {
        self.transaction_notifier.clone()
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
}
