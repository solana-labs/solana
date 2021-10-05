/// Main entry for the PostgreSQL plugin
use {
    crate::{
        accounts_selector::AccountsSelector,
        postgres_client::{
            ParallelPostgresClient, PostgresClient, PostgresClientBuilder, SimplePostgresClient,
        },
    },
    bs58,
    log::*,
    serde_derive::{Deserialize, Serialize},
    serde_json,
    solana_accountsdb_plugin_interface::accountsdb_plugin_interface::{
        AccountsDbPlugin, AccountsDbPluginError, ReplicaAccountInfoVersions, Result, SlotStatus,
    },
    std::{fs::File, io::Read},
    thiserror::Error,
};

enum PostgresClientEnum {
    Simple(SimplePostgresClient),
    Parallel(ParallelPostgresClient),
}

#[derive(Default)]
pub struct AccountsDbPluginPostgres {
    client: Option<PostgresClientEnum>,
    accounts_selector: Option<AccountsSelector>,
}

impl std::fmt::Debug for AccountsDbPluginPostgres {
    fn fmt(&self, _: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AccountsDbPluginPostgresConfig {
    pub host: String,
    pub user: String,
    pub threads: Option<usize>,
}

#[derive(Error, Debug)]
pub enum AccountsDbPluginPostgresError {
    #[error("Error connecting to the backend data store.")]
    DataStoreConnectionError { msg: String },

    #[error("Error preparing data store schema.")]
    DataSchemaError { msg: String },
}

impl AccountsDbPlugin for AccountsDbPluginPostgres {
    fn name(&self) -> &'static str {
        "AccountsDbPluginPostgres"
    }

    /// Do initialization for the PostgreSQL plugin.
    /// # Arguments
    ///
    /// Format of the config file:
    /// The `accounts_selector` section allows the user to controls accounts selections.
    /// "accounts_selector" : {
    ///     "accounts" : \["pubkey-1", "pubkey-2", ..., "pubkey-n"\],
    /// }
    /// or:
    /// "accounts_selector" = {
    ///     "owners" : \["pubkey-1", "pubkey-2", ..., "pubkey-m"\]
    /// }
    /// Accounts either satisyfing the accounts condition or owners condition will be selected.
    /// When only owners is specified,
    /// all accounts belonging to the owners will be streamed.
    /// The accounts field support wildcard to select all accounts:
    /// "accounts_selector" : {
    ///     "accounts" : \["*"\],
    /// }
    /// "host" specifies the PostgreSQL server.
    /// "user" specifies the PostgreSQL user.
    /// "threads" optional, specifies the number of worker threads for the plugin. A thread
    /// maintains a PostgreSQL connection to the server.
    /// # Examples
    /// {
    ///    "libpath": "/home/solana/target/release/libsolana_accountsdb_plugin_postgres.so",
    ///    "host": "host_foo",
    ///    "user": "solana",
    ///    "threads": 10,
    ///    "accounts_selector" : {
    ///       "owners" : ["9oT9R5ZyRovSVnt37QvVoBttGpNqR3J7unkb567NP8k3"]
    /// }

    fn on_load(&mut self, config_file: &str) -> Result<()> {
        solana_logger::setup_with_default("info");
        info!(
            "Loading plugin {:?} from config_file {:?}",
            self.name(),
            config_file
        );
        let mut file = File::open(config_file)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;

        let result: serde_json::Value = serde_json::from_str(&contents).unwrap();
        self.accounts_selector = Some(Self::create_accounts_selector_from_config(&result));

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
                self.client = if config.threads.is_some() && config.threads.unwrap() > 1 {
                    let client = PostgresClientBuilder::build_pararallel_postgres_client(&config)?;
                    Some(PostgresClientEnum::Parallel(client))
                } else {
                    let client = PostgresClientBuilder::build_simple_postgres_client(&config)?;
                    Some(PostgresClientEnum::Simple(client))
                };
            }
        }

        Ok(())
    }

    fn on_unload(&mut self) {
        info!("Unloading plugin: {:?}", self.name());

        match &mut self.client {
            None => {}
            Some(client) => match client {
                PostgresClientEnum::Parallel(client) => client.join().unwrap(),
                PostgresClientEnum::Simple(client) => {
                    client.join().unwrap();
                }
            },
        }
    }

    fn update_account(&mut self, account: ReplicaAccountInfoVersions, slot: u64) -> Result<()> {
        match account {
            ReplicaAccountInfoVersions::V0_0_1(account) => {
                if let Some(accounts_selector) = &self.accounts_selector {
                    if !accounts_selector.is_account_selected(account.pubkey, account.owner) {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                }

                debug!(
                    "Updating account {:?} with owner {:?} at slot {:?} using account selector {:?}",
                    bs58::encode(account.pubkey).into_string(),
                    bs58::encode(account.owner).into_string(),
                    slot,
                    self.accounts_selector.as_ref().unwrap()
                );

                match &mut self.client {
                    None => {
                        return Err(AccountsDbPluginError::Custom(Box::new(
                            AccountsDbPluginPostgresError::DataStoreConnectionError {
                                msg: "There is no connection to the PostgreSQL database."
                                    .to_string(),
                            },
                        )));
                    }
                    Some(client) => {
                        let result = match client {
                            PostgresClientEnum::Parallel(client) => {
                                client.update_account(account, slot)
                            }
                            PostgresClientEnum::Simple(client) => {
                                client.update_account(account, slot)
                            }
                        };

                        if let Err(err) = result {
                            return Err(AccountsDbPluginError::AccountsUpdateError {
                                msg: format!("Failed to persist the update of account to the PostgreSQL database. Error: {:?}", err)
                            });
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn update_slot_status(
        &mut self,
        slot: u64,
        parent: Option<u64>,
        status: SlotStatus,
    ) -> Result<()> {
        info!("Updating slot {:?} at with status {:?}", slot, status);

        match &mut self.client {
            None => {
                return Err(AccountsDbPluginError::Custom(Box::new(
                    AccountsDbPluginPostgresError::DataStoreConnectionError {
                        msg: "There is no connection to the PostgreSQL database.".to_string(),
                    },
                )));
            }
            Some(client) => {
                let result = match client {
                    PostgresClientEnum::Parallel(client) => {
                        client.update_slot_status(slot, parent, status)
                    }
                    PostgresClientEnum::Simple(client) => {
                        client.update_slot_status(slot, parent, status)
                    }
                };

                if let Err(err) = result {
                    return Err(AccountsDbPluginError::SlotStatusUpdateError{
                        msg: format!("Failed to persist the update of slot to the PostgreSQL database. Error: {:?}", err)
                    });
                }
            }
        }

        Ok(())
    }
}

impl AccountsDbPluginPostgres {
    fn create_accounts_selector_from_config(config: &serde_json::Value) -> AccountsSelector {
        let accounts_selector = &config["accounts_selector"];

        if accounts_selector.is_null() {
            AccountsSelector::default()
        } else {
            let accounts = &accounts_selector["accounts"];
            let accounts: Vec<String> = if accounts.is_array() {
                accounts
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|val| val.as_str().unwrap().to_string())
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
                    .map(|val| val.as_str().unwrap().to_string())
                    .collect()
            } else {
                Vec::default()
            };
            AccountsSelector::new(&accounts, &owners)
        }
    }

    pub fn new() -> Self {
        AccountsDbPluginPostgres {
            client: None,
            accounts_selector: None,
        }
    }
}

#[no_mangle]
#[allow(improper_ctypes_definitions)]
/// # Safety
///
/// This function returns the AccountsDbPluginPostgres pointer as trait AccountsDbPlugin.
pub unsafe extern "C" fn _create_plugin() -> *mut dyn AccountsDbPlugin {
    let plugin = AccountsDbPluginPostgres::new();
    let plugin: Box<dyn AccountsDbPlugin> = Box::new(plugin);
    Box::into_raw(plugin)
}

#[cfg(test)]
pub(crate) mod tests {
    use {super::*, serde_json};

    #[test]
    fn test_accounts_selector_from_config() {
        let config = "{\"accounts_selector\" : { \
           \"owners\" : [\"9xQeWvG816bUx9EPjHmaT23yvVM2ZWbrrpZb9PusVFin\"] \
        }}";

        let config: serde_json::Value = serde_json::from_str(config).unwrap();
        AccountsDbPluginPostgres::create_accounts_selector_from_config(&config);
    }
}
