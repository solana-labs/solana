---
title: Rust API
---

Solana's Rust crates are [published to crates.io][crates.io] and can be found
[on docs.rs with the "solana-" prefix][docs.rs].

[crates.io]: https://crates.io/search?q=solana-
[docs.rs]: https://docs.rs/releases/search?query=solana-

Some important crates:

- [`solana-program`] &mdash; Imported by programs running on Solana, compiled
  to SBF. This crate contains many fundamental data types and is re-exported from
  [`solana-sdk`], which cannot be imported from a Solana program.

- [`solana-sdk`] &mdash; The basic off-chain SDK, it re-exports
  [`solana-program`] and adds more APIs on top of that. Most Solana programs
  that do not run on-chain will import this.

- [`solana-client`] &mdash; For interacting with a Solana node via the
  [JSON RPC API](/api).

- [`solana-cli-config`] &mdash; Loading and saving the Solana CLI configuration
  file.

- [`solana-clap-utils`] &mdash; Routines for setting up a CLI, using [`clap`],
  as used by the main Solana CLI. Includes functions for loading all types of
  signers supported by the CLI.

[`solana-program`]: https://docs.rs/solana-program
[`solana-sdk`]: https://docs.rs/solana-sdk
[`solana-client`]: https://docs.rs/solana-client
[`solana-cli-config`]: https://docs.rs/solana-cli-config
[`solana-clap-utils`]: https://docs.rs/solana-clap-utils
[`clap`]: https://docs.rs/clap
