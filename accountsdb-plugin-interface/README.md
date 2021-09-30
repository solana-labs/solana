<p align="center">
  <a href="https://solana.com">
    <img alt="Solana" src="https://i.imgur.com/IKyzQ6T.png" width="250" />
  </a>
</p>

# Solana AccountsDb Plugin Interface

This crate enables an AccountsDb plugin to be plugged into the Solana Validator runtime to take actions
at the time of each account update; for example, saving the account state to an external database. The plugin must implement the `AccountsDbPlugin` trait. Please see the detail of the `accountsdb_plugin_interface.rs` for the interface definition.

The plugin should produce a `cdylib` dynamic library, which must expose a `C` function `_create_plugin()` that
instantiates the implementation of the interface.

The `solana-accountsdb-plugin-postgres` crate provides an example of how to create a plugin which saves the accounts data into an
external PostgreSQL databases.

More information about Solana is available in the [Solana documentation](https://docs.solana.com/).

Still have questions?  Ask us on [Discord](https://discordapp.com/invite/pquxPsq)
