## solana CLI

The [solana-cli crate](https://crates.io/crates/solana-cli) provides a command-line interface tool for Solana

### Examples

#### Get Pubkey

```sh
// Command
$ solana address

// Return
<PUBKEY>
```

#### Airdrop Lamports

```sh
// Command
$ solana airdrop 123

// Return
"Your balance is: 123"
```

#### Get Balance

```sh
// Command
$ solana balance

// Return
"Your balance is: 123"
```

#### Confirm Transaction

```sh
// Command
$ solana confirm <TX_SIGNATURE>

// Return
"Confirmed" / "Not found" / "Transaction failed with error <ERR>"
```

#### Deploy program

```sh
// Command
$ solana deploy <PATH>

// Return
<PROGRAM_ID>
```

#### Unconditional Immediate Transfer

```sh
// Command
$ solana pay <PUBKEY> 123

// Return
<TX_SIGNATURE>
```

#### Post-Dated Transfer

```sh
// Command
$ solana pay <PUBKEY> 123 \
    --after 2018-12-24T23:59:00 --require-timestamp-from <PUBKEY>

// Return
{signature: <TX_SIGNATURE>, processId: <PROCESS_ID>}
```
*`require-timestamp-from` is optional. If not provided, the transaction will expect a timestamp signed by this wallet's secret key*

#### Authorized Transfer

A third party must send a signature to unlock the lamports.
```sh
// Command
$ solana pay <PUBKEY> 123 \
    --require-signature-from <PUBKEY>

// Return
{signature: <TX_SIGNATURE>, processId: <PROCESS_ID>}
```

#### Post-Dated and Authorized Transfer

```sh
// Command
$ solana pay <PUBKEY> 123 \
    --after 2018-12-24T23:59 --require-timestamp-from <PUBKEY> \
    --require-signature-from <PUBKEY>

// Return
{signature: <TX_SIGNATURE>, processId: <PROCESS_ID>}
```

#### Multiple Witnesses

```sh
// Command
$ solana pay <PUBKEY> 123 \
    --require-signature-from <PUBKEY> \
    --require-signature-from <PUBKEY>

// Return
{signature: <TX_SIGNATURE>, processId: <PROCESS_ID>}
```

#### Cancelable Transfer

```sh
// Command
$ solana pay <PUBKEY> 123 \
    --require-signature-from <PUBKEY> \
    --cancelable

// Return
{signature: <TX_SIGNATURE>, processId: <PROCESS_ID>}
```

#### Cancel Transfer

```sh
// Command
$ solana cancel <PROCESS_ID>

// Return
<TX_SIGNATURE>
```

#### Send Signature

```sh
// Command
$ solana send-signature <PUBKEY> <PROCESS_ID>

// Return
<TX_SIGNATURE>
```

#### Indicate Elapsed Time

Use the current system time:
```sh
// Command
$ solana send-timestamp <PUBKEY> <PROCESS_ID>

// Return
<TX_SIGNATURE>
```

Or specify some other arbitrary timestamp:

```sh
// Command
$ solana send-timestamp <PUBKEY> <PROCESS_ID> --date 2018-12-24T23:59:00

// Return
<TX_SIGNATURE>
```

### Usage

```manpage
solana 0.12.0

USAGE:
    solana [FLAGS] [OPTIONS] [SUBCOMMAND]

FLAGS:
    -h, --help       Prints help information
        --rpc-tls    Enable TLS for the RPC endpoint
    -V, --version    Prints version information

OPTIONS:
        --drone-host <IP ADDRESS>    Drone host to use [default: same as --host]
        --drone-port <PORT>          Drone port to use [default: 9900]
    -n, --host <IP ADDRESS>          Host to use for both RPC and drone [default: 127.0.0.1]
    -k, --keypair <PATH>             /path/to/id.json
        --rpc-host <IP ADDRESS>      RPC host to use [default: same as --host]
        --rpc-port <PORT>            RPC port to use [default: 8899]

SUBCOMMANDS:
    address                  Get your public key
    airdrop                  Request a batch of lamports
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

```manpage
solana-address
Get your public key

USAGE:
    solana address

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information
```

```manpage
solana-airdrop
Request a batch of lamports

USAGE:
    solana airdrop <NUM>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

ARGS:
    <NUM>    The number of lamports to request
```

```manpage
solana-balance
Get your balance

USAGE:
    solana balance

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information
```

```manpage
solana-cancel
Cancel a transfer

USAGE:
    solana cancel <PROCESS_ID>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

ARGS:
    <PROCESS_ID>    The process id of the transfer to cancel
```

```manpage
solana-confirm
Confirm transaction by signature

USAGE:
    solana confirm <SIGNATURE>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

ARGS:
    <SIGNATURE>    The transaction signature to confirm
```

```manpage
solana-deploy
Deploy a program

USAGE:
    solana deploy <PATH>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

ARGS:
    <PATH>    /path/to/program.o
```

```manpage
solana-fees
Display current cluster fees

USAGE:
    solana fees

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information
```

```manpage
solana-get-transaction-count
Get current transaction count

USAGE:
    solana get-transaction-count

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information
```

```manpage
solana-pay
Send a payment

USAGE:
    solana pay [FLAGS] [OPTIONS] <PUBKEY> <NUM>

FLAGS:
        --cancelable
    -h, --help          Prints help information
    -V, --version       Prints version information

OPTIONS:
        --after <DATETIME>                      A timestamp after which transaction will execute
        --require-timestamp-from <PUBKEY>       Require timestamp from this third party
        --require-signature-from <PUBKEY>...    Any third party signatures required to unlock the lamports

ARGS:
    <PUBKEY>    The pubkey of recipient
    <NUM>       The number of lamports to send
```

```manpage
solana-send-signature
Send a signature to authorize a transfer

USAGE:
    solana send-signature <PUBKEY> <PROCESS_ID>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

ARGS:
    <PUBKEY>        The pubkey of recipient
    <PROCESS_ID>    The process id of the transfer to authorize
```

```manpage
solana-send-timestamp
Send a timestamp to unlock a transfer

USAGE:
    solana send-timestamp [OPTIONS] <PUBKEY> <PROCESS_ID>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
        --date <DATETIME>    Optional arbitrary timestamp to apply

ARGS:
    <PUBKEY>        The pubkey of recipient
    <PROCESS_ID>    The process id of the transfer to unlock
```
