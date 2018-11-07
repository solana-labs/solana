# Introduction to VDFs

A Verifiable Delay Function is conceptually a water clock where its water marks
can be recorded and later verified that the water most certainly passed
through.  Anatoly describes the water clock analogy in detail here:

[water clock analogy](https://medium.com/solana-labs/proof-of-history-explained-by-a-water-clock-e682183417b8)

The same technique has been used in Bitcoin since day one. The Bitcoin feature
is called nLocktime and it can be used to postdate transactions using block
height instead of a timestamp. As a Bitcoin client, you'd use block height
instead of a timestamp if you don't trust the network. Block height turns out
to be an instance of what's being called a Verifiable Delay Function in
cryptography circles. It's a cryptographically secure way to say time has
passed. In Solana, we use a far more granular verifiable delay function, a SHA
256 hash chain, to checkpoint the ledger and coordinate consensus. With it, we
implement Optimistic Concurrency Control and are now well en route towards that
theoretical limit of 710,000 transactions per second.
