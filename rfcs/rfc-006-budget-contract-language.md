
### Transfer (aka, Budget v2) Transaction Format
```rust
pub enum TransferCondition {
    /// Wait for a `Timestamp` `Instruction` at or after the given `DateTime` from `Pubkey`
    Timestamp(DateTime<Utc>, Pubkey),

    /// Wait for a `Witness` `Instruction` from `Pubkey`
    Witness(Pubkey),
}

pub struct Transfer {
    /// Amount to be transferred
    tokens: i64,

    /// `Pubkey` that the `Cancel` `Instruction` may be issued from prior to the
    // `Transfer` meeting all `conditions`
    cancellable: Maybe<Pubkey>,

    /// The list of conditions that must be met before the `Transfer` is unlocked
    conditions: Vec<TransferCondition>,
}

/// The `Transaction` `userdata` contain a serialized `TransferInstruction`
///
/// * key[0] - the `caller` key
/// * key[1] - the contract key
///
pub enum TransferInstruction {
    /// Declare and instantiate a new `Transfer` into `key[1]` using tokens from `key[0]`.
    ///
    /// `key[1]` must be unallocated before calling `Create`
    Create(Transfer),

    /// Cancel the `Transfer` identified by `key[1]`.  Fails if `key[0]` is not in `Transfer`
    /// `cancellable`, or if the `Transfer` is already unlocked.   The `tokens` will be returned to
    /// `key[0]`.
    Cancel,

    /// Tell a `Transfer` to acknowledge the given `DateTime` has past from `key[0]`.
    Timestamp(DateTime<Utc>),

    /// Tell a `Transfer` to acknowledge that `key[0]` has witnessed the transaction.
    Witness,
}
```

### Wallet CLI

The general form is:
```
$ solana-wallet transfer [common-options] [subcommand] [subcommand-specific options]
```

`common-options` include:
* `--fee xyz` - Transaction fee (0 by default)
* `--output file` - Write the raw Transaction to a file instead of sending it

#### Unconditional Immediate Transfer
```sh
$ solana-wallet transfer create PubKey 123
```

#### Time-based Transfer
```sh
$ solana-wallet transfer create TransferPubKey 123 \
    at 2018-12-24T23:59 from TimestampPubKey
```

#### Witness-based Transfer
A third party must issue a "witness" transaction to unlock the tokens.
```sh
$ solana-wallet transfer create TransferPubKey 123 \
    witness WitnessPubKey
```

#### Time and Witness Transfer
```sh
$ solana-wallet transfer create TransferPubKey 123 \
    at 2018-12-24T23:59 from TimestampPubKey \
    witness WitnessPubKey
```

#### Multiple Witnesses
```sh
$ solana-wallet transfer create TransferPubKey 123 \
    witness Witness1PubKey \
    witness Witness2PubKey
```

#### Cancellable Witnesses
```sh
$ solana-wallet transfer create TransferPubKey 123 \
    witness WitnessPubKey \
    cancellable \
```

#### Cancel Transfer
```sh
$ solana-wallet transfer cancel TransferPubKey
```

#### Witness Transfer
```sh
$ solana-wallet transfer witness TransferPubKey
```

#### Indicate Elapsed Time.
```sh
$ solana-wallet transfer timestamp TransferPubKey 2018-12-24T23:59
```


## Javascript solana-web3.js Interface

*TBD, but will look similar to what the Wallet CLI offers wrapped up in a
Javacsript object*
