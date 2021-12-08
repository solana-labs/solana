---
title: AccountsDb Plugins
---

Overview
========

It has been observed that validators under heavy RPC loads such as when serving getProgramAccounts calls can fall behind the network. To solve this problem, the Solana validator has been enhanced to support a plugin mechanism through which the information about accounts and slots can be transmitted to external data stores such as relational databases, NoSQL databases or Kafka. Then RPC services can be developed to consume data from these external data stores with the possibility of more flexible and targeted optimizations such as caching and indexing. This will leave the validator more focused on processing transactions instead of being slowed down with handling RPC requests.

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

The Plugin interface is declared in [`solana-accountsdb-plugin-interface`]. It is defined by the trait `AccountsDbPlugin`. The plugin should implement the trait and expose a "C" function _create_plugin to return the pointer to this trait. For example, in the referential implementation. The following code instantiate the Postgres plugin `AccountsDbPluginPostgres ` and return its pointer.

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



