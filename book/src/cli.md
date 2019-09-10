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

#### Airdrop SOL/Lamports

```sh
// Command
$ solana airdrop 2

// Return
"2.00000000 SOL"

// Command
$ solana airdrop 123 --lamports

// Return
"123 lamports"
```

#### Get Balance

```sh
// Command
$ solana balance

// Return
"3.00050001 SOL"
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
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/wallet/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json

SUBCOMMANDS:
    address                               Get your public key
    airdrop                               Request lamports
    authorize-voter                       Authorize a new vote signing keypair for the given vote account
    balance                               Get your balance
    cancel                                Cancel a transfer
    claim-storage-reward                  Redeem storage reward credits
    cluster-version                       Get the version of the cluster entrypoint
    confirm                               Confirm transaction by signature
    create-replicator-storage-account     Create a replicator storage account
    create-storage-mining-pool-account    Create mining pool account
    create-validator-storage-account      Create a validator storage account
    create-vote-account                   Create a vote account
    deactivate-stake                      Deactivate the delegated stake from the stake account
    delegate-stake                        Delegate stake to a vote account
    deploy                                Deploy a program
    fees                                  Display current cluster fees
    get                                   Get wallet config settings
    get-slot                              Get current slot
    get-transaction-count                 Get current transaction count
    help                                  Prints this message or the help of the given subcommand(s)
    pay                                   Send a payment
    ping                                  Submit transactions sequentially
    redeem-vote-credits                   Redeem credits in the stake account
    send-signature                        Send a signature to authorize a transfer
    send-timestamp                        Send a timestamp to unlock a transfer
    set                                   Set a wallet config setting
    show-account                          Show the contents of an account
    show-stake-account                    Show the contents of a stake account
    show-storage-account                  Show the contents of a storage account
    show-vote-account                     Show the contents of a vote account
    validator-info                        Publish/get Validator info on Solana
    withdraw-stake                        Withdraw the unstaked lamports from the stake account
```

```manpage
solana-address
Get your public key

USAGE:
    solana address [OPTIONS]

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/wallet/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json
```

```manpage
solana-airdrop
Request a batch of lamports

USAGE:
    solana airdrop [OPTIONS] <AMOUNT> [unit]

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>        Configuration file to use [default: /Users/tyeraeulberg/.config/solana/wallet/config.yml]
        --drone-host <HOST>    Drone host to use [default: the --url host]
        --drone-port <PORT>    Drone port to use [default: 9900]
    -u, --url <URL>            JSON RPC URL for the solana cluster
    -k, --keypair <PATH>       /path/to/id.json

ARGS:
    <AMOUNT>    The airdrop amount to request (default unit SOL)
    <unit>      Specify unit to use for request and balance display [possible values: SOL, lamports]

```

```manpage
solana-authorize-voter
Authorize a new vote signing keypair for the given vote account

USAGE:
    solana authorize-voter [OPTIONS] <VOTE ACCOUNT PUBKEY> <CURRENT VOTER KEYPAIR FILE> <NEW VOTER PUBKEY>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/wallet/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json

ARGS:
    <VOTE ACCOUNT PUBKEY>           Vote account in which to set the authorized voter
    <CURRENT VOTER KEYPAIR FILE>    Keypair file for the currently authorized vote signer
    <NEW VOTER PUBKEY>              New vote signer to authorize

```

```manpage
solana-balance
Get your balance

USAGE:
    solana balance [FLAGS] [OPTIONS] [PUBKEY]

FLAGS:
    -h, --help        Prints help information
        --lamports    Display balance in lamports instead of SOL
    -V, --version     Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/wallet/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json

ARGS:
    <PUBKEY>    The public key of the balance to check
```

```manpage
solana-cancel
Cancel a transfer

USAGE:
    solana cancel [OPTIONS] <PROCESS ID>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/wallet/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json

ARGS:
    <PROCESS ID>    The process id of the transfer to cancel
```

```manpage
solana-claim-storage-reward
Redeem storage reward credits

USAGE:
    solana claim-storage-reward [OPTIONS] <NODE PUBKEY> <STORAGE ACCOUNT PUBKEY>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/wallet/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json

ARGS:
    <NODE PUBKEY>               The node account to credit the rewards to
    <STORAGE ACCOUNT PUBKEY>    Storage account address to redeem credits for
```

```manpage
solana-cluster-version
Get the version of the cluster entrypoint

USAGE:
    solana cluster-version [OPTIONS]

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/wallet/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json
```

```manpage
solana-confirm
Confirm transaction by signature

USAGE:
    solana confirm [OPTIONS] <SIGNATURE>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/wallet/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json

ARGS:
    <SIGNATURE>    The transaction signature to confirm
```

```manpage
solana-create-replicator-storage-account
Create a replicator storage account

USAGE:
    solana create-replicator-storage-account [OPTIONS] <STORAGE ACCOUNT OWNER PUBKEY> <STORAGE ACCOUNT PUBKEY>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/wallet/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json

ARGS:
    <STORAGE ACCOUNT OWNER PUBKEY>
    <STORAGE ACCOUNT PUBKEY>
```

```manpage
solana-create-storage-mining-pool-account
Create mining pool account

USAGE:
    solana create-storage-mining-pool-account [OPTIONS] <STORAGE ACCOUNT PUBKEY> <AMOUNT> [unit]

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: /Users/tyeraeulberg/.config/solana/wallet/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json

ARGS:
    <STORAGE ACCOUNT PUBKEY>    Storage mining pool account address to fund
    <AMOUNT>                    The amount to assign to the storage mining pool account (default unit SOL)
    <unit>                      Specify unit to use for request [possible values: SOL, lamports]
```

```manpage
solana-create-validator-storage-account
Create a validator storage account

USAGE:
    solana create-validator-storage-account [OPTIONS] <STORAGE ACCOUNT OWNER PUBKEY> <STORAGE ACCOUNT PUBKEY>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/wallet/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json

ARGS:
    <STORAGE ACCOUNT OWNER PUBKEY>
    <STORAGE ACCOUNT PUBKEY>
```

```manpage
solana-create-vote-account
Create a vote account

USAGE:
    solana create-vote-account [OPTIONS] <VOTE ACCOUNT PUBKEY> <VALIDATOR PUBKEY> <LAMPORTS>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
        --commission <NUM>    The commission taken on reward redemption (0-255), default: 0
    -C, --config <PATH>       Configuration file to use [default: ~/.config/solana/wallet/config.yml]
    -u, --url <URL>           JSON RPC URL for the solana cluster
    -k, --keypair <PATH>      /path/to/id.json

ARGS:
    <VOTE ACCOUNT PUBKEY>    Vote account address to fund
    <VALIDATOR PUBKEY>       Validator that will vote with this account
    <LAMPORTS>               The amount of lamports to send to the vote account
```

```manpage
solana-deactivate-stake
Deactivate the delegated stake from the stake account

USAGE:
    solana deactivate-stake [OPTIONS] <STAKE ACCOUNT KEYPAIR FILE> <PUBKEY>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/wallet/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json

ARGS:
    <STAKE ACCOUNT KEYPAIR FILE>    Keypair file for the stake account, for signing the delegate transaction.
    <PUBKEY>                        The vote account to which the stake is currently delegated
```

```manpage
solana-delegate-stake
Delegate stake to a vote account

USAGE:
    solana delegate-stake [OPTIONS] <STAKE ACCOUNT KEYPAIR FILE> <VOTE ACCOUNT PUBKEY> <AMOUNT> [unit]

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: /Users/tyeraeulberg/.config/solana/wallet/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json

ARGS:
    <STAKE ACCOUNT KEYPAIR FILE>    Keypair file for the new stake account
    <VOTE ACCOUNT PUBKEY>           The vote account to which the stake will be delegated
    <AMOUNT>                        The amount to delegate (default unit SOL)
    <unit>                          Specify unit to use for request [possible values: SOL, lamports]
```

```manpage
solana-deploy
Deploy a program

USAGE:
    solana deploy [OPTIONS] <PATH TO PROGRAM>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/wallet/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json

ARGS:
    <PATH TO PROGRAM>    /path/to/program.o
```

```manpage
solana-fees
Display current cluster fees

USAGE:
    solana fees [OPTIONS]

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/wallet/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json
```

```manpage
solana-get
Get wallet config settings

USAGE:
    solana get [OPTIONS] [CONFIG_FIELD]

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/wallet/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json

ARGS:
    <CONFIG_FIELD>    Return a specific config setting [possible values: url, keypair]
```

```manpage
solana-get-slot
Get current slot

USAGE:
    solana get-slot [OPTIONS]

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/wallet/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json
```

```manpage
solana-get-transaction-count
Get current transaction count

USAGE:
    solana get-transaction-count [OPTIONS]

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/wallet/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json
```

```manpage
solana-pay
Send a payment

USAGE:
    solana pay [FLAGS] [OPTIONS] <PUBKEY> <AMOUNT> [--] [unit]

FLAGS:
        --cancelable
    -h, --help          Prints help information
    -V, --version       Prints version information

OPTIONS:
    -C, --config <PATH>                         Configuration file to use [default:
                                                /Users/tyeraeulberg/.config/solana/wallet/config.yml]
    -u, --url <URL>                             JSON RPC URL for the solana cluster
    -k, --keypair <PATH>                        /path/to/id.json
        --after <DATETIME>                      A timestamp after which transaction will execute
        --require-timestamp-from <PUBKEY>       Require timestamp from this third party
        --require-signature-from <PUBKEY>...    Any third party signatures required to unlock the lamports

ARGS:
    <PUBKEY>    The pubkey of recipient
    <AMOUNT>    The amount to send (default unit SOL)
    <unit>      Specify unit to use for request [possible values: SOL, lamports]
```

```manpage
solana-ping
Submit transactions sequentially

USAGE:
    solana ping [OPTIONS]

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>         Configuration file to use [default:
                                ~/.config/solana/wallet/config.yml]
    -c, --count <NUMBER>        Stop after submitting count transactions
    -i, --interval <SECONDS>    Wait interval seconds between submitting the next transaction [default: 2]
    -u, --url <URL>             JSON RPC URL for the solana cluster
    -k, --keypair <PATH>        /path/to/id.json
    -t, --timeout <SECONDS>     Wait up to timeout seconds for transaction confirmation [default: 10]
```

```manpage
solana-redeem-vote-credits
Redeem credits in the stake account

USAGE:
    solana redeem-vote-credits [OPTIONS] <STAKING ACCOUNT PUBKEY> <VOTE ACCOUNT PUBKEY>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/wallet/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json

ARGS:
    <STAKING ACCOUNT PUBKEY>    Staking account address to redeem credits for
    <VOTE ACCOUNT PUBKEY>       The vote account to which the stake was previously delegated.
```

```manpage
solana-send-signature
Send a signature to authorize a transfer

USAGE:
    solana send-signature [OPTIONS] <PUBKEY> <PROCESS ID>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/wallet/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json

ARGS:
    <PUBKEY>        The pubkey of recipient
    <PROCESS ID>    The process id of the transfer to authorize
```

```manpage
solana-send-timestamp
Send a timestamp to unlock a transfer

USAGE:
    solana send-timestamp [OPTIONS] <PUBKEY> <PROCESS ID>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>      Configuration file to use [default: ~/.config/solana/wallet/config.yml]
        --date <DATETIME>    Optional arbitrary timestamp to apply
    -u, --url <URL>          JSON RPC URL for the solana cluster
    -k, --keypair <PATH>     /path/to/id.json

ARGS:
    <PUBKEY>        The pubkey of recipient
    <PROCESS ID>    The process id of the transfer to unlock
```

```manpage
solana-set
Set a wallet config setting

USAGE:
    solana set [OPTIONS] <--url <URL>|--keypair <PATH>>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/wallet/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json
```

```manpage
solana-show-account
Show the contents of an account

USAGE:
    solana show-account [FLAGS] [OPTIONS] <ACCOUNT PUBKEY>

FLAGS:
    -h, --help        Prints help information
        --lamports    Display balance in lamports instead of SOL
    -V, --version     Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/wallet/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json
    -o, --output <FILE>     Write the account data to this file

ARGS:
    <ACCOUNT PUBKEY>    Account pubkey
```

```manpage
solana-show-stake-account
Show the contents of a stake account

USAGE:
    solana show-stake-account [OPTIONS] <STAKE ACCOUNT PUBKEY>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/wallet/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json

ARGS:
    <STAKE ACCOUNT PUBKEY>    Stake account pubkey
```

```manpage
solana-show-storage-account
Show the contents of a storage account

USAGE:
    solana show-storage-account [OPTIONS] <STORAGE ACCOUNT PUBKEY>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/wallet/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json

ARGS:
    <STORAGE ACCOUNT PUBKEY>    Storage account pubkey
```

```manpage
solana-show-vote-account
Show the contents of a vote account

USAGE:
    solana show-vote-account [OPTIONS] <VOTE ACCOUNT PUBKEY>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/wallet/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json

ARGS:
    <VOTE ACCOUNT PUBKEY>    Vote account pubkey
```

```manpage
solana-validator-info
Publish/get Validator info on Solana

USAGE:
    solana validator-info [OPTIONS] [SUBCOMMAND]

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/wallet/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json

SUBCOMMANDS:
    get        Get and parse Solana Validator info
    help       Prints this message or the help of the given subcommand(s)
    publish    Publish Validator info on Solana
```

```manpage
solana-withdraw-stake
Withdraw the unstaked lamports from the stake account

USAGE:
    solana withdraw-stake [OPTIONS] <STAKE ACCOUNT KEYPAIR FILE> <DESTINATION PUBKEY> <AMOUNT> [unit]

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: /Users/tyeraeulberg/.config/solana/wallet/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json

ARGS:
    <STAKE ACCOUNT KEYPAIR FILE>    Keypair file for the stake account, for signing the withdraw transaction.
    <DESTINATION PUBKEY>            The account where the lamports should be transfered
    <AMOUNT>                        The amount to withdraw from the stake account (default unit SOL)
    <unit>                          Specify unit to use for request [possible values: SOL, lamports]
```
