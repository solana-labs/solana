---
title: AccountsDb Plugins
---

Overview
========

It has been observed that validators under heavy RPC loads such as when serving getProgramAccounts calls can fall behind the network. To solve this problem, the Solana validator has been enhanced to support a plugin mechanism through which the information about accounts and slots can be transmitted to external data stores such as relational databases, NoSQL databases or Kafka. Then RPC services can be developed to consume data from these external data stores with the possibility of more flexible and targeted optimizations such as caching and indexing. This will enable the validator to be more focused on processing transactions instead of being slowed down with handling RPC requests.

This document describes the interfaces of the plugin and the referential plugin implementation for the PostgreSQL database.

[crates.io]: https://crates.io/search?q=solana-
[docs.rs]: https://docs.rs/releases/search?query=solana-

Important crates:

- [`solana-accountsdb-plugin-interface`] &mdash; This crates defines the plugin interfaces.

- [`solana-accountsdb-plugin-postgres`] &mdash; The crate for the referential plugin implementation for the PostgreSQL database.

[`solana-accountsdb-plugin-interface`]: https://docs.rs/solana-accountsdb-plugin-interface
[`solana-accountsdb-plugin-postgres`]: https://docs.rs/solana-accountsdb-plugin-postgres


The Plugin Interface
====================

The Plugin interface is declared in [`solana-accountsdb-plugin-interface`]. It is defined by the trait `AccountsDbPlugin`. The plugin should implement the trait and expose a "C" function _create_plugin to return the pointer to this trait. For example, in the referential implementation. The following code instantiate the PostgreSQL plugin `AccountsDbPluginPostgres ` and return its pointer.

```
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
```

A plugin implementation can implement the `on_load` method to initialize itself. This function is invoked after a plugin is dynmically into the validator when it starts. The configuration of the plugin is controlled by the configfile in the JSON format. The JSON file must has a field `libpath` which should point to the full path name of the shared library implementing the plugin and the plugin can extend it with other configuration information such conneciton information for the external database. The plugin configfile is specified by the validator's runtime parameter `--accountsdb-plugin-config` it must be readable to the validator process.

For example, this is an example configfile for the referential PostgreSQL plugin:

```
{
	"libpath": "/solana/target/release/libsolana_accountsdb_plugin_postgres.so",
	"host": "my-host",
	"user": "solana",
	"port": 5433,
	"threads": 20,
	"batch_size": 20,
	"panic_on_db_errors": true,
	"accounts_selector" : {
           "accounts" : ["*"]
    }
}
```

The plugin can implement `on_unload` method to do any cleanup before the plugin is unloaded when the validator is gracefully shutdown.

The following method is used for notifying accounts update:

```
    fn update_account(
        &mut self,
        account: ReplicaAccountInfoVersions,
        slot: u64,
        is_startup: bool,
    ) -> Result<()>
```

The `ReplicaAccountInfoVersions` contains the metadata of the account being updated. The `slot` points to the slot the account is being updated at. When `is_startup` is true, it indicates the account is loaded from snapshots when the validator starts up. When `is_startup` is false, the account is updated during transaction processing.


The following method is called when all accounts have been notified when the validator restore the AccountsDb from snapshots at startup.

```
fn notify_end_of_startup(&mut self) -> Result<()>
```

When `update_account` is called during processing transactions, the plugin should processing the notification as fast as possible as any delay may cause the validator to fall behind the network. Persistence to external data store can be done asynchronously.

The following method is used for notifying slot status changes:

```
    fn update_slot_status(
        &mut self,
        slot: u64,
        parent: Option<u64>,
        status: SlotStatus,
    ) -> Result<()>
```

To ensure data consistency, the plugin implementation can choose to abort the validator in case of error persisting to external stores. And when the validator restarts the account data will be re-transmitted.

For more details, please refer to the Rust documentation in [`solana-accountsdb-plugin-interface`].

The PostgreSQL Plugin
=====================

The [`solana-accountsdb-plugin-postgres`] crate impelemented a plugin storing account data to a PostgreSQL database to illustrate how a plugin can be developed.

## Configuration File Format

The plugin is configured using the input configuration file. An example configuration file looks like the following:

```
{
	"libpath": "/solana/target/release/libsolana_accountsdb_plugin_postgres.so",
	"host": "postgres-server",
	"user": "solana",
	"port": 5433,
	"threads": 20,
	"batch_size": 20,
	"panic_on_db_errors": true,
	"accounts_selector" : {
		"accounts" : ["*"]
	}
}
```

The host, user, and port controls the PostgreSQL configuration information. For more advanced connection options, please use the connection_str field. Please see [Rust postgres configuration](https://docs.rs/postgres/0.19.2/postgres/config/struct.Config.html) for the detailed format.

To improve the throuput to the database, the plugin supports connection pooling of configured threads each maintaining a connection to the PostgreSQL database. The count of the threads is controled by the `threads` field. Higher threads count offers better performance usually.

To further improve performance when saving large number of accounts at the startup, the plugin uses bulk inserts. The batch size is controlled by the `batch_size` parameter. This can help reduce the round trips to the database.

The `panic_on_db_errors` can be used to panic the validator in case of database errors to ensure data consistency.

## Account Selection

The `accounts_selector` can be used to filter the accounts to persist.

For example, one can use the following to persist only the accounts with the Base58 encoded Pubkeys,

```
    "accounts_selector" : {
         "accounts" : ["pubkey-1", "pubkey-2", ..., "pubkey-n"],
    }
```

Or use the following to select account with certain program owners:

```
    "accounts_selector" : {
         "owners" : ["pubkey-owner-1", "pubkey-owner-2", ..., "pubkey-owner-m"],
    }
```

To select all accounts, use wild card:

```
    "accounts_selector" : {
         "accounts" : ["*"],
    }
```


## Database Setup

### Install PostgreSQL Server

Please follow [PostgreSQL Ubuntu Installation](https://www.postgresql.org/download/linux/ubuntu/) on instructions to install the PostgreSQL database server. For example, for postgresql-14,

```
sudo sh -c 'echo "deb http://apt.postgresql.org/pub/repos/apt $(lsb_release -cs)-pgdg main" > /etc/apt/sources.list.d/pgdg.list'
wget --quiet -O - https://www.postgresql.org/media/keys/ACCC4CF8.asc | sudo apt-key add -
sudo apt-get update
sudo apt-get -y install postgresql-14
```
### Control the Database Access

Modify the pg_hba.conf as necessary to grant the plugin to access the database. For example, in /etc/postgresql/14/main/pg_hba.conf, the following entry allow nodes with IP with CIDR 10.138.0.0/24 to access all databases. The validator is running in on node with the ip in the specified range.

```
host    all             all             10.138.0.0/24           trust
```

It is recommended to run the database server on a separate node from the validator for better performance.

### Configure the Performance Parameters

Please refer to [PostgreSQL Server Configuration](https://www.postgresql.org/docs/14/runtime-config.html) for configuration details. The referential implementation use the following configurations for better database performance in the /etc/postgresql/14/main/postgresql.conf.

```
max_connections = 200                  # (change requires restart)
shared_buffers = 1GB                   # min 128kB
effective_io_concurrency = 1000        # 1-1000; 0 disables prefetching
wal_level = minimal                    # minimal, replica, or logical
fsync = off                            # flush data to disk for crash safety
synchronous_commit = off               # synchronization level;
full_page_writes = off                 # recover from partial page writes
max_wal_senders = 0                    # max number of walsender processes
```

### Create the Database Instance and the Role

Start the server:

```
sudo systemctl start postgresql@14-main
```

Create the database, for example the following create a database named 'solana',

```
sudo -u postgres createdb solana -p 5433
```

Create the database user, for example, the following create a regular user named 'solana',

```
sudo -u postgres createuser -p 5433 solana
```

Verify the database is working using psql, for example, assuming the node running PostgreSQL has ip 10.138.0.9, the following will land in a SQL shell where SQL commands can be entered:

```
psql -U solana -p 5433 -h 10.138.0.9 -w -d solana
```

### Create the Schema Objects

Use the [create_schema.sql](https://github.com/solana-labs/solana/blob/7ac43b16d2c766df61ae0a06d7aaf14ba61996ac/accountsdb-plugin-postgres/scripts/create_schema.sql) to create the objects for storing accounts and slots.

Download the script from github:

```
wget https://raw.githubusercontent.com/solana-labs/solana/7ac43b16d2c766df61ae0a06d7aaf14ba61996ac/accountsdb-plugin-postgres/scripts/create_schema.sql
```

Then run the script:

```
psql -U solana -p 5433 -h 10.138.0.9 -w -d solana -f create_schema.sql
```
