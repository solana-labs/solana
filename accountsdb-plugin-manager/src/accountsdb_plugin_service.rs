use {
    crate::{
        accounts_update_notifier::AccountsUpdateNotifier,
        accountsdb_plugin_manager::AccountsDbPluginManager,
        confirmed_slots_observer::SlotConfirmationObserver,
    },
    crossbeam_channel::Receiver,
    log::*,
    solana_runtime::bank_forks::BankForks,
    solana_sdk::clock::Slot,
    std::{
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
    ) -> Self {
        let plugin_manager = AccountsDbPluginManager::new();
        let plugin_manager = Arc::new(RwLock::new(plugin_manager));

        let accounts_update_notifier =
            AccountsUpdateNotifier::new(plugin_manager.clone(), bank_forks);
        let confirmed_slots_observer =
            SlotConfirmationObserver::new(confirmed_bank_receiver, accounts_update_notifier);

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
