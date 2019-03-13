## Proposed MVP of Economic Design

The preceeding sections outlined in the [Economic Design Overview](ed_overview.md), describe a long-term vision of a sustainable Solana economy. Of course, we don't expect the final implementation to perfectly match what has been described above. We intend to fully engage with network stakeholders throughout the implementation phases (i.e. pre-testnet, testnet, mainnet) to ensure the system supports and is representative of the various network participants interests. The first step toward this goal, however, is outlining a some desired MVP economic features to be available for early pre-testnet and testnet participants. Below is a rough sketch outlining basic economic functionality from which a more complete and functional system can be developed.

### MVP Economic Features

* Faucet to deliver testnet SOLs to validators for staking and dapp development.
* Validators are rewarded in proportion to their stake. Interest rate mechansism (i.e. to be determined by total % staked) to come later.
* Per epoch, rewards are distributed in proportion to the stake and the proportion of normalized votes
* Replicators to receive fixed, arbitrary reward for submitting validated PoReps. Reward size mechanism (i.e. PoRep reward as a function of total ledger redundancy) to come later.
* Pooling of replicator PoRep transaction fees and weighted distribution to validators based on PoRep verification (see [Replication-validation Transaction Fees](ed_vce_replication_validation_transaction_fees.md). It will be useful to test this protection against attacks on testnet.
