## solana-wallet CLI

The [solana crate](https://crates.io/crates/solana) is distributed with a command-line interface tool

### Examples

#### Get Pubkey

```sh
// Command
$ solana-wallet address

// Return
<PUBKEY>
```

#### Airdrop Tokens

```sh
// Command
$ solana-wallet airdrop 123

// Return
"Your balance is: 123"
```

#### Get Balance

```sh
// Command
$ solana-wallet balance

// Return
"Your balance is: 123"
```

#### Confirm Transaction

```sh
// Command
$ solana-wallet confirm <TX_SIGNATURE>

// Return
"Confirmed" / "Not found"
```

#### Deploy program

```sh
// Command
$ solana-wallet deploy <PATH>

// Return
<PROGRAM_ID>
```

#### Unconditional Immediate Transfer

```sh
// Command
$ solana-wallet pay <PUBKEY> 123

// Return
<TX_SIGNATURE>
```

#### Post-Dated Transfer

```sh
// Command
$ solana-wallet pay <PUBKEY> 123 \
    --after 2018-12-24T23:59:00 --require-timestamp-from <PUBKEY>

// Return
{signature: <TX_SIGNATURE>, processId: <PROCESS_ID>}
```
*`require-timestamp-from` is optional. If not provided, the transaction will expect a timestamp signed by this wallet's secret key*

#### Authorized Transfer

A third party must send a signature to unlock the tokens.
```sh
// Command
$ solana-wallet pay <PUBKEY> 123 \
    --require-signature-from <PUBKEY>

// Return
{signature: <TX_SIGNATURE>, processId: <PROCESS_ID>}
```

#### Post-Dated and Authorized Transfer

```sh
// Command
$ solana-wallet pay <PUBKEY> 123 \
    --after 2018-12-24T23:59 --require-timestamp-from <PUBKEY> \
    --require-signature-from <PUBKEY>

// Return
{signature: <TX_SIGNATURE>, processId: <PROCESS_ID>}
```

#### Multiple Witnesses

```sh
// Command
$ solana-wallet pay <PUBKEY> 123 \
    --require-signature-from <PUBKEY> \
    --require-signature-from <PUBKEY>

// Return
{signature: <TX_SIGNATURE>, processId: <PROCESS_ID>}
```

#### Cancelable Transfer

```sh
// Command
$ solana-wallet pay <PUBKEY> 123 \
    --require-signature-from <PUBKEY> \
    --cancelable

// Return
{signature: <TX_SIGNATURE>, processId: <PROCESS_ID>}
```

#### Cancel Transfer

```sh
// Command
$ solana-wallet cancel <PROCESS_ID>

// Return
<TX_SIGNATURE>
```

#### Send Signature

```sh
// Command
$ solana-wallet send-signature <PUBKEY> <PROCESS_ID>

// Return
<TX_SIGNATURE>
```

#### Indicate Elapsed Time

Use the current system time:
```sh
// Command
$ solana-wallet send-timestamp <PUBKEY> <PROCESS_ID>

// Return
<TX_SIGNATURE>
```

Or specify some other arbitrary timestamp:

```sh
// Command
$ solana-wallet send-timestamp <PUBKEY> <PROCESS_ID> --date 2018-12-24T23:59:00

// Return
<TX_SIGNATURE>
```

### Usage

```
solana-wallet 0.11.0

USAGE:
    solana-wallet [OPTIONS] [SUBCOMMAND]

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -k, --keypair <PATH>         /path/to/id.json
    -n, --network <HOST:PORT>    Rendezvous with the network at this gossip entry point; defaults to 127.0.0.1:8001
        --proxy <URL>            Address of TLS proxy
        --port <NUM>             Optional rpc-port configuration to connect to non-default nodes
        --timeout <SECS>         Max seconds to wait to get necessary gossip from the network

SUBCOMMANDS:
    address                  Get your public key
    airdrop                  Request a batch of tokens
    balance                  Get your balance
    cancel                   Cancel a transfer
    confirm                  Confirm transaction by signature
    deploy                   Deploy a program
    get-transaction-count    Get current transaction count
    help                     Prints this message or the help of the given subcommand(s)
    pay                      Send a payment
    send-signature           Send a signature to authorize a transfer
    send-timestamp           Send a timestamp to unlock a transfer
```

```
solana-wallet-address 
Get your public key

USAGE:
    solana-wallet address

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information
```

```
solana-wallet-airdrop 
Request a batch of tokens

USAGE:
    solana-wallet airdrop <NUM>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

ARGS:
    <NUM>    The number of tokens to request
```

```
solana-wallet-balance 
Get your balance

USAGE:
    solana-wallet balance

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information
```

```
solana-wallet-cancel 
Cancel a transfer

USAGE:
    solana-wallet cancel <PROCESS_ID>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

ARGS:
    <PROCESS_ID>    The process id of the transfer to cancel
```

```
solana-wallet-confirm 
Confirm transaction by signature

USAGE:
    solana-wallet confirm <SIGNATURE>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

ARGS:
    <SIGNATURE>    The transaction signature to confirm
```

```
solana-wallet-deploy 
Deploy a program

USAGE:
    solana-wallet deploy <PATH>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

ARGS:
    <PATH>    /path/to/program.o
```

```
solana-wallet-get-transaction-count 
Get current transaction count

USAGE:
    solana-wallet get-transaction-count

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information
```

```
solana-wallet-pay 
Send a payment

USAGE:
    solana-wallet pay [FLAGS] [OPTIONS] <PUBKEY> <NUM>

FLAGS:
        --cancelable    
    -h, --help          Prints help information
    -V, --version       Prints version information

OPTIONS:
        --after <DATETIME>                      A timestamp after which transaction will execute
        --require-timestamp-from <PUBKEY>       Require timestamp from this third party
        --require-signature-from <PUBKEY>...    Any third party signatures required to unlock the tokens

ARGS:
    <PUBKEY>    The pubkey of recipient
    <NUM>       The number of tokens to send
```

```
solana-wallet-send-signature 
Send a signature to authorize a transfer

USAGE:
    solana-wallet send-signature <PUBKEY> <PROCESS_ID>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

ARGS:
    <PUBKEY>        The pubkey of recipient
    <PROCESS_ID>    The process id of the transfer to authorize
```

```
solana-wallet-send-timestamp 
Send a timestamp to unlock a transfer

USAGE:
    solana-wallet send-timestamp [OPTIONS] <PUBKEY> <PROCESS_ID>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
        --date <DATETIME>    Optional arbitrary timestamp to apply

ARGS:
    <PUBKEY>        The pubkey of recipient
    <PROCESS_ID>    The process id of the transfer to unlock
```

