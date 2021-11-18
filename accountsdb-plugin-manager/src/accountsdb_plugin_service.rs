use {
    crate::{
        accounts_update_notifier::AccountsUpdateNotifierImpl,
        accountsdb_plugin_manager::AccountsDbPluginManager,
        slot_status_notifier::SlotStatusNotifierImpl, slot_status_observer::SlotStatusObserver,
    },
    crossbeam_channel::Receiver,
    log::*,
    serde_json,
    solana_rpc::optimistically_confirmed_bank_tracker::BankNotification,
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
pub enum AccountsdbPluginServiceError {
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

/// The service managing the AccountsDb plugin workflow.
pub struct AccountsDbPluginService {
    slot_status_observer: Option<SlotStatusObserver>,
    plugin_manager: Arc<RwLock<AccountsDbPluginManager>>,
    accounts_update_notifier: Option<AccountsUpdateNotifier>,
}

impl AccountsDbPluginService {
    /// Creates and returns the AccountsDbPluginService.
    /// # Arguments
    /// * `confirmed_bank_receiver` - The receiver for confirmed bank notification
    /// * `accountsdb_plugin_config_file` - The config file path for the plugin. The
    ///    config file controls the plugin responsible
    ///    for transporting the data to external data stores. It is defined in JSON format.
    ///    The `libpath` field should be pointed to the full path of the dynamic shared library
    ///    (.so file) to be loaded. The shared library must implement the `AccountsDbPlugin`
    ///    trait. And the shared library shall export a `C` function `_create_plugin` which
    ///    shall create the implementation of `AccountsDbPlugin` and returns to the caller.
    ///    The rest of the JSON fields' definition is up to to the concrete plugin implementation
    ///    It is usually used to configure the connection information for the external data store.

    pub fn new(
        confirmed_bank_receiver: Receiver<BankNotification>,
        accountsdb_plugin_config_files: &[PathBuf],
    ) -> Result<Self, AccountsdbPluginServiceError> {
        info!(
            "Starting AccountsDbPluginService from config files: {:?}",
            accountsdb_plugin_config_files
        );
        let mut plugin_manager = AccountsDbPluginManager::new();

        for accountsdb_plugin_config_file in accountsdb_plugin_config_files {
            Self::load_plugin(&mut plugin_manager, accountsdb_plugin_config_file)?;
        }
        let to_notify_account_data = plugin_manager.to_notify_account_data();

        let plugin_manager = Arc::new(RwLock::new(plugin_manager));

        let accounts_update_notifier: Option<AccountsUpdateNotifier> = if to_notify_account_data {
            let accounts_update_notifier = AccountsUpdateNotifierImpl::new(plugin_manager.clone());
            Some(Arc::new(RwLock::new(accounts_update_notifier)))
        } else {
            None
        };

        let slot_status_observer = if to_notify_account_data {
            let slot_status_notifier = SlotStatusNotifierImpl::new(plugin_manager.clone());
            let slot_status_notifier = Arc::new(RwLock::new(slot_status_notifier));
            Some(SlotStatusObserver::new(
                confirmed_bank_receiver,
                slot_status_notifier,
            ))
        } else {
            None
        };

        info!("Started AccountsDbPluginService");
        Ok(AccountsDbPluginService {
            slot_status_observer,
            plugin_manager,
            accounts_update_notifier,
        })
    }

    fn load_plugin(
        plugin_manager: &mut AccountsDbPluginManager,
        accountsdb_plugin_config_file: &Path,
    ) -> Result<(), AccountsdbPluginServiceError> {
        let mut file = match File::open(accountsdb_plugin_config_file) {
            Ok(file) => file,
            Err(err) => {
                return Err(AccountsdbPluginServiceError::CannotOpenConfigFile(format!(
                    "Failed to open the plugin config file {:?}, error: {:?}",
                    accountsdb_plugin_config_file, err
                )));
            }
        };

        let mut contents = String::new();
        if let Err(err) = file.read_to_string(&mut contents) {
            return Err(AccountsdbPluginServiceError::CannotReadConfigFile(format!(
                "Failed to read the plugin config file {:?}, error: {:?}",
                accountsdb_plugin_config_file, err
            )));
        }

        let result: serde_json::Value = match serde_json::from_str(&contents) {
            Ok(value) => value,
            Err(err) => {
                return Err(AccountsdbPluginServiceError::InvalidConfigFileFormat(
                    format!(
                        "The config file {:?} is not in a valid Json format, error: {:?}",
                        accountsdb_plugin_config_file, err
                    ),
                ));
            }
        };

        let libpath = result["libpath"]
            .as_str()
            .ok_or(AccountsdbPluginServiceError::LibPathNotSet)?;
        let config_file = accountsdb_plugin_config_file
            .as_os_str()
            .to_str()
            .ok_or(AccountsdbPluginServiceError::InvalidPluginPath)?;

        unsafe {
            let result = plugin_manager.load_plugin(libpath, config_file);
            if let Err(err) = result {
                let msg = format!(
                    "Failed to load the plugin library: {:?}, error: {:?}",
                    libpath, err
                );
                return Err(AccountsdbPluginServiceError::PluginLoadError(msg));
            }
        }
        Ok(())
    }

    pub fn get_accounts_update_notifier(&self) -> Option<AccountsUpdateNotifier> {
        self.accounts_update_notifier.clone()
    }

    pub fn join(self) -> thread::Result<()> {
        if let Some(mut slot_status_observer) = self.slot_status_observer {
            slot_status_observer.join()?;
        }
        self.plugin_manager.write().unwrap().unload();
        Ok(())
    }
}
