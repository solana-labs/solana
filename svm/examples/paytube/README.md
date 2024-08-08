# PayTube

A reference implementation of an off-chain [state channel](https://ethereum.org/en/developers/docs/scaling/state-channels/)
built using [Anza's SVM API](https://www.anza.xyz/blog/anzas-new-svm-api).

With the release of Agave 2.0, we've decoupled the SVM API from the rest of the
runtime, which means it can be used outside the validator. This unlocks
SVM-based solutions such as sidecars, channels, rollups, and more. This project
demonstrates everything you need to know about boostrapping with this new API.

PayTube is a state channel (more specifically a payment channel), designed to
allow multiple parties to transact amongst each other in SOL or SPL tokens
off-chain. When the channel is closed, the resulting changes in each user's
balances are posted to the base chain (Solana).

Although this project is for demonstration purposes, a payment channel similar
to PayTube could be created that scales to handle massive bandwidth of
transfers, saving the overhead of posting transactions to the chain for last.
