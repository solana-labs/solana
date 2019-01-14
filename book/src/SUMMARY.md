# Solana Architecture

- [Introduction](introduction.md)

- [Terminology](terminology.md)

- [Getting Started](getting-started.md)
  - [Example: Web Wallet](webwallet.md)

- [Programming Model](programs.md)
  - [Example: Tic-Tac-Toe](tictactoe.md)
  - [Drones](drones.md)

- [A Solana Cluster](cluster.md)
  - [Synchronization](synchronization.md)
  - [Leader Rotation](leader-rotation.md)
  - [Fork Generation](fork-generation.md)

- [Anatomy of a Fullnode](fullnode.md)
  - [TPU](tpu.md)
  - [TVU](tvu.md)
  - [Gossip Service](gossip.md)
  - [The Runtime](runtime.md)

- [Proposed Architectural Changes](proposals.md)
  - [Ledger Replication](ledger-replication.md)
  - [Secure Enclave](enclave.md)
  - [Staking Rewards](staking-rewards.md)
  - [Fork Selection](fork-selection.md)
  - [Entry Tree](entry-tree.md)
  - [Data Plane Fanout](data-plane-fanout.md)

- [Economic Design](ed_overview.md)
  - [Validation-client Economics](ed_validation_client_economics.md)
	- [State-validation Protocol-based Rewards](ed_vce_state_validation_protocol_based_rewards.md)
	- [State-validation Transaction Fees](ed_vce_state_validation_transaction_fees.md)
	- [Replication-validation Transaction Fees](ed_vce_replication_validation_transaction_fees.md)
	- [Validation Stake Delegation](ed_vce_validation_stake_delegation.md)
  - [Replication-client Economics](ed_replication_client_economics.md)
	- [Storage-replication Rewards](ed_rce_storage_replication_rewards.md)
	- [Replication-client Reward Auto-delegation](ed_rce_replication_client_reward_auto_delegation.md)
  - [Economic Sustainability](ed_economic_sustainability.md)
  - [Attack Vectors](ed_attack_vectors.md)
  - [References](ed_references.md)

## Appendix

- [Appendix](appendix.md)
  - [JSON RPC API](jsonrpc-api.md)
  - [JavaScript API](javascript-api.md)
  - [solana-wallet CLI](wallet.md)
