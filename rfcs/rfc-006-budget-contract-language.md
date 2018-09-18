
### Wallet CLI

The general form is:
```
$ solana-wallet [common-options] [command] [command-specific options]
```
`common-options` include:
* `--fee xyz` - Transaction fee (0 by default)
* `--output file` - Write the raw Transaction to a file instead of sending it

`command` variants:
* `pay`
* `cancel`
* `apply`

#### Unconditional Immediate Transfer
```sh
// Command
$ solana-wallet pay RecipientPubkey 123

// Return
TxSignature
```

#### Time-based Transfer
```sh
// Command
$ solana-wallet pay RecipientPubkey 123 \
    --after 2018-12-24T23:59 --from TimestampPubKey

// Return
{signature: TxSignature, contractId: ContractId}
```

#### Witness-based Transfer
A third party must issue a "witness" transaction to unlock the tokens.
```sh
// Command
$ solana-wallet pay RecipientPubkey 123 \
    --witness WitnessPubKey

// Return
{signature: TxSignature, contractId: ContractId}
```

#### Time and Witness Transfer
```sh
// Command
$ solana-wallet pay RecipientPubkey 123 \
    --after 2018-12-24T23:59 --from TimestampPubKey \
    --witness WitnessPubKey

// Return
{signature: TxSignature, contractId: ContractId}
```

#### Multiple Witnesses
```sh
// Command
$ solana-wallet pay RecipientPubkey 123 \
    --witness Witness1PubKey \
    --witness Witness2PubKey

// Return
{signature: TxSignature, contractId: ContractId}
```

#### Cancellable Witnesses
```sh
// Command
$ solana-wallet pay RecipientPubkey 123 \
    --witness WitnessPubKey \
    --cancellable \

// Return
{signature: TxSignature, contractId: ContractId}
```

#### Cancel Transfer
```sh
// Command
$ solana-wallet cancel ContractId

// Return
TxSignature
```

#### Witness Transfer
```sh
// Command
$ solana-wallet apply ContractId --witness

// Return
TxSignature
```

#### Indicate Elapsed Time

Use the current system time:
```sh
// Command
$ solana-wallet apply ContractId --timestamp

// Return
TxSignature
```

Or specify some other arbitrary timestamp:
```sh
// Command
$ solana-wallet apply ContractId --timestamp 2018-12-24T23:59

// Return
TxSignature
```


## Javascript solana-web3.js Interface

*TBD, but will look similar to what the Wallet CLI offers wrapped up in a
Javacsript object*
