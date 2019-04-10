# Hashed Time-Locked Contract Transactions

A Hashed Time-Locked Contract is a transaction that allows one party of a
transaction to spend tokens by forced-disclosure of the preimage of a message
digest, and allows the counterparty to spend the tokens after a predefined
lockout period.

Another way to look at HTLC is that it is a protocol to eliminate [counterparty
risk] where one side of a trade could default on their obligation.

The key requirements for HTLC is that a ledger supports time-locked transactions,
meaning a transaction that won't be executed until at a later point in time. And
conditional transactions with message digest instructions.

## Implementation

There are a couple of ways to implement HTLC in Solana. We can implement HTLC
as a transaction type or as a set of instructions akin to `OP_CHECKSEQUENCEVERIFY`
and `OP_SHA256` in bitcoin [Script].

### New Instructions

In order to be compatible with [BIP-199] and [ERC-1630] we have to support 2 message
digest algorithms, SHA-256 and Keccak-256 (SHA-3). I recommend that we also implement
Blake2 and Shake message digest algorithms.

We could implement these two instructions in Solana as [system instructions] as follows:

```rust
pub enum SystemInstruction {
  /// Evaluates to true if period_in_seconds becomes zero.
  /// Counter doesn't start until transaction is confirmed.
  CheckElapsed { period_in_seconds: u64 },

  /// SHA-256 hashing instruction.
  /// Evaluates to the SHA-256 message digest.
  SHA256 { data: &[u8] }
}

pub fn check_elapsed(account_id: &PubKey, period: u64) -> Instruction {
  // Code here to store period variable in account data and decrement it
  // by the amount of time passed since the last run.
}

pub fn hash_sha_256(data: &[u8]) -> Instruction {
  // Code here to return the message digest of data.
}
```

### New Transaction

The [budget program] already implements time-locking capability but it is missing
arbitrary conditionals including message digest verification. If we were to implement
HTLC in the [budget program] we'll need to extend the [Condition enum] to include
message digest verification as follows:

```rust
pub enum Condition {
  /// Evaluate whether a message digest of some data matches a particular hash.
  MDVerify { data: &[u8], hash: &[u8] },

  // ... other possible conditional types.
}
```

And update the `is_satisfied` method to carry out the digest verification.

[Script]: https://en.bitcoin.it/wiki/Script
[BIP-199]: https://github.com/bitcoin/bips/blob/master/bip-0199.mediawiki
[ERC-1630]: https://github.com/ethereum/EIPs/issues/1631
[budget program]: https://github.com/solana-labs/solana/tree/7da4142d338c61589c097d8edc52ff39d45060b9/programs/budget_api
[system instructions]: https://github.com/solana-labs/solana/tree/7da4142d338c61589c097d8edc52ff39d45060b9/programs/budget_api
[counterparty risk]: https://www.investopedia.com/terms/c/counterpartyrisk.asp
[Condition enum]: https://github.com/solana-labs/solana/blob/7da4142d338c61589c097d8edc52ff39d45060b9/programs/budget_api/src/budget_expr.rs#L31
