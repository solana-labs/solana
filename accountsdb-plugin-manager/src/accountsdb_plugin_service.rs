use {
    crate::{
        accounts_update_notifier::AccountsUpdateNotifier,
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

pub struct AccountsDbPluginService {
    confirmed_slots_observer: SlotConfirmationObserver,
    plugin_manager: Arc<RwLock<AccountsDbPluginManager>>,
}

impl AccountsDbPluginService {
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

        let accounts_update_notifier =
            AccountsUpdateNotifier::new(plugin_manager.clone(), bank_forks);
        let confirmed_slots_observer =
            SlotConfirmationObserver::new(confirmed_bank_receiver, accounts_update_notifier);

        let mut file = File::open(accountsdb_plugin_config_file).unwrap();
        let mut contents = String::new();
        file.read_to_string(&mut contents).unwrap();

        let result: serde_json::Value = serde_json::from_str(&contents).unwrap();

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
