use {
    crate::{
        accounts_update_notifier::{AccountsSelector, AccountsUpdateNotifier},
        accountsdb_plugin_manager::AccountsDbPluginManager,
        confirmed_slots_observer::SlotConfirmationObserver,
    },
    crossbeam_channel::Receiver,
    log::*,
    serde_json,
    solana_runtime::bank_forks::BankForks,
    solana_sdk::clock::Slot,
    std::{
        fs::File,
        io::Read,
        path::Path,
        sync::{Arc, RwLock},
        thread,
    },
};

/// The service managing the AccountsDb plugin workflow.
pub struct AccountsDbPluginService {
    confirmed_slots_observer: SlotConfirmationObserver,
    plugin_manager: Arc<RwLock<AccountsDbPluginManager>>,
}

impl AccountsDbPluginService {
    /// Creates and returns the AccountsDbPluginService.
    /// # Arguments
    /// * `confirmed_bank_receiver` - The receiver for confirmed bank notification
    /// * `bank_forks` - The bank forks.
    /// * `accountsdb_plugin_config_file` - The config file path for the plugin. The
    ///    config file controls what accounts to stream and the plugin responsible
    ///    for transporting the data to external data stores. It is defined in JSON format.
    ///    The `libpath` field should be pointed to the full path of the dynamic shared library
    ///    (.so file) to be loaded. The shared library must implement the `AccountsDbPlugin`
    ///    trait. And the shared library shall export a `C` function `_create_plugin` which
    ///    shall create the implementation of `AccountsDbPlugin` and returns to the caller.
    ///    The `accounts_selector` section allows the user to controls accounts selections.
    ///    accounts_selector = {
    ///         accounts = \['pubkey-1', 'pubkey-2', ..., 'pubkey-n'\],
    ///    }
    ///    or:
    ///    accounts_selector = {
    ///         owners = \['pubkey-1', 'pubkey-2', ..., 'pubkey-m'\]
    ///    }
    ///    When accounts and owners are specified, the program only filters only using accounts
    ///    as it is more selective. And owners field is ignored. When only owners is specified,
    ///    all accounts belonging to the owners will be streamed.
    ///    The rest of the JSON fields's definition is upto to the concrete plugin implementation
    ///    It is usually used to configure the connection information for the external data store.
    ///    The accounts field support wildcard to select all accounts:
    ///    accounts_selector = {
    ///         accounts = \['*'\],
    ///    }
    pub fn new(
        confirmed_bank_receiver: Receiver<Slot>,
        bank_forks: Arc<RwLock<BankForks>>,
        accountsdb_plugin_config_file: &Path,
    ) -> Self {
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
        let accounts_selector = &result["accounts_selector"];

        let accounts_selector = if accounts_selector.is_null() {
            AccountsSelector::default()
        } else {
            let accounts = &accounts_selector["accounts"];
            let accounts: Vec<String> = if accounts.is_array() {
                accounts
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|val| val.to_string())
                    .collect()
            } else {
                Vec::default()
            };
            let owners = &accounts_selector["owners"];
            let owners: Vec<String> = if owners.is_array() {
                owners
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|val| val.to_string())
                    .collect()
            } else {
                Vec::default()
            };
            AccountsSelector::new(&accounts, &owners)
        };

        let accounts_update_notifier =
            AccountsUpdateNotifier::new(plugin_manager.clone(), bank_forks, accounts_selector);
        let confirmed_slots_observer =
            SlotConfirmationObserver::new(confirmed_bank_receiver, accounts_update_notifier);

        unsafe {
            plugin_manager
                .write()
                .unwrap()
                .load_plugin(
                    result["libpath"].as_str().unwrap(),
                    accountsdb_plugin_config_file.as_os_str().to_str().unwrap(),
                )
                .unwrap();
        }

        info!("Started AccountsDbPluginService");
        AccountsDbPluginService {
            confirmed_slots_observer,
            plugin_manager,
        }
    }

    pub fn join(mut self) -> thread::Result<()> {
        self.confirmed_slots_observer.join()?;
        self.plugin_manager.write().unwrap().unload();
        Ok(())
    }
}
