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

## Atomic Swaps

One use case of HTLC is atomic swaps. It solves the problem of (at least) two
parties wanting to exchange their DLT tokens or coins without having to trust
a third party or a centralized exchange.

A solution to this problem was [first suggested by Tier Nolan on the Bitcoin forum]
where he described a risk-free within a time bound interactive protocol to exchange
tokens across different DLTs.

The protocol goes as follows:

```ignore
A picks a random number x

 A creates TX1: "Pay w BTC to <B's public key> if (x for H(x) known and signed by B) or (signed by A & B)"

 A creates TX2: "Pay w BTC from TX1 to <A's public key>, locked 48 hours in the future, signed by A"

 A sends TX2 to B

 B signs TX2 and returns to A

 1) A submits TX1 to the network

 B creates TX3: "Pay v alt-coins to <A-public-key> if (x for H(x) known and signed by A) or (signed by A & B)"

 B creates TX4: "Pay v alt-coins from TX3 to <B's public key>, locked 24 hours in the future, signed by B"

 B sends TX4 to A

 A signs TX4 and sends back to B

 2) B submits TX3 to the network

 3) A spends TX3, revealing x

 4) B spends TX1 using x

 This is atomic (with timeout).  If the process is halted, it can be reversed no matter when it is stopped.

 Before 1: Nothing public has been broadcast, so nothing happens
 Between 1 & 2: A can use refund transaction after 48 hours to get his money back
 Between 2 & 3: B can get refund after 24 hours.  A has 24 more hours to get his refund
 After 3: Transaction is completed by 2
 - A must spend his new coin within 24 hours or B can claim the refund and keep his coins
 - B must spend his new coin within 48 hours or A can claim the refund and keep his coins

 For safety, both should complete the process with lots of time until the deadlines.
```

## Implementation

There are a couple of ways to implement HTLC in Solana. We can implement HTLC
as a transaction type or as a set of instructions akin to `OP_CHECKSEQUENCEVERIFY`
and `OP_SHA256` in bitcoin [Script].

### New Instructions

In order to be compatible with many other DLTs HTLC implementations we have to
support at least 2 message digest algorithms, SHA-256 and Keccak-256 (SHA-3).
It is recommended to also implement Blake2 and Shake algorithms.

The [budget program] currently lacks time-locking capability and message digest
verification. If we were to implement HTLC in the [budget program] we'll need to
extend the [Condition enum] to include the message digest verification and
time-locking as follows:

```rust,ignore
pub enum Condition {
  /// Evaluate whether a message digest of some data matches a particular hash.
  MDVerify { data: &[u8], hash: &[u8] },

  /// Evaluate whether a condition has expired.
  HasExpired { expiry_date: DateTime<Utc> },

  // ... other possible conditional types.
}
```

And update the `is_satisfied` method to carry out the digest verification and check
whether the payment plan has expired.

[Script]: https://en.bitcoin.it/wiki/Script
[BIP-199]: https://github.com/bitcoin/bips/blob/master/bip-0199.mediawiki
[ERC-1630]: https://github.com/ethereum/EIPs/issues/1631
[budget program]: https://github.com/solana-labs/solana/tree/7da4142d338c61589c097d8edc52ff39d45060b9/programs/budget_api
[system instructions]: https://github.com/solana-labs/solana/tree/7da4142d338c61589c097d8edc52ff39d45060b9/programs/budget_api
[counterparty risk]: https://www.investopedia.com/terms/c/counterpartyrisk.asp
[Condition enum]: https://github.com/solana-labs/solana/blob/7da4142d338c61589c097d8edc52ff39d45060b9/programs/budget_api/src/budget_expr.rs#L31
[first suggested by Tier Nolan on the Bitcoin forum]: https://bitcointalk.org/index.php?topic=193281.msg2224949#msg2224949
