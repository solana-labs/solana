
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
