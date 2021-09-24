use {
    crate::{
        accounts_update_notifier::AccountsUpdateNotifierImpl,
        accountsdb_plugin_manager::AccountsDbPluginManager,
        slot_status_observer::SlotStatusObserver,
    },
    crossbeam_channel::Receiver,
    log::*,
    serde_json,
    solana_rpc::optimistically_confirmed_bank_tracker::BankNotification,
    solana_runtime::accounts_db::AccountsUpdateNotifier,
    std::{
        fs::File,
        io::Read,
        path::Path,
        sync::{Arc, RwLock},
        thread,
    },
    thiserror::Error,
};

#[derive(Error, Debug)]
pub enum AccountsdbPluginServiceError {
    #[error("Plugin library path is not specified in the config file")]
    LibPathNotSet,

    #[error("Invalid plugin path")]
    InvalidPluginPath,

    #[error("Cannot load plugin shared library")]
    PluginLoadError(String),
}

/// The service managing the AccountsDb plugin workflow.
pub struct AccountsDbPluginService {
    confirmed_slots_observer: SlotStatusObserver,
    plugin_manager: Arc<RwLock<AccountsDbPluginManager>>,
    accounts_update_notifier: AccountsUpdateNotifier,
}

impl AccountsDbPluginService {
    /// Creates and returns the AccountsDbPluginService.
    /// # Arguments
    /// * `confirmed_bank_receiver` - The receiver for confirmed bank notification
    /// * `accountsdb_plugin_config_file` - The config file path for the plugin. The
    ///    config file controls what accounts to stream and the plugin responsible
    ///    for transporting the data to external data stores. It is defined in JSON format.
    ///    The `libpath` field should be pointed to the full path of the dynamic shared library
    ///    (.so file) to be loaded. The shared library must implement the `AccountsDbPlugin`
    ///    trait. And the shared library shall export a `C` function `_create_plugin` which
    ///    shall create the implementation of `AccountsDbPlugin` and returns to the caller.
    ///    The rest of the JSON fields' definition is up to to the concrete plugin implementation
    ///    It is usually used to configure the connection information for the external data store.

    pub fn new(
        confirmed_bank_receiver: Receiver<BankNotification>,
        accountsdb_plugin_config_file: &Path,
    ) -> Result<Self, AccountsdbPluginServiceError> {
        info!(
            "Starting AccountsDbPluginService from config file: {:?}",
            accountsdb_plugin_config_file
        );
        let plugin_manager = AccountsDbPluginManager::new();
        let plugin_manager = Arc::new(RwLock::new(plugin_manager));

        let mut file = File::open(accountsdb_plugin_config_file).unwrap();
        let mut contents = String::new();
        file.read_to_string(&mut contents).unwrap();

        let result: serde_json::Value = serde_json::from_str(&contents).unwrap();

        let accounts_update_notifier = Arc::new(RwLock::new(AccountsUpdateNotifierImpl::new(
            plugin_manager.clone(),
        )));
        let confirmed_slots_observer =
            SlotStatusObserver::new(confirmed_bank_receiver, accounts_update_notifier.clone());

        let libpath = result["libpath"]
            .as_str()
            .ok_or(AccountsdbPluginServiceError::LibPathNotSet)?;
        let config_file = accountsdb_plugin_config_file
            .as_os_str()
            .to_str()
            .ok_or(AccountsdbPluginServiceError::InvalidPluginPath)?;

        unsafe {
            let result = plugin_manager
                .write()
                .unwrap()
                .load_plugin(libpath, config_file);
            if let Err(err) = result {
                let msg = format!(
                    "Failed to load the plugin library: {:?}, error: {:?}",
                    libpath, err
                );
                return Err(AccountsdbPluginServiceError::PluginLoadError(msg));
            }
        }

        info!("Started AccountsDbPluginService");
        Ok(AccountsDbPluginService {
            confirmed_slots_observer,
            plugin_manager,
            accounts_update_notifier,
        })
    }

    pub fn get_accounts_update_notifier(&self) -> AccountsUpdateNotifier {
        self.accounts_update_notifier.clone()
    }

    pub fn join(mut self) -> thread::Result<()> {
        self.confirmed_slots_observer.join()?;
        self.plugin_manager.write().unwrap().unload();
        Ok(())
    }
}
