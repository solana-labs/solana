/// A concurrent implementation for writing accounts into the PostgreSQL in parallel.
use {
    crate::accountsdb_plugin_postgres::AccountsDbPluginPostgresConfig,
    chrono::Utc,
    crossbeam_channel::{unbounded, Receiver, RecvTimeoutError, Sender},
    log::*,
    postgres::{Client, NoTls, Statement},
    solana_accountsdb_plugin_interface::accountsdb_plugin_interface::{
        AccountsDbPluginError, ReplicaAccountInfo,
    },
    std::{
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, Mutex,
        },
        thread::{self, Builder, JoinHandle},
        time::Duration,
    },
};

struct PostgresSqlClientWrapper {
    client: Client,
    update_account_stmt: Statement,
}

struct AccountsWriteWorker {
    client: Mutex<PostgresSqlClientWrapper>,
}

#[derive(Clone, PartialEq, Default, Debug)]
pub struct DbAccountMeta {
    pub pubkey: Vec<u8>,
    pub lamports: u64,
    pub owner: Vec<u8>,
    pub executable: bool,
    pub rent_epoch: u64,
}

impl Eq for DbAccountInfo {}

#[derive(Clone, PartialEq, Debug)]
pub struct DbAccountInfo {
    pub account_meta: DbAccountMeta,
    pub data: Vec<u8>,
}


impl From<&ReplicaAccountInfo<'_>> for DbAccountInfo {

    fn from(account: &ReplicaAccountInfo<'_>) -> Self {
        let account_meta = DbAccountMeta {
            pubkey: account.account_meta.pubkey.to_vec(),
            lamports: account.account_meta.lamports,
            owner: account.account_meta.owner.to_vec(),
            executable: account.account_meta.executable,
            rent_epoch: account.account_meta.rent_epoch,
        };

        let data = account.data.to_vec();
        Self {
            account_meta,
            data,
        }
    }
}

const NUM_ACCOUNTS_WRITE_WORKERS: usize = 10;

impl AccountsWriteWorker {
    fn new(config: AccountsDbPluginPostgresConfig) -> Result<Self, AccountsDbPluginError> {
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
                let result = client.prepare("INSERT INTO account (pubkey, slot, owner, lamports, executable, rent_epoch, data, updated_on) \
                    VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
                    ON CONFLICT (pubkey) DO UPDATE SET slot=$2, owner=$3, lamports=$4, executable=$5, rent_epoch=$6, \
                    data=$7, updated_on=$8");

                match result {
                    Err(err) => {
                        return Err(AccountsDbPluginError::DataSchemaError {
                            msg: format!(
                                "Error in preparing for the accounts update PostgreSQL database: {:?} host: {:?} user: {:?} config: {:?}",
                                err, config.host, config.user, connection_str
                            ),
                        });
                    }
                    Ok(update_account_stmt) => Ok(Self {
                        client: Mutex::new(PostgresSqlClientWrapper {
                            client,
                            update_account_stmt,
                        }),
                    }),
                }
            }
        }
    }

    fn do_work(
        &mut self,
        receiver: Receiver<(DbAccountInfo, u64)>,
        exit_worker: Arc<AtomicBool>,
    ) -> Result<(), AccountsDbPluginError> {
        while !exit_worker.load(Ordering::Relaxed) {
            let account = receiver.recv_timeout(Duration::from_millis(500));

            match account {
                Ok(account) => {
                    let slot = account.1;
                    let account = account.0;
                    debug!(
                        "Updating account {:?} {:?} at slot {:?}",
                        account.account_meta.pubkey, account.account_meta.owner, slot,
                    );

                    let slot = slot as i64; // postgres only supports i64
                    let lamports = account.account_meta.lamports as i64;
                    let rent_epoch = account.account_meta.rent_epoch as i64;
                    let updated_on = Utc::now().naive_utc();
                    let client = self.client.get_mut().unwrap();
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
                            &updated_on,
                        ],
                    );

                    if let Err(err) = result {
                        return Err(AccountsDbPluginError::AccountsUpdateError {
                            msg: format!("Failed to persist the update of account to the PostgreSQL database. Error: {:?}", err)
                        });
                    }
                }
                Err(err) => match err {
                    RecvTimeoutError::Timeout => {
                        continue;
                    }
                    _ => {
                        error!("Error in receiving the item {:?}", err);
                        break;
                    }
                },
            }
        }
        Ok(())
    }
}
pub struct AccountsWriter {
    workers: Vec<JoinHandle<Result<(), AccountsDbPluginError>>>,
    exit_worker: Arc<AtomicBool>,
    sender: Sender<(DbAccountInfo, u64)>,
}

impl AccountsWriter {
    pub fn new(config: &AccountsDbPluginPostgresConfig) -> Result<Self, AccountsDbPluginError> {
        let (sender, receiver) = unbounded();
        let exit_worker = Arc::new(AtomicBool::new(false));
        let mut workers = Vec::default();

        for i in 0..NUM_ACCOUNTS_WRITE_WORKERS {
            let cloned_receiver = receiver.clone();
            let exit_clone = exit_worker.clone();
            let config = config.clone();
            let worker = Builder::new()
                .name(format!("worker-{}", i))
                .spawn(move || -> Result<(), AccountsDbPluginError> {
                    let mut worker = AccountsWriteWorker::new(config)?;
                    worker.do_work(cloned_receiver, exit_clone)?;
                    Ok(())
                })
                .unwrap();

            workers.push(worker);
        }

        Ok(Self {
            workers,
            exit_worker,
            sender,
        })
    }

    pub fn join(mut self) -> thread::Result<()> {
        self.exit_worker.store(true, Ordering::Relaxed);
        while !self.workers.is_empty() {
            let worker = self.workers.pop();
            if worker.is_none() {
                break;
            }
            let worker = worker.unwrap();
            let result = worker.join().unwrap();
            if result.is_err() {
                error!("The worker thread has failed: {:?}", result);
            }
        }

        Ok(())
    }

    pub fn update_account(&mut self, account: &ReplicaAccountInfo, slot: u64) -> Result<(), AccountsDbPluginError> {
        if let Err(err) = self.sender.send((DbAccountInfo::from(account), slot)) {
            return Err(AccountsDbPluginError::AccountsUpdateError {
                msg: format!("Failed to update the account {:?}, error: {:?}", account.account_meta.pubkey, err)
            });
        }
        Ok(())
    }
}
