use postgres::Statement;

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

struct PostgresSqlClientWrapper {
    client: Client,
    update_account_stmt: Statement,
}

#[derive(Default)]
pub struct AccountsDbPluginPostgres {
    client: Option<Mutex<PostgresSqlClientWrapper>>,
}

impl std::fmt::Debug for AccountsDbPluginPostgres {
    fn fmt(&self, _: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct AccountsDbPluginPostgresConfig {
    host: String,
    user: String,
}

fn get_status_str(status: SlotStatus) -> &'static str {
    match status {
        SlotStatus::Confirmed => "confirmed",
        SlotStatus::Processed => "processed",
        SlotStatus::Rooted => "rooted",
    }
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
                    Ok(mut client) => {
                        let result = client.prepare("INSERT INTO account (pubkey, slot, owner, lamports, executable, rent_epoch, data, hash, updated_on) \
                            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) \
                            ON CONFLICT (pubkey) DO UPDATE SET slot=$2, owner=$3, lamports=$4, executable=$5, rent_epoch=$6, \
                            data=$7, hash=$8, updated_on=$9");

                        match result {
                            Err(err) => {
                                return Err(AccountsDbPluginError::DataSchemaError {
                                    msg: format!(
                                        "Error in preparing for the accounts update PostgreSQL database: {:?} host: {:?} user: {:?} config: {:?}",
                                        err, config.host, config.user, connection_str
                                    ),
                                });
                            }
                            Ok(update_account_stmt) => {
                                self.client = Some(Mutex::new(PostgresSqlClientWrapper {
                                    client,
                                    update_account_stmt,
                                }));
                            }
                        }
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
        debug!("Updating account {:?} at slot {:?}", account, slot);

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
                let client = client.get_mut().unwrap();
                let result = client.client.query(
                    &client.update_account_stmt,
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

                if let Err(err) = result {
                    return Err(AccountsDbPluginError::AccountsUpdateError {
                        msg: format!("Failed to persist the update of account to the PostgreSQL database. Error: {:?}", err)
                    });
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
                return Err(AccountsDbPluginError::DataStoreConnectionError {
                    msg: "There is no connection to the PostgreSQL database.".to_string(),
                });
            }
            Some(client) => {
                let slot = slot as i64; // postgres only support i64
                let parent = parent.map(|parent| parent as i64);
                let updated_on = Utc::now().naive_utc();
                let status_str = get_status_str(status);

                let result = match parent {
                        Some(parent) => {
                            client.get_mut().unwrap().client.execute(
                                "INSERT INTO slot (slot, parent, status, updated_on) \
                                VALUES ($1, $2, $3, $4) \
                                ON CONFLICT (slot) DO UPDATE SET parent=$2, status=$3, updated_on=$4",
                                &[
                                    &slot,
                                    &parent,
                                    &status_str,
                                    &updated_on,
                                ],
                            )
                        }
                        None => {
                            client.get_mut().unwrap().client.execute(
                                "INSERT INTO slot (slot, status, updated_on) \
                                VALUES ($1, $2, $3) \
                                ON CONFLICT (slot) DO UPDATE SET status=$2, updated_on=$3",
                                &[
                                    &slot,
                                    &status_str,
                                    &updated_on,
                                ],
                            )
                        }
                };

                match result {
                    Err(err) => {
                        return Err(AccountsDbPluginError::AccountsUpdateError {
                            msg: format!("Failed to persist the update of slot to the PostgreSQL database. Error: {:?}", err)
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
