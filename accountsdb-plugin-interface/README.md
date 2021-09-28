<p align="center">
  <a href="https://solana.com">
    <img alt="Solana" src="https://i.imgur.com/IKyzQ6T.png" width="250" />
  </a>
</p>

# Solana AccountsDb Plugin Interface

This crate enables AccountsDb plugin to be developed and plugged into the Solana Validator runtime. This enables the plugin to take actions
at the time of the update, for example saving the information to the external database. The plugin shall implement the `AccountsDbPlugin` trait. Please see the detail of the `accountsdb_plugin_interface.rs` on the definition of the interface. 

The plugin should produce a `cdylib` dynamical library. And the dynamic library must expose a `C` function `_create_plugin` which
instantiates the implementation of the interface.

The `solana-accountsdb-plugin-postgres` crate provides an exmaple on how to create a plugin which saves the accounts data into the
external PostgreSQL databases.

More information about Solana is available in the [Solana documentation](https://docs.solana.com/).

Still have questions?  Ask us on [Discord](https://discordapp.com/invite/pquxPsq)
