# Economic Design MVP

**Subject to change.**

The preceding sections, outlined in the [Economic Design Overview](../README.md), describe a long-term vision of a sustainable Solana economy. Of course, we don't expect the final implementation to perfectly match what has been described above. We intend to fully engage with network stakeholders throughout the implementation phases \(i.e. pre-testnet, testnet, mainnet\) to ensure the system supports, and is representative of, the various network participants' interests. The first step toward this goal, however, is outlining a some desired MVP economic features to be available for early pre-testnet and testnet participants. Below is a rough sketch outlining basic economic functionality from which a more complete and functional system can be developed.

## MVP Economic Features

* Faucet to deliver testnet SOLs to validators for staking and application development.
* Mechanism by which validators are rewarded via network inflation.
* Ability to delegate tokens to validator nodes
* Validator set commission fees on interest from delegated tokens.
* Archivers to receive fixed, arbitrary reward for submitting validated PoReps. Reward size mechanism \(i.e. PoRep reward as a function of total ledger redundancy\) to come later.
* Pooling of archiver PoRep transaction fees and weighted distribution to validators based on PoRep verification \(see [Replication-validation Transaction Fees](ed_validation_client_economics/ed_vce_replication_validation_transaction_fees.md). It will be useful to test this protection against attacks on testnet.
* Nice-to-have: auto-delegation of archiver rewards to validator.

