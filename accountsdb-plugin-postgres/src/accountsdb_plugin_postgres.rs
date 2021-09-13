/// Managing the AccountsDb plugins
use {
    log::*,
    solana_accountsdb_plugin_intf::accountsdb_plugin_intf::{AccountsDbPlugin, ReplicaAccountInfo, Result},
};

pub struct AccountsDbPluginPostgres {
}

impl AccountsDbPlugin for AccountsDbPluginPostgres {

    fn name(&self) -> &'static str {
        "AccountsDbPluginPostgres"
    }

    fn on_load(
        &mut self,
        config_file: &str,
    ) -> Result<()> {
        info!("Loading plugin {:?} from config_file {:?}", self.name(), config_file);
        Ok(())
    }

    /// Unload all plugins and loaded plugin libraries, making sure to fire
    /// their `on_plugin_unload()` methods so they can do any necessary cleanup.
    fn on_unload(&mut self) {
        info!("Unloading plugin: {:?}", self.name());
    }


    fn update_account(&self, account: ReplicaAccountInfo, slot: u64) -> Result<()> {
        info!("Updating account {:?} at slot {:?}", account, slot);
        Ok(())
    }
}


impl AccountsDbPluginPostgres {
    pub fn new() -> Self {
        AccountsDbPluginPostgres {

        }
    }
}