/// Main entry for the PostgreSQL plugin
use {
    chrono::Utc,
    log::*,
    postgres::{Client, NoTls},
    serde_derive::{Deserialize, Serialize},
    serde_json,
    solana_accountsdb_plugin_intf::accountsdb_plugin_intf::{
        AccountsDbPlugin, AccountsDbPluginError, ReplicaAccountInfo, Result, SlotStatus,
    },
    std::{fs::File, io::Read, sync::Mutex},
};

#[derive(Default)]
pub struct AccountsDbPluginPostgres {
    client: Option<Mutex<Client>>,
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
                let connection_str = format!("host={} user={}", config.host, config.user);
                match Client::connect(&connection_str, NoTls) {
                    Err(err) => {
                        return Err(AccountsDbPluginError::DataStoreConnectionError {
                            msg: format!(
                                "Error in connecting to the PostgreSQL database: {:?} host: {:?} user: {:?} config: {:?}",
                                err, config.host, config.user, connection_str
                            ),
                        });
                    }
                    Ok(client) => {
                        self.client = Some(Mutex::new(client));
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

    fn update_account(&mut self, account: &ReplicaAccountInfo, slot: u64) -> Result<()> {
        info!("Updating account {:?} at slot {:?}", account, slot);

        match &mut self.client {
            None => {
                return Err(AccountsDbPluginError::DataStoreConnectionError {
                    msg: "There is no connection to the PostgreSQL database.".to_string(),
                });
            }
            Some(client) => {
                let slot = slot as i64; // postgres only support i64
                let lamports = account.account_meta.lamports as i64;
                let rent_epoch = account.account_meta.rent_epoch as i64;
                let updated_on = Utc::now().naive_utc();
                let result = client.get_mut().unwrap().execute(
                    "INSERT INTO account (pubkey, slot, owner, lamports, executable, rent_epoch, data, hash, updated_on) \
                    VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) \
                    ON CONFLICT (pubkey) DO UPDATE SET slot=$2, owner=$3, lamports=$4, executable=$5, rent_epoch=$6, \
                    data=$7, hash=$8, updated_on=$9",
                    &[
                        &account.account_meta.pubkey,
                        &slot,
                        &account.account_meta.owner,
                        &lamports,
                        &account.account_meta.executable,
                        &rent_epoch,
                        &account.data,
                        &account.hash,
                        &updated_on,
                    ],
                );

                match result {
                    Err(err) => {
                        return Err(AccountsDbPluginError::AccountsUpdateError {
                            msg: format!("Failed to persist the update of account to the PostgreSQL database. Error: {:?}", err)
                        });
                    }
                    Ok(rows) => {
                        assert_eq!(1, rows, "Expected one rows to be updated a time");
                    }
                }
            }
        }

        Ok(())
    }

    fn update_slot_status(&mut self, _: u64, _: SlotStatus) -> Result<()> {
        Ok(())
    }
}

impl AccountsDbPluginPostgres {
    pub fn new() -> Self {
        AccountsDbPluginPostgres { client: None }
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
