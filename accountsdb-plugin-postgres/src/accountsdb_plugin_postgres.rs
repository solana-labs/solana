/// Main entry for the PostgreSQL plugin
use {
    log::*,
    postgres::{Client, NoTls},
    serde_derive::{Deserialize, Serialize},
    serde_json,
    solana_accountsdb_plugin_intf::accountsdb_plugin_intf::{
        AccountsDbPlugin, AccountsDbPluginError, ReplicaAccountInfo, Result,
    },
    std::{fs::File, io::Read},
};

#[derive(Default)]
pub struct AccountsDbPluginPostgres {
    client: Option<Client>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct AccountsDbPluginPostgresConfig {
    host: String,
    user: String,
}

impl AccountsDbPlugin for AccountsDbPluginPostgres {
    fn name(&self) -> &'static str {
        "AccountsDbPluginPostgres"
    }

    fn on_load(&mut self, config_file: &str) -> Result<()> {
        info!(
            "Loading plugin {:?} from config_file {:?}",
            self.name(),
            config_file
        );
        let mut file = File::open(config_file)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;

        let result: serde_json::Result<AccountsDbPluginPostgresConfig> =
            serde_json::from_str(&contents);
        match result {
            Err(err) => {
                return Err(AccountsDbPluginError::ConfigFileReadError {
                    msg: format!(
                        "The config file is not in the JSON format expected: {:?}",
                        err
                    ),
                })
            }
            Ok(config) => {
                let connection_str = format!("host={:?} user={:?}", config.host, config.user);
                match Client::connect(&connection_str, NoTls) {
                    Err(err) => {
                        return Err(AccountsDbPluginError::DataStoreConnectionError {
                            msg: format!(
                                "The config file is not in the JSON format expected: {:?}",
                                err
                            ),
                        });
                    }
                    Ok(client) => {
                        self.client = Some(client);
                    }
                }
            }
        }

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
        AccountsDbPluginPostgres { client: None }
    }
}
